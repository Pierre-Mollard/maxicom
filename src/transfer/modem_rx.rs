//! Receiving a file with XMODEM or YMODEM (MAX-162): the sender's side (`modem_tx.rs`) seen
//! from the other end. The same packets (`modem.rs`), read instead of written.
//!
//! ## The receiver drives
//!
//! The receiver asks first and keeps asking until a packet comes: `C` for CRC packets, NAK for
//! the 1 byte sum. Then it answers every packet: ACK to get the next one, NAK to get the same
//! one again (damaged, or nothing came in time).
//!
//! ```text
//! receiver            sender
//!    C        ──▶                    repeated every 3 s until a packet comes
//!             ◀──  block 1
//!    ACK      ──▶
//!             ◀──  block 2           damaged on the way
//!    NAK      ──▶
//!             ◀──  block 2
//!    ACK      ──▶
//!             ◀──  EOT
//!    ACK      ──▶                    done (XMODEM)
//! ```
//!
//! A sender that only knows the sum never answers `C`: after 3 unanswered ones, XMODEM asks
//! with NAK instead.
//!
//! ## YMODEM
//!
//! Each file starts with block 0, its name, size and date; the receiver acknowledges it and
//! asks for the data with one more `C`. After the file's EOT (NAKed once, to be sure it isn't
//! a damaged byte), a `C` asks for the next file's block 0. An empty block 0 ends the batch:
//!
//! ```text
//!    C        ──▶
//!             ◀──  block 0: "a.bin\0" "300 15071006615\0"
//!    ACK C    ──▶
//!             ◀──  block 1
//!    ACK      ──▶
//!             ◀──  EOT
//!    NAK      ──▶
//!             ◀──  EOT
//!    ACK C    ──▶                    file a.bin done, next one?
//!             ◀──  block 0, all NUL  no more files
//!    ACK      ──▶                    done
//! ```
//!
//! With the size, the last block's padding is cut off; XMODEM has no size, so the file keeps
//! it (`0x1A` bytes), as with lrzsz's `rx`.
//!
//! ## Errors
//!
//! A block that comes twice (its ACK was lost) is acknowledged again and skipped. A block out
//! of order means the two sides lost each other: the receiver cancels. A damaged block or
//! nothing for 10 s gets a NAK, 10 times at most in a row.

use std::{
    fmt,
    ops::Range,
    time::{Duration, Instant},
};

use super::modem::{
    ACK, ACK_TIMEOUT, CAN, CANCEL, CRC, EOT, NAK, Protocol, RETRIES, SOH, START_TIMEOUT, STX, check,
};

/// How often the receiver asks (`C` or NAK) while no packet comes
const ASK_EVERY: Duration = Duration::from_secs(3);
/// `C` sent before XMODEM asks with NAK instead: an old sender only knows the sum
const CRC_ASKS: u32 = 3;
/// A packet whose bytes stop coming for this long is dropped and asked again
const CHAR_TIMEOUT: Duration = Duration::from_secs(1);
/// How long a YMODEM receiver asks for the data or the next file before giving up
const STALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Why a receive ended before the file was all there
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveFailure {
    /// No packet in `START_TIMEOUT`
    NoSender,
    /// The sender sent CAN CAN
    SenderCancelled,
    /// A packet was damaged or missing `RETRIES` times in a row
    TooManyRetries,
    /// A block came out of order: the two sides lost each other
    OutOfSync,
    /// YMODEM: no packet after asking for the data or the next file
    Stalled,
    /// `cancel` stopped it
    Cancelled,
}

impl fmt::Display for ReceiveFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoSender => "no packet from the sender: is it sending (sb, sx)?",
            Self::SenderCancelled => "the sender cancelled",
            Self::TooManyRetries => "too many retries",
            Self::OutOfSync => "a block came out of order",
            Self::Stalled => "the sender stopped answering",
            Self::Cancelled => "cancelled",
        })
    }
}

/// What came in, for the app to write
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A YMODEM file starts: its name (the last part of the sender's path, never a folder),
    /// its size and date (seconds since 1970) when the sender gave them
    File {
        name: String,
        size: Option<u64>,
        mtime: Option<u64>,
    },
    /// The next bytes of the file
    Data(Vec<u8>),
    /// The file is complete
    FileEnd,
}

