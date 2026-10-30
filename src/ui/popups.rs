use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Padding},
};

use crate::{
    configs::{KeyPattern, KeyResolver, ThemeConfig, keybindings::KeyBindingDialogAction},
    overlays::PopupTarget,
    ui::common::popup_area,
};

pub mod alert;
pub mod command_line;
pub mod filepath;
pub mod help;
pub mod menu;
pub mod notifications;
pub mod reconfigure;
pub mod resize;
pub mod sessions;
pub mod settings;

/// Width of the form popups (settings, serial, logging, resize) and of the alerts
pub const FORM_WIDTH: u16 = 48;

/// The frame every popup shares: a title and the key hints set in by a space, and one row and
/// column of padding around the content
pub fn popup_block<'a>(title: &str, hints: &Hints<'a>, theme: &ThemeConfig) -> Block<'a> {
    let mut block = Block::bordered()
        .border_style(*theme.popup.border)
        .style(*theme.popup.background)
        .padding(Padding::uniform(1))
        .title(Span::styled(format!(" {title} "), *theme.popup.title));
    if !hints.line.spans.is_empty() {
        let mut spans = hints.line.spans.clone();
        spans.insert(0, Span::raw(" "));
        spans.push(Span::raw(" "));
        block = block.title_bottom(Line::from(spans));
    }
    block
}

/// A centered popup as tall as `lines` inside `popup_block`
pub fn form_area(parent: Rect, width: u16, lines: usize) -> Rect {
    popup_area(parent, width, lines as u16 + 4)
}

/// Row `y` of a popup's inner area, as a click target (MAX-113): cut to the area
pub fn row_area(inner: Rect, y: usize) -> Rect {
    Rect::new(inner.x, inner.y + y as u16, inner.width, 1).intersection(inner)
}

/// A section header inside a popup
pub fn section<'a>(title: &'a str, theme: &ThemeConfig) -> Line<'a> {
    Line::from(Span::styled(
        title,
        Style::default()
            .fg(theme.palette.ternary)
            .add_modifier(Modifier::BOLD),
    ))
}

/// A popup's key hints as drawn, and for the mouse where each key and label is in the line
/// (start column, width) and the dialog action a click on it runs (MAX-125)
#[derive(Default)]
pub struct Hints<'a> {
    pub line: Line<'a>,
    spots: Vec<(u16, u16, KeyBindingDialogAction)>,
}

impl Hints<'_> {
    /// The hints as click targets, their line drawn from `(x, y)`, cut to `clip`
    pub fn push_targets(
        &self,
        (x, y): (u16, u16),
        clip: Rect,
        targets: &mut Vec<(Rect, PopupTarget)>,
    ) {
        for &(start, width, action) in &self.spots {
            let area = Rect::new(x.saturating_add(start), y, width, 1).intersection(clip);
            if !area.is_empty() {
                targets.push((area, PopupTarget::Hint(action)));
            }
        }
    }

    /// The hints as click targets in the bottom border of a `popup_block` drawn on `area`
    pub fn push_block_targets(&self, area: Rect, targets: &mut Vec<(Rect, PopupTarget)>) {
        // after the corner and the space `popup_block` puts before them
        let at = (area.x + 2, area.bottom().saturating_sub(1));
        self.push_targets(at, area, targets);
    }
}

/// Hints like `Enter Apply · Esc Cancel` from the user's `[dialog_actions]` (MAX-73).
/// Each action shows its first key, so hints fit the narrow popups; a hint with no key bound is
/// left out. A text input popup types every key that is not modal (see
/// `KeyPattern::is_modal`), so only modal keys are shown there. A click on a key runs its
/// action, on the label the hint's first one
pub fn key_hints<'a>(
    resolver: &KeyResolver,
    is_text_input: bool,
    hints: &[(&[KeyBindingDialogAction], &'a str)],
    theme: &ThemeConfig,
) -> Hints<'a> {
    let hints = hints.iter().map(|&(actions, label)| {
        let keys = actions.iter().filter_map(|&action| {
            let key = resolver
                .dialog_keys(action)
                .into_iter()
                .find(|key| !is_text_input || key.is_modal())?;
            Some((key, action))
        });
        (keys.collect(), label)
    });
    let (line, spots) = hint_spans(hints, theme);
    Hints { line, spots }
}

/// Each hint's keys then its label, the hints apart by ` · `; a hint with no key is left out.
/// Also where each key and label is in the line with the value of its key (the label takes the
/// first key's), for the mouse
pub fn hint_spans<'a, T: Copy>(
    hints: impl IntoIterator<Item = (Vec<(KeyPattern, T)>, &'a str)>,
    theme: &ThemeConfig,
) -> (Line<'a>, Vec<(u16, u16, T)>) {
    let key_style = Style::default()
        .fg(theme.palette.secondary)
        .add_modifier(Modifier::BOLD);
    let muted = Style::default().fg(theme.palette.text_muted);
    let mut spans = Vec::new();
    let mut spots = Vec::new();
    let mut col = 0;
    let mut spot = |col: &mut u16, text: &str, value| {
        let width = Span::raw(text).width() as u16;
        spots.push((*col, width, value));
        *col += width;
    };

    for (keys, label) in hints {
        let Some(&(_, first)) = keys.first() else {
            continue;
        };
        if !spans.is_empty() {
            spans.push(Span::styled(" · ", muted));
            col += 3;
        }
        let mut names = Vec::with_capacity(keys.len());
        for (idx, (key, value)) in keys.into_iter().enumerate() {
            if idx > 0 {
                col += 1;
            }
            let name = key.to_string();
            spot(&mut col, &name, value);
            names.push(name);
        }
        spans.push(Span::styled(names.join("/"), key_style));
        let label = format!(" {label}");
        spot(&mut col, &label, first);
        spans.push(Span::styled(label, muted));
    }

    (Line::from(spans), spots)
}

#[cfg(test)]
mod tests {
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
            .map(|&(start, width, action)| {
                let (start, width) = (start as usize, width as usize);
                (chars[start..start + width].iter().collect(), action)
            })
            .collect()
    }

    /// Every key of a hint is a spot running its own action and the label runs the first one;
    /// a hint with no key bound is neither drawn nor a spot, and the spots stay on their text.
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
                (" Move".to_string(), NavLeft),
                ("q".to_string(), Close),
                (" Cancel".to_string(), Close),
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
}
