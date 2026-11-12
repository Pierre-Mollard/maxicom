pub mod download;
pub mod modem;
pub mod modem_rx;
pub mod modem_tx;
pub mod raw;

use std::{
    fmt,
    io::{self, BufRead, Read},
    ops::Range,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use download::Download;
use modem::Protocol;
use modem_rx::{ModemReceiver, ReceiveFailure};
use modem_tx::{ModemSender, SendFailure};
use raw::RawSender;

/// What a transfer `step` gives
#[derive(Debug, PartialEq, Eq)]
pub struct Step {
    /// The bytes to write
    pub out: Vec<u8>,
    /// The bytes of `rx` that are the protocol's only, from the other side's first ask or
    /// packet to the answer that ended it: before and after, they are the device's output, to
    /// show. Empty for a raw send
    pub protocol: Range<usize>,
}

/// Why a protocol transfer ended before the file got through
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferFailure {
    Send(SendFailure),
    Receive(ReceiveFailure),
}

impl fmt::Display for TransferFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Send(failure) => failure.fmt(f),
            Self::Receive(failure) => failure.fmt(f),
        }
    }
}

/// A file going through the link, a step per loop turn: sent as is, paced to the baud rate
/// (`send-file`, MAX-12), sent with a protocol the receiver drives (`upload`, MAX-162), or
/// received with one (`download`), the device sending. All share the progress the UI shows:
/// bytes, percent, rate, time left
pub struct FileTransfer {
    /// The file sent; a download's are in its `Download`
    name: String,
    size: u64,
    /// When the bytes began to flow, where the rate counts from: at once for a raw send, at
    /// the other side's first ask or packet for a protocol (not while it waits for `loady`)
    flowing_since: Option<Instant>,
    mode: Mode,
}

enum Mode {
    Raw {
        sender: RawSender,
        done: bool,
    },
    Modem(ModemSender),
    Receive {
        protocol: Protocol,
        receiver: ModemReceiver,
        // boxed: the biggest mode by far
        download: Box<Download>,
    },
}

impl fmt::Debug for FileTransfer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileTransfer")
            .field("name", &self.name())
            .field("size", &self.size())
            .field("sent", &self.sent())
            .finish_non_exhaustive()
    }
}

impl FileTransfer {
    pub fn raw(
        reader: Box<dyn BufRead + Send>,
        name: String,
        size: u64,
        bytes_per_sec: f64,
        line_delay: Duration,
        now: Instant,
    ) -> Self {
        Self {
            name,
            size,
            flowing_since: Some(now),
            mode: Mode::Raw {
                sender: RawSender::new(reader, bytes_per_sec, line_delay, now),
                done: false,
            },
        }
    }

    pub fn modem(
        reader: Box<dyn Read + Send>,
        protocol: Protocol,
        name: String,
        size: u64,
        bytes_per_sec: f64,
        now: Instant,
    ) -> Self {
        let sender = ModemSender::new(reader, protocol, name.clone(), size, bytes_per_sec, now);
        Self {
            name,
            size,
            flowing_since: None,
            mode: Mode::Modem(sender),
        }
    }

    /// Files received into `folder`: YMODEM's with their own names, XMODEM's as
    /// `xmodem_name`, as it sends none
    pub fn download(
        protocol: Protocol,
        folder: PathBuf,
        xmodem_name: Option<String>,
        now: Instant,
    ) -> Self {
        Self {
            name: String::new(),
            size: 0,
            flowing_since: None,
            mode: Mode::Receive {
                protocol,
                receiver: ModemReceiver::new(protocol, now),
                download: Box::new(Download::new(folder, xmodem_name)),
            },
        }
    }

    /// The file's modification time (seconds since 1970) for a protocol that sends it
    /// (YMODEM's header)
    pub fn with_mtime(mut self, mtime: u64) -> Self {
        self.mode = match self.mode {
            Mode::Modem(sender) => Mode::Modem(sender.with_mtime(mtime)),
            mode => mode,
        };
        self
    }

    /// The file sent, or the file being received; empty for a YMODEM download before its
    /// first file
    pub fn name(&self) -> String {
        match &self.mode {
            Mode::Receive { download, .. } => download.name().unwrap_or_default(),
            _ => self.name.clone(),
        }
    }

    /// The file's size; a download knows it only from YMODEM's header
    pub fn size(&self) -> Option<u64> {
        match &self.mode {
            Mode::Receive { receiver, .. } => receiver.received().1,
            _ => Some(self.size),
        }
    }

    pub fn protocol(&self) -> Option<Protocol> {
        match &self.mode {
            Mode::Raw { .. } => None,
            Mode::Modem(sender) => Some(sender.protocol()),
            Mode::Receive { protocol, .. } => Some(*protocol),
        }
    }

    /// Whether the file comes from the device (`download`)
    pub fn is_download(&self) -> bool {
        matches!(self.mode, Mode::Receive { .. })
    }

    /// Where a download's files go
    pub fn folder(&self) -> Option<&Path> {
        match &self.mode {
            Mode::Receive { download, .. } => Some(download.folder()),
            _ => None,
        }
    }

    /// The `.part` a download keeps when it stops in a file
    pub fn partial(&self) -> Option<&Path> {
        match &self.mode {
            Mode::Receive { download, .. } => download.partial(),
            _ => None,
        }
    }

