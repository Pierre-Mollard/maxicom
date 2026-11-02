use super::restore::times_line;
use super::session::{
    CaptureDevice, CaptureFormat, CaptureSource, RecordingMeta, SessionLimits, folder_file,
    prune_sessions, session_tag,
};
use crate::app::types::RxLineSplitter;
use crate::configs::{RxNewlineMode, TextEncoding};
use crate::paths::create_parent_dirs;
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

/// What a recording saves: the received lines as text in one file, or in full, a folder with
/// the files of a session: the received bytes as is with their times and meta, and the text
/// (MAX-175)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Text,
    Full,
}

impl Stream {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Full => "full",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Text => Self::Full,
            Self::Full => Self::Text,
        }
    }

    /// What the recording adds to its name: the text file's extension, or the full
    /// recording's folder `/` (MAX-175)
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Text => ".log",
            Self::Full => "/",
        }
    }
}

/// The files of a session (MAX-147) or of a full recording (MAX-175), `<stem>.log`, `.raw`,
/// `.times` and `.meta` in one folder: the text, the bytes received, when each chunk of them
/// was received (`times`, at `raw_len`), and the device. The session backup runs apart from the
/// user's recordings, which can start with its content; it is taken out of the recorder while
/// a replay sets the live link aside (MAX-171)
pub struct RecordingWriter {
    text: LogFile,
    raw: LogFile,
    times: LogFile,
    raw_len: u64,
    text_path: PathBuf,
    raw_path: PathBuf,
    timestamp: Option<TimestampFormat>,
    tx_mark: Option<String>,
    started: DateTime<Local>,
    device: Option<CaptureDevice>,
}

/// The user's recording (MAX-175), one at a time, and what it records (MAX-173)
struct Recording {
    kind: RecordingKind,
    source: CaptureSource,
}

/// A text recording, its lines timed as the capture settings say at the moment (a reload
/// changes them), or a full one: a folder like a session's, which has the text too
enum RecordingKind {
    Text {
        file: LogFile,
        timestamp: Option<TimestampFormat>,
    },
    Full(Box<RecordingWriter>),
}

/// What is received, fed whatever the mode shows, to the session backup and the user's
/// recording. The lines are cut even when nothing records text, so a recording started
/// mid-line still gets that whole line
#[derive(Default)]
pub struct Recorder {
    recording: Option<Recording>,
    session: Option<RecordingWriter>,
    lines: RxLineSplitter,
    line_buf: Vec<u8>,
}

/// `timestamp_format` parsed once, and again only when it changes (on reload), instead of on
/// every line
struct TimestampFormat {
    format: String,
    items: Vec<Item<'static>>,
}

impl Recorder {
    /// Starts recording text to the file at `path`, in place of the recording running if any,
    /// first copying in the session backup's text when `include_session`. The recording keeps
    /// `source` (MAX-173)
    pub fn start_text(
        &mut self,
        path: &Path,
        include_session: bool,
        source: CaptureSource,
    ) -> io::Result<()> {
        if include_session && let Some(session) = &self.session {
            refuse_backup_file(path, &session.text_path)?;
        }
        let mut file = LogFile::open(path)?;
        if include_session && let Some(session) = &mut self.session {
            copy_file(&mut session.text, &mut file)?;
        }
        self.stop();
        let kind = RecordingKind::Text {
            file,
            timestamp: None,
        };
        self.recording = Some(Recording { kind, source });
        Ok(())
    }

