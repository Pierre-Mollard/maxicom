use crossterm::event::KeyEvent;

use super::*;
use crate::commands::value::Value;

/// What a response copied, and as what
fn copied(response: OverlayResponse) -> Option<(String, String)> {
    match response {
        OverlayResponse::HandledCopy { text, what } => Some((text, what)),
        _ => None,
    }
}

fn key(c: char) -> DialogAction {
    DialogAction::HandleKey(KeyEvent::from(KeyCode::Char(c)))
}

/// MAX-165 (A1 step 5): the popup opens on the first line; the keys move the selection, a
/// click selects; Enter, `c` and the Copy button copy the selected line, `a` and Copy all the
/// whole answer; Esc closes.
/// How: a two-line answer, the keys and clicks one by one.
#[test]
fn query_popup_keys() {
    let value = Value::Map(vec![
        ("a".to_string(), Value::Int(1)),
        ("b".to_string(), Value::Int(2)),
    ]);
    let mut view = QueryView::new(Answer {
        query: "status",
        command: "status".to_string(),
        value,
    });
    assert_eq!(view.lines, ["a: 1", "b: 2"]);
    assert_eq!(view.selected, 0);

    view.handle_action(DialogAction::Down);
    view.handle_action(DialogAction::Down);
    assert_eq!(view.selected, 1);
    let line = Some(("b: 2".to_string(), "status line".to_string()));
    assert_eq!(copied(view.handle_action(DialogAction::Confirm)), line);
    assert_eq!(copied(view.handle_action(key('c'))), line);
    let picked = DialogAction::Pick(PopupTarget::QueryCopy);
    assert_eq!(copied(view.handle_action(picked)), line);

    let all = Some(("a: 1\nb: 2".to_string(), "status".to_string()));
    assert_eq!(copied(view.handle_action(key('a'))), all);
    let picked = DialogAction::Pick(PopupTarget::QueryCopyAll);
    assert_eq!(copied(view.handle_action(picked)), all);

    view.handle_action(DialogAction::Pick(PopupTarget::QueryRow(0)));
    assert_eq!(view.selected, 0);
    view.handle_action(DialogAction::End);
    assert_eq!(view.selected, 1);
    view.handle_action(DialogAction::Home);
    assert_eq!(view.selected, 0);
    assert!(matches!(
        view.handle_action(DialogAction::Close),
        OverlayResponse::Close
    ));
}
