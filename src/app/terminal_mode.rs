use std::{collections::VecDeque, io, path::PathBuf};

use chrono::{DateTime, Local};
use crossterm::event::KeyEvent;
use vt100::Parser;

use crate::{
    app::{
        UpdateEffect, ViewportSize, scroller::ViewportScroller, search::SearchState,
        selection::CopyModeState,
    },
    configs::{TerminalDimensionMode, TextEncoding},
    inputs::{CopyModeAction, SearchModeAction, TerminalAction, dispatch_input_key},
    logger::TerminalLogger,
};

pub struct TerminalMode {
    vt100_parser: Parser,
    logger: TerminalLogger,
    max_content_scrollback: usize,
    max_history_lines: usize,
    search_state: SearchState,
    copy_state: CopyModeState,
    scroller: ViewportScroller,
    pan_offset_y: usize,

    history_timestamps: VecDeque<DateTime<Local>>,
    screen_timestamps: Vec<Option<DateTime<Local>>>,
}

impl TerminalMode {
    pub fn new(max_history_lines: usize, rows: u16, cols: u16, logger: TerminalLogger) -> Self {
        let vt100_parser = Parser::new(rows, cols, max_history_lines);
        let search_state = SearchState::default();
        let copy_state = CopyModeState::default();
        let scroller = ViewportScroller::default();

        TerminalMode {
            vt100_parser,
            logger,
            max_content_scrollback: 0,
            max_history_lines,
            search_state,
            copy_state,
            scroller,
            pan_offset_y: 0,
            history_timestamps: VecDeque::with_capacity(max_history_lines),
            screen_timestamps: vec![None; rows as usize],
        }
    }

    pub fn process_rx(&mut self, encoding: TextEncoding, raw_bytes: &[u8]) {
        let is_alternate = self.vt100_parser.screen().alternate_screen();
        let vt_rows = self.vt100_parser.screen().size().0 as usize;

        let _ = self.logger.log_raw_bytes(raw_bytes);

        if !is_alternate {
            for &b in raw_bytes {
                if b == b'\n' {
                    let (cursor_row, _) = self.vt100_parser.screen().cursor_position();

                    if (cursor_row as usize) >= vt_rows.saturating_sub(1) {
                        // Bottom of screen
                        if let Some(top_ts) = self.screen_timestamps.first().copied().flatten() {
                            if self.history_timestamps.len() >= self.max_history_lines {
                                self.history_timestamps.pop_front();
                            }
                            self.history_timestamps.push_back(top_ts);
                        }
                        self.screen_timestamps.remove(0);
                        self.screen_timestamps.push(Some(Local::now()));
                    } else {
                        // In the middle of screen
                        let next_row = (cursor_row as usize) + 1;
                        if next_row < self.screen_timestamps.len() {
                            self.screen_timestamps[next_row] = Some(Local::now());
                        }
                    }
                }
            }
        }

        self.scroller.on_rx_received();

        let utf8_bytes = encoding.to_utf8_bytes(raw_bytes);
        self.vt100_parser.process(&utf8_bytes);

        let (r, _) = self.vt100_parser.screen().cursor_position();
        if let Some(slot) = self.screen_timestamps.get_mut(r as usize)
            && slot.is_none()
        {
            *slot = Some(Local::now());
        }

        if !is_alternate {
            self.refresh_max_scrollback();
        }
    }

