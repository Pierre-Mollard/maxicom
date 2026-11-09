/// Keyboard event polling and translation
/// Using the 'crossterm' crate
use std::{io, path::PathBuf, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;

use crate::{
    app::{
        App, InputContext,
        mash::Mash,
        search::SearchDirection,
        selection::{CopySide, LogicalPosition, MouseUnit},
        transfer::FileTransfer,
    },
    capture::ScreenshotFormat,
    commands::Invocation,
    configs::{KeyBindingAction, KeyPattern, Mode, TextEncoding, links::SavedLink},
    inputs::mouse::handle_mouse,
    link::{
        Address, LinkConfig,
        replay::ReplayControl,
        serial::{LineControl, LineSettings},
    },
    overlays::{Overlay, PopupTarget, SettingKey},
};

pub mod global_dispatchers;
pub mod local_dispatchers;
pub mod mouse;
pub mod vi;

// Re-exports
pub use local_dispatchers::{
    click_input_at, dispatch_input_key, dispatch_input_key_vi, paste_into_input,
};
pub use vi::{
    ViMode, ViPending, ViState, find_next_word, find_prev_word, find_prev_word_end, find_word_end,
};

#[derive(Debug)]
pub enum Action {
    App(AppAction),
    TransmitRaw(Vec<u8>),
    LineInput(LineInputAction),
    Terminal(TerminalAction),
    History(HistoryAction),
    CopyMode(CopyModeAction),
    Search(SearchModeAction),
    Overlay(DialogAction),
    Redraw,
    Ignore,
}

#[derive(Debug)]
pub enum AppAction {
    SetPrefix(bool),
    FocusGained(bool),
    ToggleSetting(SettingKey),
    ToggleRecording,
    ToggleAttach,
    ReloadConfig,
    OpenOverlay(Overlay),
    SideView(SideViewAction),
    ResizeSidePanel(i16),
    SideHandle(SideHandleAction),
    Hover(u16, u16),
    Scrollbar(ScrollbarAction),
    ClickHold(ClickHoldAction),
    PasteBuffer,
    OpenSessions,
    StartTransfer(Box<FileTransfer>),
    CancelCommands,
    Reconfigure(LineSettings),
    LineControl(LineControl),
    PauseCommands(Duration),
    Open(Address),
    OpenLink(LinkConfig),
    SaveLink(SavedLink),
    DeleteLink(SavedLink),
    NameSession(String),
    Replay { path: PathBuf, delay: Duration },
    StopReplay,
    ControlReplay(ReplayControl),
    SetEncoding(TextEncoding),
    SaveScreenshot(PathBuf),
    QueueCommands(Vec<Invocation>),
    ToggleTriggers,
    ArmTriggers { on: bool, name: Option<String> },
    DismissToast(Option<u64>),
    DismissToasts,
    StartMash(Box<Mash>),
    Mark(MarkAction),
    OpenMarks,
    Quit,
}

/// The history's marks (MAX-164), in the view of the current mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkAction {
    /// Waits for the next key: a letter, Enter for the first free one when adding
    Await(PendingMark),
    /// The key waited for wasn't a letter
    Cancel,
    /// On the line `add_mark` picks; none = the first free letter
    Add(Option<char>),
    /// On that history line, the first free letter: the menu's "Add mark"
    AddAt(usize),
    Jump(char),
    Delete(char),
    /// The closest mark below (`true`) or above
    Adjacent(bool),
}

/// What a letter key does while a mark waits for it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingMark {
    Add,
    Jump,
}

/// The left button on the history's scrollbar (MAX-142): a press on one of its rows (`0` the
/// `▲`), a drag held from it on a row (above or below it too), the release
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollbarAction {
    Press(u16),
    Drag(i32),
    Release,
}

/// The left button on the raw side panel's border (MAX-189): the press, a drag held from it
/// to a cell, the release
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideHandleAction {
    Press,
    Drag(u16, u16),
    Release,
}

