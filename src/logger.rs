use crate::app::types::RxLineSplitter;
use crate::configs::{RxNewlineMode, TextEncoding};
use crate::serial::{CharSize, Parity, SerialConfig, StopBits};
use chrono::format::{Item, Parsed, StrftimeItems, parse_and_remainder};
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

const FLUSH_INTERVAL: Duration = Duration::from_millis(250);
const SESSION_META_VERSION: u32 = 1;
/// How much of a past session's raw bytes is restored to the terminal (MAX-148)
pub const RESTORE_RAW_TAIL: u64 = 1 << 20;
const TAIL_CHUNK: u64 = 64 * 1024;

/// Buffered log file: writes are batched in memory and flushed at most every `FLUSH_INTERVAL`
/// (on write or on `flush_if_due` from the main loop), on stop and on drop
struct LogFile {
    writer: BufWriter<File>,
    last_flush: Instant,
}

impl LogFile {
    /// Appends to the file, creating it and its missing folders
    fn open(path: &Path) -> io::Result<Self> {
        create_parent_dirs(path)?;
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            writer: BufWriter::new(file),
            last_flush: Instant::now(),
        })
    }

    fn flush_if_due(&mut self) -> io::Result<()> {
        if self.last_flush.elapsed() >= FLUSH_INTERVAL {
            self.flush()?;
        }
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.last_flush = Instant::now();
        self.writer.flush()
    }
}

/// What a recording saves: the received lines as text, or the received bytes as is
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Text,
    Raw,
}

impl Stream {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Raw => "raw",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Text => Self::Raw,
            Self::Raw => Self::Text,
        }
    }
}

/// What a screenshot saves of the terminal screen: its text, or its text with the escape
/// sequences that draw its colors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotFormat {
    Text,
    Ansi,
}

impl ScreenshotFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Ansi => "ansi",
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            Self::Text => "txt",
            Self::Ansi => "ans",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Text => Self::Ansi,
            Self::Ansi => Self::Text,
        }
    }
}

/// The terminal screen taken in both formats, so the one to save is chosen after (MAX-147)
#[derive(Debug, Clone, Default)]
pub struct Screenshot {
    pub text: String,
    pub ansi: String,
}

impl Screenshot {
    pub fn into_contents(self, format: ScreenshotFormat) -> String {
        match format {
            ScreenshotFormat::Text => self.text,
            ScreenshotFormat::Ansi => self.ansi,
        }
    }
}

/// The session backup (MAX-147): everything received since startup, as text and raw, in the
/// cache folder. It runs apart from the user's recordings, which can start with its content
struct Journal {
    text: LogFile,
    raw: LogFile,
    text_path: PathBuf,
    raw_path: PathBuf,
    timestamp: Option<TimestampFormat>,
    tx_mark: Option<String>,
}

/// How a journal's text lines are written, taken from the capture settings when it starts and
/// kept in its meta file, so it can be read back (MAX-148): the timestamp format when it is
/// on, and the mark before sent lines when they are recorded. A reload changes the user
/// recordings only, so one journal never mixes two formats
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalText {
    pub timestamp: Option<String>,
    pub tx_mark: Option<String>,
}

/// What a session was recorded on (MAX-148): a session restores only on the same machine,
/// port, line settings and encoding, as bytes from another baud or encoding are garbage. Flow
/// control doesn't change the bytes, so it isn't part of it
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionDevice {
    pub host: String,
    pub port: String,
    pub baud: u32,
    pub char_size: CharSize,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub encoding: TextEncoding,
}

impl SessionDevice {
    pub fn new(serial: &SerialConfig, encoding: TextEncoding) -> Self {
        Self {
            host: hostname(),
            port: serial.port.clone(),
            baud: serial.baudrate,
            char_size: serial.char_size,
            parity: serial.parity,
            stop_bits: serial.stop_bits,
            encoding,
        }
    }
}

/// The `<stem>.meta` file of a session, written when it starts. A file of another `version`
/// never restores
#[derive(Debug, Serialize, Deserialize)]
struct SessionMeta {
    version: u32,
    started: i64,
    device: SessionDevice,
    text: JournalText,
}

/// A past session that can be restored: its files, when it started, and when it last
/// received (the raw file's time, right even after a crash)
#[derive(Debug, Clone, PartialEq)]
pub struct PastSession {
    pub text_path: PathBuf,
    pub raw_path: PathBuf,
    pub started: DateTime<Local>,
    pub ended: DateTime<Local>,
    pub raw_len: u64,
    pub text: JournalText,
}

/// Recordings of what is received, fed whatever the mode shows: text and raw are started and
/// stopped apart, each to its own file (MAX-147). The lines are cut even when nothing records
/// text, so a recording started mid-line still gets that whole line
#[derive(Default)]
pub struct Recorder {
    text: Option<LogFile>,
    raw: Option<LogFile>,
    journal: Option<Journal>,
    lines: RxLineSplitter,
    timestamp: Option<TimestampFormat>,
    line_buf: Vec<u8>,
}

/// `timestamp_format` parsed once, and again only when it changes (on reload), instead of on
/// every line
struct TimestampFormat {
    format: String,
    items: Vec<Item<'static>>,
}

