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
    assert_eq!(
        transfer.outcome(),
        Some(Err(TransferFailure::Send(SendFailure::Cancelled)))
    );
}

/// A download from `ModemSender`, the two passing each other what they wrote: the file in the
/// folder, the progress counted on the current file.
fn download_from_sender(protocol: Protocol, content: &[u8], folder: &Path) -> FileTransfer {
    let start = Instant::now();
    let mut sender = ModemSender::new(
        Box::new(io::Cursor::new(content.to_vec())),
        protocol,
        "fw.bin".to_string(),
        content.len() as u64,
        RATE,
        start,
    );
    let mut transfer = FileTransfer::download(
        protocol,
        folder.to_path_buf(),
        Some("dump.bin".to_string()),
        start,
    );
    assert!(transfer.waiting() && transfer.is_download() && transfer.takes_rx());
    let mut to_receiver = Vec::new();
    let mut owned = false;
    for _ in 0..1000 {
        let out = transfer.step(&to_receiver, start).unwrap().out;
        owned |= transfer.owns_link();
        if transfer.outcome().is_some() {
            break;
        }
        to_receiver = sender.step(&out, start).unwrap().out;
    }
    // from its first packet until the end
    assert!(owned && !transfer.owns_link());
    transfer
}

/// YMODEM: the file under its own name, its size known; XMODEM: under the name given, the
/// padding kept, no size (no percent).
#[test]
fn download() {
    let folder = std::env::temp_dir().join(format!(
        "maxicom_test_{}_transfer_download",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&folder);
    let content: Vec<u8> = (0..300u32).map(|i| i as u8).collect();

    let mut transfer = download_from_sender(Protocol::Ymodem, &content, &folder);
    assert_eq!(transfer.outcome(), Some(Ok(())));
    assert_eq!(transfer.take_received(), [(folder.join("fw.bin"), 300)]);
    assert_eq!(std::fs::read(folder.join("fw.bin")).unwrap(), content);
    assert_eq!(transfer.received_count(), 1);
    assert_eq!(transfer.folder(), Some(folder.as_path()));

    let mut transfer = download_from_sender(Protocol::Xmodem, &content, &folder);
    assert_eq!(transfer.outcome(), Some(Ok(())));
    assert_eq!(transfer.take_received(), [(folder.join("dump.bin"), 384)]);
    assert_eq!((transfer.size(), transfer.percent()), (None, 0));
    let written = std::fs::read(folder.join("dump.bin")).unwrap();
    assert_eq!(written[..300], content[..]);
    let _ = std::fs::remove_dir_all(&folder);
}
