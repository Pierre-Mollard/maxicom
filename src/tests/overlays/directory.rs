use super::*;

fn opens(response: OverlayResponse) -> Option<String> {
    match response {
        OverlayResponse::Open(address) => Some(address.to_string()),
        _ => None,
    }
}

/// MAX-181: the device in use is selected when listed, the first port else; moves stay
/// in the list; Enter or the button opens the selected one, a click selects then opens;
/// an empty list opens nothing.
/// How: three ports with the second in use, the moves, then each way to open; an empty
/// list.
#[test]
fn select_and_open() {
    let ports = || vec!["COM1".to_string(), "COM3".into(), "COM4".into()];
    let mut directory = Directory::new(ports(), &"COM3".parse().unwrap());
    assert_eq!(directory.selected, 1);
    assert_eq!(
        Directory::new(ports(), &"COM9".parse().unwrap()).selected,
        0
    );

    directory.handle_action(DialogAction::End);
    assert_eq!(directory.selected, 2);
    directory.handle_action(DialogAction::Down);
    assert_eq!(directory.selected, 2);
    directory.handle_action(DialogAction::PageUp);
    assert_eq!(directory.selected, 0);

    let com1 = Some("COM1".to_string());
    assert_eq!(opens(directory.handle_action(DialogAction::Confirm)), com1);
    assert_eq!(directory.opening, com1);
    let button = DialogAction::Pick(PopupTarget::DirectoryOpen);
    assert_eq!(opens(directory.handle_action(button)), com1);
    let row = |idx| DialogAction::Pick(PopupTarget::DirectoryRow(idx));
    assert_eq!(opens(directory.handle_action(row(2))), None);
    assert_eq!(directory.selected, 2);
    let com4 = Some("COM4".to_string());
    assert_eq!(opens(directory.handle_action(row(2))), com4);

    let mut empty = Directory::new(Vec::new(), &"COM3".parse().unwrap());
    assert_eq!(opens(empty.handle_action(DialogAction::Confirm)), None);
    assert!(matches!(
        empty.handle_action(DialogAction::Close),
        OverlayResponse::Close
    ));
}

/// MAX-185 step 0: Up, Home and PageDown move within the list, a click past its end
/// selects the last row; `t` and the Test button ask for the test; `s` asks a name for the
/// selected port (step 3); the other letters do nothing.
/// How: five ports, each action, the selection or the response read after each.
#[test]
fn moves_and_test_keys() {
    use crossterm::event::KeyEvent;
    let ports = (0..5).map(|i| format!("COM{i}")).collect();
    let mut directory = Directory::new(ports, &"COM0".parse().unwrap());
    directory.handle_action(DialogAction::PageDown);
    assert_eq!(directory.selected, 4);
    directory.handle_action(DialogAction::Up);
    assert_eq!(directory.selected, 3);
    directory.handle_action(DialogAction::Home);
    assert_eq!(directory.selected, 0);
    directory.handle_action(DialogAction::Up);
    assert_eq!(directory.selected, 0);
    directory.handle_action(DialogAction::Pick(PopupTarget::DirectoryRow(9)));
    assert_eq!(directory.selected, 4);

    let key = |c| DialogAction::HandleKey(KeyEvent::from(KeyCode::Char(c)));
    assert!(matches!(
        directory.handle_action(key('t')),
        OverlayResponse::TestLinks
    ));
    assert!(matches!(
        directory.handle_action(DialogAction::Pick(PopupTarget::DirectoryTest)),
        OverlayResponse::TestLinks
    ));
    assert!(matches!(
        directory.handle_action(key('s')),
        OverlayResponse::NameLink(Some(address)) if address.to_string() == "COM4"
    ));
    for c in ['n', 'e', 'x'] {
        assert!(matches!(
            directory.handle_action(key(c)),
            OverlayResponse::Handled
        ));
    }
    assert_eq!(directory.selected, 4);
    assert!(directory.opening.is_none());
}

