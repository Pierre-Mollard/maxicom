use super::restore::times_line;
use super::session::{
    CaptureDevice, CaptureFormat, CaptureSource, PruneReport, RecordingMeta, STEM_TIME,
    SessionLimits, TerminalSize, folder_file, prune_sessions, session_start, session_tag,
};
use crate::app::types::RxLineSplitter;
use crate::configs::TextEncoding;
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
/// was received (`times`, at `raw_len`), and the device and terminal size (`meta`, none when
/// the device is unknown). The session backup runs apart from the
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
    meta: Option<RecordingMeta>,
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
    /// removes the oldest sessions there past `limits` (at least the new one stays) and says
    /// what went (MAX-202)
    pub fn start_session(
        &mut self,
        dir: &Path,
        device: &CaptureDevice,
        text: &CaptureFormat,
        limits: SessionLimits,
    ) -> io::Result<PruneReport> {
        let now = session_start(dir, Local::now());
        // to the millisecond: a relaunch within the second must not append to the last
        // session and overwrite its meta (MAX-148)
        let stem = format!("{}_{}", now.format(STEM_TIME), session_tag(&device.address));
        let session = RecordingWriter::create(&dir.join(&stem), Some(device), text, now)?;
        // held while the session runs, so no pruning removes it, from this maxicom or another
        // one (MAX-148). The OS drops it with the process, even killed. A file system without
        // locks only loses that guard
        let _ = session.raw.writer.get_ref().try_lock();
        self.session = Some(session);
        prune_sessions(dir, limits, &stem)
    }

    /// The device the session backup's meta says it records (MAX-173)
    pub fn session_device(&self) -> Option<&CaptureDevice> {
        self.session
            .as_ref()
            .and_then(|session| session.meta.as_ref())
            .map(|meta| &meta.device)
    }

    /// The terminal grid is now `size`: the metas of the session backup and of a full
    /// recording say it (MAX-178)
    pub fn set_terminal_size(&mut self, size: TerminalSize) {
        let full = match self.recording.as_mut().map(|recording| &mut recording.kind) {
            Some(RecordingKind::Full(full)) => Some(&mut **full),
            _ => None,
        };
        for writer in self.session.iter_mut().chain(full) {
            writer.set_terminal_size(size);
        }
    }

    /// The session backup, for what changes in its meta (MAX-213)
    pub fn session_mut(&mut self) -> Option<&mut RecordingWriter> {
        self.session.as_mut()
    }

    /// The `.raw` of the session backup running, none without one
    pub fn session_raw(&self) -> Option<&Path> {
        self.session.as_ref().map(RecordingWriter::raw_path)
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
    /// The raw bytes to the raw writers, their lines to the text ones, a lone CR ending one
    /// with `add_lf` (MAX-181)
    pub fn record_rx(
        &mut self,
        add_lf: bool,
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
            if lines.ends_at_cr(b, add_lf) {
                self.write_rx_line(&mut lines, encoding);
            }
            if lines.push(b, now) {
                self.write_rx_line(&mut lines, encoding);
            }
        }
        self.lines = lines;
        self.flush_if_due();
    }

    /// RX that isn't text, to the raw streams only: a transfer protocol's answers (MAX-162)
    pub fn record_raw_rx(&mut self, bytes: &[u8]) {
        let now = Local::now();
        for writer in self.writers() {
            writer.record_raw(bytes, now);
        }
        self.flush_if_due();
    }

    fn write_rx_line(&mut self, lines: &mut RxLineSplitter, encoding: TextEncoding) {
        let timestamp = lines.take_timestamp();
        if self.writes_text() {
            let text = encoding.decode_cow(lines.line());
            self.write_text(timestamp, &text, Some(""), false);
        }
        lines.clear();
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
        let meta = device.map(|device| RecordingMeta::new(started, device.clone(), text.clone()));
        if let Some(meta) = &meta {
            meta.write(&RecordingMeta::path_of(&raw_path))?;
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
            meta,
        })
    }

    /// The meta written again when the terminal grid is now `size` (MAX-178)
    fn set_terminal_size(&mut self, size: TerminalSize) {
        let Some(meta) = self
            .meta
            .as_mut()
            .filter(|meta| meta.terminal != Some(size))
        else {
            return;
        };
        meta.terminal = Some(size);
        let _ = meta.write(&RecordingMeta::path_of(&self.raw_path));
    }

    /// The meta written again with `name` (MAX-213)
    pub fn set_name(&mut self, name: Option<String>) -> io::Result<()> {
        let Some(meta) = self.meta.as_mut() else {
            return Err(io::Error::other("the session has no meta file"));
        };
        meta.name = name;
        meta.write(&RecordingMeta::path_of(&self.raw_path))
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
#[path = "../tests/capture/recorder.rs"]
mod tests;
