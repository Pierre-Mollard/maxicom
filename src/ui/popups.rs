use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Padding, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use unicode_width::UnicodeWidthStr;

use crate::{
    configs::{KeyPattern, KeyResolver, ThemeConfig, keybindings::KeyBindingDialogAction},
    overlays::PopupTarget,
    ui::common::popup_area,
};

pub mod alert;
pub mod command_line;
pub mod directory;
pub mod filepath;
pub mod help;
pub mod marks;
pub mod menu;
pub mod notifications;
pub mod reconfigure;
pub mod resize;
pub mod sessions;
pub mod settings;
pub mod triggers;

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

/// The part of a popup's inner area where content `width` wide sits centered: from its
/// indent to the right edge, so a longer line is cut no sooner than without it
pub fn centered(inner: Rect, width: u16) -> Rect {
    let indent = inner.width.saturating_sub(width) / 2;
    Rect {
        x: inner.x + indent,
        width: inner.width - indent,
        ..inner
    }
}

/// A centered popup as tall as `lines` inside `popup_block`
pub fn form_area(parent: Rect, width: u16, lines: usize) -> Rect {
    popup_area(parent, width, lines as u16 + 4)
}

/// Row `y` of a popup's inner area, as a click target (MAX-113): cut to the area
pub fn row_area(inner: Rect, y: usize) -> Rect {
    Rect::new(inner.x, inner.y + y as u16, inner.width, 1).intersection(inner)
}

/// A popup's scrollbar in the column `bar`, for content scrolled to `position` of
/// `max_scroll` with `visible` rows shown: the help's, the settings', the session browser's
/// (MAX-175)
pub fn render_scrollbar(
    frame: &mut Frame,
    bar: Rect,
    (position, max_scroll, visible): (usize, usize, usize),
    theme: &ThemeConfig,
) {
    let mut state = ScrollbarState::new(max_scroll)
        .viewport_content_length(visible)
        .position(position);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(Some("▲"))
        .end_symbol(Some("▼"))
        .thumb_symbol("█")
        .track_symbol(Some("│"))
        .thumb_style(*theme.ui.scrollbar_thumb)
        .track_style(*theme.ui.scrollbar_track)
        .begin_style(*theme.ui.scrollbar_thumb)
        .end_style(*theme.ui.scrollbar_thumb);
    frame.render_stateful_widget(scrollbar, bar, &mut state);
}

/// `text` cut to `width` columns, `…` in its last one when it's cut
pub fn cut(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    let mut cut = String::new();
    for c in text.chars() {
        let next = cut.width() + c.to_string().width();
        if next + 1 > width {
            break;
        }
        cut.push(c);
    }
    cut.push('…');
    cut
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
    spots: Vec<(u16, u16, PopupTarget)>,
}

impl Hints<'_> {
    /// The hints as click targets, their line drawn from `(x, y)`, cut to `clip`
    pub fn push_targets(
        &self,
        (x, y): (u16, u16),
        clip: Rect,
        targets: &mut Vec<(Rect, PopupTarget)>,
    ) {
        for &(start, width, target) in &self.spots {
            let area = Rect::new(x.saturating_add(start), y, width, 1).intersection(clip);
            if !area.is_empty() {
                targets.push((area, target));
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

/// Hints like `Enter Apply · Esc Cancel` from the user's `[dialog]` (MAX-73).
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
            Some((key, PopupTarget::Hint(action)))
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
        // the label's spot leaves out the space before it, so its hover doesn't start there
        col += 1;
        spot(&mut col, label, first);
        spans.push(Span::styled(format!(" {label}"), muted));
    }

    (Line::from(spans), spots)
}

#[cfg(test)]
#[path = "../tests/ui/popups.rs"]
mod tests;
