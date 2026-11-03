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
    let path = std::env::temp_dir().join(format!("maxicom_test_{}_app_tx.log", std::process::id()));
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
    let base = std::env::temp_dir().join(format!("maxicom_test_{}_restore", std::process::id()));
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