    pub fn update_viewport(
        &mut self,
        viewport: ViewportSize,
        dim_mode: TerminalDimensionMode,
        conf_rows: u16,
        conf_cols: u16,
    ) {
        self.scroller.update_view_size(viewport.cols, viewport.rows);

        let avail_h = viewport.rows as u16;
        let avail_w = viewport.cols as u16;

        if avail_h > 0 && avail_w > 0 {
            let (cur_rows, cur_cols) = self.get_vt100_size();

            match dim_mode {
                TerminalDimensionMode::GrowOnly => {
                    let target_rows = cur_rows.max(avail_h);
                    let target_cols = cur_cols.max(avail_w);
                    if (target_rows, target_cols) != (cur_rows, cur_cols) {
                        self.set_vt100_size(target_rows, target_cols);
                    }
                }
                TerminalDimensionMode::FixedCols => {
                    let target_rows = avail_h;
                    let target_cols = conf_cols;
                    if (target_rows, target_cols) != (cur_rows, cur_cols) {
                        self.set_vt100_size(target_rows, target_cols);
                    }
                }

                TerminalDimensionMode::MatchView => {
                    if (avail_h, avail_w) != (cur_rows, cur_cols) {
                        self.set_vt100_size(avail_h, avail_w);
                    }
                }

                TerminalDimensionMode::Fixed => {
                    if (conf_rows, conf_cols) != (cur_rows, cur_cols) {
                        self.set_vt100_size(conf_rows, conf_cols);
                    }
                }
            }
        }

        self.clamp_pan_x(viewport.cols);
        self.reconcile_both_vertical_scroll();
    }

    pub fn row_has_content(&self, row: u16) -> bool {
        let screen = self.vt100_parser.screen();
        let cols = screen.size().1;
        for c in 0..cols {
            if let Some(cell) = screen.cell(row, c)
                && cell.has_contents()
            {
                return true;
            }
        }
        false
    }

    pub fn total_timeline_len(&self) -> usize {
        self.history_timestamps.len() + self.screen_timestamps.len()
    }

    pub fn get_timeline_timestamp(&self, global_idx: usize) -> Option<DateTime<Local>> {
        let history_len = self.history_timestamps.len();
        if global_idx < history_len {
            self.history_timestamps.get(global_idx).copied()
        } else {
            let screen_idx = global_idx - history_len;
            self.screen_timestamps.get(screen_idx).copied().flatten()
        }
    }

    pub fn clear_history(&mut self) {
        let (rows, cols) = self.get_vt100_size();

        self.vt100_parser = Parser::new(rows, cols, self.max_history_lines);
        self.history_timestamps.clear();
        self.screen_timestamps = vec![None; rows as usize];

        self.max_content_scrollback = 0;
        self.pan_offset_y = 0;

        self.scroller.scroll_to_bottom();
        self.scroller.max_scroll_y = 0;
        self.scroller.offset_x = 0;
        self.scroller.pan_right_explored = 0;
    }

    // HACK: there is no function to fetch string content from history so sb everywhere
    pub fn get_world_line_text(&mut self, world_row: usize) -> Option<String> {
        let (rows, cols) = self.get_vt100_size();
        let max_sb = self.max_content_scrollback;
        let total_world = max_sb + rows as usize;

        if world_row >= total_world {
            return None;
        }

        let current_sb = self.vt100_parser.screen().scrollback();

        let (view_row, target_sb) = if world_row < max_sb {
            // history
            (0, max_sb - world_row)
        } else {
            // live view
            ((world_row - max_sb) as u16, 0)
        };

        if current_sb != target_sb {
            self.vt100_parser.screen_mut().set_scrollback(target_sb);
        }

        let mut line_str = String::with_capacity(cols as usize);
        for c in 0..cols {
            if let Some(cell) = self.vt100_parser.screen().cell(view_row, c) {
                line_str.push_str(cell.contents());
            } else {
                line_str.push(' ');
            }
        }

        if current_sb != target_sb {
            self.vt100_parser.screen_mut().set_scrollback(current_sb);
        }

        Some(line_str)
    }

    pub fn get_world_line_trimmed_len(&mut self, world_row: usize) -> usize {
        self.get_world_line_text(world_row)
            .map(|s| s.trim_end().chars().count())
            .unwrap_or(0)
    }

    #[inline]
    pub fn vt100_screen(&self) -> &vt100::Screen {
        self.vt100_parser.screen()
    }

    #[inline]
    pub fn pan_offset(&self) -> usize {
        self.scroller.offset_x
    }

    #[inline]
    pub fn pan_offset_x(&self) -> usize {
        self.scroller.offset_x
    }

    #[inline]
    pub fn pan_offset_y(&self) -> usize {
        self.pan_offset_y
    }

    #[inline]
    pub fn scrollback_offset(&self) -> usize {
        self.scroller.offset_y
    }

