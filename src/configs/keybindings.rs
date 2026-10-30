use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::commands::{self, CommandError, Invocation, Target};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownActionError(pub String);

impl std::fmt::Display for UnknownActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown action '{}'", self.0)
    }
}

impl std::error::Error for UnknownActionError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyBindingAction {
    Quit,
    SKInputMode,
    SKLocalEcho,
    SKLineWrap,
    SKDisplayTimestamp,
    SKTextEncoding,
    SKIndentWrap,
    SKDisplayMode,
    SKDisplayRaw,
    Help,
    Settings,
    ToggleLogging,
    RecordText,
    RecordRaw,
    ResizeDialog,
    AutoResizeRemote,
    AutoResizeRemoteLine,
    AutoResizeEmulator,
    ScreenshotTxt,
    ScreenshotBytes,
    SerialSettings,
    EnterCopyMode,
    EnterSearch,
    ScrollUp,
    ScrollDown,
    ScrollToBottom,
    ScrollToTop,
    PageUp,
    PageDown,
    PanRight,
    PanLeft,
    PanHome,
    PanSideRight,
    PanSideLeft,
    ClearHistory,
    Sessions,
    DetachSerial,
    ExitPrefix,
    ReloadConfig,
    ToggleMouse,
    CommandLine,
    CancelCommands,
}

impl FromStr for KeyBindingAction {
    type Err = UnknownActionError;

    /// An action's name is its command's in the registry (MAX-12)
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        // the commands with arguments aren't keymap actions
        match commands::find_by_name(name).map(|spec| spec.target) {
            Some(Target::Action(action)) => Ok(action),
            _ => Err(UnknownActionError(name.to_string())),
        }
    }
}

impl std::fmt::Display for KeyBindingAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let spec = commands::find_by_action(*self).expect("every action has a command");
        write!(f, "{}", spec.name)
    }
}

impl KeyBindingAction {
    pub fn is_navigation(self) -> bool {
        matches!(
            self,
            Self::ScrollUp
                | Self::ScrollDown
                | Self::ScrollToTop
                | Self::ScrollToBottom
                | Self::PageUp
                | Self::PageDown
                | Self::PanLeft
                | Self::PanRight
                | Self::PanHome
                | Self::PanSideLeft
                | Self::PanSideRight
        )
    }

    pub fn repeats_when_held(self) -> bool {
        matches!(
            self,
            Self::ScrollUp
                | Self::ScrollDown
                | Self::PageUp
                | Self::PageDown
                | Self::PanLeft
                | Self::PanRight
                | Self::PanSideLeft
                | Self::PanSideRight
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyBindingLineAction {
    Submit,
    HistoryPrev,
    HistoryNext,
    AutoResizeRemoteLine,
    ClearLine,
    Interrupt,
    PageUp,
    PageDown,
    Quit,
}

impl std::str::FromStr for KeyBindingLineAction {
    type Err = UnknownActionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "submit" => Ok(Self::Submit),
            "history-prev" => Ok(Self::HistoryPrev),
            "history-next" => Ok(Self::HistoryNext),
            "auto-resize-remote-line" => Ok(Self::AutoResizeRemoteLine),
            "clear-line" => Ok(Self::ClearLine),
            "interrupt" => Ok(Self::Interrupt),
            "page-up" => Ok(Self::PageUp),
            "page-down" => Ok(Self::PageDown),
            "quit" => Ok(Self::Quit),
            other => Err(UnknownActionError(other.to_string())),
        }
    }
}

impl std::fmt::Display for KeyBindingLineAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Submit => "submit",
            Self::HistoryPrev => "history-prev",
            Self::HistoryNext => "history-next",
            Self::AutoResizeRemoteLine => "auto-resize-remote-line",
            Self::ClearLine => "clear-line",
            Self::Interrupt => "interrupt",
            Self::PageUp => "page-up",
            Self::PageDown => "page-down",
            Self::Quit => "quit",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyBindingTerminalAction {
    AutoResizeRemote,
    AutoResizeEmulator,
    Quit,
}

impl std::str::FromStr for KeyBindingTerminalAction {
    type Err = UnknownActionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto-resize-remote" => Ok(Self::AutoResizeRemote),
            "auto-resize-emulator" => Ok(Self::AutoResizeEmulator),
            "quit" => Ok(Self::Quit),
            other => Err(UnknownActionError(other.to_string())),
        }
    }
}

impl std::fmt::Display for KeyBindingTerminalAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::AutoResizeRemote => "auto-resize-remote",
            Self::AutoResizeEmulator => "auto-resize-emulator",
            Self::Quit => "quit",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyBindingSearchAction {
    Next,
    Previous,
    Cancel,
    Confirm,
    Kind,
}

impl std::str::FromStr for KeyBindingSearchAction {
    type Err = UnknownActionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "next" => Ok(Self::Next),
            "previous" => Ok(Self::Previous),
            "cancel" => Ok(Self::Cancel),
            "confirm" => Ok(Self::Confirm),
            "search-kind" => Ok(Self::Kind),
            other => Err(UnknownActionError(other.to_string())),
        }
    }
}

impl std::fmt::Display for KeyBindingSearchAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Next => "next",
            Self::Previous => "previous",
            Self::Cancel => "cancel",
            Self::Confirm => "confirm",
            Self::Kind => "search-kind",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyBindingCopyAction {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    StartOfLine,
    EndOfLine,
    Select,
    SelectLine,
    Yank,
    YankSideInspector,
    ToggleSide,
    Exit,
    ScrollUp,
    ScrollDown,
    ScrollPageUp,
    ScrollPageDown,
}

impl KeyBindingCopyAction {
    /// Moves the view only, not the copy cursor (MAX-143)
    pub fn is_scroll(self) -> bool {
        matches!(
            self,
            Self::ScrollUp | Self::ScrollDown | Self::ScrollPageUp | Self::ScrollPageDown
        )
    }
}

