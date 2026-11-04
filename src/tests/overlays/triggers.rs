use super::*;
use crossterm::event::{KeyEvent, KeyModifiers};

fn key(c: char) -> DialogAction {
    DialogAction::HandleKey(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
}

fn toggles(response: OverlayResponse) -> Option<Option<String>> {
    match response {
        OverlayResponse::ToggleTrigger(name) => Some(name),
        _ => None,
    }
}

/// The keys and clicks: moves within the rules, Enter, space or the button toggle the
/// selected one, a click selects then toggles, `a` toggles them all, `c` or its button
/// copies the file's path.
#[test]
fn keys_and_clicks() {
    let path = PathBuf::from("/conf/triggers.toml");
    let mut browser = TriggerBrowser::new(vec!["login".into(), "u-boot".into()], path);
    browser.handle_action(DialogAction::Up);
    assert_eq!(browser.selected, 0);
    browser.handle_action(DialogAction::PageDown);
    assert_eq!(browser.selected, 1);
    let u_boot = Some(Some("u-boot".to_string()));
    assert_eq!(
        toggles(browser.handle_action(DialogAction::Confirm)),
        u_boot
    );
    assert_eq!(toggles(browser.handle_action(key(' '))), u_boot);
    let button = DialogAction::Pick(PopupTarget::TriggerToggle);
    assert_eq!(toggles(browser.handle_action(button)), u_boot);

    let row = |i| DialogAction::Pick(PopupTarget::TriggerRow(i));
    assert_eq!(toggles(browser.handle_action(row(0))), None);
    assert_eq!(browser.selected, 0);
    let login = Some(Some("login".to_string()));
    assert_eq!(toggles(browser.handle_action(row(0))), login);

    assert_eq!(toggles(browser.handle_action(key('a'))), Some(None));
    let all = DialogAction::Pick(PopupTarget::TriggerToggleAll);
    assert_eq!(toggles(browser.handle_action(all)), Some(None));
    for copy in [key('c'), DialogAction::Pick(PopupTarget::TriggerCopyPath)] {
        assert!(matches!(
            browser.handle_action(copy),
            OverlayResponse::HandledCopy { text, .. } if text == "/conf/triggers.toml"
        ));
    }
    assert!(matches!(
        browser.handle_action(DialogAction::Close),
        OverlayResponse::Close
    ));
}

/// MAX-163: Tab, Space and Shift-Tab (cycle next, previous) toggle the selected one; Left
/// disarms it and Right arms it, whatever it is now.
/// How: the second rule selected, each action's response.
#[test]
fn arrows_set_and_tab_toggles() {
    let path = PathBuf::from("/conf/triggers.toml");
    let mut browser = TriggerBrowser::new(vec!["login".into(), "u-boot".into()], path);
    browser.handle_action(DialogAction::Down);
    let u_boot = Some(Some("u-boot".to_string()));
    for cycle in [DialogAction::CycleNext, DialogAction::CyclePrev] {
        assert_eq!(toggles(browser.handle_action(cycle)), u_boot);
    }
    let arms = |response| match response {
        OverlayResponse::ArmTrigger { name, on } => Some((name, on)),
        _ => None,
    };
    let u_boot = |on| Some(("u-boot".to_string(), on));
    assert_eq!(
        arms(browser.handle_action(DialogAction::Left)),
        u_boot(false)
    );
    assert_eq!(
        arms(browser.handle_action(DialogAction::Right)),
        u_boot(true)
    );
}

/// With no rule, nothing to toggle but all, which says so.
#[test]
fn empty() {
    let mut browser = TriggerBrowser::new(Vec::new(), PathBuf::new());
    assert_eq!(toggles(browser.handle_action(DialogAction::Confirm)), None);
    let right = browser.handle_action(DialogAction::Right);
    assert!(matches!(right, OverlayResponse::Handled));
    browser.handle_action(DialogAction::End);
    assert_eq!(browser.selected, 0);
    assert_eq!(toggles(browser.handle_action(key('a'))), Some(None));
}
