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
    SKIndentWrap,
    SKDisplayMode,
    SKDisplayRaw,
    Help,
    Settings,
    ToggleLogging,
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
    PanSideRight,
    PanSideLeft,
    ClearHistory,
    DetachSerial,
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
            "toggle_indent_wrap" => Ok(KeyBindingAction::SKIndentWrap),
            "toggle_display_mode" => Ok(KeyBindingAction::SKDisplayMode),
            "toggle_display_raw" => Ok(KeyBindingAction::SKDisplayRaw),
            "screenshot_bytes" => Ok(KeyBindingAction::ScreenshotBytes),
            "screenshot_text" => Ok(KeyBindingAction::ScreenshotTxt),
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
            "pan_side_right" => Ok(KeyBindingAction::PanSideRight),
            "pan_side_left" => Ok(KeyBindingAction::PanSideLeft),
            "clear_history" => Ok(KeyBindingAction::ClearHistory),
            "exit_prefix" => Ok(KeyBindingAction::ExitPrefix),
            "detach_serial" => Ok(KeyBindingAction::DetachSerial),
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
            Self::SKIndentWrap => "toggle_indent_wrap",
            Self::SKDisplayMode => "toggle_display_mode",
            Self::SKDisplayRaw => "toggle_display_raw",
            Self::ScreenshotBytes => "screenshot_bytes",
            Self::ScreenshotTxt => "screenshot_text",
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
            Self::PanSideRight => "pan_side_right",
            Self::PanSideLeft => "pan_side_left",
            Self::ClearHistory => "clear_history",
            Self::ExitPrefix => "exit_prefix",
            Self::DetachSerial => "detach_serial",
        };
        write!(f, "{s}")
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
            "search_kind" => Ok(Self::Kind),
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
            Self::Kind => "search_kind",
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
    YankSideInspector,
    ToggleSide,
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
            "yank_side_inspector" => Ok(Self::YankSideInspector),
            "toggle_side" => Ok(Self::ToggleSide),
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
            Self::YankSideInspector => "yank_side_inspector",
            Self::ToggleSide => "toggle_side",
            Self::Exit => "exit",
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
            "cycle_next" => Ok(Self::CycleNext),
            "cycle_prev" => Ok(Self::CyclePrev),
            "page_up" => Ok(Self::PageUp),
            "page_down" => Ok(Self::PageDown),
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
            KeyBindingDialogAction::CycleNext => "cycle_next",
            KeyBindingDialogAction::CyclePrev => "cycle_prev",
            KeyBindingDialogAction::PageUp => "page_up",
            KeyBindingDialogAction::PageDown => "page_down",
            KeyBindingDialogAction::Home => "home",
            KeyBindingDialogAction::End => "end",
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

pub struct KeyResolver {
    pub prefix: KeyPattern,
    pub prefix_actions: HashMap<KeyPattern, KeyBindingAction>,
    pub direct_actions: HashMap<KeyPattern, KeyBindingAction>,
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

    /// The keys bound to a search action, for the search bar hint (MAX-73)
    pub fn search_keys(&self, action: KeyBindingSearchAction) -> Vec<KeyPattern> {
        keys_of(&self.search_actions, action)
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
            "ctrl-q"     => KeyBindingAction::Quit,
            "ctrl-c"     => KeyBindingAction::Quit,
            "ctrl-f"     => KeyBindingAction::EnterSearch,
            "ctrl-d"     => KeyBindingAction::ClearHistory,
            "pageup"     => KeyBindingAction::PageUp,
            "pagedown"   => KeyBindingAction::PageDown,
            "shift-home" => KeyBindingAction::ScrollToTop,
            "shift-end"  => KeyBindingAction::ScrollToBottom,
            "ctrl-right" => KeyBindingAction::PanSideRight,
            "ctrl-left"  => KeyBindingAction::PanSideLeft,
        };

        let prefix_actions = default_bindings! {
            "q"     => KeyBindingAction::Quit,
            "x"     => KeyBindingAction::Quit,
            "m"     => KeyBindingAction::SKInputMode,
            "o"     => KeyBindingAction::SKTextEncoding,
            "B"     => KeyBindingAction::SKDisplayRaw,
            "z"     => KeyBindingAction::Help,
            "s"     => KeyBindingAction::Settings,
            "l"     => KeyBindingAction::ToggleLogging,
            "r"     => KeyBindingAction::ResizeDialog,
            "v"     => KeyBindingAction::AutoResizeRemote,
            "b"     => KeyBindingAction::AutoResizeRemoteLine,
            "t"     => KeyBindingAction::AutoResizeEmulator,
            "d"     => KeyBindingAction::SerialSettings,
            "k"     => KeyBindingAction::ScreenshotBytes,
            "up"    => KeyBindingAction::ScrollUp,
            "down"  => KeyBindingAction::ScrollDown,
            "left"  => KeyBindingAction::PanLeft,
            "right" => KeyBindingAction::PanRight,
            "["     => KeyBindingAction::EnterCopyMode,
            "/"     => KeyBindingAction::EnterSearch,
            "a"     => KeyBindingAction::ExitPrefix,
            "h"     => KeyBindingAction::DetachSerial,
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
            "y"         => KeyBindingCopyAction::Yank,
            "enter"     => KeyBindingCopyAction::Yank,
            "Y"         => KeyBindingCopyAction::YankSideInspector,
            "tab"       => KeyBindingCopyAction::ToggleSide,
            "q"         => KeyBindingCopyAction::Exit,
            "esc"       => KeyBindingCopyAction::Exit,
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
