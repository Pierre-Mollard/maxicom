use super::session::Session;
use chrono::{DateTime, Local};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// How much of a past session's raw bytes is restored to the terminal (MAX-148)
pub const RESTORE_RAW_TAIL: u64 = 1 << 20;
const TAIL_CHUNK: u64 = 64 * 1024;

/// Received bytes and when they were received, when that is known (MAX-175)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimedChunk<'a> {
    pub bytes: &'a [u8],
    pub time: Option<DateTime<Local>>,
}

/// The end of the file at `path`, at most `max` bytes, and where it starts in the file. When
/// the file is longer, it starts after the first `\n` past the cut, so no line, escape sequence
/// or UTF-8 char is cut
pub fn read_tail_bytes(path: &Path, max: u64) -> io::Result<(u64, Vec<u8>)> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut start = len.saturating_sub(max);
    file.seek(SeekFrom::Start(start))?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail)?;
    if start > 0 {
        let first_line = memchr::memchr(b'\n', &tail).map_or(tail.len(), |i| i + 1);
        tail.drain(..first_line);
        start += first_line as u64;
    }
    Ok((start, tail))
}

/// A line of a session's `.times` file (MAX-175): where a received chunk starts in its `.raw`,
/// and when it was received, in milliseconds since the Unix epoch
pub fn times_line(offset: u64, time: DateTime<Local>) -> String {
    format!("{offset} {}\n", time.timestamp_millis())
}

fn parse_times_line(line: &[u8]) -> Option<(u64, DateTime<Local>)> {
    let line = std::str::from_utf8(line).ok()?;
    let (offset, millis) = line.trim_end().split_once(' ')?;
    let time = DateTime::from_timestamp_millis(millis.parse().ok()?)?;
    Some((offset.parse().ok()?, time.with_timezone(&Local)))
}

/// The times of a session's chunks from raw offset `from` on, read back from the end of its
/// `.times` file at `path`, the chunk holding `from` first (MAX-175). Read backwards by chunks,
/// so a long session costs only its end; a line that doesn't parse is left out
pub fn read_times(path: &Path, from: u64) -> io::Result<Vec<(u64, DateTime<Local>)>> {
    read_times_by(path, from, TAIL_CHUNK)
}

fn read_times_by(path: &Path, from: u64, chunk: u64) -> io::Result<Vec<(u64, DateTime<Local>)>> {
    let mut file = File::open(path)?;
    let mut start = file.metadata()?.len();
    let mut tail: Vec<u8> = Vec::new();
    loop {
        // the first whole line: the file's first, or the one after the first `\n`
        let whole = match start {
            0 => Some(&tail[..]),
            _ => memchr::memchr(b'\n', &tail).map(|i| &tail[i + 1..]),
        };
        let reached = whole.and_then(|lines| lines.split(|&b| b == b'\n').next());
        if start == 0
            || reached
                .and_then(parse_times_line)
                .is_some_and(|(at, _)| at <= from)
        {
            let lines = whole.unwrap_or_default();
            let times: Vec<_> = lines
                .split(|&b| b == b'\n')
                .filter_map(parse_times_line)
                .collect();
            let first = times.iter().rposition(|&(at, _)| at <= from).unwrap_or(0);
            return Ok(times[first..].to_vec());
        }
        let read_from = start.saturating_sub(chunk);
        let mut block = vec![0; (start - read_from) as usize];
        file.seek(SeekFrom::Start(read_from))?;
        file.read_exact(&mut block)?;
        tail.splice(0..0, block);
        start = read_from;
    }
}

/// The raw bytes read from offset `start`, cut where each received chunk of `times` starts,
/// each with its time (MAX-175). Bytes before the first time known (none at all without a
/// `.times`) have none
pub fn timed_chunks<'a>(
    start: u64,
    raw: &'a [u8],
    times: &[(u64, DateTime<Local>)],
) -> Vec<TimedChunk<'a>> {
    let end = start + raw.len() as u64;
    let at = |offset: u64| (offset.clamp(start, end) - start) as usize;
    let cuts: Vec<_> = std::iter::once((0, None))
        .chain(times.iter().map(|&(offset, time)| (at(offset), Some(time))))
        .collect();
    cuts.iter()
        .enumerate()
        .filter_map(|(i, &(from, time))| {
            let to = cuts.get(i + 1).map_or(raw.len(), |&(next, _)| next);
            (to > from).then(|| TimedChunk {
                bytes: &raw[from..to],
                time,
            })
        })
        .collect()
}

/// The last `max` lines of the file at `path`, read backwards by chunks, so a big file costs
/// only its end. A last line without `\n` counts
pub fn read_tail_lines(path: &Path, max: usize) -> io::Result<Vec<u8>> {
    read_tail_lines_by(path, max, TAIL_CHUNK)
}

fn read_tail_lines_by(path: &Path, max: usize, chunk: u64) -> io::Result<Vec<u8>> {
    if max == 0 {
        return Ok(Vec::new());
    }
    let mut file = File::open(path)?;
    let mut start = file.metadata()?.len();
    let mut tail: Vec<u8> = Vec::new();
    loop {
        // the `\n` ending the last line doesn't start one
        let body = tail.strip_suffix(b"\n").unwrap_or(&tail);
        // the `\n` before the `max`-th line from the end
        if let Some(cut) = memchr::memrchr_iter(b'\n', body).nth(max - 1) {
            tail.drain(..=cut);
            return Ok(tail);
        }
        if start == 0 {
            return Ok(tail);
        }
        let read_from = start.saturating_sub(chunk);
        let mut block = vec![0; (start - read_from) as usize];
        file.seek(SeekFrom::Start(read_from))?;
        file.read_exact(&mut block)?;
        tail.splice(0..0, block);
        start = read_from;
    }
}

/// The line that marks where a restored session ends and the live data starts
pub fn restore_separator(session: &Session) -> String {
    format!(
        "── restored: {}, {} ──",
        session.port_tag(),
        session.time_span()
    )
}

#[cfg(test)]
mod tests {
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
}
