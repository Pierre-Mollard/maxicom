use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, btree_map},
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::{
    commands::{self, CommandError, CommandSpec, Invocation, NamedCommand, Target},
    configs::comments::{KEYMAP_COMMENTS, with_comments},
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
    Record,
    ResizeDialog,
    AutoResizeRemote,
    AutoResizeRemoteLine,
    AutoResizeEmulator,
    ScreenshotTxt,
    ScreenshotBytes,
    LinkSettings,
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
    ToggleAttach,
    ExitPrefix,
    ReloadConfig,
    ToggleMouse,
    CommandLine,
    CancelCommands,
    ToggleTriggers,
    Triggers,
    Clear,
    SKAddCr,
    SKAddLf,
    Directory,
    SideGrow,
    SideShrink,
    SideGrow5,
    SideShrink5,
    DismissNotification,
    DismissNotifications,
    PrevMark,
    NextMark,
    Marks,
    EnterSearchBackward,
    PanEnd,
    PlotFreeze,
    PlotNext,
    PlotPrev,
    /// A command of the registry run without arguments (MAX-239)
    Command(&'static CommandSpec),
}

impl FromStr for KeyBindingAction {
    type Err = UnknownActionError;

    /// An action's name is its command's in the registry (MAX-12), a command that runs
    /// without arguments included (MAX-239)
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match commands::find_by_name(name) {
            Some(spec) => match spec.target {
                Target::Action(action) => Ok(action),
                Target::Build(_) if spec.runs_bare() => Ok(Self::Command(spec)),
                Target::Build(_) => Err(UnknownActionError(name.to_string())),
            },
            None => Err(UnknownActionError(name.to_string())),
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
    /// What the action does, its command's help (MAX-239)
    pub fn help(self) -> &'static str {
        commands::find_by_action(self).map_or("", |spec| spec.help)
    }

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
                | Self::PanEnd
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

impl KeyBindingLineAction {
    /// What the action does, for the help (MAX-239)
    pub fn help(self) -> &'static str {
        match self {
            Self::Submit => "Send the line",
            Self::HistoryPrev => "The line sent before",
            Self::HistoryNext => "The line sent after",
            Self::AutoResizeRemoteLine => "Send the view's width to the device",
            Self::ClearLine => "Clear the input",
            Self::Interrupt => "Send Ctrl-c (0x03)",
            Self::PageUp => "Scroll the history up one page",
            Self::PageDown => "Scroll the history down one page",
            Self::Quit => "Quit maxicom",
        }
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

impl KeyBindingTerminalAction {
    /// What the action does, for the help (MAX-239)
    pub fn help(self) -> &'static str {
        match self {
            Self::AutoResizeRemote => "Send the emulator's size to the device",
            Self::AutoResizeEmulator => "Resize the emulator to the view",
            Self::Quit => "Quit maxicom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyBindingSearchAction {
    Next,
    Previous,
    Cancel,
    Confirm,
    Kind,
    HistoryPrev,
    HistoryNext,
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
            "history-prev" => Ok(Self::HistoryPrev),
            "history-next" => Ok(Self::HistoryNext),
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
            Self::HistoryPrev => "history-prev",
            Self::HistoryNext => "history-next",
        };
        write!(f, "{s}")
    }
}

impl KeyBindingSearchAction {
    /// What the action does, for the help (MAX-239)
    pub fn help(self) -> &'static str {
        match self {
            Self::Next => "Next match",
            Self::Previous => "Previous match",
            Self::Cancel => "Close the bar, back where the search started",
            Self::Confirm => "Close the bar, the cursor on the match",
            Self::Kind => "Search text or bytes",
            Self::HistoryPrev => "The search run before",
            Self::HistoryNext => "The search run after",
        }
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
    Mark,
    Marks,
    PrevMark,
    NextMark,
    Search,
    SearchBackward,
    SearchNext,
    SearchPrev,
    SearchWord,
    SearchWordBackward,
}

impl KeyBindingCopyAction {
    /// What the action does, for the help (MAX-239)
    pub fn help(self) -> &'static str {
        match self {
            Self::Up => "Cursor up",
            Self::Down => "Cursor down",
            Self::Left => "Cursor left",
            Self::Right => "Cursor right",
            Self::PageUp => "Cursor up one page",
            Self::PageDown => "Cursor down one page",
            Self::StartOfLine => "Cursor to the line start",
            Self::EndOfLine => "Cursor to the line end",
            Self::Select => "Start a selection",
            Self::SelectLine => "Select whole lines",
            Self::Yank => "Copy and leave",
            Self::YankSideInspector => "Copy the bytes as the raw panel shows them",
            Self::ToggleSide => "Cursor to the raw panel and back",
            Self::Exit => "Leave copy mode",
            Self::ScrollUp => "Scroll the view up a line",
            Self::ScrollDown => "Scroll the view down a line",
            Self::ScrollPageUp => "Scroll the view up a page",
            Self::ScrollPageDown => "Scroll the view down a page",
            Self::Mark => {
                "Mark the cursor's line, waits for its letter (Enter: the first free one)"
            }
            Self::Marks => "Open the marks",
            Self::PrevMark => "Cursor to the mark above",
            Self::NextMark => "Cursor to the mark below",
            Self::Search => "Search from the cursor, forward",
            Self::SearchBackward => "Search from the cursor, backward",
            Self::SearchNext => "Next match of the last search, in its direction",
            Self::SearchPrev => "Next match of the last search, the other way",
            Self::SearchWord => "Search the word under the cursor, forward",
            Self::SearchWordBackward => "Search the word under the cursor, backward",
        }
    }

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
            "mark" => Ok(Self::Mark),
            "marks" => Ok(Self::Marks),
            "prev-mark" => Ok(Self::PrevMark),
            "next-mark" => Ok(Self::NextMark),
            "search" => Ok(Self::Search),
            "search-backward" => Ok(Self::SearchBackward),
            "search-next" => Ok(Self::SearchNext),
            "search-prev" => Ok(Self::SearchPrev),
            "search-word" => Ok(Self::SearchWord),
            "search-word-backward" => Ok(Self::SearchWordBackward),
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
            Self::Mark => "mark",
            Self::Marks => "marks",
            Self::PrevMark => "prev-mark",
            Self::NextMark => "next-mark",
            Self::Search => "search",
            Self::SearchBackward => "search-backward",
            Self::SearchNext => "search-next",
            Self::SearchPrev => "search-prev",
            Self::SearchWord => "search-word",
            Self::SearchWordBackward => "search-word-backward",
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
    /// What the action does, for the help (MAX-239)
    pub fn help(self) -> &'static str {
        match self {
            Self::Close => "Close the popup",
            Self::Confirm => "Apply, open",
            Self::NavUp => "Move up",
            Self::NavDown => "Move down",
            Self::NavLeft => "Change a value, switch a tab",
            Self::NavRight => "Change a value, switch a tab",
            Self::CycleNext => "Next field or value",
            Self::CyclePrev => "Previous field or value",
            Self::PageUp => "Up one page",
            Self::PageDown => "Down one page",
            Self::Home => "To the first",
            Self::End => "To the last",
        }
    }

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

    /// Where the key sorts in the help: plain keys before Ctrl and Alt ones, then letters
    /// (`c` then `C`), symbols, and named keys (arrows, pages, Enter...), each one followed by
    /// its Shift version
    pub fn help_order(&self) -> (u8, u8, u32, bool) {
        let ctrl = self.modifiers.contains(KeyModifiers::CONTROL);
        let alt = self.modifiers.contains(KeyModifiers::ALT);
        let modifiers = u8::from(ctrl) + 2 * u8::from(alt);
        let (class, rank, upper) = match self.code {
            KeyCode::Char(c) if c.is_ascii_alphabetic() => {
                (0, c.to_ascii_lowercase() as u32, c.is_ascii_uppercase())
            }
            KeyCode::Char(' ') => (2, 8, false),
            KeyCode::Char(c) => (1, c as u32, false),
            KeyCode::Up => (2, 0, false),
            KeyCode::Down => (2, 1, false),
            KeyCode::Left => (2, 2, false),
            KeyCode::Right => (2, 3, false),
            KeyCode::PageUp => (2, 4, false),
            KeyCode::PageDown => (2, 5, false),
            KeyCode::Home => (2, 6, false),
            KeyCode::End => (2, 7, false),
            KeyCode::Enter => (2, 9, false),
            KeyCode::Tab | KeyCode::BackTab => (2, 10, false),
            KeyCode::Esc => (2, 11, false),
            KeyCode::Backspace => (2, 12, false),
            KeyCode::Delete => (2, 13, false),
            KeyCode::Insert => (2, 14, false),
            KeyCode::F(n) => (2, 100 + u32::from(n), false),
            _ => (3, 0, false),
        };
        let shift = upper || self.modifiers.contains(KeyModifiers::SHIFT);
        (modifiers, class, rank, shift)
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

        // a shifted letter shows as typed: `C`, not `Shift-c`
        let shifted_letter = self.modifiers.contains(KeyModifiers::SHIFT)
            && matches!(self.code, KeyCode::Char(c) if c.is_ascii_alphabetic());
        let is_uppercase_char = match self.code {
            KeyCode::Char(c) => c.is_ascii_uppercase(),
            _ => false,
        };
        if self.modifiers.contains(KeyModifiers::SHIFT)
            && !is_uppercase_char
            && !shifted_letter
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
            KeyCode::Char(c) if shifted_letter => c.to_ascii_uppercase().to_string(),
            KeyCode::Char(c) => c.to_string(),
            _ => "?".to_string(),
        };

        parts.push(&key_str);
        write!(f, "{}", parts.join("-"))
    }
}

