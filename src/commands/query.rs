//! The queries (MAX-165): commands that read the app's state and change nothing, run at once,
//! their `Value` shown in a popup for the user and later given to a client (MAX-284). A
//! trigger's or a profile's query does nothing: nobody waits for its answer
use super::{
    Access, ArgKind, ArgSpec, ArgValue, Asks, COMMANDS, CommandSpec, Invocation, NamedCommand,
    Target,
    jobs::{Job, Wait},
    value::Value,
};
use crate::{
    app::App,
    configs::Mode,
    link::{LinkKind, serial::LineLevel},
    overlays::{MarkRow, SettingKey},
};

/// `status`: the link (its name, kind and state, its line settings, DTR and RTS), the mode, the encoding,
/// the bytes received and sent, the recording, the session backup, the file going through,
/// the profiles laid and the jobs running
pub fn status(app: &mut App, _args: &[ArgValue]) -> Result<Value, String> {
    let kind = app.link_kind();
    let state = match (kind, app.is_link_open()) {
        (LinkKind::Replay, _) => "replay",
        (_, true) => "open",
        (_, false) => "closed",
    };
    let mut entries = vec![
        ("link".to_string(), Value::text(app.link_label())),
        ("kind".to_string(), Value::text(kind.name())),
        ("state".to_string(), Value::text(state)),
    ];
    if let Some(line) = app.line_settings() {
        let line = format!("{} {}", line.baud, line.framing);
        entries.push(("line".to_string(), Value::text(line)));
    }
    if kind.has_line_settings() {
        let (dtr, rts) = app.line_levels();
        entries.push(("dtr".to_string(), Value::text(level(dtr))));
        entries.push(("rts".to_string(), Value::text(level(rts))));
    }
    let settings = app.settings();
    let recording = app.recording().map_or("none", |stream| stream.as_str());
    let transfer = match app.transfer() {
        Some(transfer) => format!("{} at {}%", transfer.name(), transfer.percent()),
        None => "none".to_string(),
    };
    let profiles = app.profiles().iter().map(Value::text).collect();
    entries.extend([
        ("mode".to_string(), Value::text(settings.mode.as_str())),
        (
            "encoding".to_string(),
            Value::text(settings.encoding.as_str()),
        ),
        ("rx".to_string(), Value::Int(app.stats.rx() as i64)),
        ("tx".to_string(), Value::Int(app.stats.tx() as i64)),
        ("recording".to_string(), Value::text(recording)),
        ("session".to_string(), session(app)),
        ("transfer".to_string(), Value::text(transfer)),
        ("profiles".to_string(), Value::List(profiles)),
        (
            "jobs".to_string(),
            Value::List(app.jobs().running().map(job).collect()),
        ),
    ]);
    Ok(Value::Map(entries))
}

/// A control line's level as maxicom last set it (MAX-283), `unset` for one it didn't touch
/// since the open: the OS's, or the driver's under hardware flow control
fn level(level: LineLevel) -> &'static str {
    match level {
        LineLevel::On => "on",
        LineLevel::Off => "off",
        LineLevel::Keep => "unset",
    }
}

/// The session backup (MAX-283): its stem, its folder holding its files, and whether it
/// records or a replay paused it; `none` without one
fn session(app: &App) -> Value {
    let Some((raw, paused)) = app.session_backup() else {
        return Value::text("none");
    };
    let folder = raw.parent().unwrap_or(raw);
    let stem = folder.file_name().unwrap_or_default().to_string_lossy();
    let state = if paused { "paused" } else { "running" };
    Value::Map(vec![
        ("stem".to_string(), Value::text(stem)),
        (
            "folder".to_string(),
            Value::text(folder.display().to_string()),
        ),
        ("state".to_string(), Value::text(state)),
    ])
}

