//! The queries' values for a known state (MAX-283)
use super::*;
use crate::commands::QueryFn;
use crate::{
    app::tests::app,
    configs::settings::ByteSize,
    inputs::{Action, AppAction, HistoryAction},
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
/// help, `asks`, `access` (MAX-287), and each argument's kind, whether it's optional, a number's bounds and
/// suggestions, an enum's values; a command of `[commands]` its chain, asking always when one
/// of its commands does, its access the widest of its commands'; an unknown name fails.
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
            "access: device",
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
    assert!(status.contains(&"access: read".to_string()), "{status:?}");
    // a popup for the user, a value for the others (step 5): a query that never asks
    let marks = lines(&mut app, &named("marks"));
    assert!(marks.contains(&"type: query".to_string()), "{marks:?}");
    assert!(marks.contains(&"asks: never".to_string()), "{marks:?}");
    assert!(marks.contains(&"access: read".to_string()), "{marks:?}");
    assert_eq!(
        lines(&mut app, &named("board-reset")),
        [
            "name: board-reset",
            "type: named",
            "usage: board-reset",
            "help: Pulse DTR, which resets most boards",
            "run: dtr off; sleep 100; dtr on",
            "asks: never",
            "access: device",
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
    assert!(keys.contains(&"access: control".to_string()), "{keys:?}");
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

/// MAX-283 (step 7): `history` gives line mode's last lines, oldest first, each with its
/// number and its text, in the shape of every stream (MAX-290: the oldest and newest numbers
/// kept, `cut` when older ones are left out); `partial-line` the line still coming in, with the
/// number it gets once ended (MAX-290); `history-size` the lines held and the ones the cap and
/// the clears dropped.
/// How: line mode, three lines and a prompt without its end fed; the last two asked, then
/// all; times left out of the comparison; the prompt, then ended.
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
            "oldest: 0",
            "newest: 3",
            "cut: true",
            "entries:",
            "  line: 1",
            "  text: ok",
            "  line: 2",
            "  text: ready",
        ]
    );
    let partial = partial_line(&mut app, &[]).unwrap().lines();
    assert_eq!(
        (&partial[0], &partial[2]),
        (&"line: 3".to_string(), &"text: login: ".to_string())
    );
    assert!(partial[1].starts_with("time: "), "{partial:?}");
    let all = shown(&mut app, &[]);
    assert_eq!(
        all[..5],
        [
            "oldest: 0",
            "newest: 3",
            "cut: false",
            "entries:",
            "  line: 0"
        ]
    );
    let times = history(&mut app, &[]).unwrap().lines();
    assert_eq!(
        times.iter().filter(|l| l.starts_with("  time: ")).count(),
        3
    );
    assert_eq!(
        history_size(&mut app, &[]).unwrap().lines(),
        ["lines: 3", "dropped: 0", "cleared: 0"]
    );
    app.feed_rx(b"\r\n");
    assert_eq!(
        partial_line(&mut app, &[]).unwrap().lines(),
        ["line: 4", "text: "]
    );
}

/// The numbers of the lines a `history` value gives
fn line_numbers(value: &Value) -> Vec<String> {
    let lines = value.lines().into_iter();
    lines
        .filter_map(|line| Some(line.strip_prefix("  line: ")?.to_string()))
        .collect()
}

