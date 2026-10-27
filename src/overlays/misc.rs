use std::path::PathBuf;

use tui_input::Input;

use crate::{
    inputs::{DialogAction, dispatch_input_key, paste_into_input},
    logger::Stream,
    overlays::{OverlayResponse, PopupTarget},
};

/// The recording dialog: the file and the stream it records (MAX-147). Switching the stream
/// swaps in the other stream's default path, unless the path was edited
#[derive(Debug, Clone)]
pub struct FilePathDialog {
    pub file_input: Input,
    pub stream: Stream,
    text_path: String,
    raw_path: String,
}

impl FilePathDialog {
    pub fn new(stream: Stream, text_path: String, raw_path: String) -> Self {
        let path = match stream {
            Stream::Text => &text_path,
            Stream::Raw => &raw_path,
        };
        Self {
            file_input: Input::new(path.clone()),
            stream,
            text_path,
            raw_path,
        }
    }

    fn default_path(&self, stream: Stream) -> &str {
        match stream {
            Stream::Text => &self.text_path,
            Stream::Raw => &self.raw_path,
        }
    }

    fn switch_stream(&mut self) {
        let is_edited = self.file_input.value() != self.default_path(self.stream);
        self.stream = self.stream.other();
        if !is_edited {
            self.file_input = Input::new(self.default_path(self.stream).to_string());
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
                    OverlayResponse::StartCapture {
                        path: PathBuf::from(raw),
                        stream: self.stream,
                    }
                }
            }
            DialogAction::CycleNext
            | DialogAction::CyclePrev
            | DialogAction::Pick(PopupTarget::CaptureStream) => {
                self.switch_stream();
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
mod tests {
    //! The recording dialog's stream choice (MAX-147).

    use super::*;

    fn dialog() -> FilePathDialog {
        FilePathDialog::new(Stream::Text, "a.log".to_string(), "a.raw".to_string())
    }

    /// Switching the stream swaps in its default path while the path wasn't edited, and Apply
    /// starts the chosen stream.
    /// How: Tab (CycleNext), then a click on the stream row (back to text), then Apply.
    #[test]
    fn switching_the_stream_swaps_the_default_path() {
        let mut dialog = dialog();
        dialog.handle_action(DialogAction::CycleNext);
        assert_eq!(
            (dialog.stream, dialog.file_input.value()),
            (Stream::Raw, "a.raw")
        );
        dialog.handle_action(DialogAction::Pick(PopupTarget::CaptureStream));
        assert_eq!(
            (dialog.stream, dialog.file_input.value()),
            (Stream::Text, "a.log")
        );
        assert!(matches!(
            dialog.handle_action(DialogAction::Confirm),
            OverlayResponse::StartCapture { stream: Stream::Text, path } if path.as_os_str() == "a.log"
        ));
    }

    /// An edited path stays when the stream switches.
    /// How: types "x" at the end, then Tab.
    #[test]
    fn switching_the_stream_keeps_an_edited_path() {
        let mut dialog = dialog();
        dialog.handle_action(DialogAction::Paste("x".to_string()));
        dialog.handle_action(DialogAction::CycleNext);
        assert_eq!(
            (dialog.stream, dialog.file_input.value()),
            (Stream::Raw, "a.logx")
        );
    }
}
