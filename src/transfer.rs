// TODO: (MAX-162) remove the allow once the upload command (step 5) starts protocol transfers
#[allow(dead_code)]
pub mod modem;
pub mod raw;

use std::{
    io::{self, BufRead, Read},
    ops::Range,
    time::{Duration, Instant},
};

use modem::{Failure, ModemSender, Protocol};
use raw::RawSender;

/// What a transfer `step` gives
#[derive(Debug, PartialEq, Eq)]
pub struct Step {
    /// The bytes to write
    pub out: Vec<u8>,
    /// The bytes of `rx` that are the protocol's only, from the receiver's first NAK or C to
    /// the answer that ended it: before and after, they are the device's output, to show.
    /// Empty for a raw send
    pub protocol: Range<usize>,
}

/// A file being sent, a step per loop turn: as is, paced to the baud rate (`send-file`,
/// MAX-12), or with a protocol the receiver drives (`upload`, MAX-162). Both share the
/// progress the UI shows: bytes sent, percent, rate, time left
pub struct FileTransfer {
    name: String,
    size: u64,
    /// When the bytes began to flow, where the rate counts from: at once for a raw send, at
    /// the receiver's first NAK or C for a protocol (not while it waits for `loady`)
    flowing_since: Option<Instant>,
    mode: Mode,
}

enum Mode {
    Raw { sender: RawSender, done: bool },
    Modem(ModemSender),
}

impl std::fmt::Debug for FileTransfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileTransfer")
            .field("name", &self.name)
            .field("size", &self.size)
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

    // TODO: (MAX-162) remove the allow once the upload command (step 5) calls it
    #[allow(dead_code)]
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

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn protocol(&self) -> Option<Protocol> {
        match &self.mode {
            Mode::Raw { .. } => None,
            Mode::Modem(sender) => Some(sender.protocol()),
        }
    }

    /// Whether the link's RX goes through `step`: a protocol reads the receiver's answers in
    /// it, a raw send ignores it
    pub fn takes_rx(&self) -> bool {
        matches!(self.mode, Mode::Modem(_))
    }

    /// Whether nothing else may write to the link: a protocol transfer the receiver started,
    /// until it ends, as any stray byte breaks a packet (MAX-162). A raw send leaves it open
    pub fn owns_link(&self) -> bool {
        match &self.mode {
            Mode::Raw { .. } => false,
            Mode::Modem(sender) => sender.started() && sender.outcome().is_none(),
        }
    }

    /// Whether a protocol transfer still waits for the receiver's first NAK or C
    pub fn waiting(&self) -> bool {
        match &self.mode {
            Mode::Raw { .. } => false,
            Mode::Modem(sender) => !sender.started() && sender.outcome().is_none(),
        }
    }

    /// The packets sent again so far, for a protocol
    pub fn retries(&self) -> Option<u32> {
        match &self.mode {
            Mode::Raw { .. } => None,
            Mode::Modem(sender) => Some(sender.retries()),
        }
    }

    /// The file bytes sent; with a protocol, those the receiver acknowledged
    pub fn sent(&self) -> u64 {
        match &self.mode {
            Mode::Raw { sender, .. } => sender.sent(),
            Mode::Modem(sender) => sender.sent(),
        }
    }

    /// How much is sent, in percent; 100 for an empty file
    pub fn percent(&self) -> u64 {
        (self.sent() * 100).checked_div(self.size).unwrap_or(100)
    }

    /// Bytes a second since the bytes began to flow; none before
    pub fn rate(&self, now: Instant) -> Option<f64> {
        let secs = now
            .saturating_duration_since(self.flowing_since?)
            .as_secs_f64();
        (secs > 0.0).then(|| self.sent() as f64 / secs)
    }

    /// The time left at the rate so far; none until bytes went through
    pub fn eta(&self, now: Instant) -> Option<Duration> {
        let rate = self.rate(now).filter(|&rate| rate > 0.0)?;
        let left = self.size.saturating_sub(self.sent());
        Some(Duration::from_secs_f64(left as f64 / rate))
    }

    /// None while it runs; a raw send ends at the end of the file, a protocol when the
    /// receiver acknowledged it all or something went wrong
    pub fn outcome(&self) -> Option<Result<(), Failure>> {
        match &self.mode {
            Mode::Raw { done, .. } => done.then_some(Ok(())),
            Mode::Modem(sender) => sender.outcome(),
        }
    }

    /// A loop turn: takes `rx`, what the link received (a raw send ignores it), and gives the
    /// bytes to write now. A read error leaves it running: `cancel` it
    pub fn step(&mut self, rx: &[u8], now: Instant) -> io::Result<Step> {
        match &mut self.mode {
            Mode::Raw { sender, done } => {
                let out = sender.next_chunk(now)?.unwrap_or_else(|| {
                    *done = true;
                    Vec::new()
                });
                Ok(Step {
                    out,
                    protocol: rx.len()..rx.len(),
                })
            }
            Mode::Modem(sender) => {
                let step = sender.step(rx, now)?;
                if sender.started() {
                    self.flowing_since.get_or_insert(now);
                }
                Ok(step)
            }
        }
    }

    /// Stops it: the bytes that tell the receiver (a protocol's CANs), none for a raw send
    pub fn cancel(&mut self) -> Vec<u8> {
        match &mut self.mode {
            Mode::Raw { .. } => Vec::new(),
            Mode::Modem(sender) => sender.cancel(),
        }
    }
}

#[cfg(test)]
#[path = "tests/transfer.rs"]
mod tests;
