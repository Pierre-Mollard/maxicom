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
    Plot,
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

/// A mark's letter, waited for as the next key when left out (MAX-164)
const MARK_LETTER: ArgSpec = ArgSpec {
    name: "letter",
    kind: ArgKind::Enum(build::MARK_LETTERS),
    optional: true,
};

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
        CommandSpec::action(
            "enter-search-backward",
            A::EnterSearchBackward,
            "Search the history from the end",
        ),
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
        CommandSpec::action("pan-end", A::PanEnd, "Pan to the line end"),
        CommandSpec::action("pan-side-right", A::PanSideRight, "Pan the side view right"),
        CommandSpec::action("pan-side-left", A::PanSideLeft, "Pan the side view left"),
        CommandSpec::action("side-grow", A::SideGrow, "Grow the side view by 1 %"),
        CommandSpec::action("side-shrink", A::SideShrink, "Shrink the side view by 1 %"),
        CommandSpec::action("side-grow-5", A::SideGrow5, "Grow the side view by 5 %"),
        CommandSpec::action(
            "side-shrink-5",
            A::SideShrink5,
            "Shrink the side view by 5 %",
        ),
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
        CommandSpec::action(
            "plot-freeze",
            A::PlotFreeze,
            "Freeze / unfreeze the plot, its data still collected",
        ),
        CommandSpec::action(
            "plot-next",
            A::PlotNext,
            "Show the next plot of plots.toml, off after the last",
        ),
        CommandSpec::action(
            "plot-prev",
            A::PlotPrev,
            "Show the previous plot of plots.toml, off before the first",
        ),
        CommandSpec::action(
            "dismiss-notification",
            A::DismissNotification,
            "Close the top notification",
        ),
        CommandSpec::action(
            "dismiss-notifications",
            A::DismissNotifications,
            "Close all the notifications",
        ),
        CommandSpec::action(
            "marks",
            A::Marks,
            "Open the marks: a letter jumps to its mark",
        ),
        CommandSpec::action("prev-mark", A::PrevMark, "Go to the mark above"),
        CommandSpec::action("next-mark", A::NextMark, "Go to the mark below"),
        CommandSpec::build(
            "mark",
            &[MARK_LETTER],
            build::mark,
            "Mark a line; without a letter, waits for it (Enter: the first free one)",
        ),
        CommandSpec::build(
            "jump-mark",
            &[MARK_LETTER],
            build::jump_mark,
            "Go to a mark; without a letter, waits for it",
        ),
        CommandSpec::build(
            "delete-mark",
            &[arg("letter", ArgKind::Enum(build::MARK_LETTERS))],
            build::delete_mark,
            "Delete a mark",
        ),
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
            "upload",
            &[
                arg("protocol", ArgKind::Enum(build::UPLOAD_PROTOCOLS)),
                arg("path", ArgKind::Path),
            ],
            build::upload,
            "Send a file with XMODEM or YMODEM to a receiver (loady, loadx, rx, rb)",
        ),
        CommandSpec::build(
            "download",
            &[
                arg("protocol", ArgKind::Enum(build::DOWNLOAD_PROTOCOLS)),
                ArgSpec {
                    name: "path",
                    kind: ArgKind::Path,
                    optional: true,
                },
            ],
            build::download,
            "Receive files sent with XMODEM or YMODEM (sb, sx), into [transfers] dir or a path",
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
            "session-name",
            &[ArgSpec {
                name: "text",
                kind: ArgKind::String,
                optional: true,
            }],
            build::session_name,
            "Name the running session, in quotes with spaces; no text takes the name away",
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
            "plot",
            &[ArgSpec {
                name: "name",
                kind: ArgKind::Plot,
                optional: true,
            }],
            build::plot,
            "Show a plot of plots.toml by name, off, or the next one",
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
/// lists, for an address; `plot_names` the plots of `plots.toml` (MAX-222)
pub fn complete(
    before_cursor: &str,
    trigger_names: &[String],
    link_names: &[String],
    ports: &[String],
    plot_names: &[String],
) -> Completion {
    complete_in(
        COMMANDS,
        before_cursor,
        trigger_names,
        link_names,
        ports,
        plot_names,
    )
}

fn complete_in(
    commands: &'static [CommandSpec],
    line: &str,
    trigger_names: &[String],
    link_names: &[String],
    ports: &[String],
    plot_names: &[String],
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
                ArgKind::Plot => (plot_names.iter().map(String::as_str))
                    .chain(["off"])
                    .map(escape_word)
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
    tokenizer::split(line, is_path_word)?
        .iter()
        .map(|words| invocation(words))
        .collect()
}

/// The next word of a command, after `words`, is a path or an address: its `\` stays as is
fn is_path_word(words: &[Word]) -> bool {
    let Some((name, args)) = words.split_first() else {
        return false;
    };
    std::str::from_utf8(name)
        .ok()
        .and_then(find_by_name)
        .and_then(|spec| spec.args.get(args.len()))
        .is_some_and(|arg| matches!(arg.kind, ArgKind::Path | ArgKind::Address))
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
            Self::String | Self::Trigger | Self::Link | Self::Plot => {
                Ok(ArgValue::String(text.to_string()))
            }
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
            | Self::Link
            | Self::Plot => &[],
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
#[path = "tests/commands.rs"]
mod tests;
