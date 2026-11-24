use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Clear, Padding, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::App,
    configs::{KeyPattern, ThemeConfig, keybindings::KeyBindingDialogAction},
    overlays::{PopupTarget, QueryView},
    ui::{
        common::popup_area,
        popups::{Hints, cut, hint_spans, popup_block, render_scrollbar},
    },
};

/// A line past this is cut: the popup stays a list, 76 columns at most, as the marks popup
const TEXT_WIDTH: usize = 72;
const EMPTY: &str = "Nothing to show";

/// A query's answer (MAX-165): the query's name as title, one row per line of its value, the
/// selected one reversed, the buttons in the bottom border. As wide as its longest line, the
/// screen's size at most
pub fn render_query_popup(
    app: &App,
    view: &QueryView,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let resolver = &app.key_resolver;
    let dialog_key = |action| resolver.dialog_keys(action).into_iter().next();
    let all = KeyPattern {
        code: KeyCode::Char('a'),
        modifiers: KeyModifiers::NONE,
    };
    let buttons = [
        (
            dialog_key(KeyBindingDialogAction::Confirm),
            PopupTarget::QueryCopy,
            "Copy",
        ),
        (Some(all), PopupTarget::QueryCopyAll, "Copy all"),
        (
            dialog_key(KeyBindingDialogAction::Close),
            PopupTarget::Hint(KeyBindingDialogAction::Close),
            "Close",
        ),
    ];
    let buttons = buttons
        .into_iter()
        .map(|(key, target, label)| (key.map(|key| (key, target)).into_iter().collect(), label));
    let (line, spots) = hint_spans(buttons, theme);
    let hints = Hints {
        line,
        spots: Vec::new(),
    };

    let lines = &view.lines;
    let body = (lines.iter())
        .map(|line| line.width().min(TEXT_WIDTH))
        .max()
        .unwrap_or(EMPTY.width());
    let width = (body as u16).max(hints.line.width() as u16) + 4;
    let area = popup_area(frame.area(), width, lines.len().max(1) as u16 + 3);

    let block = popup_block(view.query, &hints, theme).padding(Padding::new(1, 1, 0, 1));
    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let (x, y) = (area.x + 2, area.bottom().saturating_sub(1));
    for (start, width, target) in spots {
        let button = Rect::new(x.saturating_add(start), y, width, 1).intersection(area);
        if !button.is_empty() {
            targets.push((button, target));
        }
    }

    if lines.is_empty() {
        let muted = Style::default().fg(theme.palette.text_muted);
        frame.render_widget(Paragraph::new(Line::styled(EMPTY, muted)), inner);
        return area;
    }

    // from the row that keeps the selection in view; each row is a click target
    let visible = inner.height as usize;
    let room = (inner.width as usize).min(TEXT_WIDTH);
    let first = (view.selected + 1).saturating_sub(visible);
    let rows: Vec<Line> = (lines.iter().enumerate().skip(first).take(visible))
        .map(|(idx, line)| {
            let mut style = Style::default().fg(theme.palette.text);
            if idx == view.selected {
                style = style.add_modifier(Modifier::REVERSED);
            }
            let row_area = Rect::new(inner.x, inner.y + (idx - first) as u16, inner.width, 1);
            targets.push((row_area, PopupTarget::QueryRow(idx)));
            let text = cut(line, room);
            let pad = (inner.width as usize).saturating_sub(text.width());
            Line::styled(format!("{text}{}", " ".repeat(pad)), style)
        })
        .collect();
    frame.render_widget(Paragraph::new(rows), inner);
    let max_scroll = lines.len().saturating_sub(visible);
    if max_scroll > 0 {
        let bar = Rect {
            x: inner.right(),
            width: 1,
            ..inner
        }
        .intersection(area);
        render_scrollbar(frame, bar, (first, max_scroll, visible), theme);
    }
    area
}
