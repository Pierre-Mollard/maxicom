use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, ScrollbarState},
};

use crate::{
    app::{
        App,
        selection::CopySide,
        types::{FocusTarget, RowChars, ScrollbarMap, TextRow},
    },
    configs::{KeywordHighlight, Mode, ThemeConfig},
    invalid_bytes::{shown, shown_string},
    terminal::{MaxicomCursor, MaxicomTerminal, vt100_size},
    ui::{
        common::{draw_scrollbar_marks, keyword_matches, render_search_bar, scrollbar},
        raw_side,
    },
};

/// Colors the rules' matches in the grid rows drawn in `area`, panned by `pan` (columns, rows).
/// Each row is matched as part of its line: the rows the remote wrapped joined, read over the
/// grid's whole width, so neither the wrap nor the pan cuts a word (MAX-229). A line wrapped
/// from above the grid's window (scrolled back) can only be read from the window's first row.
/// A line on one row is read from vt100's row text, without its blank end, and its cells only
/// looked up when a rule matched (MAX-237: a lookup per cell was most of the frame's cost)
fn highlight_grid(
    buf: &mut Buffer,
    area: Rect,
    screen: &vt100::Screen,
    (pan_x, pan_y): (u16, u16),
    highlights: &[KeywordHighlight],
) {
    if highlights.iter().all(|hl| hl.pattern.is_empty()) {
        return;
    }
    let (grid_rows, cols) = vt100_size(screen);
    let shown_end = grid_rows.min(pan_y.saturating_add(area.height));
    // the first line shown can start above the view
    let mut row = pan_y;
    while row > 0 && screen.row_wrapped(row - 1) {
        row -= 1;
    }
    let mut row_texts = screen
        .rows(0, cols)
        .skip(usize::from(row))
        .map(shown_string);
    let mut joined = String::new();
    // each cell's text start in the line's text, its grid row and column
    let mut cells: Vec<(usize, u16, u16)> = Vec::new();
    while row < shown_end {
        let first_row = row;
        let mut row_text = row_texts.next().unwrap_or_default();
        cells.clear();
        // a line the remote wrapped: its rows read cell by cell, the blank end of each but the
        // last one included (they are part of the line)
        if screen.row_wrapped(row) && row + 1 < grid_rows {
            joined.clear();
            loop {
                let wrapped = screen.row_wrapped(row) && row + 1 < grid_rows;
                push_row_cells(screen, row, cols, wrapped, &mut joined, &mut cells);
                row += 1;
                if !wrapped {
                    break;
                }
                row_texts.next();
            }
            std::mem::swap(&mut row_text, &mut joined);
        } else {
            row += 1;
        }
        if row_text.is_empty() {
            continue;
        }
        // one row of ASCII: one byte per cell, a match's bytes are its columns
        let by_column = cells.is_empty() && row_text.is_ascii();
        let mut set_style = |row: u16, col: u16, style: Style| {
            let y = row.checked_sub(pan_y).filter(|&y| y < area.height);
            let x = col.checked_sub(pan_x).filter(|&x| x < area.width);
            if let (Some(x), Some(y)) = (x, y) {
                buf[(area.x + x, area.y + y)].set_style(style);
            }
        };

        for hl in highlights.iter().filter(|hl| !hl.pattern.is_empty()) {
            for (start, end) in keyword_matches(&row_text, hl) {
                if by_column {
                    for col in start..end {
                        set_style(first_row, col as u16, hl.style);
                    }
                    continue;
                }
                if cells.is_empty() {
                    joined.clear();
                    push_row_cells(screen, first_row, cols, false, &mut joined, &mut cells);
                    debug_assert_eq!(joined, row_text);
                }
                let first = cells.partition_point(|cell| cell.0 < start);
                let last = cells.partition_point(|cell| cell.0 < end);
                for &(_, row, col) in &cells[first..last] {
                    set_style(row, col, hl.style);
                }
            }
        }
    }
}

