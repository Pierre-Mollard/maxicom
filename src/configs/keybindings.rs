use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

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
    Help,
    Settings,
    ToggleLogging,
    ResizeDialog,
    AutoResizeRemote,
    AutoResizeRemoteLine,
    AutoResizeEmulator,
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
    ClearHistory,
    ExitPrefix,
}

impl FromStr for KeyBindingAction {
    type Err = UnknownActionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "quit" => Ok(KeyBindingAction::Quit),
            "toggle_input_mode" => Ok(KeyBindingAction::SKInputMode),
            "toggle_local_echo" => Ok(KeyBindingAction::SKLocalEcho),
            "toggle_line_wrap" => Ok(KeyBindingAction::SKLineWrap),
            "toggle_display_timestamp" => Ok(KeyBindingAction::SKDisplayTimestamp),
            "toggle_text_encoding" => Ok(KeyBindingAction::SKTextEncoding),
            "help" => Ok(KeyBindingAction::Help),
            "settings" => Ok(KeyBindingAction::Settings),
            "toggle_logging" => Ok(KeyBindingAction::ToggleLogging),
            "resize_dialog" => Ok(KeyBindingAction::ResizeDialog),
            "auto_resize_remote" => Ok(KeyBindingAction::AutoResizeRemote),
            "auto_resize_remote_line" => Ok(KeyBindingAction::AutoResizeRemoteLine),
            "auto_resize_emulator" => Ok(KeyBindingAction::AutoResizeEmulator),
            "serial_settings" => Ok(KeyBindingAction::SerialSettings),
            "enter_copy_mode" => Ok(KeyBindingAction::EnterCopyMode),
            "enter_search" => Ok(KeyBindingAction::EnterSearch),
            "scroll_up" => Ok(KeyBindingAction::ScrollUp),
            "scroll_down" => Ok(KeyBindingAction::ScrollDown),
            "scroll_to_bottom" => Ok(KeyBindingAction::ScrollToBottom),
            "scroll_to_top" => Ok(KeyBindingAction::ScrollToTop),
            "page_up" => Ok(KeyBindingAction::PageUp),
            "page_down" => Ok(KeyBindingAction::PageDown),
            "pan_right" => Ok(KeyBindingAction::PanRight),
            "pan_left" => Ok(KeyBindingAction::PanLeft),
            "clear_history" => Ok(KeyBindingAction::ClearHistory),
            "exit_prefix" => Ok(KeyBindingAction::ExitPrefix),
            other => Err(UnknownActionError(other.to_string())),
        }
    }
}

impl std::fmt::Display for KeyBindingAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Quit => "quit",
            Self::SKInputMode => "toggle_input_mode",
            Self::SKLocalEcho => "toggle_local_echo",
            Self::SKLineWrap => "toggle_line_wrap",
            Self::SKDisplayTimestamp => "toggle_display_timestamp",
            Self::SKTextEncoding => "toggle_text_encoding",
            Self::Help => "help",
            Self::Settings => "settings",
            Self::ToggleLogging => "toggle_logging",
            Self::ResizeDialog => "resize_dialog",
            Self::AutoResizeRemote => "auto_resize_remote",
            Self::AutoResizeRemoteLine => "auto_resize_remote_line",
            Self::AutoResizeEmulator => "auto_resize_emulator",
            Self::SerialSettings => "serial_settings",
            Self::EnterCopyMode => "enter_copy_mode",
            Self::EnterSearch => "enter_search",
            Self::ScrollUp => "scroll_up",
            Self::ScrollDown => "scroll_down",
            Self::ScrollToBottom => "scroll_to_bottom",
            Self::ScrollToTop => "scroll_to_top",
            Self::PageUp => "page_up",
            Self::PageDown => "page_down",
            Self::PanRight => "pan_right",
            Self::PanLeft => "pan_left",
            Self::ClearHistory => "clear_history",
            Self::ExitPrefix => "exit_prefix",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyBindingLineAction {
    Submit,
    HistoryPrev,
    HistoryNext,
    AutoResizeRemoteLine,
    ClearLine,
    Quit,
}

