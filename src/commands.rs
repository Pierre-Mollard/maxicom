/// Command registry and parser (MAX-12): the one table of named commands that keys, the
/// command line and later the API (MAX-74) run. The parser and the completion are pure, but for
/// the paths read from the disk; the commands with arguments build their actions in `build`
use std::path::{Path, PathBuf};

use crate::{
    app::App, configs::KeyBindingAction, inputs::Action, link::Address, paths::expand_home,
};

pub mod build;
pub mod tokenizer;

use tokenizer::Word;

/// `Bytes` is any bytes, the word as the tokenizer made it (escapes and all), not only UTF-8.
/// `Address` is a link's address, checked when the line is read (MAX-168)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    Int {
        min: i64,
        max: i64,
        suggestions: &'static [&'static str],
    },
    Bool,
    Enum(&'static [&'static str]),
    String,
    Path,
    Address,
    Hex,
    Bytes,
    Trigger,
    Link,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgValue {
    Int(i64),
    Bool(bool),
    Enum(&'static str),
    String(String),
    Path(PathBuf),
    Address(Address),
    Bytes(Vec<u8>),
}

/// One argument of a command. Optional ones come last
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArgSpec {
    pub name: &'static str,
    pub kind: ArgKind,
    pub optional: bool,
}

/// What a command runs: a keymap action, or the action built from its arguments (checked
/// against its spec), refused with the reason when the link can't do it
#[derive(Debug, Clone, Copy)]
pub enum Target {
    Action(KeyBindingAction),
    Build(fn(&App, &[ArgValue]) -> Result<Action, String>),
}

#[derive(Debug)]
pub struct CommandSpec {
    pub name: &'static str,
    pub args: &'static [ArgSpec],
    pub help: &'static str,
    pub target: Target,
}

/// A spec is one entry of a registry: the same spec is the same entry
impl PartialEq for CommandSpec {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for CommandSpec {}

impl CommandSpec {
    const fn action(name: &'static str, action: KeyBindingAction, help: &'static str) -> Self {
        Self {
            name,
            args: &[],
            help,
            target: Target::Action(action),
        }
    }

    const fn build(
        name: &'static str,
        args: &'static [ArgSpec],
        build: fn(&App, &[ArgValue]) -> Result<Action, String>,
        help: &'static str,
    ) -> Self {
        Self {
            name,
            args,
            help,
            target: Target::Build(build),
        }
    }

    /// The name and its arguments, `<required>` and `[optional]`, for the help
    pub fn usage(&self) -> String {
        let mut usage = self.name.to_string();
        for arg in self.args {
            if arg.optional {
                usage.push_str(&format!(" [{}]", arg.name));
            } else {
                usage.push_str(&format!(" <{}>", arg.name));
            }
        }
        usage
    }
}

const fn arg(name: &'static str, kind: ArgKind) -> ArgSpec {
    ArgSpec {
        name,
        kind,
        optional: false,
    }
}

/// A parsed command, its arguments checked against its spec
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub spec: &'static CommandSpec,
    pub args: Vec<ArgValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    UnclosedQuote,
    BadEscape(String),
    UnknownCommand(String),
    MissingArgument {
        command: &'static str,
        name: &'static str,
    },
    TooManyArguments {
        command: &'static str,
        max: usize,
    },
    BadArgument {
        command: &'static str,
        index: usize,
        reason: String,
    },
    /// Refused when it ran: the link can't do it
    Failed {
        command: &'static str,
        reason: String,
    },
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnclosedQuote => write!(f, "unclosed quote"),
            Self::BadEscape(escape) => write!(f, "bad escape '{escape}'"),
            Self::UnknownCommand(name) => write!(f, "unknown command '{name}'"),
            Self::MissingArgument { command, name } => {
                write!(f, "{command}: missing argument <{name}>")
            }
            Self::TooManyArguments { command, max: 0 } => write!(f, "{command} takes no argument"),
            Self::TooManyArguments { command, max } => {
                write!(f, "{command} takes {max} argument(s) at most")
            }
            Self::BadArgument {
                command,
                index,
                reason,
            } => write!(f, "{command}: bad argument {index}: {reason}"),
            Self::Failed { command, reason } => write!(f, "{command}: {reason}"),
        }
    }
}

impl std::error::Error for CommandError {}