/// A key bound to a named command of `[commands]` (MAX-239): its name, what it runs and its
/// description
#[derive(Debug, Clone)]
pub struct BoundCommand {
    pub name: String,
    pub invocations: Vec<Invocation>,
    pub help: String,
}

impl std::fmt::Display for BoundCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

pub struct KeyResolver {
    pub prefix: KeyPattern,
    pub prefix_actions: HashMap<KeyPattern, KeyBindingAction>,
    pub direct_actions: HashMap<KeyPattern, KeyBindingAction>,
    pub prefix_commands: HashMap<KeyPattern, BoundCommand>,
    pub direct_commands: HashMap<KeyPattern, BoundCommand>,
    pub line_actions: HashMap<KeyPattern, KeyBindingLineAction>,
    pub line_commands: HashMap<KeyPattern, BoundCommand>,
    pub terminal_actions: HashMap<KeyPattern, KeyBindingTerminalAction>,
    pub terminal_commands: HashMap<KeyPattern, BoundCommand>,
    pub search_actions: HashMap<KeyPattern, KeyBindingSearchAction>,
    pub copy_actions: HashMap<KeyPattern, KeyBindingCopyAction>,
    pub copy_commands: HashMap<KeyPattern, BoundCommand>,
    pub dialog_actions: HashMap<KeyPattern, KeyBindingDialogAction>,
    /// `[commands]`, by name (MAX-239)
    pub commands: Vec<NamedCommand>,
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
    /// A command line on a key: it goes in `[commands]` under a name (MAX-239)
    LineOnKey { section: &'static str, key: String },
    /// A named command of `[commands]` that can't be (MAX-239)
    BadNamedCommand { name: String, reason: String },
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
            KeymapError::LineOnKey { section, key } => write!(
                f,
                "in [{section}]: {key}: a command line goes in [commands] under a name, then bind the name"
            ),
            KeymapError::BadNamedCommand { name, reason } => {
                write!(f, "in [commands]: {name}: {reason}")
            }
        }
    }
}

