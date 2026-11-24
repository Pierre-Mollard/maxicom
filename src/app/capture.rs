//! The app's captures (MAX-172): the session backup it starts and switches, the past
//! session it restores at startup, the recordings the user starts, and the session browser.
//! `capture::Recorder` writes the files
use std::{
    ops::Range,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use jiff::Timestamp;

use super::App;
use crate::{
    app::{LineMode, terminal_mode::add_newlines, types::LogEntry},
    capture::{
        CaptureDevice, CaptureFormat, CaptureSource, PruneReport, RESTORE_RAW_TAIL,
        RecordingWriter, Session, Stream, TerminalSize, TimedChunk, clean_session_name,
        find_last_session, group_separator, read_tail_bytes, read_times, restore_separator,
        session_name_notice, tail_lines_len, timed_chunks, timed_ranges,
    },
    commands::report::{Note, Report},
    configs::{Mode, settings::RestoreMode},
    event::Event,
    notification::ToastLevel,
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
/// Within those, the lines read for each line line mode holds and each row terminal mode keeps:
/// a line makes at least a row, but a full-screen program redraws rows in place, so terminal
/// mode gets twice its rows in lines (MAX-272)
const GROUP_TERMINAL_LINES_PER_ROW: usize = 2;
/// The most a group load feeds at once, so a slice ends soon after its time is up (MAX-272)
const GROUP_PIECE: usize = 64 * 1024;
/// A loop turn's share of a group load, a frame drawn between two (MAX-272)
const GROUP_SLICE: Duration = Duration::from_millis(25);
/// A byte fed to terminal mode costs about this many fed to line mode (measured MAX-272: 51 MB
/// in 1.05 s against 107 MB in 0.24 s), for the gauge and the time left
const TERMINAL_WEIGHT: u64 = 8;

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
            Ok(report) if report.removed > 0 => {
                let message = pruned_message(&report);
                self.emit(Event::SessionsPruned { message });
            }
            Ok(_) => {}
            Err(err) => {
                let error = err.to_string();
                self.emit(Event::BackupOff { error });
            }
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
        // another board's crashes aren't this one's (MAX-220)
        for trigger in &mut self.triggers {
            trigger.hits = 0;
        }
        self.recorder.end_line(self.settings.encoding);
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

    /// The running session named, or its name taken away when `name` leaves nothing: the live
    /// link's, paused while a replay runs. Its recording holds the meta, so it writes it, and a
    /// later write keeps the name (MAX-213). With what to say; why not else
    pub fn name_session(&mut self, name: &str) -> Result<Note, String> {
        let Some(session) = self.live_session_mut() else {
            return Err("no session running".to_string());
        };
        let name = clean_session_name(name);
        (session.set_name(name.clone())).map_err(|err| err.to_string())?;
        let notice = session_name_notice(name.as_deref());
        Ok(Note::new(ToastLevel::Success, notice))
    }

    /// The live link's session backup: the recorder's, or the one paused while a replay runs
    pub fn live_session_mut(&mut self) -> Option<&mut RecordingWriter> {
        let paused = (self.replay.as_mut())
            .and_then(|run| run.replaced.as_deref_mut())
            .and_then(|replaced| replaced.session.as_mut());
        self.recorder.session_mut().or(paused)
    }

    /// The session backup `pause_session` set aside records again, the live link back
    pub fn resume_session(&mut self, session: Option<RecordingWriter>) {
        self.recorder.set_session(session);
    }

    /// The captures follow the device, so their meta stays true (MAX-173): a new session backup
    /// when the line settings or the encoding changed since it started; when the recording's
    /// source changed (another address too), a full recording stops, a text recording gets a
    /// marker line. The session's address is `opened`'s: a new port gets its session once it
    /// is open. Their metas say the terminal grid's size (MAX-178) and the profile stack
    /// (MAX-240). Run before the received
    /// bytes are recorded, and on each tick for a change with nothing received after it
    pub fn follow_device(&mut self) {
        let changed =
            (self.recorder.source()).is_some_and(|source| !self.is_current_source(source));
        match self.recorder.recording().filter(|_| changed) {
            Some(Stream::Full) => {
                self.recorder.stop();
                self.emit(Event::RecordingStopped);
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
        // the stack laid now: the last one of a session is its meta's (MAX-240)
        self.recorder.set_profiles(&self.profiles);
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
            self.emit(Event::RestoreOverflowed);
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
                let span = session.time_span();
                self.emit(Event::SessionRestored { span });
            }
            Err(err) => {
                let error = err.to_string();
                self.emit(Event::RestoreFailed { error });
            }
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
        let trace = self.settings.terminal_mode.byte_trace;
        // its markers come back with its bytes (MAX-253)
        self.sync_unhandled();
        (self.terminal_mode).restore(encoding, &shown_chunks, separator, trace);
    }

    /// Loads a group of sessions, oldest first, for a replay (MAX-202): the end of the group that
    /// line mode can hold on a new history of `lines`, then the end that terminal mode can, with
    /// a separator after each session. Only that is read, however big the group. One mode after
    /// the other: fed in turns, they fragment the heap, as measured in MAX-214. A toast says what
    /// doesn't fit. All at once, for the tests: the app loads it a slice a loop turn
    /// (`load_group_slice`, MAX-272)
    #[cfg(test)]
    pub fn load_group(&mut self, sessions: &[Session], lines: usize) {
        let mut load = self.start_group_load(sessions.to_vec(), lines);
        let later = Instant::now() + Duration::from_secs(3600);
        while !self.step_group_load(&mut load, later) {}
        self.end_group_load(&load);
    }

    /// The group load running, the loading popup's (MAX-272)
    pub fn group_load(&self) -> Option<&GroupLoad> {
        self.replay.as_ref()?.loading.as_deref()
    }

    /// A loop turn's slice of the group being loaded (MAX-272): true when it fed some. Its end
    /// closes the loading popup and says what was loaded
    pub fn load_group_slice(&mut self, now: Instant) -> bool {
        let Some(mut load) = self.replay.as_mut().and_then(|run| run.loading.take()) else {
            return false;
        };
        if !self.step_group_load(&mut load, now + GROUP_SLICE) {
            if let Some(run) = &mut self.replay {
                run.loading = Some(load);
            }
            return true;
        }
        if matches!(self.overlay, Some(Overlay::Loading)) {
            self.close_overlay();
        }
        self.end_group_load(&load);
        true
    }

    /// A group load on a new line mode history of `lines`, nothing read yet (MAX-272)
    pub fn start_group_load(&mut self, sessions: Vec<Session>, lines: usize) -> GroupLoad {
        let send_history = self.settings.line_mode.send_history_size;
        self.line_mode = LineMode::new(lines, send_history);
        let rows = self.settings.terminal_mode.history_size;
        let line_tail = group_tail(&sessions, lines, lines as u64 * GROUP_LINE_BYTES);
        let terminal_tail = group_tail(
            &sessions,
            rows.saturating_mul(GROUP_TERMINAL_LINES_PER_ROW),
            rows as u64 * GROUP_TERMINAL_BYTES_PER_ROW,
        );
        let total = (line_tail.iter().chain(&terminal_tail))
            .map(|&(_, take)| take)
            .sum();
        GroupLoad {
            sessions,
            lines,
            line_tail,
            terminal_tail,
            phase: Mode::Line,
            next: 0,
            feeding: None,
            ends: Vec::new(),
            done: 0,
            total,
            started: Instant::now(),
        }
    }

    /// Feeds `load` piece by piece until `until`, at least one piece: true once it is all
    /// loaded (MAX-272)
    pub fn step_group_load(&mut self, load: &mut GroupLoad, until: Instant) -> bool {
        loop {
            if load.feeding.is_none() && !self.open_group_session(load) {
                return true;
            }
            self.feed_group_piece(load);
            if Instant::now() >= until {
                return false;
            }
        }
    }

    /// The next session of the mode being loaded, line mode's then terminal mode's, read; one
    /// that can't be read is passed. False when none is left
    fn open_group_session(&mut self, load: &mut GroupLoad) -> bool {
        loop {
            let Some(&(index, take)) = load.tail().get(load.next) else {
                if load.phase == Mode::Terminal {
                    return false;
                }
                load.phase = Mode::Terminal;
                load.next = 0;
                continue;
            };
            load.next += 1;
            let session = &load.sessions[index];
            let read = (session.raw_path()).and_then(|path| read_tail_bytes(path, take).ok());
            let Some((start, raw)) = read else {
                load.done += take;
                continue;
            };
            let times = session_times(session, start);
            let chunks = timed_ranges(start, raw.len(), &times);
            if load.phase == Mode::Terminal {
                if let Some(size) = session.meta.as_ref().and_then(|meta| meta.terminal) {
                    self.terminal_mode.set_vt100_size(size.rows, size.cols);
                }
                // its markers come back with its bytes (MAX-253)
                self.sync_unhandled();
            }
            load.feeding = Some(Feeding {
                session: index,
                take,
                done_before: load.done,
                raw,
                chunks,
                chunk: 0,
                at: 0,
                after_cr: false,
            });
            return true;
        }
    }

    /// A piece of the session being fed, at most `GROUP_PIECE` bytes, its end once fed whole
    fn feed_group_piece(&mut self, load: &mut GroupLoad) {
        let Some(feeding) = &mut load.feeding else {
            return;
        };
        if let Some((range, time)) = feeding.chunks.get(feeding.chunk) {
            let from = feeding.at.max(range.start);
            let to = range.end.min(from + GROUP_PIECE);
            let chunk = TimedChunk {
                bytes: &feeding.raw[from..to],
                time: *time,
            };
            let settings = &self.settings;
            let (add_cr, add_lf, encoding) = (settings.add_cr, settings.add_lf, settings.encoding);
            if load.phase == Mode::Terminal {
                // add CR / add LF as live, through all the chunks (MAX-181)
                let shown = add_newlines(chunk.bytes, add_cr, add_lf, &mut feeding.after_cr);
                let chunk = TimedChunk {
                    bytes: &shown,
                    ..chunk
                };
                let trace = settings.terminal_mode.byte_trace;
                self.terminal_mode.feed_restored(encoding, &chunk, trace);
            } else {
                self.line_mode.feed_restored_rx(add_lf, encoding, &chunk);
            }
            feeding.at = to;
            if to == range.end {
                feeding.chunk += 1;
            }
            load.done = feeding.done_before + to as u64;
            if feeding.chunk < feeding.chunks.len() {
                return;
            }
        }

        // the session's end
        let session = &load.sessions[feeding.session];
        load.done = feeding.done_before + feeding.take;
        if load.phase == Mode::Terminal {
            self.terminal_mode.end_restore(&group_separator(session));
        } else {
            self.line_mode.end_restored_rx(self.settings.encoding);
            let separator = LogEntry::restored(group_separator(session), None);
            self.line_mode.restore_entries([separator]);
            (load.ends).push(self.line_mode.history().len() + self.line_mode.dropped_lines());
        }
        load.feeding = None;
    }

    /// A group loaded: its end in view, and a toast saying what didn't fit (MAX-202)
    pub fn end_group_load(&mut self, load: &GroupLoad) {
        let dropped = self.line_mode.dropped_lines();
        let (sessions, ends) = (&load.sessions, &load.ends);
        let rows = self.settings.terminal_mode.history_size;
        let fit = GroupFit {
            sessions: sessions.len(),
            unreadable: load.line_tail.len() - ends.len(),
            kept: ends.iter().filter(|&&end| end > dropped).count(),
            cut: dropped > 0 || tail_is_cut(sessions, &load.line_tail),
            lines: load.lines,
            terminal_cut: rows < ends.last().copied().unwrap_or(0)
                || tail_is_cut(sessions, &load.terminal_tail),
            terminal_rows: rows,
        };
        // the end of the group in view, not the empty screen under the last separator
        let (screen_rows, _) = self.terminal_mode.get_vt100_size();
        self.terminal_mode
            .scroll_up(usize::from(screen_rows.saturating_sub(1)));

        let (message, warn) = fit.message();
        self.emit(Event::GroupLoaded { message, warn });
    }

    /// Stops the recording, or opens the recording dialog on the mode's stream when none runs
    /// (MAX-175: one recording at a time, the dialog picks text or full)
    pub fn toggle_recording(&mut self) -> Option<Note> {
        match self.recorder.recording() {
            Some(stream) => {
                self.recorder.stop();
                Some(Note::info(format!("Stopped recording {}", stream.as_str())))
            }
            None => {
                self.open_capture_dialog(self.mode_stream());
                None
            }
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

    /// The receiver's answers during a protocol transfer (MAX-162): kept by the raw streams,
    /// not text lines
    pub fn record_protocol_rx(&mut self, bytes: &[u8]) {
        self.recorder.record_raw_rx(bytes);
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
    pub fn start_capture(&mut self, path: &Path, stream: Stream, include_session: bool) -> Report {
        let path = expand_home(path);
        match stream {
            Stream::Text => {
                let mut file = path.into_os_string();
                file.push(Stream::Text.suffix());
                self.start_text_capture(Path::new(&file), include_session)
            }
            Stream::Full => {
                let source = self.capture_source();
                let text = self.capture_format();
                let result = (self.recorder).start_full(&path, include_session, source, &text);
                self.report_capture_start(result, stream)
            }
        }
    }

    /// Starts the text recording in the file `file` as it's named, as minicom's `-C` does
    /// (MAX-181)
    pub fn start_text_capture(&mut self, file: &Path, include_session: bool) -> Report {
        let file = expand_home(file);
        let source = self.capture_source();
        let result = (self.recorder).start_text(&file, include_session, source);
        self.report_capture_start(result, Stream::Text)
    }

    /// A recording started, or its failure shown in the error popup
    fn report_capture_start(&mut self, result: std::io::Result<()>, stream: Stream) -> Report {
        match result {
            Ok(()) => {
                let started = format!("Started recording {}", stream.as_str());
                Report::from(Note::new(ToastLevel::Success, started))
            }
            Err(e) => {
                let message = format!("Could not open file for writing: {e}");
                self.open_overlay(Overlay::Error(
                    ErrorDialog::new("Capture Failed", "Could not open file for writing")
                        .with_details(e),
                ));
                Report::shown(message)
            }
        }
    }

    /// The session browser on the session folder, the session restored into the history marked
    /// (MAX-148, MAX-202)
    pub fn open_session_browser(&mut self) -> Option<Note> {
        let Some(dir) = self.sessions_dir.clone() else {
            return Some(Note::warn("No session folder"));
        };
        let max_size = self.settings.sessions.limits().max_size;
        // the live link's session backup, the one a replay paused while it runs
        let replaced = self.replaced_link();
        let paused_raw = replaced.and_then(|replaced| {
            let session = replaced.session.as_ref()?;
            Some(session.raw_path().to_path_buf())
        });
        let live_raw = self.recorder.session_raw().or(paused_raw.as_deref());
        let mut browser = SessionBrowser::new(dir, self.restored_session.clone())
            .with_max_size(max_size)
            .with_group_by(self.settings.sessions.group_by.clone())
            .with_focus(live_raw)
            .with_own_session(live_raw)
            .with_link_colors(self.link_colors());
        if replaced.is_some() {
            browser = browser.with_live(paused_raw.clone());
        }
        self.open_overlay(Overlay::Sessions(Box::new(browser)));
        None
    }
}

/// The end of a group that fits in `lines` lines and `bytes` bytes, oldest first: each
/// session's index with how much of its `.raw` to read (MAX-202). Lines are counted by their
/// `\n`, backwards from the newest session, so short lines cost what the history holds and not
/// a byte budget (MAX-272); the byte budget still caps data with few line ends
fn group_tail(sessions: &[Session], lines: usize, bytes: u64) -> Vec<(usize, u64)> {
    let (mut lines_left, mut bytes_left) = (lines, bytes);
    let mut tail = Vec::new();
    for (index, session) in sessions.iter().enumerate().rev() {
        if lines_left == 0 || bytes_left == 0 {
            break;
        }
        // its separator is a line too, and one alone says nothing
        lines_left -= 1;
        if lines_left == 0 {
            break;
        }
        let max = session.raw_len.min(bytes_left);
        // one that can't be read is said unreadable when it's loaded
        let (take, seen) = (session.raw_path())
            .and_then(|path| tail_lines_len(path, lines_left, max).ok())
            .unwrap_or((max, 0));
        bytes_left -= take;
        lines_left = lines_left.saturating_sub(seen);
        tail.push((index, take));
    }
    tail.reverse();
    tail
}

/// Whether `tail` leaves out a session of `sessions`, or the start of its oldest
fn tail_is_cut(sessions: &[Session], tail: &[(usize, u64)]) -> bool {
    let whole_oldest = tail
        .first()
        .is_none_or(|&(oldest, take)| take == sessions[oldest].raw_len);
    tail.len() < sessions.len() || !whole_oldest
}

/// A group replay being loaded (MAX-272): line mode's part of the group, then terminal mode's,
/// a session at a time, each fed a piece at a time so the loop draws between slices
pub struct GroupLoad {
    sessions: Vec<Session>,
    lines: usize,
    /// Each mode's end of the group: session index, bytes to read
    line_tail: Vec<(usize, u64)>,
    terminal_tail: Vec<(usize, u64)>,
    /// The mode whose part is being fed: line mode's, then terminal mode's
    phase: Mode,
    /// The next entry of the tail being loaded
    next: usize,
    feeding: Option<Feeding>,
    /// Where each session read ends in line mode's history, for the toast
    ends: Vec<usize>,
    done: u64,
    total: u64,
    started: Instant,
}

impl GroupLoad {
    pub fn sessions(&self) -> usize {
        self.sessions.len()
    }

    /// The mode being loaded, the session of its part being read (from 1) and how many that
    /// part has: `(Line, 3, 12)`
    pub fn at(&self) -> (Mode, usize, usize) {
        let of = self.tail().len();
        (self.phase, self.next.clamp(1, of.max(1)), of)
    }

    /// The part being fed
    fn tail(&self) -> &[(usize, u64)] {
        match self.phase {
            Mode::Line => &self.line_tail,
            Mode::Terminal => &self.terminal_tail,
        }
    }

    /// Bytes fed so far, of both modes' parts, and their sum
    pub fn progress(&self) -> (u64, u64) {
        (self.done, self.total)
    }

    /// How much is done, the gauge's: terminal mode's bytes weigh `TERMINAL_WEIGHT` each
    pub fn percent(&self) -> u64 {
        let (done, total) = self.work();
        (done * 100).checked_div(total).unwrap_or(100)
    }

    /// The work done and the whole, bytes weighed by what they cost: terminal mode's part,
    /// loaded after line mode's, costs `TERMINAL_WEIGHT` times more per byte
    fn work(&self) -> (u64, u64) {
        let line_total = self.line_tail.iter().map(|&(_, take)| take).sum::<u64>();
        let weigh =
            |bytes: u64| bytes.min(line_total) + bytes.saturating_sub(line_total) * TERMINAL_WEIGHT;
        (weigh(self.done), weigh(self.total))
    }

    /// Since the load started
    pub fn elapsed(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.started)
    }

    /// Bytes a second so far; none at the start
    pub fn rate(&self, now: Instant) -> Option<f64> {
        let secs = self.elapsed(now).as_secs_f64();
        (secs > 0.0 && self.done > 0).then(|| self.done as f64 / secs)
    }

    /// The time left at the rate so far, by the work: the rate in bytes drops in terminal
    /// mode's part
    pub fn eta(&self, now: Instant) -> Option<Duration> {
        let secs = self.elapsed(now).as_secs_f64();
        let (done, total) = self.work();
        let rate = (secs > 0.0 && done > 0).then(|| done as f64 / secs)?;
        Some(Duration::from_secs_f64(
            total.saturating_sub(done) as f64 / rate,
        ))
    }
}

/// The session of a group load being fed: its bytes read, cut where its chunks were received
struct Feeding {
    session: usize,
    take: u64,
    /// The load's bytes done when it started
    done_before: u64,
    raw: Vec<u8>,
    chunks: Vec<(Range<usize>, Option<Timestamp>)>,
    chunk: usize,
    /// The next byte to feed
    at: usize,
    /// The last byte fed was a CR, for add LF (MAX-181)
    after_cr: bool,
}

/// When a session's chunks from raw offset `start` on were received, none when it isn't known
fn session_times(session: &Session, start: u64) -> Vec<(u64, Timestamp)> {
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
