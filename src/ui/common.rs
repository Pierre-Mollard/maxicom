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
    app::{App, search::SearchModeKind, types::FocusTarget},
    configs::{KeywordHighlight, Mode, ThemeConfig},
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

pub fn monitor_bottom_title<'a>(app: &App, theme: &ThemeConfig) -> Vec<Line<'a>> {
    let mut titles = Vec::with_capacity(2);
    let arrow_style = Style::default()
        .fg(theme.secondary_color)
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
        Style::default().fg(Color::Green)
    } else if !query_str.is_empty() {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let kind_str = match s.mode_kind {
        SearchModeKind::Text => "text",
        SearchModeKind::RawBytes => "byte",
    };

    let spans = vec![
        Span::styled(
            format!(" [{}]", kind_str),
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
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
        Span::styled(
            " [ ←/→, Tab: Mode, Esc: Exit ] ",
            Style::default().fg(theme.text_muted),
        ),
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

pub fn popup_area(
    parent: Rect,
    preferred_width: u16,
    preferred_height: u16,
    min_width: u16,
    min_height: u16,
) -> Rect {
    if parent.width <= min_width || parent.height <= min_height {
        return parent; // fullscreen fallback
    }

    let max_w = parent.width.saturating_sub(2);
    let max_h = parent.height.saturating_sub(2);
    if max_w <= min_width || max_h <= min_height {
        return parent; // fullscreen fallback
    }

    let target_w = preferred_width.clamp(min_width, max_w);
    let target_h = preferred_height.clamp(min_height, max_h);

    let x = parent.x + (parent.width.saturating_sub(target_w)) / 2;
    let y = parent.y + (parent.height.saturating_sub(target_h)) / 2;

    Rect::new(x, y, target_w, target_h)
}

pub fn border_style_for(
    target: FocusTarget,
    current: FocusTarget,
    app_focused: bool,
    theme: &ThemeConfig,
) -> Style {
    if !app_focused {
        *theme.border_app_unfocused
    } else if target == current {
        *theme.border_panel_focused
    } else {
        *theme.border_panel
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
