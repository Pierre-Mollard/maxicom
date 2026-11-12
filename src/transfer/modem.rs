//! Sending a file with XMODEM, XMODEM-1K or YMODEM (MAX-162), written from Chuck Forsberg's
//! "XMODEM/YMODEM Protocol Reference", not from lrzsz or minicom (GPL).
//!
//! ## The idea
//!
//! The receiver drives everything. The sender never sends on its own: it waits for the receiver
//! to ask, sends one packet, then waits for the answer to that packet before the next one. The
//! receiver checks each packet and says ACK (good, next one) or NAK (bad, send it again). It is
//! slow (one round trip per packet) but very simple, which is why bootloaders like it.
//!
//! ## A packet
//!
//! ```text
//! +-----------+-------+--------+---------------------+------------------------+
//! | SOH / STX | block | !block | data, 128 or 1024 B | checksum (1 B) or CRC  |
//! +-----------+-------+--------+---------------------+------------------------+
//! ```
//!
//! - SOH starts a 128 byte packet, STX a 1024 byte one (XMODEM-1K, YMODEM).
//! - `block` counts the packets from 1 and wraps 255 → 0; `!block` is the same number with its
//!   bits flipped, so the receiver sees a damaged number. The block number also lets the
//!   receiver spot a packet sent twice (its ACK got lost) and ignore the copy.
//! - The data is always full: the end of the file is padded with 0x1A.
//! - The check is either a 1 byte sum of the data (original XMODEM) or a 2 byte CRC-16
//!   (catches far more errors). The receiver picks: it starts by sending NAK for the sum, or the
//!   letter `C` for the CRC.
//!
//! ## XMODEM, one file, no name
//!
//! ```text
//! receiver            sender
//!    C / NAK  ──▶                    "ready, use CRC / the sum", repeated every few seconds
//!             ◀──  block 1
//!    ACK      ──▶
//!             ◀──  block 2
//!    NAK      ──▶                    block 2 arrived damaged
//!             ◀──  block 2           same packet again
//!    ACK      ──▶
//!             ◀──  EOT               end of the file
//!    ACK      ──▶                    done
//! ```
//!
//! The receiver doesn't know the file's name or size: it keeps the padding, and the user names
//! the file on its side.
//!
//! ## YMODEM, XMODEM-1K with a header
//!
//! YMODEM adds a block 0 in front, carrying the name and the size, so the receiver can drop the
//! padding. It is a batch protocol (several files in a row); maxicom sends one, then the empty
//! block 0 that says "no more files".
//!
//! ```text
//! receiver            sender
//!    C        ──▶
//!             ◀──  block 0: "fw.bin\0" "300\0" NULs
//!    ACK      ──▶
//!    C        ──▶                    "now the data"
//!             ◀──  block 1 … block n (as XMODEM-1K)
//!    ACK      ──▶
//!             ◀──  EOT
//!    NAK      ──▶                    many receivers NAK the first EOT, to be sure it is one
//!             ◀──  EOT
//!    ACK      ──▶
//!    C        ──▶                    "next file?"
//!             ◀──  block 0, all NUL  "no more files"
//!    ACK      ──▶                    done
//! ```
//!
//! ## Stopping
//!
//! Either side stops the transfer with CAN bytes; two in a row are needed, so one CAN byte
//! damaged by noise doesn't kill a transfer.
//!
//! ## In this module
//!
//! `ModemSender` does no I/O, so the tests drive it with a scripted receiver. The loop calls
//! `step` every turn with what the link received (often nothing) and writes what it returns;
//! the time is passed in, so the timeouts are checked there too.

use std::{
    fmt,
    io::{self, Read},
    time::{Duration, Instant},
};

use super::Step;

const SOH: u8 = 0x01; // start 128  byte block
const STX: u8 = 0x02; // start 1024 byte block
const EOT: u8 = 0x04; // end of transfer
const ACK: u8 = 0x06; // acknowledged
const NAK: u8 = 0x15; // request resend
const CAN: u8 = 0x18; // abort
/// The receiver asks for CRC-16 blocks with it instead of NAK
const CRC: u8 = b'C';
/// Pads the last block (CP/M end of file)
const PAD: u8 = 0x1A;

