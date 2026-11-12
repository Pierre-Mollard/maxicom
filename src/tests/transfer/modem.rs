use std::ops::Range;

use super::*;

/// 115200 baud in 8N1
const RATE: f64 = 11_520.0;

fn sender(protocol: Protocol, content: &[u8], start: Instant) -> ModemSender {
    ModemSender::new(
        Box::new(io::Cursor::new(content.to_vec())),
        protocol,
        "fw.bin".to_string(),
        content.len() as u64,
        RATE,
        start,
    )
}

fn step(sender: &mut ModemSender, rx: &[u8], at: Instant) -> Vec<u8> {
    sender.step(rx, at).unwrap().out
}

fn protocol_part(sender: &mut ModemSender, rx: &[u8], at: Instant) -> Range<usize> {
    sender.step(rx, at).unwrap().protocol
}

/// What a receiver checks in a packet: the start byte, the block number and its complement,
/// the check; gives the block number and the data
fn parse(packet: &[u8], crc: bool) -> (u8, Vec<u8>) {
    let len = match packet[0] {
        SOH => 128,
        STX => 1024,
        other => panic!("not a packet start: {other:#04x}"),
    };
    assert_eq!(packet.len(), 3 + len + if crc { 2 } else { 1 });
    assert_eq!(packet[1], !packet[2]);
    let data = packet[3..3 + len].to_vec();
    if crc {
        assert_eq!(packet[3 + len..], crc16(&data).to_be_bytes());
    } else {
        let sum = data.iter().fold(0u8, |sum, &byte| sum.wrapping_add(byte));
        assert_eq!(packet[3 + len], sum);
    }
    (packet[1], data)
}

fn padded(content: &[u8], len: usize) -> Vec<u8> {
    let mut data = content.to_vec();
    data.resize(len, PAD);
    data
}

#[test]
fn crc16_check_value() {
    assert_eq!(crc16(b"123456789"), 0x31C3);
}

/// NAK picks the checksum: 128 byte blocks numbered from 1, the last padded, then EOT.
#[test]
fn xmodem_checksum() {
    let start = Instant::now();
    let content: Vec<u8> = (0..200u8).collect();
    let mut sender = sender(Protocol::Xmodem, &content, start);
    assert_eq!(step(&mut sender, b"", start), b"");

    let packet = step(&mut sender, &[NAK], start);
    assert_eq!(parse(&packet, false), (1, content[..128].to_vec()));
    let packet = step(&mut sender, &[ACK], start);
    assert_eq!(parse(&packet, false), (2, padded(&content[128..], 128)));
    assert_eq!(sender.sent(), 128);
    assert_eq!(step(&mut sender, &[ACK], start), [EOT]);
    assert_eq!(sender.sent(), 200);
    assert_eq!(sender.outcome(), None);
    assert_eq!(step(&mut sender, &[ACK], start), b"");
    assert_eq!(sender.outcome(), Some(Ok(())));
}

/// C picks CRC-16; plain XMODEM keeps 128 byte blocks.
#[test]
fn xmodem_crc() {
    let start = Instant::now();
    let mut sender = sender(Protocol::Xmodem, b"hello", start);
    let packet = step(&mut sender, b"C", start);
    assert_eq!(parse(&packet, true), (1, padded(b"hello", 128)));
    assert_eq!(step(&mut sender, &[ACK], start), [EOT]);
    step(&mut sender, &[ACK], start);
    assert_eq!(sender.outcome(), Some(Ok(())));
}

/// XMODEM-1K sends 1024 byte blocks, and a 128 one when 128 bytes or less are left.
#[test]
fn xmodem_1k_blocks() {
    let start = Instant::now();
    let content = vec![b'x'; 1100];
    let mut sender = sender(Protocol::Xmodem1k, &content, start);
    let packet = step(&mut sender, b"C", start);
    assert_eq!(parse(&packet, true), (1, content[..1024].to_vec()));
    let packet = step(&mut sender, &[ACK], start);
    assert_eq!(parse(&packet, true), (2, padded(&content[1024..], 128)));

    // 200 bytes left: one 1024 block, not two 128 ones
    let content = vec![b'x'; 1224];
    let mut sender = self::sender(Protocol::Xmodem1k, &content, start);
    step(&mut sender, b"C", start);
    let packet = step(&mut sender, &[ACK], start);
    assert_eq!(parse(&packet, true), (2, padded(&content[1024..], 1024)));
}

