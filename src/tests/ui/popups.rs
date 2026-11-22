//! Key hints and where their keys and labels are, for the mouse (MAX-125).

use super::*;
use crate::configs::KeyBindingsConfig;

fn hints_with(dialog_actions: &[(&str, &str)]) -> Hints<'static> {
    let config = KeyBindingsConfig {
        dialog_actions: dialog_actions
            .iter()
            .map(|(k, a)| (k.to_string(), a.to_string()))
            .collect(),
        ..Default::default()
    };
    let resolver = KeyResolver::try_new(&config).unwrap();
    key_hints(
        &resolver,
        false,
        &[
            (
                &[
                    KeyBindingDialogAction::NavLeft,
                    KeyBindingDialogAction::NavRight,
                ],
                "Move",
            ),
            (&[KeyBindingDialogAction::Confirm], "Apply"),
            (&[KeyBindingDialogAction::Close], "Cancel"),
        ],
        &ThemeConfig::default(),
    )
}

/// The text of each spot, cut from the drawn line, with its action
fn spots(hints: &Hints) -> Vec<(String, KeyBindingDialogAction)> {
    let text: String = hints
        .line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    let chars: Vec<char> = text.chars().collect();
    hints
        .spots
        .iter()
        .map(|&(start, width, target)| {
            let (start, width) = (start as usize, width as usize);
            let PopupTarget::Hint(action) = target else {
                panic!("a key hint's spot is its action: {target:?}");
            };
            (chars[start..start + width].iter().collect(), action)
        })
        .collect()
}

/// Every key of a hint is a spot running its own action and the label runs the first one;
/// a hint with no key bound is neither drawn nor a spot, and the spots stay on their text,
/// the label's without the space before it (its hover starts on the word).
/// How: `h`/`l` Move, Enter unbound, `q` Cancel.
#[test]
fn key_hints_spots_follow_the_line() {
    use KeyBindingDialogAction::{Close, NavLeft, NavRight};
    let hints = hints_with(&[("h", "left"), ("l", "right"), ("q", "close")]);
    assert_eq!(
        spots(&hints),
        [
            ("h".to_string(), NavLeft),
            ("l".to_string(), NavRight),
            ("Move".to_string(), NavLeft),
            ("q".to_string(), Close),
            ("Cancel".to_string(), Close),
        ]
    );

    let mut targets = Vec::new();
    hints.push_targets((10, 3), Rect::new(0, 0, 13, 5), &mut targets);
    // cut at column 13: "h/l Move" from 10 keeps h, l and 1 column of the label
    assert_eq!(
        targets,
        [
            (Rect::new(10, 3, 1, 1), PopupTarget::Hint(NavLeft)),
            (Rect::new(12, 3, 1, 1), PopupTarget::Hint(NavRight)),
        ]
    );
}

/// MAX-274: an end cut keeps the start of a long text, `…` in its last column, wide characters
/// counted as two columns; no space is left before the `…` (the help's descriptions did that).
/// How: texts that fit, one column too long, Japanese text, a cut that lands after a space.
#[test]
fn end_cut_keeps_the_start() {
    assert_eq!(cut("pid", 10), "pid");
    assert_eq!(cut("0123456789", 10), "0123456789");
    assert_eq!(cut("0123456789a", 10), "012345678…");
    assert_eq!(cut("温度温度温度", 6), "温度…");
    assert_eq!(cut("Hold the line in break", 15), "Hold the line…");
}

/// MAX-279: a middle cut keeps both ends of a long text, `…` between, in the width given:
/// text that fits is left; the end gets the larger half (a `by-id` path keeps its port
/// suffix); a wide character that doesn't fit leaves its column to the end; nothing fits in
/// 0 columns, only `…` in 1, the last character joins it in 2.
/// How: a `by-id` path in 20 columns; Japanese text (2 columns a character) in 9 and 8.
#[test]
fn middle_cut_keeps_both_ends() {
    let path = "/dev/serial/by-id/usb-FTDI_FT232R_A50285BI-if00-port0";
    assert_eq!(cut_middle("/dev/ttyUSB0", 20), "/dev/ttyUSB0");
    let cut = cut_middle(path, 20);
    assert_eq!(cut, "/dev/seri…if00-port0");
    assert_eq!(cut.width(), 20);

    let wide = "日本語のポート名です";
    assert_eq!(cut_middle(wide, 9), "日本…です");
    // 7 columns of room: one character in the start's 3, two in the end's 4 and the 1 left
    assert_eq!(cut_middle(wide, 8), "日…です");

    assert_eq!(cut_middle(path, 0), "");
    assert_eq!(cut_middle(path, 1), "…");
    assert_eq!(cut_middle(path, 2), "…0");
}