pub static COMMANDS: &[CommandSpec] = {
    use KeyBindingAction as A;
    &[
        CommandSpec::action("quit", A::Quit, "Quit maxicom"),
        CommandSpec::action(
            "toggle-input-mode",
            A::SKInputMode,
            "Switch line / terminal mode",
        ),
        CommandSpec::action("toggle-local-echo", A::SKLocalEcho, "Toggle local echo"),
        CommandSpec::action("toggle-line-wrap", A::SKLineWrap, "Toggle line wrap"),
        CommandSpec::action("toggle-add-cr", A::SKAddCr, "Read a received LF as CR LF"),
        CommandSpec::action("toggle-add-lf", A::SKAddLf, "Read a received CR as CR LF"),
        CommandSpec::action(
            "toggle-display-timestamp",
            A::SKDisplayTimestamp,
            "Toggle timestamps",
        ),
        CommandSpec::action(
            "toggle-text-encoding",
            A::SKTextEncoding,
            "Cycle the text encoding",
        ),
        CommandSpec::action(
            "toggle-indent-wrap",
            A::SKIndentWrap,
            "Toggle indented wrap",
        ),
        CommandSpec::action(
            "toggle-display-mode",
            A::SKDisplayMode,
            "Cycle the display mode",
        ),
        CommandSpec::action(
            "toggle-display-raw",
            A::SKDisplayRaw,
            "Toggle the raw side view",
        ),
        CommandSpec::action(
            "screenshot-bytes",
            A::ScreenshotBytes,
            "Screenshot the terminal with its colors",
        ),
        CommandSpec::action(
            "screenshot-text",
            A::ScreenshotTxt,
            "Screenshot the terminal as plain text",
        ),
        CommandSpec::action("help", A::Help, "Open the help"),
        CommandSpec::action("settings", A::Settings, "Open the settings"),
        CommandSpec::action("record", A::Record, "Start / stop recording (text or full)"),
        CommandSpec::action("resize-dialog", A::ResizeDialog, "Set the terminal size"),
        CommandSpec::action(
            "auto-resize-remote",
            A::AutoResizeRemote,
            "Resize the remote to the emulator",
        ),
        CommandSpec::action(
            "auto-resize-remote-line",
            A::AutoResizeRemoteLine,
            "Resize the remote to the line view width",
        ),
        CommandSpec::action(
            "auto-resize-emulator",
            A::AutoResizeEmulator,
            "Resize the emulator to the view",
        ),
        CommandSpec::action("link-settings", A::LinkSettings, "Open the link settings"),
        CommandSpec::action("enter-copy-mode", A::EnterCopyMode, "Enter copy mode"),
        CommandSpec::action("enter-search", A::EnterSearch, "Search the history"),
        CommandSpec::action("scroll-up", A::ScrollUp, "Scroll up one line"),
        CommandSpec::action("scroll-down", A::ScrollDown, "Scroll down one line"),
        CommandSpec::action(
            "scroll-to-bottom",
            A::ScrollToBottom,
            "Scroll to the bottom",
        ),
        CommandSpec::action("scroll-to-top", A::ScrollToTop, "Scroll to the top"),
        CommandSpec::action("page-up", A::PageUp, "Scroll up one page"),
        CommandSpec::action("page-down", A::PageDown, "Scroll down one page"),
        CommandSpec::action("pan-right", A::PanRight, "Pan right"),
        CommandSpec::action("pan-left", A::PanLeft, "Pan left"),
        CommandSpec::action("pan-home", A::PanHome, "Pan to the line start"),
        CommandSpec::action("pan-side-right", A::PanSideRight, "Pan the side view right"),
        CommandSpec::action("pan-side-left", A::PanSideLeft, "Pan the side view left"),
        CommandSpec::action("clear", A::Clear, "Clear the view, the history a scroll up"),
        CommandSpec::action("clear-history", A::ClearHistory, "Clear the history"),
        CommandSpec::action("sessions", A::Sessions, "Open the session backups"),
        CommandSpec::action(
            "directory",
            A::Directory,
            "Open the directory: the serial ports to switch to",
        ),
        CommandSpec::action("exit-prefix", A::ExitPrefix, "Leave the prefix"),
        CommandSpec::action("toggle-attach", A::ToggleAttach, "Detach / attach the link"),
        CommandSpec::action("reload-config", A::ReloadConfig, "Reload the config files"),
        CommandSpec::action("toggle-mouse", A::ToggleMouse, "Toggle mouse support"),
        CommandSpec::action("command-line", A::CommandLine, "Open the command line"),
        CommandSpec::action(
            "cancel",
            A::CancelCommands,
            "Stop the file transfer and queued commands",
        ),
        CommandSpec::action(
            "toggle-triggers",
            A::ToggleTriggers,
            "Arm / disarm the triggers",
        ),
        CommandSpec::action("triggers", A::Triggers, "Open the triggers"),
        CommandSpec::build(
            "send",
            &[arg("text", ArgKind::Bytes)],
            build::send,
            "Send text, with escapes: \\r \\n \\t \\xNN",
        ),
        CommandSpec::build(
            "send-hex",
            &[arg("bytes", ArgKind::Hex)],
            build::send,
            "Send bytes written in hex: 0d0a",
        ),
        CommandSpec::build(
            "send-file",
            &[
                arg("path", ArgKind::Path),
                ArgSpec {
                    name: "line-delay-ms",
                    kind: ArgKind::Int {
                        min: 0,
                        max: 60_000,
                        suggestions: &[],
                    },
                    optional: true,
                },
            ],
            build::send_file,
            "Send a file at the baud rate, waiting after each line",
        ),
        CommandSpec::build(
            "open",
            &[arg("address", ArgKind::Address)],
            build::open,
            "Switch to a saved link, a port (its settings kept), tcp://host:port or file://path",
        ),
        CommandSpec::build(
            "link-save",
            &[ArgSpec {
                name: "name",
                kind: ArgKind::String,
                optional: true,
            }],
            build::link_save,
            "Save the link in use to links.toml, named after its address when no name is given",
        ),
        CommandSpec::build(
            "link-delete",
            &[arg("name", ArgKind::Link)],
            build::link_delete,
            "Delete a saved link from links.toml",
        ),
        CommandSpec::build(
            "replay",
            &[
                arg("path", ArgKind::Path),
                ArgSpec {
                    name: "delay-ms",
                    kind: ArgKind::Int {
                        min: 0,
                        max: 10_000,
                        suggestions: &[],
                    },
                    optional: true,
                },
            ],
            build::replay,
            "Replay a raw capture in place of the link, a buffer every delay",
        ),
        CommandSpec::build(
            "replay-stop",
            &[],
            build::replay_stop,
            "Stop the replay, back to the link it replaced",
        ),
        CommandSpec::build("replay-pause", &[], build::replay_pause, "Pause the replay"),
        CommandSpec::build(
            "replay-resume",
            &[],
            build::replay_resume,
            "Resume the paused replay",
        ),
        CommandSpec::build(
            "replay-speed",
            &[ArgSpec {
                name: "delay-ms",
                kind: ArgKind::Int {
                    min: 0,
                    max: 10_000,
                    suggestions: &[],
                },
                optional: false,
            }],
            build::replay_speed,
            "Set the replay's delay between two buffers",
        ),
        CommandSpec::build(
            "replay-restart",
            &[],
            build::replay_restart,
            "Play the replay again from the start, on a clean screen",
        ),
        CommandSpec::build(
            "sleep",
            &[arg(
                "ms",
                ArgKind::Int {
                    min: 1,
                    max: 60_000,
                    suggestions: &[],
                },
            )],
            build::pause_commands,
            "Pause the rest of the line, the screen stays live",
        ),
        CommandSpec::build(
            "dtr",
            &[arg("state", ArgKind::Bool)],
            build::dtr,
            "Set the DTR line: on or off",
        ),
        CommandSpec::build(
            "rts",
            &[arg("state", ArgKind::Bool)],
            build::rts,
            "Set the RTS line: on or off",
        ),
        CommandSpec::build(
            "break",
            &[ArgSpec {
                name: "ms",
                kind: ArgKind::Int {
                    min: 1,
                    max: 10_000,
                    suggestions: &[],
                },
                optional: true,
            }],
            build::line_break,
            "Hold the line in break, 250 ms by default",
        ),
        CommandSpec::build(
            "baud",
            &[arg(
                "rate",
                ArgKind::Int {
                    min: 50,
                    max: 4_000_000,
                    suggestions: &[
                        "9600", "19200", "38400", "57600", "115200", "230400", "460800", "921600",
                    ],
                },
            )],
            build::baud,
            "Set the baud rate",
        ),
        CommandSpec::build(
            "framing",
            &[arg("framing", ArgKind::String)],
            build::framing,
            "Set data bits, parity and stop bits: 8N1",
        ),
        CommandSpec::build(
            "set",
            &[
                arg("setting", ArgKind::Enum(build::ON_OFF_SETTINGS)),
                arg("state", ArgKind::Bool),
            ],
            build::set,
            "Set local-echo, timestamps, mouse, answer-queries, add-cr or add-lf on or off",
        ),
        CommandSpec::build(
            "encoding",
            &[arg(
                "encoding",
                ArgKind::Enum(&["utf8", "ascii", "cp437", "latin1"]),
            )],
            build::encoding,
            "Read the received bytes as utf8, ascii, cp437 or latin1",
        ),
        CommandSpec::build(
            "search",
            &[arg("text", ArgKind::String)],
            build::search,
            "Search the history for the text",
        ),
        CommandSpec::build(
            "screenshot",
            &[arg("name", ArgKind::Path)],
            build::screenshot,
            "Save the terminal to a file: .ans keeps the colors",
        ),
        CommandSpec::build(
            "mash",
            &[
                arg("text", ArgKind::Bytes),
                arg("until", ArgKind::Bytes),
                ArgSpec {
                    name: "every-ms",
                    kind: ArgKind::Int {
                        min: 10,
                        max: 10_000,
                        suggestions: &[],
                    },
                    optional: true,
                },
                ArgSpec {
                    name: "timeout-ms",
                    kind: ArgKind::Int {
                        min: 100,
                        max: 600_000,
                        suggestions: &[],
                    },
                    optional: true,
                },
            ],
            build::mash,
            "Send text every 50 ms until a text comes in, 15 s at most",
        ),
        CommandSpec::build(
            "trigger",
            &[
                arg("state", ArgKind::Enum(&["on", "off", "toggle"])),
                ArgSpec {
                    name: "name",
                    kind: ArgKind::Trigger,
                    optional: true,
                },
            ],
            build::trigger,
            "Arm, disarm or toggle the triggers, or the one named",
        ),
        CommandSpec::build(
            "flow",
            &[arg("flow", ArgKind::Enum(&["none", "hw", "sw"]))],
            build::flow,
            "Set flow control: none, hw (RTS/CTS), sw (XON/XOFF)",
        ),
    ]
};

