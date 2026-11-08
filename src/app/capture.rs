//! The app's captures (MAX-172): the session backup it starts and switches, the past
//! session it restores at startup, the recordings the user starts, and the session browser.
//! `capture::Recorder` writes the files
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};

use super::App;
use crate::{
    app::{LineMode, terminal_mode::add_newlines, types::LogEntry},
    capture::{
        CaptureDevice, CaptureFormat, CaptureSource, PruneReport, RESTORE_RAW_TAIL,
        RecordingWriter, Session, Stream, TerminalSize, TimedChunk, find_last_session,
        group_separator, read_tail_bytes, read_times, restore_separator, timed_chunks,
    },
    configs::{Mode, settings::RestoreMode},
    overlays::{
        ErrorDialog, FilePathDialog, Overlay, SessionBrowser,
        sessions::{sessions as session_count, size},
    },
    paths::expand_home,
};

/// The most kept while the restore dialog asks: past it, the dialog closes without a restore
const HELD_RX_MAX: usize = 4 << 20;
/// What a group replay reads for each line line mode holds and each row terminal mode keeps:
/// past that it would be dropped anyway, so a big group costs no more than a small one (MAX-202)
const GROUP_LINE_BYTES: u64 = 256;
const GROUP_TERMINAL_BYTES_PER_ROW: u64 = 512;

impl App {
    /// Starts the session backup in `dir` when `sessions.enabled` is on. Without it, a toast
    /// says so and the app runs on (MAX-147)
    pub fn start_session(&mut self, dir: Option<PathBuf>) {
        // the browser lists the past sessions even with the backup off
        self.sessions_dir.clone_from(&dir);
        let sessions = &self.settings.sessions;
        if !sessions.enabled {
            return;
        }
        let device = self.session_device();
        let text = self.capture_format();
        let result = match dir {
            Some(dir) => self
                .recorder
                .start_session(&dir, &device, &text, sessions.limits()),
            None => Err(std::io::Error::other("no cache folder")),
        };
        match result {
            Ok(report) if report.removed > 0 => self.notify_info(pruned_message(&report)),
            Ok(_) => {}
            Err(err) => self.notify_warn(format!("Session backup off: {err}")),
        }
    }

    /// A new session backup: for the device switched to, after its restore (MAX-12, MAX-187),
    /// and for new line settings on the same port (MAX-173)
    pub fn restart_session(&mut self) {
        self.start_session(self.sessions_dir.clone());
    }

    /// `--restore` / `--no-restore`, kept for the devices switched to (MAX-187)
    pub fn set_restore_flag(&mut self, restore: Option<bool>) {
        self.restore_flag = restore;
    }

    /// The device switched to starts as at start on it (MAX-187): a clean screen, its last
    /// session restored, then its own session backup
    pub fn switch_session(&mut self) {
        self.clear_screen();
        self.restored_session = None;
        let dir = self.sessions_dir.clone();
        self.restore_last_session(dir.as_deref(), self.restore_flag);
        self.restart_session();
    }

    /// The session backup set aside while a replay replaces the live link (MAX-166): out of the
    /// recorder, its files kept open and locked, so no pruning removes it, and nothing recorded
    /// until `resume_session`
    pub fn pause_session(&mut self) -> Option<RecordingWriter> {
        self.recorder.take_session()
    }

    /// The session backup `pause_session` set aside records again, the live link back
    pub fn resume_session(&mut self, session: Option<RecordingWriter>) {
        self.recorder.set_session(session);
    }

    /// The captures follow the device, so their meta stays true (MAX-173): a new session backup
    /// when the line settings or the encoding changed since it started; when the recording's
    /// source changed (another address too), a full recording stops, a text recording gets a
    /// marker line. The session's address is `opened`'s: a new port gets its session once it
    /// is open. Their metas say the terminal grid's size (MAX-178). Run before the received
    /// bytes are recorded, and on each tick for a change with nothing received after it
    pub fn follow_device(&mut self) {
        let changed =
            (self.recorder.source()).is_some_and(|source| !self.is_current_source(source));
        match self.recorder.recording().filter(|_| changed) {
            Some(Stream::Full) => {
                self.recorder.stop();
                self.notify_warn("Full recording stopped: the device changed");
            }
            Some(Stream::Text) => {
                let source = self.capture_source();
                let marker = source.marker(self.settings.encoding);
                self.recorder.mark_device(&marker, source);
            }
            None => {}
        }

        let config = self.link.config();
        let changed = self.recorder.session_device().is_some_and(|device| {
            device.address == config.address && !device.same_setup(config, self.settings.encoding)
        });
        if changed {
            // ended first: a new one that fails to start leaves none, not the stale one
            drop(self.recorder.take_session());
            self.restart_session();
        }

        let (rows, cols) = self.terminal_mode.get_vt100_size();
        self.recorder.set_terminal_size(TerminalSize { rows, cols });
    }

