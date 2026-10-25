use std::path::PathBuf;

/// Module to reduce inter modules exchange or something
use crossterm::event::KeyCode;

use crate::{
    configs::keybindings::KeyBindingDialogAction, inputs::DialogAction, serial::SerialConfig,
};

// Re-exports
pub use help::{HelpCategory, HelpDialog};
pub use misc::{ErrorDialog, FilePathDialog};
pub use reconfigure::{SerialConfigDialog, SerialSettingField};
pub use resize::{ResizeDialog, ResizeField};

pub mod help;
pub mod misc;
pub mod reconfigure;
pub mod resize;

#[derive(Debug, Clone)]
pub enum Overlay {
    Help(HelpDialog),
    Settings,
    TerminalSize(ResizeDialog),
    SerialSettings(SerialConfigDialog),
    FilePath(FilePathDialog),
    DeviceDisconnected(String),
    DeviceDetached(String),
    Error(ErrorDialog),
    ConfirmQuit,
}

/// A part of a popup the mouse picks (MAX-113)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupTarget {
    HelpTab(HelpCategory),
    Setting(SettingKey),
    SerialField(SerialSettingField),
    ResizeField(ResizeField),
    Hint(KeyBindingDialogAction),
}

pub enum OverlayResponse {
    Handled,
    Close,
    ToggleSetting(SettingKey),
    ResizeTerminal { rows: u16, cols: u16 },
    ReconfigureSerial(SerialConfig),
    ReconnectSerial,
    StartCapture(PathBuf),
    Quit,
}

impl Overlay {
    pub fn is_text_input(&self) -> bool {
        matches!(self, Overlay::FilePath(_) | Overlay::TerminalSize(_))
    }

    pub fn closes_on_click_outside(&self) -> bool {
        !matches!(
            self,
            Overlay::DeviceDisconnected(_) | Overlay::DeviceDetached(_)
        )
    }

    pub fn handle_action(&mut self, action: DialogAction) -> OverlayResponse {
        match self {
            Overlay::Help(dialog) => dialog.handle_action(action),
            Overlay::SerialSettings(dialog) => dialog.handle_action(action),
            Overlay::TerminalSize(dialog) => dialog.handle_action(action),
            Overlay::FilePath(dialog) => dialog.handle_action(action),
            Overlay::Error(_) => match action {
                DialogAction::Close | DialogAction::Confirm => OverlayResponse::Close,
                _ => OverlayResponse::Handled,
            },
            Overlay::ConfirmQuit => match action {
                DialogAction::Confirm => OverlayResponse::Quit,
                DialogAction::Close => OverlayResponse::Close,
                DialogAction::HandleKey(key) => match key.code {
                    KeyCode::Char('y' | 'Y') => OverlayResponse::Quit,
                    KeyCode::Char('n' | 'N') => OverlayResponse::Close,
                    _ => OverlayResponse::Handled,
                },
                _ => OverlayResponse::Handled,
            },
            Overlay::DeviceDisconnected(_) => match action {
                DialogAction::Close | DialogAction::Confirm => OverlayResponse::Quit,
                _ => OverlayResponse::Handled,
            },
            Overlay::DeviceDetached(_) => match action {
                DialogAction::Confirm => OverlayResponse::ReconnectSerial,
                DialogAction::Close => OverlayResponse::Quit,
                _ => OverlayResponse::Handled,
            },
            Overlay::Settings => match action {
                DialogAction::Close => OverlayResponse::Close,
                DialogAction::Pick(PopupTarget::Setting(key)) => {
                    OverlayResponse::ToggleSetting(key)
                }
                DialogAction::HandleKey(key) => match key.code {
                    KeyCode::Char('e' | 'E') => {
                        OverlayResponse::ToggleSetting(SettingKey::LocalEcho)
                    }
                    KeyCode::Char('w' | 'W') => {
                        OverlayResponse::ToggleSetting(SettingKey::LineWrap)
                    }
                    KeyCode::Char('m' | 'M') => OverlayResponse::ToggleSetting(SettingKey::Mode),
                    KeyCode::Char('i' | 'I') => {
                        OverlayResponse::ToggleSetting(SettingKey::IdentWrap)
                    }
                    KeyCode::Char('d' | 'D') => {
                        OverlayResponse::ToggleSetting(SettingKey::DisplayMode)
                    }
                    KeyCode::Char('b' | 'B') => {
                        OverlayResponse::ToggleSetting(SettingKey::DisplayRaw)
                    }
                    KeyCode::Char('t' | 'T') => {
                        OverlayResponse::ToggleSetting(SettingKey::DisplayTimestamp)
                    }
                    KeyCode::Char('x' | 'X') => {
                        OverlayResponse::ToggleSetting(SettingKey::TextEncoding)
                    }
                    KeyCode::Char('u' | 'U') => OverlayResponse::ToggleSetting(SettingKey::Mouse),
                    _ => OverlayResponse::Handled,
                },
                _ => OverlayResponse::Handled,
            },
        }
    }
}

/// All settings types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKey {
    LocalEcho,
    Mode,
    LineWrap,
    IdentWrap,
    DisplayTimestamp,
    TextEncoding,
    DisplayMode,
    DisplayRaw,
    Mouse,
}
