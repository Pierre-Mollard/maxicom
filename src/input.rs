/// Keyboard event polling and translation
/// Using the 'crossterm' crate
use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use terminput::Encoding;
use terminput_crossterm::to_terminput_key;

use crate::{
    app::App,
    config::InputMode,
    types::{Overlay, ResizeDialog, SerialConfigDialog, SettingKey},
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
    CopyMode(CopyModeAction),
    /// Open popup
    OpenOverlay(Overlay),
    /// Popup overlay action forwarding
    OverlayKey(KeyEvent),
    /// Global application commands
    App(AppAction),
    /// Nothing happening
    Ignore,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AppAction {
    SetPrefix(bool),
    FocusGained(bool),
    ToggleSetting(SettingKey),
    ToggleLogging,
    Quit,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LineInputAction {
    Submit,
    HistoryUp,
    HistoryDown,
    HandleKey(KeyEvent),
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
    ScrollUpPage,
    ScrollDownPage,
    ScrollToBottom,
    ScrollToTop,
    PanLeft(usize),
    PanRight(usize),
    Clear,
    ToggleSearch,
    SearchNext,
    SearchPrevious,
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
    ToggleSelection,
    Yank,
    Exit,
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
    if app.overlay().is_some() {
        return Action::OverlayKey(key_event);
    }

    if app.is_copy_mode_active() {
        return handle_copy_mode_keys(key_event);
    }

    if let Some(action) = handle_common_keys(&key_event, app) {
        return action;
    }

    match app.config().input_mode {
        InputMode::Line => handle_line_mode(key_event),
        InputMode::Terminal => handle_terminal_mode(key_event),
    }
}

// Common check
fn handle_common_keys(key: &KeyEvent, app: &App) -> Option<Action> {
    // TODO: cleanup and refactor a bit HistoryAction for search in both mode
    let is_search_active = match app.config().input_mode {
        InputMode::Line => app.line_mode.search().active,
        InputMode::Terminal => app.terminal_mode.search().active,
    };

    // PREFIX SECTION
    if app.is_prefix_active() {
        // SUB NO MODIFIER SECTION
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => return Some(Action::App(AppAction::Quit)),
            KeyCode::Char('m') | KeyCode::Char('M') => {
                return Some(Action::App(AppAction::ToggleSetting(SettingKey::InputMode)));
            }
            KeyCode::Char('a') => {
                // in search both mode, execute "Go to start of line"
                if is_search_active {
                    return Some(Action::History(HistoryAction::HandleKey(KeyEvent::new(
                        KeyCode::Home,
                        KeyModifiers::NONE,
                    ))));
                }
                // in line mode, execute "Go to start of line"
                if app.config().input_mode == InputMode::Line {
                    return Some(Action::LineInput(LineInputAction::HandleKey(
                        KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
                    )));
                } else {
                    return Some(Action::TransmitRaw(vec![0x01]));
                }
            }
            KeyCode::Char('h') | KeyCode::Char('H') => {
                return Some(Action::OpenOverlay(Overlay::Help));
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                return Some(Action::OpenOverlay(Overlay::Settings));
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                return Some(Action::App(AppAction::ToggleLogging));
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                let (rows, cols) = app.terminal_mode.get_vt100_size();
                let dialog = ResizeDialog::new(rows, cols);
                return Some(Action::OpenOverlay(Overlay::TerminalSize(dialog)));
            }
            KeyCode::Char('t') | KeyCode::Char('T') => {
                return Some(Action::Terminal(TerminalAction::AutoResizeRemoteToViewer));
            }
            KeyCode::Char('v') | KeyCode::Char('V') => {
                return Some(Action::Terminal(TerminalAction::AutoResizeEmulatorToViewer));
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let dialog = SerialConfigDialog::new(&app.serial_config);
                return Some(Action::OpenOverlay(Overlay::SerialSettings(dialog)));
            }
            KeyCode::Char('[') => {
                return Some(Action::CopyMode(CopyModeAction::Enter));
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
            KeyCode::Char('f') | KeyCode::Char('F') => {
                return Some(Action::History(HistoryAction::ToggleSearch));
            }
            KeyCode::Char('p') if is_search_active => {
                return Some(Action::History(HistoryAction::SearchPrevious));
            }
            KeyCode::Char('n') if is_search_active => {
                return Some(Action::History(HistoryAction::SearchNext));
            }
            _ if is_search_active => return Some(Action::History(HistoryAction::HandleKey(*key))),
            _ => return None,
        }
    }

    // ALT SECTION
    if key.modifiers.contains(KeyModifiers::ALT) {
        match key.code {
            KeyCode::Char('c') => return Some(Action::History(HistoryAction::Clear)),
            _ if is_search_active => return Some(Action::History(HistoryAction::HandleKey(*key))),
            _ => return None,
        }
    }

    // SHIFT SECTION
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        match key.code {
            KeyCode::Home => return Some(Action::History(HistoryAction::ScrollToTop)),
            KeyCode::End => return Some(Action::History(HistoryAction::ScrollToBottom)),
            _ if is_search_active => return Some(Action::History(HistoryAction::HandleKey(*key))),
            _ => return None,
        }
    }

    // NOT MODIFIER SECTION
    match key.code {
        KeyCode::PageUp => Some(Action::History(HistoryAction::ScrollUpPage)),
        KeyCode::PageDown => Some(Action::History(HistoryAction::ScrollDownPage)),
        KeyCode::Enter if is_search_active => Some(Action::History(HistoryAction::SearchNext)),
        KeyCode::Esc if is_search_active => Some(Action::History(HistoryAction::ToggleSearch)),
        _ if is_search_active => Some(Action::History(HistoryAction::HandleKey(*key))),
        _ => None,
    }
}

fn handle_copy_mode_keys(key: KeyEvent) -> Action {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Action::CopyMode(CopyModeAction::MoveUp),
        KeyCode::Down | KeyCode::Char('j') => Action::CopyMode(CopyModeAction::MoveDown),
        KeyCode::Left | KeyCode::Char('h') => Action::CopyMode(CopyModeAction::MoveLeft),
        KeyCode::Right | KeyCode::Char('l') => Action::CopyMode(CopyModeAction::MoveRight),
        KeyCode::PageUp => Action::CopyMode(CopyModeAction::PageUp),
        KeyCode::PageDown => Action::CopyMode(CopyModeAction::PageDown),
        KeyCode::Home | KeyCode::Char('0') => Action::CopyMode(CopyModeAction::StartOfLine),
        KeyCode::End | KeyCode::Char('$') => Action::CopyMode(CopyModeAction::EndOfLine),

        KeyCode::Char(' ') | KeyCode::Char('v') => {
            Action::CopyMode(CopyModeAction::ToggleSelection)
        }

        KeyCode::Enter | KeyCode::Char('y') => Action::CopyMode(CopyModeAction::Yank),

        KeyCode::Esc | KeyCode::Char('q') => Action::CopyMode(CopyModeAction::Exit),

        _ => Action::Ignore,
    }
}

// Final check in LINE MODE
fn handle_line_mode(key: KeyEvent) -> Action {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('c') | KeyCode::Char('q') => return Action::App(AppAction::Quit),
            _ => return Action::LineInput(LineInputAction::HandleKey(key)),
        }
    }

    match key.code {
        KeyCode::Enter => Action::LineInput(LineInputAction::Submit),
        KeyCode::Up => Action::LineInput(LineInputAction::HistoryUp),
        KeyCode::Down => Action::LineInput(LineInputAction::HistoryDown),
        _ => Action::LineInput(LineInputAction::HandleKey(key)),
    }
}

// Final check in RAW MODE
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
