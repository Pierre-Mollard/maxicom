//! The received bytes kept for the terminal raw panel (MAX-252).

use super::*;

/// The bytes of `range`, both slices joined
fn bytes_of(stream: &RxStream, range: Range<u64>) -> Vec<u8> {
    let (front, back) = stream.slices(range);
    [front, back].concat()
}

fn time(second: u32) -> Option<DateTime<Local>> {
    use chrono::TimeZone;
    Local.with_ymd_and_hms(2026, 11, 16, 12, 0, second).single()
}

/// Reads go in order, each one's offset and time kept; an empty read adds nothing.
/// How: "abc" at 1 s, "" at 2 s, "de" at 3 s.
#[test]
fn reads_get_their_offsets_and_times() {
    let mut stream = RxStream::new(100);
    assert_eq!(stream.push(b"abc", time(1)), 0);
    assert_eq!(stream.push(b"", time(2)), 3);
    assert_eq!(stream.push(b"de", time(3)), 3);
    assert_eq!((stream.start(), stream.end()), (0, 5));
    assert_eq!(bytes_of(&stream, 1..4), b"bcd");
    let reads: Vec<Read> = stream.reads().iter().copied().collect();
    assert_eq!(
        reads,
        [
            Read {
                offset: 0,
                time: time(1)
            },
            Read {
                offset: 3,
                time: time(3)
            },
        ]
    );
}

/// Past the ceiling the oldest bytes go, the offsets of the others don't change, and the read
/// cut in two starts at the first byte kept.
/// How: a 64 KiB stream (the least), 40 KiB of "a" then 40 KiB of "b".
#[test]
fn ceiling_drops_the_oldest_bytes() {
    let mut stream = RxStream::new(1);
    stream.push(&[b'a'; 40 * 1024], time(1));
    let second = stream.push(&[b'b'; 40 * 1024], time(2));
    assert_eq!(second, 40 * 1024);
    assert_eq!(stream.end(), 80 * 1024);
    assert_eq!(stream.start(), 16 * 1024);
    assert_eq!(bytes_of(&stream, second..second + 2), b"bb");
    assert_eq!(bytes_of(&stream, 0..stream.start() + 1), b"a");
    let reads: Vec<u64> = stream.reads().iter().map(|read| read.offset).collect();
    assert_eq!(reads, [16 * 1024, 40 * 1024]);
}

/// The bytes come back right when the ring wraps, a range across the wrap included.
/// How: a 64 KiB stream (the least) fed 7 byte reads past its ceiling, so the ring wraps; read
/// every range of 3 bytes kept.
#[test]
fn slices_across_the_wrap() {
    let mut stream = RxStream::new(1);
    let mut all = Vec::new();
    let mut wrapped = false;
    for round in 0..40_000u32 {
        let read: Vec<u8> = (0..7).map(|i| (round * 7 + i) as u8).collect();
        stream.push(&read, None);
        all.extend_from_slice(&read);
        wrapped |= !stream.bytes.as_slices().1.is_empty();
    }
    assert!(wrapped);
    assert!(stream.start() > 0);
    for from in stream.start()..stream.end() - 3 {
        let expected = &all[from as usize..from as usize + 3];
        assert_eq!(bytes_of(&stream, from..from + 3), expected, "from {from}");
    }
}

/// Clear keeps counting from where the stream was: an offset is never given twice.
/// How: "abc", clear, "d" is at 3.
#[test]
fn clear_keeps_the_offsets() {
    let mut stream = RxStream::new(100);
    stream.push(b"abc", None);
    stream.clear();
    assert_eq!((stream.start(), stream.end()), (3, 3));
    assert_eq!(stream.push(b"d", None), 3);
}
