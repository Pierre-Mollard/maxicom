use super::session::{Session, SessionText};
use chrono::format::{Item, Parsed, StrftimeItems, parse_and_remainder};
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// How much of a past session's raw bytes is restored to the terminal (MAX-148)
pub const RESTORE_RAW_TAIL: u64 = 1 << 20;
const TAIL_CHUNK: u64 = 64 * 1024;

/// A line of a session's text read back: its time when it has one, whether it was sent, and
/// its text without them
#[derive(Debug, PartialEq)]
pub struct SessionTextLine<'a> {
    pub time: Option<DateTime<Local>>,
    pub tx: bool,
    pub text: &'a str,
}

/// Reads a session's text lines back with the format they were written with (MAX-148). A time
/// that doesn't parse leaves the whole line as text, with no time. A format without a date
/// takes the session's start date, and the next day when the time goes back by more than 12
/// hours (midnight: a sent line written before a pending received one goes back a little)
pub struct SessionReader {
    timestamp: Option<Vec<Item<'static>>>,
    tx_mark: Option<String>,
    date: NaiveDate,
    last: Option<NaiveDateTime>,
}

impl SessionReader {
    pub fn new(text: &SessionText, started: DateTime<Local>) -> Self {
        Self {
            timestamp: text
                .timestamp
                .as_deref()
                .and_then(|format| StrftimeItems::new(format).parse_to_owned().ok()),
            tx_mark: text.tx_mark.clone().filter(|mark| !mark.is_empty()),
            date: started.date_naive(),
            last: None,
        }
    }

    /// One line, without its EOL
    pub fn parse<'a>(&mut self, line: &'a str) -> SessionTextLine<'a> {
        let (time, rest) = match self.parse_time(line) {
            Some((time, rest)) => (Some(time), rest),
            None => (None, line),
        };
        let sent = self
            .tx_mark
            .as_deref()
            .and_then(|mark| rest.strip_prefix(mark));
        SessionTextLine {
            time,
            tx: sent.is_some(),
            text: sent.unwrap_or(rest),
        }
    }

    /// The time at the start of `line` and the rest after its space, as `build_line` writes
    fn parse_time<'a>(&mut self, line: &'a str) -> Option<(DateTime<Local>, &'a str)> {
        let items = self.timestamp.as_ref()?;
        let mut parsed = Parsed::new();
        let rest = parse_and_remainder(&mut parsed, line, items.iter()).ok()?;
        let rest = rest.strip_prefix(' ')?;
        let time = match parsed.to_naive_datetime_with_offset(0) {
            Ok(time) => time,
            Err(_) => {
                let mut time = self.date.and_time(parsed.to_naive_time().ok()?);
                if self
                    .last
                    .is_some_and(|last| last - time > chrono::TimeDelta::hours(12))
                {
                    self.date = self.date.succ_opt()?;
                    time = self.date.and_time(time.time());
                }
                time
            }
        };
        self.last = Some(time);
        Some((Local.from_local_datetime(&time).earliest()?, rest))
    }
}

