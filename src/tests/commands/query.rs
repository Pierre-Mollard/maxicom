//! The queries' values for a known state (MAX-283)
use super::*;
use crate::{
    app::tests::app,
    inputs::{Action, AppAction},
    notification::ToastLevel,
    overlays::SettingKey,
};

fn lines(app: &mut App, args: &[ArgValue]) -> Vec<String> {
    commands(app, args).unwrap().lines()
}

fn named(name: &str) -> Vec<ArgValue> {
    vec![ArgValue::String(name.to_string())]
}

/// MAX-283 (step 3): bare, `commands` gives every command's name and help, the registry's
/// and the default `[commands]`', sorted.
/// How: the default keymap's named commands.
#[test]
fn commands_lists_every_command() {
    let mut app = app();
    let Value::Map(entries) = commands(&mut app, &[]).unwrap() else {
        panic!("not a map");
    };
    let names: Vec<&str> = entries.iter().map(|(name, _)| name.as_str()).collect();
    assert!(names.is_sorted(), "{names:?}");
    assert_eq!(
        entries.len(),
        COMMANDS.len() + app.key_resolver.commands.len()
    );
    let lines = lines(&mut app, &[]);
    assert!(lines.contains(&"baud: Set the baud rate".to_string()));
    let reset = "board-reset: Pulse DTR, which resets most boards".to_string();
    assert!(lines.contains(&reset), "{lines:?}");
}

/// MAX-283 (step 3): with a name, `commands` gives that command in detail: type, usage,
/// help, `asks`, and each argument's kind, whether it's optional, a number's bounds and
/// suggestions, an enum's values; a command of `[commands]` its chain, asking always when one
/// of its commands does; an unknown name fails.
/// How: `baud`, `mark` (bare: waits for its letter), `trigger`, `status`, `board-reset`, a
/// named `keys` running `help`, `zz`.
#[test]
fn commands_shows_one_command() {
    let mut app = app();
    assert_eq!(
        lines(&mut app, &named("baud")),
        [
            "name: baud",
            "type: action",
            "usage: baud <rate>",
            "help: Set the baud rate",
            "asks: never",
            "args:",
            "  rate:",
            "    kind: int",
            "    optional: false",
            "    min: 50",
            "    max: 4000000",
            "    suggestions: 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600",
        ]
    );
    let mark = lines(&mut app, &named("mark"));
    assert!(mark.contains(&"asks: bare".to_string()), "{mark:?}");
    assert!(mark.contains(&"    optional: true".to_string()), "{mark:?}");
    let trigger = lines(&mut app, &named("trigger"));
    assert!(
        trigger.contains(&"    values: on, off, toggle, reset".to_string()),
        "{trigger:?}"
    );
    assert!(
        trigger.contains(&"    kind: trigger".to_string()),
        "{trigger:?}"
    );
    let status = lines(&mut app, &named("status"));
    assert!(status.contains(&"type: query".to_string()), "{status:?}");
    assert!(status.contains(&"args: none".to_string()), "{status:?}");
    // a popup for the user, a value for the others (step 5): a query that never asks
    let marks = lines(&mut app, &named("marks"));
    assert!(marks.contains(&"type: query".to_string()), "{marks:?}");
    assert!(marks.contains(&"asks: never".to_string()), "{marks:?}");
    assert_eq!(
        lines(&mut app, &named("board-reset")),
        [
            "name: board-reset",
            "type: named",
            "usage: board-reset",
            "help: Pulse DTR, which resets most boards",
            "run: dtr off; sleep 100; dtr on",
            "asks: never",
            "args: none",
        ]
    );
    app.key_resolver.commands.push(NamedCommand {
        name: "keys".to_string(),
        run: "help".to_string(),
        help: String::new(),
        invocations: crate::commands::parse("help").unwrap(),
    });
    let keys = lines(&mut app, &named("keys"));
    assert!(keys.contains(&"help: help".to_string()), "{keys:?}");
    assert!(keys.contains(&"asks: always".to_string()), "{keys:?}");
    assert_eq!(
        commands(&mut app, &named("zz")),
        Err("unknown command 'zz'".to_string())
    );
}

