use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Padding, Paragraph},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    app::App,
    clock,
    configs::{ThemeConfig, keybindings::KeyBindingDialogAction},
    notification::Notice,
    overlays::{NoticeBrowser, PopupTarget},
    ui::{
        common::popup_area,
        popups::{Hints, cut, hint_spans, popup_block, render_scrollbar},
    },
};

/// A row's text past this is cut, the selected one whole under the list: the popup stays a
/// list, 76 columns at most, as the marks popup
const TEXT_WIDTH: usize = 60;
/// The level's icon, the time and the gaps before a row's text
const ROW_START: usize = 2 + 8 + 2;
/// The most lines kept under the list for the selected text in full
const FULL_LINES: usize = 6;
const EMPTY: &str = "No notification yet";

/// The notifications popup (MAX-276): one row per line of the history, its level's icon in its
/// color, its time and its text, the selected one reversed. A text cut in its row is whole
/// under the list when its row is selected; the room for it is kept whatever the selection, so
/// the popup doesn't jump while moving
pub fn render_notices_popup(
    app: &App,
    browser: &NoticeBrowser,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let resolver = &app.key_resolver;
    let dialog_key = |action| resolver.dialog_keys(action).into_iter().next();
    let buttons = [
        (
            dialog_key(KeyBindingDialogAction::Confirm),
            PopupTarget::NoticeCopy,
            "Copy",
        ),
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

    let notices = &browser.notices;
    let body = (notices.iter())
        .map(|notice| ROW_START + text(notice).width().min(TEXT_WIDTH))
        .max()
        .unwrap_or(EMPTY.width());
    let width = (body as u16).max(hints.line.width() as u16) + 4;
    // the borders and the padding around, none on top
    let inner_width = width.min(frame.area().width).saturating_sub(4) as usize;
    let text_room = inner_width.saturating_sub(ROW_START).min(TEXT_WIDTH);
    let full_lines = |notice: &Notice| {
        let text = text(notice);
        if text.width() <= text_room {
            return Vec::new();
        }
        let mut lines = wrap(&text, inner_width);
        lines.truncate(FULL_LINES);
        lines
    };
    let below = notices
        .iter()
        .map(|n| full_lines(n).len())
        .max()
        .unwrap_or(0);
    // a blank row between the list and the text
    let below = if below > 0 { below + 1 } else { 0 };
    let rows = notices.len().max(1) + below;
    let area = popup_area(frame.area(), width, rows as u16 + 3);

    let block = popup_block("Notifications", &hints, theme).padding(Padding::new(1, 1, 0, 1));
    frame.render_widget(Clear, area);
    let mut inner = block.inner(area);
    frame.render_widget(block, area);
    let (x, y) = (area.x + 2, area.bottom().saturating_sub(1));
    for (start, width, target) in spots {
        let button = Rect::new(x.saturating_add(start), y, width, 1).intersection(area);
        if !button.is_empty() {
            targets.push((button, target));
        }
    }

    let muted = Style::default().fg(theme.palette.text_muted);
    if notices.is_empty() {
        frame.render_widget(Paragraph::new(Span::styled(EMPTY, muted)), inner);
        return area;
    }

    // the selected text in full at the bottom, the list above it
    let below = (below as u16).min(inner.height.saturating_sub(1));
    if below > 1
        && let Some(notice) = notices.get(browser.selected)
    {
        let full = Rect {
            y: inner.bottom() - below + 1,
            height: below - 1,
            ..inner
        };
        let lines: Vec<Line> = (full_lines(notice).into_iter())
            .map(|line| Line::styled(line, Style::default().fg(theme.palette.text)))
            .collect();
        frame.render_widget(Paragraph::new(lines), full);
    }
    inner.height -= below;

    // from the row that keeps the selection in view; each row is a click target
    let visible = inner.height as usize;
    let first = (browser.selected + 1).saturating_sub(visible);
    let lines: Vec<Line> = (notices.iter().enumerate().skip(first).take(visible))
        .map(|(idx, notice)| {
            let mut base = Style::default().fg(theme.palette.text);
            if idx == browser.selected {
                base = base.add_modifier(Modifier::REVERSED);
            }
            let row_area = Rect::new(inner.x, inner.y + (idx - first) as u16, inner.width, 1);
            targets.push((row_area, PopupTarget::NoticeRow(idx)));
            let text = cut(&text(notice), text_room);
            let pad = inner_width.saturating_sub(ROW_START + text.width());
            Line::from(vec![
                Span::styled(notice.level.icon(), base.fg(notice.level.color())),
                Span::styled(clock::format(notice.time, "%H:%M:%S"), muted.patch(base)),
                Span::styled("  ", base),
                Span::styled(text, base),
                Span::styled(" ".repeat(pad), base),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
    let max_scroll = notices.len().saturating_sub(visible);
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

/// A notice's text on one line
fn text(notice: &Notice) -> String {
    notice.message.replace(['\r', '\n'], " ")
}

/// `text` in lines of `width` columns at most, cut anywhere: a path has no spaces to wrap at
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut room = width;
    for ch in text.chars() {
        let w = ch.width().unwrap_or(0);
        if w > room && !lines.last().is_some_and(String::is_empty) {
            lines.push(String::new());
            room = width;
        }
        room = room.saturating_sub(w);
        lines.last_mut().expect("never empty").push(ch);
    }
    lines
}

#[cfg(test)]
#[path = "../../tests/ui/popups/notices.rs"]
mod tests;