/// The end of the file at `path`, at most `max` bytes. When the file is longer, it starts after
/// the first `\n` past the cut, so no line, escape sequence or UTF-8 char is cut
pub fn read_tail_bytes(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(max);
    file.seek(SeekFrom::Start(start))?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail)?;
    if start > 0 {
        let first_line = memchr::memchr(b'\n', &tail).map_or(tail.len(), |i| i + 1);
        tail.drain(..first_line);
    }
    Ok(tail)
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
    //! Reading a session back: its text lines, the file tails and the separator.

    use super::*;
    use crate::capture::tests::{device, record, temp_dir};
    use crate::capture::{Recorder, find_last_session};
    use crate::configs::TextEncoding;
    use std::fs;

    fn reader(timestamp: Option<&str>, tx_mark: Option<&str>) -> SessionReader {
        let text = SessionText {
            timestamp: timestamp.map(str::to_string),
            tx_mark: tx_mark.map(str::to_string),
        };
        let started = Local.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        SessionReader::new(&text, started)
    }

    fn at(day: u32, h: u32, m: u32, s: u32, ms: u32) -> Option<DateTime<Local>> {
        let date = NaiveDate::from_ymd_opt(2026, 1, day)?;
        Local
            .from_local_datetime(&date.and_hms_milli_opt(h, m, s, ms)?)
            .earliest()
    }

    /// MAX-148: a session's text line is read back with its format: the time (dated from the
    /// session start, the next day past midnight, not on a sent line slightly earlier), the mark,
    /// and a line whose time doesn't parse kept whole.
    #[test]
    fn session_lines_read_back() {
        let mut r = reader(Some("[%H:%M:%S%.3f]"), Some("> "));
        let line = |time, tx, text| SessionTextLine { time, tx, text };
        assert_eq!(
            r.parse("[23:59:59.500] rx"),
            line(at(1, 23, 59, 59, 500), false, "rx")
        );
        assert_eq!(
            r.parse("[23:59:59.400] > cmd"),
            line(at(1, 23, 59, 59, 400), true, "cmd")
        );
        assert_eq!(
            r.parse("[00:00:01.000] next day"),
            line(at(2, 0, 0, 1, 0), false, "next day")
        );
        assert_eq!(
            r.parse("[00:00:02.000] "),
            line(at(2, 0, 0, 2, 0), false, "")
        );
        assert_eq!(r.parse("garbage > x"), line(None, false, "garbage > x"));
        assert_eq!(r.parse("> no time"), line(None, true, "no time"));

        let mut dated = reader(Some("%Y-%m-%d %H:%M:%S"), None);
        assert_eq!(
            dated.parse("2026-01-05 10:00:00 > a"),
            line(at(5, 10, 0, 0, 0), false, "> a")
        );
    }

    /// MAX-148: without a timestamp or a mark in the meta, the line is all text, received.
    #[test]
    fn session_lines_without_time_or_mark() {
        let line = |text| SessionTextLine {
            time: None,
            tx: false,
            text,
        };
        for (timestamp, mark) in [(None, None), (None, Some(""))] {
            let mut r = reader(timestamp, mark);
            assert_eq!(r.parse("[10:00:00.000] > a"), line("[10:00:00.000] > a"));
        }
        let mut r = reader(Some("[%H:%M:%S%.3f]"), Some(""));
        assert_eq!(r.parse("[10:00:00.000] > a").text, "> a");
    }

    /// MAX-148: what the session backup writes, with each combination of timestamp and mark, reads
    /// back as what was received and sent.
    /// How: a session backup per combination, "rx" received and "tx" sent, its text parsed back.
    #[test]
    fn session_text_round_trip() {
        let dir = temp_dir("session_round_trip");
        for (i, (timestamp, tx_mark)) in [
            (Some("[%H:%M:%S%.3f]"), Some("> ")),
            (Some("%Y-%m-%dT%H:%M:%S"), Some("TX: ")),
            (Some("[%H:%M:%S%.3f]"), None),
            (None, Some("> ")),
            (None, None),
        ]
        .into_iter()
        .enumerate()
        {
            let text = SessionText {
                timestamp: timestamp.map(str::to_string),
                tx_mark: tx_mark.map(str::to_string),
            };
            let port = format!("p{i}");
            let mut recorder = Recorder::default();
            recorder
                .start_session(&dir, &device(&port), &text, 20)
                .unwrap();
            record(&mut recorder, b"rx\n");
            recorder.record_tx(TextEncoding::Utf8, None, None, b"tx\n");
            drop(recorder);
            let session = find_last_session(&dir, &device(&port), None).unwrap();

            let log = fs::read_to_string(session.text_path().unwrap()).unwrap();
            let meta = session.meta.as_ref().unwrap();
            let mut r = SessionReader::new(&meta.text, session.started.unwrap());
            let lines: Vec<_> = log.lines().map(|l| r.parse(l)).collect();
            let has_time = timestamp.is_some();
            assert_eq!(lines[0].text, "rx", "{log:?}");
            assert_eq!(lines[0].time.is_some(), has_time, "{log:?}");
            if tx_mark.is_some() {
                assert_eq!((lines[1].tx, lines[1].text), (true, "tx"), "{log:?}");
                assert_eq!(lines[1].time.is_some(), has_time, "{log:?}");
            } else {
                assert_eq!(lines.len(), 1, "{log:?}");
            }
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-148: the raw tail is the whole file when it fits, else it starts after the first
    /// line ending past the cut.
    #[test]
    fn raw_tail_starts_on_a_line() {
        let dir = temp_dir("raw_tail");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.raw");
        fs::write(&path, b"aaaa\r\nbb\x1b[31mcc\r\ndd").unwrap();
        assert_eq!(
            read_tail_bytes(&path, 100).unwrap(),
            b"aaaa\r\nbb\x1b[31mcc\r\ndd"
        );
        // the cut falls inside "bb\x1b[31mcc": that line is dropped whole
        assert_eq!(read_tail_bytes(&path, 10).unwrap(), b"dd");
        assert_eq!(read_tail_bytes(&path, 2).unwrap(), b"");
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
