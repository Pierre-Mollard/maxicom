use super::*;
use crossterm::event::{KeyEvent, KeyModifiers};

fn key(c: char) -> DialogAction {
    DialogAction::HandleKey(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
}

fn row(name: &str) -> Listed {
    Listed {
        name: name.to_string(),
        description: String::new(),
        enabled: true,
        error: None,
    }
}

/// MAX-240: the keys and clicks: moves within the rows, Enter, space, Tab or the button
/// toggle the selected profile, a click selects then toggles, Right / Left lay it / take it
/// off, `K` / `J` or their buttons
/// move it up / down the stack, `c` or its button copies the folder's path.
/// How: two rows, each key and click in turn.
#[test]
fn keys_and_clicks() {
    let path = PathBuf::from("/conf/profiles");
    let mut browser = ProfileBrowser::new(vec![row("arduino"), row("esp32")], path);
    browser.handle_action(DialogAction::Up);
    assert_eq!(browser.selected, 0);
    browser.handle_action(DialogAction::End);
    assert_eq!(browser.selected, 1);
    let toggles =
        |response| matches!(response, OverlayResponse::ToggleProfile(ref name) if name == "esp32");
    assert!(toggles(browser.handle_action(DialogAction::Confirm)));
    assert!(toggles(browser.handle_action(key(' '))));
    assert!(toggles(browser.handle_action(DialogAction::CycleNext)));
    assert!(toggles(
        browser.handle_action(DialogAction::Pick(PopupTarget::ProfileToggle))
    ));

    let click = |i| DialogAction::Pick(PopupTarget::ProfileRow(i));
    assert!(matches!(
        browser.handle_action(click(0)),
        OverlayResponse::Handled
    ));
    assert_eq!(browser.selected, 0);
    assert!(matches!(
        browser.handle_action(click(0)),
        OverlayResponse::ToggleProfile(name) if name == "arduino"
    ));

    let sets = |response, to_on: bool| matches!(response, OverlayResponse::SetProfile { ref name, on } if name == "arduino" && on == to_on);
    assert!(sets(browser.handle_action(DialogAction::Right), true));
    assert!(sets(browser.handle_action(DialogAction::Left), false));

    let moves = |response, to_up: bool| matches!(response, OverlayResponse::MoveProfile { ref name, up } if name == "arduino" && up == to_up);
    assert!(moves(browser.handle_action(key('K')), true));
    assert!(moves(browser.handle_action(key('J')), false));
    assert!(moves(
        browser.handle_action(DialogAction::Pick(PopupTarget::ProfileMove(true))),
        true
    ));
    assert!(matches!(
        browser.handle_action(key('c')),
        OverlayResponse::HandledCopy { text, .. } if text == "/conf/profiles"
    ));
    assert!(matches!(
        browser.handle_action(DialogAction::Close),
        OverlayResponse::Close
    ));
}