    /// The files a download completed since the last call, with their sizes
    pub fn take_received(&mut self) -> Vec<(PathBuf, u64)> {
        match &mut self.mode {
            Mode::Receive { download, .. } => download.take_received(),
            _ => Vec::new(),
        }
    }

    /// How many files a download completed
    pub fn received_count(&self) -> usize {
        match &self.mode {
            Mode::Receive { download, .. } => download.count(),
            _ => 0,
        }
    }

    /// The bytes of the files a download completed
    pub fn received_total(&self) -> u64 {
        match &self.mode {
            Mode::Receive { download, .. } => download.total(),
            _ => 0,
        }
    }

    /// The last file a download completed, with its size
    pub fn last_received(&self) -> Option<&(PathBuf, u64)> {
        match &self.mode {
            Mode::Receive { download, .. } => download.last(),
            _ => None,
        }
    }

    /// Whether the link's RX goes through `step`: a protocol reads the other side's bytes in
    /// it, a raw send ignores it
    pub fn takes_rx(&self) -> bool {
        !matches!(self.mode, Mode::Raw { .. })
    }

    /// Whether nothing else may write to the link: a protocol transfer the other side
    /// started, until it ends, as any stray byte breaks a packet (MAX-162). A raw send leaves
    /// it open
    pub fn owns_link(&self) -> bool {
        self.protocol_started() && self.outcome().is_none()
    }

    /// Whether a protocol transfer still waits for the other side: the receiver's first ask,
    /// or the sender's first packet
    pub fn waiting(&self) -> bool {
        self.takes_rx() && !self.protocol_started() && self.outcome().is_none()
    }

    fn protocol_started(&self) -> bool {
        match &self.mode {
            Mode::Raw { .. } => false,
            Mode::Modem(sender) => sender.started(),
            Mode::Receive { receiver, .. } => receiver.started(),
        }
    }

    /// The packets sent or asked again so far, for a protocol
    pub fn retries(&self) -> Option<u32> {
        match &self.mode {
            Mode::Raw { .. } => None,
            Mode::Modem(sender) => Some(sender.retries()),
            Mode::Receive { receiver, .. } => Some(receiver.retries()),
        }
    }

    /// The file bytes through: sent (with a protocol, those the receiver acknowledged), or
    /// received for the current file of a download
    pub fn sent(&self) -> u64 {
        match &self.mode {
            Mode::Raw { sender, .. } => sender.sent(),
            Mode::Modem(sender) => sender.sent(),
            Mode::Receive { receiver, .. } => receiver.received().0,
        }
    }

    /// How much is through, in percent; 100 for an empty file, 0 when the size isn't known
    pub fn percent(&self) -> u64 {
        match self.size() {
            Some(size) => (self.sent() * 100).checked_div(size).unwrap_or(100),
            None => 0,
        }
    }

    /// Bytes a second since the bytes began to flow; none before
    pub fn rate(&self, now: Instant) -> Option<f64> {
        let secs = now
            .saturating_duration_since(self.flowing_since?)
            .as_secs_f64();
        (secs > 0.0).then(|| self.sent() as f64 / secs)
    }

    /// The time left at the rate so far; none until bytes went through, or without a size
    pub fn eta(&self, now: Instant) -> Option<Duration> {
        let rate = self.rate(now).filter(|&rate| rate > 0.0)?;
        let left = self.size()?.saturating_sub(self.sent());
        Some(Duration::from_secs_f64(left as f64 / rate))
    }

    /// None while it runs; a raw send ends at the end of the file, a protocol when the other
    /// side has it all or something went wrong
    pub fn outcome(&self) -> Option<Result<(), TransferFailure>> {
        match &self.mode {
            Mode::Raw { done, .. } => done.then_some(Ok(())),
            Mode::Modem(sender) => Some(sender.outcome()?.map_err(TransferFailure::Send)),
            Mode::Receive { receiver, .. } => {
                Some(receiver.outcome()?.map_err(TransferFailure::Receive))
            }
        }
    }

    /// A loop turn: takes `rx`, what the link received (a raw send ignores it), and gives the
    /// bytes to write now. A download writes what came in to its files. A read or write
    /// error leaves it running: `cancel` it
    pub fn step(&mut self, rx: &[u8], now: Instant) -> io::Result<Step> {
        let step = match &mut self.mode {
            Mode::Raw { sender, done } => {
                let out = sender.next_chunk(now)?.unwrap_or_else(|| {
                    *done = true;
                    Vec::new()
                });
                Step {
                    out,
                    protocol: rx.len()..rx.len(),
                }
            }
            Mode::Modem(sender) => sender.step(rx, now)?,
            Mode::Receive {
                receiver, download, ..
            } => {
                let step = receiver.step(rx, now);
                for event in step.events {
                    download.apply(event)?;
                }
                Step {
                    out: step.out,
                    protocol: step.protocol,
                }
            }
        };
        if self.protocol_started() {
            self.flowing_since.get_or_insert(now);
        }
        Ok(step)
    }

    /// Stops it: the bytes that tell the other side (a protocol's CANs), none for a raw send
    pub fn cancel(&mut self) -> Vec<u8> {
        match &mut self.mode {
            Mode::Raw { .. } => Vec::new(),
            Mode::Modem(sender) => sender.cancel(),
            Mode::Receive { receiver, .. } => receiver.cancel(),
        }
    }
}

#[cfg(test)]
#[path = "tests/transfer.rs"]
mod tests;
