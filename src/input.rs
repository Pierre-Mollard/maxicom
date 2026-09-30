/// Keyboard event polling and translation
/// Using the 'crossterm' crate
use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use terminput::Encoding;
use terminput_crossterm::to_terminput_key;

use crate::{
    app::{App, InputContext},
    config::InputMode,
    keybindings::{KeyBindingPrefixAction, KeyPattern},
    types::{Overlay, ResizeDialog, SerialConfigDialog, SettingKey},
};

#[derive(Debug)]
pub enum Action {
    App(AppAction),
    TransmitRaw(Vec<u8>),
    LineInput(LineInputAction),
    Terminal(TerminalAction),
    History(HistoryAction),
    CopyMode(CopyModeAction),
    Search(SearchAction),
    OverlayKey(KeyEvent),
    Ignore,
}

#[derive(Debug)]
pub enum AppAction {
    SetPrefix(bool),
    FocusGained(bool),
    ToggleSetting(SettingKey),
    ToggleLogging,
    OpenOverlay(Overlay),
    Quit,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LineInputAction {
    Submit,
    HistoryUp,
    HistoryDown,
    ClearLine,
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
}

#[derive(Debug, PartialEq, Eq)]
pub enum SearchAction {
    Start,
    Next,
    Previous,
    Cancel,
    Confirm,
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
    Select,
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

    let pattern = KeyPattern::from_event(&key_event);
    let resolver = &app.key_resolver;

    if pattern == resolver.prefix && app.input_context() != InputContext::Prefix {
        return Action::App(AppAction::SetPrefix(true));
    }