/// A job running (MAX-283): its number, who started it (and the trigger's or the hook's
/// name), its command due and how many are left, counting that one, and what it waits for
fn job(job: &Job) -> Value {
    let origin = job.origin();
    let mut entries = vec![
        ("id".to_string(), Value::Int(job.id() as i64)),
        ("origin".to_string(), Value::text(origin.kind())),
    ];
    if let Some(name) = origin.name() {
        entries.push(("name".to_string(), Value::text(name)));
    }
    let next = job.commands().front().map_or("none", |next| next.spec.name);
    let wait = match job.wait() {
        None => "none",
        Some(Wait::Until(_)) => "time",
        Some(Wait::LinkOpen { .. }) => "link-open",
        Some(Wait::Mash) => "mash",
        Some(Wait::Text { .. }) => "text",
        Some(Wait::Transfer) => "transfer",
        Some(Wait::Asked { .. }) => "user",
    };
    entries.extend([
        ("next".to_string(), Value::text(next)),
        ("left".to_string(), Value::Int(job.commands().len() as i64)),
        ("wait".to_string(), Value::text(wait)),
    ]);
    Value::Map(entries)
}

/// `commands [name]` (MAX-283), the API's description of itself: bare, every command's name
/// and help, the registry's and `[commands]`', sorted; with a name, that command in detail:
/// its type (action, query, or named for one of `[commands]`), usage, help, whether it needs
/// the user (`asks`), what it reaches (`access`, the API level a client needs, MAX-287), and
/// its arguments (kind, optional, bounds, values)
pub fn commands(app: &mut App, args: &[ArgValue]) -> Result<Value, String> {
    let named = &app.key_resolver.commands;
    let name = match args {
        [] => {
            let builtin = COMMANDS.iter().map(|spec| (spec.name, spec.help));
            let named =
                (named.iter()).map(|command| (command.name.as_str(), command.description()));
            let mut list: Vec<_> = builtin.chain(named).collect();
            list.sort_unstable_by_key(|(name, _)| *name);
            let entries = list
                .into_iter()
                .map(|(name, help)| (name.to_string(), Value::text(help)));
            return Ok(Value::Map(entries.collect()));
        }
        [ArgValue::String(name)] => name,
        _ => unreachable!("commands takes a command's name"),
    };
    if let Some(spec) = COMMANDS.iter().find(|spec| spec.name == name) {
        return Ok(builtin(spec));
    }
    (named.iter().find(|command| command.name == *name))
        .map(named_command)
        .ok_or_else(|| format!("unknown command '{name}'"))
}

/// A command of the registry, in detail
fn builtin(spec: &CommandSpec) -> Value {
    let kind = match spec.target {
        Target::Action(_) | Target::Build(_) => "action",
        Target::Query(_) | Target::Popup { .. } => "query",
    };
    let asks = match spec.asks {
        Asks::Never => "never",
        Asks::Always => "always",
        Asks::Bare => "bare",
    };
    let access = access_word(spec.access);
    let args = spec
        .args
        .iter()
        .map(|arg| (arg.name.to_string(), argument(arg)));
    Value::Map(vec![
        ("name".to_string(), Value::text(spec.name)),
        ("type".to_string(), Value::text(kind)),
        ("usage".to_string(), Value::text(spec.usage())),
        ("help".to_string(), Value::text(spec.help)),
        ("asks".to_string(), Value::text(asks)),
        ("access".to_string(), Value::text(access)),
        ("args".to_string(), Value::Map(args.collect())),
    ])
}

/// An argument: its kind, whether it can be left out, a number's bounds and suggestions, an
/// enum's values
fn argument(arg: &ArgSpec) -> Value {
    let texts = |values: &[&str]| Value::List(values.iter().copied().map(Value::text).collect());
    let mut entries = vec![
        ("kind".to_string(), Value::text(arg.kind.name())),
        ("optional".to_string(), Value::Bool(arg.optional)),
    ];
    match arg.kind {
        ArgKind::Int {
            min,
            max,
            suggestions,
        } => {
            entries.push(("min".to_string(), Value::Int(min)));
            entries.push(("max".to_string(), Value::Int(max)));
            if !suggestions.is_empty() {
                entries.push(("suggestions".to_string(), texts(suggestions)));
            }
        }
        ArgKind::Enum(values) => entries.push(("values".to_string(), texts(values))),
        _ => {}
    }
    Value::Map(entries)
}