    /// What a recording started now records: the link's address and device, a replay's device
    /// the one its meta gave, if any (MAX-173)
    fn capture_source(&self) -> CaptureSource {
        let device = if self.link_kind().keeps_sessions() {
            Some(self.session_device())
        } else {
            self.replay_device().cloned()
        };
        CaptureSource {
            address: self.link.config().address.clone(),
            device,
        }
    }

    /// Whether the link still gives what `source` says: the same address, and for a live link
    /// the same line settings and encoding (a replay's device comes from its file)
    fn is_current_source(&self, source: &CaptureSource) -> bool {
        let config = self.link.config();
        let same_setup = |device: &CaptureDevice| {
            !self.link_kind().keeps_sessions() || device.same_setup(config, self.settings.encoding)
        };
        source.address == config.address && source.device.as_ref().is_none_or(same_setup)
    }

    /// How a session's or a full recording's text is written: as the capture settings say when
    /// it starts, reloads aside
    fn capture_format(&self) -> CaptureFormat {
        let capture = &self.settings.capture;
        CaptureFormat {
            timestamp: capture
                .timestamp
                .then(|| self.settings.timestamp_format.clone()),
            tx_mark: capture.record_tx.then(|| capture.tx_mark.clone()),
        }
    }

    /// What this session runs on, for its backup and to find a past one to restore
    fn session_device(&self) -> CaptureDevice {
        CaptureDevice::new(self.link.config(), self.settings.encoding)
    }

    /// Restores the last session on this device found in `dir` when `restore` (`--restore` /
    /// `--no-restore`) or else `sessions.restore` says so, with a toast either way. With `ask`,
    /// a dialog asks first, and what is received meanwhile waits for the answer. To call
    /// before `start_session`, whose pruning could remove it (MAX-148)
    pub fn restore_last_session(&mut self, dir: Option<&Path>, restore: Option<bool>) {
        let sessions = &self.settings.sessions;
        let mode = match restore {
            Some(true) => RestoreMode::Always,
            Some(false) => RestoreMode::Never,
            None => sessions.restore,
        };
        let Some(dir) = dir.filter(|_| mode != RestoreMode::Never) else {
            return;
        };
        let Some(session) = find_last_session(dir, &self.session_device(), sessions.max_age())
        else {
            return;
        };
        if mode == RestoreMode::Ask {
            self.held_rx = Some(Vec::new());
            self.open_overlay(Overlay::RestoreSession(Box::new(session)));
        } else {
            self.restore_now(&session);
        }
    }

    /// Keeps `chunk` while the restore dialog is open, so the restored data comes before it.
    /// When the dialog was replaced by another popup, or too much came in, what was kept is
    /// fed without a restore, then `chunk` is not kept
    pub fn hold_rx(&mut self, chunk: &[u8]) -> bool {
        let Some(held) = &mut self.held_rx else {
            return false;
        };
        let asking = matches!(self.overlay, Some(Overlay::RestoreSession(_)));
        if asking && held.len() + chunk.len() <= HELD_RX_MAX {
            held.extend_from_slice(chunk);
            return true;
        }
        if asking {
            self.close_overlay();
            self.notify_warn("Session not restored: too much data came in while asking");
        }
        self.release_held_rx();
        false
    }

    pub fn release_held_rx(&mut self) {
        if let Some(held) = self.held_rx.take() {
            self.feed(&held, false);
        }
    }

    /// The answer of the restore dialog: the session first when `yes`, then what came in
    pub fn answer_restore(&mut self, session: &Session, yes: bool) {
        if yes {
            self.restore_now(session);
        }
        self.release_held_rx();
    }

    fn restore_now(&mut self, session: &Session) {
        match self.restore_session(session) {
            Ok(()) => {
                self.restored_session = Some(session.stem.clone());
                self.notify_info(format!("Restored the session of {}", session.time_span()));
            }
            Err(err) => self.notify_warn(format!("Session not restored: {err}")),
        }
    }

