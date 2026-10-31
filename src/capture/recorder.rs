use super::create_parent_dirs;
use super::session::{
    JournalText, SESSION_META_VERSION, SessionDevice, SessionMeta, prune_sessions, session_tag,
};
use crate::app::types::RxLineSplitter;
use crate::configs::{RxNewlineMode, TextEncoding};
use chrono::format::{Item, StrftimeItems};
use chrono::{DateTime, Local};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
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
    /// Appends to the file, creating it and its missing folders. Readable too: Windows only
    /// locks a handle that can read or write, and append alone can't
    fn open(path: &Path) -> io::Result<Self> {
        create_parent_dirs(path)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(path)?;
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

/// Recordings of what is received, fed whatever the mode shows: text and raw are started and
/// stopped apart, each to its own file (MAX-147). The lines are cut even when nothing records
/// text, so a recording started mid-line still gets that whole line
#[derive(Default)]
pub struct Recorder {
    text: Option<LogFile>,
    raw: Option<LogFile>,
    journal: Option<Journal>,
    /// The journal a replay paused (MAX-166): open and locked, recording nothing
    paused_journal: Option<Journal>,
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
            // through the backup's own handle: on Windows the `.raw` lock blocks reads from any
            // other one. Appends still go to the end, wherever this leaves the position
            let mut reader = backup.writer.get_ref();
            reader.seek(SeekFrom::Start(0))?;
            io::copy(&mut reader, &mut file.writer)?;
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
        // to the millisecond: a relaunch within the second must not append to the last
        // session and overwrite its meta (MAX-148)
        let stem = format!(
            "{}_{}",
            now.format("%Y%m%d_%H%M%S%3f"),
            session_tag(&device.address)
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
        let raw = LogFile::open(&raw_path)?;
        // held while the session runs, so no pruning removes it, from this maxicom or another
        // one (MAX-148). The OS drops it with the process, even killed. A file system without
        // locks only loses that guard
        let _ = raw.writer.get_ref().try_lock();
        self.journal = Some(Journal {
            text: LogFile::open(&text_path)?,
            raw,
            text_path,
            raw_path,
            timestamp: text.timestamp.as_deref().map(TimestampFormat::new),
            tx_mark: text.tx_mark.clone(),
        });
        prune_sessions(dir, keep.max(1), &stem)
    }

    pub fn has_journal(&self) -> bool {
        self.journal.is_some()
    }

    /// Stops recording to the session backup until `resume_journal`, its files kept open, and
    /// its lock with them, so no pruning removes it (MAX-166). The line being cut is dropped
    /// at both ends, so a line never mixes the live bytes and the replayed ones
    pub fn pause_journal(&mut self) {
        if let Some(mut journal) = self.journal.take() {
            let _ = journal.text.flush();
            let _ = journal.raw.flush();
            self.paused_journal = Some(journal);
        }
        self.lines = RxLineSplitter::default();
    }

    pub fn resume_journal(&mut self) {
        if let Some(journal) = self.paused_journal.take() {
            self.journal = Some(journal);
        }
        self.lines = RxLineSplitter::default();
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
    use crate::capture::find_last_session;
    use crate::capture::tests::{device, record, temp_dir};

    fn temp_log_path(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("maxicom_test_{}_{name}.log", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
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

    /// MAX-166: a paused journal records nothing, a user recording still does, and the journal
    /// goes on in the same files once resumed; the line cut at the pause is dropped.
    /// How: "a\n" and half a line received, the journal paused, a text recording started, "b\n"
    /// received, the journal resumed, "c\n" received, then the files read back.
    #[test]
    fn paused_journal_records_nothing() {
        let dir = temp_dir("journal_paused");
        let mut recorder = Recorder::default();
        recorder
            .start_journal(&dir, &device("port"), &JournalText::default(), 20)
            .unwrap();
        record(&mut recorder, b"a\nhal");
        recorder.pause_journal();
        assert!(!recorder.has_journal());
        let path = dir.join("mine.log");
        recorder.start(Stream::Text, path.clone(), true).unwrap();
        record(&mut recorder, b"b\n");
        recorder.resume_journal();
        assert!(recorder.has_journal());
        record(&mut recorder, b"c\n");
        assert_eq!(stop_and_read(&mut recorder, Stream::Text, &path), b"b\nc\n");

        drop(recorder);
        let read = |extension: &str| {
            let entry = fs::read_dir(&dir)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| path.to_string_lossy().ends_with(extension))
                .unwrap();
            fs::read(entry).unwrap()
        };
        assert_eq!(read("_port.log"), b"a\nc\n");
        assert_eq!(read("_port.raw"), b"a\nhalc\n");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A raw recording can start with the backup's `.raw` while the journal holds its lock
    /// (Windows blocks reads from another handle), and the backup still appends after it.
    /// How: "a\n" received, a raw recording including the session, "b\n" received.
    #[test]
    fn raw_recording_starts_with_the_locked_backup() {
        let dir = temp_dir("journal_raw");
        let mut recorder = Recorder::default();
        recorder
            .start_journal(&dir, &device("port"), &JournalText::default(), 20)
            .unwrap();
        record(&mut recorder, b"a\n");

        let path = dir.join("mine.raw");
        recorder.start(Stream::Raw, path.clone(), true).unwrap();
        record(&mut recorder, b"b\n");
        assert_eq!(stop_and_read(&mut recorder, Stream::Raw, &path), b"a\nb\n");

        drop(recorder);
        let backup = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.to_string_lossy().ends_with("_port.raw"))
            .unwrap();
        assert_eq!(fs::read(backup).unwrap(), b"a\nb\n");
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
            fs::read(session.text_path().unwrap()).unwrap(),
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
        assert_eq!(fs::read(session.text_path().unwrap()).unwrap(), b"rx\n");
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
}