    /// Starts a full recording in the folder `folder`, in place of the recording running if
    /// any, its files named after it, its text written as `text` says (MAX-175). With `include_session` each file starts with the
    /// session backup's, and the meta with the session's start. The meta says the device of
    /// `source` when known (a plain file replayed has none: no meta). The recording keeps
    /// `source` (MAX-173)
    pub fn start_full(
        &mut self,
        folder: &Path,
        include_session: bool,
        source: CaptureSource,
        text: &CaptureFormat,
    ) -> io::Result<()> {
        if folder.file_name().is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "no folder name",
            ));
        }
        let session = self.session.as_mut().filter(|_| include_session);
        // before anything is written: never over the session backup's own files
        if let Some(session) = &session {
            refuse_backup_file(&folder_file(folder, "raw"), &session.raw_path)?;
        }
        let started = session.as_ref().map_or_else(Local::now, |s| s.started);
        let mut full = RecordingWriter::create(folder, source.device.as_ref(), text, started)?;
        if let Some(session) = session {
            copy_file(&mut session.text, &mut full.text)?;
            copy_file(&mut session.raw, &mut full.raw)?;
            copy_file(&mut session.times, &mut full.times)?;
            full.raw_len = session.raw_len;
        }
        self.stop();
        let kind = RecordingKind::Full(Box::new(full));
        self.recording = Some(Recording { kind, source });
        Ok(())
    }

    /// What the recording records, while one runs (MAX-173)
    pub fn source(&self) -> Option<&CaptureSource> {
        self.recording.as_ref().map(|recording| &recording.source)
    }

    /// The device changed under a text recording: `marker` on a line of its own, and `source`
    /// is what it records from there (MAX-173). An RX line pending is written when it ends,
    /// after it
    pub fn mark_device(&mut self, marker: &str, source: CaptureSource) {
        if let Some(recording) = &mut self.recording
            && let RecordingKind::Text { file, .. } = &mut recording.kind
        {
            let _ = writeln!(file.writer, "{marker}");
            recording.source = source;
        }
    }

    /// Starts the session backup in a folder of its own in `dir` (MAX-175), named after now and
    /// the port, its text written as `text` says, with its meta file for a later restore, then
    /// removes the oldest sessions there past `limits` (at least the new one stays)
    pub fn start_session(
        &mut self,
        dir: &Path,
        device: &CaptureDevice,
        text: &CaptureFormat,
        limits: SessionLimits,
    ) -> io::Result<()> {
        let now = Local::now();
        // to the millisecond: a relaunch within the second must not append to the last
        // session and overwrite its meta (MAX-148)
        let stem = format!(
            "{}_{}",
            now.format("%Y%m%d_%H%M%S%3f"),
            session_tag(&device.address)
        );
        let session = RecordingWriter::create(&dir.join(&stem), Some(device), text, now)?;
        // held while the session runs, so no pruning removes it, from this maxicom or another
        // one (MAX-148). The OS drops it with the process, even killed. A file system without
        // locks only loses that guard
        let _ = session.raw.writer.get_ref().try_lock();
        self.session = Some(session);
        let limits = SessionLimits {
            keep: limits.keep.max(1),
            ..limits
        };
        prune_sessions(dir, limits, &stem)
    }

    /// The device the session backup's meta says it records (MAX-173)
    pub fn session_device(&self) -> Option<&CaptureDevice> {
        self.session
            .as_ref()
            .and_then(|session| session.device.as_ref())
    }

    pub fn has_session(&self) -> bool {
        self.session.is_some()
    }

    /// The session backup out of the recorder, flushed, which records nothing until it is set
    /// back with `set_session` (MAX-166). Its files stay open, and its lock with them, so no
    /// pruning removes it. The line being cut is dropped at both ends, so a line never mixes
    /// the live bytes and the replayed ones
    pub fn take_session(&mut self) -> Option<RecordingWriter> {
        let mut session = self.session.take();
        if let Some(session) = &mut session {
            session.flush();
        }
        self.lines = RxLineSplitter::default();
        session
    }

    pub fn set_session(&mut self, session: Option<RecordingWriter>) {
        self.session = session;
        self.lines = RxLineSplitter::default();
    }

    /// Stops the recording, if one runs
    pub fn stop(&mut self) {
        match self.recording.take().map(|recording| recording.kind) {
            Some(RecordingKind::Text { mut file, .. }) => {
                let _ = file.flush();
            }
            Some(RecordingKind::Full(mut full)) => full.flush(),
            None => {}
        }
    }

    /// What the recording running saves
    pub fn recording(&self) -> Option<Stream> {
        self.recording
            .as_ref()
            .map(|recording| match recording.kind {
                RecordingKind::Text { .. } => Stream::Text,
                RecordingKind::Full(_) => Stream::Full,
            })
    }

    fn kind(&mut self) -> Option<&mut RecordingKind> {
        self.recording.as_mut().map(|recording| &mut recording.kind)
    }

    fn text_file(&mut self) -> Option<&mut LogFile> {
        match self.kind() {
            Some(RecordingKind::Text { file, .. }) => Some(file),
            _ => None,
        }
    }

    /// The writers of the session backup and of a full recording
    fn writers(&mut self) -> impl Iterator<Item = &mut RecordingWriter> {
        let full = match self.recording.as_mut().map(|recording| &mut recording.kind) {
            Some(RecordingKind::Full(full)) => Some(&mut **full),
            _ => None,
        };
        self.session.iter_mut().chain(full)
    }

    pub fn flush_if_due(&mut self) {
        if let Some(text) = self.text_file() {
            let _ = text.flush_if_due();
        }
        for writer in self.writers() {
            for file in writer.files() {
                let _ = file.flush_if_due();
            }
        }
    }

    /// Records received bytes: as is to the raw files with their time, and each line they end,
    /// without its EOL, to the text files, after its first byte's time when `timestamp_format`
    /// is given
    pub fn record_rx(
        &mut self,
        rx_eol_mode: RxNewlineMode,
        encoding: TextEncoding,
        timestamp_format: Option<&str>,
        bytes: &[u8],
    ) {
        let now = Local::now();
        for writer in self.writers() {
            writer.record_raw(bytes, now);
        }

        self.set_timestamp_format(timestamp_format);
        // taken out while the lines are written, as the writes borrow the recorder
        let mut lines = std::mem::take(&mut self.lines);
        for &b in bytes {
            if lines.push(b, rx_eol_mode, now) {
                let timestamp = lines.take_timestamp();
                if self.writes_text() {
                    let text = encoding.decode_cow(lines.line());
                    self.write_text(timestamp, &text, Some(""), false);
                }
                lines.clear();
            }
        }
        self.lines = lines;
        self.flush_if_due();
    }

    fn writes_text(&self) -> bool {
        self.recording.is_some() || self.session.is_some()
    }

    /// Records sent bytes as text, one line for each line they hold (CR, LF or CRLF ends one),
    /// at the time they are sent: to the text recording after `mark` (`> line`, `None` when
    /// TX isn't recorded), to the session backup and the full recording after their own mark.
    /// Raw stays what is received, to be replayed. An RX line still pending is written when it
    /// ends, after these
    pub fn record_tx(
        &mut self,
        encoding: TextEncoding,
        timestamp_format: Option<&str>,
        mark: Option<&str>,
        bytes: &[u8],
    ) {
        if !self.writes_text() {
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

    /// The text recording's format: `format` parsed when it isn't the one already parsed
    fn set_timestamp_format(&mut self, format: Option<&str>) {
        let Some(RecordingKind::Text { timestamp, .. }) = self.kind() else {
            return;
        };
        match format {
            None => *timestamp = None,
            Some(format) if timestamp.as_ref().is_some_and(|t| t.format == format) => {}
            Some(format) => *timestamp = Some(TimestampFormat::new(format)),
        }
    }

    /// Writes a text line to a text recording after `mark`, and to the session backup and a
    /// full recording after their own mark (a sent line when `tx`), each with its own
    /// timestamp format. A `None` mark leaves that file out. The line is built again only for a
    /// file written another way than the one before
    fn write_text(&mut self, time: DateTime<Local>, text: &str, mark: Option<&str>, tx: bool) {
        // the format and mark `line_buf` was built with
        let mut built: Option<(Option<String>, String)> = None;
        let mut write = |file: &mut LogFile, format: &Option<TimestampFormat>, mark: &str| {
            let key = (format.as_ref().map(|t| t.format.clone()), mark.to_string());
            if built.as_ref() != Some(&key) {
                build_line(&mut self.line_buf, format, time, mark, text);
                built = Some(key);
            }
            let _ = file.writer.write_all(&self.line_buf);
        };
        let (text_file, full) = match self.recording.as_mut().map(|recording| &mut recording.kind) {
            Some(RecordingKind::Text { file, timestamp }) => (Some((file, &*timestamp)), None),
            Some(RecordingKind::Full(full)) => (None, Some(&mut **full)),
            None => (None, None),
        };
        if let (Some((file, timestamp)), Some(mark)) = (text_file, mark) {
            write(file, timestamp, mark);
        }
        for writer in self.session.iter_mut().chain(full) {
            let mark = if tx {
                writer.tx_mark.as_deref()
            } else {
                Some("")
            };
            if let Some(mark) = mark {
                write(&mut writer.text, &writer.timestamp, mark);
            }
        }
    }
}

impl RecordingWriter {
    /// The `.log`, `.raw` and `.times` files in `folder`, created with it, and the meta when
    /// `device` is known, started at `started`, the text written as `text` says
    fn create(
        folder: &Path,
        device: Option<&CaptureDevice>,
        text: &CaptureFormat,
        started: DateTime<Local>,
    ) -> io::Result<Self> {
        let path = |extension: &str| folder_file(folder, extension);
        let (text_path, raw_path) = (path("log"), path("raw"));
        create_parent_dirs(&raw_path)?;
        if let Some(device) = device {
            RecordingMeta::new(started, device.clone(), text.clone())
                .write(&RecordingMeta::path_of(&raw_path))?;
        }
        Ok(Self {
            text: LogFile::open(&text_path)?,
            raw: LogFile::open(&raw_path)?,
            times: LogFile::open(&path("times"))?,
            raw_len: 0,
            text_path,
            raw_path,
            timestamp: text.timestamp.as_deref().map(TimestampFormat::new),
            tx_mark: text.tx_mark.clone(),
            started,
            device: device.cloned(),
        })
    }

    /// The bytes received at `time`: the time first, so a crash between the writes leaves
    /// bytes without a time, not a time past the bytes (MAX-175)
    fn record_raw(&mut self, bytes: &[u8], time: DateTime<Local>) {
        let line = times_line(self.raw_len, time);
        let _ = self.times.writer.write_all(line.as_bytes());
        let _ = self.raw.writer.write_all(bytes);
        self.raw_len += bytes.len() as u64;
    }

    fn files(&mut self) -> [&mut LogFile; 3] {
        [&mut self.text, &mut self.raw, &mut self.times]
    }

    fn flush(&mut self) {
        for file in self.files() {
            let _ = file.flush();
        }
    }

    /// Its `.raw`: the session the browser marks as this maxicom's own
    pub fn raw_path(&self) -> &Path {
        &self.raw_path
    }
}

/// Refused when `path` is the session backup's file at `backup`: copying a file into itself
/// would never end
fn refuse_backup_file(path: &Path, backup: &Path) -> io::Result<()> {
    let same = fs::canonicalize(path).ok() == Some(fs::canonicalize(backup)?);
    if same {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "this is the session backup file",
        ));
    }
    Ok(())
}