/// An access as the API's levels name it
fn access_word(access: Access) -> &'static str {
    match access {
        Access::Read => "read",
        Access::Control => "control",
        Access::Device => "device",
    }
}

/// A command of `[commands]`: its chain, no arguments, `asks` always when one of its
/// commands needs the user, and the widest access of its commands
fn named_command(command: &NamedCommand) -> Value {
    let asks = (command.invocations.iter()).any(|invocation| invocation.asks_user().is_some());
    let access = (command.invocations.iter())
        .map(Invocation::access)
        .max()
        .unwrap_or(Access::Read);
    Value::Map(vec![
        ("name".to_string(), Value::text(&command.name)),
        ("type".to_string(), Value::text("named")),
        ("usage".to_string(), Value::text(&command.name)),
        ("help".to_string(), Value::text(command.description())),
        ("run".to_string(), Value::text(&command.run)),
        (
            "asks".to_string(),
            Value::text(if asks { "always" } else { "never" }),
        ),
        ("access".to_string(), Value::text(access_word(access))),
        ("args".to_string(), Value::Map(Vec::new())),
    ])
}

/// `get [setting]` (MAX-283): a setting's value, or bare every setting's by name. A switch is
/// a yes or no, the others the word the settings file takes, the plot its name or `off`
pub fn get(app: &mut App, args: &[ArgValue]) -> Result<Value, String> {
    match args {
        [] => Ok(Value::Map(
            (SettingKey::ALL.into_iter())
                .map(|key| (key.name().to_string(), setting(app, key)))
                .collect(),
        )),
        [ArgValue::Enum(name)] => {
            let key = SettingKey::named(name).expect("get takes one of SettingKey::NAMES");
            Ok(setting(app, key))
        }
        _ => unreachable!("get takes a setting's name"),
    }
}

/// A setting's value as `get` gives it: on/off, or its word (MAX-284: `setting-changed` too)
pub fn setting(app: &App, key: SettingKey) -> Value {
    if let Some(on) = app.setting_is_on(key) {
        return Value::Bool(on);
    }
    let settings = app.settings();
    let line_mode = &settings.line_mode;
    match key {
        SettingKey::Mode => Value::text(settings.mode.as_str()),
        SettingKey::LineWrap => config_word(line_mode.wrap_mode),
        SettingKey::IdentWrap => config_word(line_mode.wrap_indent),
        SettingKey::PrefixPlace => config_word(line_mode.prefix_place),
        SettingKey::TextEncoding => config_word(settings.encoding),
        SettingKey::DisplayMode => config_word(line_mode.display_mode),
        SettingKey::DisplayRaw => config_word(settings.raw_side.view),
        SettingKey::DimensionMode => config_word(settings.terminal_mode.dimension_mode),
        SettingKey::TxEol => config_word(line_mode.tx_eol),
        SettingKey::Backspace => config_word(settings.terminal_mode.backspace),
        SettingKey::Plot => Value::text(app.plot().map_or("off", |plot| plot.name.as_str())),
        SettingKey::LocalEcho
        | SettingKey::DisplayTimestamp
        | SettingKey::Mouse
        | SettingKey::AnswerQueries
        | SettingKey::ByteTrace
        | SettingKey::AddCr
        | SettingKey::AddLf => unreachable!("a switch, read above"),
    }
}

/// A setting's value as the settings file writes it: `word`, `grow_only`
fn config_word(value: impl serde::Serialize) -> Value {
    match toml::Value::try_from(value) {
        Ok(toml::Value::String(word)) => Value::Text(word),
        _ => unreachable!("a setting's enum is written as a word"),
    }
}

/// `marks` (MAX-283), for a trigger, a profile or a client (the user gets the popup): each
/// view's marks by letter, with their line (a history entry's index in line mode, a world
/// row's in terminal mode), its time when known, and its text
pub fn marks(app: &mut App, _args: &[ArgValue]) -> Result<Value, String> {
    let mut view = |mode| Value::Map(app.mark_rows(mode).into_iter().map(mark).collect());
    Ok(Value::Map(vec![
        ("line".to_string(), view(Mode::Line)),
        ("terminal".to_string(), view(Mode::Terminal)),
    ]))
}

