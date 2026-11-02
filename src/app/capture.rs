//! The app's captures (MAX-172): the session backup it starts and switches, the past
//! session it restores at startup, the recordings the user starts, and the session browser.
//! `capture::Recorder` writes the files
use std::path::{Path, PathBuf};

use super::App;
use crate::{
    app::{terminal_mode::add_newlines, types::LogEntry},
    capture::{
        CaptureDevice, CaptureFormat, CaptureSource, RESTORE_RAW_TAIL, RecordingWriter, Session,
        Stream, TerminalSize, TimedChunk, find_last_session, read_tail_bytes, read_times,
        restore_separator, timed_chunks,
    },
    configs::{Mode, settings::RestoreMode},
    overlays::{ErrorDialog, FilePathDialog, Overlay, SessionBrowser},
    paths::expand_home,
};

/// The most kept while the restore dialog asks: past it, the dialog closes without a restore
const HELD_RX_MAX: usize = 4 << 20;

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
        if let Err(err) = result {
            self.notify_warn(format!("Session backup off: {err}"));
        }
    }

    /// A new session backup, for the port `open` switched to: the backups go by port (MAX-12)
    pub fn restart_session(&mut self) {
        self.start_session(self.sessions_dir.clone());
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
            Ok(()) => self.notify_info(format!("Restored the session of {}", session.time_span())),
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

        let settings = &self.settings;
        let (add_cr, add_lf, encoding) = (settings.add_cr, settings.add_lf, settings.encoding);
        self.line_mode.restore_rx(add_lf, encoding, &chunks);
        // like a `clear`, the separator on the top row (MAX-176)
        self.line_mode.clear_view();
        let separator_line = LogEntry::restored(separator.clone(), None);
        self.line_mode.restore_entries([separator_line]);
        // the view's size applies once the UI is up, by the dimension mode
        if let Some(size) = meta.terminal {
            self.terminal_mode.set_vt100_size(size.rows, size.cols);
        }
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
        (self.terminal_mode).restore(encoding, &shown_chunks, &separator);
        Ok(())
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

    /// The session browser on the session folder, the session this device would restore marked
    /// (MAX-148)
    pub fn open_session_browser(&mut self) {
        let Some(dir) = self.sessions_dir.clone() else {
            self.notify_warn("No session folder");
            return;
        };
        let max_age = self.settings.sessions.max_age();
        let restorable =
            find_last_session(&dir, &self.session_device(), max_age).map(|session| session.stem);
        let mut browser = SessionBrowser::new(dir, restorable);
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

#[cfg(test)]
pub mod tests {
    //! Recordings, the session restore and its dialog, on an app with no real link.
    use std::time::Duration;

    use super::*;
    use crate::{
        app::tests::{app, record_full_meta, set_line, texts, toasts},
        capture::RecordingMeta,
        configs::DualBufferMode,
        inputs::{Action, AppAction, DialogAction},
        link::LinkConfig,
    };

    /// MAX-147: a text recording gets what is received in terminal mode too, even when line
    /// mode isn't fed (it used to go quiet there).
    /// How: `ActiveOnly` buffering in terminal mode, a text recording, one line received.
    #[test]
    fn text_recording_follows_rx_in_terminal_mode() {
        let mut app = app();
        app.settings.mode = Mode::Terminal;
        app.settings.dual_mode_buffering = DualBufferMode::ActiveOnly;
        app.settings.capture.timestamp = false;
        let path =
            std::env::temp_dir().join(format!("maxicom_test_{}_app_text.log", std::process::id()));
        let _ = std::fs::remove_file(&path);
        app.recorder
            .start_text(&path, false, app.capture_source())
            .unwrap();

        app.feed_rx(b"seen\r\n");
        app.recorder.stop();
        assert!(app.line_mode.history().is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "seen\n");
        let _ = std::fs::remove_file(&path);
    }

    /// MAX-147: sent lines are recorded in line mode after `capture.tx_mark`, not in terminal
    /// mode nor with `capture.record_tx` off.
    /// How: a text recording, one send in each case, then one with the mark "TX: " and one
    /// with no mark.
    #[test]
    fn tx_is_recorded_in_line_mode_only() {
        let mut app = app();
        app.settings.capture.timestamp = false;
        let path =
            std::env::temp_dir().join(format!("maxicom_test_{}_app_tx.log", std::process::id()));
        let _ = std::fs::remove_file(&path);
        app.recorder
            .start_text(&path, false, app.capture_source())
            .unwrap();

        app.settings.mode = Mode::Line;
        app.record_tx(b"line\r\n");
        app.settings.mode = Mode::Terminal;
        app.record_tx(b"terminal\r");
        app.settings.mode = Mode::Line;
        app.settings.capture.record_tx = false;
        app.record_tx(b"off\r\n");
        app.settings.capture.record_tx = true;
        app.settings.capture.tx_mark = "TX: ".to_string();
        app.record_tx(b"marked\r\n");
        app.settings.capture.tx_mark = String::new();
        app.record_tx(b"bare\r\n");
        app.recorder.stop();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "> line\nTX: marked\nbare\n"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// MAX-175: one recording at a time: `record` (the `REC` click) opens the dialog on the
    /// mode's stream (text in line mode, full in terminal mode) when none runs, else stops it;
    /// starting one stops the other, and `REC` is on with either.
    /// How: `record` in each mode, then a text recording replaced by a full one, `record`.
    #[test]
    fn one_recording_at_a_time() {
        let mut app = app();
        let temp = |name: &str| {
            std::env::temp_dir().join(format!("maxicom_test_{}_{name}", std::process::id()))
        };
        let open_dialog = |app: &App| match app.overlay() {
            Some(Overlay::FilePath(dialog)) => Some(dialog.stream),
            _ => None,
        };
        for (mode, stream) in [(Mode::Line, Stream::Text), (Mode::Terminal, Stream::Full)] {
            app.settings.mode = mode;
            app.update(Action::App(AppAction::ToggleRecording));
            assert_eq!(open_dialog(&app), Some(stream));
            app.close_overlay();
        }

        app.recorder
            .start_text(&temp("act.log"), false, app.capture_source())
            .unwrap();
        assert_eq!(app.recording(), Some(Stream::Text));
        let text = CaptureFormat::default();
        app.recorder
            .start_full(&temp("act"), false, app.capture_source(), &text)
            .unwrap();
        assert_eq!(app.recording(), Some(Stream::Full));
        app.update(Action::App(AppAction::ToggleRecording));
        assert_eq!(app.recording(), None);
        assert!(app.overlay().is_none());
        let _ = std::fs::remove_file(temp("act.log"));
        let _ = std::fs::remove_dir_all(temp("act"));
    }

    /// MAX-148: a restore puts the session's last lines in line mode, then the separator; the
    /// terminal gets the raw bytes, back on the main screen. Nothing goes to the new session or
    /// the stats. MAX-175: line mode is cut from the raw bytes, so a sent line isn't restored,
    /// and the lines get their times from the session's `.times`; a last line not ended is
    /// one too.
    /// How: a session with "> " marks and times, ending in the alternate screen; an app with
    /// its own session backup restores it.
    #[test]
    fn restore_fills_both_modes_and_records_nothing() {
        let base =
            std::env::temp_dir().join(format!("maxicom_test_{}_restore", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let text = CaptureFormat {
            timestamp: Some("[%H:%M:%S%.3f]".to_string()),
            tx_mark: Some("> ".to_string()),
        };
        let session = crate::capture::tests::write_test_session(
            &base.join("old"),
            "/dev/ttyUSB0",
            &text,
            b"one\r\ntwo\r\n\x1b[?1049h\x1b[?1hvim",
            b"cmd\r\n",
        );

        let mut app = app();
        app.terminal_mode.set_vt100_size(24, 80);
        app.start_session(Some(base.join("new")));
        app.restore_session(&session).unwrap();

        let history: Vec<_> = app.line_mode.history().iter().collect();
        let texts = texts(&app);
        assert_eq!(texts[..3], ["one", "two", "\x1b[?1049h\x1b[?1hvim"]);
        assert_eq!(texts[3], "── restored", "{texts:?}");
        assert!(history[..3].iter().all(|e| e.timestamp.is_some()));
        assert_eq!(history[3].timestamp, None);
        // the live view starts on the separator (MAX-176)
        let floor = app.line_mode.view_floor().map(|floor| floor.line_idx);
        assert_eq!(floor, Some(3));

        let screen = app.terminal_mode.vt100_screen();
        assert!(!screen.alternate_screen() && !screen.application_cursor());
        assert!(screen.contents().starts_with("── restored: ttyUSB0, "));
        let lines = app.terminal_mode.all_text_lines();
        let separator = lines
            .iter()
            .position(|l| l.starts_with("── restored"))
            .unwrap();
        assert_eq!(lines[separator - 2..separator], ["one", "two"], "{lines:?}");
        assert_eq!(app.stats.rx(), 0);

        drop(app);
        for entry in std::fs::read_dir(base.join("new")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|ext| ext != "meta") {
                assert_eq!(std::fs::read(&path).unwrap(), b"", "{path:?}");
            }
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    /// MAX-175: line mode restored shows what it showed live: the same entries, no blank or
    /// repeated row for the lines sent in line mode (the session's `.log` has them, marked).
    /// How: a shell-like exchange (an empty Enter, `ls`, zsh's mode resets) fed live to one
    /// app and recorded with its sent lines in a session, restored in another.
    #[test]
    fn restored_line_mode_is_what_live_showed() {
        let dir = crate::capture::tests::temp_dir("restore_as_live");
        let rx: &[u8] = b"$ \x1b[?1l\x1b>\x1b[?2004l\r\r\n$ ls\x1b[?2004l\r\r\nfile\r\n";
        let mut live = app();
        live.feed_rx(rx);
        let session = crate::capture::tests::write_test_session(
            &dir,
            "/dev/ttyUSB0",
            &CaptureFormat {
                timestamp: None,
                tx_mark: Some("> ".to_string()),
            },
            rx,
            b"\r\nls\r\n",
        );
        let mut restored = app();
        restored.restore_session(&session).unwrap();
        let mut restored_texts = texts(&restored);
        assert_eq!(restored_texts.pop().as_deref(), Some("── restored"));
        assert_eq!(restored_texts, texts(&live));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MAX-178: the session meta says the terminal grid's size, rewritten when it changes, and
    /// a restore replays the raw bytes at that size: a line wider than the app's grid comes
    /// back whole, not cut at its width.
    /// How: an app on an 80-column grid records a session, its grid widened to 120 columns
    /// before a 100-column line comes in; another app on 80 columns restores it.
    #[test]
    fn restore_replays_at_the_recorded_terminal_size() {
        let dir = crate::capture::tests::temp_dir("restore_size");
        let wide = "x".repeat(100);
        let mut live = app();
        live.terminal_mode.set_vt100_size(24, 80);
        live.start_session(Some(dir.clone()));
        live.tick();
        live.terminal_mode.set_vt100_size(10, 120);
        live.feed_rx(format!("{wide}\r\n").as_bytes());
        let device = live.session_device();
        drop(live);
        let session = find_last_session(&dir, &device, None).unwrap();
        let size = session.meta.as_ref().and_then(|meta| meta.terminal);
        assert_eq!(
            size,
            Some(TerminalSize {
                rows: 10,
                cols: 120
            })
        );

        let mut restored = app();
        restored.terminal_mode.set_vt100_size(24, 80);
        restored.restore_session(&session).unwrap();
        assert_eq!(restored.terminal_mode.get_vt100_size(), (10, 120));
        let lines = restored.terminal_mode.all_text_lines();
        assert!(lines.contains(&wide), "{lines:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MAX-175: a restore gives each line the time its first byte was received, and each
    /// terminal row the time the cursor reached it, as live, read from the session's `.times`.
    /// How: a session receiving "one" then "\r\ntwo\r\n", its `.times` set to 1 s and 2 s.
    #[test]
    fn restore_takes_the_times_of_the_chunks() {
        let dir = crate::capture::tests::temp_dir("restore_times");
        let session = crate::capture::tests::write_test_session(
            &dir,
            "/dev/ttyUSB0",
            &CaptureFormat::default(),
            b"one\r\ntwo\r\n",
            b"",
        );
        std::fs::write(session.times_path().unwrap(), "0 1000\n3 2000\n").unwrap();
        let second = |s| chrono::DateTime::from_timestamp(s, 0).map(Into::into);

        let mut app = app();
        app.terminal_mode.set_vt100_size(24, 80);
        app.restore_session(&session).unwrap();
        let times: Vec<_> = app
            .line_mode
            .history()
            .iter()
            .map(|e| e.timestamp)
            .collect();
        assert_eq!(times, [second(1), second(2), None]);
        let lines = app.terminal_mode.all_text_lines();
        let row = |text: &str| lines.iter().position(|l| l == text).unwrap();
        let terminal = &app.terminal_mode;
        assert_eq!(terminal.get_timeline_timestamp(row("one")), second(1));
        assert_eq!(terminal.get_timeline_timestamp(row("two")), second(2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MAX-172: a session is its raw bytes and its meta, its text a bonus: without its `.log`
    /// it is still found and restored, both modes from the raw bytes (line mode too since
    /// MAX-175).
    /// How: a session written, its `.log` removed, then found and restored.
    #[test]
    fn session_without_text_restores_from_raw() {
        let dir = crate::capture::tests::temp_dir("restore_no_text");
        let port = LinkConfig::default().address.to_string();
        let session = crate::capture::tests::write_test_session(
            &dir,
            &port,
            &CaptureFormat::default(),
            b"from raw\r\n",
            b"",
        );
        std::fs::remove_file(session.text_path().unwrap()).unwrap();

        let mut app = app();
        app.terminal_mode.set_vt100_size(24, 80);
        app.settings.sessions.restore = RestoreMode::Always;
        app.restore_last_session(Some(&dir), None);
        assert_eq!(texts(&app), ["from raw", "── restored"]);
        let lines = app.terminal_mode.all_text_lines();
        let separator = lines
            .iter()
            .position(|l| l.starts_with("── restored"))
            .unwrap();
        assert_eq!(lines[separator - 1], "from raw", "{lines:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MAX-172: a full recording's meta starts when the recording does, or when the session
    /// started when it includes the session.
    /// How: a session backup started, then two metas written once the clock is a second later,
    /// with and without the session.
    #[test]
    fn raw_recording_meta_starts_with_the_session_it_includes() {
        let dir = crate::capture::tests::temp_dir("meta_started");
        let mut app = app();
        app.start_session(Some(dir.join("sessions")));
        let sessions = crate::capture::list_sessions(&dir.join("sessions")).unwrap();
        let started = sessions[0].meta.as_ref().unwrap().started;
        // the metas keep whole seconds of the wall clock, which a fixed sleep doesn't
        // guarantee to pass (WSL2 steps it back at times): wait on the clock itself
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while chrono::Local::now().timestamp() <= started {
            assert!(
                std::time::Instant::now() < deadline,
                "the clock didn't move"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let read = |name: &str| RecordingMeta::read(&dir.join(name)).unwrap().started;
        record_full_meta(&mut app, &dir.join("with"), true);
        record_full_meta(&mut app, &dir.join("without"), false);
        assert_eq!(read("with/with.meta"), started);
        assert!(read("without/without.meta") > started);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MAX-173: the session backup follows the device: a new baud, framing or encoding starts
    /// a new session whose meta says it, the bytes after the change in it; a new flow control
    /// keeps the session (not in the meta). With nothing received, the tick starts it.
    /// How: a session, "a" received; `flow` changed, "b"; `baud` and `framing`, "c"; the
    /// encoding, then a tick. The sessions' meta and raw files read back in order.
    #[test]
    fn session_backup_follows_the_device() {
        let dir = crate::capture::tests::temp_dir("follow_device");
        let mut app = app();
        app.start_session(Some(dir.clone()));
        let line = |app: &App| app.link_config().line;
        // sessions are named to the millisecond: within one, the next takes over the files, so
        // the changes below wait a little
        app.feed_rx(b"a");

        let mut flow = line(&app);
        flow.flow_control = crate::link::serial::FlowControl::RtsCts;
        set_line(&mut app, flow);
        app.feed_rx(b"b");

        std::thread::sleep(Duration::from_millis(2));
        let mut new_line = line(&app);
        new_line.baud = 9600;
        new_line.framing = "7E1".parse().unwrap();
        set_line(&mut app, new_line);
        app.feed_rx(b"c");

        std::thread::sleep(Duration::from_millis(2));
        app.settings.encoding = crate::configs::TextEncoding::Cp437;
        app.tick();
        drop(app);

        let metas: Vec<_> = crate::capture::tests::session_paths(&dir)
            .into_iter()
            .filter(|path| path.extension().is_some_and(|ext| ext == "meta"))
            .collect();
        let sessions: Vec<_> = metas
            .iter()
            .map(|path| {
                let device = RecordingMeta::read(path).unwrap().device;
                let raw = std::fs::read(path.with_extension("raw")).unwrap();
                let line = device.line.unwrap();
                let raw = String::from_utf8(raw).unwrap();
                (line.baud, line.framing.to_string(), device.encoding, raw)
            })
            .collect();
        use crate::configs::TextEncoding::{Cp437, Utf8};
        let framing = |f: &str| {
            f.parse::<crate::link::serial::Framing>()
                .unwrap()
                .to_string()
        };
        let default = LinkConfig::default().line;
        assert_eq!(
            sessions,
            [
                (
                    default.baud,
                    default.framing.to_string(),
                    Utf8,
                    "ab".to_string()
                ),
                (9600, framing("7E1"), Utf8, "c".to_string()),
                (9600, framing("7E1"), Cp437, String::new()),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MAX-173: a full recording stops when what it records changes, with a toast: the baud,
    /// the encoding, a replay starting or ending, another replay; a new flow control keeps it,
    /// and a replay's own encoding toggle too (its device comes from its file). The bytes
    /// after the change don't reach it.
    /// How: a recording on the live link, `flow` then `baud` changed with "b" received; one
    /// stopped by the encoding; one stopped by a replay; one during the replay, the encoding
    /// toggled, then stopped by the end of the replay.
    #[test]
    fn full_recording_stops_when_the_device_changes() {
        let dir = crate::capture::tests::temp_dir("raw_follow");
        let mut app = app();
        // the toast is the last one: each start has its own before
        let stopped = |app: &App| {
            let last = toasts(app).pop();
            app.recording().is_none()
                && last.as_deref() == Some("Full recording stopped: the device changed")
        };
        let start = |app: &mut App, name: &str| {
            app.start_capture(&dir.join(name), Stream::Full, false);
            assert!(app.recording() == Some(Stream::Full));
        };

        start(&mut app, "baud");
        app.feed_rx(b"a");
        let mut line = app.link_config().line;
        line.flow_control = crate::link::serial::FlowControl::RtsCts;
        set_line(&mut app, line);
        app.tick();
        assert!(app.recording() == Some(Stream::Full));
        line.baud = 9600;
        set_line(&mut app, line);
        app.feed_rx(b"b");
        assert!(stopped(&app));
        assert_eq!(std::fs::read(dir.join("baud/baud.raw")).unwrap(), b"a");

        start(&mut app, "encoding");
        app.settings.encoding = crate::configs::TextEncoding::Cp437;
        app.tick();
        assert!(stopped(&app));

        start(&mut app, "replay");
        let session = crate::capture::tests::device("/dev/ttyUSB1");
        app.start_replay(dir.join("old.raw"), Some(&session));
        app.tick();
        assert!(stopped(&app));

        start(&mut app, "during");
        app.settings.encoding = crate::configs::TextEncoding::Utf8;
        app.tick();
        assert!(app.recording() == Some(Stream::Full));
        assert!(app.stop_replay());
        app.tick();
        assert!(stopped(&app));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MAX-181: minicom's `-C` records the text to the file as named, no `.log` added, where
    /// the dialog's name gets one.
    /// How: `-C`'s start on `boot.cap`, "a" received, then the dialog's start on `rec`.
    #[test]
    fn text_capture_to_the_named_file() {
        let dir = crate::capture::tests::temp_dir("text_named");
        let mut app = app();
        app.settings.capture.timestamp = false;
        app.start_text_capture(&dir.join("boot.cap"), false);
        assert_eq!(app.recording(), Some(Stream::Text));
        app.feed_rx(b"a\r\n");
        app.recorder.stop();
        let text = std::fs::read_to_string(dir.join("boot.cap")).unwrap();
        assert_eq!(text.lines().collect::<Vec<_>>(), ["a"]);
        assert!(!dir.join("boot.cap.log").exists());

        app.start_capture(&dir.join("rec"), Stream::Text, false);
        app.recorder.stop();
        assert!(dir.join("rec.log").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MAX-173: a text recording keeps running through device changes, a marker line where
    /// each happens, before the lines received after it; a new flow control gets none.
    /// How: a text recording, "a" received; `flow` changed, "b"; `baud`, "c"; a replay of a
    /// session started (nothing received, the tick sees it).
    #[test]
    fn text_recording_marks_device_changes() {
        let dir = crate::capture::tests::temp_dir("text_follow");
        let path = dir.join("rec.log");
        let mut app = app();
        let port = app.link_config().address.clone();
        app.settings.capture.timestamp = false;
        app.start_capture(&dir.join("rec"), Stream::Text, false);
        app.feed_rx(b"a\r\n");

        let mut line = app.link_config().line;
        line.flow_control = crate::link::serial::FlowControl::RtsCts;
        set_line(&mut app, line);
        app.feed_rx(b"b\r\n");
        line.baud = 9600;
        set_line(&mut app, line);
        app.feed_rx(b"c\r\n");

        let session = crate::capture::tests::device("/dev/ttyUSB1");
        app.start_replay(dir.join("old.raw"), Some(&session));
        app.tick();
        assert!(app.recording() == Some(Stream::Text));
        app.recorder.stop();

        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let marker = format!("── device: {}, 9600 8N1, UTF8 ──", port.tag());
        assert_eq!(
            lines,
            [
                "a",
                "b",
                marker.as_str(),
                "c",
                "── device: old.raw, 115200 8N1, UTF8 ──"
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MAX-148: the last session restores at startup with `always` or `--restore`, not with
    /// `never`, `ask` (before its answer) or `--no-restore`, nor when older than
    /// `restore_max_age`; `0` is no limit. A session restores again on the next launch.
    /// How: one session on the app's port, then each case on a fresh app.
    #[test]
    fn last_session_restores_as_settings_say() {
        let dir =
            std::env::temp_dir().join(format!("maxicom_test_{}_restore_last", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let port = LinkConfig::default().address.to_string();
        let session = crate::capture::tests::write_test_session(
            &dir,
            &port,
            &CaptureFormat::default(),
            b"old line\r\n",
            b"",
        );
        let restored = |mode: RestoreMode, flag: Option<bool>, max_age: u64| {
            let mut app = app();
            app.settings.sessions.restore = mode;
            app.settings.sessions.restore_max_age = Duration::from_secs(max_age);
            app.restore_last_session(Some(&dir), flag);
            app.line_mode
                .history()
                .iter()
                .any(|e| e.text == "old line\r\n")
        };
        assert!(restored(RestoreMode::Always, None, 3600));
        assert!(restored(RestoreMode::Always, None, 3600), "a second launch");
        assert!(!restored(RestoreMode::Never, None, 3600));
        // `ask` asks first: nothing restored before the answer
        assert!(!restored(RestoreMode::Ask, None, 3600));
        assert!(restored(RestoreMode::Never, Some(true), 3600));
        assert!(!restored(RestoreMode::Always, Some(false), 3600));

        let two_hours_ago = std::time::SystemTime::now() - Duration::from_secs(2 * 3600);
        std::fs::File::options()
            .write(true)
            .open(session.raw_path().unwrap())
            .unwrap()
            .set_modified(two_hours_ago)
            .unwrap();
        assert!(!restored(RestoreMode::Always, None, 3600));
        assert!(restored(RestoreMode::Always, None, 0));

        let mut app = app();
        app.settings.sessions.restore = RestoreMode::Always;
        app.restore_last_session(None, None);
        assert!(app.line_mode.history().is_empty(), "no cache folder");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An app whose `ask` dialog is open on a session with "old line", and its folder.
    pub fn asking_app(name: &str) -> (App, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("maxicom_test_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::capture::tests::write_test_session(
            &dir,
            &LinkConfig::default().address.to_string(),
            &CaptureFormat::default(),
            b"old line\r\n",
            b"",
        );
        let mut app = app();
        app.settings.mode = Mode::Line;
        app.settings.sessions.restore = RestoreMode::Ask;
        app.restore_last_session(Some(&dir), None);
        assert!(matches!(app.overlay(), Some(Overlay::RestoreSession(_))));
        (app, dir)
    }

    /// MAX-148: with `ask`, what comes in while the dialog is open waits; Enter (or `y`)
    /// restores the session first, then feeds it; Esc (or `n`) feeds it without a restore.
    /// How: "live" received while asking, then each answer on a fresh app.
    #[test]
    fn ask_dialog_holds_rx_until_the_answer() {
        let key = |c| {
            DialogAction::HandleKey(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(c),
                crossterm::event::KeyModifiers::NONE,
            ))
        };
        for (answer, restored) in [
            (DialogAction::Confirm, true),
            (key('y'), true),
            (DialogAction::Close, false),
            (key('n'), false),
        ] {
            let (mut app, dir) = asking_app("ask_answer");
            app.feed_rx(b"live\r\n");
            assert!(app.line_mode.history().is_empty());
            app.update(Action::Overlay(answer));
            assert!(app.overlay().is_none());
            let expected: &[&str] = if restored {
                &["old line", "── restored", "live"]
            } else {
                &["live"]
            };
            assert_eq!(texts(&app), expected);
            app.feed_rx(b"after\r\n");
            assert_eq!(texts(&app).last().unwrap(), "after");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// MAX-148: the held data is never stuck: when another popup replaced the dialog, or more
    /// than the cap came in, it is fed without a restore.
    #[test]
    fn ask_dialog_never_keeps_rx_stuck() {
        let (mut app, dir) = asking_app("ask_stuck");
        app.feed_rx(b"held\r\n");
        app.open_overlay(Overlay::ConfirmQuit);
        app.feed_rx(b"next\r\n");
        assert_eq!(texts(&app), ["held", "next"]);

        let (mut app, _) = asking_app("ask_stuck");
        app.feed_rx(b"held\r\n");
        app.feed_rx(&vec![b'x'; HELD_RX_MAX]);
        assert!(app.overlay().is_none());
        assert_eq!(texts(&app), ["held"]);
        app.feed_rx(b"\r\n");
        assert_eq!(texts(&app)[1].len(), HELD_RX_MAX);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