impl std::str::FromStr for KeyBindingCopyAction {
    type Err = UnknownActionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "page-up" => Ok(Self::PageUp),
            "page-down" => Ok(Self::PageDown),
            "start-of-line" => Ok(Self::StartOfLine),
            "end-of-line" => Ok(Self::EndOfLine),
            "select" => Ok(Self::Select),
            "select-line" => Ok(Self::SelectLine),
            "yank" => Ok(Self::Yank),
            "yank-side-inspector" => Ok(Self::YankSideInspector),
            "toggle-side" => Ok(Self::ToggleSide),
            "exit" => Ok(Self::Exit),
            "scroll-up" => Ok(Self::ScrollUp),
            "scroll-down" => Ok(Self::ScrollDown),
            "scroll-page-up" => Ok(Self::ScrollPageUp),
            "scroll-page-down" => Ok(Self::ScrollPageDown),
            other => Err(UnknownActionError(other.to_string())),
        }
    }
}

impl std::fmt::Display for KeyBindingCopyAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
            Self::PageUp => "page-up",
            Self::PageDown => "page-down",
            Self::StartOfLine => "start-of-line",
            Self::EndOfLine => "end-of-line",
            Self::Select => "select",
            Self::SelectLine => "select-line",
            Self::Yank => "yank",
            Self::YankSideInspector => "yank-side-inspector",
            Self::ToggleSide => "toggle-side",
            Self::Exit => "exit",
            Self::ScrollUp => "scroll-up",
            Self::ScrollDown => "scroll-down",
            Self::ScrollPageUp => "scroll-page-up",
            Self::ScrollPageDown => "scroll-page-down",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyBindingDialogAction {
    Close,
    Confirm,
    NavUp,
    NavDown,
    NavLeft,
    NavRight,
    CycleNext,
    CyclePrev,
    PageUp,
    PageDown,
    Home,
    End,
}

impl std::str::FromStr for KeyBindingDialogAction {
    type Err = UnknownActionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "close" => Ok(Self::Close),
            "confirm" => Ok(Self::Confirm),
            "up" => Ok(Self::NavUp),
            "down" => Ok(Self::NavDown),
            "left" => Ok(Self::NavLeft),
            "right" => Ok(Self::NavRight),
            "cycle-next" => Ok(Self::CycleNext),
            "cycle-prev" => Ok(Self::CyclePrev),
            "page-up" => Ok(Self::PageUp),
            "page-down" => Ok(Self::PageDown),
            "home" => Ok(Self::Home),
            "end" => Ok(Self::End),
            other => Err(UnknownActionError(other.to_string())),
        }
    }
}

impl std::fmt::Display for KeyBindingDialogAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            KeyBindingDialogAction::Close => "close",
            KeyBindingDialogAction::Confirm => "confirm",
            KeyBindingDialogAction::NavUp => "up",
            KeyBindingDialogAction::NavDown => "down",
            KeyBindingDialogAction::NavLeft => "left",
            KeyBindingDialogAction::NavRight => "right",
            KeyBindingDialogAction::CycleNext => "cycle-next",
            KeyBindingDialogAction::CyclePrev => "cycle-prev",
            KeyBindingDialogAction::PageUp => "page-up",
            KeyBindingDialogAction::PageDown => "page-down",
            KeyBindingDialogAction::Home => "home",
            KeyBindingDialogAction::End => "end",
        };
        write!(f, "{s}")
    }
}