/// MAX-290: a line keeps its number across the cap's drops and a clear; `history-since` polls
/// from a number: from the oldest kept and `cut` when it is older, none from the newest on.
/// How: a 3-line history in line mode fed 5 lines, polled from 0, 4, 5 and 99; cleared, a line
/// more, polled from 5; `history-size` then.
#[test]
fn history_since_polls_by_line_number() {
    let mut app = app_with(|settings| {
        settings.mode = Mode::Line;
        settings.line_mode.history_size = 3;
    });
    app.feed_rx(b"l0\nl1\nl2\nl3\nl4\n");
    let since = |app: &mut App, line: i64| history_since(app, &[ArgValue::Int(line)]).unwrap();
    let from_start = since(&mut app, 0);
    assert_eq!(line_numbers(&from_start), ["2", "3", "4"]);
    assert_eq!(
        from_start.lines()[..3],
        ["oldest: 2", "newest: 5", "cut: true"]
    );
    let last = since(&mut app, 4);
    assert_eq!(line_numbers(&last), ["4"]);
    assert!(last.lines().contains(&"cut: false".to_string()));
    assert!(line_numbers(&since(&mut app, 5)).is_empty());
    assert!(line_numbers(&since(&mut app, 99)).is_empty());

    app.update(Action::History(HistoryAction::Clear));
    app.feed_rx(b"l5\n");
    let after = since(&mut app, 5);
    assert_eq!(line_numbers(&after), ["5"]);
    assert!(after.lines().iter().any(|line| line == "  text: l5"));
    assert_eq!(
        history_size(&mut app, &[]).unwrap().lines(),
        ["lines: 1", "dropped: 2", "cleared: 3"]
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

/// A field of a map value
fn field<'a>(value: &'a Value, name: &str) -> &'a Value {
    let Value::Map(entries) = value else {
        panic!("not a map: {value:?}")
    };
    match entries.iter().find(|(key, _)| key == name) {
        Some((_, value)) => value,
        None => panic!("no {name} in {value:?}"),
    }
}

/// A `rows` value's `[oldest, newest]`, `cut` and its rows: number, text, wrapped
fn rows_parts(value: &Value) -> ([i64; 2], bool, Vec<(i64, String, bool)>) {
    let int = |value: &Value, name| match field(value, name) {
        Value::Int(n) => *n,
        other => panic!("{name}: {other:?}"),
    };
    let flag = |value: &Value, name| match field(value, name) {
        Value::Bool(flag) => *flag,
        other => panic!("{name}: {other:?}"),
    };
    let Value::List(entries) = field(value, "entries") else {
        panic!("no entries: {value:?}")
    };
    let rows = entries.iter().map(|row| {
        let Value::Text(text) = field(row, "text") else {
            panic!("text: {row:?}")
        };
        (int(row, "row"), text.clone(), flag(row, "wrapped"))
    });
    (
        [int(value, "oldest"), int(value, "newest")],
        flag(value, "cut"),
        rows.collect(),
    )
}

/// MAX-290: `rows` gives the terminal rows that scrolled up, each with its number, its text
/// (with its colors as escapes for `colors`) and whether the terminal wrapped it, in the shape
/// of every stream; the screen's rows aren't there. `rows-since` polls from a number: none
/// from the newest on.
/// How: a line 2 columns wider than the screen, a bold line, then enough lines to scroll them
/// up; `rows`, `rows colors 0 2`, `rows-since` from the second row and from the newest.
#[test]
fn rows_give_the_scrolled_rows() {
    let mut app = app();
    let (height, cols) = app.terminal_mode.get_vt100_size();
    let long = "x".repeat(usize::from(cols) + 2);
    let lines: String = (0..=height).map(|n| format!("\r\nl{n}")).collect();
    app.feed_rx(format!("{long}\r\n\x1b[1mbold\x1b[0m{lines}").as_bytes());

    let ([oldest, newest], cut, scrolled) = rows_parts(&rows(&mut app, &[]).unwrap());
    assert_eq!((oldest, cut), (0, false));
    assert_eq!(newest, scrolled.len() as i64);
    assert_eq!(
        scrolled[..4],
        [
            (0, "x".repeat(usize::from(cols)), true),
            (1, "xx".to_string(), false),
            (2, "bold".to_string(), false),
            (3, "l0".to_string(), false),
        ]
    );
    let last_on_screen = format!("l{height}");
    assert!(scrolled.iter().all(|(_, text, _)| *text != last_on_screen));

    let colors = [ArgValue::Enum("colors"), ArgValue::Int(0), ArgValue::Int(2)];
    let (_, cut, from_two) = rows_parts(&rows_since(&mut app, &[ArgValue::Int(2)]).unwrap());
    assert!(!cut);
    assert_eq!(from_two[0].0, 2);
    let (_, _, last_two) = rows_parts(&rows(&mut app, &colors).unwrap());
    assert_eq!(last_two.len(), 2);
    let bold = rows_since(
        &mut app,
        &[ArgValue::Int(2), ArgValue::Int(1), ArgValue::Enum("colors")],
    );
    let (_, _, bold) = rows_parts(&bold.unwrap());
    assert!(bold[0].1.contains("\x1b[1mbold"), "{bold:?}");
    let past = rows_since(&mut app, &[ArgValue::Int(newest)]).unwrap();
    assert!(rows_parts(&past).2.is_empty());
}