impl Recorder {
    fn slot(&mut self, stream: Stream) -> &mut Option<LogFile> {
        match stream {
            Stream::Text => &mut self.text,
            Stream::Raw => &mut self.raw,
        }
    }

    /// Starts recording `stream` to `path`, first copying in what the session backup holds
    /// when `include_session`
    pub fn start(
        &mut self,
        stream: Stream,
        path: PathBuf,
        include_session: bool,
    ) -> io::Result<()> {
        let mut file = LogFile::open(&path)?;
        if include_session && let Some(journal) = &mut self.journal {
            let (backup, backup_path) = match stream {
                Stream::Text => (&mut journal.text, &journal.text_path),
                Stream::Raw => (&mut journal.raw, &journal.raw_path),
            };
            // copying a file into itself would never end
            if fs::canonicalize(&path)? == fs::canonicalize(backup_path)? {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "this is the session backup file",
                ));
            }
            backup.flush()?;
            io::copy(&mut File::open(backup_path)?, &mut file.writer)?;
        }
        *self.slot(stream) = Some(file);
        Ok(())
    }

    /// Starts the session backup in `dir`, named after now and the port, its text written as
    /// `text` says, with its meta file for a later restore, then removes the oldest sessions
    /// there so `keep` are left (at least the new one)
    pub fn start_journal(
        &mut self,
        dir: &Path,
        device: &SessionDevice,
        text: &JournalText,
        keep: usize,
    ) -> io::Result<()> {
        let now = Local::now();
        let stem = format!(
            "{}_{}",
            now.format("%Y%m%d_%H%M%S"),
            session_tag(&device.port)
        );
        let text_path = dir.join(format!("{stem}.log"));
        let raw_path = dir.join(format!("{stem}.raw"));
        let meta = SessionMeta {
            version: SESSION_META_VERSION,
            started: now.timestamp(),
            device: device.clone(),
            text: text.clone(),
        };
        let meta = toml::to_string(&meta).map_err(io::Error::other)?;
        create_parent_dirs(&text_path)?;
        fs::write(dir.join(format!("{stem}.meta")), meta)?;
        self.journal = Some(Journal {
            text: LogFile::open(&text_path)?,
            raw: LogFile::open(&raw_path)?,
            text_path,
            raw_path,
            timestamp: text.timestamp.as_deref().map(TimestampFormat::new),
            tx_mark: text.tx_mark.clone(),
        });
        prune_sessions(dir, keep.max(1))
    }

    pub fn has_journal(&self) -> bool {
        self.journal.is_some()
    }

    pub fn stop(&mut self, stream: Stream) {
        if let Some(mut file) = self.slot(stream).take() {
            let _ = file.flush();
        }
    }

    pub fn is_active(&self, stream: Stream) -> bool {
        match stream {
            Stream::Text => self.text.is_some(),
            Stream::Raw => self.raw.is_some(),
        }
    }

    pub fn is_any_active(&self) -> bool {
        self.text.is_some() || self.raw.is_some()
    }

    pub fn flush_if_due(&mut self) {
        let journal = self
            .journal
            .iter_mut()
            .flat_map(|j| [&mut j.text, &mut j.raw]);
        for file in self.text.iter_mut().chain(&mut self.raw).chain(journal) {
            let _ = file.flush_if_due();
        }
    }

    /// Records received bytes: as is to the raw files, and each line they end, without its
    /// EOL, to the text files, after its first byte's time when `timestamp_format` is given
    pub fn record_rx(
        &mut self,
        rx_eol_mode: RxNewlineMode,
        encoding: TextEncoding,
        timestamp_format: Option<&str>,
        bytes: &[u8],
    ) {
        let journal_raw = self.journal.as_mut().map(|j| &mut j.raw);
        for file in self.raw.iter_mut().chain(journal_raw) {
            let _ = file.writer.write_all(bytes);
        }

        self.set_timestamp_format(timestamp_format);
        let now = Local::now();
        // taken out while the lines are written, as the writes borrow the recorder
        let mut lines = std::mem::take(&mut self.lines);
        for &b in bytes {
            if lines.push(b, rx_eol_mode, now) {
                let timestamp = lines.take_timestamp();
                if self.text.is_some() || self.journal.is_some() {
                    let text = encoding.decode_cow(lines.line());
                    self.write_text(timestamp, &text, Some(""), false);
                }
                lines.clear();
            }
        }
        self.lines = lines;
        self.flush_if_due();
    }

    /// Records sent bytes as text, one line for each line they hold (CR, LF or CRLF ends one),
    /// at the time they are sent: to the text recording after `mark` (`> line`, `None` when
    /// TX isn't recorded), to the journal after its own mark. Raw stays what is received, to
    /// be replayed. An RX line still pending is written when it ends, after these
    pub fn record_tx(
        &mut self,
        encoding: TextEncoding,
        timestamp_format: Option<&str>,
        mark: Option<&str>,
        bytes: &[u8],
    ) {
        if self.text.is_none() && self.journal.is_none() {
            return;
        }
        self.set_timestamp_format(timestamp_format);
        let now = Local::now();
        for line in tx_lines(bytes) {
            let text = encoding.decode_cow(line);
            self.write_text(now, &text, mark, true);
        }
        self.flush_if_due();
    }

    /// Parses `format` when it isn't the one already parsed
    fn set_timestamp_format(&mut self, format: Option<&str>) {
        match format {
            None => self.timestamp = None,
            Some(format) if self.timestamp.as_ref().is_some_and(|t| t.format == format) => {}
            Some(format) => self.timestamp = Some(TimestampFormat::new(format)),
        }
    }

    /// Writes a text line to the text recording after `mark` and to the journal after its own
    /// mark (a sent line when `tx`), each with its own timestamp format. A `None` mark leaves
    /// that file out. The line is built once when both are written the same way
    fn write_text(&mut self, time: DateTime<Local>, text: &str, mark: Option<&str>, tx: bool) {
        // the format and mark `line_buf` was built with
        let mut built = None;
        if let (Some(file), Some(mark)) = (&mut self.text, mark) {
            build_line(&mut self.line_buf, &self.timestamp, time, mark, text);
            let _ = file.writer.write_all(&self.line_buf);
            built = Some((self.timestamp.as_ref().map(|t| t.format.as_str()), mark));
        }
        if let Some(journal) = &mut self.journal {
            let mark = if tx {
                journal.tx_mark.as_deref()
            } else {
                Some("")
            };
            if let Some(mark) = mark {
                let format = journal.timestamp.as_ref().map(|t| t.format.as_str());
                if built != Some((format, mark)) {
                    build_line(&mut self.line_buf, &journal.timestamp, time, mark, text);
                }
                let _ = journal.text.writer.write_all(&self.line_buf);
            }
        }
    }
}

