//! The link to the device: where the bytes come from and go to. Every kind reads and writes
//! the same way (nothing waiting reads 0, a lost link is `SerialError::InputClosed`), so the
//! loop, the terminal, capture and sessions work on any of them. The address says the kind:
//!
//! * serial port, `serial`: `/dev/ttyUSB0`, `COM3`. Has line settings (baud, framing)
//! * raw TCP, `tcp` (MAX-156): `tcp://host:port`. A bridge (ESP32 firmware, ser2net, socat,
//!   a serial device server, QEMU) owns the line settings
//! * replay, `replay`: `file://path`, a raw capture played back. Never written, no session
//!
//! Planned: telnet `telnet://host:port` (MAX-161, no line settings), RFC 2217
//! `rfc2217://host:port` (line settings set remotely), Unix sockets and Windows named pipes
pub mod replay;
pub mod serial;
pub mod tcp;

use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::Duration,
};

use replay::Replay;
use serial::{LineControl, SerialConfig, SerialDevice, SerialError};
use tcp::TcpDevice;

/// A TCP connect gives up after this. Off the loop, except at start before the UI
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

pub enum ByteSource {
    Live(SerialDevice),
    Tcp(TcpDevice),
    Replay(Replay),
}

impl ByteSource {
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, SerialError> {
        match self {
            ByteSource::Live(dev) => dev.read(buf),
            ByteSource::Tcp(dev) => dev.read(buf),
            ByteSource::Replay(replay) => replay.read(buf),
        }
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<(), SerialError> {
        match self {
            ByteSource::Live(dev) => dev.write(bytes),
            ByteSource::Tcp(dev) => dev.write(bytes),
            ByteSource::Replay(_) => Ok(()),
        }
    }

    /// Sets a control line (MAX-12), a serial port's only
    pub fn control(&mut self, control: LineControl) -> Result<(), SerialError> {
        match self {
            ByteSource::Live(dev) => dev.control(control),
            ByteSource::Tcp(_) | ByteSource::Replay(_) => Err(SerialError::NoControlLines),
        }
    }

    /// Ends the break `control` started; nothing for a link without control lines
    pub fn end_break(&mut self) -> Result<(), SerialError> {
        match self {
            ByteSource::Live(dev) => dev.end_break(),
            ByteSource::Tcp(_) | ByteSource::Replay(_) => Ok(()),
        }
    }
}

/// The `host:port` of a raw TCP address `tcp://host:port`
pub fn tcp_address(port: &str) -> Option<&str> {
    port.strip_prefix("tcp://")
}

/// Whether `port` is a replayed file (`file://`): no link, nothing goes out
pub fn is_replay(port: &str) -> bool {
    port.starts_with("file://")
}

/// Whether maxicom sets the link's line settings (baud, framing, flow control): a serial port
/// does, a raw TCP link doesn't, its bridge owns them. Later RFC 2217 will, telnet won't
pub fn has_line_settings(port: &str) -> bool {
    tcp_address(port).is_none()
}

/// What a link without line settings shows in place of its baud and framing, read together
/// as "raw TCP": in the status bar and the session browser
pub fn line_stand_in(port: &str) -> Option<[&'static str; 2]> {
    // TODO: (MAX-161) "telnet TCP" for telnet://, and the later kinds without line settings
    tcp_address(port).map(|_| ["raw", "TCP"])
}

/// Opens the link `config.port` names, waiting for it: a serial port, or a TCP connection
pub fn open(config: &SerialConfig) -> Result<ByteSource, SerialError> {
    match tcp_address(&config.port) {
        Some(addr) => TcpDevice::connect(addr, CONNECT_TIMEOUT)
            .map(ByteSource::Tcp)
            .map_err(|source| SerialError::ConnectFailed {
                addr: addr.to_string(),
                source,
            }),
        None => SerialDevice::open(config).map(ByteSource::Live),
    }
}

/// Opens links for the loop, which never waits on one: a TCP connect runs in a thread, as an
/// unreachable host takes the whole timeout; a serial port opens at once. One open at a time
#[derive(Default)]
pub struct Opener {
    pending: Option<Receiver<Result<ByteSource, SerialError>>>,
}

impl Opener {
    /// Starts opening `config`'s link, unless an open is still running. Its result comes from
    /// `finished`, right away for a serial port
    pub fn start(&mut self, config: &SerialConfig) {
        if self.pending.is_some() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        if tcp_address(&config.port).is_some() {
            let config = config.clone();
            thread::spawn(move || {
                let _ = sender.send(open(&config));
            });
        } else {
            let _ = sender.send(open(config));
        }
        self.pending = Some(receiver);
    }

