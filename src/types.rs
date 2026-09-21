use std::path::PathBuf;

/// Module to reduce inter modules exchange or something
use chrono::{DateTime, Local};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_input::{Input, backend::crossterm::EventHandler};

use crate::serial::{CharSize, FlowControl, Parity, SerialConfig, StopBits};

pub struct LogEntry {
    pub text: String,
    pub timestamp: DateTime<Local>,
}

impl LogEntry {
    const FORMAT: &'static str = "%H:%M:%S%.3f";

    // TODO : make format config
    pub fn formatted_time(&self) -> String {
        format!("[{}]", self.timestamp.format(Self::FORMAT))
    }

    pub fn formatted_time_builder(timestamp: DateTime<Local>) -> String {
        format!("[{}]", timestamp.format(Self::FORMAT))
    }
}

#[derive(Debug, Clone)]
pub enum Overlay {
    Help,
    Settings,
    TerminalSize(ResizeDialog),
    SerialSettings(SerialConfigDialog),
    FilePath(FilePathDialog),
    DeviceDisconnected(String),
    Error(ErrorDialog),
}

pub enum OverlayResponse {
    Handled,
    Close,
    ToggleSetting(SettingKey),
    ResizeTerminal { rows: u16, cols: u16 },
    ReconfigureSerial(SerialConfig),
    StartCapture(PathBuf),
    Quit,
}

