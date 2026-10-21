use std::path::PathBuf;

use tui_input::Input;

use crate::{
    inputs::{DialogAction, dispatch_input_key},
    overlays::OverlayResponse,
};

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

    pub fn handle_action(&mut self, action: DialogAction) -> OverlayResponse {
        match action {
            DialogAction::Close => OverlayResponse::Close,
            DialogAction::Confirm => {
                let raw = self.file_input.value().trim();
                if raw.is_empty() {
                    OverlayResponse::Close
                } else {
                    OverlayResponse::StartCapture(PathBuf::from(raw))
                }
            }
            DialogAction::HandleKey(raw_key) => {
                dispatch_input_key(&mut self.file_input, raw_key);
                OverlayResponse::Handled
            }
            _ => OverlayResponse::Handled,
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