    /// Puts a past session in the history of both modes, before any live data (MAX-148): the
    /// end of its raw bytes, cut into lines in line mode as they were live (MAX-175) and
    /// through the terminal at the grid size the session ended on, when its meta says it
    /// (MAX-178), then a separator where the live data starts. A session is its raw bytes and
    /// its meta; its text is for people to read, never read back (MAX-172). Not recorded nor
    /// counted: the new session starts empty
    pub fn restore_session(&mut self, session: &Session) -> std::io::Result<()> {
        // a session from `find_last_session` has both
        let (Some(raw_path), Some(meta)) = (session.raw_path(), &session.meta) else {
            return Err(std::io::Error::other("not a complete session"));
        };
        let (start, raw) = read_tail_bytes(raw_path, RESTORE_RAW_TAIL)?;
        // without its times (a session before MAX-175, a crash), the lines have none
        let times = (session.times_path())
            .and_then(|path| read_times(path, start).ok())
            .unwrap_or_default();
        let chunks = timed_chunks(start, &raw, &times);
        let separator = restore_separator(session);

        let (add_lf, encoding) = (self.settings.add_lf, self.settings.encoding);
        self.line_mode.restore_rx(add_lf, encoding, &chunks);
        // like a `clear`, the separator on the top row (MAX-176)
        self.line_mode.clear_view();
        let separator_line = LogEntry::restored(separator.clone(), None);
        self.line_mode.restore_entries([separator_line]);
        self.restore_terminal(meta.terminal, &chunks, &separator);
        Ok(())
    }

    /// A past session's chunks through the terminal, then `separator`, at the size the session
    /// ended on when it is known. The view's size applies once the UI is up, by the dimension
    /// mode
    fn restore_terminal(
        &mut self,
        size: Option<TerminalSize>,
        chunks: &[TimedChunk],
        separator: &str,
    ) {
        if let Some(size) = size {
            self.terminal_mode.set_vt100_size(size.rows, size.cols);
        }
        let settings = &self.settings;
        let (add_cr, add_lf, encoding) = (settings.add_cr, settings.add_lf, settings.encoding);
        // add CR / add LF as live, through all the chunks (MAX-181)
        let mut after_cr = false;
        let shown: Vec<_> = (chunks.iter())
            .map(|chunk| add_newlines(chunk.bytes, add_cr, add_lf, &mut after_cr))
            .collect();
        let shown_chunks: Vec<TimedChunk> = (chunks.iter().zip(&shown))
            .map(|(chunk, bytes)| TimedChunk {
                bytes,
                time: chunk.time,
            })
            .collect();
        (self.terminal_mode).restore(encoding, &shown_chunks, separator);
    }

    /// Loads a group of sessions, oldest first, for a replay (MAX-202): the end of the group that
    /// line mode can hold on a new history of `lines`, then the end that terminal mode can, with
    /// a separator after each session. Only that is read, however big the group. One mode after
    /// the other: fed in turns, they fragment the heap, as measured in MAX-214. A toast says what
    /// doesn't fit
    pub fn load_group(&mut self, sessions: &[Session], lines: usize) {
        let send_history = self.settings.line_mode.send_history_size;
        self.line_mode = LineMode::new(lines, send_history);
        let (add_lf, encoding) = (self.settings.add_lf, self.settings.encoding);
        let tail = group_tail(sessions, lines as u64 * GROUP_LINE_BYTES);
        let mut ends = Vec::new();
        for &(session, take) in &tail {
            let read = (session.raw_path()).and_then(|path| read_tail_bytes(path, take).ok());
            let Some((start, raw)) = read else {
                continue;
            };
            let times = session_times(session, start);
            self.line_mode
                .restore_rx(add_lf, encoding, &timed_chunks(start, &raw, &times));
            let separator = LogEntry::restored(group_separator(session), None);
            self.line_mode.restore_entries([separator]);
            ends.push(self.line_mode.history().len() + self.line_mode.dropped_lines());
        }
        let dropped = self.line_mode.dropped_lines();
        let rows = self.settings.terminal_mode.history_size;
        let terminal_tail = group_tail(sessions, rows as u64 * GROUP_TERMINAL_BYTES_PER_ROW);
        let fit = GroupFit {
            sessions: sessions.len(),
            unreadable: tail.len() - ends.len(),
            kept: ends.iter().filter(|&&end| end > dropped).count(),
            cut: dropped > 0 || tail_is_cut(sessions, &tail),
            lines,
            terminal_cut: rows < ends.last().copied().unwrap_or(0)
                || tail_is_cut(sessions, &terminal_tail),
            terminal_rows: rows,
        };

        for &(session, take) in &terminal_tail {
            let read = (session.raw_path()).and_then(|path| read_tail_bytes(path, take).ok());
            let Some((start, raw)) = read else {
                continue;
            };
            let times = session_times(session, start);
            let chunks = timed_chunks(start, &raw, &times);
            let size = session.meta.as_ref().and_then(|meta| meta.terminal);
            self.restore_terminal(size, &chunks, &group_separator(session));
        }
        // the end of the group in view, not the empty screen under the last separator
        let (screen_rows, _) = self.terminal_mode.get_vt100_size();
        self.terminal_mode
            .scroll_up(usize::from(screen_rows.saturating_sub(1)));

        let (message, warn) = fit.message();
        if warn {
            self.notify_warn(message);
        } else {
            self.notify_info(message);
        }
    }