impl std::str::FromStr for KeyBindingLineAction {
    type Err = UnknownActionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "submit" => Ok(Self::Submit),
            "history_prev" => Ok(Self::HistoryPrev),
            "history_next" => Ok(Self::HistoryNext),
            "auto_resize_remote_line" => Ok(Self::AutoResizeRemoteLine),
            "clear_line" => Ok(Self::ClearLine),
            "quit" => Ok(Self::Quit),
            other => Err(UnknownActionError(other.to_string())),
        }
    }
}

impl std::fmt::Display for KeyBindingLineAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Submit => "submit",
            Self::HistoryPrev => "history_prev",
            Self::HistoryNext => "history_next",
            Self::AutoResizeRemoteLine => "auto_resize_remote_line",
            Self::ClearLine => "clear_line",
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
            "auto_resize_remote" => Ok(Self::AutoResizeRemote),
            "auto_resize_emulator" => Ok(Self::AutoResizeEmulator),
            "quit" => Ok(Self::Quit),
            other => Err(UnknownActionError(other.to_string())),
        }
    }
}

impl std::fmt::Display for KeyBindingTerminalAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::AutoResizeRemote => "auto_resize_remote",
            Self::AutoResizeEmulator => "auto_resize_emulator",
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
}

impl std::str::FromStr for KeyBindingSearchAction {
    type Err = UnknownActionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "next" => Ok(Self::Next),
            "previous" => Ok(Self::Previous),
            "cancel" => Ok(Self::Cancel),
            "confirm" => Ok(Self::Confirm),
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
    Yank,
    Exit,
}

impl std::str::FromStr for KeyBindingCopyAction {
    type Err = UnknownActionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "page_up" => Ok(Self::PageUp),
            "page_down" => Ok(Self::PageDown),
            "start_of_line" => Ok(Self::StartOfLine),
            "end_of_line" => Ok(Self::EndOfLine),
            "select" => Ok(Self::Select),
            "yank" => Ok(Self::Yank),
            "exit" => Ok(Self::Exit),
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
            Self::PageUp => "page_up",
            Self::PageDown => "page_down",
            Self::StartOfLine => "start_of_line",
            Self::EndOfLine => "end_of_line",
            Self::Select => "select",
            Self::Yank => "yank",
            Self::Exit => "exit",
        };
        write!(f, "{s}")
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
            other => (other, ev.modifiers),
        };

        Self { code, modifiers }
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

        let code = code.ok_or_else(|| "Missing key code".to_string())?;
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
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            parts.push("Shift");
        }

        let key_str = match self.code {
            KeyCode::Enter => "Enter".to_string(),
            KeyCode::Esc => "Esc".to_string(),
            KeyCode::Backspace => "Backspace".to_string(),
            KeyCode::Tab => "Tab".to_string(),
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
            KeyCode::Char(c) => c.to_uppercase().to_string(),
            _ => "?".to_string(),
        };

        parts.push(&key_str);
        write!(f, "{}", parts.join("-"))
    }
}

pub struct KeyResolver {
    pub prefix: KeyPattern,
    pub prefix_actions: HashMap<KeyPattern, KeyBindingAction>,
    pub direct_actions: HashMap<KeyPattern, KeyBindingAction>,
    pub line_actions: HashMap<KeyPattern, KeyBindingLineAction>,
    pub terminal_actions: HashMap<KeyPattern, KeyBindingTerminalAction>,
    pub search_actions: HashMap<KeyPattern, KeyBindingSearchAction>,
    pub copy_actions: HashMap<KeyPattern, KeyBindingCopyAction>,
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
        }
    }
}

impl std::error::Error for KeymapError {}

