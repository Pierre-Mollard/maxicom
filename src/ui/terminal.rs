use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::{
    app::{
        App,
        data_processing::{build_display_to_raw_byte_map, map_disp_byte_to_raw},
        types::FocusTarget,
    },
    configs::{KeywordHighlight, RawSideView, ThemeConfig, settings::raw_view_to_display_mode},
    terminal::{MaxicomCursor, MaxicomTerminal},
    ui::{
        common::{apply_keyword_highlights, render_search_bar},
        raw_side::{
            CharHighlight, format_raw_bytes_with_highlights, raw_panel_layout,
            render_raw_panel_widget,
        },
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
    let cursor = MaxicomCursor::from_config(
        *theme.ui.cursor_unfocused,
        app.settings()
            .terminal_mode
            .unfocused_cursor_glyph
            .to_owned(),
    );
    let raw_view = app.settings().raw_side.view;

    if let Some((target_row, target_col)) = app.terminal_mode.search_mut().target_jump.take() {
        app.terminal_mode.center_on_target_match(
            target_row,
            target_col,
            content_area.width as usize,
            view_height,
        );
    }

    let term_widget = MaxicomTerminal::new(app.terminal_mode.vt100_screen())
        .x_offset(app.terminal_mode.pan_offset_x() as u16)
        .y_offset(app.terminal_mode.pan_offset_y() as u16)
        .focused(is_terminal_focused && app.is_app_focused())
        .cursor(cursor)
        .style(*theme.ui.background)
        .filling_char(&app.settings().terminal_mode.empty_fill_char)
        .filling_char_style(*theme.terminal.term_outside);

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

    let is_search_active =
        app.terminal_mode.search().active && !app.terminal_mode.search().matches.is_empty();

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
            let row_raw_bytes = app
                .terminal_mode
                .vt100_screen()
                .rows_formatted(0, cols)
                .nth(emulator_row)
                .unwrap_or_default();

            let plain_text = app
                .terminal_mode
                .vt100_screen()
                .rows(0, cols)
                .nth(emulator_row)
                .unwrap_or_default();

            let disp_map = build_display_to_raw_byte_map(
                &row_raw_bytes,
                crate::configs::LineDisplayMode::Pretty,
            );

            // Map target_col (which is a visual char index) to display byte, then to raw byte:
            let plain_byte_offset = plain_text
                .char_indices()
                .nth(target_col)
                .map(|(b, _)| b)
                .unwrap_or(plain_text.len());

            let raw_byte_offset =
                map_disp_byte_to_raw(plain_byte_offset, &disp_map, row_raw_bytes.len());

            if let Some(raw_area) = raw_side_area {
                let usable_width = (raw_area.width as usize).saturating_sub(1);
                if is_search_active {
                    app.terminal_mode.center_raw_pan_on_byte(
                        raw_view,
                        usable_width,
                        raw_byte_offset,
                        row_raw_bytes.len(),
                    );
                } else {
                    app.terminal_mode.sync_raw_pan_with_local_col(
                        raw_view,
                        usable_width,
                        raw_byte_offset,
                        row_raw_bytes.len(),
                    );
                }
            }
        }
    }

    if let Some(raw_area) = raw_side_area {
        render_vt100_raw_side_panel(app, frame, raw_area, theme);
    }

    // seach
    if is_search_active {
        let normal_style = *theme.search.search_match;
        let active_style = *theme.search.search_match_active;
        let active_match_idx = app.terminal_mode.search().current_match;

        let buf = frame.buffer_mut();

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

            let char_col_start = search_match.col_idx;
            let match_len = search_match.match_char_len.max(1);

            if char_col_start + match_len <= pan_x {
                continue;
            }

            let is_active = idx == active_match_idx;
            let style = if is_active {
                active_style
            } else {
                normal_style
            };

            for c in 0..match_len {
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
                cell.set_style(style);
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
                        cell.set_style(theme.ui.selection);
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
                buf[(cursor_x, cursor_y)].set_style(*theme.ui.cursor_unfocused);
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
                cell.set_bg(theme.terminal.term_flash_bg_color);
                cell.set_fg(theme.terminal.term_flash_fg_color);
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
                Span::styled(formatted, theme.terminal.timestamp),
                Span::styled("│", theme.terminal.timestamp_del),
            ]));
        } else {
            let spaces = " ".repeat(ts_width);
            lines.push(Line::from(vec![
                Span::raw(spaces),
                Span::styled("│", theme.terminal.timestamp_del_empty),
            ]));
        }
    }

    let gutter = Paragraph::new(lines);
    frame.render_widget(gutter, area);
}

