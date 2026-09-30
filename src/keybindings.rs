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
pub struct KeyPattern {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyPattern {
    pub fn from_event(ev: &KeyEvent) -> Self {
        let (code, modifiers) = match ev.code {
            KeyCode::Char(c) if c.is_ascii_uppercase() => (
                KeyCode::Char(c.to_ascii_lowercase()),
                ev.modifiers - KeyModifiers::SHIFT,
            ),
            KeyCode::Char(c) => (KeyCode::Char(c.to_ascii_lowercase()), ev.modifiers),
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
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => modifiers.insert(KeyModifiers::CONTROL),
                "alt" => modifiers.insert(KeyModifiers::ALT),
                "shift" => modifiers.insert(KeyModifiers::SHIFT),
                "enter" | "return" => code = Some(KeyCode::Enter),
                "esc" | "escape" => code = Some(KeyCode::Esc),
                "backspace" => code = Some(KeyCode::Backspace),
                "tab" => code = Some(KeyCode::Tab),
                "space" => code = Some(KeyCode::Char(' ')),
                "up" => code = Some(KeyCode::Up),
                "down" => code = Some(KeyCode::Down),
                "left" => code = Some(KeyCode::Left),
                "right" => code = Some(KeyCode::Right),
                "pageup" => code = Some(KeyCode::PageUp),
                "pagedown" => code = Some(KeyCode::PageDown),
                "home" => code = Some(KeyCode::Home),
                "end" => code = Some(KeyCode::End),
                f if f.starts_with('f') && f.len() > 1 => {
                    let num = f[1..].parse::<u8>().map_err(|_| "Invalid F-key")?;
                    code = Some(KeyCode::F(num));
                }
                c if c.chars().count() == 1 => {
                    let ch = c.chars().next().unwrap().to_ascii_lowercase();
                    code = Some(KeyCode::Char(ch));
                }
                other => return Err(format!("Unknown key sequence: {other}")),
            }
        }

        let code = code.ok_or_else(|| "Missing key code".to_string())?;
        Ok(Self { code, modifiers })
    }
}

pub struct KeyResolver {
    pub prefix: KeyPattern,
    pub prefix_actions: HashMap<KeyPattern, KeyBindingPrefixAction>,
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

        Ok(Self {
            prefix,
            prefix_actions,
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
    pub copy_mode: BTreeMap<String, String>,
}

impl Default for KeyBindingsConfig {
    fn default() -> Self {
        let direct = btreemap! {
            "ctrl-q"   => "quit",
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
            "a"     => KeyBindingPrefixAction::ExitPrefix,
        };

        let copy_mode = btreemap! {
            "q"     => "exit",
            "esc"   => "exit",
            "v"     => "toggle_selection",
            "y"     => "yank",
            "h"     => "move_left",
            "left"  => "move_left",
            "j"     => "move_down",
            "down"  => "move_down",
            "k"     => "move_up",
            "up"    => "move_up",
            "l"     => "move_right",
            "right" => "move_right",
            "0"     => "start_of_line",
            "$"     => "end_of_line",
        };

        Self {
            prefix: "ctrl-a".to_string(),
            direct,
            prefix_actions,
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
