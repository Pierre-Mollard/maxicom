use std::path::PathBuf;

/// Module to reduce inter modules exchange or something
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::serial::SerialConfig;

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
    pub fn handle_key(&mut self, key: KeyEvent) -> OverlayResponse {
        match self {
            Overlay::Settings => match key.code {
                KeyCode::Esc => OverlayResponse::Close,
                KeyCode::Char('e' | 'E') => OverlayResponse::ToggleSetting(SettingKey::LocalEcho),
                KeyCode::Char('w' | 'W') => OverlayResponse::ToggleSetting(SettingKey::LineWrap),
                KeyCode::Char('m' | 'M') => OverlayResponse::ToggleSetting(SettingKey::InputMode),
                KeyCode::Char('i' | 'I') => OverlayResponse::ToggleSetting(SettingKey::IdentWrap),
                KeyCode::Char('d' | 'D') => OverlayResponse::ToggleSetting(SettingKey::DisplayMode),
                KeyCode::Char('b' | 'B') => OverlayResponse::ToggleSetting(SettingKey::DisplayRaw),
                KeyCode::Char('t' | 'T') => {
                    OverlayResponse::ToggleSetting(SettingKey::DisplayTimestamp)
                }
                KeyCode::Char('x' | 'X') => {
                    OverlayResponse::ToggleSetting(SettingKey::TextEncoding)
                }
                _ => OverlayResponse::Handled,
            },
            Overlay::Help(dialog) => dialog.handle_key(key),
            Overlay::TerminalSize(dialog) => dialog.handle_key(key),
            Overlay::SerialSettings(dialog) => dialog.handle_key(key),
            Overlay::FilePath(dialog) => dialog.handle_key(key),
            Overlay::DeviceDisconnected(_) => {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(key.code, KeyCode::Char('c' | 'C'))
                    || matches!(key.code, KeyCode::Esc | KeyCode::Enter)
                {
                    OverlayResponse::Quit
                } else {
                    OverlayResponse::Handled
                }
            }
            Overlay::DeviceDetached(_) => {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(key.code, KeyCode::Char('c' | 'C'))
                    || matches!(key.code, KeyCode::Esc)
                {
                    OverlayResponse::Quit
                } else if matches!(key.code, KeyCode::Enter) {
                    OverlayResponse::ReconnectSerial
                } else {
                    OverlayResponse::Handled
                }
            }
            Overlay::Error(_) => match key.code {
                KeyCode::Esc | KeyCode::Enter => OverlayResponse::Close,
                _ => OverlayResponse::Handled,
            },
        }
    }
}

/// All settings types
#[derive(Debug, PartialEq, Eq)]
pub enum SettingKey {
    LocalEcho,
    InputMode,
    LineWrap,
    IdentWrap,
    DisplayTimestamp,
    TextEncoding,
    DisplayMode,
    DisplayRaw,
}