/// MAX-290: a row keeps its number as the scrollback evicts and across a clear; `rows-since`
/// from older than kept starts at the oldest and says `cut`. On the alternate screen nothing
/// scrolls: no rows, the numbers kept.
/// How: a 5-row terminal history fed 40 lines; a clear and a line more; a full-screen program
/// writing 40 lines.
#[test]
fn rows_keep_their_numbers() {
    let mut app = app_with(|settings| settings.terminal_mode.history_size = 5);
    let lines: Vec<String> = (0..40).map(|n| format!("l{n}")).collect();
    app.feed_rx(lines.join("\r\n").as_bytes());
    let ([oldest, newest], cut, kept) =
        rows_parts(&rows_since(&mut app, &[ArgValue::Int(0)]).unwrap());
    assert!(cut);
    assert_eq!(newest - oldest, 5);
    let numbered = (kept.iter()).all(|(row, text, _)| *text == format!("l{row}"));
    assert!(numbered, "{kept:?}");

    app.update(Action::History(HistoryAction::Clear));
    let ([cleared, _], _, none) =
        rows_parts(&rows_since(&mut app, &[ArgValue::Int(newest)]).unwrap());
    assert!(none.is_empty());
    assert!(cleared >= 40, "{cleared}");

    let full_screen: String = (0..40).map(|n| format!("menu {n}\r\n")).collect();
    app.feed_rx(format!("\x1b[?1049h{full_screen}").as_bytes());
    let ([_, during], _, none) = rows_parts(&rows(&mut app, &[]).unwrap());
    assert!(none.is_empty());
    app.feed_rx(b"\x1b[?1049l");
    let ([_, after], _, _) = rows_parts(&rows(&mut app, &[]).unwrap());
    assert_eq!((during, after), (cleared, cleared));
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

/// The parts of a `raw` value: its bytes, `[start, oldest, newest]` and `cut`
fn raw_parts(value: &Value) -> (Vec<u8>, [i64; 3], bool) {
    let Value::Map(entries) = value else {
        panic!("not a map: {value:?}")
    };
    let field = |name: &str| match entries.iter().find(|(key, _)| key == name) {
        Some((_, value)) => value,
        None => panic!("no {name}"),
    };
    let int = |name: &str| match field(name) {
        Value::Int(n) => *n,
        other => panic!("{name}: {other:?}"),
    };
    let (Value::Bytes(bytes), Value::Bool(cut)) = (field("bytes"), field("cut")) else {
        panic!("bytes and cut: {value:?}")
    };
    (
        bytes.clone(),
        [int("start"), int("oldest"), int("newest")],
        *cut,
    )
}

/// MAX-284 (step 2a): `raw [from] [count]` gives the last bytes received, skipping `from`
/// newest, in the shape of every stream (MAX-290): the oldest and newest offsets kept, `cut`
/// when older bytes kept are left out, the first byte's offset and the bytes; `raw-since`
/// gives them from an offset on, `count` at most, none past the end. The TUI shows them as hex
/// and escaped text.
/// How: two reads in terminal mode, then each form.
#[test]
fn raw_gives_the_bytes_received() {
    let mut app = app();
    app.feed_rx(b"boot\r\n");
    app.feed_rx(b"\x1b[1mok\\");
    let all = raw(&mut app, &[]).unwrap();
    assert_eq!(
        raw_parts(&all),
        (b"boot\r\n\x1b[1mok\\".to_vec(), [0, 0, 13], false)
    );
    assert_eq!(
        all.lines(),
        [
            "oldest: 0",
            "newest: 13",
            "cut: false",
            "start: 0",
            "bytes:",
            "  62 6f 6f 74 0d 0a 1b 5b 31 6d 6f 6b 5c           boot\\r\\n\\x1b[1mok\\\\",
        ]
    );
    let middle = raw(&mut app, &[ArgValue::Int(3), ArgValue::Int(4)]).unwrap();
    assert_eq!(raw_parts(&middle), (b"\x1b[1m".to_vec(), [6, 0, 13], true));
    let since = raw_since(&mut app, &[ArgValue::Int(6), ArgValue::Int(2)]).unwrap();
    assert_eq!(raw_parts(&since), (b"\x1b[".to_vec(), [6, 0, 13], false));
    let rest = raw_since(&mut app, &[ArgValue::Int(8)]).unwrap();
    assert_eq!(raw_parts(&rest), (b"1mok\\".to_vec(), [8, 0, 13], false));
    let past = raw_since(&mut app, &[ArgValue::Int(99)]).unwrap();
    assert_eq!(raw_parts(&past), (Vec::new(), [13, 0, 13], false));
    assert!(past.lines().contains(&"bytes: none".to_string()));
}

/// MAX-284 (step 2a): bytes older than the stream keeps are gone: `raw-since` from before
/// them starts at the oldest kept and says `cut` (MAX-290); after a clear-history none are
/// kept, the offsets counting on.
/// How: more than the stream's 64 KiB floor fed, then `raw-since 0`; a clear-history.
#[test]
fn raw_since_older_than_kept() {
    // `rx_bytes` at 0: the stream keeps its 64 KiB floor
    let mut app = app_with(|settings| settings.rx_bytes = ByteSize(0));
    app.feed_rx(&vec![b'a'; 70 * 1024]);
    app.feed_rx(b"end");
    let value = raw_since(&mut app, &[ArgValue::Int(0), ArgValue::Int(10)]).unwrap();
    let (bytes, [start, oldest, newest], cut) = raw_parts(&value);
    assert!(oldest > 0 && cut, "{oldest}");
    assert_eq!((start, newest), (oldest, 70 * 1024 + 3));
    assert_eq!(bytes, vec![b'a'; 10]);
    let last = raw(&mut app, &[ArgValue::Int(0), ArgValue::Int(3)]).unwrap();
    assert_eq!(raw_parts(&last).0, b"end");
    app.update(Action::History(HistoryAction::Clear));
    let cleared = raw_since(&mut app, &[ArgValue::Int(0)]).unwrap();
    assert_eq!(
        raw_parts(&cleared),
        (Vec::new(), [newest, newest, newest], true)
    );
}

/// MAX-284 (step 2a): a mode's getters are refused when the bytes received don't go to that
/// mode, saying why: `rows`, `rows-since`, `screen`, `cursor` with terminal mode disabled or hidden under
/// `active_only`; `history`, `history-since`, `partial-line`, `history-size` the same for line mode. Fed by
/// `always_both`, the hidden mode's getters answer. `raw` and `raw-since` answer in any mode
/// (MAX-290).
/// How: an app for each setting, each getter.
#[test]
fn getters_of_a_mode_not_fed_refused() {
    use crate::configs::{DualBufferMode, settings::DisabledMode};
    let since = [ArgValue::Int(0)];
    let terminal: [(QueryFn, &[ArgValue]); 4] = [
        (rows, &[]),
        (rows_since, &since),
        (screen, &[]),
        (cursor, &[]),
    ];
    let line: [(QueryFn, &[ArgValue]); 4] = [
        (history, &[]),
        (history_since, &since),
        (partial_line, &[]),
        (history_size, &[]),
    ];
    let any: [(QueryFn, &[ArgValue]); 2] = [(raw, &[]), (raw_since, &since)];
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
    for query in any {
        assert_eq!(refusal(&mut no_terminal, query), None);
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
    for query in any {
        assert_eq!(refusal(&mut line_only, query), None);
        assert_eq!(refusal(&mut terminal_only, query), None);
    }
    let mut both = app_with(|settings| settings.mode = Mode::Line);
    for query in terminal.into_iter().chain(line) {
        assert_eq!(refusal(&mut both, query), None);
    }
}