fn mark(row: MarkRow) -> (String, Value) {
    let mut entries = vec![("line".to_string(), Value::Int(row.line as i64))];
    if let Some(at) = row.time {
        entries.push(("time".to_string(), time(at)));
    }
    entries.push(("text".to_string(), Value::Text(row.text)));
    (row.letter.to_string(), Value::Map(entries))
}

/// `triggers` (MAX-283), for a trigger, a profile or a client (the user gets the popup):
/// each trigger by name, its text and chain, whether it is armed, rearms, is silent, the
/// mark it puts, how many times it fired and when it last did
pub fn triggers(app: &mut App, _args: &[ArgValue]) -> Result<Value, String> {
    let triggers = app.triggers().iter().map(|trigger| {
        let mut entries = vec![
            ("text".to_string(), Value::text(&trigger.text)),
            ("run".to_string(), Value::text(&trigger.run)),
            ("armed".to_string(), Value::Bool(trigger.is_armed())),
            ("rearm".to_string(), Value::Bool(trigger.rearm)),
            ("silent".to_string(), Value::Bool(trigger.silent)),
            (
                "mark".to_string(),
                Value::text(trigger.mark.map_or("none".to_string(), String::from)),
            ),
            ("hits".to_string(), Value::Int(i64::from(trigger.hits))),
        ];
        if let Some(at) = trigger.last_hit {
            entries.push(("last".to_string(), time(at)));
        }
        (trigger.name.clone(), Value::Map(entries))
    });
    Ok(Value::Map(triggers.collect()))
}

/// A time as the getters give it (MAX-283): UTC, to the millisecond, `2026-11-24T21:20:52.854Z`
fn time(at: jiff::Timestamp) -> Value {
    Value::text(utc(at))
}

/// A time as the API writes it, the getters' and the API's own (MAX-74)
pub fn utc(at: jiff::Timestamp) -> String {
    format!("{at:.3}")
}

/// How many lines a getter gives at most, and by default (MAX-283)
pub const MAX_COUNT: i64 = 500;
const DEFAULT_COUNT: usize = 50;

/// The lines `[from] [count]` picks out of `len`, `from` counted back from the newest: the
/// range, oldest first, and whether older ones are left out
fn page(len: usize, args: &[ArgValue]) -> (std::ops::Range<usize>, bool) {
    let size = |n: i64| usize::try_from(n).unwrap_or(usize::MAX);
    let (from, count) = match args {
        [] => (0, DEFAULT_COUNT),
        [ArgValue::Int(from)] => (size(*from), DEFAULT_COUNT),
        [ArgValue::Int(from), ArgValue::Int(count)] => (size(*from), size(*count)),
        _ => unreachable!("[from] [count] are numbers"),
    };
    let end = len.saturating_sub(from);
    let start = end.saturating_sub(count);
    (start..end, start > 0)
}

/// `notifications [from] [count]` (MAX-283), for a trigger, a profile or a client (the user
/// gets the popup): the notification history's lines, oldest first, the last `count` (50 by
/// default, 500 at most) after skipping `from` newest; how many there are, and `cut` when
/// older ones were left out
pub fn notifications(app: &mut App, args: &[ArgValue]) -> Result<Value, String> {
    let history = app.notifications.history();
    let (range, cut) = page(history.len(), args);
    let notices = history[range].iter().map(|notice| {
        Value::Map(vec![
            ("time".to_string(), time(notice.time)),
            ("level".to_string(), Value::text(notice.level.word())),
            ("text".to_string(), Value::text(&notice.message)),
        ])
    });
    Ok(Value::Map(vec![
        ("total".to_string(), Value::Int(history.len() as i64)),
        ("cut".to_string(), Value::Bool(cut)),
        ("notices".to_string(), Value::List(notices.collect())),
    ]))
}

