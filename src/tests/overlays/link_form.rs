use super::*;
use crate::link::serial::{FlowControl, Parity};
use crossterm::event::KeyEvent;

fn key(code: KeyCode) -> DialogAction {
    DialogAction::HandleKey(KeyEvent::from(code))
}

fn type_in(form: &mut LinkForm, text: &str) {
    for c in text.chars() {
        form.handle_action(key(KeyCode::Char(c)));
    }
}

fn saved(response: OverlayResponse) -> (SavedLink, Option<SavedLink>) {
    match response {
        OverlayResponse::SaveLink { link, replaces } => (link, replaces),
        _ => panic!("not saved"),
    }
}

/// MAX-185: a new link starts with `[link]`'s line; Tab and Down go through the fields, Up
/// and Shift-Tab back, round; Left, Right and Space cycle a line value; Enter gives the
/// link with all its line settings. A baud that isn't a number or an empty name says so.
/// How: a link for one machine, each field typed or cycled, then Enter; then a bad baud,
/// then no name.
#[test]
fn new_link() {
    let defaults = LinkDefaults::default();
    let link = SavedLink {
        hosts: vec!["laptop".into()],
        ..SavedLink::default()
    };
    let mut form = LinkForm::new(link, &defaults);
    assert_eq!(form.hosts.value(), "laptop");
    assert_eq!(form.baud.value(), "115200");
    type_in(&mut form, "esp32");
    form.handle_action(DialogAction::CycleNext);
    type_in(&mut form, "/dev/ttyUSB0");
    form.handle_action(key(KeyCode::Down));
    form.handle_action(key(KeyCode::Down));
    assert_eq!(form.field, LinkFormField::Baud);
    form.baud = Input::new("74880".into());
    form.handle_action(key(KeyCode::Down));
    form.handle_action(key(KeyCode::Left));
    assert_eq!(form.field, LinkFormField::Bits);
    form.handle_action(DialogAction::CycleNext);
    form.handle_action(key(KeyCode::Char(' ')));
    form.handle_action(key(KeyCode::Down));
    form.handle_action(key(KeyCode::Down));
    form.handle_action(key(KeyCode::Right));
    assert_eq!(form.field, LinkFormField::Flow);
    form.handle_action(key(KeyCode::Down));
    assert_eq!(form.field, LinkFormField::Name);
    form.handle_action(key(KeyCode::Up));
    assert_eq!(form.field, LinkFormField::Flow);

    let (link, replaces) = saved(form.handle_action(DialogAction::Confirm));
    assert!(replaces.is_none());
    let expected = SavedLink {
        name: "esp32".into(),
        hosts: vec!["laptop".into()],
        address: "/dev/ttyUSB0".into(),
        baud: Some(74880),
        bits: Some(7),
        parity: Some(Parity::Even),
        stop_bits: Some(1),
        flow: Some(FlowControl::RtsCts),
    };
    assert_eq!(link, expected);

    form.baud = Input::new("fast".into());
    assert!(matches!(
        form.handle_action(DialogAction::Confirm),
        OverlayResponse::HandledNotify { message, .. } if message == "baud: 'fast' is not a number"
    ));
    form.name = Input::new(" ".into());
    assert!(matches!(
        form.handle_action(DialogAction::Confirm),
        OverlayResponse::HandledNotify { message, .. } if message == "the name is empty"
    ));
}

/// MAX-185: an edit starts on the link as saved, its missing line fields `[link]`'s; a
/// TCP address skips the line fields and saves none; the hosts split on commas, none for
/// every machine. Enter gives the link in place of the one edited; Esc goes back to the
/// directory.
/// How: a TCP link without hosts edited to two hosts, Tab through it; then Esc.
#[test]
fn edit_link() {
    let defaults = LinkDefaults {
        baud: 9600,
        ..LinkDefaults::default()
    };
    let old = SavedLink {
        name: "router".into(),
        address: "tcp://10.0.0.1:4000".into(),
        ..SavedLink::default()
    };
    let mut form = LinkForm::edit(old.clone(), &defaults);
    assert_eq!(form.baud.value(), "9600");
    assert!(!form.has_line());
    form.handle_action(DialogAction::CycleNext);
    form.handle_action(DialogAction::CycleNext);
    assert_eq!(form.field, LinkFormField::Hosts);
    type_in(&mut form, "a, ,b");
    form.handle_action(DialogAction::CycleNext);
    assert_eq!(form.field, LinkFormField::Name);
    form.handle_action(DialogAction::Pick(PopupTarget::LinkFormField(
        LinkFormField::Parity,
    )));
    assert_eq!(form.field, LinkFormField::Name);

    let (link, replaces) = saved(form.handle_action(DialogAction::Confirm));
    assert_eq!(replaces, Some(old));
    assert_eq!(link.hosts, ["a", "b"]);
    assert_eq!(link.baud, None);
    assert!(matches!(
        form.handle_action(DialogAction::Close),
        OverlayResponse::Run(KeyBindingAction::Directory)
    ));
}