impl TimestampFormat {
    /// Settings validate the format, so one that doesn't parse gives an empty timestamp
    fn new(format: &str) -> Self {
        Self {
            format: format.to_string(),
            items: StrftimeItems::new(format)
                .parse_to_owned()
                .unwrap_or_default(),
        }
    }
}

/// A line of a journal's text read back: its time when it has one, whether it was sent, and
/// its text without them
#[derive(Debug, PartialEq)]
pub struct JournalLine<'a> {
    pub time: Option<DateTime<Local>>,
    pub tx: bool,
    pub text: &'a str,
}

/// Reads a journal's text lines back with the format they were written with (MAX-148). A time
/// that doesn't parse leaves the whole line as text, with no time. A format without a date
/// takes the session's start date, and the next day when the time goes back by more than 12
/// hours (midnight: a sent line written before a pending received one goes back a little)
pub struct JournalReader {
    timestamp: Option<Vec<Item<'static>>>,
    tx_mark: Option<String>,
    date: NaiveDate,
    last: Option<NaiveDateTime>,
}

impl JournalReader {
    pub fn new(text: &JournalText, started: DateTime<Local>) -> Self {
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
    pub fn parse<'a>(&mut self, line: &'a str) -> JournalLine<'a> {
        let (time, rest) = match self.parse_time(line) {
            Some((time, rest)) => (Some(time), rest),
            None => (None, line),
        };
        let sent = self
            .tx_mark
            .as_deref()
            .and_then(|mark| rest.strip_prefix(mark));
        JournalLine {
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

/// The lines of sent bytes, without their EOL. A last part without EOL is a line too, as a
/// send is whole (a lone ^C)
fn tx_lines(bytes: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if matches!(bytes[i], b'\r' | b'\n') {
            lines.push(&bytes[start..i]);
            if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                i += 1;
            }
            start = i + 1;
        }
        i += 1;
    }
    if start < bytes.len() {
        lines.push(&bytes[start..]);
    }
    lines
}

/// Removes the oldest sessions in `dir` so `keep` are left. A session is the `.log`, `.raw`
/// and `.meta` files of one stem, which starts with its date so the names sort by age
fn prune_sessions(dir: &Path, keep: usize) -> io::Result<()> {
    let mut sessions: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let is_session = matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("log" | "raw" | "meta")
        );
        if is_session && let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
            sessions.entry(stem.to_string()).or_default().push(path);
        }
    }
    let old = sessions.len().saturating_sub(keep);
    for path in sessions.into_values().take(old).flatten() {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

/// The newest session in `dir` recorded on `device` that received something, and within
/// `max_age` of now when given. To call before `start_journal`, whose pruning could remove it
pub fn find_last_session(
    dir: &Path,
    device: &SessionDevice,
    max_age: Option<Duration>,
) -> Option<PastSession> {
    let mut stems: Vec<String> = fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()? != "meta" {
                return None;
            }
            Some(path.file_stem()?.to_str()?.to_string())
        })
        .collect();
    // the stem starts with its date: the newest first
    stems.sort_unstable_by(|a, b| b.cmp(a));
    stems
        .iter()
        .find_map(|stem| past_session(dir, stem, device, max_age))
}

