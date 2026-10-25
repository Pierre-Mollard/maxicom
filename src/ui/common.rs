use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use std::fmt::Write;

use crate::{
    app::{App, search::SearchModeKind, types::FocusTarget},
    configs::{KeyBindingSearchAction, KeywordHighlight, Mode, ThemeConfig},
    ui::popups::hints_line,
};

pub fn monitor_title(app: &App, area: &Rect) -> String {
    let mut title = String::with_capacity(80);

    let (scroll_offset, pan_offset) = match app.settings().mode {
        Mode::Terminal => {
            let screen = app.terminal_mode.vt100_screen();
            if app.terminal_mode.is_new_content_missed() {
                let _ = write!(title, "! ");
            }
            if app.terminal_mode.is_logging() {
                let _ = write!(title, "● REC ");
            }
            let _ = write!(
                title,
                " MaxiCom TERMINAL [VT {}x{} | VIEW {}x{}",
                screen.size().1,
                screen.size().0,
                area.width.saturating_sub(2),
                area.height.saturating_sub(2),
            );
            (
                app.terminal_mode.vt100_screen().scrollback(),
                app.terminal_mode.pan_offset(),
            )
        }
        Mode::Line => {
            if app.line_mode.is_new_content_missed() {
                let _ = write!(title, "! ");
            }
            if app.line_mode.is_logging() {
                let _ = write!(title, "● REC ");
            }
            let _ = write!(
                title,
                " MaxiCom LINE [VIEW {}x{}",
                area.width.saturating_sub(2),
                area.height.saturating_sub(2),
            );
            (
                app.line_mode.scroll_status().offset_from_bottom,
                app.line_mode.x_offset(),
            )
        }
    };

    // Append scroll state
    if scroll_offset > 0 {
        let _ = write!(title, " | SCROLL +{scroll_offset}");
    } else {
        title.push_str(" | LIVE");
    }

    // Append pan state
    if pan_offset > 0 {
        let _ = write!(title, " | PAN +{pan_offset}");
    }

    title.push(']');

    if app.is_prefix_active() {
        title.push_str(" | PREFIX");
    }

    title.push(' ');
    title
}

pub fn monitor_bottom_title<'a>(app: &App, theme: &ThemeConfig) -> Vec<Line<'a>> {
    let mut titles = Vec::with_capacity(2);
    let arrow_style = Style::default()
        .fg(theme.palette.secondary)
        .add_modifier(Modifier::BOLD);

    if let Some(left) = pan_left_indicator(app) {
        titles
            .push(Line::from(vec![Span::raw(" "), Span::styled(left, arrow_style)]).left_aligned());
    }

    if let Some(right) = pan_right_indicator(app) {
        titles.push(
            Line::from(vec![Span::styled(right, arrow_style), Span::raw(" ")]).right_aligned(),
        );
    }

    titles
}

pub fn pan_left_indicator(app: &App) -> Option<String> {
    app.pan_left_status()
        .filter(|&cols| cols > 0)
        .map(|cols| format!("◀ ({})", cols))
}

pub fn pan_right_indicator(app: &App) -> Option<String> {
    app.pan_right_status().and_then(|(cols, explored)| {
        if cols == 0 {
            return None;
        }

        let alert = if app.settings().mode == crate::configs::Mode::Line
            && cols > explored.saturating_sub(app.line_mode.x_offset())
        {
            "!"
        } else {
            ""
        };

        Some(format!("{}({}) ▶", alert, cols))
    })
}

