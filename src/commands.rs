// TODO: (MAX-12) the command line (step 2) runs the parser, only the tests do until then
#![allow(dead_code)]

/// Command registry and parser (MAX-12): the one table of named commands that keys, the
/// command line and later the API (MAX-74) run. Pure functions, no UI
use std::path::PathBuf;

use crate::configs::KeyBindingAction;

pub mod tokenizer;

use tokenizer::Word;

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
    Hex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgValue {
    Int(i64),
    Bool(bool),
    Enum(&'static str),
    String(String),
    Path(PathBuf),
    Bytes(Vec<u8>),
}

/// One argument of a command. Optional ones come last
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArgSpec {
    pub name: &'static str,
    pub kind: ArgKind,
    pub optional: bool,
}

/// What a command runs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Action(KeyBindingAction),
}

#[derive(Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub name: &'static str,
    pub args: &'static [ArgSpec],
    pub help: &'static str,
    pub target: Target,
}

impl CommandSpec {
    const fn action(name: &'static str, action: KeyBindingAction, help: &'static str) -> Self {
        Self {
            name,
            args: &[],
            help,
            target: Target::Action(action),
        }
    }
}

/// A parsed command, its arguments checked against its spec
#[derive(Debug, PartialEq, Eq)]
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
        CommandSpec::action(
            "toggle-logging",
            A::ToggleLogging,
            "Start / stop the log file",
        ),
        CommandSpec::action("record-text", A::RecordText, "Start / stop recording text"),
        CommandSpec::action(
            "record-raw",
            A::RecordRaw,
            "Start / stop recording raw bytes",
        ),
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
        CommandSpec::action(
            "serial-settings",
            A::SerialSettings,
            "Open the port settings",
        ),
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
        CommandSpec::action("clear-history", A::ClearHistory, "Clear the history"),
        CommandSpec::action("sessions", A::Sessions, "Open the session backups"),
        CommandSpec::action("exit-prefix", A::ExitPrefix, "Leave the prefix"),
        CommandSpec::action("detach-serial", A::DetachSerial, "Detach / attach the port"),
        CommandSpec::action("reload-config", A::ReloadConfig, "Reload the config files"),
        CommandSpec::action("toggle-mouse", A::ToggleMouse, "Toggle mouse support"),
    ]
};

pub fn find_by_name(name: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|spec| spec.name == name)
}

pub fn find_by_action(action: KeyBindingAction) -> Option<&'static CommandSpec> {
    COMMANDS
        .iter()
        .find(|spec| spec.target == Target::Action(action))
}

/// The command names starting with `prefix`, sorted, for completion
pub fn complete_names(prefix: &str) -> Vec<&'static str> {
    let mut names: Vec<_> = COMMANDS
        .iter()
        .map(|spec| spec.name)
        .filter(|name| name.starts_with(prefix))
        .collect();
    names.sort_unstable();
    names
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
            Self::String => Ok(ArgValue::String(text.to_string())),
            Self::Path => Ok(ArgValue::Path(PathBuf::from(text))),
            Self::Hex => parse_hex(text).map(ArgValue::Bytes),
        }
    }

    /// The values starting with `prefix` to complete this argument with
    pub fn candidates(self, prefix: &str) -> Vec<&'static str> {
        let values: &[&str] = match self {
            Self::Int { suggestions, .. } => suggestions,
            Self::Bool => &["on", "off"],
            Self::Enum(values) => values,
            // TODO: (MAX-12) paths complete from the file system in the command line (step 2)
            Self::String | Self::Path | Self::Hex => &[],
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

    static TEST_SPEC: CommandSpec = CommandSpec {
        name: "test",
        args: TEST_ARGS,
        help: "",
        target: Target::Action(KeyBindingAction::Quit),
    };

    fn args(line: &str) -> Result<Vec<ArgValue>, CommandError> {
        let words = tokenizer::split(line).unwrap().remove(0);
        parse_args(&TEST_SPEC, &words[1..])
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
            A::ToggleLogging => 11,
            A::RecordText => 12,
            A::RecordRaw => 13,
            A::ResizeDialog => 14,
            A::AutoResizeRemote => 15,
            A::AutoResizeRemoteLine => 16,
            A::AutoResizeEmulator => 17,
            A::ScreenshotTxt => 18,
            A::ScreenshotBytes => 19,
            A::SerialSettings => 20,
            A::EnterCopyMode => 21,
            A::EnterSearch => 22,
            A::ScrollUp => 23,
            A::ScrollDown => 24,
            A::ScrollToBottom => 25,
            A::ScrollToTop => 26,
            A::PageUp => 27,
            A::PageDown => 28,
            A::PanRight => 29,
            A::PanLeft => 30,
            A::PanHome => 31,
            A::PanSideRight => 32,
            A::PanSideLeft => 33,
            A::ClearHistory => 34,
            A::Sessions => 35,
            A::DetachSerial => 36,
            A::ExitPrefix => 37,
            A::ReloadConfig => 38,
            A::ToggleMouse => 39,
        }
    }

    /// Every action is one zero-argument command, found by its name and by its action
    #[test]
    fn every_action_has_one_command() {
        let mut indexes: Vec<usize> = COMMANDS
            .iter()
            .map(|spec| match spec.target {
                Target::Action(action) => {
                    assert!(spec.args.is_empty(), "{}", spec.name);
                    assert_eq!(find_by_name(spec.name), Some(spec));
                    assert_eq!(find_by_action(action), Some(spec));
                    action_index(action)
                }
            })
            .collect();
        indexes.sort_unstable();
        assert_eq!(indexes, (0..40).collect::<Vec<_>>());
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

    #[test]
    fn completion() {
        assert_eq!(
            complete_names("toggle-d"),
            [
                "toggle-display-mode",
                "toggle-display-raw",
                "toggle-display-timestamp"
            ]
        );
        assert_eq!(complete_names("").len(), COMMANDS.len());
        assert!(complete_names("zz").is_empty());
        assert_eq!(TEST_ARGS[0].kind.candidates("1"), ["115200"]);
        assert_eq!(TEST_ARGS[1].kind.candidates(""), ["on", "off"]);
        assert!(TEST_ARGS[1].kind.candidates("t").is_empty());
        assert_eq!(TEST_ARGS[2].kind.candidates("h"), ["hw"]);
        assert!(TEST_ARGS[3].kind.candidates("").is_empty());
    }
}
