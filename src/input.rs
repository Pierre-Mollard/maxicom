/// Keyboard event polling and translation
/// Using the 'crossterm' crate
use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use terminput::Encoding;
use terminput_crossterm::to_terminput_key;

use crate::{
    app::{App, InputContext},
    keybindings::{KeyBindingAction, KeyBindingCopyAction, KeyBindingSearchAction, KeyPattern},
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
                let raw_byte = match resolver.prefix.code {
                    KeyCode::Char(c)
                        if resolver.prefix.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        (c as u8).to_ascii_lowercase() - b'a' + 1
                    }
                    KeyCode::Char(c) => c as u8,
                    _ => 0x01,
                };
                return Action::TransmitRaw(vec![raw_byte]);
            }
            if let Some(prefix_action_binding) = resolver.prefix_actions.get(&pattern) {
                prefix_action_binding.to_action(app)
            } else {
                Action::App(AppAction::SetPrefix(false))
            }
        }
        InputContext::CopyMode => {
            if let Some(copy_action_binding) = resolver.copy_actions.get(&pattern) {
                copy_action_binding.to_action()
            } else {
                Action::Ignore
            }
        }
        InputContext::Search => {
            if let Some(search_action_binding) = resolver.search_actions.get(&pattern) {
                search_action_binding.to_action()
            } else {
                Action::Search(SearchAction::HandleKey(key_event))
            }
        }
        InputContext::LineInput => {
            if let Some(action_binding) = resolver.direct_actions.get(&pattern) {
                action_binding.to_action(app)
            } else {
                handle_line_mode(key_event)
            }
        }
        InputContext::Terminal => {
            if let Some(action_binding) = resolver.direct_actions.get(&pattern) {
                action_binding.to_action(app)
            } else {
                handle_terminal_mode(key_event)
            }
        }
    }
}

impl KeyBindingAction {
    pub fn to_action(self, app: &App) -> Action {
        match self {
            KeyBindingAction::Quit => Action::App(AppAction::Quit),
            KeyBindingAction::ToggleInputMode => {
                Action::App(AppAction::ToggleSetting(SettingKey::InputMode))
            }
            KeyBindingAction::Help => Action::App(AppAction::OpenOverlay(Overlay::Help)),
            KeyBindingAction::Settings => Action::App(AppAction::OpenOverlay(Overlay::Settings)),
            KeyBindingAction::ToggleLogging => Action::App(AppAction::ToggleLogging),
            KeyBindingAction::ResizeDialog => {
                let (r, c) = app.terminal_mode.get_vt100_size();
                Action::App(AppAction::OpenOverlay(Overlay::TerminalSize(
                    ResizeDialog::new(r, c),
                )))
            }

            KeyBindingAction::AutoResizeRemote => {
                Action::Terminal(TerminalAction::AutoResizeRemoteToViewer)
            }
            KeyBindingAction::AutoResizeEmulator => {
                Action::Terminal(TerminalAction::AutoResizeEmulatorToViewer)
            }
            KeyBindingAction::SerialSettings => {
                let dialog = SerialConfigDialog::new(&app.serial_config);
                Action::App(AppAction::OpenOverlay(Overlay::SerialSettings(dialog)))
            }

            KeyBindingAction::ScrollUp => Action::History(HistoryAction::ScrollUp(1)),
            KeyBindingAction::ScrollDown => Action::History(HistoryAction::ScrollDown(1)),
            KeyBindingAction::ScrollToBottom => Action::History(HistoryAction::ScrollToBottom),
            KeyBindingAction::ScrollToTop => Action::History(HistoryAction::ScrollToTop),
            KeyBindingAction::PanLeft => Action::History(HistoryAction::PanLeft(1)),
            KeyBindingAction::PanRight => Action::History(HistoryAction::PanRight(1)),
            KeyBindingAction::PageUp => Action::History(HistoryAction::ScrollUpPage),
            KeyBindingAction::PageDown => Action::History(HistoryAction::ScrollDownPage),

            KeyBindingAction::EnterCopyMode => Action::CopyMode(CopyModeAction::Enter),
            KeyBindingAction::EnterSearch => Action::Search(SearchAction::Start),
            KeyBindingAction::ClearHistory => Action::History(HistoryAction::Clear),
            KeyBindingAction::ExitPrefix => Action::App(AppAction::SetPrefix(false)),
        }
    }
}

impl KeyBindingSearchAction {
    pub fn to_action(self) -> Action {
        match self {
            Self::Next => Action::Search(SearchAction::Next),
            Self::Previous => Action::Search(SearchAction::Previous),
            Self::Cancel => Action::Search(SearchAction::Cancel),
            Self::Confirm => Action::Search(SearchAction::Confirm),
        }
    }
}

impl KeyBindingCopyAction {
    pub fn to_action(self) -> Action {
        match self {
            Self::Up => Action::CopyMode(CopyModeAction::MoveUp),
            Self::Down => Action::CopyMode(CopyModeAction::MoveDown),
            Self::Left => Action::CopyMode(CopyModeAction::MoveLeft),
            Self::Right => Action::CopyMode(CopyModeAction::MoveRight),
            Self::PageUp => Action::CopyMode(CopyModeAction::PageUp),
            Self::PageDown => Action::CopyMode(CopyModeAction::PageDown),
            Self::StartOfLine => Action::CopyMode(CopyModeAction::StartOfLine),
            Self::EndOfLine => Action::CopyMode(CopyModeAction::EndOfLine),
            Self::Select => Action::CopyMode(CopyModeAction::Select),
            Self::Yank => Action::CopyMode(CopyModeAction::Yank),
            Self::Exit => Action::CopyMode(CopyModeAction::Exit),
        }
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
