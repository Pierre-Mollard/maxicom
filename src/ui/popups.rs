use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Padding},
};

use crate::{
    configs::{KeyPattern, KeyResolver, ThemeConfig, keybindings::KeyBindingDialogAction},
    ui::common::popup_area,
};

pub mod alert;
pub mod filepath;
pub mod help;
pub mod notifications;
pub mod reconfigure;
pub mod resize;
pub mod settings;

/// Width of the form popups (settings, serial, logging, resize) and of the alerts
pub const FORM_WIDTH: u16 = 48;

/// The frame every popup shares: a title and the key hints set in by a space, and one row and
/// column of padding around the content
pub fn popup_block<'a>(title: &str, hints: Line<'a>, theme: &ThemeConfig) -> Block<'a> {
    let mut block = Block::bordered()
        .border_style(*theme.popup.border)
        .style(*theme.popup.background)
        .padding(Padding::uniform(1))
        .title(Span::styled(format!(" {title} "), *theme.popup.title));
    if !hints.spans.is_empty() {
        let mut spans = hints.spans;
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

/// Hints like `Enter Apply · Esc Cancel` from the user's `[dialog_actions]` (MAX-73).
/// Each action shows its first key, so hints fit the narrow popups; a hint with no key bound is
/// left out. A text input popup types every key that is not modal (see
/// `KeyPattern::is_modal`), so only modal keys are shown there.
pub fn key_hints<'a>(
    resolver: &KeyResolver,
    is_text_input: bool,
    hints: &[(&[KeyBindingDialogAction], &'a str)],
    theme: &ThemeConfig,
) -> Line<'a> {
    let hints = hints.iter().map(|&(actions, label)| {
        let keys = actions.iter().filter_map(|&action| {
            resolver
                .dialog_keys(action)
                .into_iter()
                .find(|key| !is_text_input || key.is_modal())
        });
        (keys.collect(), label)
    });
    hints_line(hints, theme)
}

/// Each hint's keys then its label, the hints apart by ` · `; a hint with no key is left out
pub fn hints_line<'a>(
    hints: impl IntoIterator<Item = (Vec<KeyPattern>, &'a str)>,
    theme: &ThemeConfig,
) -> Line<'a> {
    let key_style = Style::default()
        .fg(theme.palette.secondary)
        .add_modifier(Modifier::BOLD);
    let muted = Style::default().fg(theme.palette.text_muted);
    let mut spans = Vec::new();

    for (keys, label) in hints {
        if keys.is_empty() {
            continue;
        }
        if !spans.is_empty() {
            spans.push(Span::styled(" · ", muted));
        }
        let keys: Vec<String> = keys.iter().map(|key| key.to_string()).collect();
        spans.push(Span::styled(keys.join("/"), key_style));
        spans.push(Span::styled(format!(" {label}"), muted));
    }

    Line::from(spans)
}
