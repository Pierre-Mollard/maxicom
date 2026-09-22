use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::{
    app::App,
    terminal::{MaxicomCursor, MaxicomTerminal},
    types::FocusTarget,
};

pub fn render_vt100_terminal(app: &mut App, frame: &mut Frame, area: Rect, outer_area: Rect) {
    let view_height = area.height as usize;
    let focus = app.focus_target();
    let is_terminal_focused = focus == FocusTarget::Terminal;
    let cursor = MaxicomCursor::from_config(&app.config().terminal_mode.cursor);

    let term_widget = MaxicomTerminal::new(app.terminal_mode.vt100_screen())
        .x_offset(app.terminal_mode.pan_offset_x() as u16)
        .y_offset(app.terminal_mode.pan_offset_y() as u16)
        .focused(is_terminal_focused && app.is_app_focused())
        .cursor(cursor)
        .filling_char(&app.config().terminal_mode.empty_fill_char);

    frame.render_widget(term_widget, area);

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

    if !app.terminal_mode.search().active
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
            frame.set_cursor_position((area.x + col, area.y + row));
        }
    }

    if app.terminal_mode.search().active {
        let visual_cursor = app.terminal_mode.search().query.visual_cursor() as u16;

        // outer_area.x + 1 (left border) + " SEARCH: " (9) + 1 (space) + visual_cursor
        let cursor_x = outer_area.x + 1 + 9 + 1 + visual_cursor;
        // Bottom border line of the monitor outer box
        let cursor_y = outer_area.y + outer_area.height.saturating_sub(1);

        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

pub fn render_timestamp_gutter(app: &App, frame: &mut Frame, area: Rect) {
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
            let formatted = ts.format("%H:%M:%S%.3f").to_string();
            lines.push(Line::from(vec![
                Span::styled(formatted, Style::default().fg(Color::DarkGray)),
                Span::styled("│", Style::default().fg(Color::DarkGray)),
            ]));
        } else {
            lines.push(Line::raw("            │"));
        }
    }

    let gutter = Paragraph::new(lines);
    frame.render_widget(gutter, area);
}