    // for 2D terminal mode the metrics and clamping is handled within the emulator
    fn clamp_pan_x(&mut self, visible_cols: usize) {
        let vt_cols = self.vt100_parser.screen().size().1 as usize;
        let max_pan = vt_cols.saturating_sub(visible_cols);
        self.scroller.max_pan_x = max_pan;
        self.scroller.offset_x = self.scroller.offset_x.min(max_pan);
    }

    pub fn pan_to_right(&mut self, cols: usize, visible_cols: usize) {
        self.scroller.pan_right(cols);
        self.clamp_pan_x(visible_cols);
    }

    pub fn pan_to_left(&mut self, cols: usize, visible_cols: usize) {
        self.scroller.pan_left(cols);
        self.clamp_pan_x(visible_cols);
    }

    pub fn can_pan_right(&self) -> Option<(usize, usize)> {
        self.scroller.can_pan_right()
    }

    pub fn can_pan_left(&self) -> Option<usize> {
        self.scroller.can_pan_left()
    }

    pub fn max_screen_pan_y(&self) -> usize {
        let vt_rows = self.vt100_parser.screen().size().0 as usize;
        vt_rows.saturating_sub(self.scroller.view_height)
    }

    pub fn max_vertical_scroll(&self) -> usize {
        let max_pan_y = self.max_screen_pan_y();
        if self.vt100_parser.screen().alternate_screen() {
            max_pan_y
        } else {
            max_pan_y + self.max_content_scrollback
        }
    }

    // for 2D terminal mode the metrics and clamping is handled within the emulator
    pub fn reconcile_both_vertical_scroll(&mut self) {
        let max_pan_y = self.max_screen_pan_y();
        let max_total = self.max_vertical_scroll();

        self.scroller.max_scroll_y = max_total;
        self.scroller.clamp();

        let (y_offset, vt_scrollback) = if self.scroller.offset_y <= max_pan_y {
            // In the active screen: pan from bottom to top
            let y = max_pan_y - self.scroller.offset_y;
            (y as u16, 0)
        } else {
            // Past the active screen: camera is at top (y=0), vt100 scrolls history
            let history_offset = self.scroller.offset_y - max_pan_y;
            (0, history_offset)
        };

        self.pan_offset_y = y_offset as usize;
        self.vt100_parser.screen_mut().set_scrollback(vt_scrollback);
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroller.max_scroll_y = self.max_vertical_scroll();
        self.scroller.scroll_up(lines);
        self.reconcile_both_vertical_scroll();
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroller.max_scroll_y = self.max_vertical_scroll();
        self.scroller.scroll_down(lines);
        self.reconcile_both_vertical_scroll();
    }

    pub fn scroll_up_page(&mut self) {
        self.scroll_up(self.scroller.view_height.saturating_sub(1));
    }

    pub fn scroll_down_page(&mut self) {
        self.scroll_down(self.scroller.view_height.saturating_sub(1));
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroller.scroll_to_bottom();
        self.reconcile_both_vertical_scroll();
    }

    pub fn scroll_to_top(&mut self) {
        self.scroller.max_scroll_y = self.max_vertical_scroll();
        self.scroller.scroll_to_top();
        self.reconcile_both_vertical_scroll();
    }

    pub fn scroll_to_target_line(&mut self, target_line_idx: usize, view_height: usize) {
        let (grid_rows, _) = self.get_vt100_size();
        let max_sb = self.max_content_scrollback;
        let total_world_lines = max_sb + (grid_rows as usize);

        if total_world_lines == 0 || target_line_idx >= total_world_lines {
            return;
        }

        let lines_from_bottom = (total_world_lines - 1).saturating_sub(target_line_idx);

        let effective_height = (grid_rows as usize).min(view_height);
        let half_canvas = effective_height / 2;

        let target_offset = lines_from_bottom.saturating_sub(half_canvas);

        self.scroller.max_scroll_y = self.max_vertical_scroll();
        self.scroller.scroll_to(target_offset);
        self.reconcile_both_vertical_scroll();
    }