pub fn find_by_name(name: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|spec| spec.name == name)
}

pub fn find_by_action(action: KeyBindingAction) -> Option<&'static CommandSpec> {
    COMMANDS
        .iter()
        .find(|spec| matches!(spec.target, Target::Action(a) if a == action))
}

/// A value to complete a word with, and its help line (a command's, none for a value)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub value: String,
    pub help: &'static str,
}

/// What completes the word before the cursor: where the word starts in the line (in bytes), and
/// the candidates starting like it
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Completion {
    pub start: usize,
    pub candidates: Vec<Candidate>,
}

/// The completion of the last word of `before_cursor`, the line up to the cursor: the command
/// names (sorted) for a command's first word, else the values of the argument it is. None for
/// a word already whole, nor for one with a quote or an escape. `link_names` are this
/// machine's saved links (MAX-185), first for an address; `ports` the serial ports the system
/// lists, for an address
pub fn complete(
    before_cursor: &str,
    trigger_names: &[String],
    link_names: &[String],
    ports: &[String],
) -> Completion {
    complete_in(COMMANDS, before_cursor, trigger_names, link_names, ports)
}

fn complete_in(
    commands: &'static [CommandSpec],
    line: &str,
    trigger_names: &[String],
    link_names: &[String],
    ports: &[String],
) -> Completion {
    let Some(word) = last_word(line) else {
        return Completion::default();
    };
    let prefix = &line[word.start..];
    let mut candidates: Vec<Candidate> = match word.command {
        None => {
            let mut specs: Vec<_> = commands
                .iter()
                .filter(|spec| spec.name.starts_with(prefix))
                .collect();
            specs.sort_unstable_by_key(|spec| spec.name);
            specs
                .into_iter()
                .map(|spec| Candidate {
                    value: spec.name.to_string(),
                    help: spec.help,
                })
                .collect()
        }
        Some(name) => commands
            .iter()
            .find(|spec| spec.name == name)
            .and_then(|spec| spec.args.get(word.index - 1))
            .map(|arg| match arg.kind {
                ArgKind::Path => path_candidates(prefix),
                ArgKind::Address => address_candidates(prefix, link_names, ports),
                ArgKind::Trigger => (trigger_names.iter())
                    .map(|name| escape_word(name))
                    .filter(|name| name.starts_with(prefix))
                    .collect(),
                ArgKind::Link => (link_names.iter())
                    .map(|name| escape_word(name))
                    .filter(|name| name.starts_with(prefix))
                    .collect(),
                kind => kind
                    .candidates(prefix)
                    .into_iter()
                    .map(String::from)
                    .collect(),
            })
            .unwrap_or_default()
            .into_iter()
            .map(|value| Candidate { value, help: "" })
            .collect(),
    };
    if let [only] = &candidates[..]
        && only.value == prefix
    {
        candidates.clear();
    }
    Completion {
        start: word.start,
        candidates,
    }
}

