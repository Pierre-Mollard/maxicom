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
    format!("── restored: {} ──", separator_subject(session))
}

/// The line that ends a session in a group replay (MAX-202)
pub fn group_separator(session: &Session) -> String {
    format!("── end of session: {} ──", separator_subject(session))
}

/// A session in a separator: its name when it has one (MAX-213), its port and its times
fn separator_subject(session: &Session) -> String {
    let name = session
        .name()
        .map_or(String::new(), |name| format!("{name}, "));
    format!("{name}{}, {}", session.port_tag(), session.time_span())
}

#[cfg(test)]
#[path = "../tests/capture/restore.rs"]
mod tests;
