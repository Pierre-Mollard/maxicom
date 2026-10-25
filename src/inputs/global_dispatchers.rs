/// Converts Keybinding config action to a real action within contexts (mostly no context needed)
use crate::{
    app::App,
    configs::{
        KeyBindingAction, KeyBindingCopyAction, KeyBindingLineAction, KeyBindingSearchAction,
        KeyBindingTerminalAction, keybindings::KeyBindingDialogAction,
    },
    inputs::{
        Action, AppAction, CopyModeAction, DialogAction, HistoryAction, LineInputAction,
        SearchModeAction, SideViewAction, TerminalAction,
    },
    overlays::{HelpDialog, Overlay, ResizeDialog, SerialConfigDialog, SettingKey},
};

impl KeyBindingAction {
    pub fn to_action(self, app: &App) -> Action {
        match self {
            KeyBindingAction::Quit => Action::App(AppAction::Quit),
            KeyBindingAction::SKInputMode => {
                Action::App(AppAction::ToggleSetting(SettingKey::Mode))
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
            KeyBindingAction::SKDisplayRaw => {
                Action::App(AppAction::ToggleSetting(SettingKey::DisplayRaw))
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
            KeyBindingAction::ScreenshotBytes => Action::Terminal(TerminalAction::ScreenshotBytes),
            KeyBindingAction::ScreenshotTxt => Action::Terminal(TerminalAction::ScreenshotTxt),
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
            KeyBindingAction::PanSideRight => {
                Action::App(AppAction::SideView(SideViewAction::PanRight(2)))
            }
            KeyBindingAction::PanSideLeft => {
                Action::App(AppAction::SideView(SideViewAction::PanLeft(2)))
            }
            KeyBindingAction::ClearHistory => Action::History(HistoryAction::Clear),
            KeyBindingAction::ExitPrefix => Action::App(AppAction::SetPrefix(false)),
            KeyBindingAction::DetachSerial => Action::App(AppAction::DetachSerial),
            KeyBindingAction::ReloadConfig => Action::App(AppAction::ReloadConfig),
            KeyBindingAction::ToggleMouse => {
                Action::App(AppAction::ToggleSetting(SettingKey::Mouse))
            }
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
            Self::Kind => Action::Search(SearchModeAction::Kind),
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
            Self::SelectLine => Action::CopyMode(CopyModeAction::SelectLine),
            Self::Yank => Action::CopyMode(CopyModeAction::Yank),
            Self::YankSideInspector => Action::CopyMode(CopyModeAction::YankRaw),
            Self::ToggleSide => Action::CopyMode(CopyModeAction::ToggleSide),
            Self::Exit => Action::CopyMode(CopyModeAction::Exit),
        }
    }
}

impl KeyBindingDialogAction {
    pub fn to_action(self) -> Action {
        match self {
            Self::Close => Action::Overlay(DialogAction::Close),
            Self::Confirm => Action::Overlay(DialogAction::Confirm),
            Self::NavUp => Action::Overlay(DialogAction::Up),
            Self::NavDown => Action::Overlay(DialogAction::Down),
            Self::NavLeft => Action::Overlay(DialogAction::Left),
            Self::NavRight => Action::Overlay(DialogAction::Right),
            Self::CycleNext => Action::Overlay(DialogAction::CycleNext),
            Self::CyclePrev => Action::Overlay(DialogAction::CyclePrev),
            Self::PageUp => Action::Overlay(DialogAction::PageUp),
            Self::PageDown => Action::Overlay(DialogAction::PageDown),
            Self::Home => Action::Overlay(DialogAction::Home),
            Self::End => Action::Overlay(DialogAction::End),
        }
    }
}