/// A receiver asking for checksums gets 128 byte blocks: 1K blocks need the CRC.
#[test]
fn xmodem_1k_falls_back_to_128_with_checksum() {
    let start = Instant::now();
    let content = vec![b'x'; 300];
    let mut sender = sender(Protocol::Xmodem1k, &content, start);
    let packet = step(&mut sender, &[NAK], start);
    assert_eq!(parse(&packet, false), (1, content[..128].to_vec()));
}

/// Block numbers wrap from 255 to 0.
#[test]
fn block_number_wraps() {
    let start = Instant::now();
    let mut sender = sender(Protocol::Xmodem, &[0; 128 * 257], start);
    let mut packet = step(&mut sender, &[NAK], start);
    for _ in 1..256 {
        packet = step(&mut sender, &[ACK], start);
    }
    assert_eq!(parse(&packet, false).0, 0);
    let packet = step(&mut sender, &[ACK], start);
    assert_eq!(parse(&packet, false).0, 1);
}

/// A NAK sends the same packet again; the 11th refusal ends the transfer.
#[test]
fn nak_retries() {
    let start = Instant::now();
    let mut sender = sender(Protocol::Xmodem, b"abc", start);
    let first = step(&mut sender, &[NAK], start);
    for retry in 1..=RETRIES {
        assert_eq!(step(&mut sender, &[NAK], start), first);
        assert_eq!(sender.retries(), retry);
    }
    assert_eq!(step(&mut sender, &[NAK], start), b"");
    assert_eq!(sender.outcome(), Some(Err(Failure::TooManyRetries)));
}

/// No answer: the packet goes again once the line carried it and the ACK timeout ran out.
#[test]
fn timeout_resends() {
    let start = Instant::now();
    let mut sender = sender(Protocol::Xmodem, b"abc", start);
    let first = step(&mut sender, &[NAK], start);
    // 132 bytes at 11520 a second: about 11.5 ms
    assert_eq!(step(&mut sender, b"", start + ACK_TIMEOUT), b"");
    let again = start + ACK_TIMEOUT + Duration::from_millis(12);
    assert_eq!(step(&mut sender, b"", again), first);
    assert_eq!(sender.retries(), 1);
}

/// Text the receiver prints before it starts is ignored; nothing for a minute ends it.
#[test]
fn no_receiver() {
    let start = Instant::now();
    let mut sender = sender(Protocol::Xmodem, b"abc", start);
    assert_eq!(
        step(&mut sender, b"## ready for binary download\r\n", start),
        b""
    );
    assert_eq!(sender.outcome(), None);
    step(&mut sender, b"", start + START_TIMEOUT);
    assert_eq!(sender.outcome(), Some(Err(Failure::NoReceiver)));
}

/// Two CANs in a row cancel; a single one is noise.
#[test]
fn receiver_cancels() {
    let start = Instant::now();
    let mut sender = sender(Protocol::Xmodem, &[0; 300], start);
    step(&mut sender, &[NAK], start);
    let packet = step(&mut sender, &[CAN, ACK], start);
    assert_eq!(parse(&packet, false).0, 2);
    assert_eq!(step(&mut sender, &[CAN, CAN, ACK], start), b"");
    assert_eq!(sender.outcome(), Some(Err(Failure::ReceiverCancelled)));
}

/// `cancel` gives the CANs once.
#[test]
fn cancel() {
    let start = Instant::now();
    let mut sender = sender(Protocol::Xmodem, b"abc", start);
    step(&mut sender, &[NAK], start);
    assert_eq!(sender.cancel(), CANCEL);
    assert_eq!(sender.outcome(), Some(Err(Failure::Cancelled)));
    assert_eq!(sender.cancel(), b"");
    assert_eq!(step(&mut sender, &[ACK], start), b"");
}

