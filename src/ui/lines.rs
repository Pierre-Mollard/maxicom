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
    configs::{KeywordHighlight, LineDisplayMode, ThemeConfig, WrapMode},
    types::{FocusTarget, ViewportSize},
    ui::{
        PostRenderEffect,
        common::{apply_keyword_highlights, apply_search_highlight, border_style_for},
    },
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
    timestamp_f: &str,
    prefix_str: &str,
    search_query: &str,
    active_occurrence: Option<usize>,
    theme: &ThemeConfig,
) -> Line<'static> {
    let mut prefix_spans = Vec::new();

    if timestamp_in_line {
        let time = entry.timestamp.format(timestamp_f).to_string();
        prefix_spans.push(Span::styled(time, *theme.timestamp_line));
    }

    prefix_spans.push(Span::styled(prefix_str.to_string(), theme.line_prefix));

    let content_spans = format_line_content(&entry.text, mode);

    if search_query.is_empty() {
        prefix_spans.extend(content_spans);
        return Line::from(prefix_spans);
    }

    let highlighted_spans = apply_search_highlight(content_spans, search_query, active_occurrence);
    prefix_spans.extend(highlighted_spans);
    Line::from(prefix_spans)
}

pub fn render_monitor(
    app: &mut App,
    frame: &mut Frame,
    area: Rect,
    outer_area: Rect,
    theme: &ThemeConfig,
    highlights: &[KeywordHighlight],
) -> PostRenderEffect {
    let timestamp_in_line = app.settings().display_live_timestamp;
    let content_width = area.width as usize;
    let content_height = area.height as usize;
    let wrap_mode = app.settings().line_mode.wrap_mode;
    let display_mode = app.settings().line_mode.display_mode;
    let mut post_effect = PostRenderEffect::None;

    app.update_viewport(ViewportSize::new(area.width, area.height));

    let is_search_active = app.line_mode.search().active;
    let search_query = if is_search_active {
        app.line_mode.search().query_str()
    } else {
        ""
    };

    let mut history_lines: Vec<Line> = if is_search_active {
        app.line_mode
            .history()
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let active_occ = app.line_mode.search().active_occurrence_for_line(idx);
                history_entry_to_line(
                    entry,
                    timestamp_in_line,
                    display_mode,
                    &app.settings().timestamp_format,
                    &app.settings().line_mode.line_prefix,
                    search_query,
                    active_occ,
                    theme,
                )
            })
            .collect()
    } else {
        app.line_mode
            .history()
            .iter()
            .map(|entry| {
                history_entry_to_line(
                    entry,
                    timestamp_in_line,
                    display_mode,
                    &app.settings().timestamp_format,
                    &app.settings().line_mode.line_prefix,
                    "",
                    None,
                    theme,
                )
            })
            .collect()
    };

    if app.settings().line_mode.display_partial_rx && !app.line_mode.partial_rx().is_empty() {
        let display_mode = app.settings().line_mode.display_mode;
        let partial_raw = String::from_utf8_lossy(app.line_mode.partial_rx());

        let mut partial_spans = Vec::new();

        if app.settings().display_live_timestamp
            && let Some(partial_timestamp) = app.line_mode.partial_line_timestamp()
        {
            let time = partial_timestamp
                .format(&app.settings().timestamp_format)
                .to_string();
            partial_spans.push(Span::styled(time, theme.timestamp_line));
        }

        let partial_prefix = app.settings().line_mode.line_partial_prefix.to_string();
        let partial_prefix_style = *theme.line_partial_prefix;
        partial_spans.push(Span::styled(partial_prefix, partial_prefix_style));
        partial_spans.extend(format_line_content(&partial_raw, display_mode));
        history_lines.push(Line::from(partial_spans));
    }

    let history_line_count = history_lines.len();
    let mut history_paragraph = Paragraph::new(&history_lines[..]);

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

    // PERF: Visual row calculation for word-wrapped text when search-jump
    if let Some(target_idx) = app.line_mode.search_mut().target_jump_index.take()
        && target_idx < history_lines.len()
    {
        let target_visual_row: usize = match wrap_mode {
            WrapMode::Truncate => target_idx,
            WrapMode::Wrap => {
                let w = (content_width as u16).max(1);
                history_lines
                    .iter()
                    .take(target_idx)
                    .map(|line| {
                        Paragraph::new(vec![line.clone()])
                            .wrap(Wrap { trim: false })
                            .line_count(w)
                            .max(1)
                    })
                    .sum()
            }
        };

        let rows_from_bottom = total_visual_rows
            .saturating_sub(1)
            .saturating_sub(target_visual_row);
        let half_screen = content_height / 2;
        let centered_scroll = rows_from_bottom.saturating_sub(half_screen);

        app.line_mode.scroll_to(centered_scroll.min(max_scroll));
    }

    let scroll_from_bottom = app.line_mode.scroll_offset();
    let paragraph_y_offset = max_scroll.saturating_sub(scroll_from_bottom);
    let line_x_offset = match wrap_mode {
        WrapMode::Wrap => 0,
        WrapMode::Truncate => app.line_mode.x_offset(),
    };

    history_paragraph = history_paragraph.scroll((
        paragraph_y_offset.min(u16::MAX as usize) as u16,
        line_x_offset.min(u16::MAX as usize) as u16,
    ));
    frame.render_widget(history_paragraph, area);

    apply_keyword_highlights(frame.buffer_mut(), area, highlights);

    // selection
    if app.line_mode.copy_state().active {
        let copy_state = app.line_mode.copy_state_mut();
        let cursor = copy_state.cursor;
        let buf = frame.buffer_mut();

        // yank request
        if copy_state.pending_yank {
            let yanked_text = copy_state.extract_selected_from_buffer(
                buf,
                area,
                paragraph_y_offset,
                line_x_offset,
            );

            if let Some(text) = yanked_text {
                post_effect = PostRenderEffect::CopyToClipboard(text);
                copy_state.exit();
            }
        }

        // selection area
        if copy_state.selection_anchor.is_some() {
            for r in 0..area.height as usize {
                let visual_row = paragraph_y_offset + r;
                if visual_row >= total_visual_rows {
                    break;
                }

                for c in 0..area.width as usize {
                    let visual_col = line_x_offset + c;
                    if copy_state.is_selected(visual_row, visual_col) {
                        let cell = &mut buf[(area.x + c as u16, area.y + r as u16)];
                        cell.set_style(theme.selection);
                    }
                }
            }
        }

        // selector cursor
        if cursor.row >= paragraph_y_offset {
            let viewer_row = cursor.row - paragraph_y_offset;
            if viewer_row < area.height as usize && cursor.col >= line_x_offset {
                let viewer_col = cursor.col - line_x_offset;
                if viewer_col < area.width as usize {
                    let screen_x = area.x + viewer_col as u16;
                    let screen_y = area.y + viewer_row as u16;

                    if !app.is_app_focused() {
                        buf[(screen_x, screen_y)]
                            .set_style(Style::default().add_modifier(Modifier::REVERSED));
                    }
                    frame.set_cursor_position((screen_x, screen_y));
                }
            }
        }
    }

    // scrollbar
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

    // search
    if app.line_mode.search().active {
        let visual_cursor = app.line_mode.search().query.visual_cursor() as u16;

        // outer_area.x + 1 (left border) + " SEARCH: " (9) + 1 (space) + visual_cursor
        let cursor_x = outer_area.x + 1 + 9 + 1 + visual_cursor;
        // Bottom border line of the monitor outer box
        let cursor_y = outer_area.y + outer_area.height.saturating_sub(1);

        if !app.is_app_focused() {
            let buf = frame.buffer_mut();
            buf[(cursor_x, cursor_y)].set_style(Style::default().add_modifier(Modifier::REVERSED));
        }
        frame.set_cursor_position((cursor_x, cursor_y));
    }

    post_effect
}

