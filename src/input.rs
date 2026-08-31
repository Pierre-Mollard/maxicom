/// Keyboard event polling and translation
/// Using the 'crossterm' crate
use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use terminput::Encoding;
use terminput_crossterm::to_terminput_key;

use crate::{
    app::{App, Overlay},
    config::InputMode,
};

// TODO: maybe find a way to separate actions by groups?
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    // --- Line Mode Actions ---
    InsertChar(char),
    DeleteBack,
    Submit,
    HistoryUp,
    HistoryDown,

    // --- Raw Mode Actions ---
    TransmitRaw(Vec<u8>),

    // System Commands (via prefix / bindings)
    ToggleInputMode,
    SetPrefixMode(bool),
    ScrollUp(usize),
    ScrollDown(usize),
    ScrollToBottom,
    ScrollToTop,
    ClearHistory,
    Quit,
    Ignore,

    // Overlay Triggers
    OpenOverlay(Overlay),
    CloseOverlay,

    // Overlay Menu Actions
    ToggleEcho,
    ToggleWrap,
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
    let Event::Key(key_event) = event else {
        return Action::Ignore;
    };

    // ignores release events if emitted (only processing press/repeat)
    if key_event.kind == KeyEventKind::Release {
        return Action::Ignore;
    }

    // Popup check
    if let Some(current_overlay) = app.overlay() {
        if key_event.code == KeyCode::Esc {
            return Action::CloseOverlay;
        }

        return match current_overlay {
            Overlay::Help => Action::CloseOverlay,
            Overlay::Settings => match key_event.code {
                KeyCode::Char('e') | KeyCode::Char('E') => Action::ToggleEcho,
                KeyCode::Char('w') | KeyCode::Char('W') => Action::ToggleWrap,
                KeyCode::Char('m') | KeyCode::Char('M') => Action::ToggleInputMode,
                _ => Action::Ignore,
            },
        };
    }

    if let Some(action) = handle_common_keys(&key_event, app) {
        return action;
    }

    match app.config().input_mode {
        InputMode::Line => handle_line_mode(key_event),
        InputMode::Raw => handle_raw_mode(key_event),
    }
}

// Common check
fn handle_common_keys(key: &KeyEvent, app: &App) -> Option<Action> {
    // PREFIX SECTION
    if app.is_prefix_active() {
        // SUB NO MODIFIER SECTION
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => return Some(Action::Quit),
            KeyCode::Char('m') | KeyCode::Char('M') => return Some(Action::ToggleInputMode),
            KeyCode::Char('a') => return Some(Action::TransmitRaw(vec![0x01])),
            KeyCode::Char('h') | KeyCode::Char('H') => {
                return Some(Action::OpenOverlay(Overlay::Help));
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                return Some(Action::OpenOverlay(Overlay::Settings));
            }

            KeyCode::Esc => return Some(Action::SetPrefixMode(false)),
            _ => return Some(Action::SetPrefixMode(false)),
        }
    }

    // CTRL SECTION
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('a') => return Some(Action::SetPrefixMode(true)),
            _ => return None,
        }
    }

    // ALT SECTION
    if key.modifiers.contains(KeyModifiers::ALT) {
        match key.code {
            KeyCode::Char('c') => return Some(Action::ClearHistory),
            _ => return None,
        }
    }

    // SHIFT SECTION
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        match key.code {
            KeyCode::Home => return Some(Action::ScrollToTop),
            KeyCode::End => return Some(Action::ScrollToBottom),
            _ => return None,
        }
    }

    // NOT MODIFIER SECTION
    match key.code {
        KeyCode::PageUp => Some(Action::ScrollUp(10)),
        KeyCode::PageDown => Some(Action::ScrollDown(10)),
        _ => None,
    }
}

// Final check in LINE MODE
fn handle_line_mode(key: KeyEvent) -> Action {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('c') | KeyCode::Char('q') => return Action::Quit,
            _ => return Action::Ignore,
        }
    }

    match key.code {
        // Typing
        KeyCode::Char(c) => Action::InsertChar(c),
        KeyCode::Backspace => Action::DeleteBack,
        KeyCode::Enter => Action::Submit,
        KeyCode::Up => Action::HistoryUp,
        KeyCode::Down => Action::HistoryDown,

        _ => Action::Ignore,
    }
}

// Final check in RAW MODE
fn handle_raw_mode(key_event_crossterm: KeyEvent) -> Action {
    // Convert Crossterm KeyEvent -> Terminput KeyEvent
    if let Ok(key_event_terminput) = to_terminput_key(key_event_crossterm) {
        let event_terminput = terminput::Event::Key(key_event_terminput);
        let mut buf = [0u8; 32];

        // TODO: handle other encoding
        if let Ok(len) = event_terminput.encode(&mut buf, Encoding::Xterm) {
            return Action::TransmitRaw(buf[..len].to_vec());
        }
    }

    Action::Ignore
}