impl KeyBindingDialogAction {
    /// The actions a text input popup takes from its keys, the others edit the field (MAX-155)
    pub fn is_text_input_action(self) -> bool {
        matches!(
            self,
            Self::Close | Self::Confirm | Self::CycleNext | Self::CyclePrev
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyPattern {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyPattern {
    pub fn from_event(ev: &KeyEvent) -> Self {
        let (code, modifiers) = match ev.code {
            KeyCode::Char(c) if c.is_ascii_uppercase() => (
                KeyCode::Char(c.to_ascii_lowercase()),
                ev.modifiers | KeyModifiers::SHIFT,
            ),
            KeyCode::Char(c) => (KeyCode::Char(c), ev.modifiers - KeyModifiers::SHIFT),
            KeyCode::BackTab => (KeyCode::BackTab, ev.modifiers | KeyModifiers::SHIFT),
            KeyCode::Tab if ev.modifiers.contains(KeyModifiers::SHIFT) => {
                (KeyCode::BackTab, ev.modifiers | KeyModifiers::SHIFT)
            }
            other => (other, ev.modifiers),
        };

        Self { code, modifiers }
    }

    pub fn is_modal(&self) -> bool {
        matches!(
            self.code,
            KeyCode::Esc | KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab
        ) || self
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    }

    pub fn to_raw_bytes(self) -> Vec<u8> {
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            match self.code {
                KeyCode::Char(' ') | KeyCode::Char('@') => vec![0x00],

                KeyCode::Char(c) if c.is_ascii_alphabetic() => {
                    let byte = c.to_ascii_uppercase() as u8;
                    vec![byte & 0x1F]
                }

                KeyCode::Char('[') => vec![0x1B],
                KeyCode::Char('\\') => vec![0x1C],
                KeyCode::Char(']') => vec![0x1D],
                KeyCode::Char('^') => vec![0x1E],
                KeyCode::Char('_') => vec![0x1F],
                KeyCode::Char('?') => vec![0x7F],

                _ => Vec::new(),
            }
        } else {
            match self.code {
                KeyCode::Char(c) => c.to_string().into_bytes(),
                KeyCode::Enter => vec![b'\r'],
                KeyCode::Tab => vec![b'\t'],
                KeyCode::Backspace => vec![0x08],
                KeyCode::Esc => vec![0x1B],
                _ => Vec::new(),
            }
        }
    }
}

impl FromStr for KeyPattern {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut modifiers = KeyModifiers::NONE;
        let mut code = None;

        for part in s.split('-') {
            // Special keys (no case)
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => {
                    modifiers.insert(KeyModifiers::CONTROL);
                    continue;
                }
                "alt" => {
                    modifiers.insert(KeyModifiers::ALT);
                    continue;
                }
                "shift" => {
                    modifiers.insert(KeyModifiers::SHIFT);
                    continue;
                }
                "enter" | "return" => {
                    code = Some(KeyCode::Enter);
                    continue;
                }
                "esc" | "escape" => {
                    code = Some(KeyCode::Esc);
                    continue;
                }
                "backspace" => {
                    code = Some(KeyCode::Backspace);
                    continue;
                }
                "tab" => {
                    code = Some(KeyCode::Tab);
                    continue;
                }
                "backtab" | "shifttab" => {
                    modifiers.insert(KeyModifiers::SHIFT);
                    code = Some(KeyCode::BackTab);
                    continue;
                }
                "delete" | "del" => {
                    code = Some(KeyCode::Delete);
                    continue;
                }
                "insert" | "ins" => {
                    code = Some(KeyCode::Insert);
                    continue;
                }
                "space" => {
                    code = Some(KeyCode::Char(' '));
                    continue;
                }
                "up" => {
                    code = Some(KeyCode::Up);
                    continue;
                }
                "down" => {
                    code = Some(KeyCode::Down);
                    continue;
                }
                "left" => {
                    code = Some(KeyCode::Left);
                    continue;
                }
                "right" => {
                    code = Some(KeyCode::Right);
                    continue;
                }
                "pageup" => {
                    code = Some(KeyCode::PageUp);
                    continue;
                }
                "pagedown" => {
                    code = Some(KeyCode::PageDown);
                    continue;
                }
                "home" => {
                    code = Some(KeyCode::Home);
                    continue;
                }
                "end" => {
                    code = Some(KeyCode::End);
                    continue;
                }
                _ => {}
            }

            // function keys f1..f12 (no case)
            let lower = part.to_ascii_lowercase();
            if lower.starts_with('f')
                && lower.len() > 1
                && let Ok(num) = lower[1..].parse::<u8>()
            {
                code = Some(KeyCode::F(num));
                continue;
            }

            // single char original case
            if part.chars().count() == 1 {
                let ch = part.chars().next().unwrap();
                if ch.is_ascii_uppercase() {
                    modifiers.insert(KeyModifiers::SHIFT);
                    code = Some(KeyCode::Char(ch.to_ascii_lowercase()));
                } else {
                    code = Some(KeyCode::Char(ch));
                }
                continue;
            }

            return Err(format!("Unknown key sequence: {part}"));
        }

        let mut code = code.ok_or_else(|| "Missing key code".to_string())?;
        if code == KeyCode::Tab && modifiers.contains(KeyModifiers::SHIFT) {
            code = KeyCode::BackTab;
        }

        Ok(Self { code, modifiers })
    }
}

impl std::fmt::Display for KeyPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = Vec::new();
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            parts.push("Ctrl");
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            parts.push("Alt");
        }

        let is_uppercase_char = match self.code {
            KeyCode::Char(c) => c.is_ascii_uppercase(),
            _ => false,
        };
        if self.modifiers.contains(KeyModifiers::SHIFT)
            && !is_uppercase_char
            && self.code != KeyCode::BackTab
        {
            parts.push("Shift");
        }

        let key_str = match self.code {
            KeyCode::Enter => "Enter".to_string(),
            KeyCode::Esc => "Esc".to_string(),
            KeyCode::Backspace => "Backspace".to_string(),
            KeyCode::Tab => "Tab".to_string(),
            KeyCode::BackTab => "Shift-Tab".to_string(),
            KeyCode::Delete => "Delete".to_string(),
            KeyCode::Insert => "Insert".to_string(),
            KeyCode::Up => "Up".to_string(),
            KeyCode::Down => "Down".to_string(),
            KeyCode::Left => "Left".to_string(),
            KeyCode::Right => "Right".to_string(),
            KeyCode::PageUp => "PageUp".to_string(),
            KeyCode::PageDown => "PageDown".to_string(),
            KeyCode::Home => "Home".to_string(),
            KeyCode::End => "End".to_string(),
            KeyCode::F(n) => format!("F{n}"),
            KeyCode::Char(' ') => "Space".to_string(),
            KeyCode::Char(c) => c.to_string(),
            _ => "?".to_string(),
        };

        parts.push(&key_str);
        write!(f, "{}", parts.join("-"))
    }
}

/// A command line bound to a key in `[prefix_actions]` or `[direct_actions]` (MAX-12): its
/// text as written, parsed when the keymap loads
#[derive(Debug, Clone)]
pub struct BoundLine {
    pub text: String,
    pub invocations: Vec<Invocation>,
}

pub struct KeyResolver {
    pub prefix: KeyPattern,
    pub prefix_actions: HashMap<KeyPattern, KeyBindingAction>,
    pub direct_actions: HashMap<KeyPattern, KeyBindingAction>,
    pub prefix_commands: HashMap<KeyPattern, BoundLine>,
    pub direct_commands: HashMap<KeyPattern, BoundLine>,
    pub line_actions: HashMap<KeyPattern, KeyBindingLineAction>,
    pub terminal_actions: HashMap<KeyPattern, KeyBindingTerminalAction>,
    pub search_actions: HashMap<KeyPattern, KeyBindingSearchAction>,
    pub copy_actions: HashMap<KeyPattern, KeyBindingCopyAction>,
    pub dialog_actions: HashMap<KeyPattern, KeyBindingDialogAction>,
}

#[derive(Debug)]
pub enum KeymapError {
    InvalidKey {
        section: &'static str,
        key: String,
        reason: String,
    },
    InvalidAction {
        section: &'static str,
        action: String,
    },
    BadCommandLine {
        section: &'static str,
        key: String,
        error: CommandError,
    },
}