/// What `from` holds so far, appended to `to`, read through `from`'s own handle: on Windows the
/// session `.raw` lock blocks reads from any other one. Appends still go to the end, wherever
/// this leaves the position
fn copy_file(from: &mut LogFile, to: &mut LogFile) -> io::Result<()> {
    from.flush()?;
    let mut reader = from.writer.get_ref();
    reader.seek(SeekFrom::Start(0))?;
    io::copy(&mut reader, &mut to.writer)?;
    Ok(())
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
    use crate::capture::tests::{device, record, session_paths, source, temp_dir};

    /// A fresh temp path for a recording named `name`: a text file, or a full recording's
    /// folder. Apart from `temp_dir(name)`, which a test may use for its session backup
    fn temp_log_path(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("maxicom_test_{}_{name}_rec", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    /// The file a recording of `stream` at `path` holds what it received in: the text file,
    /// or the full recording's `.raw`
    fn data_path(stream: Stream, path: &Path) -> PathBuf {
        match stream {
            Stream::Text => path.to_path_buf(),
            Stream::Full => folder_file(path, "raw"),
        }
    }

    /// Starts the stream to a fresh temp file or folder, gives the file it records in.
    fn start(recorder: &mut Recorder, stream: Stream, name: &str) -> PathBuf {
        let path = temp_log_path(name);
        let source = source("port");
        match stream {
            Stream::Text => recorder.start_text(&path, false, source),
            Stream::Full => recorder.start_full(&path, false, source, &CaptureFormat::default()),
        }
        .unwrap();
        data_path(stream, &path)
    }

    /// Reads the stream's file after stopping it, and removes it (a full recording's folder).
    fn stop_and_read(recorder: &mut Recorder, stream: Stream, path: &Path) -> Vec<u8> {
        recorder.stop();
        let bytes = std::fs::read(path).unwrap();
        match stream {
            Stream::Text => std::fs::remove_file(path).unwrap(),
            Stream::Full => std::fs::remove_dir_all(path.parent().unwrap()).unwrap(),
        }
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
        let path = start(&mut recorder, Stream::Full, "dropped");
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

    /// A full recording's raw gets the bytes as received, its text gets them decoded.
    /// How: Latin1 E9 and an escape sequence, a full recording on.
    #[test]
    fn raw_is_received_bytes_and_text_is_decoded() {
        let mut recorder = Recorder::default();
        let raw = start(&mut recorder, Stream::Full, "decoded");
        recorder.record_rx(
            RxNewlineMode::Auto,
            TextEncoding::Latin1,
            None,
            b"\xE9\x1b[31mred\r\n",
        );
        recorder.stop();
        let text = fs::read(raw.with_extension("log")).unwrap();
        assert_eq!(text, "é\x1b[31mred\n".as_bytes());
        assert_eq!(
            stop_and_read(&mut recorder, Stream::Full, &raw),
            b"\xE9\x1b[31mred\r\n"
        );
    }

    /// MAX-175: one recording at a time: a text recording started mid-line gets the whole
    /// line, and a full one started next takes its place.
    /// How: "ab" with nothing on, text on, "c\n", full on, "d\n".
    #[test]
    fn one_recording_and_text_gets_the_whole_line() {
        let mut recorder = Recorder::default();
        record(&mut recorder, b"ab");
        let text = start(&mut recorder, Stream::Text, "apart_text");
        assert_eq!(recorder.recording(), Some(Stream::Text));
        record(&mut recorder, b"c\n");
        let raw = start(&mut recorder, Stream::Full, "apart_raw");
        assert_eq!(recorder.recording(), Some(Stream::Full));
        record(&mut recorder, b"d\n");
        assert_eq!(fs::read(&text).unwrap(), b"abc\n");
        let _ = fs::remove_file(&text);
        assert_eq!(stop_and_read(&mut recorder, Stream::Full, &raw), b"d\n");
        assert_eq!(recorder.recording(), None);
    }

    /// MAX-147: the session backup gets everything from its start, text and raw, whatever
    /// records, and a recording can start with its content.
    /// How: backup in a temp folder, "a\n" received, a text recording including the session,
    /// "b\n" received, then the backup files read back.
    #[test]
    fn session_records_apart_and_a_recording_can_start_with_it() {
        let dir = temp_dir("session");
        let mut recorder = Recorder::default();
        recorder
            .start_session(
                &dir,
                &device("port"),
                &CaptureFormat::default(),
                SessionLimits::keep(20),
            )
            .unwrap();
        assert!(recorder.has_session() && recorder.recording().is_none());
        record(&mut recorder, b"a\n");

        let path = dir.join("mine.log");
        recorder.start_text(&path, true, source("port")).unwrap();
        record(&mut recorder, b"b\n");
        assert_eq!(stop_and_read(&mut recorder, Stream::Text, &path), b"a\nb\n");

        drop(recorder);
        let files = session_paths(&dir);
        assert_eq!(files.len(), 4);
        assert!(files[0].to_string_lossy().ends_with("_port.log"));
        assert_eq!(fs::read(&files[0]).unwrap(), b"a\nb\n");
        assert!(files[1].to_string_lossy().ends_with("_port.meta"));
        assert_eq!(fs::read(&files[2]).unwrap(), b"a\nb\n");
        assert!(files[3].to_string_lossy().ends_with("_port.times"));
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-166: a paused session backup records nothing, a user recording still does, and the
    /// session backup goes on in the same files once resumed; the line cut at the pause is dropped.
    /// How: "a\n" and half a line received, the session backup paused, a text recording started,
    /// "b\n" received, the session backup resumed, "c\n" received, then the files read back.
    #[test]
    fn paused_session_records_nothing() {
        let dir = temp_dir("session_paused");
        let mut recorder = Recorder::default();
        recorder
            .start_session(
                &dir,
                &device("port"),
                &CaptureFormat::default(),
                SessionLimits::keep(20),
            )
            .unwrap();
        record(&mut recorder, b"a\nhal");
        let session = recorder.take_session();
        assert!(session.is_some() && !recorder.has_session());
        let path = dir.join("mine.log");
        recorder.start_text(&path, true, source("port")).unwrap();
        record(&mut recorder, b"b\n");
        recorder.set_session(session);
        assert!(recorder.has_session());
        record(&mut recorder, b"c\n");
        assert_eq!(stop_and_read(&mut recorder, Stream::Text, &path), b"b\nc\n");

        drop(recorder);
        let read = |extension: &str| {
            let entry = session_paths(&dir)
                .into_iter()
                .find(|path| path.to_string_lossy().ends_with(extension))
                .unwrap();
            fs::read(entry).unwrap()
        };
        assert_eq!(read("_port.log"), b"a\nc\n");
        assert_eq!(read("_port.raw"), b"a\nhalc\n");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A full recording can start with the backup's `.raw` while the session backup holds its
    /// lock (Windows blocks reads from another handle), and the backup still appends after it.
    /// MAX-175: its `.times` and `.log` start with the session's too, its offsets going on
    /// from there.
    /// How: "a\n" received, a full recording including the session, "b\n" received.
    #[test]
    fn full_recording_starts_with_the_locked_backup() {
        let dir = temp_dir("session_raw");
        let mut recorder = Recorder::default();
        recorder
            .start_session(
                &dir,
                &device("port"),
                &CaptureFormat::default(),
                SessionLimits::keep(20),
            )
            .unwrap();
        record(&mut recorder, b"a\n");

        let mine = temp_log_path("mine");
        let text = CaptureFormat::default();
        recorder
            .start_full(&mine, true, source("port"), &text)
            .unwrap();
        record(&mut recorder, b"b\n");
        recorder.stop();
        let read = |extension: &str| {
            fs::read_to_string(data_path(Stream::Full, &mine).with_extension(extension))
        };
        assert_eq!(read("raw").unwrap(), "a\nb\n");
        assert_eq!(read("log").unwrap(), "a\nb\n");
        let times = read("times").unwrap();
        let offsets: Vec<&str> = times
            .lines()
            .map(|l| l.split(' ').next().unwrap())
            .collect();
        assert_eq!(offsets, ["0", "2"], "{times:?}");
        // no device in the source: no meta
        assert!(read("meta").is_err());
        let _ = fs::remove_dir_all(&mine);

        drop(recorder);
        let backup = session_paths(&dir)
            .into_iter()
            .find(|path| path.to_string_lossy().ends_with("_port.raw"))
            .unwrap();
        assert_eq!(fs::read(backup).unwrap(), b"a\nb\n");
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-175: the session backup writes where each received chunk starts in its `.raw` and
    /// when, to its `.times`; a paused session writes none.
    /// How: a session backup, 2 chunks received, paused for 1, then 1 more.
    #[test]
    fn session_writes_the_times_of_its_chunks() {
        let dir = temp_dir("times");
        let mut recorder = Recorder::default();
        recorder
            .start_session(
                &dir,
                &device("port"),
                &CaptureFormat::default(),
                SessionLimits::keep(20),
            )
            .unwrap();
        record(&mut recorder, b"ab");
        record(&mut recorder, b"cde");
        let session = recorder.take_session();
        record(&mut recorder, b"replayed");
        recorder.set_session(session);
        record(&mut recorder, b"f");
        drop(recorder);
        let times = session_paths(&dir)
            .into_iter()
            .find(|path| path.extension().is_some_and(|ext| ext == "times"))
            .unwrap();
        let text = fs::read_to_string(times).unwrap();
        let offsets: Vec<&str> = text.lines().map(|l| l.split(' ').next().unwrap()).collect();
        assert_eq!(offsets, ["0", "2", "5"], "{text:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-147: sent lines go to the text files (a full recording's, the backup's) as
    /// `> line`, raw files get none of them.
    /// How: a full recording and the backup on, "rx\n" received, then "a\r\nb\n\r\n" and a
    /// lone ^C sent.
    #[test]
    fn tx_lines_go_to_text_files_only() {
        let dir = temp_dir("tx");
        let mut recorder = Recorder::default();
        let text = CaptureFormat {
            timestamp: None,
            tx_mark: Some("> ".to_string()),
        };
        recorder
            .start_session(&dir, &device("port"), &text, SessionLimits::keep(20))
            .unwrap();
        let full = temp_log_path("tx_full");
        recorder
            .start_full(&full, false, source("port"), &text)
            .unwrap();
        let raw = data_path(Stream::Full, &full);
        record(&mut recorder, b"rx\n");
        recorder.record_tx(TextEncoding::Utf8, None, Some("> "), b"a\r\nb\n\r\n");
        recorder.record_tx(TextEncoding::Utf8, None, Some("> "), b"\x03");

        let expected = b"rx\n> a\n> b\n> \n> \x03\n";
        recorder.stop();
        assert_eq!(fs::read(raw.with_extension("log")).unwrap(), expected);
        assert_eq!(stop_and_read(&mut recorder, Stream::Full, &raw), b"rx\n");
        drop(recorder);
        let backup = session_paths(&dir)
            .into_iter()
            .find(|path| path.extension().is_some_and(|ext| ext == "log"))
            .unwrap();
        assert_eq!(fs::read(backup).unwrap(), expected);
        let _ = fs::remove_dir_all(&dir);
    }

    /// MAX-148: the session backup keeps the text format it started with, whatever the recording
    /// uses later, and records sent lines only when it started with a mark.
    /// How: the session backup with "[j]" and "J ", the recording with "[r]" and "R ", then a TX
    /// with the recording's TX off; a second session backup started without a mark.
    #[test]
    fn session_keeps_its_text_format() {
        let dir = temp_dir("session_format");
        let mut recorder = Recorder::default();
        let text = CaptureFormat {
            timestamp: Some("[j]".to_string()),
            tx_mark: Some("J ".to_string()),
        };
        recorder
            .start_session(&dir, &device("a"), &text, SessionLimits::keep(20))
            .unwrap();
        let rec = start(&mut recorder, Stream::Text, "session_format");
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
            .start_session(
                &dir,
                &device("b"),
                &CaptureFormat::default(),
                SessionLimits::keep(20),
            )
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
        let path = dir.join("a").join("b").join("x");
        let mut recorder = Recorder::default();
        let text = CaptureFormat::default();
        recorder
            .start_full(&path, false, source("port"), &text)
            .unwrap();
        record(&mut recorder, b"x");
        let raw = data_path(Stream::Full, &path);
        assert_eq!(stop_and_read(&mut recorder, Stream::Full, &raw), b"x");
        let _ = fs::remove_dir_all(&dir);
    }
}
