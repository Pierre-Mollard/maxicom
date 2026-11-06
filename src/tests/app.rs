//! App-level wiring of the viewport (#35) and of a config reload (MAX-112).

use super::*;
use crate::{
    app::capture::tests::asking_app,
    capture::{CaptureFormat, RecordingMeta},
    configs::KeyBindingsConfig,
    inputs::DialogAction,
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

/// MAX-12: the command line runs its chain one action at a time, in order, each effect
/// returned for the loop to apply, and closes; a line that doesn't parse runs nothing,
/// warns and stays open.
/// How: a chain toggling a setting then giving two effects, then a chain with a typo.
#[test]
fn command_line_runs_its_chain_in_order() {
    let mut app = app();
    let run_line = |app: &mut App, line: &str| {
        app.update(KeyBindingAction::CommandLine.to_action(app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm))
    };
    let echo = app.settings.local_echo;
    let effect = run_line(&mut app, "toggle-local-echo; reload-config; toggle-attach");
    assert!(matches!(effect, UpdateEffect::None));
    assert!(app.overlay().is_none());
    let mut effects = Vec::new();
    while let Some(action) = app.next_command_action(Instant::now()) {
        effects.push(app.update(action));
    }
    assert!(matches!(
        effects[..],
        [
            UpdateEffect::None,
            UpdateEffect::ReloadConfig,
            UpdateEffect::ToggleAttach
        ]
    ));
    assert_ne!(app.settings.local_echo, echo);

    run_line(&mut app, "toggle-local-echo; nope");
    assert!(matches!(app.overlay(), Some(Overlay::CommandLine(_))));
    assert!(app.next_command_action(Instant::now()).is_none());
    assert_ne!(app.settings.local_echo, echo);
    let toast = app.notifications.active_toasts().last().unwrap();
    assert_eq!(toast.message, "unknown command 'nope'");
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
        while let Some(action) = app.next_command_action(Instant::now()) {
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
        while let Some(action) = app.next_command_action(Instant::now()) {
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
        while let Some(action) = app.next_command_action(Instant::now()) {
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

/// MAX-6: the toggle arms every trigger when none is armed, else disarms them all; the
/// `trigger` command arms, disarms or toggles all or one by name, refuses a name that
/// isn't one, and each change says what is armed. With no trigger at all, both say so.
/// How: the default triggers (`login`, `u-boot`), then none.
#[test]
fn arming_triggers() {
    let mut app = app();
    app.set_triggers(crate::configs::TriggersConfig::default().compile().unwrap());
    let last_toast = |app: &App| {
        let toasts = app.notifications.active_toasts();
        toasts.last().unwrap().message.clone()
    };
    let run = |app: &mut App, line: &str| {
        app.update(Action::App(AppAction::QueueCommands(
            crate::commands::parse(line).unwrap(),
        )));
        while let Some(action) = app.next_command_action(Instant::now()) {
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
        while let Some(action) = app.next_command_action(now) {
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
        while let Some(action) = app.next_command_action(Instant::now()) {
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
        while let Some(action) = app.next_command_action(Instant::now()) {
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
/// come from `next_transfer_chunk` until the end of the file, with a notice. A second
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
        while let Some(action) = app.next_command_action(Instant::now()) {
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
        if let Some(chunk) = app.next_transfer_chunk(start + Duration::from_secs(step)) {
            assert!(chunk.len() <= 96);
            sent += chunk.len();
        }
    }
    assert_eq!(sent, 300);
    assert!(app.transfer().is_none());
    assert!(
        last_toast(&app).starts_with("Sent maxicom_test_"),
        "{}",
        last_toast(&app)
    );

    run_line(&mut app, &format!("send-file {file}"));
    app.next_transfer_chunk(start + Duration::from_secs(1));
    run_line(&mut app, "cancel");
    assert!(app.transfer().is_none());
    assert!(
        last_toast(&app).ends_with(" at 32%"),
        "{}",
        last_toast(&app)
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
        while let Some(action) = app.next_command_action(Instant::now()) {
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

/// MAX-12: `sleep` pauses the rest of the chain, which goes on once its time is up; the
/// pause the loop sets for a break does the same. A failure drops a paused chain, pause
/// and all.
/// How: the actions taken one by one, at chosen times.
#[test]
fn command_line_sleep() {
    let mut app = app();
    let queue_line = |app: &mut App, line: &str| {
        app.update(KeyBindingAction::CommandLine.to_action(app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
    };
    let sent = |app: &mut App, action: Option<Action>| match app.update(action.unwrap()) {
        UpdateEffect::Transmit(bytes) => bytes,
        _ => panic!("expected a transmit"),
    };

    queue_line(&mut app, "send a; sleep 100; send b");
    let now = Instant::now();
    let action = app.next_command_action(now);
    assert_eq!(sent(&mut app, action), b"a");
    let sleep = app.next_command_action(now).unwrap();
    app.update(sleep);
    assert!(app.next_command_action(Instant::now()).is_none());
    let later = Instant::now() + Duration::from_millis(150);
    let action = app.next_command_action(later);
    assert_eq!(sent(&mut app, action), b"b");
    assert!(app.next_command_action(later).is_none());

    // the loop pauses the chain for a break
    queue_line(&mut app, "send c");
    app.pause_commands(later + Duration::from_millis(100));
    assert!(app.next_command_action(later).is_none());
    app.fail_commands("break: failed".to_string());
    assert!(
        app.next_command_action(later + Duration::from_secs(1))
            .is_none()
    );
    queue_line(&mut app, "send d");
    let action = app.next_command_action(Instant::now());
    assert_eq!(sent(&mut app, action), b"d");
    assert!(parse_fails(&mut app, "sleep"));
    assert!(parse_fails(&mut app, "sleep 60001"));
}

/// MAX-12: a line starting with `cancel` runs it at once, not behind the commands waiting:
/// it drops them, pause and all, and stops the file being sent, then the rest of its line
/// runs.
/// How: a chain paused by `sleep`, then a transfer and a paused chain together.
#[test]
fn command_line_cancel() {
    use crate::app::transfer::FileTransfer;
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

    queue_line(&mut app, "cancel");
    assert_eq!(last_toast(&app), "Nothing to cancel");

    queue_line(&mut app, "sleep 1000; send a");
    let sleep = app.next_command_action(Instant::now()).unwrap();
    app.update(sleep);
    queue_line(&mut app, "cancel; send b");
    assert_eq!(last_toast(&app), "Dropped 1 waiting command");
    let send = app.next_command_action(Instant::now()).unwrap();
    assert!(matches!(app.update(send), UpdateEffect::Transmit(b) if b == b"b"));

    let transfer = FileTransfer::new(
        Box::new(std::io::Cursor::new(vec![b'x'; 10])),
        "fw.bin".to_string(),
        10,
        960.0,
        Duration::ZERO,
        Instant::now(),
    );
    app.update(Action::App(AppAction::StartTransfer(Box::new(transfer))));
    queue_line(&mut app, "sleep 1000; send c; send d");
    let sleep = app.next_command_action(Instant::now()).unwrap();
    app.update(sleep);
    queue_line(&mut app, "cancel");
    assert_eq!(
        last_toast(&app),
        "Stopped sending fw.bin at 0%, dropped 2 waiting commands"
    );
    assert!(app.transfer().is_none());
    assert!(
        app.next_command_action(Instant::now() + Duration::from_secs(2))
            .is_none()
    );
}

/// MAX-12: a command line bound to a key runs as a typed one, but stays out of the command
/// history; it waits behind a waiting line, unless it starts with `cancel`.
#[test]
fn bound_line_runs_as_typed() {
    let mut app = app();
    let line = |text: &str| crate::commands::parse(text).unwrap();
    app.update(Action::App(AppAction::QueueCommands(line(
        "send a; sleep 1000; send b",
    ))));
    let now = Instant::now();
    let send = app.next_command_action(now).unwrap();
    assert!(matches!(app.update(send), UpdateEffect::Transmit(a) if a == b"a"));
    let sleep = app.next_command_action(now).unwrap();
    app.update(sleep);
    app.update(Action::App(AppAction::QueueCommands(line(
        "cancel; send c",
    ))));
    let send = app.next_command_action(Instant::now()).unwrap();
    assert!(matches!(app.update(send), UpdateEffect::Transmit(c) if c == b"c"));
    assert!(app.next_command_action(Instant::now()).is_none());
    assert!(app.command_history().is_empty());
}

/// The line doesn't parse: the command line stays open
fn parse_fails(app: &mut App, line: &str) -> bool {
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
    let open = app.next_command_action(now).unwrap();
    assert!(
        matches!(app.update(open), UpdateEffect::Open(address) if address == "/dev/ttyUSB9".parse::<Address>().unwrap())
    );
    // the loop: the link closed, the line waiting for the open
    set_link_open(&mut app, false);
    app.pause_commands(now + Duration::from_secs(6));
    assert!(app.next_command_action(now).is_none());
    set_link_open(&mut app, true);
    app.resume_commands();
    let send = app.next_command_action(now).unwrap();
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
    let open = app.next_command_action(now).unwrap();
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
        let action = app.next_command_action(now)?;
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
    let action = app.next_command_action(now).expect("replay-stop queued");
    assert!(matches!(app.update(action), UpdateEffect::StopReplay));
    app.stop_replay();
    app.link
        .set_address("file:///tmp/x.raw".parse::<Address>().unwrap());
    let detach = Action::App(AppAction::ToggleAttach);
    assert!(matches!(app.update(detach), UpdateEffect::ToggleAttach));
}

/// MAX-12: `set` turns an on/off setting on or off (nothing when it is so already),
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
        while let Some(action) = app.next_command_action(Instant::now()) {
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
    assert!(!app.settings.terminal_mode.answer_queries);
    assert!(parse_fails(&mut app, "set line-wrap on"));

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
        while let Some(action) = app.next_command_action(Instant::now()) {
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
    assert_eq!(on, ("a\nb".to_string(), vec!["Add CR ON".to_string()]));
    assert_eq!(screen(None, b"c\rd").0, "d");
    let on = screen(Some(SettingKey::AddLf), b"c\rd");
    assert_eq!(on, ("c\nd".to_string(), vec!["Add LF ON".to_string()]));

    let mut app = app();
    for (line, on) in [("set add-cr on", true), ("set add-cr off", false)] {
        app.update(KeyBindingAction::CommandLine.to_action(&app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
        while let Some(action) = app.next_command_action(Instant::now()) {
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

/// The mouse toggle flips the capture setting and says so (MAX-113).
#[test]
fn mouse_toggle_flips_the_setting() {
    let mut app = app();
    assert!(app.settings.ui.mouse);
    app.toggle_setting(SettingKey::Mouse);
    assert!(!app.settings.ui.mouse);
    assert_eq!(toasts(&app), ["Mouse capture OFF"]);
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
    app.open_replay(dir.join("rec"), Duration::ZERO, None)
        .unwrap();
    let replayed = crate::link::Address::Replay(dir.join("rec/rec.raw"));
    assert_eq!(app.link_config().address, replayed);
    assert_eq!(app.settings().encoding, TextEncoding::Cp437);
    assert_eq!(app.recorded_line(), Some(CaptureLine::of(&line)));

    record_full_meta(&mut app, &dir.join("during"), false);
    let meta = RecordingMeta::read(&dir.join("during/during.meta")).unwrap();
    assert_eq!(meta.device.encoding, TextEncoding::Cp437);
    assert_eq!(meta.device.line, Some(CaptureLine::of(&line)));

    app.open_replay(plain, Duration::ZERO, None).unwrap();
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

/// MAX-151: the settings apply to what is received after them: the popup toggle turns the
/// answers off and on, and the answerback answers ENQ once set.
#[test]
fn query_answers_follow_the_settings() {
    let mut app = app();
    app.settings.mode = Mode::Terminal;
    app.toggle_setting(SettingKey::AnswerQueries);
    app.feed_rx(DSR);
    assert!(app.take_query_answers().is_empty());
    app.toggle_setting(SettingKey::AnswerQueries);
    app.feed_rx(DSR);
    assert_eq!(app.take_query_answers(), DSR_READY);

    app.feed_rx(b"\x05");
    assert!(app.take_query_answers().is_empty());
    app.settings.terminal_mode.answerback = "board-42".to_string();
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

/// MAX-159: a restored session that turned mode 2048 on doesn't leave it on: a resize sends
/// nothing and DECRQM says off, until the live app turns it on again.
/// How: a session holding `?2048h`, restored, resized to 30x100, DECRQM 2048; then `h` live.
#[test]
fn restored_size_reports_off() {
    let base = std::env::temp_dir().join(format!("maxicom_test_{}_reports", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let session = crate::capture::tests::write_test_session(
        &base,
        "/dev/ttyUSB0",
        &CaptureFormat::default(),
        b"nvim\r\n\x1b[?2048h",
        b"",
    );
    let mut app = app();
    app.settings.mode = Mode::Terminal;
    app.restore_session(&session).unwrap();
    app.terminal_mode.set_vt100_size(30, 100);
    assert!(app.take_query_answers().is_empty());
    app.feed_rx(b"\x1b[?2048$p");
    assert_eq!(app.take_query_answers(), b"\x1b[?2048;2$y");

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
