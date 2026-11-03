
//! Reading a session back: the file tails and the separator.

use super::*;
use crate::capture::tests::temp_dir;
use chrono::TimeZone;
use std::fs;

fn ms(millis: i64) -> DateTime<Local> {
    DateTime::from_timestamp_millis(millis).unwrap().into()
}

/// MAX-175: the raw bytes are cut where each chunk's time starts; the bytes before the
/// first time have none, a tail starting mid-chunk takes that chunk's time, a time past the
/// end (a crash) gives nothing.
/// How: 10 bytes read from offset 100, times at 98, 104, 107 and 200; then times from 103
/// only; then no times.
#[test]
fn raw_cut_into_timed_chunks() {
    let raw = b"0123456789";
    let chunk = |bytes: &'static [u8], time| TimedChunk { bytes, time };
    let times = [(98, ms(1)), (104, ms(2)), (107, ms(3)), (200, ms(4))];
    assert_eq!(
        timed_chunks(100, raw, &times),
        [
            chunk(b"0123", Some(ms(1))),
            chunk(b"456", Some(ms(2))),
            chunk(b"789", Some(ms(3))),
        ]
    );
    assert_eq!(
        timed_chunks(100, raw, &[(103, ms(5))]),
        [chunk(b"012", None), chunk(b"3456789", Some(ms(5)))]
    );
    assert_eq!(timed_chunks(100, raw, &[]), [chunk(raw, None)]);
}

/// MAX-175: the `.times` are read back from the end, from the chunk holding the given raw
/// offset; a line that doesn't parse is left out.
/// How: 6 times written as the session writer does and a broken line, read from offsets 25
/// and 0 by 16-byte blocks (several blocks back), and past the last one.
#[test]
fn times_read_back_from_an_offset() {
    let dir = temp_dir("times_tail");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("x.times");
    let lines: Vec<String> = (0..6).map(|i| times_line(i * 10, ms(i as i64))).collect();
    let text = format!("{}broken\n{}", lines[..3].concat(), lines[3..].concat());
    fs::write(&path, text).unwrap();
    let offsets = |from| -> Vec<u64> {
        let times = read_times_by(&path, from, 16).unwrap();
        times.iter().map(|&(offset, _)| offset).collect()
    };
    assert_eq!(offsets(25), [20, 30, 40, 50]);
    assert_eq!(offsets(0), [0, 10, 20, 30, 40, 50]);
    assert_eq!(offsets(99), [50]);
    let times = read_times(&path, 30).unwrap();
    assert_eq!(times[0], (30, ms(3)));
    let _ = fs::remove_dir_all(&dir);
}

/// MAX-148: the raw tail is the whole file when it fits, else it starts after the first
/// line ending past the cut. MAX-175: with where it starts in the file.
#[test]
fn raw_tail_starts_on_a_line() {
    let dir = temp_dir("raw_tail");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("x.raw");
    let raw = b"aaaa\r\nbb\x1b[31mcc\r\ndd";
    fs::write(&path, raw).unwrap();
    let tail = |max| read_tail_bytes(&path, max).unwrap();
    assert_eq!(tail(100), (0, raw.to_vec()));
    // the cut falls inside "bb\x1b[31mcc": that line is dropped whole
    assert_eq!(tail(10), (17, b"dd".to_vec()));
    assert_eq!(tail(2), (19, Vec::new()));
    let _ = fs::remove_dir_all(&dir);
}

/// MAX-148: the text tail is the last lines, read backwards across chunks, with or without
/// a last line ending.
/// How: chunks of 3 bytes so a line spans several.
#[test]
fn text_tail_is_the_last_lines() {
    let dir = temp_dir("text_tail");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("x.log");
    fs::write(&path, b"one\ntwo\nthree\nfour\n").unwrap();
    let tail = |max| read_tail_lines_by(&path, max, 3).unwrap();
    assert_eq!(tail(2), b"three\nfour\n");
    assert_eq!(tail(4), b"one\ntwo\nthree\nfour\n");
    assert_eq!(tail(10), b"one\ntwo\nthree\nfour\n");
    assert_eq!(tail(0), b"");
    fs::write(&path, b"one\ntwo\nlast").unwrap();
    assert_eq!(tail(1), b"last");
    assert_eq!(tail(2), b"two\nlast");
    let _ = fs::remove_dir_all(&dir);
}

/// MAX-148: the separator names the port and the session's times, the end without its
/// date on the same day.
#[test]
fn restore_separator_names_port_and_times() {
    let day = |d, h| Local.with_ymd_and_hms(2026, 1, d, h, 15, 0).unwrap();
    let mut session = Session {
        stem: "20260101_101500_ttyUSB0".to_string(),
        folder: std::path::PathBuf::new(),
        files: Vec::new(),
        meta: None,
        started: Some(day(1, 10)),
        ended: Some(day(1, 16)),
        raw_len: 1,
        size: 1,
        running: false,
    };
    assert_eq!(
        restore_separator(&session),
        "── restored: ttyUSB0, 2026-01-01 10:15 → 16:15 ──"
    );
    session.ended = Some(day(2, 9));
    assert_eq!(
        restore_separator(&session),
        "── restored: ttyUSB0, 2026-01-01 10:15 → 2026-01-02 09:15 ──"
    );
}