impl KeyResolver {
    pub fn try_new(config: &KeyBindingsConfig) -> Result<Self, KeymapError> {
        let prefix = config
            .prefix
            .parse()
            .map_err(|err| KeymapError::InvalidKey {
                section: "root",
                key: config.prefix.clone(),
                reason: err,
            })?;

        let mut prefix_actions = HashMap::new();
        for (k, action_str) in &config.prefix_actions {
            let pat = k.parse().map_err(|err| KeymapError::InvalidKey {
                section: "prefix_actions",
                key: k.clone(),
                reason: err,
            })?;
            let act: KeyBindingAction =
                action_str.parse().map_err(|_| KeymapError::InvalidAction {
                    section: "prefix_actions",
                    action: action_str.clone(),
                })?;
            prefix_actions.insert(pat, act);
        }

        let mut direct_actions = HashMap::new();
        for (k, action_str) in &config.direct_actions {
            let pat = k.parse().map_err(|err| KeymapError::InvalidKey {
                section: "direct_actions",
                key: k.clone(),
                reason: err,
            })?;
            let act: KeyBindingAction =
                action_str.parse().map_err(|_| KeymapError::InvalidAction {
                    section: "direct_actions",
                    action: action_str.clone(),
                })?;
            direct_actions.insert(pat, act);
        }

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

        Ok(Self {
            prefix,
            prefix_actions,
            direct_actions,
            line_actions,
            terminal_actions,
            search_actions,
            copy_actions,
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
}

impl Default for KeyBindingsConfig {
    fn default() -> Self {
        let direct_actions = default_bindings! {
            "ctrl-q"     => KeyBindingAction::Quit,
            "ctrl-c"     => KeyBindingAction::Quit,
            "ctrl-f"     => KeyBindingAction::EnterSearch,
            "ctrl-d"     => KeyBindingAction::ClearHistory,
            "pageup"     => KeyBindingAction::PageUp,
            "pagedown"   => KeyBindingAction::PageDown,
            "shift-home" => KeyBindingAction::ScrollToTop,
            "shift-end"  => KeyBindingAction::ScrollToBottom,
        };

        let prefix_actions = default_bindings! {
            "q"     => KeyBindingAction::Quit,
            "x"     => KeyBindingAction::Quit,
            "m"     => KeyBindingAction::SKInputMode,
            "o"     => KeyBindingAction::SKTextEncoding,
            "z"     => KeyBindingAction::Help,
            "s"     => KeyBindingAction::Settings,
            "l"     => KeyBindingAction::ToggleLogging,
            "r"     => KeyBindingAction::ResizeDialog,
            "v"     => KeyBindingAction::AutoResizeRemote,
            "b"     => KeyBindingAction::AutoResizeRemoteLine,
            "t"     => KeyBindingAction::AutoResizeEmulator,
            "d"     => KeyBindingAction::SerialSettings,
            "up"    => KeyBindingAction::ScrollUp,
            "down"  => KeyBindingAction::ScrollDown,
            "left"  => KeyBindingAction::PanLeft,
            "right" => KeyBindingAction::PanRight,
            "["     => KeyBindingAction::EnterCopyMode,
            "/"     => KeyBindingAction::EnterSearch,
            "a"     => KeyBindingAction::ExitPrefix,
        };

        let line_mode = default_bindings! {
            "enter"  => KeyBindingLineAction::Submit,
            "up"     => KeyBindingLineAction::HistoryPrev,
            "down"   => KeyBindingLineAction::HistoryNext,
            "ctrl-c" => KeyBindingLineAction::Quit,
            "ctrl-u" => KeyBindingLineAction::ClearLine,
            "alt-r"  => KeyBindingLineAction::AutoResizeRemoteLine,
        };

        let terminal_mode = default_bindings! {
            "alt-r" => KeyBindingTerminalAction::AutoResizeRemote,
            "alt-v" => KeyBindingTerminalAction::AutoResizeEmulator,
            "alt-q" => KeyBindingTerminalAction::Quit,
        };

        let search_mode = default_bindings! {
            "ctrl-n"     => KeyBindingSearchAction::Next,
            "ctrl-p"     => KeyBindingSearchAction::Previous,
            "esc"        => KeyBindingSearchAction::Cancel,
            "enter"      => KeyBindingSearchAction::Confirm,
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
            "y"         => KeyBindingCopyAction::Yank,
            "enter"     => KeyBindingCopyAction::Yank,
            "q"         => KeyBindingCopyAction::Exit,
            "esc"       => KeyBindingCopyAction::Exit,
        };

        Self {
            prefix: "ctrl-a".to_string(),
            direct_actions,
            prefix_actions,
            line_mode,
            terminal_mode,
            search_mode,
            copy_mode,
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
