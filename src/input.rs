/// Keyboard event polling and translation
/// Using the 'crossterm' crate
use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use terminput::Encoding;
use terminput_crossterm::to_terminput_key;

use crate::{
    app::App,
    config::{InputMode, SettingKey},
    types::{Overlay, ResizeDialog},
};

#[derive(Debug)]
pub enum Action {
    /// Raw bytes for Raw Mode or special cases NOTE: maybe Serial category for later if needed
    TransmitRaw(Vec<u8>),
    /// Text input operations only for Line Mode
    LineInput(LineInputAction),
    /// Terminal operations for Terminal Mode
    Terminal(TerminalAction),
    /// Receive history navigation like scrolling and searching
    History(HistoryAction),
    /// Popup overlay action like open, navigate, close, toggle settings
    Overlay(OverlayAction),
    /// Global application commands
    App(AppAction),
    /// Nothing happening
    Ignore,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AppAction {
    SetPrefix(bool),
    FocusGained(bool),
    Quit,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LineInputAction {
    InsertChar(char),
    DeleteBack,
    Submit,
    HistoryUp,
    HistoryDown,
}

#[derive(Debug)]
pub enum TerminalAction {
    AutoResizeRemoteToViewer,
    AutoResizeEmulatorToViewer,
}

#[derive(Debug, PartialEq, Eq)]
pub enum HistoryAction {
    ScrollUp(usize),
    ScrollDown(usize),
    ScrollToBottom,
    ScrollToTop,
    PanLeft(usize),
    PanRight(usize),
    Clear,
} // TODO: add search, navigate seach up and down, next and previous, copy also?

#[derive(Debug)]
pub enum OverlayAction {
    Open(Overlay),
    Close,
    SelectNext,
    SelectPrev,
    Confirm,
    ForwardKey(KeyEvent),
    ToggleSetting(SettingKey),
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

    // Popup check
    if let Some(current_overlay) = app.overlay() {
        // NOTE: cannot generalize ESC because of disconnect and errors (todo) popups
        //
        //        if key_event.code == KeyCode::Esc && current_overlay != Overlay::DeviceDisconnected(_) {
        //           return Action::Overlay(OverlayAction::Close);
        //      }

        return match current_overlay {
            Overlay::Help => Action::Overlay(OverlayAction::Close),
            Overlay::Settings => match key_event.code {
                KeyCode::Esc => Action::Overlay(OverlayAction::Close),
                KeyCode::Char('e') | KeyCode::Char('E') => {
                    Action::Overlay(OverlayAction::ToggleSetting(SettingKey::LocalEcho))
                }
                KeyCode::Char('w') | KeyCode::Char('W') => {
                    Action::Overlay(OverlayAction::ToggleSetting(SettingKey::LineWrap))
                }
                KeyCode::Char('m') | KeyCode::Char('M') => {
                    Action::Overlay(OverlayAction::ToggleSetting(SettingKey::InputMode))
                }
                _ => Action::Ignore,
            },
            Overlay::TerminalSize(_) => match key_event.code {
                KeyCode::Esc => Action::Overlay(OverlayAction::Close),
                KeyCode::Tab => Action::Overlay(OverlayAction::SelectNext),
                KeyCode::Enter => Action::Overlay(OverlayAction::Confirm),
                _ => Action::Overlay(OverlayAction::ForwardKey(key_event)),
            },
            Overlay::DeviceDisconnected(_) => {
                if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                    match key_event.code {
                        KeyCode::Char('c') | KeyCode::Char('C') => {
                            return Action::App(AppAction::Quit);
                        }
                        _ => (),
                    }
                }

                match key_event.code {
                    KeyCode::Esc => Action::App(AppAction::Quit),
                    KeyCode::Enter => Action::App(AppAction::Quit),
                    _ => Action::Ignore,
                }
            }
        };
    }

    if let Some(action) = handle_common_keys(&key_event, app) {
        return action;
    }

    match app.config().input_mode {
        InputMode::Line => handle_line_mode(key_event),
        InputMode::Terminal => handle_raw_mode(key_event),
    }
}

// Common check
fn handle_common_keys(key: &KeyEvent, app: &App) -> Option<Action> {
    // PREFIX SECTION
    if app.is_prefix_active() {
        // SUB NO MODIFIER SECTION
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => return Some(Action::App(AppAction::Quit)),
            KeyCode::Char('m') | KeyCode::Char('M') => {
                // NOTE: this is from the overlay category but could be using another alias route?
                return Some(Action::Overlay(OverlayAction::ToggleSetting(
                    SettingKey::InputMode,
                )));
            }
            KeyCode::Char('a') => return Some(Action::TransmitRaw(vec![0x01])),
            KeyCode::Char('h') | KeyCode::Char('H') => {
                return Some(Action::Overlay(OverlayAction::Open(Overlay::Help)));
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                return Some(Action::Overlay(OverlayAction::Open(Overlay::Settings)));
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                let (rows, cols) = app.terminal_mode.get_vt100_size();
                let dialog = ResizeDialog::new(rows, cols);
                return Some(Action::Overlay(OverlayAction::Open(Overlay::TerminalSize(
                    dialog,
                ))));
            }
            KeyCode::Char('t') | KeyCode::Char('T') => {
                return Some(Action::Terminal(TerminalAction::AutoResizeRemoteToViewer));
            }
            KeyCode::Char('v') | KeyCode::Char('V') => {
                return Some(Action::Terminal(TerminalAction::AutoResizeEmulatorToViewer));
            }

            KeyCode::Up => {
                return Some(Action::History(HistoryAction::ScrollUp(1)));
            }
            KeyCode::Down => {
                return Some(Action::History(HistoryAction::ScrollDown(1)));
            }
            KeyCode::Right => {
                return Some(Action::History(HistoryAction::PanRight(1)));
            }
            KeyCode::Left => {
                return Some(Action::History(HistoryAction::PanLeft(1)));
            }

            KeyCode::Esc => return Some(Action::App(AppAction::SetPrefix(false))),
            _ => return Some(Action::App(AppAction::SetPrefix(false))),
        }
    }