/// What a receiver `step` gives
#[derive(Debug, PartialEq, Eq)]
pub struct ReceiveStep {
    /// The bytes to write: the asks and answers
    pub out: Vec<u8>,
    /// The bytes of `rx` that are the protocol's only, from the first packet to the one that
    /// ended it: before and after, they are the device's output, to show
    pub protocol: Range<usize>,
    pub events: Vec<Event>,
}

/// The packet the receiver waits for
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    /// YMODEM's block 0, the next file's name and size
    Header,
    /// That data block
    Block(u8),
}

/// What the receiver does between packets
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// No packet yet (or YMODEM between stages): asks every `ASK_EVERY` until `deadline`
    Asking {
        next_ask: Instant,
        asks: u32,
        deadline: Instant,
    },
    /// Waits for the next packet: NAK when none comes by `deadline`
    Waiting {
        deadline: Instant,
    },
    Done(Result<(), ReceiveFailure>),
}

/// The receiving side of XMODEM / YMODEM, without I/O, like `ModemSender`: `step` takes the
/// bytes the sender sent and the time, gives the bytes to write and what came in
#[derive(Debug)]
pub struct ModemReceiver {
    ymodem: bool,
    /// CRC-16 packets, else the 1 byte sum: what the receiver asked for
    crc: bool,
    expect: Expect,
    phase: Phase,
    /// The packet being read: its bytes so far, how many it has in all (0: none started)
    packet: Vec<u8>,
    packet_len: usize,
    /// When the packet's last byte came
    last_byte: Instant,
    /// The current file's size, from YMODEM's block 0
    size: Option<u64>,
    /// The current file's bytes received
    received: u64,
    /// The first EOT got its NAK (YMODEM): the next one ends the file
    eot_nakked: bool,
    /// Damaged or missing packets in a row
    errors: u32,
    /// NAKs sent, over the whole transfer
    retries: u32,
    /// CANs received in a row
    cans: u8,
    /// The first packet came: the link is the transfer's until it ends
    started: bool,
}

impl ModemReceiver {
    /// XMODEM and XMODEM-1K are the same to a receiver: it takes both packet sizes
    pub fn new(protocol: Protocol, now: Instant) -> Self {
        let ymodem = protocol == Protocol::Ymodem;
        Self {
            ymodem,
            crc: true,
            expect: if ymodem {
                Expect::Header
            } else {
                Expect::Block(1)
            },
            // the first ask goes at the first step
            phase: Phase::Asking {
                next_ask: now,
                asks: 0,
                deadline: now + START_TIMEOUT,
            },
            packet: Vec::new(),
            packet_len: 0,
            last_byte: now,
            size: None,
            received: 0,
            eot_nakked: false,
            errors: 0,
            retries: 0,
            cans: 0,
            started: false,
        }
    }

    /// The current file's bytes received, and its size when the sender gave it
    pub fn received(&self) -> (u64, Option<u64>) {
        (self.received, self.size)
    }

    pub fn retries(&self) -> u32 {
        self.retries
    }

    pub fn started(&self) -> bool {
        self.started
    }

    /// None while it runs
    pub fn outcome(&self) -> Option<Result<(), ReceiveFailure>> {
        match self.phase {
            Phase::Done(outcome) => Some(outcome),
            _ => None,
        }
    }

    /// Takes `rx`, what the sender sent since the last step, then checks the timeouts: the
    /// asks and answers to write, the protocol's part of `rx` and what came in
    pub fn step(&mut self, rx: &[u8], now: Instant) -> ReceiveStep {
        let mut out = Vec::new();
        let mut events = Vec::new();
        let mut start = (self.started && self.outcome().is_none()).then_some(0);
        let mut end = rx.len();
        for (i, &byte) in rx.iter().enumerate() {
            // Ended: what follows is the device talking again
            if self.outcome().is_some() {
                end = i;
                break;
            }
            self.byte(byte, now, &mut out, &mut events);
            if start.is_none() && self.started {
                start = Some(i);
            }
        }
        self.check_time(now, &mut out);
        let start = start.unwrap_or(rx.len());
        ReceiveStep {
            out,
            protocol: start..end.max(start),
            events,
        }
    }