impl std::error::Error for KeymapError {}

/// A section's keys to their action, and to their named command
type SectionWithCommands<A> = (HashMap<KeyPattern, A>, HashMap<KeyPattern, BoundCommand>);

/// A section whose keys take an action of the section, or else a named command of
/// `[commands]` (MAX-239): the global ones, `[line_mode]`, `[terminal_mode]` and `[copy_mode]`.
/// A value neither is an error naming its key, a command line one saying where it goes
fn section_with_commands<A: std::str::FromStr>(
    section: &'static str,
    entries: &BTreeMap<String, String>,
    named: &[NamedCommand],
) -> Result<SectionWithCommands<A>, KeymapError> {
    let mut actions = HashMap::new();
    let mut commands = HashMap::new();
    for (key, value) in entries {
        let pattern = key.parse().map_err(|reason| KeymapError::InvalidKey {
            section,
            key: key.clone(),
            reason,
        })?;
        let value = value.trim();
        if let Ok(action) = value.parse() {
            actions.insert(pattern, action);
        } else if let Some(command) = named.iter().find(|command| command.name == value) {
            let bound = BoundCommand {
                name: command.name.clone(),
                invocations: command.invocations.clone(),
                help: command.description().to_string(),
            };
            commands.insert(pattern, bound);
        } else if value.contains(|c: char| c.is_whitespace() || c == ';') {
            let key = key.clone();
            return Err(KeymapError::LineOnKey { section, key });
        } else {
            let action = value.to_string();
            return Err(KeymapError::InvalidAction { section, action });
        }
    }
    Ok((actions, commands))
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

    /// The keys bound to a copy mode action, for the search bar kept in copy mode (MAX-215)
    pub fn copy_keys(&self, action: KeyBindingCopyAction) -> Vec<KeyPattern> {
        keys_of(&self.copy_actions, action)
    }

    /// The keys bound in `[line_mode]` or `[terminal_mode]` that never get there, because the
    /// prefix or `[direct]` take them first (MAX-155), for a warning
    pub fn shadowed_keys(&self) -> Vec<String> {
        let line_keys = (self.line_actions.keys()).chain(self.line_commands.keys());
        let terminal_keys = (self.terminal_actions.keys()).chain(self.terminal_commands.keys());
        let mode_keys = (line_keys.map(|key| ("line_mode", key)))
            .chain(terminal_keys.map(|key| ("terminal_mode", key)));
        let direct_keys = (self.direct_actions.keys())
            .chain(self.direct_commands.keys())
            .map(|key| ("direct", key));
        let mut shadowed: Vec<String> = mode_keys
            .chain(direct_keys)
            .filter_map(|(section, key)| {
                let is_direct =
                    self.direct_actions.contains_key(key) || self.direct_commands.contains_key(key);
                let by = if *key == self.prefix {
                    "the prefix"
                } else if section != "direct" && is_direct {
                    "[direct]"
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

        // the default named commands stay under the file's, which win by name: the default
        // keys run them, whatever `[commands]` the file has
        let mut commands = KeyBindingsConfig::default().commands;
        commands.extend(config.commands.clone());
        let commands = named_commands(&commands)?;
        let (prefix_actions, prefix_commands) =
            section_with_commands("prefixed", &config.prefix_actions, &commands)?;
        let (direct_actions, direct_commands) =
            section_with_commands("direct", &config.direct_actions, &commands)?;
        let (line_actions, line_commands) =
            section_with_commands("line_mode", &config.line_mode, &commands)?;
        let (terminal_actions, terminal_commands) =
            section_with_commands("terminal_mode", &config.terminal_mode, &commands)?;

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

        let (copy_actions, copy_commands) =
            section_with_commands("copy_mode", &config.copy_mode, &commands)?;

        let mut dialog_actions = HashMap::new();
        for (k, action_str) in &config.dialog_actions {
            let pat = k.parse().map_err(|err| KeymapError::InvalidKey {
                section: "dialog",
                key: k.clone(),
                reason: err,
            })?;
            let act: KeyBindingDialogAction =
                action_str.parse().map_err(|_| KeymapError::InvalidAction {
                    section: "dialog",
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
            line_commands,
            terminal_actions,
            terminal_commands,
            search_actions,
            copy_actions,
            copy_commands,
            dialog_actions,
            commands,
        })
    }
}

/// An action of a mode section is named `name`: a named command named so would be ambiguous
/// there (MAX-239)
fn is_mode_action(name: &str) -> bool {
    name.parse::<KeyBindingLineAction>().is_ok()
        || name.parse::<KeyBindingTerminalAction>().is_ok()
        || name.parse::<KeyBindingSearchAction>().is_ok()
        || name.parse::<KeyBindingCopyAction>().is_ok()
        || name.parse::<KeyBindingDialogAction>().is_ok()
}

/// `[commands]` checked (MAX-239): a name is one word that no built-in command nor any action
/// of a section has, its line parses, and it runs built-in commands only: a named command
/// running another one would need a check for loops
fn named_commands(
    config: &BTreeMap<String, NamedCommandConfig>,
) -> Result<Vec<NamedCommand>, KeymapError> {
    let mut named = Vec::new();
    for (name, entry) in config {
        let refuse = |reason: String| KeymapError::BadNamedCommand {
            name: name.clone(),
            reason,
        };
        let word = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
        if name.is_empty() || !name.chars().all(word) {
            return Err(refuse("a name takes letters, digits, - and _".to_string()));
        }
        if commands::find_by_name(name).is_some() {
            return Err(refuse("a built-in command has this name".to_string()));
        }
        if is_mode_action(name) {
            return Err(refuse("an action has this name".to_string()));
        }
        let invocations = commands::parse(&entry.run).map_err(|error| match error {
            CommandError::UnknownCommand(other) if config.contains_key(&other) => {
                refuse(format!("runs '{other}': a named command can't run another"))
            }
            error => refuse(error.to_string()),
        })?;
        if invocations.is_empty() {
            return Err(refuse("nothing to run".to_string()));
        }
        named.push(NamedCommand {
            name: name.clone(),
            run: entry.run.trim().to_string(),
            help: entry.help.clone(),
            invocations,
        });
    }
    Ok(named)
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

    /// The sections are named after the help's tabs (MAX-239): `prefix` is the key itself
    #[serde(rename = "direct")]
    pub direct_actions: BTreeMap<String, String>,
    #[serde(rename = "prefixed")]
    pub prefix_actions: BTreeMap<String, String>,
    pub line_mode: BTreeMap<String, String>,
    pub terminal_mode: BTreeMap<String, String>,
    pub search_mode: BTreeMap<String, String>,
    pub copy_mode: BTreeMap<String, String>,
    #[serde(rename = "dialog")]
    pub dialog_actions: BTreeMap<String, String>,
    pub commands: BTreeMap<String, NamedCommandConfig>,
}

/// A command line of `[commands]` under its name, its help shown with it (MAX-239)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedCommandConfig {
    pub run: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub help: String,
}

impl KeyBindingsConfig {
    /// Adds the default bindings a section lacks, the ones whose key and action it doesn't use
    /// at all, as `--write-config` does: a section replaces its default, so a new default key
    /// never reaches an existing file otherwise (MAX-209). A key removed on purpose comes back,
    /// which the returned list says, as `section: key = action`
    pub fn add_new_defaults(&mut self) -> Vec<String> {
        let defaults = Self::default();
        let mut added = Vec::new();
        for (name, section, default) in [
            ("direct", &mut self.direct_actions, &defaults.direct_actions),
            (
                "prefixed",
                &mut self.prefix_actions,
                &defaults.prefix_actions,
            ),
            ("line_mode", &mut self.line_mode, &defaults.line_mode),
            (
                "terminal_mode",
                &mut self.terminal_mode,
                &defaults.terminal_mode,
            ),
            ("search_mode", &mut self.search_mode, &defaults.search_mode),
            ("copy_mode", &mut self.copy_mode, &defaults.copy_mode),
            ("dialog", &mut self.dialog_actions, &defaults.dialog_actions),
        ] {
            let keys: Vec<KeyPattern> = (section.keys())
                .filter_map(|key| key.parse().ok())
                .collect();
            let actions: Vec<String> = section.values().cloned().collect();
            for (key, action) in default {
                let key_used = key.parse().is_ok_and(|key| keys.contains(&key));
                if !key_used && !actions.contains(action) {
                    section.insert(key.clone(), action.clone());
                    added.push(format!("{name}: {key} = {action}"));
                }
            }
        }
        for (name, command) in defaults.commands {
            if let btree_map::Entry::Vacant(entry) = self.commands.entry(name) {
                added.push(format!("commands: {} = {}", entry.key(), command.run));
                entry.insert(command);
            }
        }
        added
    }
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
            "O"     => KeyBindingAction::SKTextEncoding,
            "e"     => KeyBindingAction::SKLocalEcho,
            "w"     => KeyBindingAction::SKLineWrap,
            "t"     => KeyBindingAction::SKDisplayTimestamp,
            "B"     => KeyBindingAction::SKDisplayRaw,
            "M"     => KeyBindingAction::ToggleMouse,
            "j"     => KeyBindingAction::Sessions,
            "c"     => KeyBindingAction::Clear,
            "C"     => KeyBindingAction::ClearHistory,
            "z"     => KeyBindingAction::Help,
            "?"     => KeyBindingAction::EnterSearchBackward,
            "o"     => KeyBindingAction::Settings,
            "s"     => KeyBindingAction::Settings,
            "l"     => KeyBindingAction::Record,
            "r"     => KeyBindingAction::ReloadConfig,
            "W"     => KeyBindingAction::ResizeDialog,
            "E"     => KeyBindingAction::AutoResizeEmulator,
            "p"     => KeyBindingAction::LinkSettings,
            "d"     => KeyBindingAction::Directory,
            "k"     => KeyBindingAction::ScreenshotBytes,
            "K"     => KeyBindingAction::ScreenshotTxt,
            "n"     => KeyBindingAction::DismissNotification,
            "N"     => KeyBindingAction::DismissNotifications,
            "up"    => KeyBindingAction::ScrollUp,
            "down"  => KeyBindingAction::ScrollDown,
            "left"  => KeyBindingAction::PanLeft,
            "right" => KeyBindingAction::PanRight,
            "0"     => KeyBindingAction::PanHome,
            "$"     => KeyBindingAction::PanEnd,
            "shift-left"  => KeyBindingAction::PanSideLeft,
            "shift-right" => KeyBindingAction::PanSideRight,
            "pageup"      => KeyBindingAction::PageUp,
            "pagedown"    => KeyBindingAction::PageDown,
            "["     => KeyBindingAction::EnterCopyMode,
            "b"     => KeyBindingAction::EnterCopyMode,
            "/"     => KeyBindingAction::EnterSearch,
            ":"     => KeyBindingAction::CommandLine,
            "u"     => KeyBindingAction::SKAddCr,
            "a"     => KeyBindingAction::SKAddLf,
            "D"     => KeyBindingAction::ToggleAttach,
            "T"     => KeyBindingAction::Triggers,
            ">"     => KeyBindingAction::SideGrow5,
            "<"     => KeyBindingAction::SideShrink5,
            "("     => KeyBindingAction::PrevMark,
            ")"     => KeyBindingAction::NextMark,
            "'"     => KeyBindingAction::Marks,
            ","     => "mark", // waits for the letter
            "f"     => "break", // a break like minicom's
            "h"     => "board-reset",
            "U"     => "u-boot-toggle",
            "L"     => "replay-stop", // back to the live link, as the session browser's L
            "g"     => "plot", // the next plot of plots.toml, off after the last
            "G"     => KeyBindingAction::PlotFreeze,
        };

        let line_mode = default_bindings! {
            "enter"  => KeyBindingLineAction::Submit,
            "up"     => KeyBindingLineAction::HistoryPrev,
            "down"   => KeyBindingLineAction::HistoryNext,
            "ctrl-p" => KeyBindingLineAction::HistoryPrev,
            "ctrl-n" => KeyBindingLineAction::HistoryNext,
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
            "up"         => KeyBindingSearchAction::HistoryPrev,
            "down"       => KeyBindingSearchAction::HistoryNext,
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
            "("         => KeyBindingCopyAction::PrevMark,
            ")"         => KeyBindingCopyAction::NextMark,
            "'"         => KeyBindingCopyAction::Marks,
            ","         => KeyBindingCopyAction::Mark,
            "/"         => KeyBindingCopyAction::Search,
            "?"         => KeyBindingCopyAction::SearchBackward,
            "n"         => KeyBindingCopyAction::SearchNext,
            "N"         => KeyBindingCopyAction::SearchPrev,
            "*"         => KeyBindingCopyAction::SearchWord,
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

        let board_reset = NamedCommandConfig {
            run: "dtr off; sleep 100; dtr on".to_string(),
            help: "Pulse DTR, which resets most boards".to_string(),
        };
        // the default triggers.toml's
        let u_boot = NamedCommandConfig {
            run: "trigger toggle u-boot".to_string(),
            help: "Arm / disarm the u-boot trigger".to_string(),
        };
        let send = |hex: &str, help: &str| NamedCommandConfig {
            run: format!("send-hex {hex}"),
            help: help.to_string(),
        };
        let commands = BTreeMap::from([
            ("board-reset".to_string(), board_reset),
            ("u-boot-toggle".to_string(), u_boot),
            (
                "ctrl-c".to_string(),
                send("03", "Send Ctrl-c, which stops the program running"),
            ),
            (
                "ctrl-d".to_string(),
                send(
                    "04",
                    "Send Ctrl-d: the end of the input, or a shell's logout",
                ),
            ),
        ]);

        Self {
            prefix: "ctrl-a".to_string(),
            direct_actions,
            prefix_actions,
            line_mode,
            terminal_mode,
            search_mode,
            copy_mode,
            dialog_actions,
            commands,
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

        fs::write(path, with_comments(&toml_string, KEYMAP_COMMENTS)).map_err(|err| {
            KeymapFileError::Io {
                path: path.to_path_buf(),
                source: err,
            }
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
#[path = "../tests/configs/keybindings.rs"]
mod tests;
