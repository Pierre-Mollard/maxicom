//! The queries' values for a known state (MAX-283)
use super::*;
use crate::commands::QueryFn;
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

/// An app built with `settings` changed
fn app_with(change: impl FnOnce(&mut crate::configs::SettingsConfig)) -> App {
    use crate::{
        configs::{KeyBindingsConfig, KeyResolver, SettingsConfig},
        link::LinkConfig,
    };
    let mut settings = SettingsConfig::default();
    change(&mut settings);
    App::new(
        settings,
        KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap(),
        LinkConfig::default(),
    )
}

/// The bytes, offsets and lines of a `raw` value
fn raw_parts(value: &Value) -> (Vec<u8>, [i64; 4]) {
    let Value::Map(entries) = value else {
        panic!("not a map: {value:?}")
    };
    let int = |name: &str| match entries.iter().find(|(key, _)| key == name) {
        Some((_, Value::Int(n))) => *n,
        other => panic!("{name}: {other:?}"),
    };
    let Some((_, Value::Bytes(bytes))) = entries.iter().find(|(key, _)| key == "bytes") else {
        panic!("no bytes")
    };
    (
        bytes.clone(),
        [int("start"), int("end"), int("oldest"), int("newest")],
    )
}

/// MAX-284 (step 2a): `raw [from] [count]` gives the last bytes received, skipping `from`
/// newest, with their offsets and the stream's oldest and newest; `raw-since` gives them from
/// an offset on, `count` at most, none past the end. The TUI shows them as hex and escaped
/// text.
/// How: two reads in terminal mode, then each form.
#[test]
fn raw_gives_the_bytes_received() {
    let mut app = app();
    app.feed_rx(b"boot\r\n");
    app.feed_rx(b"\x1b[1mok\\");
    let all = raw(&mut app, &[]).unwrap();
    assert_eq!(
        raw_parts(&all),
        (b"boot\r\n\x1b[1mok\\".to_vec(), [0, 13, 0, 13])
    );
    assert_eq!(
        all.lines(),
        [
            "start: 0",
            "end: 13",
            "oldest: 0",
            "newest: 13",
            "bytes:",
            "  62 6f 6f 74 0d 0a 1b 5b 31 6d 6f 6b 5c           boot\\r\\n\\x1b[1mok\\\\",
        ]
    );
    let middle = raw(&mut app, &[ArgValue::Int(3), ArgValue::Int(4)]).unwrap();
    assert_eq!(raw_parts(&middle), (b"\x1b[1m".to_vec(), [6, 10, 0, 13]));
    let since = raw_since(&mut app, &[ArgValue::Int(6), ArgValue::Int(2)]).unwrap();
    assert_eq!(raw_parts(&since), (b"\x1b[".to_vec(), [6, 8, 0, 13]));
    let rest = raw_since(&mut app, &[ArgValue::Int(8)]).unwrap();
    assert_eq!(raw_parts(&rest), (b"1mok\\".to_vec(), [8, 13, 0, 13]));
    let past = raw_since(&mut app, &[ArgValue::Int(99)]).unwrap();
    assert_eq!(raw_parts(&past), (Vec::new(), [13, 13, 0, 13]));
    assert!(past.lines().contains(&"bytes: none".to_string()));
}

/// MAX-284 (step 2a): bytes older than the stream keeps are gone: `raw-since` from before
/// them starts at the oldest kept, which a client compares with where it asked from; after a
/// clear-history none are kept, the offsets counting on.
/// How: more than the stream's 64 KiB floor fed, then `raw-since 0`; a clear-history.
#[test]
fn raw_since_older_than_kept() {
    // a history of a few rows: the stream keeps its 64 KiB floor
    let mut app = app_with(|settings| settings.terminal_mode.history_size = 10);
    app.feed_rx(&vec![b'a'; 70 * 1024]);
    app.feed_rx(b"end");
    let value = raw_since(&mut app, &[ArgValue::Int(0), ArgValue::Int(10)]).unwrap();
    let (bytes, [start, end, oldest, newest]) = raw_parts(&value);
    assert!(oldest > 0, "{oldest}");
    assert_eq!((start, end, newest), (oldest, oldest + 10, 70 * 1024 + 3));
    assert_eq!(bytes, vec![b'a'; 10]);
    let last = raw(&mut app, &[ArgValue::Int(0), ArgValue::Int(3)]).unwrap();
    assert_eq!(raw_parts(&last).0, b"end");
    app.terminal_mode.clear_history();
    let cleared = raw_since(&mut app, &[ArgValue::Int(0)]).unwrap();
    assert_eq!(
        raw_parts(&cleared),
        (Vec::new(), [newest, newest, newest, newest])
    );
}

/// MAX-284 (step 2a): a mode's getters are refused when the bytes received don't go to that
/// mode, saying why: `raw`, `raw-since`, `screen`, `cursor` with terminal mode disabled or
/// hidden under `active_only`; `history`, `history-size` the same for line mode. Fed by
/// `always_both`, the hidden mode's getters answer.
/// How: an app for each setting, each getter.
#[test]
fn getters_of_a_mode_not_fed_refused() {
    use crate::configs::{DualBufferMode, settings::DisabledMode};
    let since = [ArgValue::Int(0)];
    let terminal: [(QueryFn, &[ArgValue]); 4] = [
        (raw, &[]),
        (raw_since, &since),
        (screen, &[]),
        (cursor, &[]),
    ];
    let line: [(QueryFn, &[ArgValue]); 2] = [(history, &[]), (history_size, &[])];
    let refusal = |app: &mut App, (query, args): (QueryFn, &[ArgValue])| query(app, args).err();

    let mut no_terminal = app_with(|settings| {
        settings.mode = Mode::Line;
        settings.disabled_mode = DisabledMode::Terminal;
    });
    for query in terminal {
        let error = refusal(&mut no_terminal, query);
        assert_eq!(
            error.as_deref(),
            Some("terminal mode is disabled (disabled_mode)")
        );
    }
    let hidden = "terminal mode isn't fed while it's hidden (dual_mode_buffering = active_only)";
    let mut line_only = app_with(|settings| {
        settings.mode = Mode::Line;
        settings.dual_mode_buffering = DualBufferMode::ActiveOnly;
    });
    for query in terminal {
        assert_eq!(refusal(&mut line_only, query).as_deref(), Some(hidden));
    }
    for query in line {
        assert_eq!(refusal(&mut line_only, query), None);
    }
    let mut terminal_only = app_with(|settings| {
        settings.dual_mode_buffering = DualBufferMode::ActiveOnly;
    });
    let hidden = "line mode isn't fed while it's hidden (dual_mode_buffering = active_only)";
    for query in line {
        assert_eq!(refusal(&mut terminal_only, query).as_deref(), Some(hidden));
    }
    let mut both = app_with(|settings| settings.mode = Mode::Line);
    for query in terminal.into_iter().chain(line) {
        assert_eq!(refusal(&mut both, query), None);
    }
}
