/// Keyboard event polling and translation
/// Using the 'crossterm' crate
use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::config::InputMode;

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    // --- Line Mode Actions ---
    /// Printable character to add to the active input line
    InsertChar(char),
    /// Backspace / delete character
    DeleteBack,
    /// Enter pressed: send the buffer
    Submit,
    /// Ctrl-C or Ctrl-Q requested exit
    Quit,
    /// Unhandled key (arrows, F-keys, mouse, etc.)
    Ignore,
    /// History Up
    HistoryUp,
    /// History Down
    HistoryDown,

    ScrollUp(usize),
    ScrollDown(usize),
    ScrollToBottom,

    // --- Raw Mode Actions ---
    TransmitRaw(Vec<u8>),
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
    let Event::Key(KeyEvent {
        code,
        modifiers,
        kind,
        ..
    }) = event
    else {
        return Action::Ignore;
    };

    // ignores release events if emitted
    if kind == KeyEventKind::Release {
        return Action::Ignore;
    }

    // check for exit shortcuts (Ctrl-C or Ctrl-Q)
    if modifiers.contains(KeyModifiers::CONTROL) {
        match code {
            KeyCode::Char('c') | KeyCode::Char('q') => return Action::Quit,
            _ => return Action::Ignore,
        }
    }

    match mode {
        InputMode::Line => handle_line_mode(code, modifiers),
        InputMode::Raw => handle_raw_mode(code, modifiers),
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

fn handle_raw_mode(code: KeyCode, modifiers: KeyModifiers) -> Action {
    // In Raw Mode, encode keys as direct ASCII / ANSI escape sequences
    match code {
        KeyCode::Char(c) => {
            if modifiers.contains(KeyModifiers::CONTROL) {
                // Ctrl+A = 0x01, Ctrl+B = 0x02, ..., Ctrl+Z = 0x1A
                let byte = (c.to_ascii_lowercase() as u8).saturating_sub(b'a') + 1;
                Action::TransmitRaw(vec![byte])
            } else {
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                Action::TransmitRaw(s.as_bytes().to_vec())
            }
        }
        KeyCode::Enter => Action::TransmitRaw(vec![b'\r']), // or b"\r\n"
        KeyCode::Backspace => Action::TransmitRaw(vec![0x08]), // BS (or 0x7F DEL)
        KeyCode::Tab => Action::TransmitRaw(vec![b'\t']),
        KeyCode::Esc => Action::TransmitRaw(vec![0x1B]),
        // Standard ANSI arrow sequences for remote shell navigation
        KeyCode::Up => Action::TransmitRaw(b"\x1b[A".to_vec()),
        KeyCode::Down => Action::TransmitRaw(b"\x1b[B".to_vec()),
        KeyCode::Right => Action::TransmitRaw(b"\x1b[C".to_vec()),
        KeyCode::Left => Action::TransmitRaw(b"\x1b[D".to_vec()),
        _ => Action::Ignore,
    }
}