/// The entries of the folder `prefix` names (the current one when none, `~` the home folder)
/// starting like its last part, written as `prefix` with them: a folder with a `/` after it,
/// the characters the tokenizer splits on escaped. Hidden entries only for a part starting
/// with a `.`; the names that aren't UTF-8 or have other blanks than spaces are left out
fn path_candidates(prefix: &str) -> Vec<String> {
    let (folder, part) = prefix.split_at(prefix.rfind('/').map_or(0, |i| i + 1));
    let dir = match folder {
        "" => PathBuf::from("."),
        folder => expand_home(Path::new(folder)),
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut candidates: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let hidden = name.starts_with('.') && !part.starts_with('.');
            let blank = name.chars().any(|c| c.is_whitespace() && c != ' ');
            if !name.starts_with(part) || hidden || blank {
                return None;
            }
            // through a link to a folder too
            let slash = if entry.path().is_dir() { "/" } else { "" };
            Some(format!("{folder}{}{slash}", escape_word(&name)))
        })
        .collect();
    candidates.sort_unstable();
    candidates
}

/// An address's completion (MAX-168): the saved links' names (MAX-185), the `tcp://` and
/// `file://` schemes and the serial ports starting like it, then the disk's entries for a path
/// being typed. After `file://`, the paths only; after `tcp://`, nothing (no host is known)
fn address_candidates(prefix: &str, link_names: &[String], ports: &[String]) -> Vec<String> {
    if let Some(path) = prefix.strip_prefix("file://") {
        return (path_candidates(path).into_iter())
            .map(|path| format!("file://{path}"))
            .collect();
    }
    if prefix.starts_with("tcp://") {
        return Vec::new();
    }
    let schemes = ["tcp://", "file://"].into_iter().map(String::from);
    let mut candidates: Vec<String> = (link_names.iter().map(|name| escape_word(name)))
        .chain(schemes)
        .chain(ports.iter().map(|port| escape_word(port)))
        .filter(|candidate| candidate.starts_with(prefix))
        .collect();
    if prefix.contains('/') {
        for path in path_candidates(prefix) {
            if !candidates.contains(&path) {
                candidates.push(path);
            }
        }
    }
    candidates
}

/// `text` as one word for the tokenizer: its spaces, quotes, `;` and `\` escaped
fn escape_word(text: &str) -> String {
    let mut word = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, ' ' | '"' | '\'' | ';' | '\\') {
            word.push('\\');
        }
        word.push(c);
    }
    word
}

/// The last word of a line, as the tokenizer splits it
struct LastWord<'a> {
    start: usize,
    /// Its place in its command, `0` the command's name
    index: usize,
    /// Its command's name, when it isn't the name
    command: Option<&'a str>,
}

/// None when the word has a quote or an escape: it isn't text a candidate starts like
fn last_word(line: &str) -> Option<LastWord<'_>> {
    let mut start = 0;
    let mut index = 0;
    let mut in_word = false;
    let mut command = None;
    let mut quote = None;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if let Some(open) = quote {
            if c == open {
                quote = None;
            } else if c == '\\' && open == '"' {
                escaped = true;
            }
            continue;
        }
        match c {
            ';' => {
                index = 0;
                in_word = false;
                command = None;
                start = i + 1;
            }
            c if c.is_whitespace() => {
                if in_word {
                    if index == 0 {
                        command = Some(&line[start..i]);
                    }
                    index += 1;
                    in_word = false;
                }
                start = i + c.len_utf8();
            }
            c => {
                if !in_word {
                    in_word = true;
                    start = i;
                }
                match c {
                    '\\' => escaped = true,
                    '\'' | '"' => quote = Some(c),
                    _ => {}
                }
            }
        }
    }
    let word = &line[start..];
    if quote.is_some() || escaped || word.contains(['\\', '\'', '"']) {
        return None;
    }
    Some(LastWord {
        start,
        index,
        command,
    })
}

/// Parses a command line: commands chained with `;`, each checked against the registry
pub fn parse(line: &str) -> Result<Vec<Invocation>, CommandError> {
    tokenizer::split(line)?
        .iter()
        .map(|words| invocation(words))
        .collect()
}