    /// The result of the open started last, once it is done
    pub fn finished(&mut self) -> Option<Result<ByteSource, SerialError>> {
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
mod tests {
    //! Opening by address: the kind it picks, and the opener off the loop.
    use super::*;
    use std::net::TcpListener;
    use std::time::Instant;

    fn config(port: &str) -> SerialConfig {
        SerialConfig {
            port: port.to_string(),
            ..SerialConfig::default()
        }
    }

    /// Waits for the opener's result, as the loop polls it each frame
    fn wait(opener: &mut Opener) -> Result<ByteSource, SerialError> {
        let start = Instant::now();
        loop {
            if let Some(result) = opener.finished() {
                return result;
            }
            assert!(start.elapsed() < CONNECT_TIMEOUT * 2, "no result");
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Only `tcp://` names a TCP link; anything else is a serial port.
    /// How: addresses of each kind.
    #[test]
    fn address_kinds() {
        assert_eq!(
            tcp_address("tcp://192.168.1.50:4000"),
            Some("192.168.1.50:4000")
        );
        assert_eq!(tcp_address("tcp://localhost:23"), Some("localhost:23"));
        assert_eq!(tcp_address("/dev/ttyUSB0"), None);
        assert_eq!(tcp_address("COM3"), None);
        assert_eq!(tcp_address("TCP://host:1"), None);
        assert!(has_line_settings("/dev/ttyUSB0") && has_line_settings("COM3"));
        assert!(!has_line_settings("tcp://192.168.1.50:4000"));
        assert_eq!(
            line_stand_in("tcp://192.168.1.50:4000"),
            Some(["raw", "TCP"])
        );
        assert_eq!(line_stand_in("/dev/ttyUSB0"), None);
    }

    /// A TCP open runs off the loop and gives a TCP link; a second start while it runs is
    /// ignored, and the result comes once.
    /// How: a local listener, two starts, then polls until the result.
    #[test]
    fn opener_connects_tcp() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let config = config(&format!("tcp://{}", listener.local_addr().unwrap()));
        let mut opener = Opener::default();
        opener.start(&config);
        opener.start(&config);
        assert!(matches!(wait(&mut opener), Ok(ByteSource::Tcp(_))));
        assert!(opener.finished().is_none());
        assert!(listener.accept().is_ok());
    }

    /// A refused connect is a `ConnectFailed` naming the address, and the opener can start
    /// again after it.
    /// How: a port freed by dropping its listener, opened twice.
    #[test]
    fn opener_reports_a_refused_connect() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        drop(listener);
        let mut opener = Opener::default();
        for _ in 0..2 {
            opener.start(&config(&format!("tcp://{addr}")));
            match wait(&mut opener) {
                Err(SerialError::ConnectFailed { addr: failed, .. }) => assert_eq!(failed, addr),
                other => panic!("{:?}", other.err()),
            }
        }
    }

    /// A serial port opens in the loop: its result is there right after the start.
    /// How: a port that doesn't exist.
    #[test]
    fn opener_opens_serial_at_once() {
        let mut opener = Opener::default();
        opener.start(&config("/nonexistent/ttyMAXICOM"));
        assert!(matches!(
            opener.finished(),
            Some(Err(SerialError::OpenFailed { .. }))
        ));
    }
}
