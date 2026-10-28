use crate::app::types::RxLineSplitter;
use crate::configs::{RxNewlineMode, TextEncoding};
use chrono::{DateTime, Local};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const FLUSH_INTERVAL: Duration = Duration::from_millis(250);

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

    /// Starts the session backup in `dir`, named after now and `tag`, then removes the oldest
    /// sessions there so `keep` are left (at least the new one)
    pub fn start_journal(&mut self, dir: &Path, tag: &str, keep: usize) -> io::Result<()> {
        let stem = format!("{}_{tag}", Local::now().format("%Y%m%d_%H%M%S"));
        let text_path = dir.join(format!("{stem}.log"));
        let raw_path = dir.join(format!("{stem}.raw"));
        self.journal = Some(Journal {
            text: LogFile::open(&text_path)?,
            raw: LogFile::open(&raw_path)?,
            text_path,
            raw_path,
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

        let now = Local::now();
        for &b in bytes {
            if self.lines.push(b, rx_eol_mode, now) {
                let timestamp = self.lines.take_timestamp();
                if self.text.is_some() || self.journal.is_some() {
                    let line = encoding.decode(self.lines.line());
                    let journal_text = self.journal.as_mut().map(|j| &mut j.text);
                    for file in self.text.iter_mut().chain(journal_text) {
                        let _ = write_line(file, &line, timestamp, timestamp_format);
                    }
                }
                self.lines.clear();
            }
        }
        self.flush_if_due();
    }

    /// Records sent bytes to the text files as `> line`, one for each line they hold (CR, LF
    /// or CRLF ends one), at the time they are sent. Raw stays what is received, to be
    /// replayed. An RX line still pending is written when it ends, after these
    pub fn record_tx(
        &mut self,
        encoding: TextEncoding,
        timestamp_format: Option<&str>,
        bytes: &[u8],
    ) {
        if self.text.is_none() && self.journal.is_none() {
            return;
        }
        let now = Local::now();
        for line in tx_lines(bytes) {
            let line = format!("> {}", encoding.decode(line));
            let journal_text = self.journal.as_mut().map(|j| &mut j.text);
            for file in self.text.iter_mut().chain(journal_text) {
                let _ = write_line(file, &line, now, timestamp_format);
            }
        }
        self.flush_if_due();
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

/// Removes the oldest sessions in `dir` so `keep` are left. A session is the `.log` and `.raw`
/// files of one stem, which starts with its date so the names sort by age
fn prune_sessions(dir: &Path, keep: usize) -> io::Result<()> {
    let mut sessions: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let is_session = matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("log" | "raw")
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

fn write_line(
    file: &mut LogFile,
    line: &str,
    timestamp: DateTime<Local>,
    timestamp_format: Option<&str>,
) -> io::Result<()> {
    let line = line.strip_suffix('\n').unwrap_or(line);
    let line = line.strip_suffix('\r').unwrap_or(line);
    match timestamp_format {
        Some(format) => writeln!(file.writer, "{} {line}", timestamp.format(format)),
        None => writeln!(file.writer, "{line}"),
    }
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
        recorder.start_journal(&dir, "port", 20).unwrap();
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
        assert_eq!(files.len(), 2);
        assert!(files[0].to_string_lossy().ends_with("_port.log"));
        assert_eq!(fs::read(&files[0]).unwrap(), b"a\nb\n");
        assert_eq!(fs::read(&files[1]).unwrap(), b"a\nb\n");
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-147: sent lines go to the text recording and the backup text as `> line`, raw
    /// files get none of them.
    /// How: text, raw and backup on, "rx\n" received, then "a\r\nb\n\r\n" and a lone ^C sent.
    #[test]
    fn tx_lines_go_to_text_streams_only() {
        let dir = temp_dir("tx");
        let mut recorder = Recorder::default();
        recorder.start_journal(&dir, "port", 20).unwrap();
        let text = start(&mut recorder, Stream::Text, "tx_text");
        let raw = start(&mut recorder, Stream::Raw, "tx_raw");
        record(&mut recorder, b"rx\n");
        recorder.record_tx(TextEncoding::Utf8, None, b"a\r\nb\n\r\n");
        recorder.record_tx(TextEncoding::Utf8, None, b"\x03");

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

    /// MAX-147: pruning keeps the newest sessions (both files of each) and leaves other files.
    /// How: 3 sessions and a note, keep 2.
    #[test]
    fn prune_keeps_the_newest_sessions() {
        let dir = temp_dir("prune");
        fs::create_dir_all(&dir).unwrap();
        for name in [
            "20260101_000000_a.log",
            "20260101_000000_a.raw",
            "20260102_000000_a.log",
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
                "20260102_000000_a.raw",
                "20260103_000000_b.log",
                "notes.txt"
            ]
        );
        let _ = fs::remove_dir_all(&dir);
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