/// How long the receiver has to send its first NAK or C: the user starts it (`loady`, `rx`)
/// after the transfer, and it asks again every few seconds
const START_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a sent packet waits for its answer, once the line has carried it
const ACK_TIMEOUT: Duration = Duration::from_secs(10);
/// How many times a packet is sent again (NAK or no answer) before giving up
const RETRIES: u32 = 10;
/// CANs sent to stop: the spec needs two in a row, the rest covers one lost to noise
const CANCEL: [u8; 5] = [CAN; 5];

/// The protocols a file is sent with (MAX-162), written from Forsberg's XMODEM/YMODEM Protocol
/// Reference. The receiver picks the check: NAK for a checksum, C for CRC-16
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Xmodem,
    /// 1024 byte blocks in CRC mode, 128 in checksum mode and for a short last block
    Xmodem1k,
    /// XMODEM-1K after a block 0 with the name and size, and an empty block 0 to end the batch
    Ymodem,
}

/// Why a send ended before the receiver had the file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// No NAK or C in `START_TIMEOUT`
    NoReceiver,
    /// The receiver sent CAN CAN
    ReceiverCancelled,
    /// A packet was refused or unanswered `RETRIES` times
    TooManyRetries,
    /// The receiver stopped asking (YMODEM: no C after the header or the end of the file)
    Stalled,
    /// `cancel` stopped it
    Cancelled,
}

impl Protocol {
    pub fn name(self) -> &'static str {
        match self {
            Self::Xmodem => "XMODEM",
            Self::Xmodem1k => "XMODEM-1K",
            Self::Ymodem => "YMODEM",
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoReceiver => "no NAK or C from the receiver: is it waiting (loady, loadx, rx)?",
            Self::ReceiverCancelled => "the receiver cancelled",
            Self::TooManyRetries => "too many retries",
            Self::Stalled => "the receiver stopped answering",
            Self::Cancelled => "cancelled",
        })
    }
}

/// What the sender waits for next
#[derive(Debug)]
enum State {
    /// A NAK or C, then sends `next`
    Start {
        next: Next,
        deadline: Instant,
    },
    /// The answer to `packet`, sent again on a NAK or a timeout
    Answer {
        stage: Stage,
        packet: Vec<u8>,
        /// The file bytes in it, without the padding
        data_len: usize,
        retries: u32,
        deadline: Instant,
    },
    Done(Result<(), Failure>),
}

/// What a NAK or C starts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    /// The transfer: the receiver's first NAK or C also picks the check
    First,
    /// The first data block, after the YMODEM header
    Data,
    /// The empty block 0 that ends a YMODEM batch, after the file's EOT
    EndBatch,
}

/// What the packet waiting for its answer is
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Header,
    Data,
    Eot,
    EndBatch,
}

/// The sending side of XMODEM / YMODEM, without I/O: `step` takes the bytes the receiver sent
/// and the time, and gives the bytes to write. Called every loop turn, with no bytes too, for
/// the timeouts
pub struct ModemSender {
    reader: Box<dyn Read + Send>,
    protocol: Protocol,
    name: String,
    size: u64,
    bytes_per_sec: f64,
    /// CRC-16 blocks, else an 8 bit checksum: the receiver's first answer decides
    crc: bool,
    block: u8,
    /// File bytes the receiver acknowledged
    sent: u64,
    /// Packets sent again, over the whole transfer
    retries: u32,
    /// CANs received in a row
    cans: u8,
    /// The receiver's first NAK or C came: the link is the transfer's until it ends
    started: bool,
    state: State,
}