fn invocation(words: &[Word]) -> Result<Invocation, CommandError> {
    let (name, rest) = words
        .split_first()
        .expect("the tokenizer drops empty commands");
    let spec = std::str::from_utf8(name)
        .ok()
        .and_then(find_by_name)
        .ok_or_else(|| CommandError::UnknownCommand(String::from_utf8_lossy(name).into()))?;
    Ok(Invocation {
        spec,
        args: parse_args(spec, rest)?,
    })
}

fn parse_args(spec: &'static CommandSpec, words: &[Word]) -> Result<Vec<ArgValue>, CommandError> {
    if words.len() > spec.args.len() {
        return Err(CommandError::TooManyArguments {
            command: spec.name,
            max: spec.args.len(),
        });
    }
    let mut args = Vec::with_capacity(words.len());
    for (index, arg) in spec.args.iter().enumerate() {
        match words.get(index) {
            Some(word) => {
                let value = arg
                    .kind
                    .parse(word)
                    .map_err(|reason| CommandError::BadArgument {
                        command: spec.name,
                        index: index + 1,
                        reason,
                    })?;
                args.push(value);
            }
            None if arg.optional => break,
            None => {
                return Err(CommandError::MissingArgument {
                    command: spec.name,
                    name: arg.name,
                });
            }
        }
    }
    Ok(args)
}

impl ArgKind {
    fn parse(self, word: &[u8]) -> Result<ArgValue, String> {
        if self == Self::Bytes {
            return Ok(ArgValue::Bytes(word.to_vec()));
        }
        let text = std::str::from_utf8(word).map_err(|_| "not valid UTF-8".to_string())?;
        match self {
            Self::Int { min, max, .. } => {
                let n: i64 = text
                    .parse()
                    .map_err(|_| format!("'{text}' is not a number"))?;
                if !(min..=max).contains(&n) {
                    return Err(format!("{n} is out of {min}..={max}"));
                }
                Ok(ArgValue::Int(n))
            }
            Self::Bool => match text {
                "on" | "true" | "1" => Ok(ArgValue::Bool(true)),
                "off" | "false" | "0" => Ok(ArgValue::Bool(false)),
                _ => Err(format!("'{text}' is not on/true/1 or off/false/0")),
            },
            Self::Enum(values) => values
                .iter()
                .find(|value| **value == text)
                .map(|value| ArgValue::Enum(value))
                .ok_or_else(|| format!("'{text}' is not one of {}", values.join(", "))),
            Self::String | Self::Trigger | Self::Link => Ok(ArgValue::String(text.to_string())),
            Self::Path => Ok(ArgValue::Path(PathBuf::from(text))),
            Self::Address => text.parse().map(ArgValue::Address),
            Self::Hex => parse_hex(text).map(ArgValue::Bytes),
            Self::Bytes => unreachable!("bytes aren't text"),
        }
    }

    /// The values starting with `prefix` to complete this argument with
    pub fn candidates(self, prefix: &str) -> Vec<&'static str> {
        let values: &[&str] = match self {
            Self::Int { suggestions, .. } => suggestions,
            Self::Bool => &["on", "off"],
            Self::Enum(values) => values,
            // paths and addresses complete from the disk, in `path_candidates` and
            // `address_candidates`, triggers and saved links from their names
            Self::String
            | Self::Path
            | Self::Address
            | Self::Hex
            | Self::Bytes
            | Self::Trigger
            | Self::Link => &[],
        };
        values
            .iter()
            .copied()
            .filter(|value| value.starts_with(prefix))
            .collect()
    }
}