/// Appends grid row `row`'s text to `text` as vt100's row text has it (an empty cell before a
/// char as a space, a wide char's second cell left out: its first cell's style covers both),
/// and each of those cells' text start, row and column to `cells`; with `to_end`, the empty
/// cells at the end too
fn push_row_cells(
    screen: &vt100::Screen,
    row: u16,
    cols: u16,
    to_end: bool,
    text: &mut String,
    cells: &mut Vec<(usize, u16, u16)>,
) {
    let mut blanks_from = None;
    let mut prev_was_wide = false;
    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else {
            continue;
        };
        if std::mem::take(&mut prev_was_wide) {
            continue;
        }
        prev_was_wide = cell.is_wide();
        if !cell.has_contents() {
            blanks_from.get_or_insert(col);
            continue;
        }
        for blank in blanks_from.take().unwrap_or(col)..col {
            cells.push((text.len(), row, blank));
            text.push(' ');
        }
        cells.push((text.len(), row, col));
        text.push_str(&shown(cell.contents()));
    }
    if to_end && let Some(from) = blanks_from {
        for blank in from..cols {
            cells.push((text.len(), row, blank));
            text.push(' ');
        }
    }
}

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
    let raw_view = app.settings().raw_side.view;
    // matches shown while data came, found again once a frame (MAX-218)
    app.terminal_mode.refresh_stale_matches(raw_view);

    // a raw search's matches are in the dump, its cursor brought it there (MAX-252)
    let dump_search = app.terminal_mode.dump_search();
    let search_jump = app.terminal_mode.search_mut().target_jump.take();
    if let Some((target_row, target_col)) = search_jump.filter(|_| !dump_search) {
        app.terminal_mode.center_on_target_match(
            target_row,
            target_col,
            content_area.width as usize,
            view_height,
        );
    }

    // the dump copy cursor's cell (MAX-256), before the view: the screen may follow it
    let traced = app.terminal_mode.trace_cell(app.settings().encoding);

    let cursor = MaxicomCursor::from_config(
        *theme.ui.cursor_unfocused,
        &app.settings().terminal_mode.unfocused_cursor_glyph,
    );
    let term_widget = MaxicomTerminal::new(app.terminal_mode.vt100_screen())
        .x_offset(app.terminal_mode.pan_offset_x() as u16)
        .y_offset(app.terminal_mode.pan_offset_y() as u16)
        .focused(is_terminal_focused && app.is_app_focused())
        .cursor(cursor)
        .style(*theme.terminal.term_inside)
        .filling_char(&app.settings().terminal_mode.empty_fill_char)
        .filling_char_style(*theme.terminal.term_outside)
        .zero_width(app.settings().terminal_mode.draw_zero_width);

    frame.render_widget(term_widget, content_area);
    let pan = (
        app.terminal_mode.pan_offset_x() as u16,
        app.terminal_mode.pan_offset_y() as u16,
    );
    let screen = app.terminal_mode.vt100_screen();
    highlight_grid(frame.buffer_mut(), content_area, screen, pan, highlights);

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
    // the dump copy selection's cells on the rows shown, in its look (MAX-256)
    let first_shown = top_visible_world_line + pan_y;
    let shown_lines = first_shown..first_shown + content_area.height as usize;
    let mirror = (app.terminal_mode).selection_cells(app.settings().encoding, shown_lines);

    // the grid rows shown, for mouse clicks (MAX-113): one char per column
    if app.settings().ui.mouse {
        let shown_rows =
            (content_area.height as usize).min((grid_rows as usize).saturating_sub(pan_y));
        let map = app.frame_map_mut();
        map.text_area = content_area;
        map.text_rows.extend((0..shown_rows).map(|r| {
            Some(TextRow {
                line_idx: top_visible_world_line + pan_y + r,
                chars: RowChars::Grid { first: pan_x },
            })
        }));
    }

    let is_search_active = app.terminal_mode.search().shows_matches() && !dump_search;

    if let Some(raw_area) = raw_side_area {
        raw_side::terminal::render(app, frame, raw_area, theme);
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

            // one pass over the rows, rows().nth(r) would rebuild every row before r
            let visible_rows = app
                .terminal_mode
                .vt100_screen()
                .rows(0, cols)
                .skip(pan_y)
                .take(content_area.height as usize)
                .map(shown_string);

            for (r, row_text) in visible_rows.enumerate() {
                let emulator_row = r + pan_y;
                let world_row = top_visible_world_line + emulator_row;

                if world_row < start_pos.line_idx || world_row > end_pos.line_idx {
                    continue;
                }
                let trimmed_len = row_text.trim_end().chars().count();
                for c in 0..content_area.width as usize {
                    let world_col = c + pan_x;
                    if copy_state.is_selected(world_row, world_col) && world_col < trimmed_len {
                        let cell = &mut buf[(content_area.x + c as u16, content_area.y + r as u16)];
                        let raw = copy_state.side == CopySide::Raw;
                        cell.set_style(theme.ui.selection_style(raw));
                    }
                }
            }
        }

        for &((line, col), shown) in &mirror {
            let (Some(row), Some(col)) = (line.checked_sub(first_shown), col.checked_sub(pan_x))
            else {
                continue;
            };
            if row < content_area.height as usize && col < content_area.width as usize {
                let style = match shown {
                    true => theme.ui.selection_style(true),
                    false => *theme.ui.cursor_ghost_old,
                };
                buf[(content_area.x + col as u16, content_area.y + row as u16)].set_style(style);
            }
        }

        // the dump copy cursor's cell, its byte still shown or written over (MAX-256)
        if copy_state.side == CopySide::Raw
            && let Some(((line, col), shown)) = traced
            && let Some(row) = (line.checked_sub(top_visible_world_line + pan_y))
                .filter(|&row| row < content_area.height as usize)
            && let Some(col) = col
                .checked_sub(pan_x)
                .filter(|&col| col < content_area.width as usize)
        {
            let style = match shown {
                true => *theme.ui.cursor_ghost,
                false => *theme.ui.cursor_ghost_old,
            };
            buf[(content_area.x + col as u16, content_area.y + row as u16)].set_style(style);
        }

        // selection cursor
        if copy_state.side == CopySide::Text && cursor.line_idx >= top_visible_world_line {
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
        let has_borders = app.show_borders();

        if max_vertical > 0 && view_height > 0 {
            let position_from_top =
                max_vertical.saturating_sub(app.terminal_mode.scrollback_offset());

            let mut scrollbar_state = ScrollbarState::new(max_vertical)
                .viewport_content_length(view_height)
                .position(position_from_top);

            frame.render_stateful_widget(scrollbar(theme), sb_area, &mut scrollbar_state);
            // the marks' ticks, each at its row of the scrollback and the screen (MAX-164)
            if app.settings().ui.marks.scrollbar {
                let marks = app.terminal_mode.marks().sorted();
                let total = app.terminal_mode.total_timeline_len();
                draw_scrollbar_marks(frame.buffer_mut(), sb_area, &marks, total, theme);
            }
            // for the mouse (MAX-142)
            app.frame_map_mut().scrollbar = Some(ScrollbarMap {
                area: sb_area,
                max: max_vertical,
                position: position_from_top,
                view: view_height,
            });
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
    // open, or its matches kept in copy mode (MAX-215)
    if app.terminal_mode.search().active || app.terminal_mode.search().shows_matches() {
        let (targets, query) = render_search_bar(app, frame, content_area, theme);
        let map = app.frame_map_mut();
        map.click_targets.extend(targets);
        map.search_query = query;
    }

    // the row a mark jump landed on, flashed for a moment (MAX-164)
    if let Some(flash) = app.mark_flash(Mode::Terminal) {
        let (top, bottom) = app.terminal_mode.visible_lines();
        if (top..=bottom).contains(&flash) {
            let y = content_area.y + (flash - top) as u16;
            if y < content_area.bottom() {
                let row = Rect {
                    y,
                    height: 1,
                    ..content_area
                };
                frame.buffer_mut().set_style(row, *theme.ui.flash);
            }
        }
    }

    // flash for screenshot
    if app.terminal_mode.is_flashing() {
        let buf = frame.buffer_mut();
        for y in content_area.top()..content_area.bottom() {
            for x in content_area.left()..content_area.right() {
                let cell = &mut buf[(x, y)];
                cell.set_style(*theme.ui.flash);
            }
        }
    }
}

pub fn render_timestamp_gutter(app: &App, frame: &mut Frame, area: Rect, theme: &ThemeConfig) {
    let screen = app.terminal_mode.vt100_screen();
    let vt_rows = vt100_size(screen).0 as usize;
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
    let blank_ts = " ".repeat(app.settings().timestamp_width());

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
            lines.push(Line::from(vec![
                Span::raw(blank_ts.as_str()),
                Span::styled("│", theme.terminal.timestamp_del_empty),
            ]));
        }
    }

    let gutter = Paragraph::new(lines);
    frame.render_widget(gutter, area);
}

/// The history marks' letters on their rows (MAX-164), next to the text: on the timestamp
/// gutter's `│`, else in the left border; with neither, none (never over the screen's text)
pub fn draw_mark_letters(
    app: &App,
    buf: &mut Buffer,
    content_area: Rect,
    gutter: Option<Rect>,
    theme: &ThemeConfig,
) {
    if !app.settings().ui.marks.letters {
        return;
    }
    let x = if let Some(gutter) = gutter {
        // the bar after the time
        gutter.x + app.settings().timestamp_width() as u16
    } else if app.show_borders() && content_area.x > 0 {
        content_area.x - 1
    } else {
        return;
    };
    let marks = app.terminal_mode.marks();
    let (top, bottom) = app.terminal_mode.visible_lines();
    for (row, y) in (top..=bottom).zip(content_area.top()..content_area.bottom()) {
        if let Some(letter) = marks.letter_at(row) {
            buf[(x, y)]
                .set_symbol(letter.encode_utf8(&mut [0; 4]))
                .set_style(*theme.ui.mark);
        }
    }
}
