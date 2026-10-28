use std::path::PathBuf;

use tui_input::Input;

use crate::{
    inputs::{DialogAction, dispatch_input_key, paste_into_input},
    logger::{Screenshot, ScreenshotFormat, Stream},
    overlays::{OverlayResponse, PopupTarget},
};

/// The recording dialog: the file and the stream it records (MAX-147). Switching the stream
/// swaps in the other stream's default path, unless the path was edited. With a session
/// backup running, `include_session` is whether the recording starts with its content
#[derive(Debug, Clone)]
pub struct FilePathDialog {
    pub file_input: Input,
    pub stream: Stream,
    pub include_session: Option<bool>,
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
            include_session: None,
            text_path,
            raw_path,
        }
    }

    pub fn with_session_backup(mut self) -> Self {
        self.include_session = Some(false);
        self
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
            OverlayResponse::StartCapture { stream: Stream::Text, path, include_session: false } if path.as_os_str() == "a.log"
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

    /// With a session backup, Shift-Tab (CyclePrev) or a click on its row toggles "since
    /// session start" and Apply passes it on; without one there is nothing to toggle.
    /// How: Shift-Tab then Apply, with and without the backup.
    #[test]
    fn include_session_toggles_only_with_a_backup() {
        let mut backed = dialog().with_session_backup();
        backed.handle_action(DialogAction::CyclePrev);
        assert_eq!(backed.include_session, Some(true));
        assert!(matches!(
            backed.handle_action(DialogAction::Confirm),
            OverlayResponse::StartCapture {
                include_session: true,
                ..
            }
        ));
        backed.handle_action(DialogAction::Pick(PopupTarget::CaptureSession));
        assert_eq!(backed.include_session, Some(false));

        let mut plain = dialog();
        plain.handle_action(DialogAction::CyclePrev);
        assert_eq!((plain.stream, plain.include_session), (Stream::Text, None));
    }

    fn shot() -> Screenshot {
        Screenshot {
            text: "plain".to_string(),
            ansi: "\x1b[31mred".to_string(),
        }
    }

    /// MAX-147: the screenshot dialog starts on the key's format; switching it swaps the
    /// extension while the path wasn't edited, and Enter saves that format's screen.
    /// How: opened on ansi, Tab (text), a click on the format row (ansi), Tab (text), Apply.
    #[test]
    fn screenshot_format_switch_swaps_the_extension() {
        let mut dialog = ScreenshotDialog::new(ScreenshotFormat::Ansi, "shot", shot());
        assert_eq!(dialog.file_input.value(), "shot.ans");
        dialog.handle_action(DialogAction::CycleNext);
        assert_eq!(
            (dialog.format, dialog.file_input.value()),
            (ScreenshotFormat::Text, "shot.txt")
        );
        dialog.handle_action(DialogAction::Pick(PopupTarget::ScreenshotFormat));
        assert_eq!(
            (dialog.format, dialog.file_input.value()),
            (ScreenshotFormat::Ansi, "shot.ans")
        );
        dialog.handle_action(DialogAction::CycleNext);
        assert!(matches!(
            dialog.handle_action(DialogAction::Confirm),
            OverlayResponse::SaveScreenshot { path, contents } if path.as_os_str() == "shot.txt" && contents == "plain"
        ));
    }

    /// An edited path stays when the format switches, an empty one closes the dialog.
    #[test]
    fn screenshot_format_switch_keeps_an_edited_path() {
        let mut dialog = ScreenshotDialog::new(ScreenshotFormat::Text, "shot", shot());
        dialog.file_input = Input::new(" mine.log ".to_string());
        dialog.handle_action(DialogAction::CycleNext);
        assert!(matches!(
            dialog.handle_action(DialogAction::Confirm),
            OverlayResponse::SaveScreenshot { path, contents } if path.as_os_str() == "mine.log" && contents == "\x1b[31mred"
        ));
        dialog.file_input = Input::default();
        assert!(matches!(
            dialog.handle_action(DialogAction::Confirm),
            OverlayResponse::Close
        ));
    }
}
