//! The app driving a real link: a TCP listener stands for the device, a file for a replay.
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread,
};

use super::*;
use crate::{
    capture::CaptureFormat,
    configs::{
        KeyBindingsConfig, KeyResolver, SettingsConfig, TextEncoding, settings::RestoreMode,
    },
    inputs::AppAction,
    link::{Address, LinkConfig, state::REOPEN_EVERY},
};

fn app_on(address: &str) -> App {
    App::new(
        SettingsConfig::default(),
        KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap(),
        LinkConfig {
            address: address.parse::<Address>().unwrap(),
            ..LinkConfig::default()
        },
    )
}

fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("tcp://{}", listener.local_addr().unwrap());
    (listener, address)
}

fn toasts(app: &App) -> Vec<String> {
    let toasts = app.notifications.active_toasts();
    toasts.iter().map(|toast| toast.message.clone()).collect()
}

/// Polls the link as the loop does, until `done` or a few seconds
fn poll_until(app: &mut App, what: &str, done: impl Fn(&App) -> bool) {
    let mut buffer = [0; 256];
    let start = Instant::now();
    while !done(app) {
        app.poll_link(&mut buffer).unwrap();
        assert!(start.elapsed() < Duration::from_secs(5), "{what}");
        thread::sleep(Duration::from_millis(5));
    }
}

/// A TCP link opened as at start: connecting, then connected, with the peer's stream
fn connected(listener: &TcpListener, address: &str) -> (App, TcpStream) {
    let mut app = app_on(address);
    app.start_link(None);
    assert!(toasts(&app).contains(&format!("Connecting to {address}")));
    let (peer, _) = listener.accept().unwrap();
    poll_until(&mut app, "connected", |app| app.link.is_open());
    assert!(toasts(&app).contains(&format!("Connected to {address}")));
    (app, peer)
}

/// MAX-168: a TCP link connects in the loop at start; what it sends is read, counted and
/// fed; written bytes reach it, counted; when the peer closes, the link closes with the
/// disconnected popup and a notice.
/// How: a local listener, a line from it, a write, then the peer dropped.
#[test]
fn tcp_link_reads_writes_and_is_lost() {
    let (listener, address) = listener();
    let (mut app, mut peer) = connected(&listener, &address);
    peer.write_all(b"hi\r\n").unwrap();
    poll_until(&mut app, "read", |app| app.stats.rx() == 4);
    assert!(app.terminal_mode.vt100_screen().contents().contains("hi"));

    app.dispatch(Action::TransmitRaw(b"x".to_vec()));
    let mut byte = [0; 1];
    peer.read_exact(&mut byte).unwrap();
    assert_eq!((&byte, app.stats.tx()), (b"x", 1));

    drop(peer);
    poll_until(&mut app, "lost", |app| !app.link.is_open());
    assert!(matches!(app.overlay(), Some(Overlay::DeviceDisconnected)));
    assert!(toasts(&app).contains(&"TCP connection lost".to_string()));
}

/// MAX-199: a lost link keeps its popup, back when closed by hand, and reopens on its own
/// once the device answers again, the popup closed. The path a read error is to take.
/// How: the peer and the listener dropped, Esc in the popup, then a listener on the same
/// port again.
#[test]
fn lost_link_reconnects() {
    use crate::inputs::DialogAction;
    let (listener, address) = listener();
    let (mut app, peer) = connected(&listener, &address);
    drop((peer, listener));
    poll_until(&mut app, "lost", |app| !app.link.is_open());
    assert!(matches!(app.overlay(), Some(Overlay::DeviceDisconnected)));

    app.dispatch(Action::Overlay(DialogAction::Close));
    assert!(matches!(app.overlay(), Some(Overlay::DeviceDisconnected)));

    let listener = TcpListener::bind(address.trim_start_matches("tcp://")).unwrap();
    poll_until(&mut app, "reconnected", |app| app.link.is_open());
    let _peer = listener.accept().unwrap();
    assert!(app.overlay().is_none());
    assert!(toasts(&app).contains(&format!("Reconnected to {address}")));
    // connected, lost and reconnected followed each other in one box (MAX-228)
    let link = app.notifications.active_toasts();
    assert_eq!(link.iter().filter(|t| t.key == Some("link")).count(), 1);
    assert!(!toasts(&app).contains(&"TCP connection lost".to_string()));
}

