//! The received bytes kept (MAX-252, MAX-290).

use super::*;

/// The bytes of `range`, both slices joined
fn bytes_of(stream: &RxStream, range: Range<u64>) -> Vec<u8> {
    let (front, back) = stream.slices(range);
    [front, back].concat()
}

/// Reads go in order, each one's first byte at the next offset; an empty read adds nothing.
/// How: "abc", "", "de".
#[test]
fn reads_get_their_offsets() {
    let mut stream = RxStream::new(1024);
    assert_eq!(stream.push(b"abc"), 0);
    assert_eq!(stream.push(b""), 3);
    assert_eq!(stream.push(b"de"), 3);
    assert_eq!((stream.start(), stream.end()), (0, 5));
    assert_eq!(bytes_of(&stream, 1..4), b"bcd");
}

/// Past the ceiling the oldest bytes go, the offsets of the others don't change.
/// How: a 64 KiB stream (the least), 40 KiB of "a" then 40 KiB of "b".
#[test]
fn ceiling_drops_the_oldest_bytes() {
    let mut stream = RxStream::new(0);
    stream.push(&[b'a'; 40 * 1024]);
    let second = stream.push(&[b'b'; 40 * 1024]);
    assert_eq!(second, 40 * 1024);
    assert_eq!(stream.end(), 80 * 1024);
    assert_eq!(stream.start(), 16 * 1024);
    assert_eq!(bytes_of(&stream, second..second + 2), b"bb");
    assert_eq!(bytes_of(&stream, 0..stream.start() + 1), b"a");
}

/// The bytes come back right when the ring wraps, a range across the wrap included.
/// How: a 64 KiB stream (the least) fed 7 byte reads past its ceiling, so the ring wraps; read
/// every range of 3 bytes kept.
#[test]
fn slices_across_the_wrap() {
    let mut stream = RxStream::new(0);
    let mut all = Vec::new();
    let mut wrapped = false;
    for round in 0..40_000u32 {
        let read: Vec<u8> = (0..7).map(|i| (round * 7 + i) as u8).collect();
        stream.push(&read);
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
    let mut stream = RxStream::new(1024);
    stream.push(b"abc");
    stream.clear();
    assert_eq!((stream.start(), stream.end()), (3, 3));
    assert_eq!(stream.push(b"d"), 3);
}