impl std::fmt::Display for KeymapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeymapError::InvalidKey {
                section,
                key,
                reason,
            } => {
                write!(f, "in [{section}]: invalid key '{key}': {reason}")
            }
            KeymapError::InvalidAction { section, action } => {
                write!(f, "in [{section}]: unknown action '{action}'")
            }
            KeymapError::BadCommandLine {
                section,
                key,
                error,
            } => write!(f, "in [{section}]: {key}: {error}"),
        }
    }
}

impl std::error::Error for KeymapError {}

/// A global section's keys to their action, and to their command line
type GlobalSection = (
    HashMap<KeyPattern, KeyBindingAction>,
    HashMap<KeyPattern, BoundLine>,
);

/// A global section's keys, to an action name or else to a command line (MAX-12). A value
/// neither is an error naming its key: an unknown action, the line's parse error
fn global_section(
    section: &'static str,
    entries: &BTreeMap<String, String>,
) -> Result<GlobalSection, KeymapError> {
    let mut actions = HashMap::new();
    let mut lines = HashMap::new();
    for (key, value) in entries {
        let pattern = key.parse().map_err(|reason| KeymapError::InvalidKey {
            section,
            key: key.clone(),
            reason,
        })?;
        if let Ok(action) = value.parse() {
            actions.insert(pattern, action);
            continue;
        }
        match commands::parse(value) {
            Ok(invocations) if !invocations.is_empty() => {
                let text = value.trim().to_string();
                lines.insert(pattern, BoundLine { text, invocations });
            }
            Ok(_) => {
                return Err(KeymapError::InvalidAction {
                    section,
                    action: value.clone(),
                });
            }
            Err(error) => {
                return Err(KeymapError::BadCommandLine {
                    section,
                    key: key.clone(),
                    error,
                });
            }
        }
    }
    Ok((actions, lines))
}

/// The keys bound to `action` in a fixed order (the map's changes between runs): plain keys
/// before modified ones, named keys before letters, then by name
fn keys_of<A: Copy + PartialEq>(map: &HashMap<KeyPattern, A>, action: A) -> Vec<KeyPattern> {
    let mut keys: Vec<KeyPattern> = map
        .iter()
        .filter(|&(_, &bound)| bound == action)
        .map(|(&key, _)| key)
        .collect();
    keys.sort_by_cached_key(|key| {
        (
            key.modifiers.bits().count_ones(),
            matches!(key.code, KeyCode::Char(_)),
            key.to_string(),
        )
    });
    keys
}

impl KeyResolver {
    /// The keys bound to a dialog action, for hints (MAX-73)
    pub fn dialog_keys(&self, action: KeyBindingDialogAction) -> Vec<KeyPattern> {
        keys_of(&self.dialog_actions, action)
    }

    /// The keys bound to an action, the direct ones first, then the prefix ones, for the
    /// right-click menu (MAX-130). A prefix key is shown after the prefix
    pub fn action_keys(&self, action: KeyBindingAction) -> Vec<String> {
        let direct = keys_of(&self.direct_actions, action).into_iter();
        let prefixed = keys_of(&self.prefix_actions, action).into_iter();
        direct
            .map(|key| key.to_string())
            .chain(prefixed.map(|key| format!("{} {key}", self.prefix)))
            .collect()
    }

    /// The keys bound to a search action, for the search bar hint (MAX-73)
    pub fn search_keys(&self, action: KeyBindingSearchAction) -> Vec<KeyPattern> {
        keys_of(&self.search_actions, action)
    }

    /// The keys bound in `[line_mode]` or `[terminal_mode]` that never get there, because the
    /// prefix or `[direct_actions]` take them first (MAX-155), for a warning
    pub fn shadowed_keys(&self) -> Vec<String> {
        let mode_keys = (self.line_actions.keys().map(|key| ("line_mode", key))).chain(
            self.terminal_actions
                .keys()
                .map(|key| ("terminal_mode", key)),
        );
        let direct_keys = (self.direct_actions.keys())
            .chain(self.direct_commands.keys())
            .map(|key| ("direct_actions", key));
        let mut shadowed: Vec<String> = mode_keys
            .chain(direct_keys)
            .filter_map(|(section, key)| {
                let is_direct =
                    self.direct_actions.contains_key(key) || self.direct_commands.contains_key(key);
                let by = if *key == self.prefix {
                    "the prefix"
                } else if section != "direct_actions" && is_direct {
                    "[direct_actions]"
                } else {
                    return None;
                };
                Some(format!("{key} in [{section}] (taken by {by})"))
            })
            .collect();
        shadowed.sort();
        shadowed
    }

