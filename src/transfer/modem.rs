//! XMODEM, XMODEM-1K and YMODEM (MAX-162), written from Chuck Forsberg's "XMODEM/YMODEM
//! Protocol Reference", not from lrzsz or minicom (GPL).
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
//! What both sides share is here: the bytes, the timeouts, the packet layout and the CRC.
//! The sender is `ModemSender` in `modem_tx.rs`, the receiver `ModemReceiver` in
//! `modem_rx.rs`. Neither does I/O, so the tests drive them with a scripted other side, or
//! with each other. The loop calls their `step` every turn with what the link received (often
//! nothing) and writes what it returns; the time is passed in, so the timeouts are checked
//! there too.

use std::time::Duration;

pub const SOH: u8 = 0x01; // start 128  byte block
pub const STX: u8 = 0x02; // start 1024 byte block
pub const EOT: u8 = 0x04; // end of transfer
pub const ACK: u8 = 0x06; // acknowledged
pub const NAK: u8 = 0x15; // request resend
pub const CAN: u8 = 0x18; // abort
/// The receiver asks for CRC-16 blocks with it instead of NAK
pub const CRC: u8 = b'C';
/// Pads the last block (CP/M end of file)
pub const PAD: u8 = 0x1A;

/// How long the receiver has to send its first NAK or C: the user starts it (`loady`, `rx`)
/// after the transfer, and it asks again every few seconds
pub const START_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a sent packet waits for its answer, once the line has carried it
pub const ACK_TIMEOUT: Duration = Duration::from_secs(10);
/// How many times a packet is sent again (NAK or no answer) before giving up
pub const RETRIES: u32 = 10;
/// CANs sent to stop: the spec needs two in a row, the rest covers one lost to noise
pub const CANCEL: [u8; 5] = [CAN; 5];

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

impl Protocol {
    pub fn name(self) -> &'static str {
        match self {
            Self::Xmodem => "XMODEM",
            Self::Xmodem1k => "XMODEM-1K",
            Self::Ymodem => "YMODEM",
        }
    }
}

/// SOH (128 bytes) or STX (1024), the block number and its complement, the data, then the
/// checksum or the CRC-16 high byte first
pub fn packet(block: u8, data: &[u8], crc: bool) -> Vec<u8> {
    let mut packet = Vec::with_capacity(data.len() + 5);
    packet.push(if data.len() == 1024 { STX } else { SOH });
    // `!block` flips the bits: block 1 goes with 0xFE, block 0 with 0xFF
    packet.extend_from_slice(&[block, !block]);
    packet.extend_from_slice(data);
    packet.extend_from_slice(&check(data, crc));
    packet
}

/// The check after a packet's data: the CRC-16 high byte first, or the 1 byte sum. Only the
/// data is checked, not the header bytes before it
pub fn check(data: &[u8], crc: bool) -> Vec<u8> {
    if crc {
        crc16(data).to_be_bytes().to_vec()
    } else {
        // The original XMODEM check: all data bytes added, keeping the low 8 bits
        vec![data.iter().fold(0u8, |sum, &byte| sum.wrapping_add(byte))]
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
