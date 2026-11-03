
//! The right-click menu's rows and keys (MAX-130).

use super::*;

/// The rows follow the mode and each recording's state, both recordings in both modes
/// (MAX-147); the mouse toggle is never one. The screenshot and the terminal size are
/// terminal mode's only (MAX-132). The link's rows say its kind (MAX-168).
/// How: line mode on a serial port with nothing recording, terminal mode on a TCP link
/// recording, then a replay.
#[test]
fn rows_follow_the_state() {
    let labels = |menu: &ActionMenu| menu.items.iter().map(|(l, _)| *l).collect::<Vec<_>>();
    let line = ActionMenu::new((0, 0), Mode::Line, LinkKind::Serial, false);
    assert!(labels(&line).contains(&"Switch to TERM"));
    assert!(labels(&line).contains(&"Record…"));
    assert!(!labels(&line).contains(&"Screenshot"));
    assert!(!labels(&line).contains(&"Terminal size"));
    assert!(labels(&line).contains(&"Serial port"));
    assert!(labels(&line).contains(&"Detach / attach port"));

    let term = ActionMenu::new((0, 0), Mode::Terminal, LinkKind::Tcp, true);
    assert!(labels(&term).contains(&"Switch to LINE"));
    assert!(labels(&term).contains(&"Stop recording"));
    assert!(!labels(&term).contains(&"Record…"));
    assert!(labels(&term).contains(&"Screenshot"));
    assert!(labels(&term).contains(&"Terminal size"));
    assert!(labels(&term).contains(&"TCP connection"));
    assert!(labels(&term).contains(&"Detach / attach connection"));
    let replay = ActionMenu::new((0, 0), Mode::Terminal, LinkKind::Replay, false);
    assert!(labels(&replay).contains(&"Replay"));
    assert!(labels(&replay).contains(&"Back to live link"));
    assert!(
        !term
            .items
            .iter()
            .any(|&(_, action)| action == KeyBindingAction::ToggleMouse)
    );
}

/// Up and Down wrap around, Enter and a click run the row's action, Esc closes.
/// How: Up from the first row, Down back, Enter; a click on row 1; Esc.
#[test]
fn keys_and_clicks() {
    let mut menu = ActionMenu::new((0, 0), Mode::Line, LinkKind::Serial, false);
    let last = menu.items.len() - 1;
    menu.handle_action(DialogAction::Up);
    assert_eq!(menu.selected, last);
    menu.handle_action(DialogAction::Down);
    assert_eq!(menu.selected, 0);
    assert!(matches!(
        menu.handle_action(DialogAction::Confirm),
        OverlayResponse::Run(KeyBindingAction::Help)
    ));
    assert!(matches!(
        menu.handle_action(DialogAction::Pick(PopupTarget::MenuItem(1))),
        OverlayResponse::Run(KeyBindingAction::Settings)
    ));
    assert!(matches!(
        menu.handle_action(DialogAction::Close),
        OverlayResponse::Close
    ));
}