    match app.input_context() {
        InputContext::Overlay => Action::OverlayKey(key_event),
        InputContext::Prefix => {
            if pattern == resolver.prefix {
                return Action::TransmitRaw(vec![0x01]);
            }
            if let Some(prefix_action_binding) = resolver.prefix_actions.get(&pattern) {
                prefix_action_binding.to_action(app)
            } else {
                Action::App(AppAction::SetPrefix(false))
            }
        }
        InputContext::CopyMode => handle_copy_mode_keys(key_event),
        InputContext::Search => {
            // TODO: make a search mode section
            if let Some(action) = handle_common_keys(&key_event, app) {
                action
            } else {
                Action::Ignore
            }
        }
        InputContext::LineInput => {
            // TODO: better handling of common and line/terminal modes
            if let Some(action) = handle_common_keys(&key_event, app) {
                return action;
            }
            handle_line_mode(key_event)
        }
        InputContext::Terminal => {
            if let Some(action) = handle_common_keys(&key_event, app) {
                return action;
            }
            handle_terminal_mode(key_event)
        }
    }
}

impl KeyBindingPrefixAction {
    pub fn to_action(self, app: &App) -> Action {
        match self {
            KeyBindingPrefixAction::Quit => Action::App(AppAction::Quit),
            KeyBindingPrefixAction::ToggleInputMode => {
                Action::App(AppAction::ToggleSetting(SettingKey::InputMode))
            }
            KeyBindingPrefixAction::Help => Action::App(AppAction::OpenOverlay(Overlay::Help)),
            KeyBindingPrefixAction::Settings => {
                Action::App(AppAction::OpenOverlay(Overlay::Settings))
            }
            KeyBindingPrefixAction::ToggleLogging => Action::App(AppAction::ToggleLogging),
            KeyBindingPrefixAction::ResizeDialog => {
                let (r, c) = app.terminal_mode.get_vt100_size();
                Action::App(AppAction::OpenOverlay(Overlay::TerminalSize(
                    ResizeDialog::new(r, c),
                )))
            }

            KeyBindingPrefixAction::AutoResizeRemote => {
                Action::Terminal(TerminalAction::AutoResizeRemoteToViewer)
            }
            KeyBindingPrefixAction::AutoResizeEmulator => {
                Action::Terminal(TerminalAction::AutoResizeEmulatorToViewer)
            }
            KeyBindingPrefixAction::SerialSettings => {
                let dialog = SerialConfigDialog::new(&app.serial_config);
                Action::App(AppAction::OpenOverlay(Overlay::SerialSettings(dialog)))
            }

            KeyBindingPrefixAction::ScrollUp => Action::History(HistoryAction::ScrollUp(1)),
            KeyBindingPrefixAction::ScrollDown => Action::History(HistoryAction::ScrollDown(1)),
            KeyBindingPrefixAction::PanLeft => Action::History(HistoryAction::PanLeft(1)),
            KeyBindingPrefixAction::PanRight => Action::History(HistoryAction::PanRight(1)),

            KeyBindingPrefixAction::EnterCopyMode => Action::CopyMode(CopyModeAction::Enter),
            KeyBindingPrefixAction::ExitPrefix => Action::App(AppAction::SetPrefix(false)),
        }
    }
}

// Common check
fn handle_common_keys(key: &KeyEvent, app: &App) -> Option<Action> {
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
                    return Some(Action::Search(SearchAction::HandleKey(KeyEvent::new(
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
                return Some(Action::App(AppAction::OpenOverlay(Overlay::Help)));
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                return Some(Action::App(AppAction::OpenOverlay(Overlay::Settings)));
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                return Some(Action::App(AppAction::ToggleLogging));
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                let (rows, cols) = app.terminal_mode.get_vt100_size();
                let dialog = ResizeDialog::new(rows, cols);
                return Some(Action::App(AppAction::OpenOverlay(Overlay::TerminalSize(
                    dialog,
                ))));
            }
            KeyCode::Char('t') | KeyCode::Char('T') => {
                return Some(Action::Terminal(TerminalAction::AutoResizeRemoteToViewer));
            }
            KeyCode::Char('v') | KeyCode::Char('V') => {
                return Some(Action::Terminal(TerminalAction::AutoResizeEmulatorToViewer));
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let dialog = SerialConfigDialog::new(&app.serial_config);
                return Some(Action::App(AppAction::OpenOverlay(
                    Overlay::SerialSettings(dialog),
                )));
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
                return Some(Action::Search(SearchAction::Start));
            }
            KeyCode::Char('p') if is_search_active => {
                return Some(Action::Search(SearchAction::Previous));
            }
            KeyCode::Char('n') if is_search_active => {
                return Some(Action::Search(SearchAction::Next));
            }
            _ if is_search_active => return Some(Action::Search(SearchAction::HandleKey(*key))),
            _ => return None,
        }
    }

    // ALT SECTION
    if key.modifiers.contains(KeyModifiers::ALT) {
        match key.code {
            KeyCode::Char('c') => return Some(Action::History(HistoryAction::Clear)),
            _ if is_search_active => return Some(Action::Search(SearchAction::HandleKey(*key))),
            _ => return None,
        }
    }

    // SHIFT SECTION
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        match key.code {
            KeyCode::Home => return Some(Action::History(HistoryAction::ScrollToTop)),
            KeyCode::End => return Some(Action::History(HistoryAction::ScrollToBottom)),
            _ if is_search_active => return Some(Action::Search(SearchAction::HandleKey(*key))),
            _ => return None,
        }
    }

    // NOT MODIFIER SECTION
    match key.code {
        KeyCode::PageUp => Some(Action::History(HistoryAction::ScrollUpPage)),
        KeyCode::PageDown => Some(Action::History(HistoryAction::ScrollDownPage)),
        KeyCode::Enter if is_search_active => Some(Action::Search(SearchAction::Next)),
        KeyCode::Esc if is_search_active => Some(Action::Search(SearchAction::Start)),
        _ if is_search_active => Some(Action::Search(SearchAction::HandleKey(*key))),
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

        KeyCode::Char(' ') | KeyCode::Char('v') => Action::CopyMode(CopyModeAction::Select),

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
