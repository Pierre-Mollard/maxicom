use std::io;

use super::*;
use crate::transfer::{modem::packet, modem_tx::ModemSender};

fn receiver(protocol: Protocol, start: Instant) -> ModemReceiver {
    ModemReceiver::new(protocol, start)
}

/// What a run of steps gave: the bytes written, and the events
fn feed(receiver: &mut ModemReceiver, rx: &[u8], at: Instant) -> (Vec<u8>, Vec<Event>) {
    let step = receiver.step(rx, at);
    (step.out, step.events)
}

/// The file the events wrote, and how many files they started and ended
fn written(events: &[Event]) -> Vec<u8> {
    events
        .iter()
        .flat_map(|event| match event {
            Event::Data(data) => data.clone(),
            _ => Vec::new(),
        })
        .collect()
}

/// The receiver against `ModemSender`, each passing the other what it wrote, at one instant
fn round_trip(send: Protocol, receive: Protocol, content: &[u8]) -> Vec<Event> {
    let start = Instant::now();
    let mut sender = ModemSender::new(
        Box::new(io::Cursor::new(content.to_vec())),
        send,
        "fw.bin".to_string(),
        content.len() as u64,
        11_520.0,
        start,
    )
    .with_mtime(0o15071006615);
    let mut receiver = receiver(receive, start);
    let mut events = Vec::new();
    let mut to_sender = Vec::new();
    for _ in 0..10_000 {
        let step = receiver.step(&to_sender_rx(&mut to_sender), start);
        events.extend(step.events);
        let sent = sender.step(&step.out, start).unwrap().out;
        to_sender = sent;
        if receiver.outcome().is_some() && sender.outcome().is_some() {
            break;
        }
    }
    assert_eq!(sender.outcome(), Some(Ok(())));
    assert_eq!(receiver.outcome(), Some(Ok(())));
    events
}

fn to_sender_rx(bytes: &mut Vec<u8>) -> Vec<u8> {
    std::mem::take(bytes)
}

fn padded(content: &[u8], len: usize) -> Vec<u8> {
    let mut data = content.to_vec();
    data.resize(len, 0x1A);
    data
}

/// YMODEM against the sender: the name, size and date, the data cut to the size, then the
/// batch's end. Sizes on and around the block boundaries.
#[test]
fn ymodem_round_trip() {
    for len in [0, 1, 128, 129, 1024, 1025, 3000] {
        let content: Vec<u8> = (0..len).map(|i| (i * 7) as u8).collect();
        let events = round_trip(Protocol::Ymodem, Protocol::Ymodem, &content);
        assert_eq!(
            events.first(),
            Some(&Event::File {
                name: "fw.bin".to_string(),
                size: Some(len as u64),
                mtime: Some(0o15071006615),
            })
        );
        assert_eq!(events.last(), Some(&Event::FileEnd));
        assert_eq!(written(&events), content, "{len} bytes");
    }
}

/// XMODEM and XMODEM-1K against the sender: the data and the last block's padding, no name.
#[test]
fn xmodem_round_trip() {
    let content: Vec<u8> = (0..1100u32).map(|i| i as u8).collect();
    for (send, padded_len) in [(Protocol::Xmodem, 1152), (Protocol::Xmodem1k, 1152)] {
        let events = round_trip(send, Protocol::Xmodem, &content);
        assert!(!events.iter().any(|e| matches!(e, Event::File { .. })));
        assert_eq!(events.last(), Some(&Event::FileEnd));
        assert_eq!(written(&events), padded(&content, padded_len));
    }
}

/// Before the first packet the receiver asks with `C` every 3 s; XMODEM asks with NAK after
/// 3 unanswered `C`, and takes the 1 byte sum from then.
#[test]
fn xmodem_falls_back_to_the_sum() {
    let start = Instant::now();
    let secs = |secs| start + Duration::from_secs(secs);
    let mut receiver = receiver(Protocol::Xmodem, start);
    assert_eq!(feed(&mut receiver, b"", start).0, b"C");
    assert_eq!(feed(&mut receiver, b"", secs(1)).0, b"");
    assert_eq!(feed(&mut receiver, b"", secs(3)).0, b"C");
    assert_eq!(feed(&mut receiver, b"", secs(6)).0, b"C");
    assert_eq!(feed(&mut receiver, b"", secs(9)).0, [NAK]);

    let data = [b'x'; 128];
    let (out, events) = feed(&mut receiver, &packet(1, &data, false), secs(10));
    assert_eq!((out, events), (vec![ACK], vec![Event::Data(data.to_vec())]));
}