/// `history [from] [count]` (MAX-283): line mode's history, the last `count` lines (50 by
/// default, 500 at most) after skipping `from` newest, oldest first, in the shape of every
/// stream (`stream_value`, MAX-290): each line with its number (`line`), its time when known
/// and its text. The line still coming in is `partial-line`'s
pub fn history(app: &mut App, args: &[ArgValue]) -> Result<Value, String> {
    fed(app, Mode::Line)?;
    let (from, count) = back_args(args, DEFAULT_COUNT);
    let window = Window::back(app.line_mode.lines_kept(), from, count);
    Ok(history_value(app, window))
}

/// `history-since <line> [count]` (MAX-290): the lines from number `line` on, `count` at most
/// (50 by default, 500 at most), a client's poll: from the oldest kept when `line` is older
/// (`cut`), none when it is past the newest
pub fn history_since(app: &mut App, args: &[ArgValue]) -> Result<Value, String> {
    fed(app, Mode::Line)?;
    let (since, count) = since_args(args, DEFAULT_COUNT);
    let window = Window::since(app.line_mode.lines_kept(), since, count);
    Ok(history_value(app, window))
}

/// The lines of `window`
fn history_value(app: &App, window: Window) -> Value {
    let line_mode = &app.line_mode;
    let first = line_mode.first_line();
    let entries = window.numbers().map(|line| {
        let entry = &line_mode.history()[(line - first) as usize];
        line_value(line, entry.timestamp, &entry.text)
    });
    let entries = ("entries".to_string(), Value::List(entries.collect()));
    stream_value(line_mode.lines_kept(), window.cut, [entries])
}

/// A line as the line mode getters give it: its number, its time when known, its text without
/// its end of line
fn line_value(line: u64, at: Option<jiff::Timestamp>, text: &str) -> Value {
    let mut entries = vec![("line".to_string(), int(line))];
    if let Some(at) = at {
        entries.push(("time".to_string(), time(at)));
    }
    let text = text.trim_end_matches(['\r', '\n']);
    entries.push(("text".to_string(), Value::text(text)));
    Value::Map(entries)
}

/// `partial-line` (MAX-290): the line still coming in, line mode's live part as `screen` is
/// terminal mode's: the number it gets once ended (`newest`), the time it started when known,
/// its text so far (empty when none): a prompt without its end of line is there
pub fn partial_line(app: &mut App, _args: &[ArgValue]) -> Result<Value, String> {
    fed(app, Mode::Line)?;
    let line_mode = &app.line_mode;
    let text = app.settings().encoding.decode(line_mode.partial_rx());
    let line = line_mode.lines_kept().end;
    Ok(line_value(line, line_mode.partial_time(), &text))
}

/// `history-size` (MAX-283): the lines line mode's history holds, the ones its cap dropped
/// since the start, and the ones `clear-history` took away (MAX-290)
pub fn history_size(app: &mut App, _args: &[ArgValue]) -> Result<Value, String> {
    fed(app, Mode::Line)?;
    let line_mode = &app.line_mode;
    Ok(Value::Map(vec![
        (
            "lines".to_string(),
            Value::Int(line_mode.history().len() as i64),
        ),
        (
            "dropped".to_string(),
            Value::Int(line_mode.dropped_lines() as i64),
        ),
        (
            "cleared".to_string(),
            Value::Int(line_mode.cleared_lines() as i64),
        ),
    ]))
}

/// `rows [text|colors] [from] [count]` (MAX-290): the terminal's rows that scrolled into the
/// scrollback, the last `count` (50 by default, 500 at most) after skipping `from` newest, in
/// the shape of every stream (`stream_value`): each with its number (`row`), its text (its
/// colors as escapes with `colors`) and whether the terminal wrapped it into the next row. The
/// screen's rows can still change: `screen` gives them
pub fn rows(app: &mut App, args: &[ArgValue]) -> Result<Value, String> {
    fed(app, Mode::Terminal)?;
    let (styled, args) = match args {
        [ArgValue::Enum(format), rest @ ..] => (*format == "colors", rest),
        _ => (false, args),
    };
    let (from, count) = back_args(args, DEFAULT_COUNT);
    let window = Window::back(app.terminal_mode.rows_kept(), from, count);
    Ok(rows_value(app, window, styled))
}