    /// Stops the recording, or opens the recording dialog on the mode's stream when none runs
    /// (MAX-175: one recording at a time, the dialog picks text or full)
    pub fn toggle_recording(&mut self) {
        match self.recorder.recording() {
            Some(stream) => {
                self.recorder.stop();
                self.notify_info(format!("Stopped recording {}", stream.as_str()));
            }
            None => self.open_capture_dialog(self.mode_stream()),
        }
    }

    /// The dialog on `stream`, the name made from `[capture]` now
    fn open_capture_dialog(&mut self, stream: Stream) {
        let capture = &self.settings.capture;
        let path = capture
            .path_for(&capture.name)
            .to_string_lossy()
            .into_owned();
        let mut dialog = FilePathDialog::new(stream, path);
        if self.recorder.has_session() {
            dialog = dialog.with_session_backup();
        }
        self.open_overlay(Overlay::FilePath(dialog));
    }

    /// The stream the recording dialog starts on in each mode: text in line mode, full in terminal
    /// mode
    fn mode_stream(&self) -> Stream {
        match self.settings.mode {
            Mode::Line => Stream::Text,
            Mode::Terminal => Stream::Full,
        }
    }

    /// Records what was sent to the text streams, in line mode only: in terminal mode the keys
    /// go one by one and the remote echoes them, so the received text has them (MAX-147)
    pub fn record_tx(&mut self, bytes: &[u8]) {
        let settings = &self.settings;
        if settings.mode != Mode::Line {
            return;
        }
        let capture = &settings.capture;
        let timestamp_format = capture
            .timestamp
            .then_some(settings.timestamp_format.as_str());
        // the session backup records them as it started, whatever `record_tx` is now (MAX-148)
        let mark = capture.record_tx.then_some(capture.tx_mark.as_str());
        self.recorder
            .record_tx(settings.encoding, timestamp_format, mark, bytes);
    }

    /// Starts recording `stream` under the name `path`, as the recording dialog asked
    /// (MAX-175): text to `<path>.log`, a full recording to the folder `<path>`. A full one
    /// gets a meta file when its device is known, so its replay decodes and shows its line
    /// settings as a session's does: the live link's, or while replaying the replayed
    /// session's (a plain file's isn't). A failure opens the error popup
    pub fn start_capture(&mut self, path: &Path, stream: Stream, include_session: bool) {
        let path = expand_home(path);
        match stream {
            Stream::Text => {
                let mut file = path.into_os_string();
                file.push(Stream::Text.suffix());
                self.start_text_capture(Path::new(&file), include_session);
            }
            Stream::Full => {
                let source = self.capture_source();
                let text = self.capture_format();
                let result = (self.recorder).start_full(&path, include_session, source, &text);
                self.report_capture_start(result, stream);
            }
        }
    }

    /// Starts the text recording in the file `file` as it's named, as minicom's `-C` does
    /// (MAX-181)
    pub fn start_text_capture(&mut self, file: &Path, include_session: bool) {
        let file = expand_home(file);
        let source = self.capture_source();
        let result = (self.recorder).start_text(&file, include_session, source);
        self.report_capture_start(result, Stream::Text);
    }

