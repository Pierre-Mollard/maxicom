//! Sending a file with XMODEM, XMODEM-1K or YMODEM (MAX-162): the protocols are explained in
//! `modem.rs`, with what both sides share.

use std::{
    fmt,
    io::{self, Read},
    time::{Duration, Instant},
};

use super::{
    Step,
    modem::{
        ACK, ACK_TIMEOUT, CAN, CANCEL, CRC, EOT, NAK, PAD, Protocol, RETRIES, START_TIMEOUT, packet,
    },
};

/// Why a send ended before the receiver had the file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendFailure {
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

impl fmt::Display for SendFailure {
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
    Done(Result<(), SendFailure>),
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
    /// The file's modification time, seconds since 1970, for the YMODEM header
    mtime: Option<u64>,
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
            mtime: None,
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

    /// Sends the file's modification time in the YMODEM header: without it, lrzsz's `rb`
    /// dates the file it writes with garbage
    pub fn with_mtime(mut self, mtime: u64) -> Self {
        self.mtime = Some(mtime);
        self
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
    pub fn outcome(&self) -> Option<Result<(), SendFailure>> {
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
                    self.state = State::Done(Err(SendFailure::ReceiverCancelled));
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
        self.state = State::Done(Err(SendFailure::Cancelled));
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
                    SendFailure::NoReceiver
                } else {
                    SendFailure::Stalled
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
            self.state = State::Done(Err(SendFailure::TooManyRetries));
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
        // e.g. "fw.bin\0" "131072 15071006615\0" then NULs: the size in decimal, then the
        // modification time in octal (seconds since 1970), separated by a space. The spec
        // allows more fields (mode, serial number) that receivers treat as optional. With the
        // size the receiver cuts the padding off the last block
        let mut data = Vec::new();
        if !end_batch {
            data.extend_from_slice(self.name.as_bytes());
            data.push(0);
            data.extend_from_slice(self.size.to_string().as_bytes());
            if let Some(mtime) = self.mtime {
                data.extend_from_slice(format!(" {mtime:o}").as_bytes());
            }
            data.push(0);
        }
        // A very long name doesn't fit 128 bytes: the header goes in a 1024 block then
        let len = if data.len() <= 128 { 128 } else { 1024 };
        data.truncate(len);
        data.resize(len, 0);
        self.packet(0, &data)
    }

    fn packet(&self, block: u8, data: &[u8]) -> Vec<u8> {
        packet(block, data, self.crc)
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

#[cfg(test)]
#[path = "../tests/transfer/modem_tx.rs"]
mod tests;
