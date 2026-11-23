//! App-level wiring of the viewport (#35) and of a config reload (MAX-112).

use super::*;
use crate::{
    app::capture::tests::asking_app,
    app::search::SearchDirection,
    capture::{CaptureFormat, RecordingMeta},
    configs::{
        KeyBindingsConfig,
        settings::{DisabledMode, UnhandledGroup},
    },
    inputs::{DialogAction, SearchModeAction},
};

/// An app whose link is open
pub fn app() -> App {
    let mut app = App::new(
        SettingsConfig::default(),
        KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap(),
        LinkConfig::default(),
    );
    set_link_open(&mut app, true);
    app
}

/// The history's texts without their EOL, the separator as its start
pub fn texts(app: &App) -> Vec<String> {
    let history = app.line_mode.history().iter();
    history
        .map(|e| match e.text.trim_end_matches(['\r', '\n']) {
            text if text.starts_with("── restored") => "── restored".to_string(),
            text => text.to_string(),
        })
        .collect()
}

/// A full recording to the folder `path` started and stopped at once, for its meta file
pub fn record_full_meta(app: &mut App, path: &Path, include_session: bool) {
    app.start_capture(path, crate::capture::Stream::Full, include_session);
    app.recorder.stop();
}

/// The line settings the port took, as the link sets them
pub fn set_line(app: &mut App, line: LineSettings) {
    let config = LinkConfig {
        line,
        ..app.link_config().clone()
    };
    app.link.set_config(config);
}

/// Opens or closes the app's link: a device that reads and writes nothing stands for an
/// open link
fn set_link_open(app: &mut App, open: bool) {
    app.link.start(open.then(|| {
        let path =
            std::env::temp_dir().join(format!("maxicom_test_{}_open_link", std::process::id()));
        std::fs::write(&path, b"").unwrap();
        let replay = crate::link::replay::Replay::open(&path, Duration::ZERO).unwrap();
        crate::link::ByteSource::Replay(replay)
    }));
}

