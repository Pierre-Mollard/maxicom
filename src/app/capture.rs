//! The app's captures (MAX-172): the session backup it starts and switches, the past
//! session it restores at startup, the recordings the user starts, and the session browser.
//! `capture::Recorder` writes the files
use std::path::{Path, PathBuf};

use super::App;
use crate::{
    app::types::LogEntry,
    capture::{
        CaptureSource, RESTORE_RAW_TAIL, Session, SessionDevice, SessionReader, SessionText,
        SessionWriter, Stream, expand_home, find_last_session, read_tail_bytes, read_tail_lines,
        restore_separator,
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

    /// The session backup set aside while a replay replaces the live link (MAX-166): out of the
    /// recorder, its files kept open and locked, so no pruning removes it, and nothing recorded
    /// until `resume_session`
    pub fn pause_session(&mut self) -> Option<SessionWriter> {
        self.recorder.take_session()
    }

    /// The session backup `pause_session` set aside records again, the live link back
    pub fn resume_session(&mut self, session: Option<SessionWriter>) {
        self.recorder.set_session(session);
    }

    /// The captures follow the device, so their meta stays true (MAX-173): a new session backup
    /// when the line settings or the encoding changed since it started; when its source
    /// changed (another address too), the raw recording stops, the text recording gets a
    /// marker line. The session's address is `opened`'s: a new port gets its session once it
    /// is open. Run before the received bytes are recorded, and on each tick for a change with
    /// nothing received after it
    pub fn follow_device(&mut self) {
        let changed = |app: &App, stream| {
            (app.recorder.source(stream)).is_some_and(|source| !app.is_current_source(source))
        };
        if changed(self, Stream::Raw) {
            self.recorder.stop(Stream::Raw);
            self.notify_warn("Raw recording stopped: the device changed");
        }
        if changed(self, Stream::Text) {
            let source = self.capture_source();
            let marker = source.marker(self.settings.encoding);
            self.recorder.mark_device(&marker, source);
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
    }

    /// What a recording started now records: the link's address and device, a replay's device
    /// the one its meta gave, if any (MAX-173)
    fn capture_source(&self) -> CaptureSource {
        let device = if self.link_kind().keeps_sessions() {
            Some(self.session_device())
        } else {
            self.replay.device.clone()
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
        let same_setup = |device: &SessionDevice| {
            !self.link_kind().keeps_sessions() || device.same_setup(config, self.settings.encoding)
        };
        source.address == config.address && source.device.as_ref().is_none_or(same_setup)
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

    /// Starts recording `stream` to `path`, as the recording dialog asked. A raw recording gets
    /// a meta file when its device is known, so its replay decodes and shows its line settings
    /// as a session's does: the live link's, or while replaying the replayed session's (a plain
    /// file's isn't). A failure opens the error popup
    pub fn start_capture(&mut self, path: &Path, stream: Stream, include_session: bool) {
        let path = expand_home(path);
        let source = self.capture_source();
        match self.recorder.start(stream, path, include_session, source) {
            Ok(_) => {
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
        app::tests::{app, record_raw_meta, set_line, texts, toasts},
        capture::SessionMeta,
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
            .start(Stream::Text, path.clone(), false, app.capture_source())
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
            .start(Stream::Text, path.clone(), false, app.capture_source())
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
            .start(Stream::Text, temp("act.log"), false, app.capture_source())
            .unwrap();
        app.recorder
            .start(Stream::Raw, temp("act.raw"), false, app.capture_source())
            .unwrap();
        app.update(Action::App(AppAction::ToggleRecording(Stream::Raw)));
        assert!(app.is_logging_active() && !app.is_recording(Stream::Raw));
        app.recorder
            .start(Stream::Raw, temp("act.raw"), false, app.capture_source())
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
        record_raw_meta(&mut app, &dir.join("with.raw"), true);
        record_raw_meta(&mut app, &dir.join("without.raw"), false);
        assert_eq!(read("with.meta"), started);
        assert!(read("without.meta") > started);
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

        let mut metas: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "meta"))
            .collect();
        metas.sort();
        let sessions: Vec<_> = metas
            .iter()
            .map(|path| {
                let device = SessionMeta::read(path).unwrap().device;
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

    /// MAX-173: a raw recording stops when what it records changes, with a toast: the baud,
    /// the encoding, a replay starting or ending, another replay; a new flow control keeps it,
    /// and a replay's own encoding toggle too (its device comes from its file). The bytes
    /// after the change don't reach it.
    /// How: a recording on the live link, `flow` then `baud` changed with "b" received; one
    /// stopped by the encoding; one stopped by a replay; one during the replay, the encoding
    /// toggled, then stopped by the end of the replay.
    #[test]
    fn raw_recording_stops_when_the_device_changes() {
        let dir = crate::capture::tests::temp_dir("raw_follow");
        let mut app = app();
        // the toast is the last one: each start has its own before
        let stopped = |app: &App| {
            let last = toasts(app).pop();
            !app.is_recording(Stream::Raw)
                && last.as_deref() == Some("Raw recording stopped: the device changed")
        };
        let start = |app: &mut App, name: &str| {
            app.start_capture(&dir.join(name), Stream::Raw, false);
            assert!(app.is_recording(Stream::Raw));
        };

        start(&mut app, "baud.raw");
        app.feed_rx(b"a");
        let mut line = app.link_config().line;
        line.flow_control = crate::link::serial::FlowControl::RtsCts;
        set_line(&mut app, line);
        app.tick();
        assert!(app.is_recording(Stream::Raw));
        line.baud = 9600;
        set_line(&mut app, line);
        app.feed_rx(b"b");
        assert!(stopped(&app));
        assert_eq!(std::fs::read(dir.join("baud.raw")).unwrap(), b"a");

        start(&mut app, "encoding.raw");
        app.settings.encoding = crate::configs::TextEncoding::Cp437;
        app.tick();
        assert!(stopped(&app));

        start(&mut app, "replay.raw");
        let session = crate::capture::tests::device("/dev/ttyUSB1");
        app.start_replay(dir.join("old.raw"), Some(&session));
        app.tick();
        assert!(stopped(&app));

        start(&mut app, "during.raw");
        app.settings.encoding = crate::configs::TextEncoding::Utf8;
        app.tick();
        assert!(app.is_recording(Stream::Raw));
        assert!(app.stop_replay());
        app.tick();
        assert!(stopped(&app));
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
        app.start_capture(&path, Stream::Text, false);
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
        assert!(app.is_recording(Stream::Text));
        app.recorder.stop(Stream::Text);

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