    pub fn try_new(config: &KeyBindingsConfig) -> Result<Self, KeymapError> {
        let prefix = config
            .prefix
            .parse()
            .map_err(|err| KeymapError::InvalidKey {
                section: "root",
                key: config.prefix.clone(),
                reason: err,
            })?;

        let (prefix_actions, prefix_commands) =
            global_section("prefix_actions", &config.prefix_actions)?;
        let (direct_actions, direct_commands) =
            global_section("direct_actions", &config.direct_actions)?;

        let mut line_actions = HashMap::new();
        for (k, action_str) in &config.line_mode {
            let pat = k.parse().map_err(|err| KeymapError::InvalidKey {
                section: "line_mode",
                key: k.clone(),
                reason: err,
            })?;
            let act: KeyBindingLineAction =
                action_str.parse().map_err(|_| KeymapError::InvalidAction {
                    section: "line_mode",
                    action: action_str.clone(),
                })?;
            line_actions.insert(pat, act);
        }

        let mut terminal_actions = HashMap::new();
        for (k, action_str) in &config.terminal_mode {
            let pat = k.parse().map_err(|err| KeymapError::InvalidKey {
                section: "terminal_mode",
                key: k.clone(),
                reason: err,
            })?;
            let act: KeyBindingTerminalAction =
                action_str.parse().map_err(|_| KeymapError::InvalidAction {
                    section: "terminal_mode",
                    action: action_str.clone(),
                })?;
            terminal_actions.insert(pat, act);
        }

        let mut search_actions = HashMap::new();
        for (k, action_str) in &config.search_mode {
            let pat = k.parse().map_err(|err| KeymapError::InvalidKey {
                section: "search_mode",
                key: k.clone(),
                reason: err,
            })?;
            let act: KeyBindingSearchAction =
                action_str.parse().map_err(|_| KeymapError::InvalidAction {
                    section: "search_mode",
                    action: action_str.clone(),
                })?;
            search_actions.insert(pat, act);
        }

        let mut copy_actions = HashMap::new();
        for (k, action_str) in &config.copy_mode {
            let pat = k.parse().map_err(|err| KeymapError::InvalidKey {
                section: "copy_mode",
                key: k.clone(),
                reason: err,
            })?;
            let act: KeyBindingCopyAction =
                action_str.parse().map_err(|_| KeymapError::InvalidAction {
                    section: "copy_mode",
                    action: action_str.clone(),
                })?;
            copy_actions.insert(pat, act);
        }

        let mut dialog_actions = HashMap::new();
        for (k, action_str) in &config.dialog_actions {
            let pat = k.parse().map_err(|err| KeymapError::InvalidKey {
                section: "dialog_actions",
                key: k.clone(),
                reason: err,
            })?;
            let act: KeyBindingDialogAction =
                action_str.parse().map_err(|_| KeymapError::InvalidAction {
                    section: "dialog_actions",
                    action: action_str.clone(),
                })?;
            dialog_actions.insert(pat, act);
        }

        Ok(Self {
            prefix,
            prefix_actions,
            direct_actions,
            prefix_commands,
            direct_commands,
            line_actions,
            terminal_actions,
            search_actions,
            copy_actions,
            dialog_actions,
        })
    }
}