    // HACK: there is no scrollback current len from the crate, make PR?
    pub fn refresh_max_scrollback(&mut self) {
        let current = self.vt100_parser.screen().scrollback();
        self.vt100_parser.screen_mut().set_scrollback(usize::MAX);
        self.max_content_scrollback = self.vt100_parser.screen().scrollback();
        self.vt100_parser.screen_mut().set_scrollback(current);
        self.scroller.max_scroll_y = self.max_vertical_scroll();
    }

    pub fn set_vt100_size(&mut self, rows: u16, cols: u16) {
        let (old_rows, old_cols) = self.vt100_parser.screen().size();
        if (old_rows, old_cols) == (rows, cols) || rows == 0 || cols == 0 {
            return;
        }

        self.vt100_parser.screen_mut().set_size(rows, cols);

        let new_rows = rows as usize;
        self.screen_timestamps.resize(new_rows, None);
    }

    /// Return (rows, cols)
    pub fn get_vt100_size(&self) -> (u16, u16) {
        self.vt100_parser.screen().size()
    }

    pub fn get_max_scrollback(&self) -> usize {
        self.max_content_scrollback
    }

    pub fn is_new_content_missed(&self) -> bool {
        self.scroller.new_content_missed
    }

    pub fn all_text_lines(&mut self) -> Vec<String> {
        let max_sb = self.max_content_scrollback;
        let (rows, cols) = self.get_vt100_size();
        let current_sb = self.vt100_parser.screen().scrollback();

        let mut lines = Vec::with_capacity(max_sb + rows as usize);

        let mut sb = max_sb;
        while sb > 0 {
            self.vt100_parser.screen_mut().set_scrollback(sb);
            if let Some(first_row) = self.vt100_parser.screen().rows(0, cols).next() {
                lines.push(first_row);
            }
            sb -= 1;
        }

        self.vt100_parser.screen_mut().set_scrollback(0);
        for row in self.vt100_parser.screen().rows(0, cols) {
            lines.push(row);
        }

        self.vt100_parser.screen_mut().set_scrollback(current_sb);

        lines
    }

    pub fn update_action_terminal(&mut self, action: TerminalAction) -> UpdateEffect {
        match action {
            TerminalAction::AutoResizeRemoteToViewer => {
                let rows = self.scroller.view_height as u16;
                let cols = self.scroller.view_width as u16;
                let cmd =
                    format!("stty rows {rows} cols {cols}; export LINES={rows} COLUMNS={cols}\n");
                UpdateEffect::Transmit(cmd.into_bytes())
            }
            TerminalAction::AutoResizeEmulatorToViewer => {
                self.set_vt100_size(
                    self.scroller.view_height as u16,
                    self.scroller.view_width as u16,
                );
                self.scroller.offset_x = 0;
                self.scroller.max_pan_x = 0;
                UpdateEffect::None
            }
        }
    }
    pub fn handle_search_action(&mut self, action: SearchModeAction) {
        match action {
            SearchModeAction::Start => {
                self.search_state.toggle_itself();
                if self.search_state.active {
                    self.update_search_matches();
                }
            }
            SearchModeAction::Next => {
                if self.search_state.active
                    && let Some(target_line_idx) = self.search_state.next_match()
                {
                    self.search_state.target_jump_index = Some(target_line_idx);
                }
            }
            SearchModeAction::Previous => {
                if self.search_state.active
                    && let Some(target_line_idx) = self.search_state.prev_match()
                {
                    self.search_state.target_jump_index = Some(target_line_idx);
                }
            }
            SearchModeAction::Cancel => {
                self.search_state.toggle(false);
            }
            SearchModeAction::Confirm => {
                let match_pos = self.search_state.current_match_position();
                self.search_state.toggle(false);

                if let Some((row, col)) = match_pos {
                    self.copy_state.enter_at(row, col);
                } else {
                    self.copy_state.enter_at(0, 0);
                }
            }
            SearchModeAction::HandleKey(key_event) => {
                if self.search_state.active {
                    self.handle_input_search(key_event);
                }
            }
        }
    }

