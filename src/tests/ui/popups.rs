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
