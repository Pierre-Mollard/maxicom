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
    assert!(labels(&line).contains(&"Detach / attach"));

    let term = ActionMenu::new((0, 0), Mode::Terminal, LinkKind::Tcp, true);
    assert!(labels(&term).contains(&"Switch to LINE"));
    assert!(labels(&term).contains(&"Stop recording"));
    assert!(!labels(&term).contains(&"Record…"));
    assert!(labels(&term).contains(&"Screenshot"));
    assert!(labels(&term).contains(&"Terminal size"));
    assert!(labels(&term).contains(&"TCP connection"));
    assert!(labels(&term).contains(&"Detach / attach"));
    let replay = ActionMenu::new((0, 0), Mode::Terminal, LinkKind::Replay, false);
    assert!(labels(&replay).contains(&"Replay"));
    assert!(labels(&replay).contains(&"Back to live"));
    assert!(
        !term
            .items
            .iter()
            .any(|&(_, run)| run == MenuRun::Action(KeyBindingAction::ToggleMouse))
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

/// MAX-164: the marks' rows go after Search: "Add mark", for a menu opened on a scrolled view
/// or in copy mode, marks the line it was opened on; "Marks…", while there are marks, opens
/// the popup. Each comes alone too; the menu without them has neither.
/// How: a line mode menu with none, both, then each alone; Enter and clicks on them.
#[test]
fn marks_rows_after_search() {
    let labels = |menu: &ActionMenu| menu.items.iter().map(|(l, _)| *l).collect::<Vec<_>>();
    let plain = ActionMenu::new((0, 0), Mode::Line, LinkKind::Serial, false);
    assert!(!labels(&plain).contains(&"Add mark"));
    assert!(!labels(&plain).contains(&"Marks…"));

    let menu = |add, listed| {
        ActionMenu::new((0, 0), Mode::Line, LinkKind::Serial, false).with_marks(add, listed)
    };
    assert_eq!(labels(&menu(None, false)), labels(&plain));
    let mut both = menu(Some(Some(7)), true);
    let names = labels(&both);
    let search = names.iter().position(|&l| l == "Search").unwrap();
    assert_eq!(names[search + 1..search + 3], ["Add mark", "Marks…"]);
    assert_eq!(names.len(), labels(&plain).len() + 2);

    both.selected = search + 1;
    assert!(matches!(
        both.handle_action(DialogAction::Confirm),
        OverlayResponse::AddMark(Some(7))
    ));
    assert!(matches!(
        both.handle_action(DialogAction::Pick(PopupTarget::MenuItem(search + 2))),
        OverlayResponse::Run(KeyBindingAction::Marks)
    ));

    let add_only = menu(Some(None), false);
    assert_eq!(labels(&add_only)[search + 1], "Add mark");
    assert!(!labels(&add_only).contains(&"Marks…"));
    assert!(matches!(
        add_only.run(search + 1),
        OverlayResponse::AddMark(None)
    ));
    let list_only = menu(None, true);
    assert_eq!(labels(&list_only)[search + 1], "Marks…");
    assert!(!labels(&list_only).contains(&"Add mark"));
}