/// YMODEM needs the CRC: it keeps asking with `C`.
#[test]
fn ymodem_keeps_asking_with_c() {
    let start = Instant::now();
    let mut receiver = receiver(Protocol::Ymodem, start);
    for secs in [0, 3, 6, 9, 12] {
        let out = feed(&mut receiver, b"", start + Duration::from_secs(secs)).0;
        assert_eq!(out, b"C");
    }
}

/// Nothing for a minute: no sender.
#[test]
fn no_sender() {
    let start = Instant::now();
    let mut receiver = receiver(Protocol::Ymodem, start);
    feed(&mut receiver, b"", start + START_TIMEOUT);
    assert_eq!(receiver.outcome(), Some(Err(ReceiveFailure::NoSender)));
}

/// A damaged packet (data or block number) gets a NAK, then comes again; 10 damaged in a row
/// cancel.
#[test]
fn damaged_packets() {
    let start = Instant::now();
    let mut receiver = receiver(Protocol::Xmodem, start);
    feed(&mut receiver, b"", start);
    let mut bad = packet(1, &[b'x'; 128], true);
    bad[50] ^= 0xFF;
    assert_eq!(feed(&mut receiver, &bad, start).0, [NAK]);
    let mut bad_number = packet(1, &[b'x'; 128], true);
    bad_number[2] = 0;
    assert_eq!(feed(&mut receiver, &bad_number, start).0, [NAK]);
    let (out, events) = feed(&mut receiver, &packet(1, &[b'x'; 128], true), start);
    assert_eq!((out, events.len()), (vec![ACK], 1));
    assert_eq!(receiver.retries(), 2);

    for _ in 0..RETRIES {
        assert_eq!(feed(&mut receiver, &bad, start).0, [NAK]);
    }
    assert_eq!(feed(&mut receiver, &bad, start).0, CANCEL);
    assert_eq!(
        receiver.outcome(),
        Some(Err(ReceiveFailure::TooManyRetries))
    );
}

/// A block sent twice (its ACK was lost) is acknowledged and skipped; a block out of order
/// cancels.
#[test]
fn duplicate_and_out_of_order() {
    let start = Instant::now();
    let mut receiver = receiver(Protocol::Xmodem, start);
    feed(&mut receiver, b"", start);
    let block1 = packet(1, &[1; 128], true);
    assert_eq!(feed(&mut receiver, &block1, start).1.len(), 1);
    let (out, events) = feed(&mut receiver, &block1, start);
    assert_eq!((out, events), (vec![ACK], vec![]));
    assert_eq!(
        feed(&mut receiver, &packet(2, &[2; 128], true), start).0,
        [ACK]
    );
    assert_eq!(
        feed(&mut receiver, &packet(4, &[4; 128], true), start).0,
        CANCEL
    );
    assert_eq!(receiver.outcome(), Some(Err(ReceiveFailure::OutOfSync)));
}

/// A packet that comes in pieces is read whole; one whose bytes stop for a second is
/// dropped with a NAK. No packet for 10 s: a NAK too.
#[test]
fn packets_in_pieces_and_timeouts() {
    let start = Instant::now();
    let ms = |ms| start + Duration::from_millis(ms);
    let mut receiver = receiver(Protocol::Xmodem, start);
    feed(&mut receiver, b"", start);
    let block1 = packet(1, &[1; 128], true);
    assert_eq!(feed(&mut receiver, &block1[..60], ms(10)).0, b"");
    assert_eq!(feed(&mut receiver, &block1[60..], ms(20)).0, [ACK]);

    let block2 = packet(2, &[2; 128], true);
    assert_eq!(feed(&mut receiver, &block2[..60], ms(30)).0, b"");
    assert_eq!(feed(&mut receiver, b"", ms(1030)).0, [NAK]);
    assert_eq!(feed(&mut receiver, &block2, ms(1040)).0, [ACK]);

    assert_eq!(feed(&mut receiver, b"", ms(1040 + 9_999)).0, b"");
    assert_eq!(feed(&mut receiver, b"", ms(1040 + 10_000)).0, [NAK]);
}