/// MAX-283 (step 4): bare, `get` gives every setting by name, a switch as a yes or no, the
/// others as the settings file writes them; with a name, that setting's value alone.
/// How: the default settings, then `mode` and `local-echo` after their toggles.
#[test]
fn get_gives_the_settings() {
    let mut app = app();
    assert_eq!(
        get(&mut app, &[]).unwrap().lines(),
        [
            "mode: terminal",
            "local-echo: false",
            "timestamps: false",
            "encoding: utf8",
            "mouse: true",
            "display-raw: off",
            "plot: off",
            "add-cr: false",
            "add-lf: false",
            "tx-eol: lf",
            "line-wrap: word",
            "indent-wrap: timestamp",
            "prefix-place: border",
            "display-mode: pretty",
            "size-mode: auto",
            "answer-queries: true",
            "byte-trace: true",
            "backspace: del",
        ]
    );
    let one = |app: &mut App, name: &'static str| get(app, &[ArgValue::Enum(name)]).unwrap();
    assert_eq!(one(&mut app, "size-mode"), Value::text("auto"));
    for key in [SettingKey::Mode, SettingKey::LocalEcho] {
        app.update(Action::App(AppAction::ToggleSetting(key)));
    }
    assert_eq!(one(&mut app, "mode"), Value::text("line"));
    assert_eq!(one(&mut app, "local-echo"), Value::Bool(true));
}

/// MAX-283 (step 4): every setting has its own name, which `get` parses and finds back; the
/// names `set` takes are switches' names.
/// How: each `SettingKey` (the match fails to build when one is added and not listed), and
/// each of `set`'s names.
#[test]
fn every_setting_is_reachable() {
    for key in SettingKey::ALL {
        match key {
            SettingKey::LocalEcho
            | SettingKey::Mode
            | SettingKey::LineWrap
            | SettingKey::IdentWrap
            | SettingKey::PrefixPlace
            | SettingKey::DisplayTimestamp
            | SettingKey::TextEncoding
            | SettingKey::DisplayMode
            | SettingKey::DisplayRaw
            | SettingKey::Mouse
            | SettingKey::DimensionMode
            | SettingKey::AnswerQueries
            | SettingKey::ByteTrace
            | SettingKey::AddCr
            | SettingKey::AddLf
            | SettingKey::TxEol
            | SettingKey::Backspace
            | SettingKey::Plot => {}
        }
        assert_eq!(SettingKey::named(key.name()), Some(key));
        let line = format!("get {}", key.name());
        assert!(crate::commands::parse(&line).is_ok(), "{line}");
    }
    let mut names = SettingKey::NAMES.to_vec();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), SettingKey::ALL.len(), "a name twice");
    let app = app();
    for name in crate::commands::build::ON_OFF_SETTINGS {
        let key = SettingKey::named(name).unwrap();
        assert!(app.setting_is_on(key).is_some(), "{name}");
    }
}

/// MAX-283 (step 6): `[from] [count]` picks the last `count` lines after skipping `from`
/// newest, oldest first, and says when older ones are left out.
/// How: ten lines, paged in each way, past both ends.
#[test]
fn page_counts_from_the_newest() {
    let int = ArgValue::Int;
    assert_eq!(page(10, &[]), (0..10, false));
    assert_eq!(page(100, &[]), (50..100, true));
    assert_eq!(page(10, &[int(2), int(3)]), (5..8, true));
    assert_eq!(page(10, &[int(7), int(5)]), (0..3, false));
    assert_eq!(page(10, &[int(20)]), (0..0, false));
    assert_eq!(page(0, &[int(0), int(5)]), (0..0, false));
    assert_eq!(page(10, &[int(i64::MAX)]), (0..0, false));
}

/// MAX-283 (step 6): `notifications` gives how many lines the history holds, whether older
/// ones were left out, and the lines picked, oldest first: time, level, text.
/// How: three toasts, the last two asked; times left out of the comparison.
#[test]
fn notifications_gives_the_history() {
    let mut app = app();
    for (text, level) in [
        ("one", ToastLevel::Info),
        ("two", ToastLevel::Warn),
        ("three", ToastLevel::Error),
    ] {
        app.notifications
            .push(text, level, std::time::Duration::from_secs(1));
    }
    let args = [ArgValue::Int(0), ArgValue::Int(2)];
    let lines = notifications(&mut app, &args).unwrap().lines();
    // UTC to the millisecond
    let time = lines[3].strip_prefix("  time: ").unwrap();
    let (_, fraction) = time.split_once('.').unwrap();
    assert_eq!(fraction.len(), 4, "{time}");
    assert!(time.ends_with('Z'), "{time}");
    let lines: Vec<&str> = (lines.iter().map(String::as_str))
        .filter(|line| !line.starts_with("  time: "))
        .collect();
    assert_eq!(
        lines,
        [
            "total: 3",
            "cut: true",
            "notices:",
            "  level: warn",
            "  text: two",
            "  level: error",
            "  text: three",
        ]
    );
}