/// `rows-since <row> [count] [text|colors]` (MAX-290): the scrolled rows from number `row` on,
/// `count` at most (50 by default, 500 at most), a client's poll: from the oldest kept when
/// `row` is older (`cut`), none when it is past the newest. Nothing scrolls while a full-screen
/// program runs on the alternate screen
pub fn rows_since(app: &mut App, args: &[ArgValue]) -> Result<Value, String> {
    fed(app, Mode::Terminal)?;
    let (styled, args) = match args {
        [rest @ .., ArgValue::Enum(format)] => (*format == "colors", rest),
        _ => (false, args),
    };
    let (since, count) = since_args(args, DEFAULT_COUNT);
    let window = Window::since(app.terminal_mode.rows_kept(), since, count);
    Ok(rows_value(app, window, styled))
}

fn rows_value(app: &mut App, window: Window, styled: bool) -> Value {
    let rows = app.terminal_mode.scrolled_rows(window.numbers(), styled);
    let entries = rows.into_iter().map(|row| {
        Value::Map(vec![
            ("row".to_string(), int(row.number)),
            ("text".to_string(), Value::Text(row.text)),
            ("wrapped".to_string(), Value::Bool(row.wrapped)),
        ])
    });
    let entries = ("entries".to_string(), Value::List(entries.collect()));
    stream_value(app.terminal_mode.rows_kept(), window.cut, [entries])
}

/// `screen [text|colors]` (MAX-283): the terminal's live screen, its rows on lines, plain or
/// with its colors and styles as escape sequences; the view scrolled back doesn't change it
pub fn screen(app: &mut App, args: &[ArgValue]) -> Result<Value, String> {
    fed(app, Mode::Terminal)?;
    let styled = match args {
        [] | [ArgValue::Enum("text")] => false,
        [ArgValue::Enum("colors")] => true,
        _ => unreachable!("screen takes text or colors"),
    };
    Ok(Value::Text(app.terminal_mode.live_screen(styled)))
}

/// `cursor` (MAX-283): the terminal cursor's row and column on the live screen, from 0, and
/// whether the remote shows it
pub fn cursor(app: &mut App, _args: &[ArgValue]) -> Result<Value, String> {
    fed(app, Mode::Terminal)?;
    let (row, col, visible) = app.terminal_mode.cursor();
    Ok(Value::Map(vec![
        ("row".to_string(), Value::Int(i64::from(row))),
        ("col".to_string(), Value::Int(i64::from(col))),
        ("visible".to_string(), Value::Bool(visible)),
    ]))
}

/// The bytes `raw` gives by default, and at most (MAX-284)
pub const RAW_COUNT: usize = 4096;
pub const MAX_RAW: i64 = 64 * 1024;

/// `raw [from] [count]` (MAX-284): the last bytes received in any mode (MAX-290), `count`
/// (4096 by default, 64 KiB at most) after skipping `from` newest, in the shape of every
/// stream (`stream_value`): their first byte's offset (`start`) and the bytes
pub fn raw(app: &mut App, args: &[ArgValue]) -> Result<Value, String> {
    let (from, count) = back_args(args, RAW_COUNT);
    let window = Window::back(bytes_kept(app), from, count);
    Ok(raw_value(app, window))
}

/// `raw-since <offset> [count]` (MAX-284): the bytes received from `offset` on, a client's
/// poll: from the oldest kept when `offset` is older (`cut`), none when it is past the end
pub fn raw_since(app: &mut App, args: &[ArgValue]) -> Result<Value, String> {
    let (since, count) = since_args(args, RAW_COUNT);
    let window = Window::since(bytes_kept(app), since, count);
    Ok(raw_value(app, window))
}

/// The offsets of the bytes received still kept
fn bytes_kept(app: &App) -> std::ops::Range<u64> {
    app.rx_bytes.start()..app.rx_bytes.end()
}

fn raw_value(app: &App, window: Window) -> Value {
    let bytes = app.rx_bytes.bytes(window.numbers());
    let items = [
        ("start".to_string(), int(window.start)),
        ("bytes".to_string(), Value::Bytes(bytes)),
    ];
    stream_value(bytes_kept(app), window.cut, items)
}