// NOTE: verify evolution with mapping
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

    let (block, inner_area) =
        raw_panel_layout(raw_area, app.settings().raw_side.orientation, theme);
    let view_height = inner_area.height as usize;

    let display_mode = raw_view_to_display_mode(raw_view);
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

    let search = app.terminal_mode.search();
    let is_search_active = search.active && !search.matches.is_empty();
    let active_match_idx = search.current_match;

    let normal_style = *theme.search.search_match;
    let active_style = *theme.search.search_match_active;

    let rows_to_render = view_height.min((grid_rows as usize).saturating_sub(pan_y));
    let mut raw_lines = Vec::with_capacity(rows_to_render);

    for (r, row_raw_bytes) in screen
        .rows_formatted(0, cols)
        .skip(pan_y)
        .take(rows_to_render)
        .enumerate()
    {
        let emulator_row = r + pan_y;
        let world_row = top_visible_world_line + emulator_row;
        let raw_bytes_slice = row_raw_bytes.as_slice();

        // Plain text for column -> byte resolution:
        let plain_text = screen.rows(0, cols).nth(emulator_row).unwrap_or_default();

        let mut plain_char_to_byte = Vec::with_capacity(plain_text.chars().count() + 1);
        for (b_idx, _) in plain_text.char_indices() {
            plain_char_to_byte.push(b_idx);
        }
        plain_char_to_byte.push(plain_text.len());

        let disp_map =
            build_display_to_raw_byte_map(raw_bytes_slice, crate::configs::LineDisplayMode::Pretty);

        let mut highlights = Vec::new();

        if is_search_active {
            let start_idx = search.matches.partition_point(|m| m.line_idx < world_row);
            let end_idx = search.matches.partition_point(|m| m.line_idx <= world_row);

            for (global_m_idx, m) in search.matches[start_idx..end_idx]
                .iter()
                .enumerate()
                .map(|(offset, m)| (start_idx + offset, m))
            {
                let disp_start_b = plain_char_to_byte.get(m.col_idx).copied().unwrap_or(0);
                let disp_end_b = plain_char_to_byte
                    .get(m.col_idx + m.match_char_len.max(1))
                    .copied()
                    .unwrap_or(plain_text.len());

                let raw_m_start =
                    map_disp_byte_to_raw(disp_start_b, &disp_map, raw_bytes_slice.len());
                let raw_m_end = map_disp_byte_to_raw(disp_end_b, &disp_map, raw_bytes_slice.len());

                if raw_m_start < raw_m_end {
                    highlights.push(CharHighlight {
                        start_col: raw_m_start,
                        end_col: raw_m_end,
                        style: if global_m_idx == active_match_idx {
                            active_style
                        } else {
                            normal_style
                        },
                    });
                }
            }
        }

        if let Some((start_pos, end_pos)) = normalized_selection
            && world_row >= start_pos.line_idx
            && world_row <= end_pos.line_idx
        {
            let is_first_line = world_row == start_pos.line_idx;
            let is_last_line = world_row == end_pos.line_idx;

            let raw_from_b = if !is_first_line || start_pos.char_idx == 0 {
                0
            } else {
                let disp_from_b = plain_char_to_byte
                    .get(start_pos.char_idx)
                    .copied()
                    .unwrap_or(0);
                map_disp_byte_to_raw(disp_from_b, &disp_map, raw_bytes_slice.len())
            };

            let raw_to_b = if !is_last_line {
                raw_bytes_slice.len()
            } else {
                let to_char = end_pos.char_idx + 1;
                if to_char >= plain_text.chars().count() {
                    raw_bytes_slice.len()
                } else {
                    let disp_to_b = plain_char_to_byte
                        .get(to_char)
                        .copied()
                        .unwrap_or(plain_text.len());
                    map_disp_byte_to_raw(disp_to_b, &disp_map, raw_bytes_slice.len())
                }
            };

            if raw_from_b < raw_to_b {
                highlights.push(CharHighlight {
                    start_col: raw_from_b,
                    end_col: raw_to_b,
                    style: *theme.ui.selection,
                });
            }
        }

        raw_lines.push(format_raw_bytes_with_highlights(
            raw_bytes_slice,
            display_mode,
            &highlights,
            theme,
        ));
    }

    render_raw_panel_widget(
        frame,
        raw_area,
        inner_area,
        block,
        raw_lines,
        app.terminal_mode.raw_pan_x(),
    );
}