    pub fn handle_copy_action(&mut self, action: CopyModeAction) -> Option<String> {
        let (grid_rows, cols) = self.get_vt100_size();
        let total_world_rows = self.max_content_scrollback + (grid_rows as usize);

        if total_world_rows == 0 {
            if action == CopyModeAction::Exit {
                self.copy_state.exit();
            }
            return None;
        }

        let max_row = total_world_rows.saturating_sub(1);
        let max_col = (cols as usize).saturating_sub(1);

        match action {
            CopyModeAction::Enter => {
                let max_sb = self.max_content_scrollback;
                let (c_row, c_col) = self.vt100_screen().cursor_position();

                let cursor_world_row = max_sb + (c_row as usize);
                let cursor_world_col = (c_col as usize).min(max_col);

                self.copy_state.enter_at(cursor_world_row, cursor_world_col);
                self.keep_terminal_copy_cursor_in_view();
                return None;
            }
            CopyModeAction::Exit => {
                self.copy_state.exit();
                return None;
            }
            CopyModeAction::MoveUp => {
                let target_row = self.copy_state.cursor.line_idx.saturating_sub(1);
                self.copy_state.cursor.line_idx = target_row;
                let trimmed_len = self.get_world_line_trimmed_len(target_row);
                let max_valid_char = trimmed_len.saturating_sub(1);
                self.copy_state.cursor.char_idx =
                    self.copy_state.cursor.preferred_col.min(max_valid_char);
                self.keep_terminal_copy_cursor_in_view();
            }
            CopyModeAction::MoveDown => {
                let target_row = (self.copy_state.cursor.line_idx + 1).min(max_row);
                self.copy_state.cursor.line_idx = target_row;
                let trimmed_len = self.get_world_line_trimmed_len(target_row);
                let max_valid_char = trimmed_len.saturating_sub(1);
                self.copy_state.cursor.char_idx =
                    self.copy_state.cursor.preferred_col.min(max_valid_char);
                self.keep_terminal_copy_cursor_in_view();
            }
            CopyModeAction::MoveLeft => {
                self.copy_state.cursor.char_idx = self.copy_state.cursor.char_idx.saturating_sub(1);
                self.copy_state.cursor.preferred_col = self.copy_state.cursor.char_idx;
                self.keep_terminal_copy_cursor_in_view();
            }
            CopyModeAction::MoveRight => {
                let current_row = self.copy_state.cursor.line_idx;
                let trimmed_len = self.get_world_line_trimmed_len(current_row);
                let max_valid_char = trimmed_len.saturating_sub(1);
                self.copy_state.cursor.char_idx =
                    (self.copy_state.cursor.char_idx + 1).min(max_valid_char);
                self.copy_state.cursor.preferred_col = self.copy_state.cursor.char_idx;
                self.keep_terminal_copy_cursor_in_view();
            }
            CopyModeAction::PageUp => {
                let jump = self.scroller.view_height.saturating_sub(1);
                let target_row = self.copy_state.cursor.line_idx.saturating_sub(jump);
                self.copy_state.cursor.line_idx = target_row;
                let trimmed_len = self.get_world_line_trimmed_len(target_row);
                let max_valid_char = trimmed_len.saturating_sub(1);
                self.copy_state.cursor.char_idx =
                    self.copy_state.cursor.preferred_col.min(max_valid_char);
                self.keep_terminal_copy_cursor_in_view();
            }
            CopyModeAction::PageDown => {
                let jump = self.scroller.view_height.saturating_sub(1);
                let target_row = (self.copy_state.cursor.line_idx + jump).min(max_row);
                self.copy_state.cursor.line_idx = target_row;
                let trimmed_len = self.get_world_line_trimmed_len(target_row);
                let max_valid_char = trimmed_len.saturating_sub(1);
                self.copy_state.cursor.char_idx =
                    self.copy_state.cursor.preferred_col.min(max_valid_char);
                self.keep_terminal_copy_cursor_in_view();
            }
            CopyModeAction::StartOfLine => {
                self.copy_state.cursor.char_idx = 0;
                self.copy_state.cursor.preferred_col = 0;
                self.keep_terminal_copy_cursor_in_view();
            }
            CopyModeAction::EndOfLine => {
                let current_row = self.copy_state.cursor.line_idx;
                let trimmed_len = self.get_world_line_trimmed_len(current_row);
                let last_char = trimmed_len.saturating_sub(1);
                self.copy_state.cursor.char_idx = last_char;
                self.copy_state.cursor.preferred_col = last_char;
                self.keep_terminal_copy_cursor_in_view();
            }
            CopyModeAction::Select => {
                self.copy_state.toggle_selection();
            }
            CopyModeAction::Yank => {
                let selected = self.extract_selected_text();
                self.copy_state.exit();
                return selected;
            }
        }

        None
    }