/// MAX-239: a named command of `[commands]` runs from the command line as its line, alone or
/// in a chain; given arguments it runs nothing and says why.
/// How: `twice` toggling local echo twice then detaching.
#[test]
fn command_line_runs_a_named_command() {
    use crate::configs::keybindings::NamedCommandConfig;
    let mut config = KeyBindingsConfig::default();
    let twice = NamedCommandConfig {
        run: "toggle-local-echo; toggle-local-echo".to_string(),
        help: String::new(),
    };
    config.commands.insert("twice".into(), twice);
    let mut app = app();
    app.key_resolver = KeyResolver::try_new(&config).unwrap();
    let run_line = |app: &mut App, line: &str| {
        app.update(KeyBindingAction::CommandLine.to_action(app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
        let mut effects = Vec::new();
        while let Some((_, action)) = app.next_command_action(Instant::now()) {
            effects.push(app.update(action));
        }
        effects
    };
    let echo = app.settings.local_echo;
    let effects = run_line(&mut app, "twice; toggle-attach");
    assert!(matches!(
        effects[..],
        [
            UpdateEffect::None,
            UpdateEffect::None,
            UpdateEffect::ToggleAttach
        ]
    ));
    assert_eq!(app.settings.local_echo, echo);

    assert!(run_line(&mut app, "twice now").is_empty());
    let toast = app.notifications.active_toasts().last().unwrap();
    assert_eq!(toast.message, "twice takes no argument");
}

/// MAX-185: a `links.toml` that doesn't load is said, the app running without its links;
/// one that loads says nothing.
/// How: an error then links set.
#[test]
fn bad_links_file_warns() {
    let mut app = app();
    app.set_links(Err(
        "invalid link in 'links.toml': link #1 'a': the name is empty".into(),
    ));
    let toast = app.notifications.active_toasts().last().unwrap();
    assert!(toast.message.starts_with("invalid link in 'links.toml'"));
    assert!(app.links.is_err());
    let count = app.notifications.active_toasts().len();
    app.set_links(Ok(LinksConfig::default()));
    assert_eq!(app.notifications.active_toasts().len(), count);
    assert!(app.links.is_ok());
}

/// MAX-185: `open` takes a saved link's name first, a serial one opening with its line
/// settings, a TCP one as an address; a word that isn't one is a device. `link-save` saves
/// the link in use, under the name given, refused once its address is saved; `link-delete`
/// deletes a saved link by name, refused for a name that isn't one.
/// How: two links in a temp `links.toml`, each command through the command line, the
/// effects and the notices read.
#[test]
fn saved_links_on_the_command_line() {
    let dir = std::env::temp_dir().join(format!("maxicom_link_cmds_{}", std::process::id()));
    let path = dir.join("links.toml");
    std::fs::create_dir_all(&dir).unwrap();
    let text = "[[link]]\nname = 'esp32'\naddress = '/dev/ttyUSB9'\nbaud = 9600\n\
                    [[link]]\nname = 'router console'\naddress = 'tcp://10.0.0.1:4000'\n";
    std::fs::write(&path, text).unwrap();
    let mut app = app();
    app.settings.links_path = Some(path.clone());
    app.set_links(Ok(LinksConfig::load(&path).unwrap()));
    let run_line = |app: &mut App, line: &str| {
        app.update(KeyBindingAction::CommandLine.to_action(app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
        let mut effects = Vec::new();
        while let Some((_, action)) = app.next_command_action(Instant::now()) {
            effects.push(app.update(action));
        }
        effects
    };
    let last_toast = |app: &App| {
        let toasts = app.notifications.active_toasts();
        toasts.last().map(|toast| toast.message.clone()).unwrap()
    };
    let names = |path: &std::path::Path| -> Vec<String> {
        (LinksConfig::load(path).unwrap().links.into_iter())
            .map(|link| link.name)
            .collect()
    };

    match &run_line(&mut app, "open esp32")[..] {
        [UpdateEffect::OpenLink(config)] => {
            assert_eq!(config.address.to_string(), "/dev/ttyUSB9");
            assert_eq!(config.line.baud, 9600);
        }
        _ => panic!("esp32 not opened with its line"),
    }
    app.drop_pending_commands();
    let tcp = "tcp://10.0.0.1:4000".parse::<Address>().unwrap();
    assert!(matches!(
        &run_line(&mut app, "open router\\ console")[..],
        [UpdateEffect::Open(address)] if *address == tcp
    ));
    app.drop_pending_commands();
    assert!(matches!(
        &run_line(&mut app, "open COM3")[..],
        [UpdateEffect::Open(address)] if address.to_string() == "COM3"
    ));
    app.drop_pending_commands();

    run_line(&mut app, "link-save board");
    assert_eq!(last_toast(&app), "Saved link 'board'");
    assert_eq!(names(&path), ["esp32", "router console", "board"]);
    run_line(&mut app, "link-save");
    assert_eq!(
        last_toast(&app),
        "link-save: /dev/ttyUSB0 is saved already, as 'board'"
    );
    run_line(&mut app, "link-delete esp32");
    assert_eq!(last_toast(&app), "Deleted link 'esp32'");
    assert_eq!(names(&path), ["router console", "board"]);
    run_line(&mut app, "link-delete nope");
    assert_eq!(last_toast(&app), "link-delete: no saved link 'nope'");
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-213: `session-name` names the running session, quoted with spaces, and no text takes the
/// name away. The name stays when the recording writes its meta again, and a replay's paused
/// live session can be named. No session refuses it.
/// How: a session started in a temp folder, each command through the command line, its meta read;
/// a resize; a replay; then an app without a session.
#[test]
fn name_the_running_session() {
    use crate::capture::{RecordingMeta, TerminalSize, list_sessions};
    let dir = std::env::temp_dir().join(format!("maxicom_name_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut app = app();
    app.start_session(Some(dir.clone()));
    let run_line = |app: &mut App, line: &str| {
        app.update(KeyBindingAction::CommandLine.to_action(app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
        while let Some((_, action)) = app.next_command_action(Instant::now()) {
            app.update(action);
        }
    };
    let last_toast = |app: &App| {
        let toasts = app.notifications.active_toasts();
        toasts.last().map(|toast| toast.message.clone()).unwrap()
    };
    let name = |dir: &std::path::Path| {
        let sessions = list_sessions(dir).unwrap();
        let raw = sessions[0].raw_path().unwrap();
        RecordingMeta::read(&RecordingMeta::path_of(raw))
            .unwrap()
            .name
    };
    run_line(&mut app, "session-name 'boot test'");
    assert_eq!(name(&dir).as_deref(), Some("boot test"));
    assert_eq!(last_toast(&app), "Session named 'boot test'");

    app.recorder
        .set_terminal_size(TerminalSize { rows: 9, cols: 40 });
    assert_eq!(name(&dir).as_deref(), Some("boot test"));

    run_line(&mut app, "session-name");
    assert_eq!(name(&dir), None);
    assert_eq!(last_toast(&app), "Session name removed");

    // the live link's session, paused by a replay over it
    app.start_replay(PathBuf::from("/tmp/a.raw"), None);
    run_line(&mut app, "session-name paused");
    assert_eq!(name(&dir).as_deref(), Some("paused"));

    let mut none = super::tests::app();
    run_line(&mut none, "session-name x");
    assert_eq!(last_toast(&none), "session-name: no session running");
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-12: `send` and `send-hex` transmit their bytes, in the chain's order. A command that
/// refuses (nothing to send, a replay) stops the chain with the error.
/// How: chains run on a live port, then on a replay (`file://`).
#[test]
fn command_line_send() {
    let mut app = app();
    let run_line = |app: &mut App, line: &str| {
        app.update(KeyBindingAction::CommandLine.to_action(app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
        let mut effects = Vec::new();
        while let Some((_, action)) = app.next_command_action(Instant::now()) {
            effects.push(app.update(action));
        }
        effects
    };
    let last_error = |app: &App| {
        app.notifications
            .active_toasts()
            .last()
            .unwrap()
            .message
            .clone()
    };

    let effects = run_line(&mut app, "send-hex 0d0a; send AT\\r");
    assert!(matches!(
        &effects[..],
        [UpdateEffect::Transmit(crlf), UpdateEffect::Transmit(at)]
            if crlf == b"\r\n" && at == b"AT\r"
    ));

    let effects = run_line(&mut app, "send ''; help; settings");
    assert!(effects.is_empty());
    assert_eq!(last_error(&app), "send: nothing to send (2 more not run)");
    assert!(app.overlay().is_none());

    app.link
        .set_address("file:///tmp/capture.raw".parse::<Address>().unwrap());
    let effects = run_line(&mut app, "help; send x");
    assert_eq!(effects.len(), 1);
    assert!(matches!(app.overlay(), Some(Overlay::Help(_))));
    assert_eq!(last_error(&app), "send: a replay has no link to send to");
    assert!(app.next_command_action(Instant::now()).is_none());
    assert!(run_line(&mut app, "send-hex 0d0a").is_empty());
    assert_eq!(
        last_error(&app),
        "send-hex: a replay has no link to send to"
    );
}

/// MAX-6: an armed trigger whose text comes in runs its command line as a typed one, with
/// a notice, once (one shot); a disarmed one doesn't. A replay's bytes and what the restore
/// question held back never fire one.
/// How: an armed `u-boot` rule and a disarmed `login` one, the U-Boot line fed live, again,
/// then as a replay and as held bytes.
#[test]
fn triggers_fire_on_live_bytes() {
    let config: crate::configs::TriggersConfig = toml::from_str(
        r#"
            [u-boot]
            match = "Hit any key to stop autoboot"
            run = "send ' '"
            armed = true
            [login]
            match = "login: "
            run = "send root\r"
            "#,
    )
    .unwrap();
    let mut app = app();
    app.set_triggers(config.compile().unwrap());
    let effects = |app: &mut App| {
        let mut effects = Vec::new();
        while let Some((_, action)) = app.next_command_action(Instant::now()) {
            effects.push(app.update(action));
        }
        effects
    };
    let uboot = b"U-Boot 2024.01\r\nHit any key to stop autoboot:  3";

    app.feed_rx(b"board login: ");
    assert!(effects(&mut app).is_empty());

    app.feed_rx(uboot);
    assert!(matches!(&effects(&mut app)[..], [UpdateEffect::Transmit(b)] if b == b" "));
    let toast = &app.notifications.active_toasts().last().unwrap().message;
    assert_eq!(toast, "Trigger u-boot: send ' '");
    assert!(app.command_history.is_empty());

    // one shot: disarmed now
    app.feed_rx(uboot);
    assert!(effects(&mut app).is_empty());

    let reload = |app: &mut App| {
        app.set_triggers(config.compile().unwrap());
    };
    reload(&mut app);
    app.held_rx = Some(uboot.to_vec());
    app.release_held_rx();
    assert!(effects(&mut app).is_empty());

    reload(&mut app);
    app.link
        .set_address("file:///tmp/capture.raw".parse::<Address>().unwrap());
    app.feed_rx(uboot);
    assert!(effects(&mut app).is_empty());
}

/// MAX-220 step 0: a trigger's command line runs once the whole chunk its text ended in is
/// fed, and `mark` in it marks where the command marks: the last complete line of a live
/// view, not the line that matched. A crash line followed by more lines in the same chunk
/// isn't the one marked; one still arriving isn't a line yet, the one before is marked.
/// How: line mode, a rearmed `mark c` trigger on `panic`; one chunk with the panic line and
/// a line after it, then a chunk ending inside another panic line.
#[test]
fn trigger_mark_command_marks_the_last_line() {
    let config: crate::configs::TriggersConfig = toml::from_str(
        r#"
            [crash]
            match = "panic"
            run = "mark c"
            armed = true
            rearm = true
            "#,
    )
    .unwrap();
    let mut app = app();
    app.settings.mode = Mode::Line;
    app.set_triggers(config.compile().unwrap());
    let fed = |app: &mut App, bytes: &[u8]| {
        app.feed_rx(bytes);
        while let Some((_, action)) = app.next_command_action(Instant::now()) {
            app.update(action);
        }
    };

    fed(&mut app, b"boot\r\npanic here\r\nreboot\r\n");
    assert_eq!(texts(&app), ["boot", "panic here", "reboot"]);
    assert_eq!(app.line_mode.marks().get('c'), Some(2), "the line after");

    fed(&mut app, b"up\r\npanic again");
    assert_eq!(app.line_mode.marks().get('c'), Some(3), "the line before");
    assert_eq!(app.line_mode.partial_rx(), b"panic again");
}

/// MAX-220: a trigger's `mark` goes on the line its text ended on, in each mode fed: not a
/// later line of the same chunk; a line still arriving is marked as the entry it becomes;
/// a text cut between two chunks marks its line; a rearmed trigger moves its letter to the
/// latest match. With only the mode shown fed, only its marks; a replay marks nothing.
/// How: a rearmed `crash` trigger marking `c` on `panic`; chunks in line mode with both modes
/// fed, read through each mode's marks popup rows; then terminal mode alone; then a replay.
#[test]
fn trigger_marks_its_line() {
    use crate::configs::settings::DualBufferMode;
    let config: crate::configs::TriggersConfig = toml::from_str(
        r#"
            [crash]
            match = "panic"
            run = "notify error crashed"
            mark = "c"
            armed = true
            rearm = true
            silent = true
            "#,
    )
    .unwrap();
    let mut app = app();
    app.settings.mode = Mode::Line;
    app.set_triggers(config.compile().unwrap());
    assert_eq!(app.triggers()[0].mark, Some('c'));
    let marked = |app: &mut App| {
        let line = app.line_mode.mark_rows().into_iter().map(|row| row.text);
        let terminal = app.terminal_mode.mark_rows().into_iter();
        let terminal = terminal.map(|row| row.text.trim_end().to_string());
        (line.collect::<Vec<_>>(), terminal.collect::<Vec<_>>())
    };
    let both = |text: &str| (vec![text.to_string()], vec![text.to_string()]);

    app.feed_rx(b"boot\r\npanic here\r\nreboot\r\n");
    assert_eq!(app.line_mode.marks().get('c'), Some(1));
    assert_eq!(marked(&mut app), both("panic here"));

    app.feed_rx(b"up\r\npanic again");
    assert_eq!(
        app.line_mode.marks().get('c'),
        Some(4),
        "the entry it becomes"
    );
    app.feed_rx(b" and again\r\n");
    assert_eq!(marked(&mut app), both("panic again and again"));

    app.feed_rx(b"x\r\npa");
    app.feed_rx(b"nic cut\r\nafter\r\n");
    assert_eq!(marked(&mut app), both("panic cut"));

    app.settings.mode = Mode::Terminal;
    app.settings.dual_mode_buffering = DualBufferMode::ActiveOnly;
    app.feed_rx(b"panic terminal\r\nmore\r\n");
    assert_eq!(
        marked(&mut app),
        (
            vec!["panic cut".to_string()],
            vec!["panic terminal".to_string()]
        )
    );

    app.link
        .set_address("file:///tmp/capture.raw".parse::<Address>().unwrap());
    app.feed_rx(b"panic replayed\r\n");
    assert_eq!(marked(&mut app).1, ["panic terminal"]);
}

/// MAX-220: each trigger counts its fires, a silent one too, not while disarmed;
/// `trigger reset` sets one's count or all back to zero with a notice, a name that isn't
/// one refused. A reload keeps the counts of the triggers of the same name; a switch to
/// another device resets them.
/// How: the default triggers armed (`login`, `panic` silent, `u-boot` one shot), their texts
/// fed; command lines; a reload; then `switch_session`.
#[test]
fn trigger_counts() {
    let mut app = app();
    let reload = |app: &mut App| {
        let config = crate::configs::TriggersConfig::default();
        app.set_triggers(config.compile().unwrap());
    };
    reload(&mut app);
    let run = |app: &mut App, line: &str| {
        let invocations = crate::commands::parse(line).unwrap();
        app.update(Action::App(AppAction::QueueCommands(invocations)));
        while let Some((_, action)) = app.next_command_action(Instant::now()) {
            app.update(action);
        }
        let toasts = app.notifications.active_toasts();
        toasts.last().unwrap().message.clone()
    };
    let hits = |app: &App| -> Vec<(String, u32)> {
        let triggers = app.triggers().iter();
        triggers.map(|t| (t.name.clone(), t.hits)).collect()
    };
    let counts = |login, panic, uboot| {
        vec![
            ("login".to_string(), login),
            ("panic".to_string(), panic),
            ("u-boot".to_string(), uboot),
        ]
    };

    app.feed_rx(b"login: Kernel panic");
    assert_eq!(hits(&app), counts(0, 0, 0), "disarmed");
    run(&mut app, "trigger on");
    for _ in 0..2 {
        app.feed_rx(b"login: Kernel panic\r\nHit any key to stop autoboot");
    }
    assert_eq!(hits(&app), counts(2, 2, 1), "u-boot one shot");

    assert_eq!(
        run(&mut app, "trigger reset panic"),
        "Trigger panic: count reset"
    );
    assert_eq!(hits(&app), counts(2, 0, 1));
    assert_eq!(
        run(&mut app, "trigger reset nope"),
        "trigger: no trigger named 'nope'"
    );

    reload(&mut app);
    assert_eq!(hits(&app), counts(2, 0, 1), "kept across a reload");
    assert_eq!(run(&mut app, "trigger reset"), "Trigger counts reset");
    assert_eq!(hits(&app), counts(0, 0, 0));

    run(&mut app, "trigger on");
    app.feed_rx(b"login: ");
    assert_eq!(hits(&app), counts(1, 0, 0));
    app.switch_session();
    assert_eq!(hits(&app), counts(0, 0, 0), "reset by a device switch");
}

/// MAX-6: the toggle arms every trigger when none is armed, else disarms them all; the
/// `trigger` command arms, disarms or toggles all or one by name, refuses a name that
/// isn't one, and each change says what is armed. With no trigger at all, both say so.
/// How: the default triggers (`login`, `u-boot`), then none.
#[test]
fn arming_triggers() {
    let mut app = app();
    app.set_triggers(
        crate::configs::triggers::tests::two_examples()
            .compile()
            .unwrap(),
    );
    let last_toast = |app: &App| {
        let toasts = app.notifications.active_toasts();
        toasts.last().unwrap().message.clone()
    };
    let run = |app: &mut App, line: &str| {
        app.update(Action::App(AppAction::QueueCommands(
            crate::commands::parse(line).unwrap(),
        )));
        while let Some((_, action)) = app.next_command_action(Instant::now()) {
            app.update(action);
        }
    };

    app.update(KeyBindingAction::ToggleTriggers.to_action(&app));
    assert_eq!(app.armed_triggers(), ["login", "u-boot"]);
    assert_eq!(last_toast(&app), "Triggers armed: login, u-boot");
    run(&mut app, "trigger off login");
    assert_eq!(app.armed_triggers(), ["u-boot"]);
    // one armed: the toggle disarms
    app.update(KeyBindingAction::ToggleTriggers.to_action(&app));
    assert!(app.armed_triggers().is_empty());
    assert_eq!(last_toast(&app), "Triggers disarmed");
    run(&mut app, "trigger on u-boot");
    assert_eq!(app.armed_triggers(), ["u-boot"]);
    run(&mut app, "trigger on");
    assert_eq!(app.armed_triggers(), ["login", "u-boot"]);
    // toggled by name, or all: none armed arms them all
    run(&mut app, "trigger toggle u-boot");
    assert_eq!(app.armed_triggers(), ["login"]);
    run(&mut app, "trigger toggle u-boot");
    assert_eq!(app.armed_triggers(), ["login", "u-boot"]);
    run(&mut app, "trigger toggle");
    assert!(app.armed_triggers().is_empty());
    run(&mut app, "trigger toggle");
    assert_eq!(app.armed_triggers(), ["login", "u-boot"]);

    run(&mut app, "trigger off uboot; trigger off");
    assert_eq!(
        last_toast(&app),
        "trigger: no trigger named 'uboot' (1 more not run)"
    );
    assert_eq!(app.armed_triggers(), ["login", "u-boot"]);

    // the command line completes their names
    app.update(KeyBindingAction::CommandLine.to_action(&app));
    app.update(Action::Overlay(DialogAction::Paste(
        "trigger on u".to_string(),
    )));
    let Some(Overlay::CommandLine(command_line)) = app.overlay() else {
        panic!("the command line");
    };
    assert_eq!(command_line.completion.candidates[0].value, "u-boot");
    app.update(Action::Overlay(DialogAction::Close));

    app.set_triggers(Vec::new());
    run(&mut app, "trigger on");
    assert_eq!(last_toast(&app), "trigger: triggers.toml has no trigger");
    app.update(KeyBindingAction::ToggleTriggers.to_action(&app));
    assert_eq!(last_toast(&app), "triggers.toml has no trigger");
}

/// MAX-6: `mash` sends its text at once then every interval, the rest of its line
/// waiting; its text coming in ends it and the line goes on. The timeout, a key typed for
/// the device and `cancel` stop it and the rest of its line. It refuses a second mash, a
/// replay and a closed link.
/// How: a space every 50 ms until `=> `, a second at most, the timeout's clock passed by
/// hand.
#[test]
fn mash_until_its_text() {
    let mut app = app();
    let last_toast = |app: &App| {
        let toasts = app.notifications.active_toasts();
        toasts.last().unwrap().message.clone()
    };
    let queue = |app: &mut App, line: &str| {
        let invocations = crate::commands::parse(line).unwrap();
        app.update(Action::App(AppAction::QueueCommands(invocations)));
    };
    // the chain's actions built at `now`, their effects
    let run = |app: &mut App, now: Instant| {
        let mut effects = Vec::new();
        while let Some((_, action)) = app.next_command_action(now) {
            effects.push(app.update(action));
        }
        effects
    };

    queue(&mut app, "mash ' ' '=> ' 50 1000; send x");
    assert!(matches!(
        &run(&mut app, Instant::now())[..],
        [UpdateEffect::None]
    ));
    assert!(app.mash().is_some());
    assert_eq!(last_toast(&app), "Mashing until '=> '");
    assert_eq!(app.next_mash_bytes(Instant::now()), Some(b" ".to_vec()));
    assert_eq!(app.next_mash_bytes(Instant::now()), None);
    // the line waits
    assert!(run(&mut app, Instant::now()).is_empty());
    app.feed_rx(b"Hit any key to stop autoboot:  0 \r\n=");
    assert!(app.mash().is_some());
    app.feed_rx(b"> ");
    assert!(app.mash().is_none());
    assert_eq!(last_toast(&app), "Mash done: '=> ' came in");
    let effects = run(&mut app, Instant::now());
    assert!(matches!(&effects[..], [UpdateEffect::Transmit(x)] if x == b"x"));

    // the timeout, then a key, then `cancel`
    queue(&mut app, "mash ' ' '=> ' 50 1000; send x");
    run(&mut app, Instant::now());
    // past the deadline, set when it was built
    let late = Instant::now() + Duration::from_millis(1000);
    assert_eq!(app.next_mash_bytes(late), None);
    assert!(app.mash().is_none());
    assert_eq!(
        last_toast(&app),
        "mash: '=> ' didn't come in (1 more not run)"
    );
    assert!(run(&mut app, late).is_empty());

    queue(&mut app, "mash ' ' '=> '; send x");
    run(&mut app, Instant::now());
    let key = crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('a'));
    app.update(Action::LineInput(LineInputAction::HandleKey(key)));
    assert!(app.mash().is_none());
    assert_eq!(
        last_toast(&app),
        "Mash stopped: a key was typed (1 more not run)"
    );

    queue(&mut app, "mash ' ' '=> '");
    run(&mut app, Instant::now());
    queue(&mut app, "cancel");
    run(&mut app, Instant::now());
    assert!(app.mash().is_none());
    assert_eq!(last_toast(&app), "Stopped mashing");

    // refused
    queue(&mut app, "mash ' ' '=> '; mash x y");
    run(&mut app, Instant::now());
    assert_eq!(last_toast(&app), "Mashing until '=> '");
    app.update(Action::App(AppAction::CancelCommands));
    set_link_open(&mut app, false);
    queue(&mut app, "mash ' ' '=> '");
    run(&mut app, Instant::now());
    assert!(last_toast(&app).starts_with("mash: the port isn't open"));
    set_link_open(&mut app, true);
    app.link
        .set_address("file:///tmp/capture.raw".parse::<Address>().unwrap());
    queue(&mut app, "mash ' ' '=> '");
    run(&mut app, Instant::now());
    assert_eq!(last_toast(&app), "mash: a replay has no link to send to");
}

/// A pty pair: maxicom opens the `path` side as its port, the test reads and writes the
/// `device` side, as `examples/fake_device.rs` does through socat
#[cfg(target_os = "linux")]
pub struct Pty {
    device: std::fs::File,
    pub path: String,
}

#[cfg(target_os = "linux")]
impl Pty {
    pub fn open() -> Self {
        use std::os::fd::FromRawFd;
        unsafe {
            let fd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK);
            assert!(fd >= 0, "posix_openpt");
            assert_eq!(libc::grantpt(fd), 0);
            assert_eq!(libc::unlockpt(fd), 0);
            let mut name = [0 as libc::c_char; 64];
            assert_eq!(libc::ptsname_r(fd, name.as_mut_ptr(), name.len()), 0);
            let path = std::ffi::CStr::from_ptr(name.as_ptr()).to_str().unwrap();
            Self {
                device: std::fs::File::from_raw_fd(fd),
                path: path.to_string(),
            }
        }
    }

    /// The port side's termios, read through the device side (a pty's master reads its
    /// slave's), so it works while maxicom holds the port with `TIOCEXCL`
    pub fn termios(&self) -> libc::termios {
        use std::os::fd::AsRawFd;
        let mut termios: libc::termios = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { libc::tcgetattr(self.device.as_raw_fd(), &mut termios) },
            0
        );
        termios
    }

    /// Sets or clears the port side's `HUPCL`, as `stty hupcl` / `stty -hupcl` would
    pub fn set_hupcl(&self, on: bool) {
        use std::os::fd::AsRawFd;
        let mut termios = self.termios();
        if on {
            termios.c_cflag |= libc::HUPCL;
        } else {
            termios.c_cflag &= !libc::HUPCL;
        }
        let fd = self.device.as_raw_fd();
        assert_eq!(unsafe { libc::tcsetattr(fd, libc::TCSANOW, &termios) }, 0);
    }

    /// What came out of the port, `len` bytes, waiting `within` at most: what is there
    /// already with no wait
    fn received(&mut self, len: usize, within: Duration) -> Vec<u8> {
        use std::io::Read;
        let deadline = Instant::now() + within;
        let mut bytes = Vec::new();
        let mut buf = [0; 256];
        while bytes.len() < len {
            match self.device.read(&mut buf) {
                Ok(n) => bytes.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("{e}"),
            }
        }
        bytes
    }
}

/// MAX-12: command lines on a real tty, their effects applied as the main loop does: the
/// bytes of `send` and `send-hex` reach the other end in order, `baud` and `framing`
/// change the port's settings, and a setting or a line the port refuses stops the rest of
/// the line.
/// How: a pty pair in place of socat and `fake_device`; a pty refuses parity, 7 bits and
/// the modem lines.
#[cfg(target_os = "linux")]
#[test]
fn commands_on_the_wire() {
    use crate::link::serial::{CharSize, LineControl, SerialDevice, StopBits};
    let mut pty = Pty::open();
    let mut app = app();
    app.link.set_address(Address::Serial(pty.path.clone()));
    let mut device = SerialDevice::open(&pty.path, &app.link_config().line).unwrap();
    // a pty has no modem lines
    assert!(device.control(LineControl::Dtr(true)).is_err());
    let mut run = |app: &mut App, line: &str| {
        let invocations = crate::commands::parse(line).unwrap();
        app.update(Action::App(AppAction::QueueCommands(invocations)));
        while let Some((_, action)) = app.next_command_action(Instant::now()) {
            match app.update(action) {
                UpdateEffect::Transmit(bytes) => device.write(&bytes).unwrap(),
                UpdateEffect::Reconfigure(line) => match device.reconfigure(&line) {
                    Ok(()) => set_line(app, line),
                    Err(_) => {
                        app.drop_pending_commands();
                    }
                },
                UpdateEffect::LineControl(control) => {
                    if let Err(e) = device.control(control) {
                        app.fail_commands(format!("{control}: {e}"));
                    }
                }
                UpdateEffect::None => {}
                _ => panic!("{line}"),
            }
        }
        device.settings().unwrap()
    };

    run(&mut app, "send AT\\r\\n; send-hex 1b5b41; send 'a;b'");
    assert_eq!(pty.received(10, Duration::from_secs(1)), b"AT\r\n\x1b[Aa;b");

    let settings = run(&mut app, "baud 9600; framing 8N2");
    assert_eq!(settings.get_baud_rate().unwrap(), 9600);
    assert_eq!(settings.get_stop_bits().unwrap(), serial2::StopBits::Two);
    assert_eq!(app.link_config().line.baud, 9600);
    assert_eq!(app.link_config().line.framing.stop_bits, StopBits::Two);

    // a pty keeps 8 bits and no parity: the port refuses, the line stops there
    let settings = run(&mut app, "framing 7E1; send y");
    assert_eq!(settings.get_char_size().unwrap(), serial2::CharSize::Bits8);
    assert_eq!(app.link_config().line.framing.char_size, CharSize::Bits8);
    assert!(pty.received(1, Duration::from_millis(50)).is_empty());

    run(&mut app, "dtr on; send x");
    let toast = app
        .notifications
        .active_toasts()
        .last()
        .unwrap()
        .message
        .clone();
    assert!(toast.starts_with("DTR on: "), "{toast}");
    assert!(toast.ends_with("(1 more not run)"), "{toast}");
    assert!(pty.received(1, Duration::from_millis(50)).is_empty());
}

/// MAX-6: `mash` on a real tty, its steps run as the main loop does: it sends its text
/// every interval, the device's prompt ends it and the rest of the line goes out after;
/// with no prompt it stops at its timeout, nothing sent after.
/// How: a pty pair, a space every 20 ms, the device answering `=> ` once it got five; then
/// `x` every 20 ms for 150 ms with no answer.
#[cfg(target_os = "linux")]
#[test]
fn mash_on_the_wire() {
    use std::io::Write;

    use crate::link::serial::SerialDevice;
    let mut pty = Pty::open();
    let mut app = app();
    app.link.set_address(Address::Serial(pty.path.clone()));
    let mut device = SerialDevice::open(&pty.path, &app.link_config().line).unwrap();
    // a turn of the main loop: what came in, the mash's step, the line's next commands
    let mut turn = |app: &mut App| {
        let mut buf = [0; 256];
        let n = device.read(&mut buf).unwrap();
        if n > 0 {
            app.feed_rx(&buf[..n]);
        }
        if let Some(bytes) = app.next_mash_bytes(Instant::now()) {
            device.write(&bytes).unwrap();
        }
        while let Some((_, action)) = app.next_command_action(Instant::now()) {
            match app.update(action) {
                UpdateEffect::Transmit(bytes) => device.write(&bytes).unwrap(),
                UpdateEffect::None => {}
                _ => panic!("unexpected effect"),
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let queue = |app: &mut App, line: &str| {
        let invocations = crate::commands::parse(line).unwrap();
        app.update(Action::App(AppAction::QueueCommands(invocations)));
    };

    queue(&mut app, "mash ' ' '=> ' 20 2000; send ok");
    let start = Instant::now();
    let mut received = Vec::new();
    while received.len() < 5 {
        assert!(start.elapsed() < Duration::from_secs(1), "{received:?}");
        turn(&mut app);
        received.extend(pty.received(5 - received.len(), Duration::ZERO));
    }
    // five sends take four intervals at least
    assert!(start.elapsed() >= Duration::from_millis(80));
    assert_eq!(received, b"     ");
    pty.device.write_all(b"U-Boot> \r\n=> ").unwrap();
    let mut after = Vec::new();
    while !after.ends_with(b"ok") {
        assert!(start.elapsed() < Duration::from_secs(2), "{after:?}");
        turn(&mut app);
        after.extend(pty.received(1, Duration::ZERO));
    }
    assert!(app.mash().is_none());
    // a space may have gone out while the prompt came, then the rest of the line
    assert!(after == b"ok" || after == b" ok", "{after:?}");

    queue(&mut app, "mash x '=> ' 20 150; send late");
    let start = Instant::now();
    turn(&mut app);
    assert!(app.mash().is_some());
    while app.mash().is_some() {
        assert!(start.elapsed() < Duration::from_secs(1));
        turn(&mut app);
    }
    assert!(start.elapsed() >= Duration::from_millis(150));
    turn(&mut app);
    let sent = pty.received(64, Duration::from_millis(50));
    // one at once, then every 20 ms until 150 ms: 8 at most, never `late`
    assert!((6..=8).contains(&sent.len()), "{sent:?}");
    assert!(sent.iter().all(|&b| b == b'x'), "{sent:?}");
}

/// MAX-12: `send-file` starts a paced transfer and the chain goes on at once; its chunks
/// come from `transfer_step` until the end of the file, with a notice. A second
/// `send-file`, a missing file or a replay refuse; `cancel` stops it.
/// How: a 300-byte file at 9600 baud (96 bytes per 100 ms at most).
#[test]
fn command_line_send_file() {
    let mut app = app();
    let mut config = app.link_config().clone();
    config.line.baud = 9600;
    app.link.set_config(config);
    let path = std::env::temp_dir().join(format!("maxicom_test_{}_send.txt", std::process::id()));
    std::fs::write(&path, [b'x'; 300]).unwrap();
    let file = path.display().to_string().replace('\\', "/");
    let run_line = |app: &mut App, line: &str| {
        app.update(KeyBindingAction::CommandLine.to_action(app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
        let mut effects = Vec::new();
        while let Some((_, action)) = app.next_command_action(Instant::now()) {
            effects.push(app.update(action));
        }
        effects
    };
    let last_toast = |app: &App| {
        app.notifications
            .active_toasts()
            .last()
            .unwrap()
            .message
            .clone()
    };

    let effects = run_line(&mut app, &format!("send-file {file}; send y"));
    assert!(matches!(
        &effects[..],
        [UpdateEffect::None, UpdateEffect::Transmit(y)] if y == b"y"
    ));
    assert_eq!(app.transfer().unwrap().percent(), 0);
    assert!(run_line(&mut app, &format!("send-file {file}")).is_empty());
    assert_eq!(
        last_toast(&app),
        "send-file: a file is being sent already (cancel stops it)"
    );

    let start = Instant::now();
    let mut sent = 0;
    for step in 1..=10 {
        let chunk = app
            .transfer_step(&[], start + Duration::from_secs(step))
            .out;
        assert!(chunk.len() <= 96);
        sent += chunk.len();
    }
    assert_eq!(sent, 300);
    assert!(app.transfer().is_none());
    // the transfer's toasts follow each other in one box, above the refusal (MAX-228)
    let transfer_toast = |app: &App| -> String {
        let toasts = app.notifications.active_toasts();
        let toast = toasts.iter().find(|toast| toast.key == Some("transfer"));
        toast.map(|toast| toast.message.clone()).unwrap_or_default()
    };
    assert!(
        transfer_toast(&app).starts_with("Sent maxicom_test_"),
        "{}",
        transfer_toast(&app)
    );

    run_line(&mut app, &format!("send-file {file}"));
    app.transfer_step(&[], start + Duration::from_secs(1));
    run_line(&mut app, "cancel");
    assert!(app.transfer().is_none());
    assert!(
        transfer_toast(&app).ends_with(" at 32%"),
        "{}",
        transfer_toast(&app)
    );

    run_line(&mut app, &format!("send-file {file}.nope; help"));
    assert!(app.overlay().is_none());
    assert!(
        last_toast(&app).contains("(1 more not run)"),
        "{}",
        last_toast(&app)
    );

    app.link
        .set_address("file:///tmp/capture.raw".parse::<Address>().unwrap());
    run_line(&mut app, &format!("send-file {file}"));
    assert_eq!(
        last_toast(&app),
        "send-file: a replay has no link to send to"
    );
    let _ = std::fs::remove_file(&path);
}

/// MAX-162: `upload <protocol> <path>` starts a protocol transfer, waiting for its receiver,
/// and the chain goes on at once. Refused with a file being sent already, a mash running, an
/// unknown protocol; a mash is refused while it runs.
#[test]
fn command_line_upload() {
    use crate::{
        commands::{ArgValue, build},
        transfer::modem::Protocol,
    };
    let mut app = app();
    let path = std::env::temp_dir().join(format!("maxicom_test_{}_upload.bin", std::process::id()));
    std::fs::write(&path, [0x55; 300]).unwrap();
    let file = path.display().to_string().replace('\\', "/");
    let run_line = |app: &mut App, line: &str| {
        app.update(KeyBindingAction::CommandLine.to_action(app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
        while let Some((_, action)) = app.next_command_action(Instant::now()) {
            app.update(action);
        }
    };
    let toasts = |app: &App| -> Vec<String> {
        let toasts = app.notifications.active_toasts();
        toasts.iter().map(|toast| toast.message.clone()).collect()
    };
    let name = path.file_name().unwrap().to_string_lossy().into_owned();

    // with the progress row, which says it waits: no start toast (MAX-228)
    run_line(&mut app, &format!("upload ymodem {file}"));
    assert!(app.transfer().is_some());
    assert!(!toasts(&app).iter().any(|t| t.starts_with("Sending")));
    run_line(&mut app, "cancel");
    // without it, the toast says it
    app.settings_mut().ui.transfer_row = false;

    for (typed, protocol) in [
        ("xmodem", Protocol::Xmodem),
        ("xmodem-1k", Protocol::Xmodem1k),
        ("ymodem", Protocol::Ymodem),
    ] {
        run_line(&mut app, &format!("upload {typed} {file}"));
        let transfer = app.transfer().unwrap();
        assert_eq!(transfer.protocol(), Some(protocol));
        assert!(transfer.waiting());
        let started = format!(
            "Sending {name} (300 bytes) with {}: waiting for the receiver",
            protocol.name()
        );
        assert!(toasts(&app).contains(&started), "{:?}", toasts(&app));
        run_line(&mut app, "cancel");
    }

    run_line(&mut app, &format!("upload ymodem {file}"));
    run_line(&mut app, &format!("upload xmodem {file}"));
    let refused = "upload: a file is being sent already (cancel stops it)".to_string();
    assert!(toasts(&app).contains(&refused), "{:?}", toasts(&app));
    run_line(&mut app, "mash a b");
    let refused = "mash: a file transfer is running (cancel stops it)".to_string();
    assert!(toasts(&app).contains(&refused), "{:?}", toasts(&app));
    run_line(&mut app, "cancel");

    // a typed line waits behind a mash: built at once, as a bound key's could be
    run_line(&mut app, "mash a b");
    assert!(app.mash().is_some());
    let args = [ArgValue::Enum("ymodem"), ArgValue::Path(path.clone())];
    let refused = build::upload(&app, &args).unwrap_err();
    assert_eq!(refused, "a mash runs (cancel stops it)");
    run_line(&mut app, "cancel");

    run_line(&mut app, &format!("upload zmodem {file}"));
    assert!(app.transfer().is_none());
    let _ = std::fs::remove_file(&path);
}

/// MAX-162: `download <protocol> [path]` receives into `[transfers] dir`, or the folder
/// given (existing, or ending with `/`); for XMODEM, another path is the file, else
/// `xmodem_name` names it. YMODEM refuses a path that isn't a folder.
#[test]
fn command_line_download() {
    use crate::{
        commands::{ArgValue, build},
        transfer::modem::Protocol,
    };
    let mut app = app();
    let root = std::env::temp_dir().join(format!("maxicom_test_{}_dl", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    app.settings_mut().transfers.dir = root.clone();
    let start = |app: &App, args: &[ArgValue]| match build::download(app, args) {
        Ok(Action::App(AppAction::StartTransfer(transfer))) => Ok(transfer),
        Ok(other) => panic!("{other:?}"),
        Err(e) => Err(e),
    };
    let path = |path: std::path::PathBuf| ArgValue::Path(path);

    let transfer = start(&app, &[ArgValue::Enum("ymodem")]).unwrap();
    assert_eq!(transfer.protocol(), Some(Protocol::Ymodem));
    assert_eq!(transfer.folder(), Some(root.as_path()));
    assert_eq!(transfer.name(), "");

    let transfer = start(&app, &[ArgValue::Enum("xmodem")]).unwrap();
    assert!(
        transfer.name().starts_with("xmodem_"),
        "{}",
        transfer.name()
    );
    assert!(transfer.name().ends_with(".bin"), "{}", transfer.name());

    let new = root.join("new/");
    let transfer = start(&app, &[ArgValue::Enum("ymodem"), path(new.clone())]).unwrap();
    assert_eq!(transfer.folder(), Some(new.as_path()));

    let file = root.join("dump.bin");
    let transfer = start(&app, &[ArgValue::Enum("xmodem"), path(file.clone())]).unwrap();
    assert_eq!(
        (transfer.folder(), transfer.name()),
        (Some(root.as_path()), "dump.bin".into())
    );
    let refused = start(&app, &[ArgValue::Enum("ymodem"), path(file.clone())]).unwrap_err();
    assert_eq!(
        refused,
        format!(
            "{} isn't a folder: YMODEM files come with their names",
            file.display()
        )
    );

    // the default folder empty: the current directory, said as `.`
    app.settings_mut().transfers.dir = std::path::PathBuf::new();
    let transfer = start(&app, &[ArgValue::Enum("ymodem")]).unwrap();
    assert_eq!(transfer.folder(), Some(std::path::Path::new(".")));
    let _ = std::fs::remove_dir_all(&root);
}

/// MAX-12: `baud`, `framing` and `flow` give the port's settings for the loop to apply,
/// each on the settings the one before left; `dtr`, `rts` and `break` give the line
/// controls. `rts` refuses with hardware flow control, and all refuse on a TCP link or a
/// replay, stopping the chain.
/// How: the loop's part played by the test, which applies each settings change.
#[test]
fn command_line_serial_lines() {
    use crate::link::serial::{CharSize, FlowControl, Parity, StopBits};
    let mut app = app();
    let run_line = |app: &mut App, line: &str| {
        app.update(KeyBindingAction::CommandLine.to_action(app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
        let mut controls = Vec::new();
        while let Some((_, action)) = app.next_command_action(Instant::now()) {
            match app.update(action) {
                UpdateEffect::Reconfigure(line) => set_line(app, line),
                UpdateEffect::LineControl(control) => controls.push(control),
                _ => {}
            }
        }
        controls
    };
    let last_error = |app: &App| {
        app.notifications
            .active_toasts()
            .last()
            .unwrap()
            .message
            .clone()
    };

    run_line(&mut app, "baud 9600; framing 7e2; flow hw");
    let config = app.link_config();
    assert_eq!(config.line.baud, 9600);
    assert_eq!(
        (
            config.line.framing.char_size,
            config.line.framing.parity,
            config.line.framing.stop_bits
        ),
        (CharSize::Bits7, Parity::Even, StopBits::Two)
    );
    assert_eq!(config.line.flow_control, FlowControl::RtsCts);

    assert!(run_line(&mut app, "rts on").is_empty());
    assert_eq!(
        last_error(&app),
        "rts: hardware flow control drives RTS (flow none first)"
    );
    let controls = run_line(&mut app, "flow none; dtr off; rts on; break; break 100");
    assert_eq!(
        controls,
        [
            LineControl::Dtr(false),
            LineControl::Rts(true),
            LineControl::Break(Duration::from_millis(250)),
            LineControl::Break(Duration::from_millis(100)),
        ]
    );
    run_line(&mut app, "framing 9x1");
    assert!(last_error(&app).starts_with("framing: '9x1' isn't a framing"));

    app.link
        .set_address("tcp://localhost:4000".parse::<Address>().unwrap());
    run_line(&mut app, "baud 115200");
    assert_eq!(
        last_error(&app),
        "baud: a TCP link has no line settings, its bridge sets them"
    );
    assert!(run_line(&mut app, "dtr on; send x").is_empty());
    assert_eq!(
        last_error(&app),
        "dtr: a TCP link has no control lines, its bridge sets them (1 more not run)"
    );
    app.link
        .set_address("file:///tmp/capture.raw".parse::<Address>().unwrap());
    run_line(&mut app, "break");
    assert_eq!(last_error(&app), "break: a replay has no control lines");
    assert!(run_line(&mut app, "dtr on; rts on").is_empty());
    assert_eq!(
        last_error(&app),
        "dtr: a replay has no control lines (1 more not run)"
    );
    run_line(&mut app, "baud 115200");
    assert_eq!(last_error(&app), "baud: a replay has no line settings");
    assert_eq!(app.link_config().line.baud, 9600);
}

/// MAX-165 (A1 step 1): only the user's own input acts as typing. A command from a trigger
/// keeps the prefix the user pressed and lets a mash go on; the same actions as input end
/// the prefix and stop the mash.
/// How: each action from a trigger, then as input.
#[test]
fn only_input_acts_as_typing() {
    let mut app = app();
    let trigger = Origin::Trigger("boot".to_string());
    app.update(Action::App(AppAction::SetPrefix(true)));
    app.update_from(&trigger, KeyBindingAction::ScrollToBottom.to_action(&app));
    assert!(app.prefix_active);
    app.update(KeyBindingAction::ScrollToBottom.to_action(&app));
    assert!(!app.prefix_active);

    let mash = crate::commands::parse("mash ' ' '=> '").unwrap();
    app.update(Action::App(AppAction::QueueCommands(mash)));
    let (origin, action) = app.next_command_action(Instant::now()).unwrap();
    app.update_from(&origin, action);
    let key = crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('a'));
    app.update_from(&trigger, Action::LineInput(LineInputAction::HandleKey(key)));
    assert!(app.mash().is_some());
    app.update(Action::LineInput(LineInputAction::HandleKey(key)));
    assert!(app.mash().is_none());
}

/// The line doesn't parse: the command line stays open
pub fn parse_fails(app: &mut App, line: &str) -> bool {
    app.update(KeyBindingAction::CommandLine.to_action(app));
    app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
    app.update(Action::Overlay(DialogAction::Confirm));
    let open = matches!(app.overlay(), Some(Overlay::CommandLine(_)));
    app.close_overlay();
    open
}

/// MAX-12: `open` gives the port for the loop to open, its settings kept; the loop pauses
/// the line until its result, which resumes it or fails it. While the link isn't open,
/// `send` and the control lines refuse; a replay refuses `open`, and so does an address
/// that could never open (MAX-168).
/// How: the loop's part played by the test.
#[test]
fn command_line_open() {
    let mut app = app();
    let queue_line = |app: &mut App, line: &str| {
        app.update(KeyBindingAction::CommandLine.to_action(app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
    };
    let last_toast = |app: &App| {
        app.notifications
            .active_toasts()
            .last()
            .unwrap()
            .message
            .clone()
    };
    let now = Instant::now();

    queue_line(&mut app, "open /dev/ttyUSB9; send x");
    let open = app.next_command_action(now).unwrap().1;
    assert!(
        matches!(app.update(open), UpdateEffect::Open(address) if address == "/dev/ttyUSB9".parse::<Address>().unwrap())
    );
    // the loop: the link closed, the line waiting for the open
    set_link_open(&mut app, false);
    app.pause_commands(now + Duration::from_secs(6));
    assert!(app.next_command_action(now).is_none());
    set_link_open(&mut app, true);
    app.resume_commands();
    let send = app.next_command_action(now).unwrap().1;
    assert!(matches!(app.update(send), UpdateEffect::Transmit(x) if x == b"x"));

    set_link_open(&mut app, false);
    queue_line(&mut app, "send y; help");
    assert!(app.next_command_action(now).is_none());
    assert_eq!(
        last_toast(&app),
        "send: the port isn't open (detached or reconnecting) (1 more not run)"
    );
    queue_line(&mut app, "dtr on");
    assert!(app.next_command_action(now).is_none());
    assert_eq!(
        last_toast(&app),
        "dtr: the port isn't open (detached or reconnecting)"
    );
    // settings wait for the port to open
    queue_line(&mut app, "baud 9600");
    assert!(app.next_command_action(now).is_some());

    set_link_open(&mut app, true);
    queue_line(&mut app, "open file:///tmp/x.raw");
    let open = app.next_command_action(now).unwrap().1;
    assert!(
        matches!(app.update(open), UpdateEffect::Replay { path, delay, session: None }
            if path == Path::new("/tmp/x.raw") && delay.is_zero())
    );
    // MAX-168: an address that could never open is refused when the line is read, and
    // the line stays open to be fixed
    queue_line(&mut app, "open tcp://host");
    assert!(app.next_command_action(now).is_none());
    assert_eq!(
        last_toast(&app),
        "open: bad argument 1: 'host' is not host:port"
    );
    assert!(matches!(app.overlay(), Some(Overlay::CommandLine(_))));
    app.close_overlay();
    app.link
        .set_address("file:///tmp/capture.raw".parse::<Address>().unwrap());
    queue_line(&mut app, "open /dev/ttyUSB0");
    assert!(app.next_command_action(now).is_none());
    assert_eq!(
        last_toast(&app),
        "open: a replay can't open a link (replay-stop ends it)"
    );
}

/// MAX-166: `replay` gives the file to replay, `~` expanded, with its delay (none by
/// default); `replay-stop` stops a replay and refuses without one. The attach key stops a
/// replay started while running, not one from the command line; so does `Ctrl-a L`, bound
/// to `replay-stop`.
/// How: the commands run on a live port, then on a replay, with and without a live link
/// set aside.
#[test]
fn command_line_replay() {
    let mut app = app();
    let now = Instant::now();
    let run = |app: &mut App, line: &str| {
        app.update(KeyBindingAction::CommandLine.to_action(app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
        let (_, action) = app.next_command_action(now)?;
        Some(app.update(action))
    };
    let home = dirs::home_dir().unwrap();
    assert!(matches!(run(&mut app, "replay ~/x.raw 50"),
            Some(UpdateEffect::Replay { path, delay, session: None })
                if path == home.join("x.raw") && delay == Duration::from_millis(50)));
    assert!(matches!(run(&mut app, "replay /tmp/x.raw"),
            Some(UpdateEffect::Replay { delay, .. }) if delay.is_zero()));
    assert!(run(&mut app, "replay-stop").is_none());
    let last_toast = app.notifications.active_toasts().last().unwrap();
    assert_eq!(last_toast.message, "replay-stop: no replay runs");

    app.start_replay(PathBuf::from("/tmp/x.raw"), None);
    assert!(matches!(
        run(&mut app, "replay-stop"),
        Some(UpdateEffect::StopReplay)
    ));
    let detach = Action::App(AppAction::ToggleAttach);
    assert!(matches!(app.update(detach), UpdateEffect::StopReplay));

    // the default `Ctrl-a L` queues `replay-stop`
    app.update(Action::App(AppAction::SetPrefix(true)));
    let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('L'),
        crossterm::event::KeyModifiers::SHIFT,
    ));
    let action = crate::inputs::handle_event(event, &app);
    app.update(action);
    let (_, action) = app.next_command_action(now).expect("replay-stop queued");
    assert!(matches!(app.update(action), UpdateEffect::StopReplay));
    app.stop_replay();
    app.link
        .set_address("file:///tmp/x.raw".parse::<Address>().unwrap());
    let detach = Action::App(AppAction::ToggleAttach);
    assert!(matches!(app.update(detach), UpdateEffect::ToggleAttach));
}

/// MAX-12: `set` turns an on/off setting on or off (nothing when it is so already), `mode`
/// the mode (MAX-247),
/// `encoding` sets the encoding, `search` opens the search bar on its text with its matches,
/// `screenshot` saves the terminal at once in `screenshot.dir`, as text or with its colors
/// (`.ans`), and refuses in line mode.
/// How: each command run, then the state it left.
#[test]
fn command_line_set_encoding_search_screenshot() {
    let mut app = app();
    app.settings.mode = Mode::Line;
    let run_line = |app: &mut App, line: &str| {
        app.update(KeyBindingAction::CommandLine.to_action(app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
        while let Some((_, action)) = app.next_command_action(Instant::now()) {
            app.update(action);
        }
    };
    let last_toast = |app: &App| {
        app.notifications
            .active_toasts()
            .last()
            .unwrap()
            .message
            .clone()
    };

    let echo = app.settings.local_echo;
    let on_off = |on: bool| if on { "on" } else { "off" };
    run_line(&mut app, &format!("set local-echo {}", on_off(echo)));
    assert_eq!(app.settings.local_echo, echo);
    run_line(&mut app, &format!("set local-echo {}", on_off(!echo)));
    assert_eq!(app.settings.local_echo, !echo);
    run_line(
        &mut app,
        "set mouse off; set timestamps on; set answer-queries 0",
    );
    assert!(!app.settings.ui.mouse && app.settings.show_timestamps);
    assert!(app.settings.terminal_mode.answer_groups.is_empty());
    assert!(parse_fails(&mut app, "set line-wrap on"));

    // MAX-247: `mode` sets the mode, nothing when it is so already
    run_line(&mut app, "mode line");
    assert_eq!(app.settings.mode, Mode::Line);
    run_line(&mut app, "mode terminal; mode terminal");
    assert_eq!(app.settings.mode, Mode::Terminal);
    run_line(&mut app, "mode line");
    assert_eq!(app.settings.mode, Mode::Line);
    assert!(parse_fails(&mut app, "mode raw"));

    run_line(&mut app, "encoding cp437");
    assert_eq!(app.settings.encoding, crate::configs::TextEncoding::Cp437);
    assert_eq!(last_toast(&app), "Rx Text Encoding is CP437");
    run_line(&mut app, "encoding utf8");

    app.feed_rx(b"hello world\r\nbye\r\n");
    run_line(&mut app, "search world");
    let search = app.line_mode.search();
    assert!(search.active);
    assert_eq!(search.query_str(), "world");
    assert_eq!(search.matches.len(), 1);

    run_line(&mut app, "screenshot shot");
    assert_eq!(
        last_toast(&app),
        "screenshot: screenshots are in terminal mode"
    );
    let dir = std::env::temp_dir().join(format!("maxicom_test_{}_screenshots", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    app.settings.screenshot.dir = dir.clone();
    app.settings.mode = Mode::Terminal;
    app.feed_rx(b"\x1b[31mred\x1b[0m");
    run_line(&mut app, "screenshot shot; screenshot shot.ans");
    let text = std::fs::read_to_string(dir.join("shot.txt")).unwrap();
    let ansi = std::fs::read_to_string(dir.join("shot.ans")).unwrap();
    assert!(text.contains("red") && !text.contains('\x1b'));
    assert!(ansi.contains("\x1b["));
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-12: the lines that ran are kept for the next command lines, trimmed, not twice in a
/// row, the oldest dropped past `command_line.history_size`; a line that doesn't parse
/// isn't kept. The next command line recalls them with Up.
/// How: a history of 2, five lines run, one with a typo.
#[test]
fn command_line_history() {
    let mut app = app();
    app.settings.command_line.history_size = 2;
    for line in ["help ", "help", "nope", "settings", "toggle-mouse"] {
        app.update(KeyBindingAction::CommandLine.to_action(&app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
        while let Some((_, action)) = app.next_command_action(Instant::now()) {
            app.update(action);
        }
        app.close_overlay();
    }
    assert_eq!(app.command_history(), &["settings", "toggle-mouse"]);

    app.update(KeyBindingAction::CommandLine.to_action(&app));
    let up = crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Up);
    app.update(Action::Overlay(DialogAction::HandleKey(up)));
    app.update(Action::Overlay(DialogAction::HandleKey(up)));
    match app.overlay() {
        Some(Overlay::CommandLine(command_line)) => {
            assert_eq!(command_line.input.value(), "settings");
        }
        _ => panic!("the command line is closed"),
    }
}

/// Setting toggles that re-lay out the view reuse the last size `render` gave, not a
/// default one.
/// How: a 100x30 viewport in line mode, then the wrap toggle and the mode toggle (into
/// terminal mode); both scrollers must see 100x30.
#[test]
fn setting_toggles_keep_the_viewport() {
    let mut app = app();
    app.settings.mode = Mode::Line;
    app.update_viewport(ViewportSize::new(100, 30));

    app.toggle_setting(SettingKey::LineWrap);
    let scroller = app.line_mode.scroller();
    assert_eq!((scroller.view_width, scroller.view_height), (100, 30));

    app.toggle_setting(SettingKey::Mode);
    assert_eq!(app.settings.mode, Mode::Terminal);
    let scroller = app.terminal_mode.scroller();
    assert_eq!((scroller.view_width, scroller.view_height), (100, 30));
}

/// MAX-260: the mode `disabled_mode` turns off can't be switched to: the toggle stays and
/// warns, a replay doesn't go to it, and it receives nothing even with `always_both`.
/// With none disabled the toggle switches.
/// How: line mode with terminal disabled: the toggle, bytes received, a replay link; then
/// `disabled_mode = none` and the toggle
#[test]
fn disabled_mode_cant_be_switched_to() {
    let mut app = app();
    app.settings.mode = Mode::Line;
    app.settings.disabled_mode = DisabledMode::Terminal;
    app.update(Action::App(AppAction::ToggleSetting(SettingKey::Mode)));
    assert_eq!(app.settings.mode, Mode::Line);
    assert_eq!(
        toasts(&app),
        ["The terminal mode is disabled (disabled_mode)"]
    );

    app.feed_rx(b"hello\r\n");
    assert_eq!(app.terminal_mode.dump().stream().end(), 0);
    assert!(!app.line_mode.history().is_empty());

    app.link
        .set_address("file:///tmp/capture.raw".parse::<Address>().unwrap());
    app.fit_mode_to_link();
    assert_eq!(app.settings.mode, Mode::Line);

    app.settings.disabled_mode = DisabledMode::None;
    app.update(Action::App(AppAction::ToggleSetting(SettingKey::Mode)));
    assert_eq!(app.settings.mode, Mode::Terminal);
}

pub fn toasts(app: &App) -> Vec<String> {
    app.notifications
        .active_toasts()
        .iter()
        .map(|toast| toast.message.clone())
        .collect()
}

/// The reload action asks `main` for the I/O.
#[test]
fn reload_action_gives_the_effect() {
    let mut app = app();
    let effect = app.update(Action::App(AppAction::ReloadConfig));
    assert!(matches!(effect, UpdateEffect::ReloadConfig));
}

/// MAX-271: the redraw action asks `main` to clear the host terminal; `redraw` names it.
/// How: the action's effect, the name parsed as a keymap action
#[test]
fn redraw_action_gives_the_effect() {
    let mut app = app();
    let effect = app.update(Action::App(AppAction::Redraw));
    assert!(matches!(effect, UpdateEffect::Redraw));
    assert_eq!(
        "redraw".parse::<KeyBindingAction>(),
        Ok(KeyBindingAction::Redraw)
    );
}

/// A reload takes the file's settings over the session toggles, the new keymap, and the
/// new timestamp format's width; the view is laid out again and the prefix dropped.
#[test]
fn reload_takes_the_files_settings() {
    let mut app = app();
    // only the terminal view is laid out yet: the reload into line mode lays out that one
    app.settings.mode = Mode::Terminal;
    app.update_viewport(ViewportSize::new(100, 30));
    app.toggle_setting(SettingKey::LocalEcho);
    app.toggle_setting(SettingKey::DisplayTimestamp);
    app.update(Action::App(AppAction::SetPrefix(true)));

    let mut settings = SettingsConfig::default();
    settings.mode = Mode::Line;
    settings.line_mode.wrap_mode = WrapMode::Truncate;
    settings.timestamp_format = "%H:%M".to_string();
    let keymap = KeyBindingsConfig {
        prefix: "ctrl-b".to_string(),
        ..Default::default()
    };
    app.apply_reload(settings, KeyResolver::try_new(&keymap).unwrap());

    assert_eq!(app.settings.mode, Mode::Line);
    assert!(!app.settings.local_echo);
    assert!(!app.settings.show_timestamps);
    assert_eq!(app.settings.timestamp_width(), 5);
    assert_eq!(app.key_resolver.prefix.to_string(), "Ctrl-b");
    assert!(!app.is_prefix_active());
    let scroller = app.line_mode.scroller();
    assert_eq!((scroller.view_width, scroller.view_height), (100, 30));
    assert_eq!(toasts(&app), ["Config reloaded"]);
}

/// MAX-168: a link that isn't written is shown in terminal mode, whatever the settings say:
/// from the start and after a reload; a serial port keeps the mode it is given.
/// How: line mode in the settings, on a replay address then on a serial port.
#[test]
fn replay_keeps_terminal_mode() {
    let line_settings = || {
        let mut settings = SettingsConfig::default();
        settings.mode = Mode::Line;
        settings
    };
    let keys = || KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
    let on = |address: &str| LinkConfig {
        address: address.parse().unwrap(),
        ..LinkConfig::default()
    };

    let mut app = App::new(line_settings(), keys(), on("file:///tmp/boot.raw"));
    assert_eq!(app.settings.mode, Mode::Terminal);
    app.apply_reload(line_settings(), keys());
    assert_eq!(app.settings.mode, Mode::Terminal);

    let mut app = App::new(line_settings(), keys(), on("/dev/ttyUSB0"));
    assert_eq!(app.settings.mode, Mode::Line);
    app.apply_reload(line_settings(), keys());
    assert_eq!(app.settings.mode, Mode::Line);
}

/// Settings only read at startup keep their running value and are named in a notice;
/// the cheap ones (inactivity timeout, RX buffer) are taken.
#[test]
fn reload_keeps_startup_only_settings() {
    let mut app = app();
    let mut settings = SettingsConfig::default();
    settings.line_mode.history_size = 7;
    settings.terminal_mode.rows = 50;
    settings.sessions.enabled = false;
    settings.inactivity_detach_timeout = Duration::from_secs(9);
    settings.rx_buffer_size = 512;
    settings.encoding.toggle();
    app.apply_reload(
        settings,
        KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap(),
    );

    let defaults = SettingsConfig::default();
    assert_eq!(
        app.settings.line_mode.history_size,
        defaults.line_mode.history_size
    );
    assert_eq!(app.settings.terminal_mode.rows, defaults.terminal_mode.rows);
    assert!(app.settings.sessions.enabled);
    assert_eq!(
        app.settings.inactivity_detach_timeout,
        Duration::from_secs(9)
    );
    assert_eq!(app.settings.rx_buffer_size, 512);
    assert_ne!(app.settings.encoding, defaults.encoding);
    assert_eq!(
        toasts(&app),
        [
            "Config reloaded; restart to apply: line_mode.history_size, terminal_mode.rows, \
              sessions.enabled"
        ]
    );
}

/// MAX-181: minicom's add CR / add LF: prefix + `u` makes a lone LF a new line at the
/// start of the screen, prefix + `a` a lone CR a new line, each said in a toast; `set`
/// turns them on and off. With add LF a CR-only device has its lines in line mode and in
/// the text recording too.
/// How: "a\nb" and "c\rd" received with each off then on, read on the screen; "a\rb\r"
/// in line mode with a text recording.
#[test]
fn add_cr_and_add_lf_on_the_screen() {
    let screen = |add: Option<SettingKey>, bytes: &[u8]| {
        let mut app = app();
        if let Some(key) = add {
            app.update(Action::App(AppAction::SetPrefix(true)));
            let letter = if key == SettingKey::AddCr { 'u' } else { 'a' };
            let event = crossterm::event::Event::Key(crossterm::event::KeyEvent::from(
                crossterm::event::KeyCode::Char(letter),
            ));
            let action = crate::inputs::handle_event(event, &app);
            app.update(action);
        }
        app.feed_rx(bytes);
        (app.terminal_mode.vt100_screen().contents(), toasts(&app))
    };
    assert_eq!(screen(None, b"a\nb").0, "a\n b");
    let on = screen(Some(SettingKey::AddCr), b"a\nb");
    assert_eq!(on, ("a\nb".to_string(), vec!["Add CR On".to_string()]));
    assert_eq!(screen(None, b"c\rd").0, "d");
    let on = screen(Some(SettingKey::AddLf), b"c\rd");
    assert_eq!(on, ("c\nd".to_string(), vec!["Add LF On".to_string()]));

    let mut app = app();
    for (line, on) in [("set add-cr on", true), ("set add-cr off", false)] {
        app.update(KeyBindingAction::CommandLine.to_action(&app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
        while let Some((_, action)) = app.next_command_action(Instant::now()) {
            app.update(action);
        }
        assert_eq!(app.setting_is_on(SettingKey::AddCr), Some(on), "{line}");
    }

    let dir = crate::capture::tests::temp_dir("add_lf_text");
    app.settings.mode = Mode::Line;
    app.settings.capture.timestamp = false;
    app.settings.add_lf = true;
    app.start_text_capture(&dir.join("rx.txt"), false);
    app.feed_rx(b"a\rb\r");
    app.feed_rx(b"c");
    app.recorder.stop();
    let lines: Vec<&str> = (app.line_mode.history().iter())
        .map(|entry| entry.text.as_str())
        .collect();
    assert_eq!(lines, ["a\r", "b\r"]);
    let text = std::fs::read_to_string(dir.join("rx.txt")).unwrap();
    assert_eq!(text.lines().collect::<Vec<_>>(), ["a", "b"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-165 (A1 step 2): a screenshot that can't be written fails its command, so the rest
/// of its chain doesn't run (it was only said before).
/// How: a name under a file, which can't be a folder.
#[test]
fn a_failed_screenshot_stops_its_chain() {
    let mut app = app();
    let file = std::env::temp_dir().join(format!("maxicom_test_{}_shot_file", std::process::id()));
    std::fs::write(&file, b"").unwrap();
    let name = file.join("s").display().to_string().replace('\\', "/");
    let chain = crate::commands::parse(&format!("screenshot {name}; send x")).unwrap();
    app.update(Action::App(AppAction::QueueCommands(chain)));
    let (origin, shot) = app.next_command_action(Instant::now()).unwrap();
    app.dispatch_from(&origin, shot);
    let toast = &app.notifications.active_toasts().last().unwrap().message;
    assert!(toast.starts_with("Failed to save screenshot: "), "{toast}");
    assert!(toast.ends_with(" (1 more not run)"), "{toast}");
    assert!(app.next_command_action(Instant::now()).is_none());
    let _ = std::fs::remove_file(&file);
}

/// The mouse toggle flips the capture setting and says so (MAX-113), as the user's action
/// presents its note (MAX-165).
#[test]
fn mouse_toggle_flips_the_setting() {
    let mut app = app();
    assert!(app.settings.ui.mouse);
    app.update(Action::App(AppAction::ToggleSetting(SettingKey::Mouse)));
    assert!(!app.settings.ui.mouse);
    assert_eq!(toasts(&app), ["Mouse capture Off"]);
}

/// MAX-234: a setting cycled shows one box with its last value, two settings two boxes; the
/// plot's show, switch, freeze and off follow each other in one box; a "Close raw panel"
/// goes in the raw view's box.
/// How: the raw view cycled 4 times, then the encoding; the plots stepped and frozen.
#[test]
fn cycling_shows_one_box() {
    let mut app = app();
    for _ in 0..4 {
        app.update(Action::App(AppAction::ToggleSetting(
            SettingKey::DisplayRaw,
        )));
    }
    assert_eq!(toasts(&app), ["Raw SideView is Off"]);
    app.update(Action::App(AppAction::ToggleSetting(
        SettingKey::DisplayRaw,
    )));
    app.update(Action::App(AppAction::ToggleSetting(
        SettingKey::TextEncoding,
    )));
    assert_eq!(
        toasts(&app),
        ["Raw SideView is Hex", "Rx Text Encoding is ASCII"]
    );
    app.update(Action::App(AppAction::OpenOverlay(Overlay::ActionMenu(
        crate::overlays::ActionMenu::new((0, 0), Mode::Line, crate::link::LinkKind::Serial, false)
            .with_close(Some(crate::app::types::SidePanel::Raw)),
    ))));
    app.update(Action::Overlay(DialogAction::Confirm));
    assert_eq!(
        toasts(&app),
        ["Raw SideView is Off", "Rx Text Encoding is ASCII"]
    );

    app.notifications.dismiss_all();
    app.set_plots(crate::configs::plots::tests::demo_plots());
    for _ in 0..3 {
        app.step_plot(true);
    }
    app.toggle_plot_freeze();
    assert_eq!(toasts(&app), ["Plot sensors frozen"]);
    app.show_plot(None);
    assert_eq!(toasts(&app), ["Plot off"]);
}

/// MAX-147: a screenshot action takes the screen at once, then opens its dialog prefilled
/// with `[capture]`'s folder, name and the format's extension; Enter saves that screen
/// there, creating the folders.
/// How: a temp `screenshot.dir` that doesn't exist, a fixed name, "hi" on the screen at
/// the action, " later" received while the dialog is open.
#[test]
fn screenshot_dialog_prefills_and_saves() {
    let mut app = app();
    app.settings.mode = Mode::Terminal;
    let dir = std::env::temp_dir().join(format!("maxicom_test_{}_shots", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    app.settings.screenshot.dir = dir.join("new");
    app.settings.screenshot.name = "shot".to_string();
    app.terminal_mode.process_rx(app.settings.encoding, b"hi");

    app.update(Action::Terminal(TerminalAction::Screenshot(
        ScreenshotFormat::Text,
    )));
    let path = dir.join("new").join("shot.txt");
    match app.overlay() {
        Some(Overlay::Screenshot(dialog)) => {
            assert_eq!(dialog.file_input.value(), path.to_string_lossy())
        }
        _ => panic!("no screenshot dialog"),
    }
    app.terminal_mode
        .process_rx(app.settings.encoding, b" later");
    app.update(Action::Overlay(DialogAction::Confirm));
    assert!(app.overlay().is_none());
    assert_eq!(std::fs::read_to_string(&path).unwrap().trim_end(), "hi");
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-147: line mode doesn't show the terminal screen, so a screenshot there opens no
/// dialog and doesn't flash.
#[test]
fn screenshot_is_not_taken_in_line_mode() {
    let mut app = app();
    app.settings.mode = Mode::Line;
    app.update(Action::Terminal(TerminalAction::Screenshot(
        ScreenshotFormat::Ansi,
    )));
    assert!(app.overlay().is_none());
    assert!(!app.terminal_mode.has_pending_flash());
}

/// MAX-166: a replay started while running takes the link's place, on fresh histories in
/// terminal mode; the live link is set aside with its histories, counts and mode, its session
/// backup paused, and stopping the replay puts it all back, the session backup going on in the
/// same files. A replay replacing a replay keeps the live link; a replay from the command line
/// has none to go back to: its stop ends it. A new replay isn't ended.
/// How: "live" received in line mode with a session backup, a replay, "replayed" fed, a second
/// replay, the stop, "back" received; then an app on an ended replay from the start.
#[test]
fn replay_sets_the_live_link_aside() {
    let dir = crate::capture::tests::temp_dir("replay_aside");
    let mut app = app();
    app.settings_mut().mode = Mode::Line;
    app.start_session(Some(dir.clone()));
    app.feed_rx(b"live\r\n");
    app.stats.update_rx(6);

    app.start_replay(PathBuf::from("/tmp/a.raw"), None);
    assert_eq!(
        app.link_config().address,
        "file:///tmp/a.raw".parse::<Address>().unwrap()
    );
    assert_eq!(app.settings().mode, Mode::Terminal);
    assert!(texts(&app).is_empty() && !app.has_session());
    assert!(!app.terminal_mode.vt100_screen().contents().contains("live"));
    assert_eq!(app.stats.rx(), 0);
    app.feed_rx(b"replayed\r\n");
    assert_eq!(texts(&app), ["replayed"]);
    app.start_replay(PathBuf::from("/tmp/b.raw"), None);
    assert!(texts(&app).is_empty());

    assert!(app.stop_replay());
    assert_eq!(
        app.link_config().address,
        "/dev/ttyUSB0".parse::<Address>().unwrap()
    );
    assert_eq!(app.settings().mode, Mode::Line);
    assert_eq!(texts(&app), ["live"]);
    assert!(app.terminal_mode.vt100_screen().contents().contains("live"));
    assert_eq!(app.stats.rx(), 6);
    assert!(app.has_session() && !app.stop_replay());
    app.feed_rx(b"back\r\n");
    drop(app);
    let raw = crate::capture::tests::session_paths(&dir)
        .into_iter()
        .find(|path| path.extension().is_some_and(|ext| ext == "raw"))
        .unwrap();
    assert_eq!(std::fs::read(raw).unwrap(), b"live\r\nback\r\n");
    let _ = std::fs::remove_dir_all(&dir);

    let mut app = self::app();
    app.link
        .set_address("file:///tmp/a.raw".parse::<Address>().unwrap());
    app.end_replay();
    app.start_replay(PathBuf::from("/tmp/b.raw"), None);
    assert!(!app.is_replay_ended());
    assert!(!app.stop_replay() && app.is_replay_ended());
}

/// MAX-166: a session's replay decodes with its meta's encoding and shows its line
/// settings; stopping it gives the live link its own encoding back. A plain file replayed
/// after it decodes with the live link's encoding and has no line settings, nor has a
/// session recorded over TCP. A serial port's are its config's, a TCP link has none.
/// How: a live app in UTF-8, a cp437 session at 9600 7E1 replayed, a plain file, the stop;
/// then a TCP session, and the link kinds.
#[test]
fn session_replay_uses_its_meta() {
    let mut app = app();
    let mut session = crate::capture::tests::device("/dev/ttyACM1");
    session.encoding = crate::configs::TextEncoding::Cp437;
    session.line = Some(CaptureLine {
        baud: 9600,
        framing: "7E1".parse().unwrap(),
    });
    assert_eq!(
        app.line_settings(),
        Some(CaptureLine::of(&app.link_config().line))
    );
    assert_eq!(app.recorded_line(), None);

    app.start_replay(PathBuf::from("/tmp/s.raw"), Some(&session));
    assert_eq!(app.settings().encoding, TextEncoding::Cp437);
    assert_eq!(app.line_settings(), session.line);
    assert_eq!(app.recorded_line(), session.line);
    app.start_replay(PathBuf::from("/tmp/plain.raw"), None);
    assert_eq!(app.settings().encoding, TextEncoding::Utf8);
    assert_eq!(app.line_settings(), None);
    app.start_replay(PathBuf::from("/tmp/s.raw"), Some(&session));
    assert!(app.stop_replay());
    assert_eq!(app.settings().encoding, TextEncoding::Utf8);
    assert_eq!(app.recorded_line(), None);

    let tcp = crate::capture::tests::device("tcp://192.168.1.50:4000");
    app.start_replay(PathBuf::from("/tmp/t.raw"), Some(&tcp));
    assert_eq!(app.line_settings(), None);
    assert!(app.stop_replay());
    app.link
        .set_address("tcp://192.168.1.50:4000".parse::<Address>().unwrap());
    assert_eq!(app.line_settings(), None);
}

/// MAX-163: a full recording gets a meta file beside its raw, and a file replayed without a
/// session (`--replay`, `replay`) takes its device from its meta file: its encoding and
/// line settings. A file without one has none. MAX-172: a recording while replaying writes
/// the replayed session's device, none while replaying a plain file.
/// How: a cp437 port at 9600 7E1 records `rec.raw`, then replays it from a UTF-8 app at
/// the default line; a recording made during that replay; a plain file, and a recording
/// during its replay.
#[test]
fn replay_reads_the_meta_file_beside() {
    let dir = crate::capture::tests::temp_dir("replay_meta");
    std::fs::create_dir_all(&dir).unwrap();
    let plain = dir.join("plain.raw");
    std::fs::write(&plain, b"x").unwrap();

    let mut app = app();
    let line = LineSettings {
        baud: 9600,
        framing: "7E1".parse().unwrap(),
        ..LineSettings::default()
    };
    set_line(&mut app, line);
    app.settings.encoding = TextEncoding::Cp437;
    record_full_meta(&mut app, &dir.join("rec"), false);
    assert!(dir.join("rec/rec.meta").exists());
    std::fs::write(dir.join("rec/rec.raw"), b"x").unwrap();

    // the recording's folder is enough (MAX-175)
    let mut app = self::app();
    app.open_replay(dir.join("rec"), Duration::ZERO, None, &[])
        .unwrap();
    let replayed = crate::link::Address::Replay(dir.join("rec/rec.raw"));
    assert_eq!(app.link_config().address, replayed);
    assert_eq!(app.settings().encoding, TextEncoding::Cp437);
    assert_eq!(app.recorded_line(), Some(CaptureLine::of(&line)));

    record_full_meta(&mut app, &dir.join("during"), false);
    let meta = RecordingMeta::read(&dir.join("during/during.meta")).unwrap();
    assert_eq!(meta.device.encoding, TextEncoding::Cp437);
    assert_eq!(meta.device.line, Some(CaptureLine::of(&line)));

    app.open_replay(plain, Duration::ZERO, None, &[]).unwrap();
    assert_eq!(app.recorded_line(), None);
    record_full_meta(&mut app, &dir.join("plain_during"), false);
    assert!(dir.join("plain_during/plain_during.raw").exists());
    assert!(!dir.join("plain_during/plain_during.meta").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

const DSR: &[u8] = b"\x1b[5n";
const DSR_READY: &[u8] = b"\x1b[0n";

/// MAX-151: a query received in terminal mode is answered, once; in line mode it is not,
/// even when the terminal is fed too, and it is not answered later on a switch.
/// How: the default `always_both` buffering, DSR status received in each mode.
#[test]
fn queries_answered_in_terminal_mode_only() {
    let mut app = app();
    app.settings.mode = Mode::Terminal;
    app.feed_rx(DSR);
    assert_eq!(app.take_query_answers(), DSR_READY);
    assert!(app.take_query_answers().is_empty());

    app.settings.mode = Mode::Line;
    app.feed_rx(DSR);
    assert!(app.take_query_answers().is_empty());
    app.settings.mode = Mode::Terminal;
    app.feed_rx(b"x");
    assert!(app.take_query_answers().is_empty());
}

/// MAX-264: with groups in `notify_unhandled`, one notification counts what the emulator
/// didn't handle in those groups, by kind, across reads while it's shown; none by default.
/// How: NUL and an unknown CSI with no group, then all but `shell` over two reads, then
/// `csi` alone (no NUL nor DEL counted)
#[test]
fn unhandled_bytes_notified() {
    let mut app = app();
    app.feed_rx(b"a\x00b\x1b[5W");
    assert!(toasts(&app).is_empty());
    let mut app = app_with_notify(&ALL_BUT_SHELL);
    app.feed_rx(b"a\x00b\x1b[5W");
    assert_eq!(toasts(&app), ["Unhandled: NUL, CSI 5W"]);
    app.feed_rx(b"\x00\x00\x7f");
    assert_eq!(toasts(&app), ["Unhandled: NUL ×3, CSI 5W, DEL"]);

    let mut app = app_with_notify(&[UnhandledGroup::Csi]);
    app.feed_rx(b"\x00\x7f\x1b[5W\x1bPq\x1b\\");
    assert_eq!(toasts(&app), ["Unhandled: CSI 5W"]);
}

/// MAX-264: the harmless sequences of shells are only in `shell`: `osc` and `csi` leave them
/// out, `shell` alone counts them.
/// How: OSC 7 and `?2026h` with all but `shell`, then with `shell` alone
#[test]
fn unhandled_shell_group() {
    let shell_bytes = b"\x1b]7;file://board/root\x07\x1b[?2026h";
    let mut app = app_with_notify(&ALL_BUT_SHELL);
    app.feed_rx(shell_bytes);
    assert!(toasts(&app).is_empty());
    let mut app = app_with_notify(&[UnhandledGroup::Shell]);
    app.feed_rx(shell_bytes);
    assert_eq!(toasts(&app), ["Unhandled: OSC 7, CSI ?2026h"]);
}

const ALL_BUT_SHELL: [UnhandledGroup; 6] = [
    UnhandledGroup::Controls,
    UnhandledGroup::C1,
    UnhandledGroup::Escape,
    UnhandledGroup::Csi,
    UnhandledGroup::Osc,
    UnhandledGroup::Strings,
];

/// An app with these groups in `notify_unhandled`
fn app_with_notify(groups: &[UnhandledGroup]) -> App {
    let mut app = app();
    app.settings.terminal_mode.notify_unhandled = groups.to_vec();
    app
}

/// MAX-253: marking doesn't change the notification; marking alone leaves nothing collected
/// behind; the setting rules each read.
/// How: NUL and an unknown CSI notified for both groups and marked for `controls`; then
/// `controls` marked only; then the list emptied
#[test]
fn unhandled_marked_and_notified() {
    let mut app = app_with_notify(&ALL_BUT_SHELL);
    app.settings.terminal_mode.mark_unhandled = vec![UnhandledGroup::Controls];
    app.feed_rx(b"a\x00b\x1b[5Wc");
    assert_eq!(toasts(&app), ["Unhandled: NUL, CSI 5W"]);
    let screen = app.terminal_mode.vt100_screen().contents();
    assert!(screen.starts_with("a\u{2400}bc"), "{screen:?}");

    let mut app = app_with_notify(&[]);
    app.settings.terminal_mode.mark_unhandled = vec![UnhandledGroup::Controls];
    app.feed_rx(b"a\x00b");
    assert!(app.terminal_mode.take_dropped().is_empty());
    assert!(toasts(&app).is_empty());
    app.settings.terminal_mode.mark_unhandled = Vec::new();
    app.feed_rx(b"\x00c");
    let screen = app.terminal_mode.vt100_screen().contents();
    assert!(screen.starts_with("a\u{2400}bc"), "{screen:?}");
}

/// MAX-264: past six kinds, the notification shows the six seen most and how many more.
/// How: NUL three times, then seven other C0 controls once each; then seven kinds once
#[test]
fn unhandled_kinds_capped() {
    let mut app = app_with_notify(&[UnhandledGroup::Controls]);
    app.feed_rx(b"\x00\x00\x00\x01\x02\x03\x04\x06\x10\x11");
    assert_eq!(
        toasts(&app),
        ["Unhandled: NUL ×3, SOH, STX, ETX, EOT, ACK, 2 more kinds"]
    );
    let mut app = app_with_notify(&[UnhandledGroup::Controls]);
    app.feed_rx(b"\x00\x01\x02\x03\x04\x06\x10");
    assert_eq!(
        toasts(&app),
        ["Unhandled: NUL, SOH, STX, ETX, EOT, ACK, 1 more kind"]
    );
}

/// MAX-275: the device's OSC 52 copy goes to the loop only with `device_clipboard` on, its
/// base64 as received, the last one kept; off, it's dropped, not kept for later.
/// How: a copy with the setting off, then on, then two copies in one read
#[test]
fn device_copy_follows_the_setting() {
    let mut app = app();
    app.feed_rx(b"\x1b]52;c;aGVsbG8=\x07");
    assert_eq!(app.take_device_copy(), None);
    app.settings.terminal_mode.device_clipboard = true;
    app.feed_rx(b"x");
    assert_eq!(app.take_device_copy(), None);
    app.feed_rx(b"\x1b]52;c;aGVsbG8=\x07");
    assert_eq!(app.take_device_copy().as_deref(), Some(&b"aGVsbG8="[..]));
    assert_eq!(app.take_device_copy(), None);
    app.feed_rx(b"\x1b]52;c;YQ==\x07\x1b]52;c;Yg==\x07");
    assert_eq!(app.take_device_copy().as_deref(), Some(&b"Yg=="[..]));
}

/// MAX-256: `byte_trace` off (the settings popup's `L`) drops the trace kept and records no
/// more; on again, the bytes received from then on are traced.
/// How: terminal mode, "abc", off, "def", on, "ghi"
#[test]
fn byte_trace_follows_the_setting() {
    let mut app = app();
    app.settings.mode = Mode::Terminal;
    let runs = |app: &App| app.terminal_mode.dump().writes().len();
    app.feed_rx(b"abc");
    assert_eq!(runs(&app), 1);
    app.toggle_setting(SettingKey::ByteTrace);
    assert_eq!(runs(&app), 0);
    app.feed_rx(b"\r\ndef");
    assert_eq!(runs(&app), 0);
    app.toggle_setting(SettingKey::ByteTrace);
    app.feed_rx(b"\r\nghi");
    assert_eq!(runs(&app), 1);
}

/// MAX-151: the settings apply to what is received after them: the popup toggle turns the
/// answers off and on, giving back the groups it put aside (all when none were), and
/// `enq_answer` answers ENQ once set.
/// How: DSR with the switch off then on; a custom list switched off and on; an empty list
/// switched on; then ENQ
#[test]
fn query_answers_follow_the_settings() {
    let mut app = app();
    app.settings.mode = Mode::Terminal;
    app.toggle_setting(SettingKey::AnswerQueries);
    assert!(app.settings.terminal_mode.answer_groups.is_empty());
    app.feed_rx(DSR);
    assert!(app.take_query_answers().is_empty());
    app.toggle_setting(SettingKey::AnswerQueries);
    app.feed_rx(DSR);
    assert_eq!(app.take_query_answers(), DSR_READY);

    let size = vec![QueryGroup::Size];
    app.settings.terminal_mode.answer_groups = size.clone();
    app.toggle_setting(SettingKey::AnswerQueries);
    app.toggle_setting(SettingKey::AnswerQueries);
    assert_eq!(app.settings.terminal_mode.answer_groups, size);
    app.settings.terminal_mode.answer_groups = Vec::new();
    app.toggle_setting(SettingKey::AnswerQueries);
    assert_eq!(app.settings.terminal_mode.answer_groups, QueryGroup::ALL);

    app.feed_rx(b"\x05");
    assert!(app.take_query_answers().is_empty());
    app.settings.terminal_mode.enq_answer = "board-42".to_string();
    app.feed_rx(b"\x05");
    assert_eq!(app.take_query_answers(), b"board-42");
}

/// One size report of mode 2048 for `rows` x `cols`, the pixels left out (the host
/// terminal's, 0 when it doesn't give them)
fn is_size_report(bytes: &[u8], rows: u16, cols: u16) -> bool {
    let text = String::from_utf8_lossy(bytes);
    text.starts_with(&format!("\x1b[48;{rows};{cols};"))
        && text.ends_with('t')
        && text.matches("\x1b[").count() == 1
}

/// MAX-159: once the remote app turns mode 2048 on, each change of the emulator's size sends
/// one report of the new size, taken with no RX; the same size sends nothing; in line mode
/// nothing is sent, now or on the way back; `?2048l` stops the reports.
/// How: terminal mode, `?2048h` (the first report), resizes to 30x100, 30x100 again, 20x60
/// in line mode, back to terminal mode, then `l` and 40x120.
#[test]
fn size_reports_on_each_resize() {
    let mut app = app();
    app.settings.mode = Mode::Terminal;
    app.feed_rx(b"\x1b[?2048h");
    let (rows, cols) = app.terminal_mode.get_vt100_size();
    assert!(is_size_report(&app.take_query_answers(), rows, cols));

    app.terminal_mode.set_vt100_size(30, 100);
    let report = app.take_query_answers();
    assert!(is_size_report(&report, 30, 100), "{report:?}");
    app.terminal_mode.set_vt100_size(30, 100);
    assert!(app.take_query_answers().is_empty());

    app.settings.mode = Mode::Line;
    app.terminal_mode.set_vt100_size(20, 60);
    assert!(app.take_query_answers().is_empty());
    app.settings.mode = Mode::Terminal;
    assert!(app.take_query_answers().is_empty());

    app.feed_rx(b"\x1b[?2048l");
    app.terminal_mode.set_vt100_size(40, 120);
    assert!(app.take_query_answers().is_empty());
}

/// MAX-277: once the remote app turns mode 1004 on, the window losing focus sends `CSI O` and
/// gaining it `CSI I`, taken with no RX; the same state again sends nothing; in line mode
/// nothing is sent, now or on the way back; without the `focus` group, nothing; `?1004l`
/// stops the reports.
/// How: terminal mode, a focus change before `?1004h`; then out, out again, in; out in line
/// mode, back; in with the groups without `focus`; then `l`
#[test]
fn focus_reports_on_focus_changes() {
    use crate::configs::QueryGroup;
    let mut app = app();
    app.settings.mode = Mode::Terminal;
    let focus = |app: &mut App, focused| {
        app.update(Action::App(AppAction::FocusGained(focused)));
        app.take_query_answers()
    };
    assert!(focus(&mut app, false).is_empty(), "not asked yet");
    assert!(focus(&mut app, true).is_empty());

    app.feed_rx(b"\x1b[?1004h");
    assert!(app.take_query_answers().is_empty(), "none at once");
    assert_eq!(focus(&mut app, false), b"\x1b[O");
    assert!(focus(&mut app, false).is_empty(), "no change");
    assert_eq!(focus(&mut app, true), b"\x1b[I");

    app.settings.mode = Mode::Line;
    assert!(focus(&mut app, false).is_empty());
    app.settings.mode = Mode::Terminal;
    assert!(app.take_query_answers().is_empty());

    app.settings.terminal_mode.answer_groups = vec![QueryGroup::Status, QueryGroup::Size];
    assert!(focus(&mut app, true).is_empty());
    app.settings.terminal_mode.answer_groups = QueryGroup::ALL.to_vec();

    app.feed_rx(b"\x1b[?1004l");
    assert!(focus(&mut app, false).is_empty());
}

/// MAX-159: a restored session that turned mode 2048 on doesn't leave it on: a resize sends
/// nothing and DECRQM says off, until the live app turns it on again. The same for the focus
/// reports, mode 1004 (MAX-277).
/// How: a session holding `?2048h` and `?1004h`, restored, resized to 30x100, DECRQM 2048 and
/// 1004; then `h` live.
#[test]
fn restored_size_reports_off() {
    let base = std::env::temp_dir().join(format!("maxicom_test_{}_reports", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let session = crate::capture::tests::write_test_session(
        &base,
        "/dev/ttyUSB0",
        &CaptureFormat::default(),
        b"nvim\r\n\x1b[?2048h\x1b[?1004h",
        b"",
    );
    let mut app = app();
    app.settings.mode = Mode::Terminal;
    app.restore_session(&session).unwrap();
    app.terminal_mode.set_vt100_size(30, 100);
    assert!(app.take_query_answers().is_empty());
    app.feed_rx(b"\x1b[?2048$p\x1b[?1004$p");
    assert_eq!(app.take_query_answers(), b"\x1b[?2048;2$y\x1b[?1004;2$y");

    app.feed_rx(b"\x1b[?2048h");
    assert!(is_size_report(&app.take_query_answers(), 30, 100));
    let _ = std::fs::remove_dir_all(&base);
}

/// MAX-151: the queries of a restored session, and the ones held while the restore dialog
/// asked, are not answered; what comes after the answer is.
/// How: a session ending in a DSR, restored; then DSR held by the dialog, answered `n`.
#[test]
fn restored_and_held_queries_unanswered() {
    let base = std::env::temp_dir().join(format!("maxicom_test_{}_answers", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let session = crate::capture::tests::write_test_session(
        &base,
        "/dev/ttyUSB0",
        &CaptureFormat::default(),
        b"old\r\n\x1b[5n",
        b"",
    );
    let mut app = app();
    app.settings.mode = Mode::Terminal;
    app.restore_session(&session).unwrap();
    app.feed_rx(b"x");
    assert!(app.take_query_answers().is_empty());
    let _ = std::fs::remove_dir_all(&base);

    let (mut app, dir) = asking_app("ask_answers");
    app.settings.mode = Mode::Terminal;
    app.feed_rx(DSR);
    let no = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('n'),
        crossterm::event::KeyModifiers::NONE,
    );
    app.update(Action::Overlay(DialogAction::HandleKey(no)));
    assert!(app.overlay().is_none());
    assert!(app.take_query_answers().is_empty());
    app.feed_rx(DSR);
    assert_eq!(app.take_query_answers(), DSR_READY);
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-164: `mark`, `jump-mark` and `delete-mark` take a mark's letter from the command line,
/// a toast saying what happened; `mark` and `jump-mark` without one wait for it as the next
/// key. A 27th mark is refused, Enter (the first free letter) then says so.
/// How: two lines in line mode, then command lines; then all 26 letters marked.
#[test]
fn mark_commands() {
    let mut app = app();
    app.settings.mode = Mode::Line;
    app.feed_rx(b"one\r\ntwo\r\n");
    let run_line = |app: &mut App, line: &str| {
        app.update(KeyBindingAction::CommandLine.to_action(app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
        while let Some((_, action)) = app.next_command_action(Instant::now()) {
            app.update(action);
        }
        let toasts = app.notifications.active_toasts();
        toasts.last().map(|toast| toast.message.clone()).unwrap()
    };
    assert_eq!(run_line(&mut app, "mark k"), "mark k");
    assert_eq!(app.line_mode.marks().get('k'), Some(1));
    assert_eq!(run_line(&mut app, "jump-mark q"), "mark q is not set");
    assert_eq!(run_line(&mut app, "delete-mark k"), "mark k deleted");
    assert_eq!(run_line(&mut app, "delete-mark k"), "mark k is not set");
    assert!(app.line_mode.marks().sorted().is_empty());
    run_line(&mut app, "mark");
    assert_eq!(app.pending_mark(), Some(PendingMark::Add));
    run_line(&mut app, "jump-mark");
    assert_eq!(app.pending_mark(), Some(PendingMark::Jump));
    app.update(Action::App(AppAction::Mark(MarkAction::Cancel)));

    let all: Vec<String> = ('a'..='z').map(|letter| format!("mark {letter}")).collect();
    run_line(&mut app, &all.join("; "));
    assert_eq!(app.line_mode.marks().first_free(), None);
    app.update(Action::App(AppAction::Mark(MarkAction::Add(None))));
    let toasts = app.notifications.active_toasts();
    assert_eq!(
        toasts.last().unwrap().message,
        "all 26 marks are set: delete one first"
    );
}

/// MAX-164: a jump's flash is its mode's only, and goes once its time is up: the tick stops
/// asking for frames.
/// How: a flash set on line mode's line 3, read from both modes, then one already over.
#[test]
fn mark_flash_ends() {
    let mut app = app();
    app.mark_flash = Some((Mode::Line, 3, Instant::now() + MARK_FLASH));
    assert_eq!(app.mark_flash(Mode::Terminal), None);
    assert_eq!(app.mark_flash(Mode::Line), Some(3));

    app.mark_flash = Some((Mode::Line, 3, Instant::now() - Duration::from_millis(1)));
    assert_eq!(app.mark_flash(Mode::Line), None);
    assert!(app.mark_flash.is_none());
    assert!(!app.tick());
}

/// MAX-215: `n` past the last match goes on from the top with a toast, `N` past the first
/// one from the end; without a match it says there is nowhere to go.
/// How: "hit" on lines 0 and 2 in line mode, a backward search confirmed on line 2, then
/// N, N, n, n (a wrap each way), then a search without a match.
#[test]
fn repeated_search_says_when_it_wraps() {
    let mut app = app();
    app.settings.mode = Mode::Line;
    app.feed_rx(b"hit\nb\nhit\n");
    let last_toast = |app: &App| {
        app.notifications
            .active_toasts()
            .last()
            .unwrap()
            .message
            .clone()
    };
    let count = |app: &App| app.notifications.active_toasts().len();
    app.update(Action::Search(SearchModeAction::Start(
        SearchDirection::Backward,
    )));
    app.update(Action::Search(SearchModeAction::Paste("hit".to_string())));
    app.update(Action::Search(SearchModeAction::Confirm));
    assert!(app.is_copy_mode_active());

    let before = count(&app);
    app.update(Action::Search(SearchModeAction::Repeat(true)));
    assert_eq!(last_toast(&app), "Search hit the end, went on from the top");
    app.update(Action::Search(SearchModeAction::Repeat(true)));
    app.update(Action::Search(SearchModeAction::Repeat(false)));
    assert_eq!(count(&app), before + 1);
    app.update(Action::Search(SearchModeAction::Repeat(false)));
    assert_eq!(last_toast(&app), "Search hit the top, went on from the end");

    app.update(Action::Search(SearchModeAction::Query("zzz".to_string())));
    app.update(Action::Search(SearchModeAction::Confirm));
    app.update(Action::Search(SearchModeAction::Repeat(false)));
    assert_eq!(last_toast(&app), "No match to go to");
}

/// MAX-215: a search from out of copy mode starts it: `?` from the live view finds the
/// newest line's match too; Esc closes the bar and copy mode, back to the live view; Enter
/// stays in copy mode on the match. From copy mode, Esc stays in it.
/// How: "hit" on lines 0 and 2 (the last), `?` then Esc, `?` then Enter, `/` then Esc.
#[test]
fn a_search_runs_in_copy_mode() {
    let mut app = app();
    app.settings.mode = Mode::Line;
    app.feed_rx(b"hit\nb\nhit\n");
    let start = |app: &mut App, direction| {
        app.line_mode.search_mut().query = tui_input::Input::default();
        app.update(Action::Search(SearchModeAction::Start(direction)));
        app.update(Action::Search(SearchModeAction::Paste("hit".to_string())));
    };
    let cursor = |app: &App| app.line_mode.copy_state().cursor.line_idx;

    start(&mut app, SearchDirection::Backward);
    assert!(app.is_copy_mode_active() && app.is_search_active());
    assert_eq!(cursor(&app), 2);
    app.update(Action::Search(SearchModeAction::Cancel));
    assert!(!app.is_copy_mode_active() && !app.is_search_active());
    assert!(app.scroll_status().is_bottom());

    start(&mut app, SearchDirection::Backward);
    app.update(Action::Search(SearchModeAction::Confirm));
    assert!(app.is_copy_mode_active());
    assert_eq!(cursor(&app), 2);

    start(&mut app, SearchDirection::Forward);
    assert_eq!(cursor(&app), 0);
    app.update(Action::Search(SearchModeAction::Cancel));
    assert!(app.is_copy_mode_active());
    assert_eq!(cursor(&app), 2);
}

/// MAX-218: the searches Enter ran are kept once each in a row, and Up / Down in the bar
/// walk them, their matches found.
/// How: "hit" then "b" then "b" again searched, then Up twice in a new bar.
#[test]
fn search_history_in_the_bar() {
    let mut app = app();
    app.settings.mode = Mode::Line;
    app.feed_rx(b"hit\nb\nhit\n");
    let search = |app: &mut App, query: &str| {
        app.update(Action::Search(SearchModeAction::Start(
            SearchDirection::Forward,
        )));
        app.update(Action::Search(SearchModeAction::Paste(query.to_string())));
        app.update(Action::Search(SearchModeAction::Confirm));
    };
    search(&mut app, "hit");
    search(&mut app, "b");
    search(&mut app, "b");
    assert_eq!(app.search_history, ["hit", "b"]);

    app.update(Action::Search(SearchModeAction::Start(
        SearchDirection::Forward,
    )));
    app.update(Action::Search(SearchModeAction::History(true)));
    assert_eq!(app.line_mode.search().query_str(), "b");
    app.update(Action::Search(SearchModeAction::History(true)));
    assert_eq!(app.line_mode.search().query_str(), "hit");
    assert_eq!(app.line_mode.search().matches_count(), 2);
}

/// MAX-228: a toast stays the time to read it: 2 s plus 50 ms a character, a warning at
/// least 4 s, an error 6 s, none more than 10 s.
#[test]
fn toast_duration_from_its_length() {
    use crate::notification::ToastLevel;
    let ms = |level, message: &str| toast_duration(level, message).as_millis();
    assert_eq!(ms(ToastLevel::Success, "Copied"), 2_300);
    // a received file with its path: about 4 s
    let received = "Received /tmp/maxicom-dl/rand-129.bin (129 bytes)";
    let len = received.chars().count() as u128;
    assert_eq!(ms(ToastLevel::Success, received), 2_000 + 50 * len);
    assert_eq!(ms(ToastLevel::Warn, "TCP connection lost"), 4_000);
    assert_eq!(ms(ToastLevel::Error, "break: busy"), 6_000);
    assert_eq!(ms(ToastLevel::Info, &"x".repeat(500)), 10_000);
}

/// MAX-228: a download says each file by name as it comes, then the folder once at the end:
/// the file and its size for one, how many and their bytes for a batch, in the transfer's
/// box.
/// How: a YMODEM batch of two files fed as packets, then the empty block 0.
#[test]
fn download_summary() {
    use crate::transfer::{FileTransfer, modem::Protocol, modem::packet};
    let mut app = app();
    let folder = std::env::temp_dir().join(format!("maxicom_test_{}_summary", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let now = Instant::now();
    let transfer = FileTransfer::download(Protocol::Ymodem, folder.clone(), None, now);
    app.update(Action::App(AppAction::StartTransfer(Box::new(transfer))));
    let block0 = |info: &[u8]| {
        let mut data = info.to_vec();
        data.resize(128, 0);
        packet(0, &data, true)
    };
    let transfer_toast = |app: &App| {
        let toasts = app.notifications.active_toasts();
        let toast = toasts.iter().find(|toast| toast.key == Some("transfer"));
        toast.map(|toast| toast.message.clone()).unwrap_or_default()
    };
    app.transfer_step(b"", now);
    for (name, size) in [("a.bin", 100), ("b.bin", 20)] {
        app.transfer_step(&block0(format!("{name}\0{size}\0").as_bytes()), now);
        app.transfer_step(&packet(1, &[7; 128], true), now);
        app.transfer_step(&[0x04], now);
        app.transfer_step(&[0x04], now);
        assert_eq!(
            transfer_toast(&app),
            format!("Received {name} ({size} bytes)")
        );
    }
    app.transfer_step(&block0(b""), now);
    assert!(app.transfer().is_none());
    assert_eq!(
        transfer_toast(&app),
        format!("Received 2 files (120 bytes) into {}", folder.display())
    );
    let _ = std::fs::remove_dir_all(&folder);
}

/// MAX-228: a cancelled download says "Cancelled", with the partial file it keeps, in the
/// transfer's box; a failure says "Stopped" and why.
/// How: a YMODEM download cancelled in its file, then one cancelled before any file.
#[test]
fn cancel_texts() {
    use crate::transfer::{FileTransfer, modem::Protocol, modem::packet};
    let mut app = app();
    let folder = std::env::temp_dir().join(format!("maxicom_test_{}_cancel", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let now = Instant::now();
    let transfer_toast = |app: &App| {
        let toasts = app.notifications.active_toasts();
        let toast = toasts.iter().find(|toast| toast.key == Some("transfer"));
        toast.map(|toast| toast.message.clone()).unwrap_or_default()
    };
    let start = |app: &mut App| {
        let transfer = FileTransfer::download(Protocol::Ymodem, folder.clone(), None, now);
        app.update(Action::App(AppAction::StartTransfer(Box::new(transfer))));
        app.transfer_step(b"", now);
    };

    start(&mut app);
    let mut header = b"a.bin\x00500\x00".to_vec();
    header.resize(128, 0);
    app.transfer_step(&packet(0, &header, true), now);
    app.transfer_step(&packet(1, &[7; 128], true), now);
    app.update(KeyBindingAction::CancelCommands.to_action(&app));
    let part = folder.join("a.bin.part");
    assert_eq!(
        transfer_toast(&app),
        format!("Cancelled: a.bin, kept as {}", part.display())
    );

    start(&mut app);
    app.update(KeyBindingAction::CancelCommands.to_action(&app));
    assert_eq!(transfer_toast(&app), "Cancelled: the download");
    let _ = std::fs::remove_dir_all(&folder);
}

/// MAX-228: `notify <level> <text>` shows a notification of the user's at that level, said
/// again it counts; an empty text is refused. Nothing goes to the device.
#[test]
fn command_line_notify() {
    use crate::notification::ToastLevel;
    let mut app = app();
    let run_line = |app: &mut App, line: &str| -> Vec<UpdateEffect> {
        app.update(KeyBindingAction::CommandLine.to_action(app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
        let mut effects = Vec::new();
        while let Some((_, action)) = app.next_command_action(Instant::now()) {
            effects.push(app.update(action));
        }
        effects
    };
    let shown = |app: &App| -> Vec<(String, ToastLevel, u32)> {
        let toasts = app.notifications.active_toasts();
        toasts
            .iter()
            .map(|t| (t.message.clone(), t.level, t.count))
            .collect()
    };

    let effects = run_line(&mut app, "notify warn \"power-cycle the board\"");
    assert!(matches!(&effects[..], [UpdateEffect::None]));
    assert_eq!(
        shown(&app),
        [("power-cycle the board".to_string(), ToastLevel::Warn, 1)]
    );
    run_line(
        &mut app,
        "notify warn 'power-cycle the board'; notify success \"done\\tok\"",
    );
    assert_eq!(
        shown(&app),
        [
            ("power-cycle the board".to_string(), ToastLevel::Warn, 2),
            ("done\tok".to_string(), ToastLevel::Success, 1),
        ]
    );

    app.notifications.dismiss_all();
    run_line(&mut app, "notify error \" \"");
    let refused = app
        .notifications
        .active_toasts()
        .last()
        .map(|t| t.message.clone());
    assert_eq!(refused.as_deref(), Some("notify: nothing to say"));
}

/// MAX-228: a trigger can run `notify`: the default file's `panic` one, silent, gives the
/// error notification only, and sends nothing; without `silent`, its own notice comes too.
/// How: the default triggers, `panic` armed, then the same rule not silent.
#[test]
fn trigger_runs_notify() {
    use crate::notification::ToastLevel;
    let fire = |app: &mut App| {
        app.feed_rx(b"[   12.345] Kernel panic - not syncing");
        let mut effects = Vec::new();
        while let Some((_, action)) = app.next_command_action(Instant::now()) {
            effects.push(app.update(action));
        }
        effects
    };
    let shown = |app: &App| -> Vec<(String, ToastLevel)> {
        let toasts = app.notifications.active_toasts();
        toasts
            .iter()
            .map(|t| (t.message.clone(), t.level))
            .collect()
    };

    let mut app = app();
    app.set_triggers(crate::configs::TriggersConfig::default().compile().unwrap());
    app.update(Action::App(AppAction::QueueCommands(
        crate::commands::parse("trigger on panic").unwrap(),
    )));
    while let Some((_, action)) = app.next_command_action(Instant::now()) {
        app.update(action);
    }
    app.notifications.dismiss_all();
    assert!(matches!(&fire(&mut app)[..], [UpdateEffect::None]));
    let panicked = ("the board panicked".to_string(), ToastLevel::Error);
    assert_eq!(shown(&app), std::slice::from_ref(&panicked));

    let config: crate::configs::TriggersConfig = toml::from_str(
        r#"
            [panic]
            match = "Kernel panic"
            run = 'notify error "the board panicked"'
            armed = true
        "#,
    )
    .unwrap();
    let mut app = self::app();
    app.set_triggers(config.compile().unwrap());
    fire(&mut app);
    let notice = (
        "Trigger panic: notify error \"the board panicked\"".to_string(),
        ToastLevel::Info,
    );
    assert_eq!(shown(&app), [notice, panicked]);
}

/// MAX-9 step 0: lines holding code addresses (a backtrace, a register dump) are history
/// lines as any other, one entry per received line and nothing added after them, live or
/// replayed.
/// How: an ESP32 panic's register and backtrace lines, and a Cortex-M one with Thumb
/// addresses, fed to the app.
#[test]
fn address_lines_add_no_line() {
    let mut app = app();
    let wire = "Guru Meditation Error: Core  0 panic'ed (LoadProhibited)\r\n\
                PC      : 0x400d1234  PS      : 0x00060630  A0      : 0x800d5678\r\n\
                Backtrace: 0x400d1234:0x3ffb1230 0x400d5678:0x3ffb1250\r\n\
                HardFault: PC=0x08001235 LR=0x080004a1\r\n";
    app.feed_rx(wire.as_bytes());
    let lines: Vec<_> = wire.split("\r\n").filter(|l| !l.is_empty()).collect();
    assert_eq!(texts(&app), lines);
}

/// MAX-270 step 0: line mode's local echo shows a submitted line and raw bytes sent (send-hex,
/// mash) in the history, nothing when off. The terminal mode echo (step 2) must not change it.
/// How: echo on, "hello" submitted and "raw" sent; echo off, "quiet" and "bytes"
#[test]
fn line_mode_local_echo() {
    let mut app = app();
    app.settings.mode = Mode::Line;
    app.settings.local_echo = true;
    let send = |app: &mut App, line: &str| {
        app.update(Action::LineInput(LineInputAction::Paste(line.to_string())));
        app.update(Action::LineInput(LineInputAction::Submit))
    };
    assert!(matches!(send(&mut app, "hello"), UpdateEffect::Transmit(_)));
    app.update(Action::TransmitRaw(b"raw".to_vec()));
    assert_eq!(texts(&app), ["hello", "raw"]);

    app.settings.local_echo = false;
    send(&mut app, "quiet");
    app.update(Action::TransmitRaw(b"bytes".to_vec()));
    assert_eq!(texts(&app), ["hello", "raw"]);
}

/// MAX-255 step 0: in line mode Up and Down walk the lines sent from maxicom only and send
/// nothing, even when the device looks like a shell; the draft comes back past the newest.
/// How: "ls" and "pwd" sent, a shell prompt received (bracketed paste on, DSR query, OSC 133),
/// a draft typed, then Up x3 and Down x2
#[test]
fn line_mode_history_stays_local() {
    let mut app = app();
    app.settings.mode = Mode::Line;
    let line = |app: &mut App, action: LineInputAction| app.update(Action::LineInput(action));
    for sent in ["ls", "pwd"] {
        line(&mut app, LineInputAction::Paste(sent.to_string()));
        line(&mut app, LineInputAction::Submit);
    }
    app.feed_rx(b"\x1b]133;A\x07\x1b[?2004h\x1b[5nroot@board:~# ");
    assert!(app.take_query_answers().is_empty());
    line(&mut app, LineInputAction::Paste("draft".to_string()));

    let steps = [
        (LineInputAction::HistoryUp, "pwd"),
        (LineInputAction::HistoryUp, "ls"),
        (LineInputAction::HistoryUp, "ls"),
        (LineInputAction::HistoryDown, "pwd"),
        (LineInputAction::HistoryDown, "draft"),
    ];
    for (action, expected) in steps {
        assert!(matches!(line(&mut app, action), UpdateEffect::None));
        assert_eq!(app.line_mode.input().value(), expected);
    }
    assert!(app.take_query_answers().is_empty());
}

/// MAX-255: in line mode the terminal fed with `always_both` sees the device is a shell; with
/// `active_only` it isn't fed, nothing is seen. A device switch's fresh screens forget it.
/// How: a DSR query in line mode with each buffering, then the views cleared
#[test]
fn shell_signal_seen_in_line_mode() {
    use crate::app::terminal_mode::ShellSignal;
    let mut app = app();
    app.settings.mode = Mode::Line;
    app.feed_rx(b"\x1b[5n");
    assert_eq!(app.terminal_mode.shell_signal(), Some(ShellSignal::Query));
    app.clear_screen();
    assert_eq!(app.terminal_mode.shell_signal(), None);

    app.settings.dual_mode_buffering = DualBufferMode::ActiveOnly;
    app.feed_rx(b"\x1b[5n");
    assert_eq!(app.terminal_mode.shell_signal(), None);
}

/// The messages of the shell hint shown (MAX-255)
fn shell_hints(app: &App) -> Vec<String> {
    let toasts = app.notifications.active_toasts().iter();
    toasts
        .filter(|toast| toast.key == Some("shell"))
        .map(|toast| toast.message.clone())
        .collect()
}

fn line_key(app: &mut App, code: KeyCode) {
    use crossterm::event::{KeyEvent, KeyModifiers};
    let key = KeyEvent::new(code, KeyModifiers::NONE);
    app.update(Action::LineInput(LineInputAction::HandleKey(key)));
}

/// MAX-255: in line mode a query shows the shell hint at once, naming it and the key that
/// switches mode; once a link: dismissed, another query doesn't bring it back, a device
/// switch's fresh screens do. In terminal mode a query shows nothing.
/// How: DSR in terminal mode, then twice in line mode with the hints dismissed, then the
/// views cleared and DSR again
#[test]
fn shell_hint_on_a_query() {
    let mut app = app();
    app.settings.mode = Mode::Terminal;
    app.feed_rx(b"\x1b[5n");
    assert_eq!(shell_hints(&app), Vec::<String>::new());

    app.settings.mode = Mode::Line;
    app.feed_rx(b"\x1b[5n");
    let key = app.key_resolver.action_keys(KeyBindingAction::SKInputMode)[0].clone();
    let hint = format!(
        "The device looks like a shell (it sent a terminal query): {key} switches to terminal \
         mode for its line editing and history"
    );
    assert_eq!(shell_hints(&app), std::slice::from_ref(&hint));
    app.notifications.dismiss_all();
    app.feed_rx(b"\x1b[5n");
    assert_eq!(shell_hints(&app), Vec::<String>::new());

    app.clear_screen();
    app.feed_rx(b"\x1b[5n");
    assert_eq!(shell_hints(&app), [hint]);
}

/// MAX-255: a weaker sign shows the hint only when Up or Down is pressed in line mode, naming
/// it; with no sign Up and Down show nothing. Tab shows its own hint, sign or not.
/// How: Up, Down and a char with no sign, then bracketed paste received and Down; a fresh app
/// with Tab and no sign, one with prompt marks then Tab
#[test]
fn shell_hint_on_up_down_and_tab() {
    let mut app = app();
    app.settings.mode = Mode::Line;
    app.update(Action::LineInput(LineInputAction::HistoryUp));
    app.update(Action::LineInput(LineInputAction::HistoryDown));
    line_key(&mut app, KeyCode::Char('x'));
    app.feed_rx(b"\x1b[?2004h$ ");
    assert_eq!(shell_hints(&app), Vec::<String>::new());
    app.update(Action::LineInput(LineInputAction::HistoryDown));
    let key = app.key_resolver.action_keys(KeyBindingAction::SKInputMode)[0].clone();
    let hint = format!(
        "The device shell keeps its own history (it turned bracketed paste on): {key} switches \
         to terminal mode to use it"
    );
    assert_eq!(shell_hints(&app), [hint]);

    let mut app = super::tests::app();
    app.settings.mode = Mode::Line;
    line_key(&mut app, KeyCode::Tab);
    let key = app.key_resolver.action_keys(KeyBindingAction::SKInputMode)[0].clone();
    let hint = format!("Tab completion is the device shell's: {key} switches to terminal mode");
    assert_eq!(shell_hints(&app), std::slice::from_ref(&hint));

    let mut app = super::tests::app();
    app.settings.mode = Mode::Line;
    app.feed_rx(b"\x1b]133;A\x07$ ");
    line_key(&mut app, KeyCode::Tab);
    assert_eq!(shell_hints(&app), [hint]);
}

/// MAX-255: no hint with `shell_hint = false`, nor when terminal mode is turned off, nor on a
/// replay.
/// How: `shell_hint` off with a query and Tab; `disabled_mode = terminal` with Tab; a replay
/// in line mode receiving a query
#[test]
fn shell_hint_needs_terminal_mode_and_a_live_link() {
    let mut app = app();
    app.settings.mode = Mode::Line;
    app.settings.line_mode.shell_hint = false;
    app.feed_rx(b"\x1b[5n");
    line_key(&mut app, KeyCode::Tab);
    assert_eq!(shell_hints(&app), Vec::<String>::new());

    let mut app = super::tests::app();
    app.settings.mode = Mode::Line;
    app.settings.disabled_mode = DisabledMode::Terminal;
    line_key(&mut app, KeyCode::Tab);
    assert_eq!(shell_hints(&app), Vec::<String>::new());

    let mut app = super::tests::app();
    app.start_replay(PathBuf::from("/tmp/shell.raw"), None);
    app.settings.mode = Mode::Line;
    app.feed_rx(b"\x1b[5n");
    line_key(&mut app, KeyCode::Tab);
    assert_eq!(shell_hints(&app), Vec::<String>::new());
}

/// MAX-270: in terminal mode, local echo shows what is typed, pasted and sent raw on the
/// screen, as maxicom's own bytes (not in the raw panel's stream): Enter as a new line,
/// Backspace going back so the next char writes over, a paste without its markers; keys sent
/// as escape sequences (arrows) aren't shown. Off, nothing shows.
/// How: "hi" Enter "x" Backspace "Z" Up, a paste "a\nb" with bracketed paste on, raw "AT\r";
/// then the same keys with echo off
#[test]
fn terminal_mode_local_echo() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut quiet = app();
    quiet.settings.mode = Mode::Terminal;
    let mut app = app();
    app.settings.mode = Mode::Terminal;
    app.settings.local_echo = true;
    let key = |app: &mut App, code: KeyCode| {
        let event = KeyEvent::new(code, KeyModifiers::NONE);
        app.update(Action::Terminal(TerminalAction::Key(event)))
    };
    for code in [
        KeyCode::Char('h'),
        KeyCode::Char('i'),
        KeyCode::Enter,
        KeyCode::Char('x'),
        KeyCode::Backspace,
        KeyCode::Char('Z'),
    ] {
        assert!(matches!(key(&mut app, code), UpdateEffect::Transmit(_)));
    }
    match key(&mut app, KeyCode::Up) {
        UpdateEffect::Transmit(bytes) => assert_eq!(bytes, b"\x1b[A"),
        _ => panic!("expected the arrow sent"),
    }
    app.terminal_mode
        .process_rx(TextEncoding::Utf8, b"\x1b[?2004h");
    let paste = Action::Terminal(TerminalAction::Paste("a\nb".to_string()));
    match app.update(paste) {
        UpdateEffect::Transmit(bytes) => assert_eq!(bytes, b"\x1b[200~a\rb\x1b[201~"),
        _ => panic!("expected the paste sent"),
    }
    app.update(Action::TransmitRaw(b"AT\r".to_vec()));
    let screen = app.terminal_mode.vt100_screen();
    assert_eq!(screen.contents(), "hi\nZa\nbAT");
    assert_eq!(screen.cursor_position(), (3, 0));
    assert_eq!(app.terminal_mode.dump().stream().end(), 0);

    key(&mut quiet, KeyCode::Char('h'));
    quiet.update(Action::TransmitRaw(b"AT\r".to_vec()));
    assert_eq!(quiet.terminal_mode.vt100_screen().contents(), "");
}

/// MAX-270: the settings popup's Backspace row switches what the Backspace key sends in
/// terminal mode, DEL by default, and back.
/// How: Backspace, toggle, Backspace, toggle, Backspace
#[test]
fn backspace_setting_toggles_the_byte_sent() {
    use crossterm::event::{KeyCode, KeyEvent};
    let mut app = app();
    app.settings.mode = Mode::Terminal;
    let sent = |app: &mut App| {
        let key = Action::Terminal(TerminalAction::Key(KeyEvent::from(KeyCode::Backspace)));
        match app.update(key) {
            UpdateEffect::Transmit(bytes) => bytes,
            _ => panic!("expected Backspace sent"),
        }
    };
    let toggle = || Action::App(AppAction::ToggleSetting(SettingKey::Backspace));
    assert_eq!(sent(&mut app), b"\x7f");
    app.update(toggle());
    let bs = crate::configs::settings::BackspaceByte::Bs;
    assert_eq!(app.settings.terminal_mode.backspace, bs);
    assert_eq!(sent(&mut app), b"\x08");
    app.update(toggle());
    assert_eq!(sent(&mut app), b"\x7f");
}

/// MAX-269: a copy says what it copied, in the copy's box; with `notify_copy` off nothing shows.
/// How: 5 chars copied, a named copy, then both again with the setting off
#[test]
fn copy_notification_can_be_turned_off() {
    let mut app = app();
    let messages = |app: &App| -> Vec<String> {
        (app.notifications.active_toasts().iter())
            .map(|toast| toast.message.clone())
            .collect()
    };
    app.notify_copied("hello", None);
    assert_eq!(messages(&app), ["Copied 5 chars to clipboard"]);
    app.notify_copied("~/.config/maxicom", Some("config path"));
    assert_eq!(
        messages(&app),
        ["Copied the config path: ~/.config/maxicom"]
    );

    app.notifications.dismiss_all();
    app.settings.ui.notify_copy = false;
    app.notify_copied("hello", None);
    app.notify_copied("~/.config/maxicom", Some("config path"));
    assert!(messages(&app).is_empty());
}

/// What a command line shows once its chain ran through `dispatch`, as one line: the effects
/// left for the loop (`effect Reload`), the popup it opens (`popup Help`), its toasts
/// (`warn [slot] text`), joined by ` | `
fn shown_after(app: &mut App, line: &str) -> String {
    app.update(KeyBindingAction::CommandLine.to_action(app));
    app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
    app.update(Action::Overlay(DialogAction::Confirm));
    let mut shown = Vec::new();
    while let Some((origin, action)) = app.next_command_action(Instant::now()) {
        let effect = match app.dispatch_from(&origin, action) {
            UpdateEffect::None => continue,
            UpdateEffect::Quit => "Quit",
            UpdateEffect::CopyToClipboard(_) => "Copy",
            UpdateEffect::CopyToClipboardAs { .. } => "CopyAs",
            UpdateEffect::ReloadConfig => "Reload",
            UpdateEffect::Redraw => "Redraw",
            UpdateEffect::LayProfiles(_) => "LayProfiles",
            _ => "link (not applied by dispatch)",
        };
        shown.push(format!("effect {effect}"));
    }
    if let Some(overlay) = app.overlay() {
        let debug = format!("{overlay:?}");
        let name = debug.split(['(', ' ', '{']).next().unwrap_or_default();
        shown.push(format!("popup {name}"));
    }
    for toast in app.notifications.active_toasts() {
        let level = format!("{:?}", toast.level).to_lowercase();
        let slot = toast.key.map(|key| format!(" [{key}]")).unwrap_or_default();
        shown.push(format!("{level}{slot} {}", toast.message));
    }
    shown.join(" | ")
}

/// MAX-165 (A1 step 0): what every command shows today when it runs alone from the command
/// line on a live link: its toasts (level, slot, text), the popup it opens, the effects left
/// for the loop. The explicit results of A1 step 3 move every `notify_*` behind the command's
/// result: this table must not change then. A new command needs its row.
/// How: each line on a fresh app, the files it writes in a temporary folder.
#[test]
fn every_command_shows_what_it_did() {
    let dir = std::env::temp_dir().join(format!("maxicom_test_{}_shown", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // forward slashes: a backslash escapes on the command line (Windows paths)
    let slashed = |path: &Path| path.display().to_string().replace('\\', "/");
    let file = dir.join("f.bin");
    std::fs::write(&file, b"ab").unwrap();
    let file = slashed(&file);
    let raw = dir.join("r.raw");
    std::fs::write(&raw, b"ab").unwrap();
    let raw = slashed(&raw);
    let shot = slashed(&dir.join("s"));

    let mut rows: Vec<(String, String)> = [
        ("quit", "popup ConfirmQuit"),
        ("toggle-input-mode", ""),
        ("toggle-local-echo", ""),
        ("toggle-line-wrap", "warn Line wrap: line mode only"),
        ("toggle-add-cr", "info [setting:add-cr] Add CR On"),
        ("toggle-add-lf", "info [setting:add-lf] Add LF On"),
        ("toggle-display-timestamp", ""),
        (
            "toggle-text-encoding",
            "info [setting:encoding] Rx Text Encoding is ASCII",
        ),
        ("toggle-indent-wrap", ""),
        (
            "toggle-display-mode",
            "info [setting:display] Display Mode is Clean",
        ),
        (
            "toggle-display-raw",
            "info [setting:raw] Raw SideView is Hex",
        ),
        ("screenshot-bytes", "popup Screenshot"),
        ("screenshot-text", "popup Screenshot"),
        ("help", "popup Help"),
        ("settings", "popup Settings"),
        ("record", "popup FilePath"),
        ("resize-dialog", "popup TerminalSize"),
        ("auto-resize-remote", ""),
        ("auto-resize-remote-line", ""),
        ("auto-resize-emulator", ""),
        ("link-settings", "popup LinkSettings"),
        ("enter-copy-mode", ""),
        ("enter-search", ""),
        ("enter-search-backward", ""),
        ("scroll-up", ""),
        ("scroll-down", ""),
        ("scroll-to-bottom", ""),
        ("scroll-to-top", ""),
        ("page-up", ""),
        ("page-down", ""),
        ("pan-right", ""),
        ("pan-left", ""),
        ("pan-home", ""),
        ("pan-end", ""),
        ("pan-side-right", ""),
        ("pan-side-left", ""),
        ("scroll-side-up", ""),
        ("scroll-side-down", ""),
        ("page-side-up", ""),
        ("page-side-down", ""),
        ("side-grow", "warn SideView is Off"),
        ("side-shrink", "warn SideView is Off"),
        ("side-grow-5", "warn SideView is Off"),
        ("side-shrink-5", "warn SideView is Off"),
        ("clear", ""),
        ("clear-history", ""),
        ("sessions", "warn No session folder"),
        ("directory", "popup Directory"),
        ("exit-prefix", ""),
        (
            "toggle-attach",
            "popup DeviceDetached | warn [link] Serial port detached",
        ),
        ("reload-config", "effect Reload"),
        ("redraw", "effect Redraw"),
        ("toggle-mouse", "info [setting:mouse] Mouse capture Off"),
        ("command-line", "popup CommandLine"),
        ("cancel", "info Nothing to cancel"),
        ("toggle-triggers", "warn triggers.toml has no trigger"),
        ("triggers", "popup Triggers"),
        ("profiles", "popup Profiles"),
        ("plot-freeze", "warn [plot] No plot shown"),
        ("plot-next", "info [plot] Plot key-value"),
        ("plot-prev", "info [plot] Plot tagged"),
        ("dismiss-notification", ""),
        ("dismiss-notifications", ""),
        ("notifications", "popup Notices"),
        ("marks", "popup Marks"),
        ("prev-mark", "info no mark above"),
        ("next-mark", "info no mark below"),
        // waits for its letter as the next key, no popup
        ("mark", ""),
        ("jump-mark", ""),
        ("delete-mark a", "warn mark a is not set"),
        ("send x", ""),
        ("send-hex 41", ""),
        ("download xmodem", ""),
        ("open /dev/ttyUSB9", "info [link] Opening /dev/ttyUSB9"),
        ("link-save", "success Saved link 'ttyUSB0'"),
        (
            "link-delete nope",
            "error link-delete: no saved link 'nope'",
        ),
        ("session-name", "error session-name: no session running"),
        ("replay-stop", "error replay-stop: no replay runs"),
        ("replay-pause", "info Replay paused"),
        (
            "replay-resume",
            "error replay-resume: the replay isn't paused",
        ),
        ("replay-speed 10", "info Replay delay 10 ms"),
        ("replay-restart", "info Replay restarted"),
        ("sleep 1", ""),
        ("dtr on", "error DTR on: the link has no control lines"),
        ("rts on", "error RTS on: the link has no control lines"),
        (
            "break",
            "error break of 250 ms: the link has no control lines",
        ),
        ("baud 9600", ""),
        ("framing 8N1", ""),
        ("set local-echo on", ""),
        ("mode terminal", ""),
        (
            "encoding latin1",
            "info [setting:encoding] Rx Text Encoding is Latin1",
        ),
        ("notify info hi", "info hi"),
        ("search x", ""),
        ("mash x y", "info Mashing until 'y'"),
        ("trigger on", "error trigger: triggers.toml has no trigger"),
        ("plot", "info [plot] Plot key-value"),
        ("profile", "info [profiles] No profile laid"),
        ("flow hw", ""),
    ]
    .into_iter()
    .map(|(line, shown)| (line.to_string(), shown.to_string()))
    .collect();
    rows.extend([
        (format!("send-file {file}"), String::new()),
        (format!("upload xmodem {file}"), String::new()),
        (format!("replay {raw}"), format!("info Replaying {raw}")),
        (
            format!("screenshot {shot}"),
            format!("success Screenshot saved to {shot}.txt"),
        ),
    ]);

    for spec in crate::commands::COMMANDS {
        let has_row = rows
            .iter()
            .any(|(line, _)| line.split(' ').next() == Some(spec.name));
        assert!(has_row, "no row for '{}'", spec.name);
    }
    for (line, expected) in &rows {
        let mut app = app();
        app.settings.links_path = Some(dir.join("links.toml"));
        app.settings.plots_path = Some(dir.join("plots.toml"));
        assert_eq!(&shown_after(&mut app, line), expected, "{line}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