/// What a stream getter gives (MAX-290): the numbers `start..end` (lines, rows or byte offsets)
/// out of the ones kept, and whether some before `start` are left out (`cut`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Window {
    start: u64,
    end: u64,
    cut: bool,
}

impl Window {
    /// `count` at most after skipping `from` newest of `kept`: `cut` when older ones are kept
    fn back(kept: std::ops::Range<u64>, from: u64, count: u64) -> Self {
        let end = kept.end.saturating_sub(from).max(kept.start);
        let start = end.saturating_sub(count).max(kept.start);
        Self {
            start,
            end,
            cut: start > kept.start,
        }
    }

    /// `count` at most from `since` on: from the oldest of `kept` when `since` is older, `cut`
    /// then (they're gone), none when it is past the newest
    fn since(kept: std::ops::Range<u64>, since: u64, count: u64) -> Self {
        let start = since.clamp(kept.start, kept.end);
        Self {
            start,
            end: start.saturating_add(count).min(kept.end),
            cut: since < kept.start,
        }
    }

    fn numbers(self) -> std::ops::Range<u64> {
        self.start..self.end
    }
}

/// The shape every stream getter answers with, `raw`, `history` and `rows` alike (MAX-290):
/// the `oldest` number kept and the `newest`, the one the next will get (a poll goes on from
/// it), `cut` (see `Window`), then `items`, what was asked for
fn stream_value(
    kept: std::ops::Range<u64>,
    cut: bool,
    items: impl IntoIterator<Item = (String, Value)>,
) -> Value {
    let mut entries = vec![
        ("oldest".to_string(), int(kept.start)),
        ("newest".to_string(), int(kept.end)),
        ("cut".to_string(), Value::Bool(cut)),
    ];
    entries.extend(items);
    Value::Map(entries)
}

/// `[from] [count]`, `count` by default
fn back_args(args: &[ArgValue], count: usize) -> (u64, u64) {
    match args {
        [] => (0, count as u64),
        [ArgValue::Int(from)] => (number(*from), count as u64),
        [ArgValue::Int(from), ArgValue::Int(count)] => (number(*from), number(*count)),
        _ => unreachable!("[from] [count] are numbers"),
    }
}

/// `<since> [count]`, `count` by default
fn since_args(args: &[ArgValue], count: usize) -> (u64, u64) {
    match args {
        [ArgValue::Int(since)] => (number(*since), count as u64),
        [ArgValue::Int(since), ArgValue::Int(count)] => (number(*since), number(*count)),
        _ => unreachable!("<since> [count] are numbers"),
    }
}

/// An argument checked as positive by its spec, as a number or a count
fn number(n: i64) -> u64 {
    u64::try_from(n).unwrap_or(0)
}

/// A number as a value
fn int(n: u64) -> Value {
    Value::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

/// A getter of a mode's state needs that mode fed (MAX-284): a mode the received bytes don't
/// go to holds what it had when they stopped, so it is refused, saying why
fn fed(app: &App, mode: Mode) -> Result<(), String> {
    let (line, terminal) = app.fed_modes();
    let (fed, name) = match mode {
        Mode::Line => (line, "line mode"),
        Mode::Terminal => (terminal, "terminal mode"),
    };
    if fed {
        Ok(())
    } else if !app.settings().mode_enabled(mode) {
        Err(format!("{name} is disabled (disabled_mode)"))
    } else {
        Err(format!(
            "{name} isn't fed while it's hidden (dual_mode_buffering = active_only)"
        ))
    }
}

/// `term-size` (MAX-283): the terminal emulator's rows and columns, the size the remote is
/// told
pub fn term_size(app: &mut App, _args: &[ArgValue]) -> Result<Value, String> {
    let (rows, cols) = app.terminal_mode.get_vt100_size();
    Ok(Value::Map(vec![
        ("rows".to_string(), Value::Int(i64::from(rows))),
        ("cols".to_string(), Value::Int(i64::from(cols))),
    ]))
}

#[cfg(test)]
#[path = "../tests/commands/query.rs"]
mod tests;