macro_rules! default_bindings {
    ($( $key:expr => $action:expr ),* $(,)?) => {{
        let mut map = std::collections::BTreeMap::new();
        $(
            map.insert($key.to_string(), $action.to_string());
        )*
        map
    }};
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyBindingsConfig {
    pub prefix: String,

    pub direct_actions: BTreeMap<String, String>,
    pub prefix_actions: BTreeMap<String, String>,
    pub line_mode: BTreeMap<String, String>,
    pub terminal_mode: BTreeMap<String, String>,
    pub search_mode: BTreeMap<String, String>,
    pub copy_mode: BTreeMap<String, String>,
    pub dialog_actions: BTreeMap<String, String>,
}

impl Default for KeyBindingsConfig {
    fn default() -> Self {
        let direct_actions = default_bindings! {
            "ctrl-q"         => KeyBindingAction::Quit,
            "shift-pageup"   => KeyBindingAction::PageUp,
            "shift-pagedown" => KeyBindingAction::PageDown,
            "shift-home"     => KeyBindingAction::ScrollToTop,
            "shift-end"      => KeyBindingAction::ScrollToBottom,
        };

        let prefix_actions = default_bindings! {
            "q"     => KeyBindingAction::Quit,
            "x"     => KeyBindingAction::Quit,
            "m"     => KeyBindingAction::SKInputMode,
            "o"     => KeyBindingAction::SKTextEncoding,
            "B"     => KeyBindingAction::SKDisplayRaw,
            "M"     => KeyBindingAction::ToggleMouse,
            "S"     => KeyBindingAction::Sessions,
            "C"     => KeyBindingAction::ClearHistory,
            "z"     => KeyBindingAction::Help,
            "?"     => KeyBindingAction::Help,
            "s"     => KeyBindingAction::Settings,
            "l"     => KeyBindingAction::ToggleLogging,
            "r"     => KeyBindingAction::ReloadConfig,
            "w"     => KeyBindingAction::ResizeDialog,
            "t"     => KeyBindingAction::AutoResizeEmulator,
            "d"     => KeyBindingAction::SerialSettings,
            "k"     => KeyBindingAction::ScreenshotBytes,
            "up"    => KeyBindingAction::ScrollUp,
            "down"  => KeyBindingAction::ScrollDown,
            "left"  => KeyBindingAction::PanLeft,
            "right" => KeyBindingAction::PanRight,
            "shift-left"  => KeyBindingAction::PanSideLeft,
            "shift-right" => KeyBindingAction::PanSideRight,
            "pageup"      => KeyBindingAction::PageUp,
            "pagedown"    => KeyBindingAction::PageDown,
            "["     => KeyBindingAction::EnterCopyMode,
            "/"     => KeyBindingAction::EnterSearch,
            ":"     => KeyBindingAction::CommandLine,
            "a"     => KeyBindingAction::ExitPrefix,
            "h"     => KeyBindingAction::DetachSerial,
            "f"     => "break", // a break like minicom's
            "p"     => "dtr off; sleep 100; dtr on", // DTR pulse which resets most boards
        };

        let line_mode = default_bindings! {
            "enter"  => KeyBindingLineAction::Submit,
            "up"     => KeyBindingLineAction::HistoryPrev,
            "down"   => KeyBindingLineAction::HistoryNext,
            "ctrl-c" => KeyBindingLineAction::Interrupt,
            "ctrl-u" => KeyBindingLineAction::ClearLine,
            "alt-r"  => KeyBindingLineAction::AutoResizeRemoteLine,
            "pageup"   => KeyBindingLineAction::PageUp,
            "pagedown" => KeyBindingLineAction::PageDown,
        };

        let terminal_mode = default_bindings! {
            "alt-r" => KeyBindingTerminalAction::AutoResizeRemote,
        };

        let search_mode = default_bindings! {
            "ctrl-n"     => KeyBindingSearchAction::Next,
            "ctrl-p"     => KeyBindingSearchAction::Previous,
            "esc"        => KeyBindingSearchAction::Cancel,
            "enter"      => KeyBindingSearchAction::Confirm,
            "tab"        => KeyBindingSearchAction::Kind,
        };

        let copy_mode = default_bindings! {
            "k"         => KeyBindingCopyAction::Up,
            "up"        => KeyBindingCopyAction::Up,
            "j"         => KeyBindingCopyAction::Down,
            "down"      => KeyBindingCopyAction::Down,
            "h"         => KeyBindingCopyAction::Left,
            "left"      => KeyBindingCopyAction::Left,
            "l"         => KeyBindingCopyAction::Right,
            "right"     => KeyBindingCopyAction::Right,
            "pageup"    => KeyBindingCopyAction::PageUp,
            "pagedown"  => KeyBindingCopyAction::PageDown,
            "0"         => KeyBindingCopyAction::StartOfLine,
            "$"         => KeyBindingCopyAction::EndOfLine,
            "v"         => KeyBindingCopyAction::Select,
            "space"     => KeyBindingCopyAction::Select,
            "V"         => KeyBindingCopyAction::SelectLine,
            "y"         => KeyBindingCopyAction::Yank,
            "enter"     => KeyBindingCopyAction::Yank,
            "Y"         => KeyBindingCopyAction::YankSideInspector,
            "tab"       => KeyBindingCopyAction::ToggleSide,
            "q"         => KeyBindingCopyAction::Exit,
            "esc"       => KeyBindingCopyAction::Exit,
            "ctrl-y"    => KeyBindingCopyAction::ScrollUp,
            "ctrl-e"    => KeyBindingCopyAction::ScrollDown,
        };

        let dialog_actions = default_bindings! {
            "esc"       => KeyBindingDialogAction::Close,
            "q"         => KeyBindingDialogAction::Close,
            "enter"     => KeyBindingDialogAction::Confirm,
            "down"      => KeyBindingDialogAction::NavDown,
            "j"         => KeyBindingDialogAction::NavDown,
            "up"        => KeyBindingDialogAction::NavUp,
            "k"         => KeyBindingDialogAction::NavUp,
            "left"      => KeyBindingDialogAction::NavLeft,
            "h"         => KeyBindingDialogAction::NavLeft,
            "right"     => KeyBindingDialogAction::NavRight,
            "l"         => KeyBindingDialogAction::NavRight,
            "tab"       => KeyBindingDialogAction::CycleNext,
            "space"     => KeyBindingDialogAction::CycleNext,
            "ctrl-n"    => KeyBindingDialogAction::CycleNext,
            "ctrl-p"    => KeyBindingDialogAction::CyclePrev,
            "backtab"   => KeyBindingDialogAction::CyclePrev,
            "pageup"    => KeyBindingDialogAction::PageUp,
            "pagedown"  => KeyBindingDialogAction::PageDown,
            "ctrl-u"    => KeyBindingDialogAction::PageUp,
            "ctrl-d"    => KeyBindingDialogAction::PageDown,
            "home"      => KeyBindingDialogAction::Home,
            "end"       => KeyBindingDialogAction::End,
            "g"         => KeyBindingDialogAction::Home,
            "G"         => KeyBindingDialogAction::End,
        };

        Self {
            prefix: "ctrl-a".to_string(),
            direct_actions,
            prefix_actions,
            line_mode,
            terminal_mode,
            search_mode,
            copy_mode,
            dialog_actions,
        }
    }
}

impl KeyBindingsConfig {
    pub fn load_or_create_default<P: AsRef<Path>>(path: P) -> Result<Self, KeymapFileError> {
        let path = path.as_ref();

        if path.exists() {
            let content = fs::read_to_string(path).map_err(|err| KeymapFileError::Io {
                path: path.to_path_buf(),
                source: err,
            })?;

            let keymap: KeyBindingsConfig =
                toml::from_str(&content).map_err(|err| KeymapFileError::TomlParse {
                    path: path.to_path_buf(),
                    source: err,
                })?;

            KeyResolver::try_new(&keymap).map_err(|err| KeymapFileError::Mapping {
                path: path.to_path_buf(),
                source: err,
            })?;

            return Ok(keymap);
        }

        let default_keymap = KeyBindingsConfig::default();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| KeymapFileError::Io {
                path: parent.to_path_buf(),
                source: err,
            })?;
        }

        let toml_string =
            toml::to_string_pretty(&default_keymap).map_err(|err| KeymapFileError::Io {
                path: path.to_path_buf(),
                source: std::io::Error::other(err.to_string()),
            })?;

        fs::write(path, toml_string).map_err(|err| KeymapFileError::Io {
            path: path.to_path_buf(),
            source: err,
        })?;

        Ok(default_keymap)
    }
}

#[derive(Debug)]
pub enum KeymapFileError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    TomlParse {
        path: PathBuf,
        source: toml::de::Error,
    },
    Mapping {
        path: PathBuf,
        source: KeymapError,
    },
}

impl std::fmt::Display for KeymapFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    f,
                    "failed to read keymap file '{}': {source}",
                    path.display()
                )
            }
            Self::TomlParse { path, source } => {
                write!(
                    f,
                    "invalid syntax in keymap file '{}':\n{source}",
                    path.display()
                )
            }
            Self::Mapping { path, source } => {
                write!(f, "invalid keybinding in '{}': {source}", path.display())
            }
        }
    }
}

