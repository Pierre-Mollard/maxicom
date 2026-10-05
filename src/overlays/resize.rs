use crossterm::event::{KeyCode, KeyEvent};
use tui_input::Input;

use crate::{input::dispatch_input_key, overlays::OverlayResponse};

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
                dispatch_input_key(input, key);
                OverlayResponse::Handled
            }
        }
    }
}
