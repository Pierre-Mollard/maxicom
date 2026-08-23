use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
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
pub fn handle_event(event: Event) -> Action {
    match event {
        Event::Key(KeyEvent {
            code,
            modifiers,
            kind,
            ..
        }) => {
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

            // normal typing
            match code {
                KeyCode::Char(c) => Action::InsertChar(c),
                KeyCode::Backspace => Action::DeleteBack,
                KeyCode::Enter => Action::Submit,
                _ => Action::Ignore,
            }
        }
        _ => Action::Ignore,
    }
}