/// MAX-199: a read error that isn't a gone device closes the link too, once: the lost popup
/// with the error as its reason, no error dialog, back after Esc; the reconnect closes it
/// and forgets the reason. Regression: the port stayed open, its dialog back every frame.
/// How: a TCP link whose device is a replay of a folder, which fails its read (EISDIR, so
/// Unix only: Windows doesn't open a folder as a file), then its listener accepting again.
#[cfg(unix)]
#[test]
fn read_error_is_lost_with_its_reason() {
    use crate::{
        inputs::DialogAction,
        link::{ByteSource, replay::Replay},
    };
    let (listener, address) = listener();
    let mut app = app_on(&address);
    let dir = crate::capture::tests::temp_dir("read_error_is_lost");
    std::fs::create_dir_all(&dir).unwrap();
    let replay = Replay::open(&dir, Duration::ZERO).unwrap();
    app.start_link(Some(ByteSource::Replay(replay)));

    let mut buffer = [0; 64];
    app.poll_link(&mut buffer).unwrap();
    assert!(!app.link.is_open());
    assert!(matches!(app.overlay(), Some(Overlay::DeviceDisconnected)));
    let reason = app.lost_reason().unwrap().to_string();
    assert!(
        reason.starts_with("failed to read from the link"),
        "{reason}"
    );
    assert!(toasts(&app).contains(&"TCP connection lost".to_string()));

    app.dispatch(Action::Overlay(DialogAction::Close));
    assert!(matches!(app.overlay(), Some(Overlay::DeviceDisconnected)));
    assert_eq!(app.lost_reason(), Some(reason.as_str()));

    poll_until(&mut app, "reconnected", |app| app.link.is_open());
    let _peer = listener.accept().unwrap();
    assert!(app.overlay().is_none());
    assert_eq!(app.lost_reason(), None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-199: a write error that leaves the link open is one notice, not one per write; a
/// write that goes through, or the link opening again, lets the next one be said.
/// Regression: a "Write Failed" dialog per write, each key typed bringing it back.
/// How: an open TCP link, its write errors made up (a real one on TCP is a gone link), a
/// write to the peer between them.
#[test]
fn failed_writes_are_said_once() {
    let (listener, address) = listener();
    let (mut app, _peer) = connected(&listener, &address);
    let failed = || LinkError::WriteFailed {
        bytes_attempted: 1,
        source: std::io::Error::other("bad frame"),
    };
    let said = |app: &App| {
        toasts(app)
            .iter()
            .filter(|toast| toast.ends_with("bad frame"))
            .count()
    };
    app.write_failed(&failed());
    app.write_failed(&failed());
    assert_eq!(said(&app), 1);
    assert!(app.link.is_open() && app.overlay().is_none());

    // said again, in the link's box (MAX-228)
    assert!(app.transmit(b"x"));
    app.notifications.dismiss_all();
    app.write_failed(&failed());
    assert_eq!(said(&app), 1);
}

/// MAX-199: a write that finds the device gone closes the link as a read does: the lost
/// popup, without a reason, and its notice, not a write error.
/// How: the peer reset (dropped with a zero linger), then writes until one fails.
#[test]
fn gone_write_closes_the_link() {
    let (listener, address) = listener();
    let (mut app, peer) = connected(&listener, &address);
    socket2::SockRef::from(&peer)
        .set_linger(Some(Duration::ZERO))
        .unwrap();
    drop(peer);

    let start = Instant::now();
    while app.transmit(b"x") {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "writes kept going"
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert!(!app.link.is_open());
    assert!(matches!(app.overlay(), Some(Overlay::DeviceDisconnected)));
    assert_eq!(app.lost_reason(), None);
    assert!(toasts(&app).contains(&"TCP connection lost".to_string()));
    assert!(
        !toasts(&app)
            .iter()
            .any(|t| t.starts_with("TCP connection:"))
    );
}

/// MAX-168: `open` switches the link and resumes the command line on success; a failed
/// open goes back to the link that was open and says why.
/// How: a link on one listener opened onto a second, then onto a closed port.
#[test]
fn open_switches_and_goes_back_on_failure() {
    let (first, first_address) = listener();
    let (second, second_address) = listener();
    let (mut app, _peer) = connected(&first, &first_address);
    app.dispatch(Action::App(AppAction::Open(
        second_address.parse::<Address>().unwrap(),
    )));
    assert!(!app.link.is_open());
    assert!(toasts(&app).contains(&format!("Opening {second_address}")));
    let _second_peer = second.accept().unwrap();
    poll_until(&mut app, "opened", |app| app.link.is_open());
    assert!(toasts(&app).contains(&format!("Opened {second_address}")));

    let (closed, closed_address) = listener();
    drop(closed);
    app.dispatch(Action::App(AppAction::Open(
        closed_address.parse::<Address>().unwrap(),
    )));
    poll_until(&mut app, "failed", |app| {
        app.link_config().address == second_address.parse::<Address>().unwrap()
    });
    let failed = toasts(&app);
    assert!(
        failed.iter().any(|toast| toast.starts_with("open: ")
            && toast.ends_with(&format!("back to {second_address}"))),
        "{failed:?}"
    );
}

/// MAX-187: each device switched to gets a session backup of its own, A -> B -> A three of
/// them; a failed open goes back to the link that was open with its views, and its backup
/// records on.
/// How: "one" on A, `open` B, "two"; `open` of a closed port, B back, "three"; `open` A,
/// "again"; the session files read back.
#[test]
fn switch_backups_and_failed_open_keep_the_history() {
    let sessions = std::env::temp_dir().join(format!(
        "maxicom_test_{}_switch_backups",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&sessions);
    let (first, first_address) = listener();
    let (second, second_address) = listener();
    let (mut app, mut peer) = connected(&first, &first_address);
    app.start_session(Some(sessions.clone()));
    let open = |app: &mut App, address: &str| {
        app.dispatch(Action::App(AppAction::Open(
            address.parse::<Address>().unwrap(),
        )));
    };
    let receive = |app: &mut App, peer: &mut TcpStream, text: &str| {
        peer.write_all(format!("{text}\r\n").as_bytes()).unwrap();
        poll_until(app, text, |app| {
            app.terminal_mode.vt100_screen().contents().contains(text)
        });
    };
    receive(&mut app, &mut peer, "one");

    open(&mut app, &second_address);
    let (mut peer, _) = second.accept().unwrap();
    poll_until(&mut app, "opened", |app| app.link.is_open());
    receive(&mut app, &mut peer, "two");

    let (closed, closed_address) = listener();
    drop(closed);
    open(&mut app, &closed_address);
    // the link goes back to B in the polls: accepted between them
    second.set_nonblocking(true).unwrap();
    let start = Instant::now();
    let mut peer = loop {
        app.poll_link(&mut [0; 256]).unwrap();
        if let Ok((peer, _)) = second.accept() {
            break peer;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "back");
        thread::sleep(Duration::from_millis(5));
    };
    peer.set_nonblocking(false).unwrap();
    poll_until(&mut app, "back", |app| app.link.is_open());
    assert_eq!(app.link_config().address.to_string(), second_address);
    assert!(app.terminal_mode.vt100_screen().contents().contains("two"));
    receive(&mut app, &mut peer, "three");

    open(&mut app, &first_address);
    let (mut peer, _) = first.accept().unwrap();
    poll_until(&mut app, "opened", |app| app.link.is_open());
    receive(&mut app, &mut peer, "again");
    drop(app);

    // the session files' raw bytes, the oldest first
    let raws: Vec<Vec<u8>> = crate::capture::tests::session_paths(&sessions)
        .iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "raw"))
        .map(|path| std::fs::read(path).unwrap())
        .collect();
    let expected: [&[u8]; 3] = [b"one\r\n", b"two\r\nthree\r\n", b"again\r\n"];
    assert_eq!(raws, expected);
    let _ = std::fs::remove_dir_all(&sessions);
}

/// MAX-187: a device switched to starts as at start on it: a clean screen and fresh counts,
/// its last session restored as `sessions.restore` says, then its own backup. Back on A,
/// A's session comes back; `--no-restore` holds for the switches; with `ask`, the dialog
/// asks on the switch and what comes in waits for the answer.
/// How: a session of B with "old b"; A with "one", `open` B, "two"; `open` A; `open` B
/// with the flag off; `open` A with `ask`, "live" while asking, then Enter.
#[test]
fn switch_clears_and_restores_the_device() {
    let sessions = crate::capture::tests::temp_dir("switch_restores");
    let (first, first_address) = listener();
    let (second, second_address) = listener();
    crate::capture::tests::write_test_session(
        &sessions,
        &second_address,
        &CaptureFormat::default(),
        b"old b\r\n",
        b"",
    );
    let (mut app, mut peer) = connected(&first, &first_address);
    app.settings.sessions.restore = RestoreMode::Always;
    app.start_session(Some(sessions.clone()));
    let open = |app: &mut App, address: &str, listener: &TcpListener| {
        app.dispatch(Action::App(AppAction::Open(
            address.parse::<Address>().unwrap(),
        )));
        let (peer, _) = listener.accept().unwrap();
        poll_until(app, "opened", |app| app.link.is_open());
        peer
    };
    let receive = |app: &mut App, peer: &mut TcpStream, text: &str| {
        peer.write_all(format!("{text}\r\n").as_bytes()).unwrap();
        poll_until(app, text, |app| {
            app.terminal_mode.vt100_screen().contents().contains(text)
        });
    };
    // in line mode's history: the terminal's restore goes to its scrollback
    let shows = |app: &App, text: &str| {
        (app.line_mode.history().iter()).any(|entry| entry.text.contains(text))
    };
    let on_screen =
        |app: &App, text: &str| app.terminal_mode.vt100_screen().contents().contains(text);
    receive(&mut app, &mut peer, "one");

    let mut peer = open(&mut app, &second_address, &second);
    assert!(!shows(&app, "one") && !on_screen(&app, "one"));
    assert!(shows(&app, "old b"));
    assert_eq!(app.stats.rx(), 0);
    receive(&mut app, &mut peer, "two");

    let _peer = open(&mut app, &first_address, &first);
    assert!(shows(&app, "one"));
    assert!(!shows(&app, "two") && !on_screen(&app, "two") && !shows(&app, "old b"));

    app.set_restore_flag(Some(false));
    let _peer = open(&mut app, &second_address, &second);
    assert!(!shows(&app, "one") && !shows(&app, "two"));

    app.set_restore_flag(None);
    app.settings.sessions.restore = RestoreMode::Ask;
    let mut peer = open(&mut app, &first_address, &first);
    assert!(matches!(app.overlay(), Some(Overlay::RestoreSession(_))));
    peer.write_all(b"live\r\n").unwrap();
    poll_until(&mut app, "held", |app| {
        app.held_rx.as_ref().is_some_and(|held| !held.is_empty())
    });
    assert!(!shows(&app, "live"));
    app.dispatch(Action::Overlay(crate::inputs::DialogAction::Confirm));
    assert!(shows(&app, "one") && shows(&app, "live") && on_screen(&app, "live"));
    let _ = std::fs::remove_dir_all(&sessions);
}

/// MAX-187: the rest of a command line waits while the restore dialog asks on the device
/// its `open` switched to, and goes on after the answer.
/// How: a session of B; `open B; send x` typed on A with `ask`; the peer reads nothing
/// while asking, then "x" after Enter.
#[test]
fn switch_commands_wait_for_the_restore_answer() {
    let sessions = crate::capture::tests::temp_dir("switch_commands_wait");
    let (first, first_address) = listener();
    let (second, second_address) = listener();
    crate::capture::tests::write_test_session(
        &sessions,
        &second_address,
        &CaptureFormat::default(),
        b"old b\r\n",
        b"",
    );
    let (mut app, _peer) = connected(&first, &first_address);
    app.settings.sessions.restore = RestoreMode::Ask;
    app.start_session(Some(sessions.clone()));
    let run = |app: &mut App| {
        while let Some(action) = app.next_command_action(Instant::now()) {
            app.dispatch(action);
        }
    };
    let command_line = crate::configs::KeyBindingAction::CommandLine.to_action(&app);
    app.dispatch(command_line);
    let line = format!("open {second_address}; send x");
    app.dispatch(Action::Overlay(crate::inputs::DialogAction::Paste(line)));
    app.dispatch(Action::Overlay(crate::inputs::DialogAction::Confirm));
    run(&mut app);
    let (mut peer, _) = second.accept().unwrap();
    poll_until(&mut app, "opened", |app| app.link.is_open());
    assert!(matches!(app.overlay(), Some(Overlay::RestoreSession(_))));
    run(&mut app);
    peer.set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let mut byte = [0; 1];
    assert!(peer.read_exact(&mut byte).is_err(), "sent while asking");

    app.dispatch(Action::Overlay(crate::inputs::DialogAction::Confirm));
    run(&mut app);
    peer.set_read_timeout(None).unwrap();
    peer.read_exact(&mut byte).unwrap();
    assert_eq!(&byte, b"x");
    let _ = std::fs::remove_dir_all(&sessions);
}

/// MAX-187: the first port picked from the start's directory gets its last session
/// restored before its backup starts, as a port given on the command line does.
/// How: a session of the port with "old", the app started on the directory, then a pick.
#[test]
fn first_pick_restores_its_session() {
    let sessions = crate::capture::tests::temp_dir("first_pick_restores");
    let (listener, address) = listener();
    crate::capture::tests::write_test_session(
        &sessions,
        &address,
        &CaptureFormat::default(),
        b"old\r\n",
        b"",
    );
    let mut app = app_on(&address);
    app.settings.sessions.restore = RestoreMode::Always;
    app.start_in_directory(vec![address.clone()], Some(sessions.clone()));
    app.dispatch(Action::Overlay(crate::inputs::DialogAction::Confirm));
    let _peer = listener.accept().unwrap();
    poll_until(&mut app, "opened", |app| app.link.is_open());
    assert!(app.line_mode.history().iter().any(|e| e.text == "old\r\n"));
    assert!(app.recorder.has_session());
    let _ = std::fs::remove_dir_all(&sessions);
}

/// MAX-168: the detach key closes the link with its popup, and the link isn't reopened
/// while detached, even when another popup takes the detached one's place; the key again
/// reattaches it and closes the popup.
/// How: a TCP link detached, the quit question opened over it, polled past the reopen
/// delay, then attached.
#[test]
fn detach_then_attach() {
    let (listener, address) = listener();
    let (mut app, _peer) = connected(&listener, &address);
    app.dispatch(Action::App(AppAction::ToggleAttach));
    assert!(!app.link.is_open());
    assert!(matches!(app.overlay(), Some(Overlay::DeviceDetached)));
    app.open_overlay(Overlay::ConfirmQuit);
    let mut buffer = [0; 256];
    let until = Instant::now() + REOPEN_EVERY * 2;
    while Instant::now() < until {
        app.poll_link(&mut buffer).unwrap();
        thread::sleep(Duration::from_millis(20));
    }
    assert!(!app.link.is_open());

    app.close_overlay();
    app.dispatch(Action::App(AppAction::ToggleAttach));
    let _peer = listener.accept().unwrap();
    poll_until(&mut app, "attached", |app| app.link.is_open());
    assert!(toasts(&app).contains(&format!("Reconnected to {address}")));
    assert!(app.overlay().is_none());
}

/// MAX-168: new line settings on a detached port are kept for its next open, with a
/// notice; on a link that isn't a port they change nothing.
/// How: a serial port that isn't there, a 9600 baud config; then a TCP link.
#[test]
fn reconfigure_keeps_settings_for_a_detached_port() {
    let mut app = app_on("/nonexistent/ttyMAXICOM");
    let line = LineSettings {
        baud: 9600,
        ..app.link_config().line
    };
    app.apply_link_effect(UpdateEffect::Reconfigure(line));
    assert_eq!(app.link_config().line.baud, 9600);
    let kept = "The port is detached: it opens with these settings";
    assert!(toasts(&app).contains(&kept.to_string()));

    let (listener, address) = listener();
    let (mut app, _peer) = connected(&listener, &address);
    let line = LineSettings {
        baud: 9600,
        ..app.link_config().line
    };
    app.apply_link_effect(UpdateEffect::Reconfigure(line));
    assert_eq!(app.link_config().line.baud, 115_200);
}

/// MAX-168: a link quiet for `inactivity_detach_timeout` is detached, with its popup.
/// How: a TCP link with a short timeout, nothing sent.
#[test]
fn idle_link_is_detached() {
    let (listener, address) = listener();
    let (mut app, _peer) = connected(&listener, &address);
    app.settings_mut().inactivity_detach_timeout = Duration::from_millis(50);
    poll_until(&mut app, "detached", |app| !app.link.is_open());
    assert!(matches!(app.overlay(), Some(Overlay::DeviceDetached)));
    assert!(toasts(&app).contains(&"Device idle for 0s: port detached".to_string()));
}

/// MAX-168: a replay takes the link's place and is read, its end said once; stopping it
/// reopens the live link as at start.
/// How: a TCP link, a replay of a small file, the stop.
#[test]
fn replay_reads_ends_and_gives_the_link_back() {
    let (listener, address) = listener();
    let (mut app, _peer) = connected(&listener, &address);
    let path = std::env::temp_dir().join(format!(
        "maxicom_test_{}_device_replay.raw",
        std::process::id()
    ));
    std::fs::write(&path, b"replayed\r\n").unwrap();
    app.dispatch(Action::App(AppAction::Replay {
        path: path.clone(),
        delay: Duration::ZERO,
    }));
    assert_eq!(app.link_kind(), LinkKind::Replay);
    poll_until(&mut app, "finished", App::is_replay_ended);
    let finished = |app: &App| {
        let toasts = toasts(app);
        toasts.iter().filter(|t| *t == "Replay finished").count()
    };
    assert_eq!(finished(&app), 1);
    app.poll_link(&mut [0; 256]).unwrap();
    assert_eq!(finished(&app), 1);
    assert!(
        app.terminal_mode
            .vt100_screen()
            .contents()
            .contains("replayed")
    );

    app.dispatch(Action::App(AppAction::StopReplay));
    assert_eq!(
        app.link_config().address,
        address.parse::<Address>().unwrap()
    );
    let _peer = listener.accept().unwrap();
    poll_until(&mut app, "back", |app| app.link.is_open());
    let _ = std::fs::remove_file(&path);
}

/// MAX-171: `open` during a replay that set a live link aside ends the replay first: the live
/// link comes back with its encoding and its session backup, then the new link opens with a
/// session backup of its own, and the attach key no longer goes back to the old link. Opening
/// the live link again keeps its session backup.
/// How: a TCP link with a session backup, "one" received; a cp437 session file replayed; `open`
/// onto a second listener, "two" received there, then the attach key; the session files read
/// back. Then the same with `open` onto the live link, "again" received.
#[test]
fn open_during_a_replay_ends_it_first() {
    let root = std::env::temp_dir().join(format!(
        "maxicom_test_{}_open_during_replay",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let sessions = root.join("sessions");
    let replayed = root.join("files").join("replayed.raw");
    std::fs::create_dir_all(replayed.parent().unwrap()).unwrap();
    std::fs::write(&replayed, b"old\r\n").unwrap();
    let (first, first_address) = listener();
    let device = CaptureDevice::new(
        &LinkConfig {
            address: first_address.parse::<Address>().unwrap(),
            ..LinkConfig::default()
        },
        TextEncoding::Cp437,
    );
    RecordingMeta::new(chrono::Local::now(), device, CaptureFormat::default())
        .write(&RecordingMeta::path_of(&replayed))
        .unwrap();
    // the session files' raw bytes, the oldest first
    let raws = || -> Vec<Vec<u8>> {
        crate::capture::tests::session_paths(&sessions)
            .iter()
            .filter(|path| path.extension().is_some_and(|ext| ext == "raw"))
            .map(|path| std::fs::read(path).unwrap())
            .collect()
    };
    let live_with_replay = || {
        let (mut app, mut peer) = connected(&first, &first_address);
        app.start_session(Some(sessions.clone()));
        peer.write_all(b"one\r\n").unwrap();
        poll_until(&mut app, "one", |app| app.stats.rx() == 5);
        let live_encoding = app.settings().encoding;
        app.dispatch(Action::App(AppAction::Replay {
            path: replayed.clone(),
            delay: Duration::ZERO,
        }));
        assert_eq!(app.settings().encoding, TextEncoding::Cp437);
        (app, live_encoding)
    };
    let open = |app: &mut App, address: &str, listener: &TcpListener, text: &[u8]| {
        app.dispatch(Action::App(AppAction::Open(
            address.parse::<Address>().unwrap(),
        )));
        let (mut peer, _) = listener.accept().unwrap();
        poll_until(app, "opened", |app| app.link.is_open());
        peer.write_all(text).unwrap();
        let text = String::from_utf8_lossy(text).trim().to_string();
        poll_until(app, "received", |app| {
            app.terminal_mode.vt100_screen().contents().contains(&text)
        });
    };

    let (second, second_address) = listener();
    let (mut app, live_encoding) = live_with_replay();
    open(&mut app, &second_address, &second, b"two\r\n");
    assert!(app.replaced_link().is_none());
    assert_eq!(app.settings().encoding, live_encoding);
    app.dispatch(Action::App(AppAction::ToggleAttach));
    assert!(matches!(app.overlay(), Some(Overlay::DeviceDetached)));
    let second_address = second_address.parse::<Address>().unwrap();
    assert_eq!(app.link_config().address, second_address);
    drop(app);
    assert_eq!(raws(), [b"one\r\n".to_vec(), b"two\r\n".to_vec()]);

    std::fs::remove_dir_all(&sessions).unwrap();
    let (mut app, _) = live_with_replay();
    open(&mut app, &first_address, &first, b"again\r\n");
    drop(app);
    assert_eq!(raws(), [b"one\r\nagain\r\n".to_vec()]);
    let _ = std::fs::remove_dir_all(&root);
}

/// MAX-166: the replay controls from the command line: a paused replay reads nothing
/// until resumed, its delay changes, a restart plays it again from the top on a clean
/// screen, not ended; each refuses with its reason when it would change nothing, and all
/// refuse without a replay. MAX-175: a `.log` isn't replayed.
/// How: a `.log` refused, then a 3-line file replayed with a long delay, then each command.
#[test]
fn replay_controls() {
    let run = |app: &mut App, line: &str| {
        let invocations = crate::commands::parse(line).unwrap();
        app.dispatch(Action::App(AppAction::QueueCommands(invocations)));
        while let Some(action) = app.next_command_action(Instant::now()) {
            app.dispatch(action);
        }
        toasts(app).last().cloned().unwrap_or_default()
    };
    let mut app = app_on("/nonexistent/ttyMAXICOM");
    assert_eq!(
        run(&mut app, "replay-pause"),
        "replay-pause: no replay runs"
    );

    // a text recording isn't replayed: what it holds isn't what was received (MAX-175)
    let log = std::env::temp_dir().join(format!(
        "maxicom_test_{}_device_controls.log",
        std::process::id()
    ));
    std::fs::write(&log, b"one\n").unwrap();
    let refused = run(&mut app, &format!("replay {}", log.display()));
    assert!(
        refused.ends_with("not a recording (a .raw, its .meta or its folder)"),
        "{refused}"
    );
    assert_ne!(app.link_kind(), LinkKind::Replay);
    let _ = std::fs::remove_file(&log);

    let path = std::env::temp_dir().join(format!(
        "maxicom_test_{}_device_controls.raw",
        std::process::id()
    ));
    std::fs::write(&path, b"one\r\ntwo\r\nthree\r\n").unwrap();
    run(&mut app, &format!("replay {} 10000", path.display()));
    assert_eq!(app.replay().unwrap().progress(), (0, 17));
    assert_eq!(
        run(&mut app, "replay-resume"),
        "replay-resume: the replay isn't paused"
    );
    assert_eq!(run(&mut app, "replay-pause"), "Replay paused");
    assert_eq!(
        run(&mut app, "replay-pause"),
        "replay-pause: the replay is paused already"
    );
    assert_eq!(run(&mut app, "replay-speed 0"), "Replay delay 0 ms");
    let mut buffer = [0; 256];
    app.poll_link(&mut buffer).unwrap();
    assert_eq!(app.replay().unwrap().progress().0, 0);
    assert_eq!(run(&mut app, "replay-resume"), "Replay resumed");
    poll_until(&mut app, "finished", App::is_replay_ended);
    assert_eq!(app.replay().unwrap().percent(), 100);
    assert!(
        app.terminal_mode
            .vt100_screen()
            .contents()
            .contains("three")
    );
    assert_eq!(
        run(&mut app, "replay-pause"),
        "replay-pause: the replay is finished (replay-restart plays it again)"
    );

    assert_eq!(run(&mut app, "replay-restart"), "Replay restarted");
    assert!(!app.is_replay_ended());
    assert!(
        !app.terminal_mode
            .vt100_screen()
            .contents()
            .contains("three")
    );
    assert_eq!(app.stats.rx(), 0);
    poll_until(&mut app, "again", App::is_replay_ended);
    assert!(
        app.terminal_mode
            .vt100_screen()
            .contents()
            .contains("three")
    );
    assert_eq!(app.stats.rx(), 17);
    let _ = std::fs::remove_file(&path);
}

/// MAX-168: a control line needs an open link with control lines: a closed link and a
/// TCP link refuse it, with the reason.
/// How: DTR on a link not open yet, then on an open TCP link.
#[test]
fn line_control_needs_a_port() {
    let (listener, address) = listener();
    let mut app = app_on(&address);
    app.dispatch(Action::App(AppAction::LineControl(LineControl::Dtr(true))));
    assert!(
        toasts(&app)
            .iter()
            .any(|t| t.ends_with("the port is detached"))
    );
    drop(app);
    let (mut app, _peer) = connected(&listener, &address);
    app.dispatch(Action::App(AppAction::LineControl(LineControl::Dtr(true))));
    let refused = toasts(&app);
    assert!(refused.iter().any(|t| t.starts_with("DTR")), "{refused:?}");
}

/// MAX-181: started on the directory, the link waits on the first port without opening
/// it, past the reopen delay too; the directory is open, no session backup runs, and the
/// attach key opens the directory. The port picked opens, and gets its backup even when
/// it's the one the link waited on.
/// How: a TCP listener as the port; polls past `REOPEN_EVERY`, the attach key, then Enter
/// in the directory.
#[test]
fn start_in_directory_opens_nothing_until_picked() {
    let (listener, address) = listener();
    listener.set_nonblocking(true).unwrap();
    let mut app = app_on(&address);
    app.start_in_directory(vec![address.clone()], None);
    assert!(app.is_device_unpicked());
    assert!(matches!(app.overlay(), Some(Overlay::Directory(_))));
    let mut buffer = [0; 256];
    let start = Instant::now();
    while start.elapsed() < REOPEN_EVERY * 2 {
        app.poll_link(&mut buffer).unwrap();
        thread::sleep(Duration::from_millis(20));
    }
    assert!(!app.link.is_open());
    assert!(listener.accept().is_err(), "a connection came in");
    assert!(!app.recorder.has_session());

    // the system's ports in the directory the attach key opens
    app.close_overlay();
    app.dispatch(Action::App(AppAction::ToggleAttach));
    assert!(matches!(app.overlay(), Some(Overlay::Directory(_))));
    assert!(!app.link.is_open());
    app.open_directory(vec![address.clone()]);
    app.dispatch(Action::Overlay(crate::inputs::DialogAction::Confirm));
    listener.set_nonblocking(false).unwrap();
    let _peer = listener.accept().unwrap();
    poll_until(&mut app, "opened", |app| app.link.is_open());
    assert!(!app.is_device_unpicked());
    assert!(app.overlay().is_none(), "the directory closes once open");
    // no cache folder in the test: the backup tried to start
    let tried = toasts(&app)
        .iter()
        .any(|t| t.starts_with("Session backup off"));
    assert!(tried, "{:?}", toasts(&app));
}

/// MAX-181: a reattach that fails (the port taken by another program meanwhile) leaves the
/// link detached, not retrying, its popup back; `d` there opens the directory, where the
/// port isn't "in use" and is tested, and closing the directory brings the popup back.
/// Regression: the popup was gone and the link retried quietly.
/// How: a TCP link detached, its listener closed, Enter (reattach) in the popup; then `d`,
/// the directory's rows, Esc.
#[test]
fn failed_reattach_keeps_the_detached_popup() {
    use crate::inputs::DialogAction;
    use crossterm::event::{KeyCode, KeyEvent};
    let (listener, address) = listener();
    let (mut app, peer) = connected(&listener, &address);
    app.dispatch(Action::App(AppAction::ToggleAttach));
    assert!(matches!(app.overlay(), Some(Overlay::DeviceDetached)));
    drop((listener, peer));

    app.dispatch(Action::Overlay(DialogAction::Confirm));
    poll_until(&mut app, "reattach failed", |app| {
        toasts(app)
            .iter()
            .any(|t| t.starts_with("Failed to reattach"))
    });
    assert!(matches!(app.overlay(), Some(Overlay::DeviceDetached)));
    assert!(app.link.is_detached() && !app.link.is_open());

    let d = KeyEvent::from(KeyCode::Char('d'));
    app.dispatch(Action::Overlay(DialogAction::HandleKey(d)));
    assert!(matches!(app.overlay(), Some(Overlay::Directory(_))));
    app.open_directory(vec![address.clone()]);
    app.test_links();
    let Some(Overlay::Directory(directory)) = app.overlay() else {
        panic!("no directory");
    };
    assert!(
        directory.tests.contains_key(&address),
        "the port isn't maxicom's"
    );
    app.link_test = None;
    app.dispatch(Action::Overlay(DialogAction::Close));
    assert!(matches!(app.overlay(), Some(Overlay::DeviceDetached)));
}

/// MAX-181: from the start's directory, picking another port than the one the link waited
/// on ends the waiting too: the next action brings no "no device" popup, the attach key
/// detaches it. Before the pick, the test checks the first port too, nothing being in use.
/// Regression: `switched || take(..)` didn't take when switched.
/// How: two TCP listeners, the test started then Down and Enter, an action, the attach key.
#[test]
fn start_in_directory_pick_another_port() {
    use crate::inputs::DialogAction;
    let (first, first_address) = listener();
    let (second, second_address) = listener();
    let mut app = app_on(&first_address);
    let ports = vec![first_address.clone(), second_address.clone()];
    app.start_in_directory(ports, None);
    app.test_links();
    let Some(Overlay::Directory(directory)) = app.overlay() else {
        panic!("no directory");
    };
    assert!(directory.tests.contains_key(&first_address));
    app.link_test = None;

    app.dispatch(Action::Overlay(DialogAction::Down));
    app.dispatch(Action::Overlay(DialogAction::Confirm));
    let _peer = second.accept().unwrap();
    poll_until(&mut app, "opened", |app| app.link.is_open());
    assert_eq!(app.link_config().address.to_string(), second_address);
    assert!(!app.is_device_unpicked());
    app.dispatch(Action::App(AppAction::SetPrefix(false)));
    assert!(
        app.overlay().is_none(),
        "{:?}",
        app.overlay().map(|_| "a popup")
    );
    app.dispatch(Action::App(AppAction::ToggleAttach));
    assert!(matches!(app.overlay(), Some(Overlay::DeviceDetached)));
    drop(first);
}

/// MAX-185 step 0: with a link open, a port picked in the directory opens with the line
/// settings the link had, and closes the directory; one that fails puts the link back on
/// the address that was open, the line unchanged, the directory staying with the reason
/// on the port's row.
/// How: a TCP link at 9600, the directory on a second listener then on a closed one,
/// Enter in each.
#[test]
fn directory_switch_keeps_the_line_and_goes_back() {
    use crate::inputs::DialogAction;
    let (first, first_address) = listener();
    let mut config = LinkConfig {
        address: first_address.parse().unwrap(),
        ..LinkConfig::default()
    };
    config.line.baud = 9600;
    let line = config.line;
    let mut app = App::new(
        SettingsConfig::default(),
        KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap(),
        config,
    );
    app.start_link(None);
    let _peer = first.accept().unwrap();
    poll_until(&mut app, "connected", |app| app.link.is_open());

    let (second, second_address) = listener();
    app.open_directory(vec![second_address.clone()]);
    app.dispatch(Action::Overlay(DialogAction::Confirm));
    let _second_peer = second.accept().unwrap();
    poll_until(&mut app, "opened", |app| app.link.is_open());
    assert_eq!(app.link_config().address.to_string(), second_address);
    assert_eq!(app.link_config().line, line);
    assert!(app.overlay().is_none(), "the directory closes once open");

    let (closed, closed_address) = listener();
    drop(closed);
    app.open_directory(vec![closed_address.clone()]);
    app.dispatch(Action::Overlay(DialogAction::Confirm));
    poll_until(&mut app, "failed", |app| {
        toasts(app).iter().any(|t| t.starts_with("open: "))
    });
    assert_eq!(app.link_config().address.to_string(), second_address);
    assert_eq!(app.link_config().line, line);
    let Some(Overlay::Directory(directory)) = app.overlay() else {
        panic!("the directory closed");
    };
    assert!(directory.opening.is_none());
    assert!(matches!(
        directory.tests[&closed_address],
        Some(ProbeState::Error(_))
    ));
}

/// MAX-185: the directory opens on the saved links, on the one in use; a saved link that
/// fails to open puts back the address and the line that were open, the reason on its
/// row; one that opens has its own line settings, and closes the directory.
/// How: a TCP link open and saved; a saved missing device at 57600, then a pty at 9600,
/// each picked with Up and Enter.
#[cfg(target_os = "linux")]
#[test]
fn directory_opens_a_saved_link_with_its_line() {
    use crate::{app::tests::Pty, inputs::DialogAction, overlays::DirectoryTab};
    let pty = Pty::open();
    let (listener, address) = listener();
    let (mut app, _peer) = connected(&listener, &address);
    let before = app.link_config().clone();
    let links = format!(
        "[[link]]\nname = 'board'\naddress = '{}'\nbaud = 9600\n\
             [[link]]\nname = 'gone'\naddress = '/nonexistent/tty'\nbaud = 57600\n\
             [[link]]\nname = 'bridge'\naddress = '{address}'\n",
        pty.path
    );
    app.set_links(Ok(toml::from_str(&links).unwrap()));
    app.open_directory(Vec::new());
    let Some(Overlay::Directory(directory)) = app.overlay() else {
        panic!("no directory");
    };
    assert_eq!(directory.tab, DirectoryTab::Saved);
    assert_eq!(directory.link_selected, 2);

    app.dispatch(Action::Overlay(DialogAction::Up));
    app.dispatch(Action::Overlay(DialogAction::Confirm));
    poll_until(&mut app, "failed", |app| {
        toasts(app).iter().any(|t| t.starts_with("open: "))
    });
    assert_eq!(*app.link_config(), before);
    let Some(Overlay::Directory(directory)) = app.overlay() else {
        panic!("the directory closed");
    };
    assert!(directory.opening.is_none());
    assert!(matches!(
        directory.tests.get("/nonexistent/tty"),
        Some(Some(_))
    ));

    app.dispatch(Action::Overlay(DialogAction::Up));
    app.dispatch(Action::Overlay(DialogAction::Confirm));
    poll_until(&mut app, "opened", |app| {
        app.link.is_open() && app.link_config().address.to_string() == pty.path
    });
    assert_eq!(app.link_config().line.baud, 9600);
    assert!(app.overlay().is_none(), "the directory closes once open");
}

/// MAX-185: `s` in the saved links opens the link form on the link in use, the name its
/// address's to start with; Enter saves it to `links.toml` and brings the directory back
/// on it. Saved already, `s` says so. `s` in the ports opens it on the selected port, for
/// this machine with `[link]`'s line; Esc there goes back to the directory; a name taken
/// or empty keeps the form, saying why. Before a pick there's no link in use to save.
/// How: a TCP link open, `links_path` in a temp folder; each key through `dispatch`.
#[test]
fn save_links_from_the_directory() {
    use crate::inputs::DialogAction;
    use crossterm::event::{KeyCode, KeyEvent};
    let dir = std::env::temp_dir().join(format!("maxicom_save_{}", std::process::id()));
    let path = dir.join("links.toml");
    let (listener, address) = listener();
    let (mut app, _peer) = connected(&listener, &address);
    app.settings_mut().links_path = Some(path.clone());
    let s = || Action::Overlay(DialogAction::HandleKey(KeyEvent::from(KeyCode::Char('s'))));
    let name_dialog = |app: &App| match app.overlay() {
        Some(Overlay::LinkForm(form)) => Some(form.clone()),
        _ => None,
    };
    let set_name = |app: &mut App, name: &str| {
        if let Some(Overlay::LinkForm(form)) = &mut app.overlay {
            form.name = name.to_string().into();
        }
    };

    app.open_directory(vec!["/dev/ttyS9".into()]);
    app.dispatch(Action::Overlay(DialogAction::CycleNext));
    app.dispatch(s());
    let dialog = name_dialog(&app).expect("no name asked");
    let host_port = address.trim_start_matches("tcp://");
    assert_eq!(dialog.name.value(), host_port);
    assert!(dialog.hosts.value().is_empty() && !dialog.has_line());
    set_name(&mut app, "bridge");
    app.dispatch(Action::Overlay(DialogAction::Confirm));
    assert!(toasts(&app).contains(&"Saved link 'bridge'".to_string()));
    let Some(Overlay::Directory(directory)) = app.overlay() else {
        panic!("no directory");
    };
    assert_eq!(directory.tab, DirectoryTab::Saved);
    assert_eq!(directory.link_selected, 0);
    let saved = LinksConfig::load(&path).unwrap();
    assert_eq!(saved.links[0].name, "bridge");
    assert_eq!(saved.links[0].address, address);

    app.dispatch(s());
    assert!(matches!(app.overlay(), Some(Overlay::Directory(_))));
    let already = format!("{address} is saved already, as 'bridge'");
    assert!(toasts(&app).contains(&already), "{:?}", toasts(&app));

    // the save listed the system's ports: the test's again
    app.open_directory(vec!["/dev/ttyS9".into()]);
    app.dispatch(Action::Overlay(DialogAction::CycleNext));
    app.dispatch(s());
    let dialog = name_dialog(&app).expect("no name asked");
    assert_eq!(dialog.name.value(), "ttyS9");
    assert_eq!(dialog.hosts.value(), app.host.clone().unwrap_or_default());
    assert_eq!(dialog.baud.value(), "115200");
    app.dispatch(Action::Overlay(DialogAction::Close));
    assert!(matches!(app.overlay(), Some(Overlay::Directory(_))));

    app.open_directory(vec!["/dev/ttyS9".into()]);
    app.dispatch(Action::Overlay(DialogAction::CycleNext));
    app.dispatch(s());
    set_name(&mut app, "bridge");
    app.dispatch(Action::Overlay(DialogAction::Confirm));
    assert!(name_dialog(&app).is_some(), "the form closed");
    let taken = "bridge: link #1 has this name on the same machine".to_string();
    assert!(toasts(&app).contains(&taken), "{:?}", toasts(&app));
    set_name(&mut app, " ");
    app.dispatch(Action::Overlay(DialogAction::Confirm));
    assert!(name_dialog(&app).is_some(), "the form closed");
    assert_eq!(LinksConfig::load(&path).unwrap().links.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);

    let mut app = app_on("/dev/ttyS9");
    app.start_in_directory(vec!["/dev/ttyS9".into()], None);
    app.dispatch(Action::Overlay(DialogAction::CycleNext));
    app.dispatch(s());
    assert!(matches!(app.overlay(), Some(Overlay::Directory(_))));
    assert!(toasts(&app).contains(&"No link in use: pick one first".to_string()));
}

/// MAX-185: `n` in the saved links opens the form, Enter adds the link and brings the
/// directory back on it; `e` edits the selected one in place; a form refused keeps it,
/// saying why; `n` and `e` do nothing in the ports.
/// How: `links_path` in a temp folder, a link added then renamed through the form, a
/// name left empty; then the ports.
#[test]
fn new_and_edit_links() {
    use crate::inputs::DialogAction;
    use crossterm::event::{KeyCode, KeyEvent};
    let dir = std::env::temp_dir().join(format!("maxicom_form_{}", std::process::id()));
    let path = dir.join("links.toml");
    let mut app = app_on("/dev/ttyS9");
    app.settings_mut().links_path = Some(path.clone());
    let press = |c| Action::Overlay(DialogAction::HandleKey(KeyEvent::from(KeyCode::Char(c))));
    let form = |app: &mut App| match &mut app.overlay {
        Some(Overlay::LinkForm(form)) => Some(form.clone()),
        _ => None,
    };
    let set = |app: &mut App, name: &str, address: &str| {
        if let Some(Overlay::LinkForm(form)) = &mut app.overlay {
            form.name = name.to_string().into();
            form.address = address.to_string().into();
        }
    };

    app.open_directory(Vec::new());
    app.dispatch(Action::Overlay(DialogAction::CycleNext));
    assert!(matches!(app.overlay(), Some(Overlay::Directory(_))));
    app.dispatch(press('n'));
    let new = form(&mut app).expect("no form");
    assert!(new.editing.is_none());
    assert_eq!(new.hosts.value(), app.host.clone().unwrap_or_default());
    set(&mut app, "board", "/dev/ttyS9");
    app.dispatch(Action::Overlay(DialogAction::Confirm));
    assert!(toasts(&app).contains(&"Saved link 'board'".to_string()));
    let Some(Overlay::Directory(directory)) = app.overlay() else {
        panic!("no directory");
    };
    assert_eq!(directory.tab, DirectoryTab::Saved);

    app.dispatch(press('e'));
    assert!(form(&mut app).expect("no form").editing.is_some());
    set(&mut app, "", "/dev/ttyS9");
    app.dispatch(Action::Overlay(DialogAction::Confirm));
    assert!(form(&mut app).is_some(), "the form closed");
    set(&mut app, "esp32", "/dev/ttyS8");
    app.dispatch(Action::Overlay(DialogAction::Confirm));
    let saved = LinksConfig::load(&path).unwrap();
    let links: Vec<_> = (saved.links.iter())
        .map(|link| (link.name.as_str(), link.address.as_str()))
        .collect();
    assert_eq!(links, [("esp32", "/dev/ttyS8")]);

    app.open_directory(Vec::new());
    app.dispatch(Action::Overlay(DialogAction::CycleNext));
    app.dispatch(press('n'));
    app.dispatch(press('e'));
    assert!(matches!(app.overlay(), Some(Overlay::Directory(_))));
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-185: `x` in the saved links asks before deleting the selected one: `n` goes back
/// to the directory, nothing deleted; `y` or Enter deletes it from `links.toml`, says so
/// and brings the directory back on the saved links, even with none left. A link gone
/// from the file meanwhile says so.
/// How: two links in a temp `links.toml`; `x` then `n`, `x` then `y` on the second, the
/// first deleted by hand then `x` and Enter on it.
#[test]
fn delete_a_link() {
    use crate::inputs::DialogAction;
    use crossterm::event::{KeyCode, KeyEvent};
    let dir = std::env::temp_dir().join(format!("maxicom_delete_{}", std::process::id()));
    let path = dir.join("links.toml");
    std::fs::create_dir_all(&dir).unwrap();
    let two = "[[link]]\nname = 'a'\naddress = 'COM1'\n[[link]]\nname = 'b'\naddress = 'COM2'\n";
    std::fs::write(&path, two).unwrap();
    let mut app = app_on("/dev/ttyS9");
    app.settings_mut().links_path = Some(path.clone());
    app.set_links(Ok(LinksConfig::load(&path).unwrap()));
    let press = |c| Action::Overlay(DialogAction::HandleKey(KeyEvent::from(KeyCode::Char(c))));
    let names = || -> Vec<String> {
        (LinksConfig::load(&path).unwrap().links.into_iter())
            .map(|link| link.name)
            .collect()
    };

    app.open_directory(Vec::new());
    app.dispatch(Action::Overlay(DialogAction::Down));
    app.dispatch(press('x'));
    assert!(matches!(app.overlay(), Some(Overlay::DeleteLink(link)) if link.name == "b"));
    app.dispatch(press('n'));
    assert!(matches!(app.overlay(), Some(Overlay::Directory(_))));
    assert_eq!(names(), ["a", "b"]);

    app.open_directory(Vec::new());
    app.dispatch(Action::Overlay(DialogAction::Down));
    app.dispatch(press('x'));
    app.dispatch(press('y'));
    assert_eq!(names(), ["a"]);
    assert!(toasts(&app).contains(&"Deleted link 'b'".to_string()));
    let Some(Overlay::Directory(directory)) = app.overlay() else {
        panic!("no directory");
    };
    assert_eq!(directory.tab, DirectoryTab::Saved);

    std::fs::write(&path, "").unwrap();
    app.open_directory(Vec::new());
    app.dispatch(press('x'));
    app.dispatch(Action::Overlay(DialogAction::Confirm));
    let gone = "a: changed in the file meanwhile: not deleted".to_string();
    assert!(toasts(&app).contains(&gone), "{:?}", toasts(&app));
    let Some(Overlay::Directory(directory)) = app.overlay() else {
        panic!("no directory");
    };
    assert_eq!(directory.tab, DirectoryTab::Saved);
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-185: the notices name a saved link by its name, its address after it; the detach
/// notice by the kind of link, as before.
/// How: a TCP listener saved as `bridge`, the link started, connected, then detached.
#[test]
fn notices_name_a_saved_link() {
    let (listener, address) = listener();
    let mut app = app_on(&address);
    let text = format!("[[link]]\nname = 'bridge'\naddress = '{address}'\n");
    app.set_links(Ok(toml::from_str(&text).unwrap()));
    app.start_link(None);
    let label = format!("bridge ({address})");
    assert!(toasts(&app).contains(&format!("Connecting to {label}")));
    let _peer = listener.accept().unwrap();
    poll_until(&mut app, "connected", |app| app.link.is_open());
    assert!(toasts(&app).contains(&format!("Connected to {label}")));
    assert_eq!(app.link_name(), Some("bridge"));
    app.dispatch(Action::App(AppAction::ToggleAttach));
    assert!(toasts(&app).contains(&"TCP connection detached".to_string()));
}

/// MAX-181: from the start's directory, a port that fails to open leaves the link waiting
/// again, not retrying it, and the directory open with the reason on its row.
/// How: a TCP address nothing listens on, picked with Enter, then polled past
/// `REOPEN_EVERY`.
#[test]
fn start_in_directory_failed_pick_waits_again() {
    let (listener, address) = listener();
    drop(listener);
    let mut app = app_on(&address);
    app.start_in_directory(vec![address.clone()], None);
    app.dispatch(Action::Overlay(crate::inputs::DialogAction::Confirm));
    poll_until(&mut app, "failed", |app| {
        toasts(app).iter().any(|t| t.starts_with("open: "))
    });
    assert!(app.is_device_unpicked());
    let mut buffer = [0; 256];
    let start = Instant::now();
    while start.elapsed() < REOPEN_EVERY * 2 {
        app.poll_link(&mut buffer).unwrap();
        thread::sleep(Duration::from_millis(20));
    }
    let opens = toasts(&app)
        .iter()
        .filter(|t| t.starts_with("open: "))
        .count();
    assert_eq!(opens, 1, "{:?}", toasts(&app));
    let Some(Overlay::Directory(directory)) = app.overlay() else {
        panic!("the directory closed");
    };
    assert!(directory.opening.is_none());
    assert!(matches!(
        directory.tests[&address],
        Some(ProbeState::Error(_))
    ));
}

/// MAX-181: started on the directory, closing it before a pick shows the no device
/// popup, which a click outside doesn't close: Enter opens the directory again, Esc quits.
/// How: Esc in the directory, then Enter and Esc in the popup.
#[test]
fn no_device_popup_until_picked() {
    use crate::inputs::DialogAction;
    let mut app = app_on("/dev/ttyUSB0");
    app.start_in_directory(vec!["/dev/ttyUSB0".into()], None);
    app.dispatch(Action::Overlay(DialogAction::Close));
    assert!(matches!(app.overlay(), Some(Overlay::NoDevice)));
    assert!(!Overlay::NoDevice.closes_on_click_outside());
    app.dispatch(Action::Overlay(DialogAction::Confirm));
    assert!(matches!(app.overlay(), Some(Overlay::Directory(_))));
    app.dispatch(Action::Overlay(DialogAction::Close));
    let quit = app.dispatch(Action::Overlay(DialogAction::Close));
    assert!(matches!(quit, UpdateEffect::Quit));
}

/// MAX-181: the directory's test runs a thread per port, the device in use left out: each
/// mark is "testing" until its result comes; past the timeout the ones left say no
/// answer, and a closed directory drops the test. MAX-186: a port not there is missing.
/// How: two ports that don't exist and the device in use; then a test whose results never
/// come, its time up; then the directory closed.
#[test]
fn directory_port_test() {
    let (listener, address) = listener();
    let (mut app, _peer) = connected(&listener, &address);
    let ports = vec![
        address.clone(),
        "/nonexistent/a".into(),
        "/nonexistent/b".into(),
    ];
    let current = app.link_config().address.clone();
    app.open_overlay(Overlay::Directory(Directory::new(ports, &current)));
    app.test_links();
    let tests = |app: &App| match app.overlay() {
        Some(Overlay::Directory(directory)) => directory.tests.clone(),
        _ => panic!("no directory"),
    };
    assert!(!tests(&app).contains_key(&address));
    let start = Instant::now();
    while app.link_test.is_some() {
        app.poll_link_test();
        assert!(start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(5));
    }
    let done = tests(&app);
    assert_eq!(done.len(), 2);
    assert!(
        done.values()
            .all(|state| state == &Some(ProbeState::Missing))
    );

    let (_sender, results) = mpsc::channel();
    let past = Instant::now() - Duration::from_millis(1);
    app.link_test = Some(LinkTest {
        results,
        until: past,
    });
    if let Some(Overlay::Directory(directory)) = &mut app.overlay {
        directory.tests.insert("/nonexistent/a".into(), None);
    }
    assert!(app.poll_link_test());
    assert_eq!(tests(&app)["/nonexistent/a"], Some(ProbeState::NoAnswer));
    assert!(app.link_test.is_none());

    let (_sender, results) = mpsc::channel();
    let until = Instant::now() + LINK_TEST_TIMEOUT;
    app.link_test = Some(LinkTest { results, until });
    app.close_overlay();
    assert!(!app.poll_link_test());
    assert!(app.link_test.is_none());
}

/// MAX-186: the test covers the saved links too, by address: a TCP link that takes the
/// connect is reachable, one nothing listens on refused, a saved serial link that is also
/// a listed port tested once, the link in use left out.
/// How: links to a listener, to a closed port, to a missing port also listed, and to the
/// link in use; the test run until done.
#[test]
fn directory_tests_saved_links() {
    use std::collections::HashMap;
    let (link, address) = listener();
    let (mut app, _peer) = connected(&link, &address);
    let (up, up_address) = listener();
    let (closed, closed_address) = listener();
    drop(closed);
    let links = format!(
        "[[link]]\nname = 'up'\naddress = '{up_address}'\n\
             [[link]]\nname = 'down'\naddress = '{closed_address}'\n\
             [[link]]\nname = 'gone'\naddress = '/nonexistent/a'\n\
             [[link]]\nname = 'here'\naddress = '{address}'\n"
    );
    app.set_links(Ok(toml::from_str(&links).unwrap()));
    let directory = app.directory(vec!["/nonexistent/a".to_string()]);
    app.open_overlay(Overlay::Directory(directory));
    app.test_links();
    let start = Instant::now();
    while app.link_test.is_some() {
        app.poll_link_test();
        assert!(start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(5));
    }
    let Some(Overlay::Directory(directory)) = app.overlay() else {
        panic!("no directory");
    };
    let expected: HashMap<String, Option<ProbeState>> = HashMap::from([
        (up_address, Some(ProbeState::Reachable)),
        (closed_address, Some(ProbeState::Refused)),
        ("/nonexistent/a".to_string(), Some(ProbeState::Missing)),
    ]);
    assert_eq!(directory.tests, expected);
    drop(up);
}

/// MAX-186: a TCP link another maxicom has open is opened anyway (some servers take
/// several clients), with a warning naming it.
/// How: maxicom's lock on the address taken with another PID in its info, then the app
/// connected to it. Skipped without a lock folder.
#[test]
fn shared_tcp_link_warns() {
    use crate::link::lock::{LinkLock, lock_dir};
    let Some(dir) = lock_dir() else {
        return;
    };
    let (listener, address) = listener();
    let parsed = address.parse::<Address>().unwrap();
    let lock = LinkLock::take(&parsed).unwrap();
    let info = dir.join(format!("{}.info", parsed.file_tag()));
    std::fs::write(&info, "pid=4242\nuser=other\n").unwrap();
    let (app, _peer) = connected(&listener, &address);
    let warning = format!("{address} is open in maxicom 4242 (other) too");
    assert!(toasts(&app).contains(&warning), "{:?}", toasts(&app));
    drop(lock);
}

/// Sessions on the app's own settings, a line of each: `one`, `two` and `three`.
fn group_of_three(name: &str) -> (std::path::PathBuf, Vec<crate::capture::Session>) {
    let dir = crate::capture::tests::temp_dir(name);
    let text = CaptureFormat::default();
    let write = crate::capture::tests::write_test_session;
    let sessions = vec![
        write(&dir, "/dev/a", &text, b"one\r\n", b""),
        write(&dir, "/dev/a", &text, b"two\r\n", b""),
        write(&dir, "/dev/a", &text, b"three\r\n", b""),
    ];
    (dir, sessions)
}

/// MAX-202: a group replay takes the link's place at once, already ended so it reads nothing and
/// says no `Replay finished`: terminal mode first, line mode holds every session in order with
/// a separator after each, on a big history of its own. Going back gives the live link its
/// histories back.
/// How: a live TCP link with a line of its own, then three sessions replayed as a group.
#[test]
fn group_replay_loads_at_once_and_gives_the_link_back() {
    let (dir, sessions) = group_of_three("group_replay");
    let (listener, address) = listener();
    let (mut app, _peer) = connected(&listener, &address);
    app.settings.mode = crate::configs::Mode::Line;
    app.feed_rx(b"live line\r\n");

    app.apply_link_effect(UpdateEffect::ReplayGroup(sessions.clone()));
    assert_eq!(app.link_kind(), LinkKind::Replay);
    assert_eq!(app.settings.mode, crate::configs::Mode::Terminal);
    assert!(app.is_replay_ended());
    assert_eq!(app.replay_group(), Some(3));
    app.poll_link(&mut [0; 256]).unwrap();
    let toasts = toasts(&app);
    assert!(!toasts.iter().any(|t| t == "Replay finished"), "{toasts:?}");
    assert!(
        toasts.iter().any(|t| t == "Loaded 3 sessions"),
        "{toasts:?}"
    );

    let texts: Vec<_> = (app.line_mode.history().iter())
        .map(|entry| entry.text.clone())
        .collect();
    let lines: Vec<_> = texts.iter().map(|text| text.trim_end()).collect();
    assert_eq!(
        [lines[0], lines[2], lines[4]],
        ["one", "two", "three"],
        "{lines:?}"
    );
    for separator in [lines[1], lines[3], lines[5]] {
        assert!(separator.starts_with("── end of session: a, "), "{lines:?}");
    }
    let terminal = app.terminal_mode.all_text_lines();
    for line in ["one", "two", "three"] {
        assert!(terminal.iter().any(|row| row == line), "{terminal:?}");
    }

    app.dispatch(Action::App(AppAction::StopReplay));
    assert_eq!(
        app.link_config().address,
        address.parse::<Address>().unwrap()
    );
    assert_eq!(app.replay_group(), None);
    assert_eq!(app.line_mode.history().len(), 1);
    assert_eq!(app.line_mode.history()[0].text.trim_end(), "live line");
    let _peer = listener.accept().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-202: what doesn't fit is said. Past line mode's history the oldest sessions go and the
/// toast warns; past terminal mode's, the toast says line mode has them all.
/// How: three sessions of two lines each (a line and a separator) in 3 lines, then a terminal
/// history of 1 row.
#[test]
fn group_replay_says_what_does_not_fit() {
    let (dir, sessions) = group_of_three("group_replay_fit");
    let mut app = crate::app::tests::app();
    app.load_group(&sessions, 3);
    let lines = app.line_mode.history();
    assert!(lines[0].text.starts_with("── end of session"));
    assert_eq!(lines[1].text.trim_end(), "three");
    assert_eq!(app.line_mode.dropped_lines(), 3);
    let shown = crate::app::tests::toasts(&app);
    assert!(
        shown
            .iter()
            .any(|t| t == "Loaded the last 2 of 3 sessions: line mode holds 3 lines"),
        "{shown:?}"
    );

    let mut app = crate::app::tests::app();
    app.settings.terminal_mode.history_size = 1;
    app.load_group(&sessions, 100);
    let shown = crate::app::tests::toasts(&app);
    assert!(
        shown.iter().any(|t| t
            == "Loaded 3 sessions. Terminal mode keeps the last 1 rows, line mode has them all"),
        "{shown:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-202: a big group costs what the histories hold: older sessions aren't read at all, and a
/// session too big has its start left out, which the toast says.
/// How: two sessions of 1000 lines in a history of 20 lines; then one alone.
#[test]
fn group_replay_reads_only_what_fits() {
    let dir = crate::capture::tests::temp_dir("group_replay_budget");
    let text = CaptureFormat::default();
    let lines = |name: &str| -> Vec<u8> {
        (0..1000)
            .flat_map(|i| format!("{name} line {i}\r\n").into_bytes())
            .collect()
    };
    let write = crate::capture::tests::write_test_session;
    let older = write(&dir, "/dev/a", &text, &lines("older"), b"");
    let newer = write(&dir, "/dev/a", &text, &lines("newer"), b"");

    let mut app = crate::app::tests::app();
    app.load_group(&[older.clone(), newer.clone()], 20);
    assert!(
        (app.line_mode.history().iter()).all(|entry| !entry.text.starts_with("older")),
        "the older session was read"
    );
    let shown = crate::app::tests::toasts(&app);
    assert!(
        shown
            .iter()
            .any(|t| t.starts_with("Loaded the last 1 of 2 sessions: line mode holds 20 lines")),
        "{shown:?}"
    );

    let mut app = crate::app::tests::app();
    app.load_group(&[newer], 20);
    let shown = crate::app::tests::toasts(&app);
    assert!(
        shown
            .iter()
            .any(|t| t.starts_with("Loaded 1 session without the start of the oldest")),
        "{shown:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-202: terminal mode gets only the end of a group, as it keeps little: with a small history
/// the older sessions aren't read into it, while line mode has them.
/// How: a large first session then a short one, a terminal history of 10 rows.
#[test]
fn group_replay_feeds_the_terminal_the_end_only() {
    let dir = crate::capture::tests::temp_dir("group_replay_tail");
    let text = CaptureFormat::default();
    let big: Vec<u8> = (0..1000)
        .flat_map(|i| format!("old line {i}\r\n").into_bytes())
        .collect();
    let write = crate::capture::tests::write_test_session;
    let sessions = vec![
        write(&dir, "/dev/a", &text, b"first of all\r\n", b""),
        write(&dir, "/dev/a", &text, &big, b""),
        write(&dir, "/dev/a", &text, b"newest\r\n", b""),
    ];
    let mut app = crate::app::tests::app();
    app.settings.terminal_mode.history_size = 10;
    app.terminal_mode = crate::app::TerminalMode::new(10, 24, 80);
    app.load_group(&sessions, 10_000);
    let terminal = app.terminal_mode.all_text_lines();
    assert!(terminal.iter().any(|row| row == "newest"), "{terminal:?}");
    assert!(
        !terminal.iter().any(|row| row == "first of all"),
        "{terminal:?}"
    );
    assert_eq!(app.line_mode.history()[0].text.trim_end(), "first of all");
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-202: a group replay has no controls, it read everything at once.
/// How: the commands after a group replay, each refused with the same reason.
#[test]
fn group_replay_has_no_controls() {
    use crate::commands::build::{replay_pause, replay_restart, replay_resume};
    let (dir, sessions) = group_of_three("group_replay_controls");
    let mut app = crate::app::tests::app();
    app.apply_link_effect(UpdateEffect::ReplayGroup(sessions));
    for command in [replay_pause, replay_resume, replay_restart] {
        let refused = command(&app, &[]).err();
        assert_eq!(
            refused.as_deref(),
            Some("a group replay is loaded at once: it has no controls")
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-162: an XMODEM upload owns the link once the receiver starts. The device's text before
/// it and after the end is shown; between, the receiver's answers go to the protocol only,
/// typed keys and `send` are refused (said once). The end gives the link back.
/// How: a TCP peer plays the receiver: text then C, an ACK per answer, text after the last.
#[test]
fn upload_owns_the_link() {
    use crate::{
        commands::{ArgValue, build},
        inputs::LineInputAction,
        transfer::{FileTransfer, modem::Protocol},
    };
    let (listener, address) = listener();
    let (mut app, mut peer) = connected(&listener, &address);
    let transfer = FileTransfer::modem(
        Box::new(std::io::Cursor::new(b"abc".to_vec())),
        Protocol::Xmodem,
        "fw.bin".to_string(),
        3,
        11_520.0,
        Instant::now(),
    );
    app.dispatch(Action::App(AppAction::StartTransfer(Box::new(transfer))));
    let mut byte = [0; 1];

    // waiting for the receiver: the user can still type `loady`
    app.dispatch(Action::TransmitRaw(b"k".to_vec()));
    peer.read_exact(&mut byte).unwrap();
    assert_eq!(&byte, b"k");

    peer.write_all(b"ready\r\nC").unwrap();
    poll_until(&mut app, "started", App::link_busy);
    let mut packet = [0; 133];
    peer.read_exact(&mut packet).unwrap();
    assert_eq!(packet[..6], [0x01, 1, 0xFE, b'a', b'b', b'c']);

    app.dispatch(Action::TransmitRaw(b"k".to_vec()));
    app.dispatch(Action::TransmitRaw(b"k".to_vec()));
    let busy = "A file transfer is running: cancel stops it".to_string();
    assert_eq!(toasts(&app).iter().filter(|t| **t == busy).count(), 1);
    let send = build::send(&app, &[ArgValue::Bytes(b"x".to_vec())]);
    assert_eq!(
        send.unwrap_err(),
        "a file transfer is running (cancel stops it)"
    );

    // a line typed with local echo isn't echoed: it stays in the input
    app.settings.mode = crate::configs::Mode::Line;
    app.settings.local_echo = true;
    let echoed = |app: &App| {
        app.line_mode
            .history()
            .iter()
            .any(|e| e.text.contains("ls"))
    };
    app.dispatch(Action::LineInput(LineInputAction::Paste("ls".to_string())));
    app.dispatch(Action::LineInput(LineInputAction::Submit));
    app.dispatch(Action::LineInput(LineInputAction::Paste(
        "ls\n".to_string(),
    )));
    app.dispatch(Action::TransmitRaw(b"ls".to_vec()));
    assert!(!echoed(&app));
    assert_eq!(app.line_mode.input().value(), "ls");
    assert_eq!(toasts(&app).iter().filter(|t| **t == busy).count(), 1);

    peer.write_all(&[0x06]).unwrap();
    poll_until(&mut app, "EOT", |app| app.stats.tx() == 1 + 133 + 1);
    peer.read_exact(&mut byte).unwrap();
    assert_eq!(byte, [0x04]);
    peer.write_all(b"\x06done\r\n").unwrap();
    poll_until(&mut app, "ended", |app| app.transfer().is_none());
    poll_until(&mut app, "text after", |app| {
        app.terminal_mode.vt100_screen().contents().contains("done")
    });

    let screen = app.terminal_mode.vt100_screen().contents();
    assert!(
        screen.contains("ready") && !screen.contains('C'),
        "{screen}"
    );
    assert!(toasts(&app).contains(&"Sent fw.bin (3 bytes, XMODEM)".to_string()));

    app.dispatch(Action::TransmitRaw(b"k".to_vec()));
    peer.read_exact(&mut byte).unwrap();
    assert_eq!(&byte, b"k");
}

/// MAX-162: `cancel` during an upload sends the CANs that stop the receiver, and gives the
/// link back; for a few seconds, the receiver's ACK or NAK gets them again.
#[test]
fn cancel_stops_the_receiver() {
    use crate::transfer::{FileTransfer, modem::Protocol};
    let (listener, address) = listener();
    let (mut app, mut peer) = connected(&listener, &address);
    let transfer = FileTransfer::modem(
        Box::new(std::io::Cursor::new(b"abc".to_vec())),
        Protocol::Xmodem,
        "fw.bin".to_string(),
        3,
        11_520.0,
        Instant::now(),
    );
    app.dispatch(Action::App(AppAction::StartTransfer(Box::new(transfer))));
    peer.write_all(b"C").unwrap();
    poll_until(&mut app, "started", App::link_busy);
    let mut packet = [0; 133];
    peer.read_exact(&mut packet).unwrap();

    app.dispatch(crate::configs::KeyBindingAction::CancelCommands.to_action(&app));
    let mut cans = [0; 5];
    peer.read_exact(&mut cans).unwrap();
    assert_eq!(cans, [0x18; 5]);
    assert!(!app.link_busy());
    assert!(toasts(&app).contains(&"Cancelled: fw.bin at 0%".to_string()));

    // a receiver that missed them (lrzsz's rb drops what came with a block's end) ACKs the
    // block: the CANs again. A `C` in the device's text doesn't
    peer.write_all(b"C\x06").unwrap();
    let tx = app.stats.tx();
    poll_until(&mut app, "CANs again", |app| app.stats.tx() == tx + 5);
    peer.read_exact(&mut cans).unwrap();
    assert_eq!(cans, [0x18; 5]);
    peer.write_all(b"Ctrl-C\r\n").unwrap();
    poll_until(&mut app, "text shown", |app| {
        app.terminal_mode
            .vt100_screen()
            .contents()
            .contains("Ctrl-C")
    });
    assert_eq!(app.stats.tx(), tx + 5);
}

/// MAX-162: a YMODEM download over a link. The shell's echo before the first packet shows;
/// the packets don't; the file lands in the folder under its own name, said by a notice.
/// How: a thread plays the device, `ModemSender` on the TCP peer's side.
#[test]
fn download_writes_the_file() {
    use crate::transfer::{FileTransfer, modem::Protocol, modem_tx::ModemSender};
    let (listener, address) = listener();
    let (mut app, mut peer) = connected(&listener, &address);
    let folder = std::env::temp_dir().join(format!(
        "maxicom_test_{}_device_download",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&folder);
    let content: Vec<u8> = (0..3000u32).map(|i| (i % 251) as u8).collect();
    let device = {
        let content = content.clone();
        thread::spawn(move || {
            peer.write_all(b"sb fw.bin\r\n").unwrap();
            peer.set_read_timeout(Some(Duration::from_millis(5)))
                .unwrap();
            let mut sender = ModemSender::new(
                Box::new(std::io::Cursor::new(content.clone())),
                Protocol::Ymodem,
                "fw.bin".to_string(),
                content.len() as u64,
                11_520.0,
                Instant::now(),
            );
            let mut buffer = [0; 256];
            while sender.outcome().is_none() {
                let n = peer.read(&mut buffer).unwrap_or(0);
                let out = sender.step(&buffer[..n], Instant::now()).unwrap().out;
                peer.write_all(&out).unwrap();
            }
            peer.write_all(b"$ ").unwrap();
            peer
        })
    };

    let transfer = FileTransfer::download(Protocol::Ymodem, folder.clone(), None, Instant::now());
    app.dispatch(Action::App(AppAction::StartTransfer(Box::new(transfer))));
    // the progress row says it waits: no toast (MAX-228)
    assert!(
        !toasts(&app).iter().any(|t| t.starts_with("Receiving")),
        "{:?}",
        toasts(&app)
    );
    poll_until(&mut app, "ended", |app| app.transfer().is_none());
    let _peer = device.join().unwrap();
    poll_until(&mut app, "prompt after", |app| {
        app.terminal_mode.vt100_screen().contents().contains("$")
    });

    assert_eq!(std::fs::read(folder.join("fw.bin")).unwrap(), content);
    // the file by name, the folder once (MAX-228)
    let received = format!("Received fw.bin (3000 bytes) into {}", folder.display());
    let transfer_toasts = app.notifications.active_toasts();
    let transfer_toasts = transfer_toasts.iter().filter(|t| t.key == Some("transfer"));
    assert_eq!(transfer_toasts.count(), 1);
    let toasts = toasts(&app);
    assert!(toasts.contains(&received), "{toasts:?}");
    assert!(
        !toasts.iter().any(|t| t.starts_with("Receiving")),
        "{toasts:?}"
    );
    let screen = app.terminal_mode.vt100_screen().contents();
    assert!(screen.starts_with("sb fw.bin\n$"), "{screen:?}");
    let _ = std::fs::remove_dir_all(&folder);
}
