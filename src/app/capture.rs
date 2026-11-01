//! The app's captures (MAX-172): the session backup it starts and switches, the past
//! session it restores at startup, the recordings the user starts, and the session browser.
//! `capture::Recorder` writes the files
use std::path::{Path, PathBuf};

use super::App;
use crate::{
    app::types::LogEntry,
    capture::{
        RESTORE_RAW_TAIL, Session, SessionDevice, SessionMeta, SessionReader, SessionText, Stream,
        expand_home, find_last_session, read_tail_bytes, read_tail_lines, restore_separator,
    },
    configs::{Mode, settings::RestoreMode},
    overlays::{ErrorDialog, FilePathDialog, Overlay, SessionBrowser},
};

/// The most kept while the restore dialog asks: past it, the dialog closes without a restore
const HELD_RX_MAX: usize = 4 << 20;

impl App {
    /// Starts the session backup in `dir` when `sessions.enabled` is on. Without it, a toast
    /// says so and the app runs on (MAX-147)
    pub fn start_session(&mut self, dir: Option<PathBuf>) {
        // the browser lists the past sessions even with the backup off
        self.sessions_dir.clone_from(&dir);
        let (capture, sessions) = (&self.settings.capture, &self.settings.sessions);
        if !sessions.enabled {
            return;
        }
        let device = self.session_device();
        // the session's text is written as the capture settings say now, reloads aside
        let text = SessionText {
            timestamp: capture
                .timestamp
                .then(|| self.settings.timestamp_format.clone()),
            tx_mark: capture.record_tx.then(|| capture.tx_mark.clone()),
        };
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

    /// What this session runs on, for its backup and to find a past one to restore
    fn session_device(&self) -> SessionDevice {
        SessionDevice::new(self.link.config(), self.settings.encoding)
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
    /// last lines of its text in line mode, read back with their times and marks, the end of
    /// its raw bytes through the terminal, then a separator where the live data starts. A
    /// session is its raw bytes and its meta; its text is a bonus: without it, line mode gets
    /// the separator alone (MAX-172). Not recorded nor counted: the new session starts empty
    pub fn restore_session(&mut self, session: &Session) -> std::io::Result<()> {
        // a session from `find_last_session` has both
        let (Some(raw_path), Some(meta), Some(started)) =
            (session.raw_path(), &session.meta, session.started)
        else {
            return Err(std::io::Error::other("not a complete session"));
        };
        let text = match session.text_path() {
            Some(path) => read_tail_lines(path, self.settings.line_mode.history_size)?,
            None => Vec::new(),
        };
        let raw = read_tail_bytes(raw_path, RESTORE_RAW_TAIL)?;
        let separator = restore_separator(session);

        let text = String::from_utf8_lossy(&text);
        let mut reader = SessionReader::new(&meta.text, started);
        let lines = text.lines().map(|line| {
            let line = reader.parse(line);
            LogEntry::restored(line.text.to_string(), line.time)
        });
        self.line_mode.restore_entries(lines);
        // like a `clear`, the separator on the top row (MAX-176)
        self.line_mode.clear_view();
        let separator_line = LogEntry::restored(separator.clone(), None);
        self.line_mode.restore_entries([separator_line]);
        self.terminal_mode
            .restore(self.settings.encoding, &raw, &separator);
        Ok(())
    }

    /// Stops every recording, or opens the recording dialog on the mode's stream when none is on
    pub fn toggle_logging(&mut self) {
        if self.recorder.is_any_active() {
            self.recorder.stop(Stream::Text);
            self.recorder.stop(Stream::Raw);
            self.notify_info("Recording stopped");
        } else {
            self.open_capture_dialog(self.mode_stream());
        }
    }

    pub fn toggle_recording(&mut self, stream: Stream) {
        if self.recorder.is_active(stream) {
            self.recorder.stop(stream);
            self.notify_info(format!("Stopped recording {}", stream.as_str()));
        } else {
            self.open_capture_dialog(stream);
        }
    }

    /// The dialog on `stream`, each stream's path made from `[capture]` now
    fn open_capture_dialog(&mut self, stream: Stream) {
        let capture = &self.settings.capture;
        let path = |name| capture.path_for(name).to_string_lossy().into_owned();
        let mut dialog =
            FilePathDialog::new(stream, path(&capture.text_name), path(&capture.raw_name));
        if self.recorder.has_session() {
            dialog = dialog.with_session_backup();
        }
        self.open_overlay(Overlay::FilePath(dialog));
    }

    /// The stream the recording toggle starts in each mode: text in line mode, raw in terminal
    /// mode
    fn mode_stream(&self) -> Stream {
        match self.settings.mode {
            Mode::Line => Stream::Text,
            Mode::Terminal => Stream::Raw,
        }
    }

    /// The meta file next to a raw recording, so its replay decodes and shows its line
    /// settings as the session's does (MAX-163): the live link's device, or while replaying the
    /// replayed session's, when its meta gave it (a plain file's device isn't known: no meta).
    /// It starts when the session started when it includes it (MAX-172)
    pub fn write_recording_meta(&mut self, raw: &Path, include_session: bool) {
        let device = if self.link_kind().keeps_sessions() {
            self.session_device()
        } else {
            match &self.replay.device {
                Some(device) => device.clone(),
                None => return,
            }
        };
        let started = (self.recorder.session_started())
            .filter(|_| include_session)
            .unwrap_or_else(chrono::Local::now);
        let meta = SessionMeta::new(started, device, SessionText::default());
        if let Err(err) = meta.write(&SessionMeta::path_of(raw)) {
            self.notify_warn(format!("No meta file for the raw recording: {err}"));
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

    /// Starts recording `stream` to `path`, as the recording dialog asked; a raw recording gets
    /// its meta file. A failure opens the error popup
    pub fn start_capture(&mut self, path: &Path, stream: Stream, include_session: bool) {
        let path = expand_home(path);
        match self.recorder.start(stream, path.clone(), include_session) {
            Ok(_) => {
                self.notify_success(format!("Started recording {}", stream.as_str()));
                if stream == Stream::Raw {
                    self.write_recording_meta(&path, include_session);
                }
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
        if let Some(replaced) = &self.replaced_link {
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
        app::tests::{app, texts},
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
            .start(Stream::Text, path.clone(), false)
            .unwrap();

        app.feed_rx(b"seen\r\n");
        app.recorder.stop(Stream::Text);
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
            .start(Stream::Text, path.clone(), false)
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
        app.recorder.stop(Stream::Text);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "> line\nTX: marked\nbare\n"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// MAX-147: each stream's action opens the dialog on it or stops it alone; the
    /// `toggle-logging` action (the `REC` click) stops both, and `REC` is on with either.
    /// How: text and raw recordings to temp files, then each action in turn.
    #[test]
    fn recording_actions_per_stream() {
        let mut app = app();
        app.settings.mode = Mode::Line;
        let temp = |name: &str| {
            std::env::temp_dir().join(format!("maxicom_test_{}_{name}", std::process::id()))
        };
        let open_dialog = |app: &App| match app.overlay() {
            Some(Overlay::FilePath(dialog)) => Some(dialog.stream),
            _ => None,
        };

        app.update(Action::App(AppAction::ToggleRecording(Stream::Raw)));
        assert_eq!(open_dialog(&app), Some(Stream::Raw));
        app.close_overlay();
        app.update(Action::App(AppAction::ToggleLogging));
        assert_eq!(open_dialog(&app), Some(Stream::Text));
        app.close_overlay();

        app.recorder
            .start(Stream::Text, temp("act.log"), false)
            .unwrap();
        app.recorder
            .start(Stream::Raw, temp("act.raw"), false)
            .unwrap();
        app.update(Action::App(AppAction::ToggleRecording(Stream::Raw)));
        assert!(app.is_logging_active() && !app.is_recording(Stream::Raw));
        app.recorder
            .start(Stream::Raw, temp("act.raw"), false)
            .unwrap();
        app.update(Action::App(AppAction::ToggleLogging));
        assert!(!app.is_logging_active());
        assert!(app.overlay().is_none());
        let _ = std::fs::remove_file(temp("act.log"));
        let _ = std::fs::remove_file(temp("act.raw"));
    }

    /// MAX-148: a restore puts the session's last lines in line mode with their times (a sent
    /// line without its mark), then the separator without a time; the terminal gets the raw
    /// bytes, back on the main screen. Nothing goes to the new session or the stats.
    /// How: a session with "> " marks and times, ending in the alternate screen; an app with
    /// its own session backup restores it.
    #[test]
    fn restore_fills_both_modes_and_records_nothing() {
        let base =
            std::env::temp_dir().join(format!("maxicom_test_{}_restore", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let text = SessionText {
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
        let texts: Vec<_> = history.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts[..3], ["one", "two", "cmd"]);
        assert!(texts[3].starts_with("── restored: ttyUSB0, "), "{texts:?}");
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

    /// MAX-172: a session is its raw bytes and its meta, its text a bonus: without its `.log`
    /// it is still found and restored, the terminal from the raw bytes, line mode with the
    /// separator alone.
    /// How: a session written, its `.log` removed, then found and restored.
    #[test]
    fn session_without_text_restores_from_raw() {
        let dir = crate::capture::tests::temp_dir("restore_no_text");
        let port = LinkConfig::default().address.to_string();
        let session = crate::capture::tests::write_test_session(
            &dir,
            &port,
            &SessionText::default(),
            b"from raw\r\n",
            b"",
        );
        std::fs::remove_file(session.text_path().unwrap()).unwrap();

        let mut app = app();
        app.terminal_mode.set_vt100_size(24, 80);
        app.settings.sessions.restore = RestoreMode::Always;
        app.restore_last_session(Some(&dir), None);
        assert_eq!(texts(&app), ["── restored"]);
        let lines = app.terminal_mode.all_text_lines();
        let separator = lines
            .iter()
            .position(|l| l.starts_with("── restored"))
            .unwrap();
        assert_eq!(lines[separator - 1], "from raw", "{lines:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MAX-172: a raw recording's meta starts when the recording does, or when the session
    /// started when it includes the session.
    /// How: a session backup started, then two metas written a second later, with and without
    /// the session.
    #[test]
    fn raw_recording_meta_starts_with_the_session_it_includes() {
        let dir = crate::capture::tests::temp_dir("meta_started");
        let mut app = app();
        app.start_session(Some(dir.join("sessions")));
        let started = app.recorder.session_started().unwrap().timestamp();
        std::thread::sleep(Duration::from_millis(1100));
        let read = |name: &str| SessionMeta::read(&dir.join(name)).unwrap().started;
        app.write_recording_meta(&dir.join("with.raw"), true);
        app.write_recording_meta(&dir.join("without.raw"), false);
        assert_eq!(read("with.meta"), started);
        assert!(read("without.meta") > started);
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
            &SessionText::default(),
            b"old line\r\n",
            b"",
        );
        let restored = |mode: RestoreMode, flag: Option<bool>, max_age: u64| {
            let mut app = app();
            app.settings.sessions.restore = mode;
            app.settings.sessions.restore_max_age = Duration::from_secs(max_age);
            app.restore_last_session(Some(&dir), flag);
            app.line_mode.history().iter().any(|e| e.text == "old line")
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
            &SessionText::default(),
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