    /// Stops the transfer: the CANs to write, none once it ended
    pub fn cancel(&mut self) -> Vec<u8> {
        if self.outcome().is_some() {
            return Vec::new();
        }
        self.phase = Phase::Done(Err(ReceiveFailure::Cancelled));
        CANCEL.to_vec()
    }

    /// One byte from the sender: the next byte of the packet being read, or between packets
    /// the start of one, EOT or CAN
    fn byte(&mut self, byte: u8, now: Instant, out: &mut Vec<u8>, events: &mut Vec<Event>) {
        if self.packet_len > 0 {
            // inside a packet any byte is data, CAN and EOT too
            self.packet.push(byte);
            self.last_byte = now;
            if self.packet.len() == self.packet_len {
                let packet = std::mem::take(&mut self.packet);
                self.packet_len = 0;
                self.read_packet(&packet, now, out, events);
            }
            return;
        }
        // One CAN can be noise, two in a row are the sender giving up
        if byte == CAN {
            self.cans += 1;
            if self.cans >= 2 {
                self.phase = Phase::Done(Err(ReceiveFailure::SenderCancelled));
            }
            return;
        }
        self.cans = 0;
        match byte {
            SOH | STX => {
                // the start byte, the block number and its complement, the data, the check
                let data_len = if byte == STX { 1024 } else { 128 };
                self.packet_len = 3 + data_len + if self.crc { 2 } else { 1 };
                self.packet.push(byte);
                self.last_byte = now;
                self.started = true;
            }
            EOT if self.started => self.end_of_file(now, out, events),
            // anything else is noise, or the device's text before the first packet (the
            // shell echoing `sb file`)
            _ => {}
        }
    }

    /// A whole packet: checked, then the block the receiver waits for, a copy of the last
    /// one, or one out of order
    fn read_packet(
        &mut self,
        packet: &[u8],
        now: Instant,
        out: &mut Vec<u8>,
        events: &mut Vec<Event>,
    ) {
        let check_len = if self.crc { 2 } else { 1 };
        let (head, rest) = packet.split_at(3);
        let (data, sum) = rest.split_at(rest.len() - check_len);
        let block = head[1];
        // a damaged block number, or damaged data: the same packet again
        if head[2] != !block || sum != check(data, self.crc) {
            self.reject(now, out);
            return;
        }
        match self.expect {
            Expect::Header if block == 0 => self.header(data, now, out, events),
            // the data block the receiver waits for
            Expect::Block(next) if block == next => {
                let mut data = data;
                // YMODEM knows the size: the last block's padding is cut off
                if let Some(size) = self.size {
                    let left = size.saturating_sub(self.received);
                    data = &data[..data.len().min(left as usize)];
                }
                self.received += data.len() as u64;
                if !data.is_empty() {
                    events.push(Event::Data(data.to_vec()));
                }
                out.push(ACK);
                self.expect = Expect::Block(next.wrapping_add(1));
                self.accepted(now);
            }
            // the block before: the sender didn't get its ACK, so it sent it again
            Expect::Block(next) if block == next.wrapping_sub(1) => {
                out.push(ACK);
                self.accepted(now);
            }
            // a block the receiver can't place: the two sides lost each other
            _ => {
                out.extend_from_slice(&CANCEL);
                self.phase = Phase::Done(Err(ReceiveFailure::OutOfSync));
            }
        }
    }

    /// YMODEM's block 0: the file's name, then its size in decimal and its date in octal,
    /// separated by a space; all NUL ends the batch
    fn header(&mut self, data: &[u8], now: Instant, out: &mut Vec<u8>, events: &mut Vec<Event>) {
        out.push(ACK);
        let name_end = data
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(data.len());
        if name_end == 0 {
            self.phase = Phase::Done(Ok(()));
            return;
        }
        let info = &data[(name_end + 1).min(data.len())..];
        let info_end = info
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(info.len());
        let info = String::from_utf8_lossy(&info[..info_end]);
        let mut fields = info.split_whitespace();
        let size = fields.next().and_then(|field| field.parse().ok());
        let mtime = fields
            .next()
            .and_then(|field| u64::from_str_radix(field, 8).ok());
        events.push(Event::File {
            name: file_name(&data[..name_end]),
            size,
            mtime,
        });
        self.size = size;
        self.received = 0;
        self.expect = Expect::Block(1);
        self.errors = 0;
        // one more `C` asks for the data
        self.ask(now, out);
    }

