use crossterm::event::KeyCode;
use tui_input::Input;

use crate::{
    inputs::{DialogAction, dispatch_input_key, paste_into_input},
    overlays::{OverlayResponse, PopupTarget},
};

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

    fn active_input(&mut self) -> &mut Input {
        match self.active_field {
            ResizeField::Rows => &mut self.rows_input,
            ResizeField::Cols => &mut self.cols_input,
        }
    }

    fn step(&mut self, by: i32) {
        let input = self.active_input();
        let value = input.value().parse::<i32>().unwrap_or(0);
        let value = (value + by).clamp(1, u16::MAX.into());
        *input = Input::new(value.to_string());
    }

    pub fn handle_action(&mut self, action: DialogAction) -> OverlayResponse {
        match action {
            DialogAction::Close => OverlayResponse::Close,
            DialogAction::Pick(PopupTarget::ResizeField(field)) => {
                self.active_field = field;
                OverlayResponse::Handled
            }
            DialogAction::CycleNext | DialogAction::Down => {
                self.active_field = match self.active_field {
                    ResizeField::Rows => ResizeField::Cols,
                    ResizeField::Cols => ResizeField::Rows,
                };
                OverlayResponse::Handled
            }
            DialogAction::CyclePrev | DialogAction::Up => {
                self.active_field = match self.active_field {
                    ResizeField::Rows => ResizeField::Cols,
                    ResizeField::Cols => ResizeField::Rows,
                };
                OverlayResponse::Handled
            }
            DialogAction::Confirm => {
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
            DialogAction::Right => {
                self.step(1);
                OverlayResponse::Handled
            }
            DialogAction::Left => {
                self.step(-1);
                OverlayResponse::Handled
            }
            DialogAction::HandleKey(raw_key) => {
                if !matches!(raw_key.code, KeyCode::Char(c) if !c.is_ascii_digit()) {
                    dispatch_input_key(self.active_input(), raw_key);
                }
                OverlayResponse::Handled
            }
            DialogAction::Paste(text) => {
                let digits: String = text.chars().filter(char::is_ascii_digit).collect();
                paste_into_input(self.active_input(), &digits);
                OverlayResponse::Handled
            }
            _ => OverlayResponse::Handled,
        }
    }
}
