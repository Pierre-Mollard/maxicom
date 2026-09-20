use ansi_to_tui::IntoText;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

use crate::{
    app::App,
    config::{LineDisplayMode, WrapMode},
    types::{FocusTarget, ViewportSize},
    ui::common::border_style_for,
};

use crate::types::LogEntry;

fn trim_trailing_incomplete_ansi(text: &str) -> &str {
    if let Some(esc_idx) = text.rfind('\x1b') {
        let tail = &text[esc_idx..];
        let is_finished = tail
            .chars()
            .skip(1)
            .any(|c| c.is_ascii_alphabetic() || ('@'..='~').contains(&c));
        if !is_finished {
            return &text[..esc_idx];
        }
    }
    text
}

// TODO: highlight search elements and current index
pub fn format_line_content(text: &str, mode: LineDisplayMode) -> Vec<Span<'static>> {
    match mode {
        LineDisplayMode::Pretty => {
            let displayable = trim_trailing_incomplete_ansi(text);
            if let Ok(parsed) = displayable.as_bytes().into_text()
                && let Some(first_line) = parsed.lines.into_iter().next()
            {
                return first_line.spans;
            }
            // Fallback
            vec![Span::raw(displayable.to_string())]
        }

        LineDisplayMode::Clean => {
            let clean_text = strip_ansi_escapes::strip_str(text);
            vec![Span::raw(clean_text)]
        }

        LineDisplayMode::Raw => {
            let mut raw = String::new();
            for ch in text.chars() {
                match ch {
                    '\x1b' => raw.push_str("^["),
                    '\r' => raw.push_str("\\r"),
                    '\t' => raw.push_str("    "),
                    c if c.is_control() => raw.push_str(&format!("\\x{:02X}", c as u32)),
                    c => raw.push(c),
                }
            }
            vec![Span::styled(raw, Style::default().fg(Color::White))]
        }
    }
}

pub fn history_entry_to_line(
    entry: &LogEntry,
    timestamp_in_line: bool,
    mode: LineDisplayMode,
) -> Line<'static> {
    let mut prefix_spans = Vec::new();

    if timestamp_in_line {
        let time_str = entry.formatted_time();
        prefix_spans.push(Span::styled(time_str, Style::default().fg(Color::Yellow)));
    }

    // TODO: custom outside and inside (|)
    prefix_spans.push(Span::styled("| ", Style::default().fg(Color::DarkGray)));

    prefix_spans.extend(format_line_content(&entry.text, mode));

    Line::from(prefix_spans)
}

pub fn render_monitor(app: &mut App, frame: &mut Frame, area: Rect, outer_area: Rect) {
    let timestamp_in_line = app.config().display_live_timestamp;
    let content_width = area.width as usize;
    let content_height = area.height as usize;
    let wrap_mode = app.config().line_mode.wrap_mode;

    app.update_viewport(ViewportSize::new(area.width, area.height));

    let mut history_lines: Vec<Line> = app
        .line_mode
        .history()
        .iter()
        .map(|entry| {
            history_entry_to_line(
                entry,
                timestamp_in_line,
                app.config().line_mode.display_mode,
            )
        })
        .collect();

    if app.config().line_mode.display_partial_rx && !app.line_mode.partial_rx().is_empty() {
        let display_mode = app.config().line_mode.display_mode;
        let partial_raw = String::from_utf8_lossy(app.line_mode.partial_rx());

        let mut partial_spans = Vec::new();

        if app.config().display_live_timestamp
            && let Some(partial_timestamp) = app.line_mode.partial_line_timestamp()
        {
            let time = LogEntry::formatted_time_builder(partial_timestamp);
            partial_spans.push(Span::styled(time, Style::default().fg(Color::Yellow)));
        }

        // Prefix with a subtle indicator or standard pipe
        partial_spans.push(Span::styled("|p ", Style::default().fg(Color::DarkGray)));

        partial_spans.extend(format_line_content(&partial_raw, display_mode));

        history_lines.push(Line::from(partial_spans));
    }

    let line_x_offset = match wrap_mode {
        WrapMode::Wrap => 0,
        WrapMode::Truncate => app.line_mode.x_offset(),
    };
    let history_line_count = history_lines.len();
    let mut history_paragraph = Paragraph::new(history_lines);

    history_paragraph = match wrap_mode {
        WrapMode::Wrap => history_paragraph.wrap(Wrap { trim: false }),
        WrapMode::Truncate => history_paragraph,
    };

    // WARN: Paragraph.line_width and .line_count are experiemental deps
    let max_pan = match wrap_mode {
        WrapMode::Wrap => 0, //unused so no need to process
        WrapMode::Truncate => history_paragraph.line_width().saturating_sub(content_width),
    };
    let total_visual_rows = match wrap_mode {
        WrapMode::Wrap => history_paragraph.line_count(content_width as u16),
        WrapMode::Truncate => history_line_count,
    };

    let max_scroll = total_visual_rows.saturating_sub(content_height);

    app.line_mode.update_limits(max_scroll, max_pan);

    let scroll_from_bottom = app.line_mode.scroll_offset();
    let paragraph_y_offset = max_scroll.saturating_sub(scroll_from_bottom);

    history_paragraph = history_paragraph.scroll((
        paragraph_y_offset.min(u16::MAX as usize) as u16,
        line_x_offset.min(u16::MAX as usize) as u16,
    ));
    frame.render_widget(history_paragraph, area);

    if total_visual_rows > content_height {
        let mut scrollbar_state = ScrollbarState::new(max_scroll)
            .viewport_content_length(content_height)
            .position(paragraph_y_offset);

        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .track_symbol(Some("│"))
            .thumb_symbol("█");

        let scrollbar_area = Rect {
            x: outer_area.x + outer_area.width.saturating_sub(1),
            y: outer_area.y + 1,
            width: 1,
            height: outer_area.height.saturating_sub(2),
        };

        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}

// NOTE: AI generated, make it better
pub fn render_input_prompt(app: &App, frame: &mut Frame, area: Rect) {
    let input_text = Line::from(vec![
        Span::styled(
            "> ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(app.line_mode.input_buffer()),
    ]);

    let border_style = border_style_for(
        app.focus_target(),
        FocusTarget::LineInput,
        app.is_app_focused(),
    );
    let input_widget = Paragraph::new(input_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(" Transmit (TX) "),
    );

    frame.render_widget(input_widget, area);

    // Place physical blinking cursor at the end of what the user is typing
    let cursor_x = area.x + 3 + app.line_mode.input_buffer().len() as u16;
    let cursor_y = area.y + 1;
    frame.set_cursor_position((cursor_x, cursor_y));
}