/// The left button held on a click target that repeats (MAX-143): the press on its cells runs
/// its action, a drag off them stops the repeat, the release
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickHoldAction {
    Press(KeyBindingAction, Rect),
    Drag(u16, u16),
    Release,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LineInputAction {
    Submit,
    HistoryUp,
    HistoryDown,
    AutoResizeRemoteToWidth,
    ClearLine,
    Interrupt,
    ClickAt(usize),
    HandleKey(KeyEvent),
    Paste(String),
}

#[derive(Debug)]
pub enum TerminalAction {
    AutoResizeRemoteToEmulator,
    AutoResizeEmulatorToViewer,
    Screenshot(ScreenshotFormat),
    Key(KeyEvent),
    Paste(String),
    MouseReport(Vec<u8>),
}

#[derive(Debug, PartialEq, Eq)]
pub enum HistoryAction {
    ScrollUp(usize),
    ScrollDown(usize),
    ScrollUpPage,
    ScrollDownPage,
    ScrollToBottom,
    ScrollToTop,
    PanLeft(usize),
    PanRight(usize),
    Clear,
    ClearView,
    ScrollTo(usize),
}

#[derive(Debug, PartialEq, Eq)]
pub enum SideViewAction {
    PanLeft(usize),
    PanRight(usize),
}

#[derive(Debug, PartialEq, Eq)]
pub enum SearchModeAction {
    Start(SearchDirection),
    Next,
    Previous,
    Cancel,
    Confirm,
    Kind,
    HandleKey(KeyEvent),
    Paste(String),
    ClickAt(usize),
    Query(String),
    Repeat(bool),
}

#[derive(Debug, PartialEq, Eq)]
pub enum CopyModeAction {
    Enter,
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    PageUp,
    PageDown,
    StartOfLine,
    EndOfLine,
    NextWord,
    NextBigWord,
    PrevWord,
    PrevBigWord,
    WordEnd,
    BigWordEnd,
    PrevWordEnd,
    PrevBigWordEnd,
    JumpToTop,
    JumpToBottom,
    Select,
    SelectLine,
    SelectRange(usize, usize),
    Yank,
    YankRaw,
    ToggleSide,
    MousePress(LogicalPosition, CopySide, MouseUnit),
    MouseDrag(LogicalPosition, (u16, u16)),
    MouseRelease,
    HandleKey(KeyEvent),
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogAction {
    Close,
    Confirm,
    Up,
    Down,
    Left,
    Right,
    CycleNext,
    CyclePrev,
    PageUp,
    PageDown,
    Home,
    End,
    HandleKey(KeyEvent),
    Paste(String),
    Pick(PopupTarget),
    ClickAt(usize),
}

/// Using crossterm's event API instead of stdin.read() to handle multi-byte ANSI sequence like
/// arrows and page up and prefix
pub fn poll_event(timeout: Duration) -> io::Result<Option<Event>> {
    if event::poll(timeout)? {
        Ok(Some(event::read()?))
    } else {
        Ok(None)
    }
}

/// Pure translation: takes a crossterm Event and turns it into a typed Action
pub fn handle_event(event: Event, app: &App) -> Action {
    // non key events
    match event {
        Event::FocusGained => {
            return Action::App(AppAction::FocusGained(true));
        }
        Event::FocusLost => {
            return Action::App(AppAction::FocusGained(false));
        }
        Event::Paste(text) => return handle_paste(text, app),
        Event::Mouse(mouse_event) => return handle_mouse(mouse_event, app),
        Event::Resize(..) => return Action::Redraw,
        _ => {}
    }

    // key event from here
    let Event::Key(key_event) = event else {
        return Action::Ignore;
    };

    // ignores release events if emitted (only processing press/repeat)
    if key_event.kind == KeyEventKind::Release {
        return Action::Ignore;
    }

    let pattern = KeyPattern::from_event(&key_event);
    let resolver = &app.key_resolver;
    let ctx = app.input_context();

    // the key after an add or jump mark key is its letter (MAX-164)
    if let Some(pending) = app.pending_mark()
        && ctx != InputContext::Overlay
    {
        return Action::App(AppAction::Mark(mark_key(pending, key_event)));
    }

    // Actions using or ignoring with prefix
    match ctx {
        InputContext::Overlay => {
            // a quit key again confirms the quit: Ctrl-Q twice quits fast (MAX-125)
            if matches!(app.overlay(), Some(Overlay::ConfirmQuit))
                && resolver.direct_actions.get(&pattern) == Some(&KeyBindingAction::Quit)
            {
                return Action::Overlay(DialogAction::Confirm);
            }
            let is_text_input = app.overlay().map_or_else(|| false, |o| o.is_text_input());

            // the marks popup jumps on a letter, `q` and `j` too (MAX-164)
            let plain_letter = matches!(key_event.code, KeyCode::Char(c) if c.is_ascii_lowercase())
                && !key_event
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
            if plain_letter && app.overlay().is_some_and(Overlay::takes_letters) {
                return Action::Overlay(DialogAction::HandleKey(key_event));
            }

            if is_text_input {
                // only close, confirm and cycle: the field keeps Ctrl-U, Ctrl-D, Home, End (MAX-155)
                if pattern.is_modal()
                    && let Some(dialog_action_binding) = resolver.dialog_actions.get(&pattern)
                    && dialog_action_binding.is_text_input_action()
                {
                    return dialog_action_binding.to_action();
                }
                // force typing text for input fields
                return Action::Overlay(DialogAction::HandleKey(key_event));
            }

            if let Some(dialog_action_binding) = resolver.dialog_actions.get(&pattern) {
                return dialog_action_binding.to_action();
            } else {
                return Action::Overlay(DialogAction::HandleKey(key_event));
            }
        }
        InputContext::Search => {
            if let Some(search_action_binding) = resolver.search_actions.get(&pattern) {
                return search_action_binding.to_action();
            }
            if pattern == resolver.prefix {
                return Action::App(AppAction::SetPrefix(true));
            }
            if let Some(action_binding) = resolver.direct_actions.get(&pattern)
                && action_binding.is_navigation()
            {
                return action_binding.to_action(app);
            }
            return Action::Search(SearchModeAction::HandleKey(key_event));
        }
        InputContext::Prefix if app.is_search_active() => {
            if pattern == resolver.prefix {
                return Action::Search(SearchModeAction::HandleKey(key_event));
            }
            return match resolver.prefix_actions.get(&pattern) {
                Some(action_binding) if action_binding.is_navigation() => {
                    action_binding.to_action(app)
                }
                _ => Action::App(AppAction::SetPrefix(false)),
            };
        }
        InputContext::Prefix => {
            // Prefix pressed twice (terminal send, line mode forward)
            if pattern == resolver.prefix {
                match app.settings().mode {
                    Mode::Line => {
                        return Action::LineInput(LineInputAction::HandleKey(key_event));
                    }
                    Mode::Terminal => {
                        let bytes = resolver.prefix.to_raw_bytes();
                        if !bytes.is_empty() {
                            return Action::TransmitRaw(bytes);
                        }
                        return Action::App(AppAction::SetPrefix(false));
                    }
                }
            }

            if let Some(prefix_action_binding) = resolver.prefix_actions.get(&pattern) {
                return prefix_action_binding.to_action(app);
            } else if let Some(line) = resolver.prefix_commands.get(&pattern) {
                return Action::App(AppAction::QueueCommands(line.invocations.clone()));
            } else {
                return Action::App(AppAction::SetPrefix(false));
            }
        }
        _ => {}
    }

    if pattern == resolver.prefix {
        return Action::App(AppAction::SetPrefix(true));
    }

    // Actions unreachable with prefix
    match ctx {
        InputContext::CopyMode => {
            if let Some(copy_action_binding) = resolver.copy_actions.get(&pattern) {
                copy_action_binding.to_action()
            } else if let Some(line) = resolver.copy_commands.get(&pattern) {
                Action::App(AppAction::QueueCommands(line.invocations.clone()))
            } else if let Some(action_binding) = resolver.direct_actions.get(&pattern)
                && action_binding.is_navigation()
            {
                // the direct scroll and pan keys move the view, the cursor stays (MAX-143)
                action_binding.to_action(app)
            } else {
                Action::CopyMode(CopyModeAction::HandleKey(key_event))
            }
        }
        InputContext::LineInput => {
            if let Some(action_binding) = resolver.direct_actions.get(&pattern) {
                action_binding.to_action(app)
            } else if let Some(line) = resolver.direct_commands.get(&pattern) {
                Action::App(AppAction::QueueCommands(line.invocations.clone()))
            } else if let Some(line_action_binding) = resolver.line_actions.get(&pattern) {
                line_action_binding.to_action()
            } else {
                Action::LineInput(LineInputAction::HandleKey(key_event))
            }
        }
        InputContext::Terminal => {
            if let Some(action_binding) = resolver.direct_actions.get(&pattern) {
                action_binding.to_action(app)
            } else if let Some(line) = resolver.direct_commands.get(&pattern) {
                Action::App(AppAction::QueueCommands(line.invocations.clone()))
            } else if let Some(term_action_binding) = resolver.terminal_actions.get(&pattern) {
                term_action_binding.to_action()
            } else {
                Action::Terminal(TerminalAction::Key(key_event))
            }
        }
        InputContext::Overlay | InputContext::Search | InputContext::Prefix => unreachable!(),
    }
}

/// The mark action for the key a mark waited for: a letter `a`..`z` (no Ctrl or Alt), Enter
/// for the first free letter when adding; anything else cancels
fn mark_key(pending: PendingMark, key_event: KeyEvent) -> MarkAction {
    let plain = !key_event
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    match (pending, key_event.code) {
        (PendingMark::Add, KeyCode::Char(letter)) if plain && letter.is_ascii_lowercase() => {
            MarkAction::Add(Some(letter))
        }
        (PendingMark::Jump, KeyCode::Char(letter)) if plain && letter.is_ascii_lowercase() => {
            MarkAction::Jump(letter)
        }
        (PendingMark::Add, KeyCode::Enter) => MarkAction::Add(None),
        _ => MarkAction::Cancel,
    }
}

fn handle_paste(text: String, app: &App) -> Action {
    if text.is_empty() {
        return Action::Ignore;
    }
    match app.input_context() {
        InputContext::Overlay => {
            if app.overlay().is_some_and(|o| o.is_text_input()) {
                Action::Overlay(DialogAction::Paste(text))
            } else {
                Action::Ignore
            }
        }
        InputContext::Search => Action::Search(SearchModeAction::Paste(text)),
        InputContext::Prefix => Action::App(AppAction::SetPrefix(false)),
        InputContext::CopyMode => Action::Ignore,
        InputContext::LineInput => Action::LineInput(LineInputAction::Paste(text)),
        InputContext::Terminal => Action::Terminal(TerminalAction::Paste(text)),
    }
}

#[cfg(test)]
#[path = "tests/inputs.rs"]
mod tests;
