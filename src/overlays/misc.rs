use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent};
use tui_input::Input;

use crate::{input::dispatch_input_key, overlays::OverlayResponse};

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
                dispatch_input_key(&mut self.file_input, key);
                OverlayResponse::Handled
            }
        }
    }
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
