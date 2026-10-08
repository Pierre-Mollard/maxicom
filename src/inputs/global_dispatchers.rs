/// Converts Keybinding config action to a real action within contexts (mostly no context needed)
use crate::{
    app::App,
    configs::{
        KeyBindingAction, KeyBindingCopyAction, KeyBindingLineAction, KeyBindingSearchAction,
        KeyBindingTerminalAction,
    },
    inputs::{
        Action, AppAction, CopyModeAction, HistoryAction, LineInputAction, SearchModeAction,
        TerminalAction,
    },
    overlays::{HelpDialog, Overlay, ResizeDialog, SerialConfigDialog, SettingKey},
};

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
            KeyBindingAction::SKIndentWrap => {
                Action::App(AppAction::ToggleSetting(SettingKey::IdentWrap))
            }
            KeyBindingAction::SKDisplayMode => {
                Action::App(AppAction::ToggleSetting(SettingKey::DisplayMode))
            }
            KeyBindingAction::Help => {
                Action::App(AppAction::OpenOverlay(Overlay::Help(HelpDialog::new())))
            }
            KeyBindingAction::Settings => Action::App(AppAction::OpenOverlay(Overlay::Settings)),
            KeyBindingAction::ToggleLogging => Action::App(AppAction::ToggleLogging),
            KeyBindingAction::ResizeDialog => {
                let (r, c) = app.terminal_mode.get_vt100_size();
                Action::App(AppAction::OpenOverlay(Overlay::TerminalSize(
                    ResizeDialog::new(r, c),
                )))
            }

            KeyBindingAction::AutoResizeRemoteLine => {
                Action::LineInput(LineInputAction::AutoResizeRemoteToWidth)
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
            Self::AutoResizeRemoteLine => {
                Action::LineInput(LineInputAction::AutoResizeRemoteToWidth)
            }
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