impl Overlay {
    pub fn handle_key(&mut self, key: KeyEvent) -> OverlayResponse {
        match self {
            Overlay::Help => match key.code {
                KeyCode::Esc | KeyCode::Enter => OverlayResponse::Close,
                _ => OverlayResponse::Handled,
            },
            Overlay::Settings => match key.code {
                KeyCode::Esc => OverlayResponse::Close,
                KeyCode::Char('e' | 'E') => OverlayResponse::ToggleSetting(SettingKey::LocalEcho),
                KeyCode::Char('w' | 'W') => OverlayResponse::ToggleSetting(SettingKey::LineWrap),
                KeyCode::Char('m' | 'M') => OverlayResponse::ToggleSetting(SettingKey::InputMode),
                KeyCode::Char('t' | 'T') => {
                    OverlayResponse::ToggleSetting(SettingKey::DisplayTimestamp)
                }
                _ => OverlayResponse::Handled,
            },
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
            Overlay::Error(_) => match key.code {
                KeyCode::Esc | KeyCode::Enter => OverlayResponse::Close,
                _ => OverlayResponse::Handled,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewportSize {
    pub cols: usize,
    pub rows: usize,
}

impl ViewportSize {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols: cols as usize,
            rows: rows as usize,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeField {
    Rows,
    Cols,
}

#[derive(Debug, Clone)]
pub struct ResizeDialog {
    pub rows_input: Input,
    pub cols_input: Input,
    pub active_field: ResizeField,
}

impl ResizeDialog {
    pub fn new(current_rows: u16, current_cols: u16) -> Self {
        Self {
            rows_input: Input::new(current_rows.to_string()),
            cols_input: Input::new(current_cols.to_string()),
            active_field: ResizeField::Rows,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> OverlayResponse {
        match key.code {
            KeyCode::Esc => OverlayResponse::Close,
            KeyCode::Tab => {
                self.active_field = match self.active_field {
                    ResizeField::Rows => ResizeField::Cols,
                    ResizeField::Cols => ResizeField::Rows,
                };
                OverlayResponse::Handled
            }
            KeyCode::Enter => {
                let rows = self.rows_input.value().parse::<u16>();
                let cols = self.cols_input.value().parse::<u16>();
                if let (Ok(r), Ok(c)) = (rows, cols)
                    && r > 0
                    && c > 0
                {
                    OverlayResponse::ResizeTerminal { rows: r, cols: c }
                } else {
                    OverlayResponse::Close
                }
            }
            _ => {
                let input = match self.active_field {
                    ResizeField::Rows => &mut self.rows_input,
                    ResizeField::Cols => &mut self.cols_input,
                };
                input.handle_event(&crossterm::event::Event::Key(key));
                OverlayResponse::Handled
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct FilePathDialog {
    pub file_input: Input,
}

impl FilePathDialog {
    pub fn new(prefilled_name: &str) -> Self {
        Self {
            file_input: Input::new(prefilled_name.to_string()),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> OverlayResponse {
        match key.code {
            KeyCode::Esc => OverlayResponse::Close,
            KeyCode::Enter => {
                let raw = self.file_input.value().trim();
                if raw.is_empty() {
                    OverlayResponse::Close
                } else {
                    OverlayResponse::StartCapture(PathBuf::from(raw))
                }
            }
            _ => {
                self.file_input
                    .handle_event(&crossterm::event::Event::Key(key));
                OverlayResponse::Handled
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerialSettingField {
    BaudRate,
    CharSize,
    Parity,
    StopBits,
    FlowControl,
}

impl SerialSettingField {
    pub const ALL: &[Self] = &[
        Self::BaudRate,
        Self::CharSize,
        Self::Parity,
        Self::StopBits,
        Self::FlowControl,
    ];

    pub fn prev(&self) -> Self {
        let idx = Self::ALL.iter().position(|f| f == self).unwrap_or(0);
        if idx == 0 {
            Self::ALL[Self::ALL.len() - 1]
        } else {
            Self::ALL[idx - 1]
        }
    }

    pub fn next(&self) -> Self {
        let idx = Self::ALL.iter().position(|f| f == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }
}

const COMMON_BAUDRATES: &[u32] = &[9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];

#[derive(Debug, Clone)]
pub struct SerialConfigDialog {
    pub selected_field: SerialSettingField,
    pub port_input: Input,
    pub baudrate_idx: usize,
    pub config: SerialConfig,
}

impl SerialConfigDialog {
    pub fn new(current_config: &SerialConfig) -> Self {
        let baud_idx = COMMON_BAUDRATES
            .iter()
            .position(|&b| b == current_config.baudrate)
            .unwrap_or(4); // default 115200

        Self {
            selected_field: SerialSettingField::BaudRate,
            port_input: Input::new(current_config.port.clone()),
            baudrate_idx: baud_idx,
            config: current_config.clone(),
        }
    }

    /// Cycle current field forward (Right Arrow or Space)
    pub fn cycle_next(&mut self) {
        match self.selected_field {
            SerialSettingField::BaudRate => {
                self.baudrate_idx = (self.baudrate_idx + 1) % COMMON_BAUDRATES.len();
                self.config.baudrate = COMMON_BAUDRATES[self.baudrate_idx];
            }
            SerialSettingField::CharSize => {
                self.config.char_size = match self.config.char_size {
                    CharSize::Bits5 => CharSize::Bits6,
                    CharSize::Bits6 => CharSize::Bits7,
                    CharSize::Bits7 => CharSize::Bits8,
                    CharSize::Bits8 => CharSize::Bits5,
                };
            }
            SerialSettingField::Parity => {
                self.config.parity = match self.config.parity {
                    Parity::None => Parity::Even,
                    Parity::Even => Parity::Odd,
                    Parity::Odd => Parity::None,
                };
            }
            SerialSettingField::StopBits => {
                self.config.stop_bits = match self.config.stop_bits {
                    StopBits::One => StopBits::Two,
                    StopBits::Two => StopBits::One,
                };
            }
            SerialSettingField::FlowControl => {
                self.config.flow_control = match self.config.flow_control {
                    FlowControl::None => FlowControl::RtsCts,
                    FlowControl::RtsCts => FlowControl::XonXoff,
                    FlowControl::XonXoff => FlowControl::None,
                };
            }
        }
    }

    /// Cycle current field backward (Left Arrow)
    pub fn cycle_prev(&mut self) {
        match self.selected_field {
            SerialSettingField::BaudRate => {
                if self.baudrate_idx == 0 {
                    self.baudrate_idx = COMMON_BAUDRATES.len() - 1;
                } else {
                    self.baudrate_idx -= 1;
                }
                self.config.baudrate = COMMON_BAUDRATES[self.baudrate_idx];
            }
            SerialSettingField::CharSize => {
                self.config.char_size = match self.config.char_size {
                    CharSize::Bits5 => CharSize::Bits8,
                    CharSize::Bits6 => CharSize::Bits5,
                    CharSize::Bits7 => CharSize::Bits6,
                    CharSize::Bits8 => CharSize::Bits7,
                };
            }
            SerialSettingField::Parity => {
                self.config.parity = match self.config.parity {
                    Parity::None => Parity::Odd,
                    Parity::Even => Parity::None,
                    Parity::Odd => Parity::Even,
                };
            }
            SerialSettingField::StopBits => {
                self.cycle_next();
            }
            SerialSettingField::FlowControl => {
                self.config.flow_control = match self.config.flow_control {
                    FlowControl::None => FlowControl::XonXoff,
                    FlowControl::RtsCts => FlowControl::None,
                    FlowControl::XonXoff => FlowControl::RtsCts,
                };
            }
        }
    }

    /// Finalize and retrieve updated config
    pub fn to_config(&self) -> SerialConfig {
        let mut final_cfg = self.config.clone();
        final_cfg.port = self.port_input.value().to_string();
        final_cfg
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> OverlayResponse {
        match key.code {
            KeyCode::Esc => OverlayResponse::Close,
            KeyCode::Tab | KeyCode::Down => {
                self.selected_field = self.selected_field.next();
                OverlayResponse::Handled
            }
            KeyCode::Up => {
                self.selected_field = self.selected_field.prev();
                OverlayResponse::Handled
            }
            KeyCode::Right | KeyCode::Char(' ') => {
                self.cycle_next();
                OverlayResponse::Handled
            }
            KeyCode::Left => {
                self.cycle_prev();
                OverlayResponse::Handled
            }
            KeyCode::Enter => {
                let new_cfg = self.to_config();
                OverlayResponse::ReconfigureSerial(new_cfg)
            }
            _ => OverlayResponse::Handled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTarget {
    Terminal,
    LineInput,
    LineHistory,
    Overlay,
}

#[derive(Debug, Clone)]
pub struct ErrorDialog {
    pub title: &'static str,
    pub summary: String,
    pub details: Option<String>,
}

impl ErrorDialog {
    pub fn new(title: &'static str, summary: impl Into<String>) -> Self {
        Self {
            title,
            summary: summary.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: impl ToString) -> Self {
        self.details = Some(details.to_string());
        self
    }
}

/// All settings types
#[derive(Debug, PartialEq, Eq)]
pub enum SettingKey {
    LocalEcho,
    InputMode,
    LineWrap,
    DisplayTimestamp,
} // TODO: add all settings