    /// EOT, the end of the file: XMODEM's ends the transfer; YMODEM's is NAKed once, then the
    /// receiver asks for the next file
    fn end_of_file(&mut self, now: Instant, out: &mut Vec<u8>, events: &mut Vec<Event>) {
        if self.expect == Expect::Header {
            return;
        }
        if self.ymodem && !self.eot_nakked {
            out.push(NAK);
            self.eot_nakked = true;
            self.phase = Phase::Waiting {
                deadline: now + ACK_TIMEOUT,
            };
            return;
        }
        out.push(ACK);
        events.push(Event::FileEnd);
        self.eot_nakked = false;
        if self.ymodem {
            self.expect = Expect::Header;
            self.size = None;
            self.ask(now, out);
        } else {
            self.phase = Phase::Done(Ok(()));
        }
    }

    /// A packet taken (or a copy acknowledged): wait for the next one
    fn accepted(&mut self, now: Instant) {
        self.errors = 0;
        self.eot_nakked = false;
        self.phase = Phase::Waiting {
            deadline: now + ACK_TIMEOUT,
        };
    }

    /// A damaged or missing packet: NAK to get it again, until `RETRIES` in a row
    fn reject(&mut self, now: Instant, out: &mut Vec<u8>) {
        self.errors += 1;
        self.retries += 1;
        if self.errors > RETRIES {
            out.extend_from_slice(&CANCEL);
            self.phase = Phase::Done(Err(ReceiveFailure::TooManyRetries));
            return;
        }
        out.push(NAK);
        self.phase = Phase::Waiting {
            deadline: now + ACK_TIMEOUT,
        };
    }

    /// YMODEM between stages: a `C` now, again every `ASK_EVERY`
    fn ask(&mut self, now: Instant, out: &mut Vec<u8>) {
        out.push(CRC);
        self.phase = Phase::Asking {
            next_ask: now + ASK_EVERY,
            asks: 1,
            deadline: now + STALL_TIMEOUT,
        };
    }

    /// The clock: a packet stopped halfway, an ask to repeat, a packet that didn't come
    fn check_time(&mut self, now: Instant, out: &mut Vec<u8>) {
        if self.packet_len > 0 {
            if now.saturating_duration_since(self.last_byte) >= CHAR_TIMEOUT {
                self.packet.clear();
                self.packet_len = 0;
                self.reject(now, out);
            }
            return;
        }
        match self.phase {
            Phase::Asking { deadline, .. } if now >= deadline => {
                self.phase = Phase::Done(Err(if self.started {
                    ReceiveFailure::Stalled
                } else {
                    ReceiveFailure::NoSender
                }));
            }
            Phase::Asking {
                next_ask,
                asks,
                deadline,
            } if now >= next_ask => {
                // XMODEM: no answer to 3 `C`, the sender may only know the sum
                if !self.ymodem && !self.started && asks >= CRC_ASKS {
                    self.crc = false;
                }
                out.push(if self.crc { CRC } else { NAK });
                self.phase = Phase::Asking {
                    next_ask: now + ASK_EVERY,
                    asks: asks + 1,
                    deadline,
                };
            }
            Phase::Waiting { deadline } if now >= deadline => self.reject(now, out),
            _ => {}
        }
    }
}

/// The name a YMODEM sender gave, as a file name only: the last part of its path (`/` or
/// `\`), so a sender can't pick the folder (`../../.bashrc` is `.bashrc`); `received.bin`
/// when nothing is left
fn file_name(raw: &[u8]) -> String {
    let name = String::from_utf8_lossy(raw);
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    match last {
        "" | "." | ".." => "received.bin".to_string(),
        last => last.to_string(),
    }
}

#[cfg(test)]
#[path = "../tests/transfer/modem_rx.rs"]
mod tests;