/// The session of `stem` when it matches `device`, has received something and isn't too old
fn past_session(
    dir: &Path,
    stem: &str,
    device: &SessionDevice,
    max_age: Option<Duration>,
) -> Option<PastSession> {
    let meta = fs::read_to_string(dir.join(format!("{stem}.meta"))).ok()?;
    let meta: SessionMeta = toml::from_str(&meta).ok()?;
    if meta.version != SESSION_META_VERSION || meta.device != *device {
        return None;
    }
    let raw_path = dir.join(format!("{stem}.raw"));
    let raw = fs::metadata(&raw_path).ok()?;
    let modified = raw.modified().ok()?;
    let is_recent = max_age.is_none_or(|max_age| {
        SystemTime::now()
            .duration_since(modified)
            .is_ok_and(|age| age <= max_age)
    });
    if raw.len() == 0 || !is_recent {
        return None;
    }
    Some(PastSession {
        text_path: dir.join(format!("{stem}.log")),
        raw_path,
        started: DateTime::from_timestamp(meta.started, 0)?.with_timezone(&Local),
        ended: modified.into(),
        raw_len: raw.len(),
        text: meta.text,
    })
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

impl PastSession {
    /// The port's part of the file names (`ttyUSB0`)
    pub fn port_tag(&self) -> &str {
        self.raw_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.splitn(3, '_').nth(2))
            .unwrap_or("session")
    }

    /// When it started and ended, the end without its date on the same day
    pub fn time_span(&self) -> String {
        let ended = if self.ended.date_naive() == self.started.date_naive() {
            self.ended.format("%H:%M")
        } else {
            self.ended.format("%Y-%m-%d %H:%M")
        };
        format!("{} → {ended}", self.started.format("%Y-%m-%d %H:%M"))
    }
}

/// The line that marks where a restored session ends and the live data starts
pub fn restore_separator(session: &PastSession) -> String {
    format!(
        "── restored: {}, {} ──",
        session.port_tag(),
        session.time_span()
    )
}

/// A session journal written in `dir` on `port` the way a run would: `rx` received, then `tx`
/// sent in line mode, and the session found back
#[cfg(test)]
pub fn write_test_session(
    dir: &Path,
    port: &str,
    text: &JournalText,
    rx: &[u8],
    tx: &[u8],
) -> PastSession {
    let device = SessionDevice::new(
        &SerialConfig {
            port: port.to_string(),
            ..SerialConfig::default()
        },
        TextEncoding::Utf8,
    );
    let mut recorder = Recorder::default();
    recorder.start_journal(dir, &device, text, 20).unwrap();
    recorder.record_rx(RxNewlineMode::Auto, TextEncoding::Utf8, None, rx);
    if !tx.is_empty() {
        recorder.record_tx(TextEncoding::Utf8, None, None, tx);
    }
    drop(recorder);
    find_last_session(dir, &device, None).unwrap()
}

/// The machine's name, empty when it can't be read
pub fn hostname() -> String {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // the buffer is valid for its length, which bounds what is written
        let res = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
        if res != 0 {
            return String::new();
        }
        let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..len]).into_owned()
    }
    #[cfg(not(unix))]
    {
        std::env::var("COMPUTERNAME").unwrap_or_default()
    }
}

/// Creates the missing folders of `path`, so a file can be written there
pub fn create_parent_dirs(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => fs::create_dir_all(parent),
        _ => Ok(()),
    }
}

/// `~` at the start of a path is the home folder, as in a shell
pub fn expand_home(path: &Path) -> PathBuf {
    match (path.strip_prefix("~"), dirs::home_dir()) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => path.to_path_buf(),
    }
}