    fn report_capture_start(&mut self, result: std::io::Result<()>, stream: Stream) {
        match result {
            Ok(()) => {
                self.notify_success(format!("Started recording {}", stream.as_str()));
            }
            Err(e) => {
                self.open_overlay(Overlay::Error(
                    ErrorDialog::new("Capture Failed", "Could not open file for writing")
                        .with_details(e),
                ));
            }
        }
    }

    /// The session browser on the session folder, the session restored into the history marked
    /// (MAX-148, MAX-202)
    pub fn open_session_browser(&mut self) {
        let Some(dir) = self.sessions_dir.clone() else {
            self.notify_warn("No session folder");
            return;
        };
        let max_size = self.settings.sessions.limits().max_size;
        let mut browser = SessionBrowser::new(dir, self.restored_session.clone())
            .with_max_size(max_size)
            .with_group_by(self.settings.sessions.group_by.clone());
        if let Some(replaced) = self.replaced_link() {
            let raw = replaced
                .session
                .as_ref()
                .map(|j| j.raw_path().to_path_buf());
            browser = browser.with_live(raw);
        }
        self.open_overlay(Overlay::Sessions(Box::new(browser)));
    }
}

/// The end of a group that fits in `budget` bytes, oldest first: each session with how much of
/// its `.raw` to read (MAX-202)
fn group_tail(sessions: &[Session], budget: u64) -> Vec<(&Session, u64)> {
    let mut left = budget;
    let mut tail = Vec::new();
    for session in sessions.iter().rev() {
        if left == 0 {
            break;
        }
        let take = session.raw_len.min(left);
        left -= take;
        tail.push((session, take));
    }
    tail.reverse();
    tail
}

/// Whether `tail` leaves out a session of `sessions`, or the start of its oldest
fn tail_is_cut(sessions: &[Session], tail: &[(&Session, u64)]) -> bool {
    let whole_oldest = tail
        .first()
        .is_none_or(|(oldest, take)| *take == oldest.raw_len);
    tail.len() < sessions.len() || !whole_oldest
}

/// When a session's chunks from raw offset `start` on were received, none when it isn't known
fn session_times(session: &Session, start: u64) -> Vec<(u64, DateTime<Local>)> {
    (session.times_path())
        .and_then(|path| read_times(path, start).ok())
        .unwrap_or_default()
}

/// What a group replay could load (MAX-202): `sessions` asked, `unreadable` of them not read,
/// `kept` with lines still in line mode's history of `lines`, whether line mode misses anything
/// (`cut`), and whether terminal mode, with its `terminal_rows`, does
struct GroupFit {
    sessions: usize,
    unreadable: usize,
    kept: usize,
    cut: bool,
    lines: usize,
    terminal_cut: bool,
    terminal_rows: usize,
}

impl GroupFit {
    /// The toast, and whether it warns: something is missing from line mode too
    fn message(&self) -> (String, bool) {
        let count = session_count(self.sessions);
        let lines = self.lines;
        let mut message = if !self.cut {
            format!("Loaded {count}")
        } else if self.kept == self.sessions {
            format!("Loaded {count} without the start of the oldest: line mode holds {lines} lines")
        } else {
            format!(
                "Loaded the last {} of {count}: line mode holds {lines} lines",
                self.kept
            )
        };
        if self.unreadable > 0 {
            message.push_str(&format!(", {} unreadable", self.unreadable));
        }
        if self.terminal_cut {
            message.push_str(&format!(
                ". Terminal mode keeps the last {} rows",
                self.terminal_rows
            ));
            if !self.cut {
                message.push_str(", line mode has them all");
            }
        }
        (message, self.cut || self.unreadable > 0)
    }
}

/// The toast for the sessions pruning removed (MAX-202): `Session folder full: removed the 3
/// oldest sessions (up to 2026-10-12, 14MB)`
pub fn pruned_message(report: &PruneReport) -> String {
    let what = match report.removed {
        1 => "the oldest session".to_string(),
        n => format!("the {n} oldest sessions"),
    };
    let up_to = (report.newest_day())
        .map(|day| format!("up to {day}, "))
        .unwrap_or_default();
    format!(
        "Session folder full: removed {what} ({up_to}{})",
        size(report.size)
    )
}

#[cfg(test)]
#[path = "../tests/app/capture.rs"]
pub mod tests;
