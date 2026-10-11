// TODO: search is not centering in main view
// TODO: not auto go to new sub line in copy mode

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::{
    app::{App, types::FocusTarget},
    configs::{
        KeywordHighlight, ThemeConfig,
        settings::{RawSideOrientation, RawSideView, raw_view_to_display_mode},
    },
    terminal::{MaxicomCursor, MaxicomTerminal},
    ui::{
        common::{apply_keyword_highlights, highlight_raw_byte_range, render_search_bar},
        data_display::format_line_content,
    },
};

pub fn render_vt100_terminal(
    app: &mut App,
    frame: &mut Frame,
    content_area: Rect,
    raw_side_area: Option<Rect>,
    scrollbar_area: Option<Rect>,
    theme: &ThemeConfig,
    highlights: &[KeywordHighlight],
) {
    let view_height = content_area.height as usize;
    let focus = app.focus_target();
    let is_terminal_focused = focus == FocusTarget::Terminal;
    let cursor = MaxicomCursor::from_config(&app.settings().terminal_mode.cursor);
    let raw_view = app.settings().raw_side.view;

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

    frame.render_widget(term_widget, content_area);
    apply_keyword_highlights(frame.buffer_mut(), content_area, highlights);

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

    let is_search_active = app.terminal_mode.search().active;

    // raw panel
    let active_focus = if app.terminal_mode.copy_state().active {
        Some((
            app.terminal_mode.copy_state().cursor.line_idx,
            app.terminal_mode.copy_state().cursor.char_idx,
        ))
    } else if is_search_active {
        app.terminal_mode.search().current_match_position()
    } else {
        None
    };

    if let Some((target_row, target_col)) = active_focus
        && target_row >= top_visible_world_line
    {
        let emulator_row = target_row - top_visible_world_line;
        if emulator_row < grid_rows as usize {
            let row_text = app
                .terminal_mode
                .vt100_screen()
                .rows(0, cols)
                .nth(emulator_row)
                .unwrap_or_default();
            let trimmed = row_text.trim_end();

            let local_char = target_col.min(trimmed.chars().count().saturating_sub(1));
            let byte_offset = trimmed
                .char_indices()
                .nth(local_char)
                .map(|(b, _)| b)
                .unwrap_or(trimmed.len());

            if let Some(raw_area) = raw_side_area {
                let usable_width = (raw_area.width as usize).saturating_sub(1);
                if is_search_active {
                    app.terminal_mode.center_raw_pan_on_byte(
                        raw_view,
                        usable_width,
                        byte_offset,
                        trimmed.len(),
                    );
                } else {
                    app.terminal_mode.sync_raw_pan_with_local_col(
                        raw_view,
                        usable_width,
                        byte_offset,
                        trimmed.len(),
                    );
                }
            }
        }
    }

    if let Some(raw_area) = raw_side_area {
        render_vt100_raw_side_panel(app, frame, raw_area, theme);
    }

    // search
    if is_search_active && !app.terminal_mode.search().query_str().is_empty() {
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
            if emulator_row >= grid_rows as usize || emulator_row < pan_y {
                continue;
            }
            let viewer_row = emulator_row - pan_y;
            if viewer_row >= content_area.height as usize {
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
                    if viewer_col >= content_area.width as usize {
                        break;
                    }

                    let buf_x = content_area.x + viewer_col as u16;
                    let buf_y = content_area.y + viewer_row as u16;

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

        // selection highlight
        if copy_state.selection_anchor.is_some() {
            let (start_pos, end_pos) = copy_state.normalized_selection().unwrap();

            for r in 0..content_area.height as usize {
                let emulator_row = r + pan_y;
                if emulator_row >= grid_rows as usize {
                    break;
                }
                let world_row = top_visible_world_line + emulator_row;

                if world_row < start_pos.line_idx || world_row > end_pos.line_idx {
                    continue;
                }
                let row_text = app
                    .terminal_mode
                    .vt100_screen()
                    .rows(0, cols)
                    .nth(emulator_row)
                    .unwrap_or_default();
                let trimmed_len = row_text.trim_end().chars().count();
                for c in 0..content_area.width as usize {
                    let world_col = c + pan_x;
                    if copy_state.is_selected(world_row, world_col) && world_col < trimmed_len {
                        let cell = &mut buf[(content_area.x + c as u16, content_area.y + r as u16)];
                        cell.set_style(theme.selection);
                    }
                }
            }
        }

        // selection cursor
        if cursor.line_idx >= top_visible_world_line {
            let emulator_row = cursor.line_idx - top_visible_world_line;
            if emulator_row >= pan_y {
                let viewer_row = emulator_row - pan_y;
                if viewer_row < content_area.height as usize && cursor.char_idx >= pan_x {
                    let viewer_col = cursor.char_idx - pan_x;
                    if viewer_col < content_area.width as usize {
                        let screen_x = content_area.x + viewer_col as u16;
                        let screen_y = content_area.y + viewer_row as u16;

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
    if let Some(sb_area) = scrollbar_area {
        let max_vertical = app.terminal_mode.max_vertical_scroll();
        let has_borders = app.settings().ui.show_borders;

        if max_vertical > 0 && view_height > 0 {
            let position_from_top =
                max_vertical.saturating_sub(app.terminal_mode.scrollback_offset());

            let mut scrollbar_state = ScrollbarState::new(max_vertical)
                .viewport_content_length(view_height)
                .position(position_from_top);

            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("▲"))
                .end_symbol(Some("▼"))
                .track_symbol(if has_borders { Some("│") } else { Some(" ") })
                .thumb_symbol("█");

            frame.render_stateful_widget(scrollbar, sb_area, &mut scrollbar_state);
        } else if !has_borders {
            let blank = Paragraph::new(" ");
            frame.render_widget(blank, sb_area);
        }
    }

    // emulation cursor
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
            && col < content_area.width
            && row < content_area.height
        {
            let cursor_x = content_area.x + col;
            let cursor_y = content_area.y + row;
            if !app.is_app_focused() {
                let buf = frame.buffer_mut();
                buf[(cursor_x, cursor_y)]
                    .set_style(Style::default().add_modifier(Modifier::REVERSED));
            }
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }

    // search input
    if app.terminal_mode.search().active {
        render_search_bar(app, frame, content_area, theme);
    }

    // flash for screenshot
    if app.terminal_mode.is_flashing() {
        let buf = frame.buffer_mut();
        for y in content_area.top()..content_area.bottom() {
            for x in content_area.left()..content_area.right() {
                let cell = &mut buf[(x, y)];
                cell.set_bg(Color::White);
                cell.set_fg(Color::DarkGray);
            }
        }
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

// NOTE: review this
pub fn render_vt100_raw_side_panel(
    app: &App,
    frame: &mut Frame,
    raw_area: Rect,
    theme: &ThemeConfig,
) {
    let raw_view = app.settings().raw_side.view;
    if raw_view == RawSideView::Off || raw_area.width == 0 || raw_area.height == 0 {
        return;
    }

    let display_mode = raw_view_to_display_mode(raw_view);

    let borders = match app.settings().raw_side.orientation {
        RawSideOrientation::Right => Borders::LEFT,
        RawSideOrientation::Bottom => Borders::TOP,
    };

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));

    let inner_area = block.inner(raw_area);
    frame.render_widget(block, raw_area);

    let screen = app.terminal_mode.vt100_screen();
    let (grid_rows, cols) = screen.size();
    let pan_y = app.terminal_mode.pan_offset_y();
    let max_sb = app.terminal_mode.get_max_scrollback();
    let current_sb = screen.scrollback();
    let top_visible_world_line = if current_sb > 0 {
        max_sb.saturating_sub(current_sb)
    } else {
        max_sb
    };

    let copy_state = app.terminal_mode.copy_state();
    let normalized_selection = if copy_state.active {
        copy_state.normalized_selection()
    } else {
        None
    };

    let is_search_active = app.terminal_mode.search().active;
    let search_query = app.terminal_mode.search().query_str();
    let q_lower = search_query.to_lowercase();
    let query_len = search_query.chars().count();
    let active_match_idx = app.terminal_mode.search().current_match;

    let normal_match_style = Style::default().bg(Color::Yellow).fg(Color::Black);
    let active_match_style = Style::default()
        .bg(Color::Rgb(255, 140, 0))
        .fg(Color::Black);

    let rows_to_render =
        (inner_area.height as usize).min((grid_rows as usize).saturating_sub(pan_y));
    let mut raw_lines = Vec::with_capacity(rows_to_render);

    for (r, row_text) in screen
        .rows(0, cols)
        .skip(pan_y)
        .take(rows_to_render)
        .enumerate()
    {
        let emulator_row = r + pan_y;
        let world_row = top_visible_world_line + emulator_row;

        let trimmed_text = row_text.trim_end();
        if trimmed_text.is_empty() {
            raw_lines.push(Line::from(""));
            continue;
        }

        let mut spans = format_line_content(trimmed_text, display_mode);

        // Map character offsets to byte offsets for multi-byte UTF-8 accuracy
        let current_chars = trimmed_text.chars().count();
        let mut char_to_byte_idx = Vec::with_capacity(current_chars + 1);
        for (b_idx, _) in trimmed_text.char_indices() {
            char_to_byte_idx.push(b_idx);
        }
        char_to_byte_idx.push(trimmed_text.len());

        // 1. Search Highlights
        if is_search_active && query_len > 0 {
            let row_lower = trimmed_text.to_lowercase();
            for (occ_idx, (b_idx, _)) in row_lower.match_indices(&q_lower).enumerate() {
                let char_start = trimmed_text[..b_idx].chars().count();
                let char_end = char_start + query_len;

                if char_end <= current_chars {
                    let byte_start = char_to_byte_idx[char_start];
                    let byte_end = char_to_byte_idx[char_end];

                    let is_active = app
                        .terminal_mode
                        .search()
                        .matches
                        .get(active_match_idx)
                        .map_or_else(
                            || false,
                            |m| m.line_idx == world_row && m.occurrence_in_line == occ_idx,
                        );

                    let style = if is_active {
                        active_match_style
                    } else {
                        normal_match_style
                    };

                    highlight_raw_byte_range(&mut spans, byte_start, byte_end - 1, style);
                }
            }
        }

        // 2. Selection Highlights
        if let Some((start_pos, end_pos)) = normalized_selection
            && world_row >= start_pos.line_idx
            && world_row <= end_pos.line_idx
        {
            let from_char = if world_row == start_pos.line_idx {
                start_pos.char_idx.min(current_chars)
            } else {
                0
            };
            let to_char = if world_row == end_pos.line_idx {
                (end_pos.char_idx + 1).min(current_chars)
            } else {
                current_chars
            };

            if from_char < to_char {
                let byte_start = char_to_byte_idx[from_char];
                let byte_end = char_to_byte_idx[to_char];
                highlight_raw_byte_range(&mut spans, byte_start, byte_end - 1, *theme.selection);
            }
        }

        raw_lines.push(Line::from(spans));
    }

    let raw_pan_x = app.terminal_mode.raw_pan_x();
    let raw_paragraph =
        Paragraph::new(raw_lines).scroll((0, raw_pan_x.min(u16::MAX as usize) as u16));

    frame.render_widget(raw_paragraph, inner_area);
}