/// The port's last part, kept to the characters safe in a file name: "/dev/ttyUSB0" gives
/// "ttyUSB0"
pub fn session_tag(port: &str) -> String {
    let name = port.rsplit(['/', '\\']).next().unwrap_or(port);
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Builds a text line in `buf`, reused so a line costs no allocation: the time when a format
/// is set, `mark`, then `text` without its EOL and a single "\n". It is built once and
/// written to every text file
fn build_line(
    buf: &mut Vec<u8>,
    format: &Option<TimestampFormat>,
    time: DateTime<Local>,
    mark: &str,
    text: &str,
) {
    buf.clear();
    if let Some(format) = format {
        let _ = write!(buf, "{} ", time.format_with_items(format.items.iter()));
    }
    let text = text.strip_suffix('\n').unwrap_or(text);
    let text = text.strip_suffix('\r').unwrap_or(text);
    buf.extend_from_slice(mark.as_bytes());
    buf.extend_from_slice(text.as_bytes());
    buf.push(b'\n');
}

#[cfg(test)]
mod tests {
    //! Recordings: what the text and raw files get, batching and final flush.

    use super::*;

    fn temp_log_path(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("maxicom_test_{}_{name}.log", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn record(recorder: &mut Recorder, bytes: &[u8]) {
        recorder.record_rx(RxNewlineMode::Auto, TextEncoding::Utf8, None, bytes);
    }

    /// Starts the stream to a fresh temp file.
    fn start(recorder: &mut Recorder, stream: Stream, name: &str) -> PathBuf {
        let path = temp_log_path(name);
        recorder.start(stream, path.clone(), false).unwrap();
        path
    }

    /// Reads the stream's file after stopping it, and removes it.
    fn stop_and_read(recorder: &mut Recorder, stream: Stream, path: &PathBuf) -> Vec<u8> {
        recorder.stop(stream);
        let bytes = std::fs::read(path).unwrap();
        let _ = std::fs::remove_file(path);
        bytes
    }

    /// A line recorded right after start stays buffered, and `stop` writes it to the file.
    /// How: reads the file before and after `stop` (well under FLUSH_INTERVAL).
    #[test]
    fn text_recording_buffers_until_stop() {
        let mut recorder = Recorder::default();
        let path = start(&mut recorder, Stream::Text, "buffered");
        record(&mut recorder, b"hello\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        assert_eq!(
            stop_and_read(&mut recorder, Stream::Text, &path),
            b"hello\n"
        );
    }

    /// Raw bytes are written as is, and dropping the recorder flushes them.
    /// How: records bytes, drops the recorder, reads the file.
    #[test]
    fn raw_recording_flushes_on_drop() {
        let mut recorder = Recorder::default();
        let path = start(&mut recorder, Stream::Raw, "dropped");
        record(&mut recorder, b"\x1b[31mraw\r\n");
        drop(recorder);
        assert_eq!(std::fs::read(&path).unwrap(), b"\x1b[31mraw\r\n");
        let _ = std::fs::remove_file(&path);
    }

    /// MAX-147: a text line is written without its own EOL, so CRLF and LF lines both end
    /// with a single "\n" (it used to get its EOL and another "\n").
    /// How: "abc\r\n" and "def\n" in one chunk.
    #[test]
    fn text_line_ends_with_one_newline() {
        let mut recorder = Recorder::default();
        let path = start(&mut recorder, Stream::Text, "eol");
        record(&mut recorder, b"abc\r\ndef\n");
        assert_eq!(
            stop_and_read(&mut recorder, Stream::Text, &path),
            b"abc\ndef\n"
        );
    }

    /// MAX-147: the timestamp is `timestamp_format` as is, with no added brackets (the
    /// default format, already bracketed, used to give "[[..]]").
    /// How: the format "[ts]" has no strftime field, so the output is fixed.
    #[test]
    fn text_timestamp_is_the_format_as_is() {
        let mut recorder = Recorder::default();
        let path = start(&mut recorder, Stream::Text, "ts");
        recorder.record_rx(
            RxNewlineMode::Auto,
            TextEncoding::Utf8,
            Some("[ts]"),
            b"abc\n",
        );
        assert_eq!(
            stop_and_read(&mut recorder, Stream::Text, &path),
            b"[ts] abc\n"
        );
    }

    /// MAX-147: the parsed timestamp format follows a changed `timestamp_format` (a reload),
    /// and turning it off writes lines without a time; invalid UTF-8 is replaced, as before.
    /// How: "[a]" then "[b]" then none, one line each, the last with a lone 0xFF byte.
    #[test]
    fn timestamp_format_change_is_parsed_again() {
        let mut recorder = Recorder::default();
        let path = start(&mut recorder, Stream::Text, "ts_change");
        let enc = TextEncoding::Utf8;
        recorder.record_rx(RxNewlineMode::Auto, enc, Some("[a]"), b"one\n");
        recorder.record_rx(RxNewlineMode::Auto, enc, Some("[b]"), b"two\n");
        recorder.record_rx(RxNewlineMode::Auto, enc, None, b"\xFF\n");
        assert_eq!(
            stop_and_read(&mut recorder, Stream::Text, &path),
            "[a] one\n[b] two\n\u{FFFD}\n".as_bytes()
        );
    }

    /// Only ended lines are written as text. The text a lone CR overwrites never is, and a
    /// partial line still pending at stop is lost.
    /// How: "old\rnew\n" then "tail" without EOL, in Auto mode.
    #[test]
    fn partial_and_overwritten_text_is_not_recorded() {
        let mut recorder = Recorder::default();
        let path = start(&mut recorder, Stream::Text, "partial");
        record(&mut recorder, b"old\rnew\ntail");
        assert_eq!(stop_and_read(&mut recorder, Stream::Text, &path), b"new\n");
    }

    /// Raw gets the bytes as received, text gets them decoded.
    /// How: Latin1 E9 and an escape sequence, both streams on.
    #[test]
    fn raw_is_received_bytes_and_text_is_decoded() {
        let mut recorder = Recorder::default();
        let text = start(&mut recorder, Stream::Text, "dec_text");
        let raw = start(&mut recorder, Stream::Raw, "dec_raw");
        recorder.record_rx(
            RxNewlineMode::Auto,
            TextEncoding::Latin1,
            None,
            b"\xE9\x1b[31mred\r\n",
        );
        assert_eq!(
            stop_and_read(&mut recorder, Stream::Raw, &raw),
            b"\xE9\x1b[31mred\r\n"
        );
        assert_eq!(
            stop_and_read(&mut recorder, Stream::Text, &text),
            "é\x1b[31mred\n".as_bytes()
        );
    }

    /// Text and raw start and stop apart, and a text recording started mid-line gets the
    /// whole line.
    /// How: "ab" with nothing on, text on, "c\n", raw on, "d\n".
    #[test]
    fn streams_start_apart_and_text_gets_the_whole_line() {
        let mut recorder = Recorder::default();
        record(&mut recorder, b"ab");
        let text = start(&mut recorder, Stream::Text, "apart_text");
        assert!(recorder.is_active(Stream::Text) && !recorder.is_active(Stream::Raw));
        record(&mut recorder, b"c\n");
        let raw = start(&mut recorder, Stream::Raw, "apart_raw");
        record(&mut recorder, b"d\n");
        assert_eq!(stop_and_read(&mut recorder, Stream::Raw, &raw), b"d\n");
        assert!(recorder.is_active(Stream::Text));
        assert_eq!(
            stop_and_read(&mut recorder, Stream::Text, &text),
            b"abc\nd\n"
        );
        assert!(!recorder.is_active(Stream::Text));
    }

    /// A device on `port`, on this machine.
    fn device(port: &str) -> SessionDevice {
        SessionDevice::new(
            &SerialConfig {
                port: port.to_string(),
                ..SerialConfig::default()
            },
            TextEncoding::Utf8,
        )
    }

    /// A fresh temp folder.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("maxicom_test_{}_{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    /// MAX-147: the session backup gets everything from its start, text and raw, whatever
    /// records, and a recording can start with its content.
    /// How: backup in a temp folder, "a\n" received, a text recording including the session,
    /// "b\n" received, then the backup files read back.
    #[test]
    fn journal_records_apart_and_a_recording_can_start_with_it() {
        let dir = temp_dir("journal");
        let mut recorder = Recorder::default();
        recorder
            .start_journal(&dir, &device("port"), &JournalText::default(), 20)
            .unwrap();
        assert!(recorder.has_journal() && !recorder.is_any_active());
        record(&mut recorder, b"a\n");

        let path = dir.join("mine.log");
        recorder.start(Stream::Text, path.clone(), true).unwrap();
        record(&mut recorder, b"b\n");
        assert_eq!(stop_and_read(&mut recorder, Stream::Text, &path), b"a\nb\n");

        drop(recorder);
        let mut files: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        files.sort();
        assert_eq!(files.len(), 3);
        assert!(files[0].to_string_lossy().ends_with("_port.log"));
        assert_eq!(fs::read(&files[0]).unwrap(), b"a\nb\n");
        assert!(files[1].to_string_lossy().ends_with("_port.meta"));
        assert_eq!(fs::read(&files[2]).unwrap(), b"a\nb\n");
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-147: sent lines go to the text recording and the backup text as `> line`, raw
    /// files get none of them.
    /// How: text, raw and backup on, "rx\n" received, then "a\r\nb\n\r\n" and a lone ^C sent.
    #[test]
    fn tx_lines_go_to_text_streams_only() {
        let dir = temp_dir("tx");
        let mut recorder = Recorder::default();
        let text = JournalText {
            timestamp: None,
            tx_mark: Some("> ".to_string()),
        };
        recorder
            .start_journal(&dir, &device("port"), &text, 20)
            .unwrap();
        let text = start(&mut recorder, Stream::Text, "tx_text");
        let raw = start(&mut recorder, Stream::Raw, "tx_raw");
        record(&mut recorder, b"rx\n");
        recorder.record_tx(TextEncoding::Utf8, None, Some("> "), b"a\r\nb\n\r\n");
        recorder.record_tx(TextEncoding::Utf8, None, Some("> "), b"\x03");

        let expected = b"rx\n> a\n> b\n> \n> \x03\n";
        assert_eq!(stop_and_read(&mut recorder, Stream::Text, &text), expected);
        assert_eq!(stop_and_read(&mut recorder, Stream::Raw, &raw), b"rx\n");
        drop(recorder);
        let backup = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().is_some_and(|ext| ext == "log"))
            .unwrap();
        assert_eq!(fs::read(backup).unwrap(), expected);
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-147: pruning keeps the newest sessions (all files of each, the meta file with them,
    /// MAX-148) and leaves other files.
    /// How: 3 sessions and a note, keep 2.
    #[test]
    fn prune_keeps_the_newest_sessions() {
        let dir = temp_dir("prune");
        fs::create_dir_all(&dir).unwrap();
        for name in [
            "20260101_000000_a.log",
            "20260101_000000_a.meta",
            "20260101_000000_a.raw",
            "20260102_000000_a.log",
            "20260102_000000_a.meta",
            "20260102_000000_a.raw",
            "20260103_000000_b.log",
            "notes.txt",
        ] {
            fs::write(dir.join(name), "").unwrap();
        }
        prune_sessions(&dir, 2).unwrap();
        let mut left: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "20260102_000000_a.log",
                "20260102_000000_a.meta",
                "20260102_000000_a.raw",
                "20260103_000000_b.log",
                "notes.txt"
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Writes a session of `stem` in `dir` as `start_journal` would, with `raw` received.
    fn write_session(dir: &Path, stem: &str, device: &SessionDevice, raw: &[u8]) {
        fs::create_dir_all(dir).unwrap();
        let meta = SessionMeta {
            version: SESSION_META_VERSION,
            started: 1_700_000_000,
            device: device.clone(),
            text: JournalText::default(),
        };
        fs::write(
            dir.join(format!("{stem}.meta")),
            toml::to_string(&meta).unwrap(),
        )
        .unwrap();
        fs::write(dir.join(format!("{stem}.log")), "").unwrap();
        fs::write(dir.join(format!("{stem}.raw")), raw).unwrap();
    }

    fn found_stem(dir: &Path, device: &SessionDevice, max_age: Option<Duration>) -> Option<String> {
        let session = find_last_session(dir, device, max_age)?;
        Some(session.raw_path.file_stem()?.to_str()?.to_string())
    }

    /// MAX-148: the last session found is the newest one on the same device that received
    /// something; another host, port, baud or encoding, an empty session, an unknown meta
    /// version or a broken meta file never match.
    /// How: sessions from oldest to newest, each newer one failing one check.
    #[test]
    fn last_session_matches_the_device() {
        let dir = temp_dir("last_session");
        let mine = device("/dev/ttyUSB0");
        write_session(&dir, "20260101_000000_ttyUSB0", &mine, b"old");
        write_session(&dir, "20260102_000000_ttyUSB0", &mine, b"good");
        let other_host = SessionDevice {
            host: format!("{}-other", mine.host),
            ..mine.clone()
        };
        write_session(&dir, "20260103_000000_ttyUSB0", &other_host, b"x");
        write_session(
            &dir,
            "20260104_000000_ttyUSB1",
            &device("/dev/ttyUSB1"),
            b"x",
        );
        let other_baud = SessionDevice {
            baud: 9600,
            ..mine.clone()
        };
        write_session(&dir, "20260105_000000_ttyUSB0", &other_baud, b"x");
        let other_encoding = SessionDevice {
            encoding: TextEncoding::Latin1,
            ..mine.clone()
        };
        write_session(&dir, "20260106_000000_ttyUSB0", &other_encoding, b"x");
        write_session(&dir, "20260107_000000_ttyUSB0", &mine, b"");
        write_session(&dir, "20260108_000000_ttyUSB0", &mine, b"x");
        let meta = dir.join("20260108_000000_ttyUSB0.meta");
        let next_version = fs::read_to_string(&meta)
            .unwrap()
            .replace("version = 1", "version = 2");
        fs::write(&meta, next_version).unwrap();
        write_session(&dir, "20260109_000000_ttyUSB0", &mine, b"x");
        fs::write(dir.join("20260109_000000_ttyUSB0.meta"), "not toml [").unwrap();

        let session = find_last_session(&dir, &mine, None).unwrap();
        assert_eq!(session.raw_path, dir.join("20260102_000000_ttyUSB0.raw"));
        assert_eq!(session.text_path, dir.join("20260102_000000_ttyUSB0.log"));
        assert_eq!(session.raw_len, 4);
        assert_eq!(session.started.timestamp(), 1_700_000_000);
        assert_eq!(
            found_stem(&dir, &other_host, None).as_deref(),
            Some("20260103_000000_ttyUSB0")
        );
        assert_eq!(found_stem(&dir, &device("/dev/ttyACM0"), None), None);
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-148: `max_age` is checked on when the session last received (its raw file's time),
    /// so a long session that started long ago still restores.
    /// How: the raw file's time set 2 hours back, then 1 and 3 hours of max age.
    #[test]
    fn last_session_max_age_uses_the_last_write() {
        let dir = temp_dir("last_session_age");
        let mine = device("/dev/ttyUSB0");
        write_session(&dir, "20260101_000000_ttyUSB0", &mine, b"x");
        let two_hours_ago = SystemTime::now() - Duration::from_secs(2 * 3600);
        File::options()
            .write(true)
            .open(dir.join("20260101_000000_ttyUSB0.raw"))
            .unwrap()
            .set_modified(two_hours_ago)
            .unwrap();
        let hours = |h: u64| Some(Duration::from_secs(h * 3600));
        assert_eq!(found_stem(&dir, &mine, hours(1)), None);
        assert_eq!(
            found_stem(&dir, &mine, hours(3)).as_deref(),
            Some("20260101_000000_ttyUSB0")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-148: the journal's meta file holds the device, so its session is found again.
    /// How: a journal started and fed, then dropped, then looked up.
    #[test]
    fn journal_session_is_found_again() {
        let dir = temp_dir("journal_meta");
        let mine = device("/dev/ttyUSB0");
        let mut recorder = Recorder::default();
        let text = JournalText {
            timestamp: Some("[%H:%M]".to_string()),
            tx_mark: None,
        };
        recorder.start_journal(&dir, &mine, &text, 20).unwrap();
        record(&mut recorder, b"hello\n");
        drop(recorder);
        let session = find_last_session(&dir, &mine, None).unwrap();
        assert_eq!(session.text, text);
        assert!(session.raw_path.to_string_lossy().ends_with("_ttyUSB0.raw"));
        assert_eq!(session.raw_len, 6);
        let log = fs::read_to_string(&session.text_path).unwrap();
        assert!(log.ends_with("] hello\n"), "{log:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-148: the journal keeps the text format it started with, whatever the recording
    /// uses later, and records sent lines only when it started with a mark.
    /// How: the journal with "[j]" and "J ", the recording with "[r]" and "R ", then a TX
    /// with the recording's TX off; a second journal started without a mark.
    #[test]
    fn journal_keeps_its_text_format() {
        let dir = temp_dir("journal_format");
        let mut recorder = Recorder::default();
        let text = JournalText {
            timestamp: Some("[j]".to_string()),
            tx_mark: Some("J ".to_string()),
        };
        recorder
            .start_journal(&dir, &device("a"), &text, 20)
            .unwrap();
        let rec = start(&mut recorder, Stream::Text, "journal_format");
        let enc = TextEncoding::Utf8;
        recorder.record_rx(RxNewlineMode::Auto, enc, Some("[r]"), b"rx\n");
        recorder.record_tx(enc, Some("[r]"), Some("R "), b"tx\n");
        recorder.record_tx(enc, None, None, b"off\n");
        assert_eq!(
            stop_and_read(&mut recorder, Stream::Text, &rec),
            b"[r] rx\n[r] R tx\n"
        );
        drop(recorder);
        let session = find_last_session(&dir, &device("a"), None).unwrap();
        assert_eq!(
            fs::read(&session.text_path).unwrap(),
            b"[j] rx\n[j] J tx\n[j] J off\n"
        );

        let mut recorder = Recorder::default();
        recorder
            .start_journal(&dir, &device("b"), &JournalText::default(), 20)
            .unwrap();
        record(&mut recorder, b"rx\n");
        recorder.record_tx(enc, None, Some("> "), b"tx\n");
        drop(recorder);
        let session = find_last_session(&dir, &device("b"), None).unwrap();
        assert_eq!(fs::read(&session.text_path).unwrap(), b"rx\n");
        let _ = fs::remove_dir_all(&dir);
    }

    fn reader(timestamp: Option<&str>, tx_mark: Option<&str>) -> JournalReader {
        let text = JournalText {
            timestamp: timestamp.map(str::to_string),
            tx_mark: tx_mark.map(str::to_string),
        };
        let started = Local.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        JournalReader::new(&text, started)
    }

    fn at(day: u32, h: u32, m: u32, s: u32, ms: u32) -> Option<DateTime<Local>> {
        let date = NaiveDate::from_ymd_opt(2026, 1, day)?;
        Local
            .from_local_datetime(&date.and_hms_milli_opt(h, m, s, ms)?)
            .earliest()
    }

    /// MAX-148: a journal line is read back with its format: the time (dated from the session
    /// start, the next day past midnight, not on a sent line slightly earlier), the mark, and
    /// a line whose time doesn't parse kept whole.
    #[test]
    fn journal_lines_read_back() {
        let mut r = reader(Some("[%H:%M:%S%.3f]"), Some("> "));
        let line = |time, tx, text| JournalLine { time, tx, text };
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
    fn journal_lines_without_time_or_mark() {
        let line = |text| JournalLine {
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

    /// MAX-148: what the journal writes, with each combination of timestamp and mark, reads
    /// back as what was received and sent.
    /// How: a journal per combination, "rx" received and "tx" sent, its text parsed back.
    #[test]
    fn journal_text_round_trip() {
        let dir = temp_dir("journal_round_trip");
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
            let text = JournalText {
                timestamp: timestamp.map(str::to_string),
                tx_mark: tx_mark.map(str::to_string),
            };
            let port = format!("p{i}");
            let mut recorder = Recorder::default();
            recorder
                .start_journal(&dir, &device(&port), &text, 20)
                .unwrap();
            record(&mut recorder, b"rx\n");
            recorder.record_tx(TextEncoding::Utf8, None, None, b"tx\n");
            drop(recorder);
            let session = find_last_session(&dir, &device(&port), None).unwrap();

            let log = fs::read_to_string(&session.text_path).unwrap();
            let mut r = JournalReader::new(&session.text, session.started);
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
        let mut session = PastSession {
            text_path: PathBuf::from("20260101_101500_ttyUSB0.log"),
            raw_path: PathBuf::from("/c/20260101_101500_ttyUSB0.raw"),
            started: day(1, 10),
            ended: day(1, 16),
            raw_len: 1,
            text: JournalText::default(),
        };
        assert_eq!(
            restore_separator(&session),
            "── restored: ttyUSB0, 2026-01-01 10:15 → 16:15 ──"
        );
        session.ended = day(2, 9);
        assert_eq!(
            restore_separator(&session),
            "── restored: ttyUSB0, 2026-01-01 10:15 → 2026-01-02 09:15 ──"
        );
    }

    /// MAX-147: a recording into folders that don't exist creates them.
    #[test]
    fn recording_creates_missing_folders() {
        let dir = temp_dir("folders");
        let path = dir.join("a").join("b").join("x.raw");
        let mut recorder = Recorder::default();
        recorder.start(Stream::Raw, path.clone(), false).unwrap();
        record(&mut recorder, b"x");
        assert_eq!(stop_and_read(&mut recorder, Stream::Raw, &path), b"x");
        let _ = fs::remove_dir_all(&dir);
    }

    /// `~` is the home folder only as the first part of a path; the port tag keeps the last
    /// part of the port, safe for a file name.
    #[test]
    fn home_expansion_and_session_tag() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(
            expand_home(Path::new("~/logs/x.log")),
            home.join("logs/x.log")
        );
        assert_eq!(expand_home(Path::new("a/~/x")), Path::new("a/~/x"));
        assert_eq!(expand_home(Path::new("~x")), Path::new("~x"));
        assert_eq!(session_tag("/dev/ttyUSB0"), "ttyUSB0");
        assert_eq!(session_tag("COM3"), "COM3");
        assert_eq!(session_tag("file:///tmp/my log.raw"), "my_log.raw");
    }
}
