use super::*;

/// 9600 baud in 8N1
const RATE: f64 = 960.0;

fn raw(content: &[u8], start: Instant) -> FileTransfer {
    FileTransfer::raw(
        Box::new(io::Cursor::new(content.to_vec())),
        "fw.bin".to_string(),
        content.len() as u64,
        RATE,
        Duration::ZERO,
        start,
    )
}

fn modem(content: &[u8], start: Instant) -> FileTransfer {
    FileTransfer::modem(
        Box::new(io::Cursor::new(content.to_vec())),
        Protocol::Xmodem,
        "fw.bin".to_string(),
        content.len() as u64,
        RATE,
        start,
    )
}

/// A raw send ignores RX (none of it is the protocol's), gives what the line drains, and ends
/// at the end of the file; the rate counts from the start.
#[test]
fn raw_send() {
    let start = Instant::now();
    let ms = |ms| start + Duration::from_millis(ms);
    let mut transfer = raw(&[b'x'; 200], start);
    assert_eq!(transfer.rate(start), None);

    let step = transfer.step(b"device output", ms(100)).unwrap();
    assert_eq!(step.out.len(), 96);
    assert_eq!(step.protocol, 13..13);
    assert_eq!(transfer.percent(), 48);
    assert_eq!(transfer.rate(ms(100)), Some(960.0));
    // 104 bytes left at 960 a second
    assert_eq!(transfer.eta(ms(100)).unwrap().as_millis(), 108);

    assert_eq!(transfer.step(b"", ms(200)).unwrap().out.len(), 96);
    assert_eq!(transfer.step(b"", ms(300)).unwrap().out.len(), 8);
    assert_eq!(transfer.outcome(), None);
    assert_eq!(transfer.step(b"", ms(400)).unwrap().out, b"");
    assert_eq!(transfer.outcome(), Some(Ok(())));
    assert_eq!(transfer.percent(), 100);
    assert_eq!(transfer.cancel(), b"");
}

#[test]
fn empty_file() {
    let start = Instant::now();
    let mut transfer = raw(b"", start);
    assert_eq!(transfer.percent(), 100);
    transfer.step(b"", start + Duration::from_secs(1)).unwrap();
    assert_eq!(transfer.outcome(), Some(Ok(())));
}

/// A protocol transfer: no rate while the receiver isn't there yet, then counted from its
/// first NAK; the progress is what the receiver acknowledged.
#[test]
fn modem_send() {
    let start = Instant::now();
    let secs = |secs| start + Duration::from_secs(secs);
    let mut transfer = modem(&[b'x'; 200], start);
    assert_eq!(transfer.step(b"", secs(5)).unwrap().out, b"");
    assert_eq!(transfer.rate(secs(5)), None);
    assert_eq!(transfer.eta(secs(5)), None);

    let step = transfer.step(b"ready\r\n\x15", secs(10)).unwrap();
    assert_eq!(step.out.len(), 132);
    assert_eq!(step.protocol, 7..8);
    assert_eq!(transfer.percent(), 0);

    transfer.step(b"\x06", secs(11)).unwrap();
    assert_eq!(transfer.sent(), 128);
    assert_eq!(transfer.rate(secs(11)), Some(128.0));
    assert_eq!(
        transfer.eta(secs(11)),
        Some(Duration::from_secs_f64(72.0 / 128.0))
    );

    transfer.step(b"\x06", secs(12)).unwrap();
    assert_eq!(transfer.percent(), 100);
    assert_eq!(transfer.outcome(), None);
    transfer.step(b"\x06", secs(12)).unwrap();
    assert_eq!(transfer.outcome(), Some(Ok(())));
}

/// Cancelling a protocol transfer gives the CANs that stop the receiver.
#[test]
fn modem_cancel() {
    let start = Instant::now();
    let mut transfer = modem(b"abc", start);
    transfer.step(b"C", start).unwrap();
    assert_eq!(transfer.cancel(), [0x18; 5]);
    assert_eq!(transfer.outcome(), Some(Err(modem::Failure::Cancelled)));
}