#[test]
fn empty_file() {
    let start = Instant::now();
    let mut sender = sender(Protocol::Xmodem, b"", start);
    assert_eq!(step(&mut sender, &[NAK], start), [EOT]);
    step(&mut sender, &[ACK], start);
    assert_eq!(sender.outcome(), Some(Ok(())));
}

/// YMODEM: header block 0 (name, size), C, data, EOT NAK EOT ACK, C, empty block 0, ACK.
#[test]
fn ymodem_batch_of_one() {
    let start = Instant::now();
    let content = vec![b'y'; 300];
    let mut sender = sender(Protocol::Ymodem, &content, start);

    let packet = step(&mut sender, b"C", start);
    let (block, data) = parse(&packet, true);
    assert_eq!(block, 0);
    assert_eq!(data, padded_nul(b"fw.bin\x00300\x00"));

    // U-Boot sends the ACK and the C together
    let packet = step(&mut sender, b"\x06C", start);
    assert_eq!(parse(&packet, true), (1, padded(&content, 1024)));
    assert_eq!(step(&mut sender, &[ACK], start), [EOT]);
    assert_eq!(step(&mut sender, &[NAK], start), [EOT]);
    assert_eq!(step(&mut sender, &[ACK], start), b"");
    assert_eq!(sender.sent(), 300);

    let packet = step(&mut sender, b"C", start);
    assert_eq!(parse(&packet, true), (0, vec![0; 128]));
    assert_eq!(sender.outcome(), None);
    step(&mut sender, &[ACK], start);
    assert_eq!(sender.outcome(), Some(Ok(())));
}

/// With the file's modification time, the YMODEM header has it after the size, in octal.
#[test]
fn ymodem_header_with_mtime() {
    let start = Instant::now();
    let mut sender = sender(Protocol::Ymodem, &[b'y'; 300], start).with_mtime(0o15071006615);
    let packet = step(&mut sender, b"C", start);
    assert_eq!(
        parse(&packet, true).1,
        padded_nul(b"fw.bin\x00300 15071006615\x00")
    );
}

/// No C after the header's ACK: the receiver stalled.
#[test]
fn ymodem_stalled() {
    let start = Instant::now();
    let mut sender = sender(Protocol::Ymodem, b"abc", start);
    step(&mut sender, b"C", start);
    step(&mut sender, &[ACK], start);
    step(&mut sender, b"", start + ACK_TIMEOUT);
    assert_eq!(sender.outcome(), Some(Err(Failure::Stalled)));
}

fn padded_nul(content: &[u8]) -> Vec<u8> {
    let mut data = content.to_vec();
    data.resize(128, 0);
    data
}

/// What the device prints before the receiver starts and after the transfer ends is its output;
/// from the first NAK or C to the last answer, the bytes are the protocol's.
#[test]
fn protocol_part_of_rx() {
    let start = Instant::now();
    let mut sender = sender(Protocol::Xmodem, b"abc", start);
    assert_eq!(protocol_part(&mut sender, b"ready\r\n", start), 7..7);
    assert!(!sender.started());
    assert_eq!(protocol_part(&mut sender, b"go\r\nCC", start), 4..6);
    assert!(sender.started());
    assert_eq!(protocol_part(&mut sender, b"C\x06", start), 0..2);
    assert_eq!(protocol_part(&mut sender, b"\x06## Total\r\n", start), 0..1);
    assert_eq!(sender.outcome(), Some(Ok(())));
    assert_eq!(protocol_part(&mut sender, b"=> ", start), 3..3);
}

/// CAN CAN before the start ends it, with nothing taken from the device's output.
#[test]
fn cancelled_before_start() {
    let start = Instant::now();
    let mut sender = sender(Protocol::Xmodem, b"abc", start);
    assert_eq!(protocol_part(&mut sender, b"\x18\x18abort", start), 7..7);
    assert_eq!(sender.outcome(), Some(Err(Failure::ReceiverCancelled)));
}
