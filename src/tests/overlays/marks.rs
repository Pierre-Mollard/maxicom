use super::*;
use crossterm::event::KeyEvent;

fn key(code: KeyCode) -> DialogAction {
    DialogAction::HandleKey(KeyEvent::new(code, KeyModifiers::NONE))
}

fn row(letter: char) -> MarkRow {
    MarkRow {
        letter,
        time: None,
        text: format!("line {letter}"),
    }
}

/// MAX-164: a letter jumps to its mark, whether listed or not (the app says when it isn't
/// set); Enter, the button or a click on the selected row jump to the selected one, a click
/// on another selects it; Delete takes the selected one off and keeps the popup on the next.
/// How: marks a, c, k; the keys, the clicks, then the deletes down to none.
#[test]
fn keys_and_clicks() {
    let mut browser = MarkBrowser::new(vec![row('a'), row('c'), row('k')]);
    let jumps = |response| match response {
        OverlayResponse::JumpMark(letter) => Some(letter),
        _ => None,
    };
    assert_eq!(
        jumps(browser.handle_action(key(KeyCode::Char('c')))),
        Some('c')
    );
    assert_eq!(
        jumps(browser.handle_action(key(KeyCode::Char('q')))),
        Some('q')
    );
    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(
        jumps(browser.handle_action(DialogAction::HandleKey(ctrl_c))),
        None
    );

    browser.handle_action(DialogAction::Down);
    assert_eq!(
        jumps(browser.handle_action(DialogAction::Confirm)),
        Some('c')
    );
    let button = DialogAction::Pick(PopupTarget::MarkJump);
    assert_eq!(jumps(browser.handle_action(button)), Some('c'));
    let click = |i| DialogAction::Pick(PopupTarget::MarkRow(i));
    assert_eq!(jumps(browser.handle_action(click(2))), None);
    assert_eq!(browser.selected, 2);
    assert_eq!(jumps(browser.handle_action(click(2))), Some('k'));
    browser.handle_action(DialogAction::Home);
    assert_eq!(browser.selected, 0);

    let deletes = |response| match response {
        OverlayResponse::DeleteMark(letter) => Some(letter),
        _ => None,
    };
    browser.handle_action(DialogAction::End);
    assert_eq!(
        deletes(browser.handle_action(key(KeyCode::Delete))),
        Some('k')
    );
    assert_eq!(browser.selected, 1);
    let button = DialogAction::Pick(PopupTarget::MarkDelete);
    assert_eq!(deletes(browser.handle_action(button)), Some('c'));
    assert_eq!(
        deletes(browser.handle_action(key(KeyCode::Delete))),
        Some('a')
    );
    assert!(browser.rows.is_empty());
    assert_eq!(deletes(browser.handle_action(key(KeyCode::Delete))), None);
    assert_eq!(jumps(browser.handle_action(DialogAction::Confirm)), None);
    assert!(matches!(
        browser.handle_action(DialogAction::Close),
        OverlayResponse::Close
    ));
}
