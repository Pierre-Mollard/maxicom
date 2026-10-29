use tui_input::Input;

use crate::{
    inputs::DialogAction,
    link::{
        has_line_settings,
        serial::{CharSize, FlowControl, Parity, SerialConfig, StopBits},
    },
    overlays::{OverlayResponse, PopupTarget},
};

const COMMON_BAUDRATES: &[u32] = &[9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];

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

#[derive(Debug, Clone)]
pub struct SerialConfigDialog {
    pub selected_field: SerialSettingField,
    pub port_input: Input,
    pub baudrate_idx: usize,
    pub config: SerialConfig,
    /// False for a link whose bridge owns the line settings (TCP): nothing to edit, the popup
    /// only tells it (MAX-156)
    pub line_settings: bool,
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
            line_settings: has_line_settings(&current_config.port),
        }
    }

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

    pub fn to_config(&self) -> SerialConfig {
        let mut final_cfg = self.config.clone();
        final_cfg.port = self.port_input.value().to_string();
        final_cfg
    }

    pub fn handle_action(&mut self, action: DialogAction) -> OverlayResponse {
        if !self.line_settings {
            return match action {
                DialogAction::Close | DialogAction::Confirm => OverlayResponse::Close,
                _ => OverlayResponse::Handled,
            };
        }
        match action {
            DialogAction::Close => OverlayResponse::Close,
            DialogAction::Pick(PopupTarget::SerialPrev(field)) => {
                self.selected_field = field;
                self.cycle_prev();
                OverlayResponse::Handled
            }
            DialogAction::Pick(PopupTarget::SerialNext(field)) => {
                self.selected_field = field;
                self.cycle_next();
                OverlayResponse::Handled
            }
            DialogAction::Pick(PopupTarget::SerialField(field)) => {
                if self.selected_field == field {
                    self.cycle_next();
                } else {
                    self.selected_field = field;
                }
                OverlayResponse::Handled
            }
            DialogAction::Down => {
                self.selected_field = self.selected_field.next();
                OverlayResponse::Handled
            }
            DialogAction::Up => {
                self.selected_field = self.selected_field.prev();
                OverlayResponse::Handled
            }
            DialogAction::Right | DialogAction::CycleNext => {
                self.cycle_next();
                OverlayResponse::Handled
            }
            DialogAction::Left | DialogAction::CyclePrev => {
                self.cycle_prev();
                OverlayResponse::Handled
            }
            DialogAction::Confirm => {
                let new_cfg = self.to_config();
                OverlayResponse::ReconfigureSerial(new_cfg)
            }
            _ => OverlayResponse::Handled,
        }
    }
}