pub fn render_input_prompt(app: &App, frame: &mut Frame, area: Rect, theme: &ThemeConfig) {
    let input = app.line_mode.input();
    let prompt_prefix = "> ";
    let prefix_width = prompt_prefix.len() as u16;
    let inner_width = (area.width.saturating_sub(2 + prefix_width)) as usize;

    let scroll = input.visual_scroll(inner_width);

    let input_text = Line::from(vec![
        Span::styled(
            prompt_prefix,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(input.value()),
    ]);

    let border_style = border_style_for(
        app.focus_target(),
        FocusTarget::LineInput,
        app.is_app_focused(),
        theme,
    );

    let mut block = Block::default();
    if app.settings().ui.show_borders {
        block = block
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(" Transmit (TX) ");
    } else {
        block = block.borders(Borders::NONE);
    }

    let input_widget = Paragraph::new(input_text)
        .scroll((0, scroll as u16))
        .block(block);

    frame.render_widget(input_widget, area);

    if !app.line_mode.search().active && !app.line_mode.copy_state().active {
        let cursor_x = if app.settings().ui.show_borders {
            area.x + 1 + prefix_width + (input.visual_cursor().saturating_sub(scroll)) as u16
        } else {
            area.x + prefix_width + (input.visual_cursor().saturating_sub(scroll)) as u16
        };
        let cursor_y = if app.settings().ui.show_borders {
            area.y + 1
        } else {
            area.y
        };

        if !app.is_app_focused() {
            let buf = frame.buffer_mut();
            buf[(cursor_x, cursor_y)].set_style(Style::default().add_modifier(Modifier::REVERSED));
        }
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}