pub fn render_search_bar(app: &App, frame: &mut Frame, area: Rect, theme: &ThemeConfig) {
    let has_borders = app.settings().ui.show_borders;
    let is_terminal = app.settings().mode == Mode::Terminal;

    let s = if is_terminal {
        app.terminal_mode.search()
    } else {
        app.line_mode.search()
    };

    let query_str = s.query_str();
    let visual_cursor = s.query.visual_cursor() as u16;
    let match_count = s.matches_count();
    let current_pos = s.current_match_display();

    let stats_style = if match_count > 0 {
        Style::default().fg(theme.palette.text_good)
    } else if !query_str.is_empty() {
        Style::default().fg(theme.palette.text_warn)
    } else {
        Style::default().fg(theme.palette.text_muted)
    };

    let kind_str = match s.mode_kind {
        SearchModeKind::Text => "text",
        SearchModeKind::RawBytes => "byte",
    };

    let mut spans = vec![
        Span::styled(format!(" [{}]", kind_str), *theme.search.search_indicator),
        Span::styled(" SEARCH: ", *theme.search.search_indicator),
        Span::styled(
            format!(" {} ", query_str),
            Style::default()
                .fg(theme.palette.secondary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("({}/{})", current_pos, match_count), stats_style),
    ];

    // one key per action, like the popups; the hints that don't fit whole are left out, the
    // least needed ones first
    let first_key = |action| app.key_resolver.search_keys(action).into_iter().take(1);
    let hints = [
        (
            first_key(KeyBindingSearchAction::Next)
                .chain(first_key(KeyBindingSearchAction::Previous))
                .collect(),
            "Match",
        ),
        (first_key(KeyBindingSearchAction::Cancel).collect(), "Exit"),
        (first_key(KeyBindingSearchAction::Confirm).collect(), "Copy"),
        (first_key(KeyBindingSearchAction::Kind).collect(), "Mode"),
    ];
    // the bar starts 2 columns in and keeps a space before the corner with borders
    let room = if has_borders {
        area.width.saturating_sub(4)
    } else {
        area.width
    } as usize;
    let used: usize = spans.iter().map(|span| span.width()).sum::<usize>() + 3;
    let hints = (0..=hints.len())
        .rev()
        .map(|count| hints_line(hints[..count].iter().cloned(), theme))
        .find(|line| line.width() + used <= room)
        .unwrap_or_default();
    if !hints.spans.is_empty() {
        spans.push(Span::raw("  "));
        spans.extend(hints.spans);
        spans.push(Span::raw(" "));
    }
    let line = Line::from(spans);

    let (cursor_x, cursor_y) = if has_borders {
        let border_y = area.y + area.height;
        let start_x = area.x + 2;

        let buf = frame.buffer_mut();
        let mut cur_x = start_x;
        for span in &line.spans {
            buf.set_string(cur_x, border_y, &span.content, span.style);
            cur_x += span.content.chars().count() as u16;
        }

        // start_x + 7 + " SEARCH: " (9) + " " (1) + visual_cursor
        (start_x + 10 + 7 + visual_cursor, border_y)
    } else {
        let bottom_y = area.y + area.height.saturating_sub(1);
        let search_area = Rect {
            x: area.x,
            y: bottom_y,
            width: area.width,
            height: 1,
        };

        frame.render_widget(Clear, search_area);
        frame.render_widget(Paragraph::new(line), search_area);

        // area.x + " SEARCH: " (9) + " " (1) + visual_cursor
        (area.x + 10 + 7 + visual_cursor, bottom_y)
    };

    if !app.is_app_focused() {
        let buf = frame.buffer_mut();
        if cursor_x < buf.area.width && cursor_y < buf.area.height {
            buf[(cursor_x, cursor_y)].set_style(Style::default().add_modifier(Modifier::REVERSED));
        }
    }
    frame.set_cursor_position((cursor_x, cursor_y));
}

/// A centered `width` x `height` popup, cut to the screen when it is smaller
pub fn popup_area(parent: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(parent.width);
    let height = height.min(parent.height);
    let x = parent.x + (parent.width - width) / 2;
    let y = parent.y + (parent.height - height) / 2;
    Rect::new(x, y, width, height)
}

pub fn border_style_for(
    target: FocusTarget,
    current: FocusTarget,
    app_focused: bool,
    theme: &ThemeConfig,
) -> Style {
    if !app_focused {
        *theme.ui.border_app_unfocused
    } else if target == current {
        *theme.ui.border_panel_focused
    } else {
        *theme.ui.border_panel
    }
}

pub fn apply_keyword_highlights(buf: &mut Buffer, area: Rect, highlights: &[KeywordHighlight]) {
    if highlights.is_empty() {
        return;
    }

    for row in area.top()..area.bottom() {
        let mut line_str = String::with_capacity(area.width as usize);
        for col in area.left()..area.right() {
            line_str.push_str(buf[(col, row)].symbol());
        }

        for hl in highlights {
            if hl.pattern.is_empty() {
                continue;
            }

            for (byte_offset, _) in line_str.match_indices(&hl.pattern) {
                let start_char_col = line_str[..byte_offset].chars().count();
                let match_len_chars = hl.pattern.chars().count();

                for char_offset in 0..match_len_chars {
                    let col = area.left() + (start_char_col + char_offset) as u16;
                    if col >= area.right() {
                        break;
                    }

                    let cell = &mut buf[(col, row)];
                    if let Some(fg) = hl.style.fg {
                        cell.set_fg(fg);
                    }
                    if let Some(bg) = hl.style.bg {
                        cell.set_bg(bg);
                    }
                    cell.set_style(cell.style().add_modifier(hl.style.add_modifier));
                }
            }
        }
    }
}

pub fn apply_selection_to_spans(
    spans: Vec<Span<'static>>,
    sel_start: usize,
    sel_end: usize,
    selection_style: Style,
) -> Vec<Span<'static>> {
    if sel_start > sel_end {
        return spans;
    }

    let mut out = Vec::with_capacity(spans.len() + 2);
    let mut char_cursor: usize = 0;

    for span in spans {
        let span_chars: Vec<char> = span.content.chars().collect();
        let span_len = span_chars.len();
        if span_len == 0 {
            out.push(span);
            continue;
        }

        let span_start = char_cursor;
        let span_end = char_cursor + span_len - 1;
        char_cursor += span_len;

        if span_end < sel_start || span_start > sel_end {
            out.push(span);
            continue;
        }

        if span_start >= sel_start && span_end <= sel_end {
            out.push(Span::styled(span.content, selection_style));
            continue;
        }

        let overlap_start = span_start.max(sel_start);
        let overlap_end = span_end.min(sel_end);

        let local_start = overlap_start - span_start;
        let local_end = overlap_end - span_start + 1;

        if local_start > 0 {
            let before: String = span_chars[..local_start].iter().collect();
            out.push(Span::styled(before, span.style));
        }

        if local_start < local_end {
            let selected: String = span_chars[local_start..local_end].iter().collect();
            out.push(Span::styled(selected, selection_style));
        }

        if local_end < span_len {
            let after: String = span_chars[local_end..].iter().collect();
            out.push(Span::styled(after, span.style));
        }
    }

    out
}