    fn keep_terminal_copy_cursor_in_view(&mut self) {
        let cursor_row = self.copy_state.cursor.line_idx;
        let cursor_col = self.copy_state.cursor.char_idx;
        let view_h = self.scroller.view_height;
        let view_w = self.scroller.view_width; // or passed-in visible columns width
        let (grid_rows, grid_cols) = self.get_vt100_size();

        let current_sb = self.vt100_screen().scrollback();
        let top_visible = if current_sb > 0 {
            self.max_content_scrollback.saturating_sub(current_sb)
        } else {
            self.max_content_scrollback
        };
        let effective_h = (grid_rows as usize).min(view_h);
        let bottom_visible = top_visible + effective_h.saturating_sub(1);

        if cursor_row < top_visible || cursor_row > bottom_visible {
            self.scroll_to_target_line(cursor_row, view_h);
        }

        if view_w > 0 && (grid_cols as usize) > view_w {
            let max_pan_x = (grid_cols as usize).saturating_sub(view_w);

            if cursor_col < self.scroller.offset_x {
                self.scroller.offset_x = cursor_col;
            } else if cursor_col >= self.scroller.offset_x + view_w {
                self.scroller.offset_x = (cursor_col + 1).saturating_sub(view_w).min(max_pan_x);
            }
        } else {
            self.scroller.offset_x = 0;
        }
    }

    fn extract_selected_text(&mut self) -> Option<String> {
        let (start, end) = self.copy_state.normalized_selection()?;
        let mut out = String::new();

        for row_idx in start.line_idx..=end.line_idx {
            let Some(line_str) = self.get_world_line_text(row_idx) else {
                continue;
            };
            let trimmed_len = line_str.trim_end().chars().count();
            let chars: Vec<char> = line_str.chars().collect();

            let col_start = if row_idx == start.line_idx {
                start.char_idx.min(chars.len())
            } else {
                0
            };
            let col_end = if row_idx == end.line_idx {
                (end.char_idx + 1).min(trimmed_len)
            } else {
                trimmed_len
            };

            if col_start < col_end {
                out.extend(&chars[col_start..col_end]);
            }
            if row_idx != end.line_idx {
                out.push('\n');
            }
        }

        if out.is_empty() { None } else { Some(out) }
    }

    pub fn update_search_matches(&mut self) {
        let text = self.all_text_lines();
        self.search_state.update_matches_from_strings(&text);
    }

    pub fn handle_input_search(&mut self, key_event: KeyEvent) {
        dispatch_input_key(&mut self.search_state.query, key_event);
        self.update_search_matches();
        if let Some(&first_match_idx) = self.search_state.matches.first() {
            self.search_state.current_match = 0;
            self.search_state.target_jump_index = Some(first_match_idx.line_idx);
        }
    }

    pub fn start_logging(&mut self, path: PathBuf) -> io::Result<()> {
        self.logger.start_with_path(path)
    }

    pub fn stop_logging(&mut self) {
        self.logger.stop();
    }

    pub fn is_logging(&self) -> bool {
        self.logger.is_active()
    }

    pub fn logger_active_path(&self) -> &PathBuf {
        self.logger.active_path()
    }

    #[inline]
    pub fn search(&self) -> &SearchState {
        &self.search_state
    }

    #[inline]
    pub fn search_mut(&mut self) -> &mut SearchState {
        &mut self.search_state
    }

    #[inline]
    pub fn copy_state(&self) -> &CopyModeState {
        &self.copy_state
    }

    #[inline]
    pub fn scroller(&self) -> &ViewportScroller {
        &self.scroller
    }
}
