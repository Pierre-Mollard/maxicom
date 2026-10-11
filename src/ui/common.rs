use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use std::fmt::Write;

use crate::{
    app::{App, types::FocusTarget},
    configs::{InputMode, KeywordHighlight, ThemeConfig},
};

pub fn monitor_title(app: &App, area: &Rect) -> String {
    let mut title = String::with_capacity(80);

    let (scroll_offset, pan_offset) = match app.settings().input_mode {
        InputMode::Terminal => {
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
        InputMode::Line => {
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
            (app.line_mode.scroll_offset(), app.line_mode.x_offset())
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

pub fn monitor_bottom_title<'a>(app: &App) -> Vec<Line<'a>> {
    let mut titles = Vec::with_capacity(2);
    let arrow_style = Style::default()
        .fg(Color::Yellow)
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

        let alert = if app.settings().input_mode == crate::configs::InputMode::Line
            && cols > explored.saturating_sub(app.line_mode.x_offset())
        {
            "!"
        } else {
            ""
        };

        Some(format!("{}({}) ▶", alert, cols))
    })
}

pub fn render_search_bar(app: &App, frame: &mut Frame, area: Rect, _theme: &ThemeConfig) {
    let has_borders = app.settings().ui.show_borders;
    let is_terminal = app.settings().input_mode == InputMode::Terminal;

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
        Style::default().fg(Color::Green)
    } else if !query_str.is_empty() {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let spans = vec![
        Span::styled(
            " SEARCH: ",
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {} ", query_str),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("({}/{})", current_pos, match_count), stats_style),
        Span::styled(" [ ←/→, Esc: Exit ] ", Style::default().fg(Color::DarkGray)),
    ];
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

        // start_x + " SEARCH: " (9) + " " (1) + visual_cursor
        (start_x + 10 + visual_cursor, border_y)
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
        (area.x + 10 + visual_cursor, bottom_y)
    };

    if !app.is_app_focused() {
        let buf = frame.buffer_mut();
        if cursor_x < buf.area.width && cursor_y < buf.area.height {
            buf[(cursor_x, cursor_y)].set_style(Style::default().add_modifier(Modifier::REVERSED));
        }
    }
    frame.set_cursor_position((cursor_x, cursor_y));
}

/// Helper function to center a popup window
pub fn centered_rect(percent_x: u16, height: u16, parent: Rect) -> Rect {
    let width = (parent.width * percent_x) / 100;
    let x = parent.x + parent.width.saturating_sub(width) / 2;
    let y = parent.y + parent.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(parent.width), height.min(parent.height))
}

/// Helper function to get standard borders
pub fn border_style_for(
    target: FocusTarget,
    current: FocusTarget,
    app_focused: bool,
    theme: &ThemeConfig,
) -> Style {
    if !app_focused {
        Style::default().fg(Color::DarkGray)
    } else if target == current {
        *theme.border
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

pub fn apply_search_highlight(
    spans: Vec<Span<'static>>,
    query: &str,
    active_occurrence: Option<usize>,
) -> Vec<Span<'static>> {
    if query.is_empty() {
        return spans;
    }

    let query_lower = query.to_lowercase();
    let query_len = query.len();
    let mut out_spans = Vec::new();
    let mut current_occurrence_counter = 0;

    let normal_match_style = Style::default().fg(Color::Black).bg(Color::Yellow);

    let active_match_style = Style::default()
        .fg(Color::Black)
        .bg(Color::Rgb(255, 140, 0))
        .add_modifier(Modifier::BOLD);

    for span in spans {
        let text = span.content.as_ref();
        let text_lower = text.to_lowercase();

        if !text_lower.contains(&query_lower) {
            out_spans.push(span);
            continue;
        }

        let mut last_idx = 0;
        for (match_start, _) in text_lower.match_indices(&query_lower) {
            let match_end = match_start + query_len;

            if match_start > last_idx {
                out_spans.push(Span::styled(
                    text[last_idx..match_start].to_string(),
                    span.style,
                ));
            }

            let style_to_apply = if Some(current_occurrence_counter) == active_occurrence {
                active_match_style
            } else {
                normal_match_style
            };

            out_spans.push(Span::styled(
                text[match_start..match_end].to_string(),
                span.style.patch(style_to_apply),
            ));

            current_occurrence_counter += 1;
            last_idx = match_end;
        }

        if last_idx < text.len() {
            out_spans.push(Span::styled(text[last_idx..].to_string(), span.style));
        }
    }

    out_spans
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
    let mut out = Vec::new();
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

        if span_end < sel_start || span_start > sel_end {
            out.push(span);
        } else if span_start >= sel_start && span_end <= sel_end {
            out.push(Span::styled(span.content, selection_style));
        } else {
            for ch in span_chars {
                if char_cursor >= sel_start && char_cursor <= sel_end {
                    out.push(Span::styled(ch.to_string(), selection_style));
                } else {
                    out.push(Span::styled(ch.to_string(), span.style));
                }
                char_cursor += 1;
            }
            continue;
        }

        char_cursor += span_len;
    }

    out
}

pub fn highlight_raw_byte_range(
    spans: &mut [Span<'static>],
    from_byte: usize,
    to_byte: usize,
    style: Style,
) {
    for byte_idx in from_byte..=to_byte {
        let span_idx = byte_idx * 2;
        if span_idx < spans.len() {
            spans[span_idx].style = style;
        }
        if byte_idx < to_byte {
            let space_idx = span_idx + 1;
            if space_idx < spans.len() {
                spans[space_idx].style = style;
            }
        }
    }
}
