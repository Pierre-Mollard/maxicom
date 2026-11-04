//! The link to the device: where the bytes come from and go to. Every kind reads and writes
//! the same way (nothing waiting reads 0, a lost link is `LinkError::InputClosed`), so the
//! loop, the terminal, capture and sessions work on any of them. The address says the kind:
//!
//! * serial port, `serial`: `/dev/ttyUSB0`, `COM3`. Has line settings (baud, framing)
//! * raw TCP, `tcp` (MAX-156): `tcp://host:port`. A bridge (ESP32 firmware, ser2net, socat,
//!   a serial device server, QEMU) owns the line settings
//! * replay, `replay`: `file://path`, a raw capture played back. Never written, no session
//!
//! The address is an `Address`, read once from its text (MAX-166). What each kind can do is
//! its `LinkKind`'s, so a check asks the kind, never the address's text.
//!
//! Planned: telnet `telnet://host:port` (MAX-161, no line settings), RFC 2217
//! `rfc2217://host:port` (line settings set remotely), Unix sockets and Windows named pipes
pub mod address;
pub mod config;
pub mod error;
pub mod lock;
pub mod probe;
pub mod replay;
pub mod serial;
pub mod state;
pub mod tcp;

use std::{
    io,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::Duration,
};

pub use address::Address;
pub use config::LinkConfig;
pub use error::LinkError;
use replay::Replay;
use serial::{LineControl, SerialDevice};
pub use state::{LinkState, OpenFor, Reconfigured};
use tcp::TcpDevice;

/// A TCP connect gives up after this. Off the loop, except at start before the UI
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

pub enum ByteSource {
    Serial(SerialDevice),
    Tcp(TcpDevice),
    Replay(Replay),
}

impl ByteSource {
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, LinkError> {
        match self {
            ByteSource::Serial(dev) => dev.read(buf),
            ByteSource::Tcp(dev) => dev.read(buf),
            ByteSource::Replay(replay) => replay.read(buf),
        }
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<(), LinkError> {
        match self {
            ByteSource::Serial(dev) => dev.write(bytes),
            ByteSource::Tcp(dev) => dev.write(bytes),
            ByteSource::Replay(_) => Ok(()),
        }
    }

    /// Sets a control line (MAX-12), a serial port's only
    pub fn control(&mut self, control: LineControl) -> Result<(), LinkError> {
        match self {
            ByteSource::Serial(dev) => dev.control(control),
            ByteSource::Tcp(_) | ByteSource::Replay(_) => Err(LinkError::NoControlLines),
        }
    }

    /// Another maxicom on the same TCP address when it connected (MAX-186): a serial port
    /// another maxicom holds is refused
    pub fn shared_with(&self) -> Option<&str> {
        match self {
            ByteSource::Tcp(dev) => dev.shared_with(),
            ByteSource::Serial(_) | ByteSource::Replay(_) => None,
        }
    }

    /// Ends the break `control` started; nothing for a link without control lines
    pub fn end_break(&mut self) -> Result<(), LinkError> {
        match self {
            ByteSource::Serial(dev) => dev.end_break(),
            ByteSource::Tcp(_) | ByteSource::Replay(_) => Ok(()),
        }
    }
}

/// The kind of link an address names, and what it can do (MAX-166)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Serial,
    Tcp,
    Replay,
}

impl LinkKind {
    /// Whether bytes go out: what is typed, sent, mashed, a trigger's line and the replies to
    /// the remote's queries, counted and recorded as TX. A link not written has no line mode
    pub fn writes(self) -> bool {
        self != LinkKind::Replay
    }

    /// Whether the link has a session backup: its recording, its pruning and the restore of the
    /// last session. A replay is a file already
    pub fn keeps_sessions(self) -> bool {
        self != LinkKind::Replay
    }

    /// Whether the link can be closed and opened again: it reconnects when lost, detaches by
    /// hand or when idle, and attaches, with the line settings changed meanwhile
    pub fn reopens(self) -> bool {
        self != LinkKind::Replay
    }

    /// Whether maxicom sets the link's line settings (baud, framing, flow control): a serial
    /// port does; a raw TCP link doesn't, its bridge owns them, nor does a replayed file. Later
    /// RFC 2217 will, telnet won't
    pub fn has_line_settings(self) -> bool {
        self == LinkKind::Serial
    }

    /// What a link without line settings shows in place of its baud and framing, read together
    /// as "raw TCP": in the status bar and the session browser. None for a replay, whose stand-in
    /// is its state, which the status bar writes
    pub fn line_stand_in(self) -> Option<[&'static str; 2]> {
        // TODO: (MAX-161) "telnet TCP" for telnet://, and the later kinds without line settings
        (self == LinkKind::Tcp).then_some(["raw", "TCP"])
    }

    /// What the popups, notices and menu call the link at the start of a label: "Serial port
    /// detached" (MAX-168)
    pub fn name(self) -> &'static str {
        match self {
            LinkKind::Serial => "Serial port",
            LinkKind::Tcp => "TCP connection",
            LinkKind::Replay => "Replay",
        }
    }
}

/// Opens the link `config.address` names, waiting for it: a serial port, or a TCP
/// connection
pub fn open(config: &LinkConfig) -> Result<ByteSource, LinkError> {
    match &config.address {
        Address::Serial(device) => SerialDevice::open(device, &config.line).map(ByteSource::Serial),
        Address::Tcp(addr) => TcpDevice::connect(addr, CONNECT_TIMEOUT)
            .map(ByteSource::Tcp)
            .map_err(|source| LinkError::ConnectFailed {
                addr: addr.clone(),
                source,
            }),
        // the loop opens a replay's file itself: a link never reopens as one
        Address::Replay(_) => Err(LinkError::OpenFailed {
            port: config.address.to_string(),
            baudrate: config.line.baud,
            source: io::Error::new(
                io::ErrorKind::Unsupported,
                "a replay isn't opened as a link",
            ),
        }),
    }
}

/// Opens links for the loop, which never waits on one: a TCP connect runs in a thread, as an
/// unreachable host takes the whole timeout; a serial port opens at once. One open at a time
#[derive(Default)]
pub struct Opener {
    pending: Option<Receiver<Result<ByteSource, LinkError>>>,
}

impl Opener {
    /// Starts opening `config`'s link, unless an open is still running. Its result comes from
    /// `finished`, right away for a serial port
    pub fn start(&mut self, config: &LinkConfig) {
        if self.pending.is_some() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        if let Address::Tcp(_) = config.address {
            let config = config.clone();
            thread::spawn(move || {
                let _ = sender.send(open(&config));
            });
        } else {
            let _ = sender.send(open(config));
        }
        self.pending = Some(receiver);
    }

    /// Starts opening `config`'s link in place of the open running, whose result is dropped:
    /// `open` switching while the last port retries (MAX-12)
    pub fn restart(&mut self, config: &LinkConfig) {
        self.pending = None;
        self.start(config);
    }

    /// The result of the open started last, once it is done
    pub fn finished(&mut self) -> Option<Result<ByteSource, LinkError>> {
        let received = self.pending.as_ref()?.try_recv();
        match received {
            Ok(result) => {
                self.pending = None;
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            // the thread died without an answer: the next start tries again
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                None
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/link.rs"]
mod tests;
