use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Padding, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::App,
    commands,
    configs::{
        KeyBindingAction, KeyPattern, LineDisplayMode, ThemeConfig,
        keybindings::KeyBindingDialogAction,
    },
    overlays::{MarkBrowser, PopupTarget},
    ui::{
        common::popup_area,
        data_display::format_line_content,
        popups::{Hints, hint_spans, popup_block},
    },
};

/// A row's text past this is cut: the popup stays a list, not a second history
const TEXT_WIDTH: usize = 60;
/// What a mark without a time shows in its place
const NO_TIME: &str = "--:--:--";

/// The marks popup (MAX-164): one row per mark, its letter, its line's time and text, the
/// selected one reversed, the buttons in the bottom border. As wide as its longest row, the
/// screen's size at most
pub fn render_marks_popup(
    app: &App,
    browser: &MarkBrowser,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let resolver = &app.key_resolver;
    let dialog_key = |action| resolver.dialog_keys(action).into_iter().next();
    let delete = KeyPattern {
        code: KeyCode::Delete,
        modifiers: KeyModifiers::NONE,
    };
    let buttons = [
        (
            dialog_key(KeyBindingDialogAction::Confirm),
            PopupTarget::MarkJump,
            "Jump",
        ),
        (Some(delete), PopupTarget::MarkDelete, "Remove"),
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

    let empty = empty_text(app);
    // each row's text as Pretty shows it: in its colors, the other escape sequences left out
    let texts: Vec<Vec<Span>> = (browser.rows.iter())
        .map(|row| format_line_content(&row.text, LineDisplayMode::Pretty, theme))
        .collect();
    let text_width = |spans: &[Span]| spans.iter().map(Span::width).sum::<usize>();
    let row_width = |text: usize| 1 + 2 + NO_TIME.len() + 2 + text.min(TEXT_WIDTH);
    let body = (texts.iter())
        .map(|spans| row_width(text_width(spans)))
        .max()
        .unwrap_or(empty.width());
    let width = (body as u16).max(hints.line.width() as u16);
    let rows = browser.rows.len().max(1) as u16;
    // the borders and the padding around, none on top
    let area = popup_area(frame.area(), width + 4, rows + 3);

    let block = popup_block("Marks", &hints, theme).padding(Padding::new(1, 1, 0, 1));
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

    let muted = Style::default().fg(theme.palette.text_muted);
    if browser.rows.is_empty() {
        frame.render_widget(Paragraph::new(Span::styled(empty, muted)), inner);
        return area;
    }

    // from the row that keeps the selection in view
    let height = inner.height as usize;
    let first = (browser.selected + 1).saturating_sub(height);
    let text_room = (inner.width as usize).saturating_sub(row_width(0));
    let lines: Vec<Line> = (browser.rows.iter().zip(texts).enumerate())
        .skip(first)
        .take(height)
        .map(|(idx, (row, text))| {
            let mut base = Style::default().fg(theme.palette.text);
            if idx == browser.selected {
                base = base.add_modifier(Modifier::REVERSED);
            }
            let row_area = Rect::new(inner.x, inner.y + (idx - first) as u16, inner.width, 1);
            targets.push((row_area, PopupTarget::MarkRow(idx)));
            let time = row.time.map_or(NO_TIME.to_string(), |time| {
                time.format("%H:%M:%S").to_string()
            });
            // the line's own colors over the popup's, the selection reversed over them
            let text = cut_spans(text, text_room.min(TEXT_WIDTH));
            let pad = (inner.width as usize).saturating_sub(row_width(0) + text_width(&text));
            let letter_style = base
                .fg(theme.palette.secondary)
                .add_modifier(Modifier::BOLD);
            let mut spans = vec![
                Span::styled(row.letter.to_string(), letter_style),
                Span::styled("  ", base),
                Span::styled(time, base.fg(theme.palette.text_muted)),
                Span::styled("  ", base),
            ];
            spans.extend(
                text.into_iter()
                    .map(|span| Span::styled(span.content, base.patch(span.style))),
            );
            spans.push(Span::styled(" ".repeat(pad), base));
            Line::from(spans)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
    area
}

/// `spans` cut to `width` columns, `…` in the last one when they're cut
fn cut_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    if spans.iter().map(Span::width).sum::<usize>() <= width {
        return spans;
    }
    let mut room = width.saturating_sub(1);
    let mut cut = Vec::new();
    for span in spans {
        if room == 0 {
            break;
        }
        let mut text = String::new();
        for ch in span.content.chars() {
            let w = ch.to_string().width();
            if w > room {
                room = 0;
                break;
            }
            room -= w;
            text.push(ch);
        }
        cut.push(Span::styled(text, span.style));
    }
    if width > 0 {
        cut.push(Span::raw("…"));
    }
    cut
}

/// What the popup says with no mark: the key that adds one, as bound
fn empty_text(app: &App) -> String {
    let mark = commands::find_by_name("mark").map(KeyBindingAction::Command);
    let key = mark.and_then(|mark| app.key_resolver.action_keys(mark).into_iter().next());
    match key {
        Some(key) => format!("No mark yet: {key} then a letter marks a line"),
        None => "No mark yet: the mark command marks a line".to_string(),
    }
}