impl fmt::Debug for ModemSender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModemSender")
            .field("protocol", &self.protocol)
            .field("name", &self.name)
            .field("sent", &self.sent)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl ModemSender {
    pub fn new(
        reader: Box<dyn Read + Send>,
        protocol: Protocol,
        name: String,
        size: u64,
        bytes_per_sec: f64,
        now: Instant,
    ) -> Self {
        Self {
            reader,
            protocol,
            name,
            size,
            bytes_per_sec,
            crc: false,
            block: 0,
            sent: 0,
            retries: 0,
            cans: 0,
            started: false,
            state: State::Start {
                next: Next::First,
                deadline: now + START_TIMEOUT,
            },
        }
    }

    pub fn protocol(&self) -> Protocol {
        self.protocol
    }

    pub fn sent(&self) -> u64 {
        self.sent
    }

    pub fn retries(&self) -> u32 {
        self.retries
    }

    pub fn started(&self) -> bool {
        self.started
    }

    /// None while it runs
    pub fn outcome(&self) -> Option<Result<(), Failure>> {
        match self.state {
            State::Done(outcome) => Some(outcome),
            _ => None,
        }
    }

    /// Takes `rx`, what the receiver sent since the last step, then checks the timeouts; gives
    /// what to write and the part of `rx` that is the protocol's only. A read error leaves the
    /// transfer running: `cancel` it
    pub fn step(&mut self, rx: &[u8], now: Instant) -> io::Result<Step> {
        let mut out = Vec::new();
        // Where the protocol's part of `rx` starts: at 0 when the transfer was already running,
        // else at the byte that starts it, if one comes in this `rx`
        let mut start = (self.started && self.outcome().is_none()).then_some(0);
        let mut end = rx.len();
        // The receiver only ever sends single bytes (C, NAK, ACK, CAN), so `rx` is read one byte
        // at a time; a read can hold several, e.g. U-Boot's "ACK C" after the YMODEM header
        for (i, &byte) in rx.iter().enumerate() {
            // Ended: what follows is the device talking again (U-Boot's "## Total Size" line)
            if self.outcome().is_some() {
                end = i;
                break;
            }
            // One CAN can be noise, two in a row are the receiver giving up
            if byte == CAN {
                self.cans += 1;
                if self.cans >= 2 {
                    self.state = State::Done(Err(Failure::ReceiverCancelled));
                }
                continue;
            }
            self.cans = 0;
            self.answer(byte, now, &mut out)?;
            if start.is_none() && self.started {
                start = Some(i);
            }
        }
        // Then the clock: the receiver may have said nothing for too long
        self.check_timeout(now, &mut out);
        let start = start.unwrap_or(rx.len());
        Ok(Step {
            out,
            protocol: start..end.max(start),
        })
    }

    /// Stops the transfer: the CANs to write, none once it ended
    pub fn cancel(&mut self) -> Vec<u8> {
        if self.outcome().is_some() {
            return Vec::new();
        }
        self.state = State::Done(Err(Failure::Cancelled));
        CANCEL.to_vec()
    }

    /// One byte from the receiver, read against what the sender waits for. This is the whole
    /// protocol, the diagrams at the top of the file in code
    fn answer(&mut self, byte: u8, now: Instant, out: &mut Vec<u8>) -> io::Result<()> {
        match self.state {
            // The receiver asks for something: at the very start, after the YMODEM header, or
            // after the file's EOT in YMODEM
            State::Start { next, .. } if byte == NAK || byte == CRC => {
                // The first ask picks the check for the whole transfer: `C` for CRC-16, NAK for
                // the 1 byte sum. From there the link is the transfer's
                if next == Next::First {
                    self.crc = byte == CRC;
                    self.started = true;
                }
                match next {
                    // YMODEM starts with block 0: the file's name and size
                    Next::First if self.protocol == Protocol::Ymodem => {
                        let packet = self.header_packet(false);
                        self.send(Stage::Header, packet, 0, 0, now, out);
                    }
                    // XMODEM starts with the data right away, YMODEM after its header
                    Next::First | Next::Data => self.send_next_data(now, out)?,
                    // YMODEM asks for the next file: an empty block 0 says there is none
                    Next::EndBatch => {
                        let packet = self.header_packet(true);
                        self.send(Stage::EndBatch, packet, 0, 0, now, out);
                    }
                }
            }
            // The receiver got the packet we sent, and it was good: what comes next depends on
            // which packet it was
            State::Answer {
                stage, data_len, ..
            } if byte == ACK => match stage {
                // YMODEM: after the header the receiver sends one more `C` before the data
                Stage::Header => self.wait_start(Next::Data, now),
                // Count the file bytes it held (not the padding), then the next block, or EOT
                // if the file is all sent
                Stage::Data => {
                    self.sent += data_len as u64;
                    self.send_next_data(now, out)?;
                }
                // YMODEM: the file is done, the receiver will ask for the next one with a `C`
                Stage::Eot if self.protocol == Protocol::Ymodem => {
                    self.wait_start(Next::EndBatch, now);
                }
                // XMODEM's EOT, or YMODEM's empty block 0, acknowledged: finished
                Stage::Eot | Stage::EndBatch => self.state = State::Done(Ok(())),
            },
            // The packet arrived damaged (bad check, bad length, bad block number): same packet
            // again. A NAK after EOT is normal in YMODEM (EOT NAK EOT ACK): the receiver makes
            // sure the EOT wasn't a damaged byte
            State::Answer { .. } if byte == NAK => self.resend(now, out),
            // Anything else is ignored: text the device printed before its receiver started,
            // a `C` still repeated while our first packet was on the way, line noise
            _ => {}
        }
        Ok(())
    }

    /// The receiver said nothing in time. While it should ask (Start), it gives up: nobody
    /// is there. While a packet waits for its answer, the packet or the answer may have been
    /// lost on the line, so the packet goes again (counts as a retry)
    fn check_timeout(&mut self, now: Instant, out: &mut Vec<u8>) {
        match self.state {
            State::Start { next, deadline } if now >= deadline => {
                self.state = State::Done(Err(if next == Next::First {
                    Failure::NoReceiver
                } else {
                    Failure::Stalled
                }));
            }
            State::Answer { deadline, .. } if now >= deadline => self.resend(now, out),
            _ => {}
        }
    }

    /// Waits for the receiver's next `C` (YMODEM, between stages); it comes right after the
    /// ACK, so the shorter timeout
    fn wait_start(&mut self, next: Next, now: Instant) {
        self.state = State::Start {
            next,
            deadline: now + ACK_TIMEOUT,
        };
    }

    fn send(
        &mut self,
        stage: Stage,
        packet: Vec<u8>,
        data_len: usize,
        retries: u32,
        now: Instant,
        out: &mut Vec<u8>,
    ) {
        out.extend_from_slice(&packet);
        // The packet is kept until its answer, to send it again on a NAK. The wait counts from
        // when the line has carried it: 1029 bytes take almost 9 s at 1200 baud
        let deadline = now + self.line_time(packet.len()) + ACK_TIMEOUT;
        self.state = State::Answer {
            stage,
            packet,
            data_len,
            retries,
            deadline,
        };
    }

    /// The packet waiting for its answer goes again, the very same bytes (same block number,
    /// so a receiver that did get the first copy sees a duplicate and ignores it). Each packet
    /// has its own count: after `RETRIES` the line is too bad, or the receiver isn't listening
    fn resend(&mut self, now: Instant, out: &mut Vec<u8>) {
        let State::Answer {
            stage,
            ref mut packet,
            data_len,
            retries,
            ..
        } = self.state
        else {
            return;
        };
        if retries >= RETRIES {
            self.state = State::Done(Err(Failure::TooManyRetries));
            return;
        }
        let packet = std::mem::take(packet);
        self.retries += 1;
        self.send(stage, packet, data_len, retries + 1, now, out);
    }

    /// The next data block, or EOT at the end of the file
    fn send_next_data(&mut self, now: Instant, out: &mut Vec<u8>) -> io::Result<()> {
        // 1024 byte blocks only with the CRC: a 1 byte sum is too weak to check 1 KB, so a
        // receiver that asked for the sum gets 128 byte blocks even in XMODEM-1K / YMODEM
        let max = if self.crc && self.protocol != Protocol::Xmodem {
            1024
        } else {
            128
        };
        // Fill the block: a `read` may give less than asked before the end of the file
        let mut data = vec![0; max];
        let mut len = 0;
        while len < max {
            match self.reader.read(&mut data[len..]) {
                Ok(0) => break,
                Ok(n) => len += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        // Nothing left: EOT is a lone byte, not a packet, answered like one (ACK or NAK)
        if len == 0 {
            self.send(Stage::Eot, vec![EOT], 0, 0, now, out);
            return Ok(());
        }
        // The last bit of the file: a short 128 block when it fits, to send less padding;
        // the rest of the block is padded with 0x1A, as packets are always full
        data.truncate(if len <= 128 { 128 } else { max });
        data[len..].fill(PAD);
        // Data blocks count from 1 (0 is YMODEM's header) and wrap after 255 (block is u8)
        self.block = self.block.wrapping_add(1);
        let packet = self.packet(self.block, &data);
        self.send(Stage::Data, packet, len, 0, now, out);
        Ok(())
    }

    /// YMODEM block 0: the name, NUL, the size in decimal, NUL padded; all NUL to end the batch
    fn header_packet(&self, end_batch: bool) -> Vec<u8> {
        // e.g. "fw.bin\0" "131072\0" then NULs. The spec allows more fields after the size
        // (date, mode), separated by spaces; receivers treat them as optional, so none here.
        // With the size the receiver cuts the padding off the last block
        let mut data = Vec::new();
        if !end_batch {
            data.extend_from_slice(self.name.as_bytes());
            data.push(0);
            data.extend_from_slice(self.size.to_string().as_bytes());
            data.push(0);
        }
        // A very long name doesn't fit 128 bytes: the header goes in a 1024 block then
        let len = if data.len() <= 128 { 128 } else { 1024 };
        data.truncate(len);
        data.resize(len, 0);
        self.packet(0, &data)
    }

    /// SOH (128 bytes) or STX (1024), the block number and its complement, the data, then the
    /// checksum or the CRC-16 high byte first
    fn packet(&self, block: u8, data: &[u8]) -> Vec<u8> {
        let mut packet = Vec::with_capacity(data.len() + 5);
        packet.push(if data.len() == 1024 { STX } else { SOH });
        // `!block` flips the bits: block 1 goes with 0xFE, block 0 with 0xFF
        packet.extend_from_slice(&[block, !block]);
        packet.extend_from_slice(data);
        // Only the data is checked, not the header bytes before it
        if self.crc {
            packet.extend_from_slice(&crc16(data).to_be_bytes());
        } else {
            // The original XMODEM check: all data bytes added, keeping the low 8 bits
            packet.push(data.iter().fold(0u8, |sum, &byte| sum.wrapping_add(byte)));
        }
        packet
    }

    /// How long the line takes to carry `len` bytes
    fn line_time(&self, len: usize) -> Duration {
        if self.bytes_per_sec > 0.0 {
            Duration::from_secs_f64(len as f64 / self.bytes_per_sec)
        } else {
            Duration::ZERO
        }
    }
}

/// CRC-16/XMODEM: polynomial 0x1021, starts at 0, not reflected.
///
/// A CRC treats the data as one long binary number and keeps the remainder of its division by
/// the polynomial; any change in the data almost surely changes the remainder. Done bit by bit
/// here: each byte goes into the top of the 16 bit register, then for each of its 8 bits the
/// register shifts left, and when a 1 falls out the top the polynomial is XORed in (that is
/// the "subtract" of a binary division without carries). Slower than a lookup table, but 1 KB
/// per packet is nothing. The standard check: "123456789" gives 0x31C3 (tested)
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ 0x1021
            };
        }
    }
    crc
}

#[cfg(test)]
#[path = "../tests/transfer/modem.rs"]
mod tests;
