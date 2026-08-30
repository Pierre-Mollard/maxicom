/// Keyboard event polling and translation
/// Using the 'crossterm' crate
use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use terminput::Encoding;
use terminput_crossterm::to_terminput_key;

use crate::config::InputMode;

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
    // TODO: ToggleInputMode,
    ScrollUp(usize),
    ScrollDown(usize),
    ScrollToBottom,
    // TODO: ScrollToTop,
    Quit,
    Ignore,
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
pub fn handle_event(event: Event, mode: InputMode) -> Action {
    let Event::Key(key_event) = event else {
        return Action::Ignore;
    };

    // ignores release events if emitted
    if key_event.kind == KeyEventKind::Release {
        return Action::Ignore;
    }

    // check for exit shortcuts (Ctrl-C or Ctrl-Q) TODO: handle all keybindings (special and both)
    if mode == InputMode::Line && key_event.modifiers.contains(KeyModifiers::CONTROL) {
        match key_event.code {
            KeyCode::Char('c') | KeyCode::Char('q') => return Action::Quit,
            _ => return Action::Ignore,
        }
    }

    match mode {
        InputMode::Line => handle_line_mode(key_event.code, key_event.modifiers),
        InputMode::Raw => handle_raw_mode(key_event),
    }
}

fn handle_line_mode(code: KeyCode, _modifiers: KeyModifiers) -> Action {
    match code {
        KeyCode::Char(c) => Action::InsertChar(c),
        KeyCode::Backspace => Action::DeleteBack,
        KeyCode::Enter => Action::Submit,
        KeyCode::Up => Action::HistoryUp,
        KeyCode::Down => Action::HistoryDown,

        // TODO: check if those should work in raw mode too
        KeyCode::PageUp => Action::ScrollUp(10),
        KeyCode::PageDown => Action::ScrollDown(10),
        KeyCode::End => Action::ScrollToBottom,

        _ => Action::Ignore,
    }
}

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