/// MAX-185: with saved links, the directory opens on them, the link in use selected;
/// Tab, Shift-Tab, Left, Right and a click on a tab switch, each tab keeping its
/// selection. Enter opens
/// a saved serial link with its line settings, a TCP one as `open` does; `t` and `r` are
/// the ports' only. No link opens on the ports; an error opens on the saved links.
/// How: a serial and a TCP link with the TCP one in use, two ports; each key; then no
/// link, then an error.
#[test]
fn tabs_and_saved_links() {
    use crossterm::event::KeyEvent;
    let config = |address: &str, baud| {
        let mut config = LinkConfig {
            address: address.parse().unwrap(),
            ..LinkConfig::default()
        };
        config.line.baud = baud;
        config
    };
    let board = config("/dev/ttyUSB0", 9600);
    let bridge = config("tcp://h:1", 115_200);
    let saved = |name: &str| SavedLink {
        name: name.into(),
        ..SavedLink::default()
    };
    let links = vec![(saved("board"), board.clone()), (saved("bridge"), bridge)];
    let current: Address = "tcp://h:1".parse().unwrap();
    let ports = vec!["COM1".to_string(), "COM2".into()];
    let mut directory = Directory::new(ports.clone(), &current).with_links(Ok(links), &current);
    assert_eq!(directory.tab, DirectoryTab::Saved);
    assert_eq!(directory.selection(), 1);

    directory.handle_action(DialogAction::Right);
    assert_eq!(directory.tab, DirectoryTab::Ports);
    directory.handle_action(DialogAction::Left);
    assert_eq!(directory.tab, DirectoryTab::Saved);
    directory.handle_action(DialogAction::CycleNext);
    assert_eq!(directory.tab, DirectoryTab::Ports);
    directory.handle_action(DialogAction::End);
    assert_eq!(directory.selection(), 1);
    directory.handle_action(DialogAction::CyclePrev);
    assert_eq!(directory.tab, DirectoryTab::Saved);
    directory.handle_action(DialogAction::Home);
    assert_eq!(directory.selection(), 0);
    directory.handle_action(DialogAction::Pick(PopupTarget::DirectoryTab(
        DirectoryTab::Ports,
    )));
    assert_eq!(
        (directory.tab, directory.selection()),
        (DirectoryTab::Ports, 1)
    );
    directory.handle_action(DialogAction::CycleNext);

    let key = |c| DialogAction::HandleKey(KeyEvent::from(KeyCode::Char(c)));
    // MAX-186: the test runs from either tab
    assert!(matches!(
        directory.handle_action(key('t')),
        OverlayResponse::TestLinks
    ));
    // asked first: nothing is deleted yet
    for delete in [
        key('x'),
        DialogAction::HandleKey(KeyEvent::from(KeyCode::Delete)),
    ] {
        assert!(matches!(
            directory.handle_action(delete),
            OverlayResponse::AskDeleteLink(link) if link.name == "board"
        ));
    }
    assert!(matches!(
        directory.handle_action(DialogAction::Confirm),
        OverlayResponse::OpenLink(config) if config == board
    ));
    assert_eq!(directory.opening.as_deref(), Some("/dev/ttyUSB0"));
    directory.handle_action(DialogAction::Down);
    assert!(matches!(
        directory.handle_action(DialogAction::Confirm),
        OverlayResponse::Open(address) if address == current
    ));

    let none = Directory::new(ports.clone(), &current).with_links(Ok(Vec::new()), &current);
    assert_eq!(none.tab, DirectoryTab::Ports);
    let mut bad = Directory::new(ports, &current).with_links(Err("bad".into()), &current);
    assert_eq!(bad.tab, DirectoryTab::Saved);
    assert!(matches!(
        bad.handle_action(DialogAction::Confirm),
        OverlayResponse::Handled
    ));
}

/// MAX-181: listed again, the selection stays on its port wherever it moved, else on its
/// row within the new list.
/// How: COM3 selected, then a list with a port before it, then one without it.
#[test]
fn refresh_keeps_the_selection() {
    let mut directory = Directory::new(
        vec!["COM1".to_string(), "COM3".into()],
        &"COM3".parse().unwrap(),
    );
    directory.set_ports(
        vec!["COM0".into(), "COM1".into(), "COM3".into()],
        Some("COM3"),
    );
    assert_eq!(directory.selected, 2);
    directory.set_ports(vec!["COM1".into()], Some("COM3"));
    assert_eq!(directory.selected, 0);
}
