
//! Mouse reports for a remote program (MAX-113).

use super::*;

fn event(kind: MouseEventKind, modifiers: KeyModifiers) -> MouseEvent {
    MouseEvent {
        kind,
        column: 0,
        row: 0,
        modifiers,
    }
}

fn report(
    mode: MouseProtocolMode,
    encoding: MouseProtocolEncoding,
    kind: MouseEventKind,
    modifiers: KeyModifiers,
    cell: (usize, usize),
) -> Option<Vec<u8>> {
    mouse_report(mode, encoding, &event(kind, modifiers), cell)
}

/// SGR: button, 1-based cell, `M` for a press and `m` for a release; the wheel is 64 and
/// up, a drag adds 32, modifiers add 4 / 8 / 16.
#[test]
fn sgr_reports() {
    use MouseProtocolEncoding::Sgr;
    use MouseProtocolMode::ButtonMotion;
    let none = KeyModifiers::NONE;
    let left = MouseButton::Left;
    let sgr = |kind, modifiers| report(ButtonMotion, Sgr, kind, modifiers, (4, 2));
    assert_eq!(
        sgr(MouseEventKind::Down(left), none).unwrap(),
        b"\x1b[<0;5;3M"
    );
    assert_eq!(
        sgr(MouseEventKind::Up(left), none).unwrap(),
        b"\x1b[<0;5;3m"
    );
    assert_eq!(
        sgr(MouseEventKind::Down(MouseButton::Right), none).unwrap(),
        b"\x1b[<2;5;3M"
    );
    assert_eq!(
        sgr(MouseEventKind::ScrollDown, none).unwrap(),
        b"\x1b[<65;5;3M"
    );
    assert_eq!(
        sgr(MouseEventKind::Drag(left), KeyModifiers::CONTROL).unwrap(),
        b"\x1b[<48;5;3M"
    );
    assert_eq!(
        sgr(
            MouseEventKind::Down(left),
            KeyModifiers::SHIFT | KeyModifiers::ALT
        )
        .unwrap(),
        b"\x1b[<12;5;3M"
    );
}

/// The default encoding: a release is button 3, values are bytes plus 32, a cell past 223
/// can't be sent; the UTF-8 one sends it as a char.
#[test]
fn default_and_utf8_reports() {
    use MouseProtocolEncoding::{Default, Utf8};
    use MouseProtocolMode::PressRelease;
    let none = KeyModifiers::NONE;
    let left = MouseButton::Left;
    let press = MouseEventKind::Down(left);
    assert_eq!(
        report(PressRelease, Default, press, none, (4, 2)).unwrap(),
        b"\x1b[M\x20\x25\x23"
    );
    assert_eq!(
        report(
            PressRelease,
            Default,
            MouseEventKind::Up(left),
            none,
            (4, 2)
        )
        .unwrap(),
        b"\x1b[M\x23\x25\x23"
    );
    assert_eq!(report(PressRelease, Default, press, none, (300, 2)), None);
    assert_eq!(
        report(PressRelease, Utf8, press, none, (300, 2)).unwrap(),
        "\x1b[M\x20\u{14D}\x23".as_bytes()
    );
}

/// Each mode reports its kinds of events only: presses (X10), and releases (1000), and
/// drags (1002), and every move (1003).
#[test]
fn modes_filter_events() {
    use MouseProtocolEncoding::Sgr;
    use MouseProtocolMode::{AnyMotion, ButtonMotion, Press, PressRelease};
    let none = KeyModifiers::NONE;
    let left = MouseButton::Left;
    let is_reported = |mode, kind| report(mode, Sgr, kind, none, (0, 0)).is_some();
    assert!(is_reported(Press, MouseEventKind::Down(left)));
    assert!(!is_reported(Press, MouseEventKind::Up(left)));
    assert!(is_reported(PressRelease, MouseEventKind::Up(left)));
    assert!(!is_reported(PressRelease, MouseEventKind::Drag(left)));
    assert!(is_reported(ButtonMotion, MouseEventKind::Drag(left)));
    assert!(!is_reported(ButtonMotion, MouseEventKind::Moved));
    assert!(is_reported(AnyMotion, MouseEventKind::Moved));
    assert_eq!(
        report(AnyMotion, Sgr, MouseEventKind::Moved, none, (0, 0)).unwrap(),
        b"\x1b[<35;1;1M"
    );
}