    // CTRL SECTION
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('a') => return Some(Action::App(AppAction::SetPrefix(true))),
            _ => return None,
        }
    }

    // ALT SECTION
    if key.modifiers.contains(KeyModifiers::ALT) {
        match key.code {
            KeyCode::Char('c') => return Some(Action::History(HistoryAction::Clear)),
            _ => return None,
        }
    }

    // SHIFT SECTION
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        match key.code {
            KeyCode::Home => return Some(Action::History(HistoryAction::ScrollToTop)),
            KeyCode::End => return Some(Action::History(HistoryAction::ScrollToBottom)),
            _ => return None,
        }
    }

    // NOT MODIFIER SECTION
    match key.code {
        // TODO: make view_height - 1 for PageUp/PageDown
        KeyCode::PageUp => Some(Action::History(HistoryAction::ScrollUp(10))),
        KeyCode::PageDown => Some(Action::History(HistoryAction::ScrollDown(10))),
        _ => None,
    }
}

// Final check in LINE MODE
fn handle_line_mode(key: KeyEvent) -> Action {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('c') | KeyCode::Char('q') => return Action::App(AppAction::Quit),
            _ => return Action::Ignore,
        }
    }

    match key.code {
        // Typing
        KeyCode::Char(c) => Action::LineInput(LineInputAction::InsertChar(c)),
        KeyCode::Backspace => Action::LineInput(LineInputAction::DeleteBack),
        KeyCode::Enter => Action::LineInput(LineInputAction::Submit),
        KeyCode::Up => Action::LineInput(LineInputAction::HistoryUp),
        KeyCode::Down => Action::LineInput(LineInputAction::HistoryDown),

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
