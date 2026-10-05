use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::{
    app::{App, types::FocusTarget},
    configs::{KeywordHighlight, ThemeConfig},
    terminal::{MaxicomCursor, MaxicomTerminal},
    ui::common::apply_keyword_highlights,
};

pub fn render_vt100_terminal(
    app: &mut App,
    frame: &mut Frame,
    area: Rect,
    outer_area: Rect,
    theme: &ThemeConfig,
    highlights: &[KeywordHighlight],
) {
    let view_height = area.height as usize;
    let focus = app.focus_target();
    let is_terminal_focused = focus == FocusTarget::Terminal;
    let cursor = MaxicomCursor::from_config(&app.settings().terminal_mode.cursor);

    if let Some(target_idx) = app.terminal_mode.search_mut().target_jump_index.take() {
        app.terminal_mode
            .scroll_to_target_line(target_idx, view_height);
    }

    let term_widget = MaxicomTerminal::new(app.terminal_mode.vt100_screen())
        .x_offset(app.terminal_mode.pan_offset_x() as u16)
        .y_offset(app.terminal_mode.pan_offset_y() as u16)
        .focused(is_terminal_focused && app.is_app_focused())
        .cursor(cursor)
        .filling_char(&app.settings().terminal_mode.empty_fill_char);

    frame.render_widget(term_widget, area);

    apply_keyword_highlights(frame.buffer_mut(), area, highlights);

    let (grid_rows, cols) = app.terminal_mode.get_vt100_size();
    let max_sb = app.terminal_mode.get_max_scrollback();
    let current_sb = app.terminal_mode.vt100_screen().scrollback();
    let pan_x = app.terminal_mode.pan_offset_x();
    let pan_y = app.terminal_mode.pan_offset_y();
    let top_visible_world_line = if current_sb > 0 {
        max_sb.saturating_sub(current_sb)
    } else {
        max_sb
    };

    if app.terminal_mode.search().active && !app.terminal_mode.search().query_str().is_empty() {
        let query = app.terminal_mode.search().query_str();
        let q_lower = query.to_lowercase();
        let query_char_count = query.chars().count();

        let normal_match_bg = Color::Yellow;
        let active_match_bg = Color::Rgb(255, 140, 0);
        let highlight_fg = Color::Black;
        let active_match_idx = app.terminal_mode.search().current_match;

        for (idx, search_match) in app.terminal_mode.search().matches.iter().enumerate() {
            if search_match.line_idx < top_visible_world_line {
                continue;
            }
            let emulator_row = search_match.line_idx - top_visible_world_line;
            if emulator_row >= grid_rows as usize {
                continue;
            }

            if emulator_row < pan_y {
                continue;
            }
            let viewer_row = emulator_row - pan_y;
            if viewer_row >= area.height as usize {
                continue;
            }

            let row_text = app
                .terminal_mode
                .vt100_screen()
                .rows(0, cols)
                .nth(emulator_row)
                .unwrap_or_default();

            let row_lower = row_text.to_lowercase();

            if let Some((byte_start, _)) = row_lower
                .match_indices(&q_lower)
                .nth(search_match.occurrence_in_line)
            {
                let char_col_start = row_text[..byte_start].chars().count();

                if char_col_start + query_char_count <= pan_x {
                    continue;
                }

                let is_active = idx == active_match_idx;
                let bg_color = if is_active {
                    active_match_bg
                } else {
                    normal_match_bg
                };

                let buf = frame.buffer_mut();

                for c in 0..query_char_count {
                    let world_col = char_col_start + c;
                    if world_col < pan_x {
                        continue;
                    }
                    let viewer_col = world_col - pan_x;
                    if viewer_col >= area.width as usize {
                        break;
                    }

                    let buf_x = area.x + viewer_col as u16;
                    let buf_y = area.y + viewer_row as u16;

                    let cell = &mut buf[(buf_x, buf_y)];
                    cell.set_bg(bg_color);
                    cell.set_fg(highlight_fg);
                }
            }
        }
    }

    // selection
    if app.terminal_mode.copy_state().active {
        let copy_state = app.terminal_mode.copy_state();
        let cursor = copy_state.cursor;
        let buf = frame.buffer_mut();

        // selection area
        if copy_state.selection_anchor.is_some() {
            for r in 0..area.height as usize {
                let emulator_row = r + pan_y;
                if emulator_row >= grid_rows as usize {
                    break;
                }
                let world_row = top_visible_world_line + emulator_row;

                for c in 0..area.width as usize {
                    let world_col = c + pan_x;
                    if copy_state.is_selected(world_row, world_col) {
                        let cell = &mut buf[(area.x + c as u16, area.y + r as u16)];
                        cell.set_style(theme.selection);
                    }
                }
            }
        }

        // selector cursor
        if cursor.row >= top_visible_world_line {
            let emulator_row = cursor.row - top_visible_world_line;
            if emulator_row >= pan_y {
                let viewer_row = emulator_row - pan_y;
                if viewer_row < area.height as usize && cursor.col >= pan_x {
                    let viewer_col = cursor.col - pan_x;
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
    }

    // scrollbar
    let max_vertical = app.terminal_mode.max_vertical_scroll();
    if max_vertical > 0 && view_height > 0 {
        let position_from_top = max_vertical.saturating_sub(app.terminal_mode.scrollback_offset());

        let mut scrollbar_state = ScrollbarState::new(max_vertical)
            .viewport_content_length(view_height)
            .position(position_from_top);

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

    // Cusor
    let screen = app.terminal_mode.vt100_screen();
    let is_scrolled_into_history = screen.scrollback() > 0;

    if !app.terminal_mode.copy_state().active
        && !app.terminal_mode.search().active
        && is_terminal_focused
        && !screen.hide_cursor()
        && !is_scrolled_into_history
    {
        let (c_row, c_col) = screen.cursor_position();

        let local_col = c_col.checked_sub(app.terminal_mode.pan_offset_x() as u16);
        let local_row = c_row.checked_sub(app.terminal_mode.pan_offset_y() as u16);

        if let (Some(col), Some(row)) = (local_col, local_row)
            && col < area.width
            && row < area.height
        {
            let cursor_x = area.x + col;
            let cursor_y = area.y + row;
            if !app.is_app_focused() {
                let buf = frame.buffer_mut();
                buf[(cursor_x, cursor_y)]
                    .set_style(Style::default().add_modifier(Modifier::REVERSED));
            }
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }

    // Search
    if app.terminal_mode.search().active {
        let visual_cursor = app.terminal_mode.search().query.visual_cursor() as u16;

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
}

pub fn render_timestamp_gutter(app: &App, frame: &mut Frame, area: Rect, theme: &ThemeConfig) {
    let screen = app.terminal_mode.vt100_screen();
    let vt_rows = screen.size().0 as usize;
    let pan_y = app.terminal_mode.pan_offset_y();

    let remaining_vt_rows = vt_rows.saturating_sub(pan_y);

    let rows_to_draw = (area.height as usize).min(remaining_vt_rows);
    if rows_to_draw == 0 {
        return;
    }

    let vt_scrollback = screen.scrollback();
    let total_timeline = app.terminal_mode.total_timeline_len();

    let end_idx = total_timeline.saturating_sub(vt_scrollback);
    let start_idx = end_idx.saturating_sub(vt_rows);

    let mut lines = Vec::with_capacity(rows_to_draw);
    let sample_ts = chrono::Local::now();
    let ts_width = sample_ts
        .format(&app.settings().timestamp_format)
        .to_string()
        .chars()
        .count();

    for row in 0..rows_to_draw {
        let source_row = row + pan_y;
        let global_ts_idx = start_idx + source_row;

        let maybe_ts = if global_ts_idx < end_idx {
            app.terminal_mode.get_timeline_timestamp(global_ts_idx)
        } else {
            None
        };

        let row_has_text = app.terminal_mode.row_has_content(source_row as u16);

        if row_has_text && let Some(ts) = maybe_ts {
            let formatted = ts.format(&app.settings().timestamp_format).to_string();
            lines.push(Line::from(vec![
                Span::styled(formatted, theme.timestamp),
                Span::styled("│", theme.timestamp),
            ]));
        } else {
            let spaces = " ".repeat(ts_width);
            lines.push(Line::from(vec![
                Span::raw(spaces),
                Span::styled("│", theme.timestamp_bg),
            ]));
        }
    }

    let gutter = Paragraph::new(lines);
    frame.render_widget(gutter, area);
}