impl std::error::Error for KeymapFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::TomlParse { source, .. } => Some(source),
            Self::Mapping { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(resolver: &KeyResolver, action: KeyBindingDialogAction) -> Vec<String> {
        resolver
            .dialog_keys(action)
            .iter()
            .map(|k| k.to_string())
            .collect()
    }

    /// The keys of an action come in the same order whatever the map's: plain keys before
    /// modified ones, named keys before letters (MAX-73).
    /// How: several resolvers from the default keymap, each map with its own random seed.
    #[test]
    fn dialog_keys_in_a_fixed_order() {
        for _ in 0..8 {
            let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
            assert_eq!(keys(&resolver, KeyBindingDialogAction::Close), ["Esc", "q"]);
            assert_eq!(
                keys(&resolver, KeyBindingDialogAction::CycleNext),
                ["Tab", "Space", "Ctrl-n"]
            );
            assert_eq!(
                keys(&resolver, KeyBindingDialogAction::CyclePrev),
                ["Shift-Tab", "Ctrl-p"]
            );
        }

        let mut config = KeyBindingsConfig::default();
        config.dialog_actions.retain(|_, action| action != "close");
        let resolver = KeyResolver::try_new(&config).unwrap();
        assert!(keys(&resolver, KeyBindingDialogAction::Close).is_empty());
    }

    /// Reload is on prefix + r, and the resize dialog moved to prefix + w (MAX-112).
    #[test]
    fn reload_and_resize_default_keys() {
        let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
        let prefix_action = |key: &str| resolver.prefix_actions[&key.parse().unwrap()];
        assert_eq!(prefix_action("r"), KeyBindingAction::ReloadConfig);
        assert_eq!(prefix_action("w"), KeyBindingAction::ResizeDialog);
        assert_eq!(
            "reload-config".parse::<KeyBindingAction>().unwrap(),
            KeyBindingAction::ReloadConfig
        );
        assert_eq!(KeyBindingAction::ReloadConfig.to_string(), "reload-config");
    }

    /// MAX-129: Ctrl-C is no global quit key: in line mode it is `interrupt`, in terminal mode
    /// unbound so it is sent to the device. Ctrl-Q still quits.
    #[test]
    fn ctrl_c_default_key() {
        let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
        let ctrl_c = "ctrl-c".parse().unwrap();
        assert!(!resolver.direct_actions.contains_key(&ctrl_c));
        assert!(!resolver.terminal_actions.contains_key(&ctrl_c));
        let action = resolver.line_actions[&ctrl_c];
        assert_eq!(action, KeyBindingLineAction::Interrupt);
        assert_eq!(action.to_string(), "interrupt");
        assert_eq!("interrupt".parse::<KeyBindingLineAction>().unwrap(), action);
        assert_eq!(
            resolver.direct_actions[&"ctrl-q".parse().unwrap()],
            KeyBindingAction::Quit
        );
    }

    /// MAX-127: `V` selects whole lines in copy mode by default.
    #[test]
    fn select_line_default_key() {
        let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
        let action = resolver.copy_actions[&"V".parse().unwrap()];
        assert_eq!(action, KeyBindingCopyAction::SelectLine);
        assert_eq!(action.to_string(), "select-line");
        assert_eq!(
            "select-line".parse::<KeyBindingCopyAction>().unwrap(),
            action
        );
    }

    /// MAX-148: prefix + `S` opens the session browser, apart from `s` (settings).
    #[test]
    fn sessions_default_key() {
        let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
        let action = resolver.prefix_actions[&"S".parse().unwrap()];
        assert_eq!(action, KeyBindingAction::Sessions);
        assert_eq!(action.to_string(), "sessions");
        assert_eq!("sessions".parse::<KeyBindingAction>().unwrap(), action);
        let settings = resolver.prefix_actions[&"s".parse().unwrap()];
        assert_eq!(settings, KeyBindingAction::Settings);
    }

    /// MAX-155: clear_history moved from Ctrl-D (stolen from the remote) to prefix + `C`,
    /// shifted because it wipes the history.
    #[test]
    fn clear_history_default_key() {
        let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
        let action = resolver.prefix_actions[&"C".parse().unwrap()];
        assert_eq!(action, KeyBindingAction::ClearHistory);
        assert!(
            !resolver
                .direct_actions
                .contains_key(&"ctrl-d".parse().unwrap())
        );
        assert!(!resolver.prefix_actions.contains_key(&"c".parse().unwrap()));
    }

    /// MAX-155: a key bound in a mode section is reported when the prefix or `[direct_actions]`
    /// take it first; the default keymap has none.
    #[test]
    fn shadowed_keys() {
        let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
        assert!(resolver.shadowed_keys().is_empty());

        let mut config = KeyBindingsConfig::default();
        let bind = |section: &mut BTreeMap<String, String>, key: &str, action: &str| {
            section.insert(key.to_string(), action.to_string());
        };
        bind(&mut config.direct_actions, "alt-x", "quit");
        bind(&mut config.line_mode, "alt-x", "submit");
        bind(&mut config.terminal_mode, "alt-x", "quit");
        bind(&mut config.terminal_mode, "ctrl-a", "quit");
        bind(&mut config.direct_actions, "ctrl-a", "quit");
        let resolver = KeyResolver::try_new(&config).unwrap();
        assert_eq!(
            resolver.shadowed_keys(),
            [
                "Alt-x in [line_mode] (taken by [direct_actions])",
                "Alt-x in [terminal_mode] (taken by [direct_actions])",
                "Ctrl-a in [direct_actions] (taken by the prefix)",
                "Ctrl-a in [terminal_mode] (taken by the prefix)",
            ]
        );
    }

    /// MAX-155: only these dialog actions work in a text input popup.
    #[test]
    fn text_input_dialog_actions() {
        use KeyBindingDialogAction as A;
        for action in [A::Close, A::Confirm, A::CycleNext, A::CyclePrev] {
            assert!(action.is_text_input_action(), "{action}");
        }
        for action in [
            A::PageUp,
            A::PageDown,
            A::Home,
            A::End,
            A::NavUp,
            A::NavLeft,
        ] {
            assert!(!action.is_text_input_action(), "{action}");
        }
    }

    #[test]
    fn mouse_toggle_default_key() {
        let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
        let action = resolver.prefix_actions[&"M".parse().unwrap()];
        assert_eq!(action, KeyBindingAction::ToggleMouse);
        assert_eq!(action.to_string(), "toggle-mouse");
        assert_eq!("toggle-mouse".parse::<KeyBindingAction>().unwrap(), action);
    }

    /// The action names of `keymap.toml` come from the command registry (MAX-12), with `-`
    /// between words: the old `_` names are refused
    #[test]
    fn action_names() {
        const NAMES: [(&str, KeyBindingAction); 42] = [
            ("quit", KeyBindingAction::Quit),
            ("toggle-input-mode", KeyBindingAction::SKInputMode),
            ("toggle-local-echo", KeyBindingAction::SKLocalEcho),
            ("toggle-line-wrap", KeyBindingAction::SKLineWrap),
            (
                "toggle-display-timestamp",
                KeyBindingAction::SKDisplayTimestamp,
            ),
            ("toggle-text-encoding", KeyBindingAction::SKTextEncoding),
            ("toggle-indent-wrap", KeyBindingAction::SKIndentWrap),
            ("toggle-display-mode", KeyBindingAction::SKDisplayMode),
            ("toggle-display-raw", KeyBindingAction::SKDisplayRaw),
            ("screenshot-bytes", KeyBindingAction::ScreenshotBytes),
            ("screenshot-text", KeyBindingAction::ScreenshotTxt),
            ("help", KeyBindingAction::Help),
            ("settings", KeyBindingAction::Settings),
            ("toggle-logging", KeyBindingAction::ToggleLogging),
            ("record-text", KeyBindingAction::RecordText),
            ("record-raw", KeyBindingAction::RecordRaw),
            ("resize-dialog", KeyBindingAction::ResizeDialog),
            ("auto-resize-remote", KeyBindingAction::AutoResizeRemote),
            (
                "auto-resize-remote-line",
                KeyBindingAction::AutoResizeRemoteLine,
            ),
            ("auto-resize-emulator", KeyBindingAction::AutoResizeEmulator),
            ("serial-settings", KeyBindingAction::SerialSettings),
            ("enter-copy-mode", KeyBindingAction::EnterCopyMode),
            ("enter-search", KeyBindingAction::EnterSearch),
            ("scroll-up", KeyBindingAction::ScrollUp),
            ("scroll-down", KeyBindingAction::ScrollDown),
            ("scroll-to-bottom", KeyBindingAction::ScrollToBottom),
            ("scroll-to-top", KeyBindingAction::ScrollToTop),
            ("page-up", KeyBindingAction::PageUp),
            ("page-down", KeyBindingAction::PageDown),
            ("pan-right", KeyBindingAction::PanRight),
            ("pan-left", KeyBindingAction::PanLeft),
            ("pan-home", KeyBindingAction::PanHome),
            ("pan-side-right", KeyBindingAction::PanSideRight),
            ("pan-side-left", KeyBindingAction::PanSideLeft),
            ("clear-history", KeyBindingAction::ClearHistory),
            ("sessions", KeyBindingAction::Sessions),
            ("exit-prefix", KeyBindingAction::ExitPrefix),
            ("detach-serial", KeyBindingAction::DetachSerial),
            ("reload-config", KeyBindingAction::ReloadConfig),
            ("toggle-mouse", KeyBindingAction::ToggleMouse),
            ("command-line", KeyBindingAction::CommandLine),
            ("cancel", KeyBindingAction::CancelCommands),
        ];
        for (name, action) in NAMES {
            assert_eq!(name.parse::<KeyBindingAction>().unwrap(), action, "{name}");
            assert_eq!(action.to_string(), name);
        }
        assert!("toggle_local_echo".parse::<KeyBindingAction>().is_err());
        assert!("".parse::<KeyBindingAction>().is_err());
    }

    /// MAX-12: `[prefix_actions]` and `[direct_actions]` take a command line where an action
    /// name goes, parsed when the keymap loads; an action name stays an action. A value that
    /// is neither fails the load, naming its key. A mode key under a direct line is warned.
    /// How: the ESP32 reset into the bootloader on F6, `send-hex` on F5, the default break and
    /// DTR pulse, then typos.
    #[test]
    fn bound_command_lines() {
        let mut config = KeyBindingsConfig::default();
        let esp32 = "dtr off; rts on; sleep 100; dtr on; rts off; sleep 50; dtr off";
        config.prefix_actions.insert("f6".into(), esp32.into());
        config
            .direct_actions
            .insert("f5".into(), " send-hex 03 ".into());
        config.line_mode.insert("f5".into(), "submit".into());
        let resolver = KeyResolver::try_new(&config).unwrap();
        let key = |name: &str| name.parse::<KeyPattern>().unwrap();
        let f6 = &resolver.prefix_commands[&key("f6")];
        assert_eq!((f6.text.as_str(), f6.invocations.len()), (esp32, 7));
        assert_eq!(resolver.direct_commands[&key("f5")].text, "send-hex 03");
        assert_eq!(
            resolver.prefix_actions[&key("h")],
            KeyBindingAction::DetachSerial
        );
        assert!(!resolver.prefix_commands.contains_key(&key("h")));
        // the default ones
        assert_eq!(resolver.prefix_commands[&key("f")].invocations.len(), 1);
        assert_eq!(resolver.prefix_commands[&key("p")].invocations.len(), 3);
        assert_eq!(
            resolver.shadowed_keys(),
            ["F5 in [line_mode] (taken by [direct_actions])"]
        );

        let error = |value: &str| {
            let mut config = KeyBindingsConfig::default();
            config.prefix_actions.insert("f7".into(), value.into());
            KeyResolver::try_new(&config).err().unwrap().to_string()
        };
        assert_eq!(
            error("rst on"),
            "in [prefix_actions]: f7: unknown command 'rst'"
        );
        assert_eq!(
            error("baud 10"),
            "in [prefix_actions]: f7: baud: bad argument 1: 10 is out of 50..=4000000"
        );
        assert_eq!(error(" ; "), "in [prefix_actions]: unknown action ' ; '");
    }

    /// Keys a text input popup still sends to `[dialog_actions]`: the others are typed.
    #[test]
    fn modal_keys() {
        for key in ["esc", "enter", "tab", "backtab", "ctrl-n", "alt-x"] {
            assert!(key.parse::<KeyPattern>().unwrap().is_modal(), "{key}");
        }
        for key in ["q", "space", "down", "shift-x"] {
            assert!(!key.parse::<KeyPattern>().unwrap().is_modal(), "{key}");
        }
    }
}
