/// Keyboard event polling and translation
/// Using the 'crossterm' crate
use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use terminput::Encoding;
use terminput_crossterm::to_terminput_key;

use crate::{
    app::{App, InputContext},
    configs::{
        KeyBindingAction, KeyBindingCopyAction, KeyBindingLineAction, KeyBindingSearchAction,
        KeyBindingTerminalAction, KeyPattern,
    },
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
    Search(SearchModeAction),
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
pub enum SearchModeAction {
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
                let bytes = resolver.prefix.to_raw_bytes();
                if !bytes.is_empty() {
                    return Action::TransmitRaw(bytes);
                }
                return Action::App(AppAction::SetPrefix(false));
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
                Action::Search(SearchModeAction::HandleKey(key_event))
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
    }
}

impl KeyBindingAction {
    pub fn to_action(self, app: &App) -> Action {
        match self {
            KeyBindingAction::Quit => Action::App(AppAction::Quit),
            KeyBindingAction::SKInputMode => {
                Action::App(AppAction::ToggleSetting(SettingKey::InputMode))
            }
            KeyBindingAction::SKLocalEcho => {
                Action::App(AppAction::ToggleSetting(SettingKey::LocalEcho))
            }
            KeyBindingAction::SKLineWrap => {
                Action::App(AppAction::ToggleSetting(SettingKey::LineWrap))
            }
            KeyBindingAction::SKDisplayTimestamp => {
                Action::App(AppAction::ToggleSetting(SettingKey::DisplayTimestamp))
            }
            KeyBindingAction::SKTextEncoding => {
                Action::App(AppAction::ToggleSetting(SettingKey::TextEncoding))
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
            KeyBindingAction::EnterSearch => Action::Search(SearchModeAction::Start),
            KeyBindingAction::ClearHistory => Action::History(HistoryAction::Clear),
            KeyBindingAction::ExitPrefix => Action::App(AppAction::SetPrefix(false)),
        }
    }
}

impl KeyBindingLineAction {
    pub fn to_action(self) -> Action {
        match self {
            Self::Submit => Action::LineInput(LineInputAction::Submit),
            Self::HistoryPrev => Action::LineInput(LineInputAction::HistoryUp),
            Self::HistoryNext => Action::LineInput(LineInputAction::HistoryDown),
            Self::ClearLine => Action::LineInput(LineInputAction::ClearLine),
            Self::Quit => Action::App(AppAction::Quit),
        }
    }
}

impl KeyBindingTerminalAction {
    pub fn to_action(self) -> Action {
        match self {
            Self::AutoResizeRemote => Action::Terminal(TerminalAction::AutoResizeRemoteToViewer),
            Self::AutoResizeEmulator => {
                Action::Terminal(TerminalAction::AutoResizeEmulatorToViewer)
            }
            Self::Quit => Action::App(AppAction::Quit),
        }
    }
}

impl KeyBindingSearchAction {
    pub fn to_action(self) -> Action {
        match self {
            Self::Next => Action::Search(SearchModeAction::Next),
            Self::Previous => Action::Search(SearchModeAction::Previous),
            Self::Cancel => Action::Search(SearchModeAction::Cancel),
            Self::Confirm => Action::Search(SearchModeAction::Confirm),
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