/// Hex digits to bytes, no spaces between them: `0d0a`
fn parse_hex(text: &str) -> Result<Vec<u8>, String> {
    let digits = text.as_bytes();
    if let Some(&bad) = digits.iter().find(|byte| !byte.is_ascii_hexdigit()) {
        return Err(format!("'{}' is not a hex digit", bad.escape_ascii()));
    }
    if digits.is_empty() {
        return Err("no hex digits".to_string());
    }
    if !digits.len().is_multiple_of(2) {
        return Err("odd number of hex digits".to_string());
    }
    Ok(digits
        .chunks(2)
        .map(|pair| {
            let hex = std::str::from_utf8(pair).expect("ASCII hex digits");
            u8::from_str_radix(hex, 16).expect("two hex digits")
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_ARGS: &[ArgSpec] = &[
        ArgSpec {
            name: "rate",
            kind: ArgKind::Int {
                min: 1,
                max: 4_000_000,
                suggestions: &["9600", "115200"],
            },
            optional: false,
        },
        ArgSpec {
            name: "state",
            kind: ArgKind::Bool,
            optional: false,
        },
        ArgSpec {
            name: "flow",
            kind: ArgKind::Enum(&["none", "hw", "sw"]),
            optional: false,
        },
        ArgSpec {
            name: "label",
            kind: ArgKind::String,
            optional: false,
        },
        ArgSpec {
            name: "file",
            kind: ArgKind::Path,
            optional: false,
        },
        ArgSpec {
            name: "bytes",
            kind: ArgKind::Hex,
            optional: true,
        },
    ];

    /// A command with an argument of each kind, and one without
    static TEST_COMMANDS: [CommandSpec; 2] = [
        CommandSpec {
            name: "test",
            args: TEST_ARGS,
            help: "",
            target: Target::Action(KeyBindingAction::Quit),
        },
        CommandSpec::action("quit", KeyBindingAction::Quit, "Quit"),
    ];

    fn args(line: &str) -> Result<Vec<ArgValue>, CommandError> {
        let words = tokenizer::split(line).unwrap().remove(0);
        parse_args(&TEST_COMMANDS[0], &words[1..])
    }

    fn bad_argument(line: &str) -> (usize, String) {
        match args(line) {
            Err(CommandError::BadArgument { index, reason, .. }) => (index, reason),
            other => panic!("{line}: {other:?}"),
        }
    }

    /// The position of an action in the enum. The match stops compiling when an action is added:
    /// give it the next index and a command in the registry
    fn action_index(action: KeyBindingAction) -> usize {
        use KeyBindingAction as A;
        match action {
            A::Quit => 0,
            A::SKInputMode => 1,
            A::SKLocalEcho => 2,
            A::SKLineWrap => 3,
            A::SKDisplayTimestamp => 4,
            A::SKTextEncoding => 5,
            A::SKIndentWrap => 6,
            A::SKDisplayMode => 7,
            A::SKDisplayRaw => 8,
            A::Help => 9,
            A::Settings => 10,
            A::Record => 11,
            A::ResizeDialog => 12,
            A::AutoResizeRemote => 13,
            A::AutoResizeRemoteLine => 14,
            A::AutoResizeEmulator => 15,
            A::ScreenshotTxt => 16,
            A::ScreenshotBytes => 17,
            A::LinkSettings => 18,
            A::EnterCopyMode => 19,
            A::EnterSearch => 20,
            A::ScrollUp => 21,
            A::ScrollDown => 22,
            A::ScrollToBottom => 23,
            A::ScrollToTop => 24,
            A::PageUp => 25,
            A::PageDown => 26,
            A::PanRight => 27,
            A::PanLeft => 28,
            A::PanHome => 29,
            A::PanSideRight => 30,
            A::PanSideLeft => 31,
            A::ClearHistory => 32,
            A::Sessions => 33,
            A::ToggleAttach => 34,
            A::ExitPrefix => 35,
            A::ReloadConfig => 36,
            A::ToggleMouse => 37,
            A::CommandLine => 38,
            A::CancelCommands => 39,
            A::ToggleTriggers => 40,
            A::Triggers => 41,
            A::Clear => 42,
            A::SKAddCr => 43,
            A::SKAddLf => 44,
            A::Directory => 45,
        }
    }

    /// Every action is one zero-argument command, found by its name and by its action
    #[test]
    fn every_action_has_one_command() {
        let mut indexes: Vec<usize> = COMMANDS
            .iter()
            .filter_map(|spec| match spec.target {
                Target::Action(action) => {
                    assert!(spec.args.is_empty(), "{}", spec.name);
                    assert_eq!(find_by_name(spec.name), Some(spec));
                    assert_eq!(find_by_action(action), Some(spec));
                    Some(action_index(action))
                }
                Target::Build(_) => None,
            })
            .collect();
        indexes.sort_unstable();
        assert_eq!(indexes, (0..46).collect::<Vec<_>>());
    }

    /// The help writes a required argument in angle brackets, an optional one in square ones
    #[test]
    fn usage() {
        let usage = |name| find_by_name(name).unwrap().usage();
        assert_eq!(usage("quit"), "quit");
        assert_eq!(usage("dtr"), "dtr <state>");
        assert_eq!(usage("send-file"), "send-file <path> [line-delay-ms]");
    }

    /// `send` takes any bytes, escapes and all, `send-hex` hex digits; both are bytes
    #[test]
    fn send_arguments() {
        let bytes = |line: &str| {
            let invocations = parse(line).unwrap();
            assert_eq!(invocations[0].args.len(), 1);
            invocations[0].args[0].clone()
        };
        assert_eq!(bytes("send AT\\r\\n"), ArgValue::Bytes(b"AT\r\n".to_vec()));
        assert_eq!(bytes("send '\\xff' "), ArgValue::Bytes(b"\\xff".to_vec()));
        assert_eq!(bytes("send \\xff\\x00"), ArgValue::Bytes(vec![0xff, 0]));
        assert_eq!(bytes("send-hex 0D0a"), ArgValue::Bytes(vec![0x0d, 0x0a]));
        assert_eq!(
            parse("send").unwrap_err().to_string(),
            "send: missing argument <text>"
        );
        assert_eq!(
            parse("send-hex 0d 0a").unwrap_err().to_string(),
            "send-hex takes 1 argument(s) at most"
        );
        assert!("send".parse::<KeyBindingAction>().is_err());
    }

    /// The arguments of the line commands: `on`/`off`, a break in 1..=10000 ms or none, a baud
    /// rate in 50..=4000000 with the usual ones completed, `none`/`hw`/`sw`
    #[test]
    fn line_command_arguments() {
        for line in [
            "dtr on",
            "rts off",
            "dtr 1",
            "break",
            "break 100",
            "baud 115200",
            "flow hw",
            "framing 7e1",
        ] {
            assert!(parse(line).is_ok(), "{line}");
        }
        for line in [
            "dtr",
            "rts maybe",
            "break 0",
            "break 10001",
            "baud 10",
            "flow rtscts",
        ] {
            assert!(parse(line).is_err(), "{line}");
        }
        assert_eq!(
            values(&complete("baud 1", &[], &[], &[])),
            ["19200", "115200"]
        );
        assert_eq!(
            values(&complete("flow ", &[], &[], &[])),
            ["none", "hw", "sw"]
        );
        assert_eq!(values(&complete("dtr o", &[], &[], &[])), ["on", "off"]);
    }

    /// MAX-6: `trigger` takes on, off or toggle, then a trigger's name, completed from the
    /// names it is given, escaped like a path.
    #[test]
    fn trigger_names() {
        let names = [
            "u-boot".to_string(),
            "uart menu".to_string(),
            "login".to_string(),
        ];
        assert_eq!(values(&complete("trigger t", &names, &[], &[])), ["toggle"]);
        assert_eq!(
            values(&complete("trigger on u", &names, &[], &[])),
            ["u-boot", "uart\\ menu"]
        );
        assert_eq!(values(&complete("trigger off ", &names, &[], &[])).len(), 3);
        assert!(complete("trigger on ", &[], &[], &[]).candidates.is_empty());
        assert!(parse("trigger toggle u-boot").is_ok());
        assert!(parse("trigger flip").is_err());
    }

    #[test]
    fn registry_is_well_formed() {
        let mut names: Vec<_> = COMMANDS.iter().map(|spec| spec.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), COMMANDS.len(), "duplicate names");
        for spec in COMMANDS {
            assert!(!spec.help.is_empty(), "{}", spec.name);
            let first_optional = spec.args.iter().position(|arg| arg.optional);
            if let Some(first) = first_optional {
                assert!(spec.args[first..].iter().all(|arg| arg.optional));
            }
        }
    }

    #[test]
    fn parse_chained_commands() {
        let invocations = parse("help; toggle-mouse;;").unwrap();
        let names: Vec<_> = invocations.iter().map(|inv| inv.spec.name).collect();
        assert_eq!(names, ["help", "toggle-mouse"]);
        assert!(invocations.iter().all(|inv| inv.args.is_empty()));
        assert!(parse("").unwrap().is_empty());
        assert!(parse("  ; ").unwrap().is_empty());
    }

    #[test]
    fn parse_errors() {
        assert_eq!(
            parse("help; nope"),
            Err(CommandError::UnknownCommand("nope".into()))
        );
        assert_eq!(
            parse("\\xff"),
            Err(CommandError::UnknownCommand("\u{fffd}".into()))
        );
        assert_eq!(
            parse("quit now"),
            Err(CommandError::TooManyArguments {
                command: "quit",
                max: 0
            })
        );
        assert_eq!(parse("help \"x"), Err(CommandError::UnclosedQuote));
        assert_eq!(
            parse("quit now").unwrap_err().to_string(),
            "quit takes no argument"
        );
    }

    #[test]
    fn arguments_of_each_kind() {
        assert_eq!(
            args("test 115200 on hw 'a label' /tmp/f 0d0A").unwrap(),
            [
                ArgValue::Int(115200),
                ArgValue::Bool(true),
                ArgValue::Enum("hw"),
                ArgValue::String("a label".into()),
                ArgValue::Path("/tmp/f".into()),
                ArgValue::Bytes(vec![0x0d, 0x0a]),
            ]
        );
        assert_eq!(args("test 1 off none x y").unwrap().len(), 5);
        for (word, value) in [
            ("on", true),
            ("true", true),
            ("1", true),
            ("off", false),
            ("false", false),
            ("0", false),
        ] {
            let line = format!("test 1 {word} none x y");
            assert_eq!(args(&line).unwrap()[1], ArgValue::Bool(value), "{word}");
        }
    }

    #[test]
    fn argument_errors() {
        assert_eq!(
            args("test 1 on none x"),
            Err(CommandError::MissingArgument {
                command: "test",
                name: "file"
            })
        );
        assert_eq!(
            args("test 1 on none x y 00 z"),
            Err(CommandError::TooManyArguments {
                command: "test",
                max: 6
            })
        );
        assert_eq!(bad_argument("test fast on none x y").0, 1);
        assert_eq!(
            bad_argument("test 0 on none x y"),
            (1, "0 is out of 1..=4000000".into())
        );
        assert_eq!(
            bad_argument("test 1 yes none x y"),
            (2, "'yes' is not on/true/1 or off/false/0".into())
        );
        assert_eq!(
            bad_argument("test 1 on rts x y"),
            (3, "'rts' is not one of none, hw, sw".into())
        );
        assert_eq!(bad_argument("test 1 on none \\xff y").1, "not valid UTF-8");
        assert_eq!(
            bad_argument("test 1 on none x y 0g").1,
            "'g' is not a hex digit"
        );
        assert_eq!(
            bad_argument("test 1 on none x y 0d0").1,
            "odd number of hex digits"
        );
        assert_eq!(bad_argument("test 1 on none x y ''").1, "no hex digits");
        assert_eq!(
            bad_argument("test 1 on none x y '0d 0a'").1,
            "' ' is not a hex digit"
        );
        let err = args("test 1 on none x y 0g").unwrap_err();
        assert_eq!(
            err.to_string(),
            "test: bad argument 6: 'g' is not a hex digit"
        );
    }

    fn values(completion: &Completion) -> Vec<&str> {
        completion
            .candidates
            .iter()
            .map(|c| c.value.as_str())
            .collect()
    }

    #[test]
    fn complete_command_names() {
        let complete = |line| complete(line, &[], &[], &[]);
        let all = complete("");
        assert_eq!(all.start, 0);
        assert_eq!(all.candidates.len(), COMMANDS.len());
        assert!(values(&all).is_sorted());
        assert_eq!(
            values(&complete("toggle-d")),
            [
                "toggle-display-mode",
                "toggle-display-raw",
                "toggle-display-timestamp"
            ]
        );
        let after_chain = complete("help; toggle-d");
        assert_eq!(after_chain.start, 6);
        assert_eq!(after_chain.candidates[0].value, "toggle-display-mode");
        assert_eq!(after_chain.candidates[0].help, "Cycle the display mode");
        assert!(complete("zz").candidates.is_empty());
        // a whole name, or an argument of a command without any
        assert!(complete("quit").candidates.is_empty());
        assert_eq!(
            complete("help "),
            Completion {
                start: 5,
                candidates: vec![]
            }
        );
    }

    #[test]
    fn complete_argument_values() {
        let complete = |line| complete_in(&TEST_COMMANDS, line, &[], &[], &[]);
        assert_eq!(values(&complete("test 1 ")), ["on", "off"]);
        assert_eq!(values(&complete("test  1   o")), ["on", "off"]);
        let flow = complete("quit; test 1 on h");
        assert_eq!((flow.start, values(&flow)), (16, vec!["hw"]));
        assert_eq!(values(&complete("test 1")), ["115200"]);
        assert_eq!(complete("test 1 on hw").candidates, []);
        // a quote or an escape: no completion
        for line in ["test 1 \"o", "test 1 'o", "test 1 \\o", "test \"a b\" o\\"] {
            assert!(complete(line).candidates.is_empty(), "{line}");
        }
        assert_eq!(values(&complete("test \"a b\" o")), ["on", "off"]);
        assert!(complete("test 1 on hw x y z ").candidates.is_empty());
    }

    /// A path completes from the folder typed: its entries starting like the last part, a
    /// folder with a `/`, hidden ones for a part starting with `.` only, a space escaped so the
    /// word parses back to the name.
    /// How: a temporary folder with files, a subfolder and a hidden file.
    #[test]
    fn complete_paths() {
        let dir = std::env::temp_dir().join(format!("maxicom_test_{}_paths", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("logs")).unwrap();
        for file in ["log.txt", "my file.bin", ".hidden", "other"] {
            std::fs::write(dir.join(file), "").unwrap();
        }
        // `/` between the parts on every system: `\` is the tokenizer's escape
        let folder = format!("{}/", dir.display()).replace('\\', "/");
        let complete = |typed: &str| {
            let line = format!("test 1 on none x {folder}{typed}");
            let completion = complete_in(&TEST_COMMANDS, &line, &[], &[], &[]);
            assert_eq!(completion.start, 17);
            completion
                .candidates
                .iter()
                .map(|c| c.value.strip_prefix(&folder).unwrap().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(complete("lo"), ["log.txt", "logs/"]);
        assert_eq!(complete(""), ["log.txt", "logs/", "my\\ file.bin", "other"]);
        assert_eq!(complete("."), [".hidden"]);
        assert!(complete("logs/").is_empty());
        assert!(complete("other").is_empty());

        let line = format!("test 1 on none x {folder}my\\ file.bin");
        // the escaped name is whole, and the argument is the name
        assert!(
            complete_in(&TEST_COMMANDS, &line, &[], &[], &[])
                .candidates
                .is_empty()
        );
        let parsed = parse_args(&TEST_COMMANDS[0], &tokenizer::split(&line).unwrap()[0][1..]);
        assert_eq!(
            parsed.unwrap()[4],
            ArgValue::Path(PathBuf::from(format!("{folder}my file.bin")))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MAX-168: `open`'s address is checked when the line is read, and completes with the
    /// schemes and the listed ports, then paths after `file://` or a `/`, nothing after
    /// `tcp://`.
    /// How: good and bad addresses; completions with two fake ports and a temporary folder.
    #[test]
    fn open_takes_an_address() {
        assert_eq!(
            parse("open tcp://h:1").unwrap()[0].args,
            [ArgValue::Address("tcp://h:1".parse().unwrap())]
        );
        for (line, reason) in [
            ("open tcp://host", "'host' is not host:port"),
            ("open file://", "file:// needs a path"),
        ] {
            let err = parse(line).unwrap_err().to_string();
            assert_eq!(err, format!("open: bad argument 1: {reason}"));
        }

        let ports = ["/dev/ttyUSB0".to_string(), "COM3".to_string()];
        let complete = |line: &str| complete(line, &[], &[], &ports);
        assert_eq!(
            values(&complete("open ")),
            ["tcp://", "file://", "/dev/ttyUSB0", "COM3"]
        );
        assert_eq!(values(&complete("open t")), ["tcp://"]);
        assert_eq!(values(&complete("open C")), ["COM3"]);
        assert!(complete("open tcp://192.").candidates.is_empty());
        // MAX-185: this machine's saved links first, escaped; `link-delete` takes them only
        let names = ["esp32".to_string(), "router console".to_string()];
        let saved = |line: &str| super::complete(line, &[], &names, &ports);
        assert_eq!(
            values(&saved("open ")),
            [
                "esp32",
                "router\\ console",
                "tcp://",
                "file://",
                "/dev/ttyUSB0",
                "COM3"
            ]
        );
        assert_eq!(values(&saved("link-delete r")), ["router\\ console"]);
        assert_eq!(values(&saved("link-delete ")).len(), 2);

        let dir = std::env::temp_dir().join(format!("maxicom_test_{}_address", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("capture.raw"), "").unwrap();
        let folder = format!("{}/", dir.display()).replace('\\', "/");
        assert_eq!(
            values(&complete(&format!("open file://{folder}c"))),
            [format!("file://{folder}capture.raw")]
        );
        assert_eq!(
            values(&complete(&format!("open {folder}c"))),
            [format!("{folder}capture.raw")]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn argument_candidates() {
        assert_eq!(TEST_ARGS[0].kind.candidates("1"), ["115200"]);
        assert_eq!(TEST_ARGS[1].kind.candidates(""), ["on", "off"]);
        assert!(TEST_ARGS[1].kind.candidates("t").is_empty());
        assert_eq!(TEST_ARGS[2].kind.candidates("h"), ["hw"]);
        assert!(TEST_ARGS[3].kind.candidates("").is_empty());
    }
}
