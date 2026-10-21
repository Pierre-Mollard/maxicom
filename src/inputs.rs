/// Keyboard event polling and translation
/// Using the 'crossterm' crate
use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use terminput::Encoding;
use terminput_crossterm::to_terminput_key;

use crate::{
    app::{App, InputContext},
    configs::{KeyPattern, Mode},
    overlays::{Overlay, SettingKey},
};

pub mod global_dispatchers;
pub mod local_dispatchers;
pub mod vi;

// Re-exports
pub use local_dispatchers::{dispatch_input_key, dispatch_input_key_vi};
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
    Ignore,
}

#[derive(Debug)]
pub enum AppAction {
    SetPrefix(bool),
    FocusGained(bool),
    ToggleSetting(SettingKey),
    ToggleLogging,
    DetachSerial,
    OpenOverlay(Overlay),
    SideView(SideViewAction),
    Quit,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LineInputAction {
    Submit,
    HistoryUp,
    HistoryDown,
    AutoResizeRemoteToWidth,
    ClearLine,
    HandleKey(KeyEvent),
}

#[derive(Debug)]
pub enum TerminalAction {
    AutoResizeRemoteToViewer,
    AutoResizeEmulatorToViewer,
    ScreenshotBytes,
    ScreenshotTxt,
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
}

#[derive(Debug, PartialEq, Eq)]
pub enum SideViewAction {
    PanLeft(usize),
    PanRight(usize),
}

#[derive(Debug, PartialEq, Eq)]
pub enum SearchModeAction {
    Start,
    Next,
    Previous,
    Cancel,
    Confirm,
    Kind,
    HandleKey(KeyEvent),
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
    SelectRange(usize, usize),
    Yank,
    YankRaw,
    HandleKey(KeyEvent),
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
        crossterm::event::Event::FocusGained => {
            return Action::App(AppAction::FocusGained(true));
        }
        crossterm::event::Event::FocusLost => {
            return Action::App(AppAction::FocusGained(false));
        }
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

    // Actions using or ignoring with prefix
    match ctx {
        InputContext::Overlay => {
            let is_text_input = app.overlay().map_or_else(|| false, |o| o.is_text_input());

            if is_text_input {
                let is_modal_command = matches!(
                    key_event.code,
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab
                ) || key_event.modifiers.contains(KeyModifiers::CONTROL)
                    || key_event.modifiers.contains(KeyModifiers::ALT);

                if is_modal_command
                    && let Some(dialog_action_binding) = resolver.dialog_actions.get(&pattern)
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
            } else {
                return Action::Search(SearchModeAction::HandleKey(key_event));
            }
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
            } else {
                Action::CopyMode(CopyModeAction::HandleKey(key_event))
            }
        }
        InputContext::LineInput => {
            if let Some(action_binding) = resolver.direct_actions.get(&pattern) {
                action_binding.to_action(app)
            } else if let Some(line_action_binding) = resolver.line_actions.get(&pattern) {
                line_action_binding.to_action()
            } else {
                Action::LineInput(LineInputAction::HandleKey(key_event))
            }
        }
        InputContext::Terminal => {
            if let Some(action_binding) = resolver.direct_actions.get(&pattern) {
                action_binding.to_action(app)
            } else if let Some(term_action_binding) = resolver.terminal_actions.get(&pattern) {
                term_action_binding.to_action()
            } else {
                handle_terminal_mode(key_event)
            }
        }
        InputContext::Overlay | InputContext::Search | InputContext::Prefix => unreachable!(),
    }
}

/// forward everything to the remote through serial
fn handle_terminal_mode(key_event_crossterm: KeyEvent) -> Action {
    // Convert Crossterm KeyEvent -> Terminput KeyEvent
    if let Ok(key_event_terminput) = to_terminput_key(key_event_crossterm) {
        let event_terminput = terminput::Event::Key(key_event_terminput);
        let mut buf = [0u8; 32];

        if let Ok(len) = event_terminput.encode(&mut buf, Encoding::Xterm) {
            return Action::TransmitRaw(buf[..len].to_vec());
        }
    }

    Action::Ignore
}