/// MAX-283 (step 6): `triggers` gives each trigger by name: text, chain, armed, rearm,
/// silent, mark, hits, and its last fire once it fired.
/// How: the default triggers armed, `login: ` fed.
#[test]
fn triggers_gives_each_trigger() {
    let mut app = app();
    let config = crate::configs::TriggersConfig::default();
    app.set_triggers(config.compile().unwrap());
    let before = triggers(&mut app, &[]).unwrap().lines();
    assert!(
        !before.iter().any(|line| line.contains("last: ")),
        "{before:?}"
    );
    let chain = crate::commands::parse("trigger on").unwrap();
    app.update(Action::App(AppAction::QueueCommands(chain)));
    while let Some((caller, action)) = app.next_command_action(std::time::Instant::now()) {
        app.update_for(&caller, action);
    }
    app.feed_rx(b"login: ");
    let lines = triggers(&mut app, &[]).unwrap().lines();
    let at = lines.iter().position(|line| line == "login:").unwrap();
    let login = &lines[at + 1..at + 9];
    assert_eq!(login[0], "  text: login: ");
    assert!(login[1].starts_with("  run: "), "{login:?}");
    assert_eq!(
        login[2..7],
        [
            "  armed: true",
            "  rearm: true",
            "  silent: false",
            "  mark: none",
            "  hits: 1",
        ]
    );
    assert!(login[7].starts_with("  last: "), "{login:?}");
}

/// MAX-283 (step 7): `history` gives line mode's last lines, oldest first, each with its line
/// (a mark's) and its text, how many there are, `cut` when older ones are left out, and the
/// line still coming in; `history-size` the lines held and the ones the cap dropped.
/// How: line mode, three lines and a prompt without its end fed; the last two asked, then
/// all; times left out of the comparison.
#[test]
fn history_gives_the_last_lines() {
    let mut app = app();
    app.update(Action::App(AppAction::ToggleSetting(SettingKey::Mode)));
    assert_eq!(
        get(&mut app, &[ArgValue::Enum("mode")]),
        Ok(Value::text("line"))
    );
    app.feed_rx(b"boot\r\nok\r\nready\r\nlogin: ");
    let shown = |app: &mut App, args: &[ArgValue]| -> Vec<String> {
        let lines = history(app, args).unwrap().lines().into_iter();
        lines.filter(|line| !line.starts_with("  time: ")).collect()
    };
    assert_eq!(
        shown(&mut app, &[ArgValue::Int(0), ArgValue::Int(2)]),
        [
            "total: 3",
            "cut: true",
            "entries:",
            "  line: 1",
            "  text: ok",
            "  line: 2",
            "  text: ready",
            "partial: login: ",
        ]
    );
    let all = shown(&mut app, &[]);
    assert_eq!(
        all[..4],
        ["total: 3", "cut: false", "entries:", "  line: 0"]
    );
    let times = history(&mut app, &[]).unwrap().lines();
    assert_eq!(
        times.iter().filter(|l| l.starts_with("  time: ")).count(),
        3
    );
    assert_eq!(
        history_size(&mut app, &[]).unwrap().lines(),
        ["lines: 3", "dropped: 0"]
    );
}

/// MAX-283 (step 8): `screen` gives the terminal's rows as lines (control characters shown as
/// pictures in the TUI, kept in the value), `cursor` its row, column and visibility,
/// `term-size` the emulator's size.
/// How: two lines fed in terminal mode, one of them bold.
#[test]
fn screen_cursor_and_size() {
    let mut app = app();
    app.feed_rx(b"hello\r\n\x1b[1mworld\x1b[0m");
    let text = screen(&mut app, &[]).unwrap();
    assert_eq!(text.lines()[..2], ["hello", "world"]);
    let colors = screen(&mut app, &[ArgValue::Enum("colors")]).unwrap();
    let Value::Text(raw) = &colors else {
        panic!("not a text");
    };
    assert!(raw.contains("\x1b[1mworld"), "{raw:?}");
    assert!(colors.lines().iter().any(|line| line.contains("␛[1mworld")));
    assert_eq!(
        cursor(&mut app, &[]).unwrap().lines(),
        ["row: 1", "col: 5", "visible: true"]
    );
    let (rows, cols) = app.terminal_mode.get_vt100_size();
    assert_eq!(
        term_size(&mut app, &[]).unwrap().lines(),
        [format!("rows: {rows}"), format!("cols: {cols}")]
    );
}
