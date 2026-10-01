use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use std::fmt::Write;

use crate::{
    app::App,
    configs::{InputMode, ThemeConfig},
    types::FocusTarget,
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

    match app.settings().input_mode {
        InputMode::Terminal => {
            if let Some(pan_left_possible) = app.terminal_mode.can_pan_left() {
                titles.push(
                    Line::from(vec![
                        Span::styled(" ◀ (", arrow_style),
                        Span::styled(pan_left_possible.to_string(), arrow_style),
                        Span::styled(")", arrow_style),
                    ])
                    .left_aligned(),
                );
            }
            if let Some(pan_right_possible) = app.terminal_mode.can_pan_right() {
                titles.push(
                    Line::from(vec![
                        Span::styled("(", arrow_style),
                        Span::styled(pan_right_possible.to_string(), arrow_style),
                        Span::styled(") ▶ ", arrow_style),
                    ])
                    .right_aligned(),
                );
            }

            if app.terminal_mode.search().active {
                let match_count = app.terminal_mode.search().matches_count();
                let current_pos = app.terminal_mode.search().current_match_display();

                titles.push(Line::from(vec![
                    Span::styled(
                        " SEARCH: ",
                        Style::default().bg(Color::Blue).fg(Color::White),
                    ),
                    Span::styled(
                        format!(" {} ", app.terminal_mode.search().query),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::styled(
                        format!("({}/{}) [ <-/->, Esc: Exit ] ", current_pos, match_count),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            }
        }
        InputMode::Line => {
            if let Some(pan_left_possible) = app.line_mode.can_pan_left() {
                titles.push(
                    Line::from(vec![
                        Span::styled(" ◀ (", arrow_style),
                        Span::styled(pan_left_possible.to_string(), arrow_style),
                        Span::styled(")", arrow_style),
                    ])
                    .left_aligned(),
                );
            }
            if let Some((pan_right_possible, pan_explored)) = app.line_mode.can_pan_right()
                && pan_right_possible > 0
            {
                let to_discover_alert =
                    if pan_right_possible > pan_explored.saturating_sub(app.line_mode.x_offset()) {
                        "!"
                    } else {
                        ""
                    };
                titles.push(
                    Line::from(vec![
                        Span::styled(to_discover_alert, arrow_style),
                        Span::styled("(", arrow_style),
                        Span::styled(pan_right_possible.to_string(), arrow_style),
                        Span::styled(") ▶ ", arrow_style),
                    ])
                    .right_aligned(),
                );
            }

            if app.line_mode.search().active {
                let match_count = app.line_mode.search().matches_count();
                let current_pos = app.line_mode.search().current_match_display();

                titles.push(Line::from(vec![
                    Span::styled(
                        " SEARCH: ",
                        Style::default().bg(Color::Blue).fg(Color::White),
                    ),
                    Span::styled(
                        format!(" {} ", app.line_mode.search().query),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::styled(
                        format!("({}/{}) [ <-/->, Esc: Exit ] ", current_pos, match_count),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            }
        }
    }
    titles
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
    let base_color = theme.border;

    if !app_focused {
        Style::default().fg(Color::DarkGray)
    } else if target == current {
        Style::default().fg(base_color).add_modifier(Modifier::BOLD)
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
