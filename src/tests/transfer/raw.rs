use super::*;

/// At `baudrate` in 8N1: 10 bits a byte
fn transfer(content: &[u8], baudrate: u32, line_delay_ms: u64, start: Instant) -> RawSender {
    RawSender::new(
        Box::new(io::Cursor::new(content.to_vec())),
        f64::from(baudrate) / 10.0,
        Duration::from_millis(line_delay_ms),
        start,
    )
}

fn chunk(transfer: &mut RawSender, at: Instant) -> Option<Vec<u8>> {
    transfer.next_chunk(at).unwrap()
}

/// At 9600 baud (960 bytes a second) a chunk is what drained since the last one, at most
/// 100 ms of it after a pause; the end of the file ends the transfer.
#[test]
fn paced_to_the_baud_rate() {
    let start = Instant::now();
    let ms = |ms| start + Duration::from_millis(ms);
    let mut transfer = transfer(&[b'x'; 200], 9600, 0, start);
    assert_eq!(chunk(&mut transfer, start), Some(vec![]));
    assert_eq!(chunk(&mut transfer, ms(50)).unwrap().len(), 48);
    assert_eq!(chunk(&mut transfer, ms(51)), Some(vec![]));
    assert_eq!(chunk(&mut transfer, ms(10_000)).unwrap().len(), 96);
    assert_eq!(transfer.sent(), 144);
    assert_eq!(chunk(&mut transfer, ms(20_000)).unwrap().len(), 56);
    assert_eq!(transfer.sent(), 200);
    assert_eq!(chunk(&mut transfer, ms(30_000)), None);
}

/// With a line delay, a chunk ends after a `\n` and the next waits the delay.
#[test]
fn line_delay() {
    let start = Instant::now();
    let ms = |ms| start + Duration::from_millis(ms);
    let mut transfer = transfer(b"ab\ncd\nef", 115_200, 100, start);
    assert_eq!(chunk(&mut transfer, ms(50)), Some(b"ab\n".to_vec()));
    assert_eq!(chunk(&mut transfer, ms(149)), Some(vec![]));
    assert_eq!(chunk(&mut transfer, ms(150)), Some(vec![]));
    assert_eq!(chunk(&mut transfer, ms(200)), Some(b"cd\n".to_vec()));
    assert_eq!(chunk(&mut transfer, ms(400)), Some(b"ef".to_vec()));
    assert_eq!(chunk(&mut transfer, ms(500)), None);
}

#[test]
fn empty_file() {
    let start = Instant::now();
    let mut transfer = transfer(b"", 9600, 0, start);
    assert_eq!(transfer.sent(), 0);
    assert_eq!(chunk(&mut transfer, start + Duration::from_secs(1)), None);
}