/// Two CANs from the sender end it; one is noise. `cancel` gives the CANs once.
#[test]
fn cancels() {
    let start = Instant::now();
    let mut receiver = receiver(Protocol::Xmodem, start);
    feed(&mut receiver, b"", start);
    feed(&mut receiver, &[CAN], start);
    assert_eq!(receiver.outcome(), None);
    feed(&mut receiver, &[CAN, CAN], start);
    assert_eq!(
        receiver.outcome(),
        Some(Err(ReceiveFailure::SenderCancelled))
    );

    let mut receiver = self::receiver(Protocol::Ymodem, start);
    assert_eq!(receiver.cancel(), CANCEL);
    assert_eq!(receiver.outcome(), Some(Err(ReceiveFailure::Cancelled)));
    assert_eq!(receiver.cancel(), b"");
}

/// The device's text before the first packet (the shell echoing `sb fw.bin`) and after the
/// end is its output; the packets and EOT between are the protocol's.
#[test]
fn protocol_part_of_rx() {
    let start = Instant::now();
    let mut receiver = receiver(Protocol::Xmodem, start);
    feed(&mut receiver, b"", start);
    let mut rx = b"sx fw.bin\r\n".to_vec();
    rx.extend(packet(1, &[1; 128], true));
    let step = receiver.step(&rx, start);
    assert_eq!(step.protocol, 11..rx.len());
    assert!(receiver.started());

    let step = receiver.step(b"\x04$ ", start);
    assert_eq!((step.out, step.protocol), (vec![ACK], 0..1));
    assert_eq!(receiver.outcome(), Some(Ok(())));
}

/// A YMODEM batch of two files, the second without a size or date, then the empty block 0.
#[test]
fn ymodem_batch() {
    let start = Instant::now();
    let mut receiver = receiver(Protocol::Ymodem, start);
    assert_eq!(feed(&mut receiver, b"", start).0, b"C");
    let header = |info: &[u8]| {
        let mut data = info.to_vec();
        data.resize(128, 0);
        packet(0, &data, true)
    };

    let (out, events) = feed(&mut receiver, &header(b"a.bin\x00130 17\x00"), start);
    assert_eq!(out, [ACK, CRC]);
    let file_a = Event::File {
        name: "a.bin".to_string(),
        size: Some(130),
        mtime: Some(0o17),
    };
    assert_eq!(events, [file_a]);
    feed(&mut receiver, &packet(1, &[b'a'; 128], true), start);
    let (_, events) = feed(&mut receiver, &packet(2, &padded(b"aa", 128), true), start);
    assert_eq!(events, [Event::Data(b"aa".to_vec())]);
    assert_eq!(receiver.received(), (130, Some(130)));
    assert_eq!(feed(&mut receiver, &[EOT], start).0, [NAK]);
    let (out, events) = feed(&mut receiver, &[EOT], start);
    assert_eq!((out, events), (vec![ACK, CRC], vec![Event::FileEnd]));

    let (_, events) = feed(&mut receiver, &header(b"dir/b.txt\x00"), start);
    let file_b = Event::File {
        name: "b.txt".to_string(),
        size: None,
        mtime: None,
    };
    assert_eq!(events, [file_b]);
    let (_, events) = feed(&mut receiver, &packet(1, &padded(b"bb", 128), true), start);
    assert_eq!(written(&events), padded(b"bb", 128));
    feed(&mut receiver, &[EOT], start);
    feed(&mut receiver, &[EOT], start);

    assert_eq!(feed(&mut receiver, &header(b""), start).0, [ACK]);
    assert_eq!(receiver.outcome(), Some(Ok(())));
}

/// A sender's name keeps its last part only: it can't pick the folder.
#[test]
fn file_names() {
    assert_eq!(file_name(b"fw.bin"), "fw.bin");
    assert_eq!(file_name(b"../../.bashrc"), ".bashrc");
    assert_eq!(file_name(b"/etc/passwd"), "passwd");
    assert_eq!(file_name(b"C:\\temp\\fw.bin"), "fw.bin");
    assert_eq!(file_name(b"dir/.."), "received.bin");
    assert_eq!(file_name(b"dir/"), "received.bin");
}

/// After YMODEM's block 0, no data comes: the receiver asks again, then gives up.
#[test]
fn ymodem_stalls() {
    let start = Instant::now();
    let mut receiver = receiver(Protocol::Ymodem, start);
    feed(&mut receiver, b"", start);
    let mut data = b"a.bin\x0010\x00".to_vec();
    data.resize(128, 0);
    feed(&mut receiver, &packet(0, &data, true), start);
    assert_eq!(feed(&mut receiver, b"", start + ASK_EVERY).0, b"C");
    feed(&mut receiver, b"", start + STALL_TIMEOUT);
    assert_eq!(receiver.outcome(), Some(Err(ReceiveFailure::Stalled)));
}
