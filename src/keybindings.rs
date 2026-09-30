use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::Path,
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
pub enum KeyBindingPrefixAction {
    Quit,
    ToggleInputMode,
    Help,
    Settings,
    ToggleLogging,
    ResizeDialog,
    AutoResizeRemote,
    AutoResizeEmulator,
    SerialSettings,
    EnterCopyMode,
    EnterSearch,
    ScrollUp,
    ScrollDown,
    PanRight,
    PanLeft,
    ExitPrefix,
}

impl FromStr for KeyBindingPrefixAction {
    type Err = UnknownActionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "quit" => Ok(KeyBindingPrefixAction::Quit),
            "toggle_input_mode" => Ok(KeyBindingPrefixAction::ToggleInputMode),
            "help" => Ok(KeyBindingPrefixAction::Help),
            "settings" => Ok(KeyBindingPrefixAction::Settings),
            "toggle_logging" => Ok(KeyBindingPrefixAction::ToggleLogging),
            "resize_dialog" => Ok(KeyBindingPrefixAction::ResizeDialog),
            "auto_resize_remote" => Ok(KeyBindingPrefixAction::AutoResizeRemote),
            "auto_resize_emulator" => Ok(KeyBindingPrefixAction::AutoResizeEmulator),
            "serial_settings" => Ok(KeyBindingPrefixAction::SerialSettings),
            "enter_copy_mode" => Ok(KeyBindingPrefixAction::EnterCopyMode),
            "enter_search" => Ok(KeyBindingPrefixAction::EnterSearch),
            "scroll_up" => Ok(KeyBindingPrefixAction::ScrollUp),
            "scroll_down" => Ok(KeyBindingPrefixAction::ScrollDown),
            "pan_right" => Ok(KeyBindingPrefixAction::PanRight),
            "pan_left" => Ok(KeyBindingPrefixAction::PanLeft),
            "exit_prefix" => Ok(KeyBindingPrefixAction::ExitPrefix),
            other => Err(UnknownActionError(other.to_string())),
        }
    }
}

impl std::fmt::Display for KeyBindingPrefixAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Quit => "quit",
            Self::ToggleInputMode => "toggle_input_mode",
            Self::Help => "help",
            Self::Settings => "settings",
            Self::ToggleLogging => "toggle_logging",
            Self::ResizeDialog => "resize_dialog",
            Self::AutoResizeRemote => "auto_resize_remote",
            Self::AutoResizeEmulator => "auto_resize_emulator",
            Self::SerialSettings => "serial_settings",
            Self::EnterCopyMode => "enter_copy_mode",
            Self::EnterSearch => "enter_search",
            Self::ScrollUp => "scroll_up",
            Self::ScrollDown => "scroll_down",
            Self::PanRight => "pan_right",
            Self::PanLeft => "pan_left",
            Self::ExitPrefix => "exit_prefix",
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

pub struct KeyResolver {
    pub prefix: KeyPattern,
    pub prefix_actions: HashMap<KeyPattern, KeyBindingPrefixAction>,
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
            let act: KeyBindingPrefixAction =
                action_str.parse().map_err(|_| KeymapError::InvalidAction {
                    section: "prefix_actions",
                    action: action_str.clone(),
                })?;
            prefix_actions.insert(pat, act);
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

macro_rules! btreemap {
    ($( $key:expr => $val:expr ),* $(,)?) => {{
        let mut map = std::collections::BTreeMap::new();
        $(
            map.insert($key.to_string(), $val.to_string());
        )*
        map
    }};
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyBindingsConfig {
    pub prefix: String,

    pub direct: BTreeMap<String, String>,
    pub prefix_actions: BTreeMap<String, String>,
    pub search_mode: BTreeMap<String, String>,
    pub copy_mode: BTreeMap<String, String>,
}

impl Default for KeyBindingsConfig {
    fn default() -> Self {
        let direct = btreemap! {
            "ctrl-q"   => "quit",
            "ctrl-f"   => "enter-search",
            "pageup"   => "scroll_page_up",
            "pagedown" => "scroll_page_down",
        };

        let prefix_actions = default_bindings! {
            "q"     => KeyBindingPrefixAction::Quit,
            "x"     => KeyBindingPrefixAction::Quit,
            "m"     => KeyBindingPrefixAction::ToggleInputMode,
            "z"     => KeyBindingPrefixAction::Help,
            "s"     => KeyBindingPrefixAction::Settings,
            "l"     => KeyBindingPrefixAction::ToggleLogging,
            "r"     => KeyBindingPrefixAction::ResizeDialog,
            "v"     => KeyBindingPrefixAction::AutoResizeRemote,
            "t"     => KeyBindingPrefixAction::AutoResizeEmulator,
            "d"     => KeyBindingPrefixAction::SerialSettings,
            "up"    => KeyBindingPrefixAction::ScrollUp,
            "down"  => KeyBindingPrefixAction::ScrollDown,
            "left"  => KeyBindingPrefixAction::PanLeft,
            "right" => KeyBindingPrefixAction::PanRight,
            "["     => KeyBindingPrefixAction::EnterCopyMode,
            "/"     => KeyBindingPrefixAction::EnterSearch,
            "a"     => KeyBindingPrefixAction::ExitPrefix,
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
            direct,
            prefix_actions,
            search_mode,
            copy_mode,
        }
    }
}

impl KeyBindingsConfig {
    pub fn load_or_create_default<P: AsRef<Path>>(
        path: P,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.as_ref();

        if path.exists() {
            let content = fs::read_to_string(path)?;
            let keymap: KeyBindingsConfig = toml::from_str(&content)?;
            return Ok(keymap);
        }

        let default_keymap = KeyBindingsConfig::default();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let toml_string = toml::to_string_pretty(&default_keymap)?;
        fs::write(path, toml_string)?;

        Ok(default_keymap)
    }
}
