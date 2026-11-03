use std::path::PathBuf;

use tui_input::Input;

use crate::{
    capture::{Screenshot, ScreenshotFormat, Stream},
    inputs::{DialogAction, dispatch_input_key, paste_into_input},
    overlays::{OverlayResponse, PopupTarget},
};

/// The recording dialog: the name and the stream it records (MAX-147). The name has no
/// extension, the stream sets what it becomes (MAX-175): `<name>.log` for text, the folder
/// `<name>` for a full recording. With a session backup running, `include_session` is whether
/// the recording starts with its content
#[derive(Debug, Clone)]
pub struct FilePathDialog {
    pub file_input: Input,
    pub stream: Stream,
    pub include_session: Option<bool>,
}

impl FilePathDialog {
    pub fn new(stream: Stream, path: String) -> Self {
        Self {
            file_input: Input::new(path),
            stream,
            include_session: None,
        }
    }

    pub fn with_session_backup(mut self) -> Self {
        self.include_session = Some(false);
        self
    }

    fn switch_stream(&mut self) {
        self.stream = self.stream.other();
    }

    pub fn handle_action(&mut self, action: DialogAction) -> OverlayResponse {
        match action {
            DialogAction::Close => OverlayResponse::Close,
            DialogAction::Confirm => {
                let raw = self.file_input.value().trim();
                if raw.is_empty() {
                    OverlayResponse::Close
                } else {
                    OverlayResponse::StartCapture {
                        path: PathBuf::from(raw),
                        stream: self.stream,
                        include_session: self.include_session == Some(true),
                    }
                }
            }
            DialogAction::CycleNext | DialogAction::Pick(PopupTarget::CaptureStream) => {
                self.switch_stream();
                OverlayResponse::Handled
            }
            DialogAction::CyclePrev | DialogAction::Pick(PopupTarget::CaptureSession) => {
                if let Some(include) = &mut self.include_session {
                    *include = !*include;
                }
                OverlayResponse::Handled
            }
            DialogAction::HandleKey(raw_key) => {
                dispatch_input_key(&mut self.file_input, raw_key);
                OverlayResponse::Handled
            }
            DialogAction::Paste(text) => {
                paste_into_input(&mut self.file_input, &text);
                OverlayResponse::Handled
            }
            _ => OverlayResponse::Handled,
        }
    }
}

/// The screenshot dialog: the screen is taken in both formats when it opens, so a fast screen
/// is caught, and the dialog asks where to save it and which format. The path is prefilled
/// from `[capture]` so Enter saves to the default name. Switching the format swaps the
/// extension, unless the path was edited (MAX-147)
#[derive(Debug, Clone)]
pub struct ScreenshotDialog {
    pub file_input: Input,
    pub format: ScreenshotFormat,
    stem: String,
    shot: Screenshot,
}

impl ScreenshotDialog {
    pub fn new(format: ScreenshotFormat, stem: &str, shot: Screenshot) -> Self {
        let mut dialog = Self {
            file_input: Input::default(),
            format,
            stem: stem.to_string(),
            shot,
        };
        dialog.file_input = Input::new(dialog.default_path(format));
        dialog
    }

    fn default_path(&self, format: ScreenshotFormat) -> String {
        format!("{}.{}", self.stem, format.extension())
    }

    fn switch_format(&mut self) {
        let is_edited = self.file_input.value() != self.default_path(self.format);
        self.format = self.format.other();
        if !is_edited {
            self.file_input = Input::new(self.default_path(self.format));
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
                    OverlayResponse::SaveScreenshot {
                        path: PathBuf::from(raw),
                        contents: std::mem::take(&mut self.shot).into_contents(self.format),
                    }
                }
            }
            DialogAction::CycleNext
            | DialogAction::CyclePrev
            | DialogAction::Pick(PopupTarget::ScreenshotFormat) => {
                self.switch_format();
                OverlayResponse::Handled
            }
            DialogAction::HandleKey(raw_key) => {
                dispatch_input_key(&mut self.file_input, raw_key);
                OverlayResponse::Handled
            }
            DialogAction::Paste(text) => {
                paste_into_input(&mut self.file_input, &text);
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

#[cfg(test)]
#[path = "../tests/overlays/misc.rs"]
mod tests;
