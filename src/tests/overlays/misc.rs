
//! The recording dialog's stream choice (MAX-147).

use super::*;

fn dialog() -> FilePathDialog {
    FilePathDialog::new(Stream::Text, "a".to_string())
}

/// MAX-175: the name has no extension, the stream sets what it saves: `.log` for text, a
/// folder for a full recording; switching keeps the name, and Apply starts the chosen
/// stream with it.
/// How: Tab (CycleNext), then a click on the stream row (back to text), then Apply.
#[test]
fn the_stream_sets_what_the_name_saves() {
    let mut dialog = dialog();
    assert_eq!(dialog.stream.suffix(), ".log");
    dialog.handle_action(DialogAction::CycleNext);
    assert_eq!((dialog.stream, dialog.stream.suffix()), (Stream::Full, "/"));
    dialog.handle_action(DialogAction::Pick(PopupTarget::CaptureStream));
    assert_eq!(
        (dialog.stream, dialog.file_input.value()),
        (Stream::Text, "a")
    );
    assert!(matches!(
        dialog.handle_action(DialogAction::Confirm),
        OverlayResponse::StartCapture { stream: Stream::Text, path, include_session: false } if path.as_os_str() == "a"
    ));
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
