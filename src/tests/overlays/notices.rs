use super::*;
use crate::notification::ToastLevel;
use crossterm::event::KeyEvent;

fn notice(text: &str) -> Notice {
    Notice {
        time: jiff::Timestamp::now(),
        level: ToastLevel::Info,
        message: text.to_string(),
    }
}

fn copied(response: OverlayResponse) -> Option<String> {
    match response {
        OverlayResponse::HandledCopy { text, what } => {
            assert_eq!(what, "notification");
            Some(text)
        }
        _ => None,
    }
}

/// MAX-276: the popup opens on the newest notice; the keys move the selection, a click selects;
/// Enter, `c` and the button copy the selected text, Ctrl-c doesn't. Empty, nothing is copied.
/// How: three notices, the keys and clicks one by one; then an empty popup.
#[test]
fn keys_and_clicks() {
    let mut browser = NoticeBrowser::new(vec![notice("a"), notice("b"), notice("c")]);
    assert_eq!(browser.selected, 2);
    assert_eq!(
        copied(browser.handle_action(DialogAction::Confirm)),
        Some("c".into())
    );
    browser.handle_action(DialogAction::Up);
    let c = DialogAction::HandleKey(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
    assert_eq!(copied(browser.handle_action(c)), Some("b".into()));
    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(
        copied(browser.handle_action(DialogAction::HandleKey(ctrl_c))),
        None
    );
    browser.handle_action(DialogAction::Pick(PopupTarget::NoticeRow(0)));
    let button = DialogAction::Pick(PopupTarget::NoticeCopy);
    assert_eq!(copied(browser.handle_action(button)), Some("a".into()));
    browser.handle_action(DialogAction::End);
    assert_eq!(browser.selected, 2);
    browser.handle_action(DialogAction::PageUp);
    assert_eq!(browser.selected, 0);
    assert!(matches!(
        browser.handle_action(DialogAction::Close),
        OverlayResponse::Close
    ));

    let mut empty = NoticeBrowser::new(Vec::new());
    assert_eq!(copied(empty.handle_action(DialogAction::Confirm)), None);
    empty.handle_action(DialogAction::Down);
    assert_eq!(empty.selected, 0);
}
