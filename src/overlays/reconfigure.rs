use crate::{
    capture::CaptureLine,
    inputs::DialogAction,
    link::{
        Address, LinkConfig,
        serial::{CharSize, FlowControl, LineSettings, Parity, StopBits},
    },
    overlays::{OverlayResponse, PopupTarget},
};

const COMMON_BAUDRATES: &[u32] = &[9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineSettingField {
    BaudRate,
    CharSize,
    Parity,
    StopBits,
    FlowControl,
}

impl LineSettingField {
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

/// The line settings of the link at `address`, edited; a link without them only says so
#[derive(Debug, Clone)]
pub struct LineSettingsDialog {
    pub selected_field: LineSettingField,
    pub baudrate_idx: usize,
    pub address: Address,
    pub line: LineSettings,
    /// What a replayed session was recorded with, shown as a replay has nothing to set (MAX-166)
    pub recorded_line: Option<CaptureLine>,
}

impl LineSettingsDialog {
    pub fn new(current_config: &LinkConfig) -> Self {
        let baud_idx = COMMON_BAUDRATES
            .iter()
            .position(|&b| b == current_config.line.baud)
            .unwrap_or(4); // default 115200

        Self {
            selected_field: LineSettingField::BaudRate,
            baudrate_idx: baud_idx,
            address: current_config.address.clone(),
            line: current_config.line,
            recorded_line: None,
        }
    }

    pub fn with_recorded_line(mut self, line: Option<CaptureLine>) -> Self {
        self.recorded_line = line;
        self
    }

    pub fn cycle_next(&mut self) {
        match self.selected_field {
            LineSettingField::BaudRate => {
                self.baudrate_idx = (self.baudrate_idx + 1) % COMMON_BAUDRATES.len();
                self.line.baud = COMMON_BAUDRATES[self.baudrate_idx];
            }
            LineSettingField::CharSize => {
                self.line.framing.char_size = match self.line.framing.char_size {
                    CharSize::Bits5 => CharSize::Bits6,
                    CharSize::Bits6 => CharSize::Bits7,
                    CharSize::Bits7 => CharSize::Bits8,
                    CharSize::Bits8 => CharSize::Bits5,
                };
            }
            LineSettingField::Parity => {
                self.line.framing.parity = match self.line.framing.parity {
                    Parity::None => Parity::Even,
                    Parity::Even => Parity::Odd,
                    Parity::Odd => Parity::None,
                };
            }
            LineSettingField::StopBits => {
                self.line.framing.stop_bits = match self.line.framing.stop_bits {
                    StopBits::One => StopBits::Two,
                    StopBits::Two => StopBits::One,
                };
            }
            LineSettingField::FlowControl => {
                self.line.flow_control = match self.line.flow_control {
                    FlowControl::None => FlowControl::RtsCts,
                    FlowControl::RtsCts => FlowControl::XonXoff,
                    FlowControl::XonXoff => FlowControl::None,
                };
            }
        }
    }

    pub fn cycle_prev(&mut self) {
        match self.selected_field {
            LineSettingField::BaudRate => {
                if self.baudrate_idx == 0 {
                    self.baudrate_idx = COMMON_BAUDRATES.len() - 1;
                } else {
                    self.baudrate_idx -= 1;
                }
                self.line.baud = COMMON_BAUDRATES[self.baudrate_idx];
            }
            LineSettingField::CharSize => {
                self.line.framing.char_size = match self.line.framing.char_size {
                    CharSize::Bits5 => CharSize::Bits8,
                    CharSize::Bits6 => CharSize::Bits5,
                    CharSize::Bits7 => CharSize::Bits6,
                    CharSize::Bits8 => CharSize::Bits7,
                };
            }
            LineSettingField::Parity => {
                self.line.framing.parity = match self.line.framing.parity {
                    Parity::None => Parity::Odd,
                    Parity::Even => Parity::None,
                    Parity::Odd => Parity::Even,
                };
            }
            LineSettingField::StopBits => {
                self.cycle_next();
            }
            LineSettingField::FlowControl => {
                self.line.flow_control = match self.line.flow_control {
                    FlowControl::None => FlowControl::XonXoff,
                    FlowControl::RtsCts => FlowControl::None,
                    FlowControl::XonXoff => FlowControl::RtsCts,
                };
            }
        }
    }

    /// False for a link whose bridge owns the line settings (TCP), or a replay: nothing to
    /// edit, the popup only tells it (MAX-156)
    pub fn has_line_settings(&self) -> bool {
        self.address.kind().has_line_settings()
    }

    pub fn handle_action(&mut self, action: DialogAction) -> OverlayResponse {
        if !self.has_line_settings() {
            return match action {
                DialogAction::Close | DialogAction::Confirm => OverlayResponse::Close,
                _ => OverlayResponse::Handled,
            };
        }
        match action {
            DialogAction::Close => OverlayResponse::Close,
            DialogAction::Pick(PopupTarget::LinePrev(field)) => {
                self.selected_field = field;
                self.cycle_prev();
                OverlayResponse::Handled
            }
            DialogAction::Pick(PopupTarget::LineNext(field)) => {
                self.selected_field = field;
                self.cycle_next();
                OverlayResponse::Handled
            }
            DialogAction::Pick(PopupTarget::LineField(field)) => {
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
            DialogAction::Confirm => OverlayResponse::Reconfigure(self.line),
            _ => OverlayResponse::Handled,
        }
    }
}
