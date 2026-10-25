use std::{
    collections::VecDeque,
    fs::File,
    io::{self, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

use chrono::{DateTime, Local};
use crossterm::event::KeyEvent;
use terminput::Encoding;
use terminput_crossterm::to_terminput_key;

use crate::{
    app::{
        UpdateEffect, ViewportSize,
        raw_scroller::RawSideScroller,
        scroller::ViewportScroller,
        search::SearchState,
        selection::{CopyActionResult, CopyModeState, TextSource},
    },
    configs::{SettingsConfig, TerminalDimensionMode, TextEncoding, settings::RawSideView},
    inputs::{
        CopyModeAction, SearchModeAction, SideViewAction, TerminalAction, ViPending,
        dispatch_input_key, local_dispatchers::dispatch_copy_key_vi, paste_into_input,
    },
    logger::TerminalLogger,
    terminal::{TerminalParser, new_parser, vt100_dim, vt100_size},
};

pub struct TerminalMode {
    vt100_parser: TerminalParser,
    logger: TerminalLogger,
    max_content_scrollback: usize,
    max_history_lines: usize,
    search_state: SearchState,
    copy_state: CopyModeState,
    scroller: ViewportScroller,
    raw_scroller: RawSideScroller,
    pan_offset_y: usize,
    vi_pending_copy: Option<ViPending>,
    flash_until: Option<Instant>,
    row_timestamps: VecDeque<Option<DateTime<Local>>>,
    search_text: Option<Vec<String>>,
}

impl TerminalMode {
    pub fn new(max_history_lines: usize, rows: u16, cols: u16, logger: TerminalLogger) -> Self {
        let vt100_parser = new_parser(rows, cols, max_history_lines);
        let search_state = SearchState::default();
        let copy_state = CopyModeState::default();
        let scroller = ViewportScroller::default();
        let raw_scroller = RawSideScroller::default();

        TerminalMode {
            vt100_parser,
            logger,
            max_content_scrollback: 0,
            max_history_lines,
            search_state,
            copy_state,
            scroller,
            raw_scroller,
            pan_offset_y: 0,
            vi_pending_copy: None,
            flash_until: None,
            row_timestamps: VecDeque::from(vec![None; rows as usize]),
            search_text: None,
        }
    }

    pub fn process_rx(&mut self, encoding: TextEncoding, raw_bytes: &[u8]) {
        let _ = self.logger.log_raw_bytes(raw_bytes);
        self.scroller.on_rx_received();
        self.search_text = None;

        let started_on_main = !self.vt100_parser.screen().alternate_screen();
        let cursor_row_before = self.vt100_parser.screen().cursor_position().0 as usize;

        let utf8_bytes = encoding.to_utf8_bytes(raw_bytes);
        self.vt100_parser.process(&utf8_bytes);
        let evicted = std::mem::take(&mut self.vt100_parser.callbacks_mut().count);

        // the alternate screen has no scrollback and leaves the main one as it is: its rows are
        // synced once the remote is back on the main screen
        if self.vt100_parser.screen().alternate_screen() {
            return;
        }

        let scrollback_before = self.max_content_scrollback;
        self.refresh_max_scrollback();
        let scrollback = self.max_content_scrollback;
        let rows = self.get_vt100_size().0 as usize;
        let cursor_row = scrollback + self.vt100_parser.screen().cursor_position().0 as usize;

        let mut stamp_from = cursor_row;
        if started_on_main {
            // rows pushed into scrollback come in at the bottom, evicted ones leave at the top
            let pushed = (scrollback + evicted).saturating_sub(scrollback_before);
            self.row_timestamps
                .extend(std::iter::repeat_n(None, pushed));
            let evicted = evicted.min(self.row_timestamps.len());
            self.row_timestamps.drain(..evicted);
            self.rebase_line_refs(evicted);
            // the row the cursor started on moved up with the evicted rows
            stamp_from = (scrollback_before + cursor_row_before).saturating_sub(evicted);
        }
        // one slot per world row whatever happened above (back from the alternate screen, or a
        // case not tracked), so the range below can't go past the end
        self.row_timestamps.resize(scrollback + rows, None);

        // every row the cursor went through gets this chunk's time, if it has none yet
        let now = Local::now();
        let last_slot = self.row_timestamps.len().saturating_sub(1);
        let first = stamp_from.min(cursor_row).min(last_slot);
        let last = stamp_from.max(cursor_row).min(last_slot);
        for slot in self.row_timestamps.range_mut(first..=last) {
            slot.get_or_insert(now);
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
        let cols = vt100_size(screen).1;
        for c in 0..cols {
            if let Some(cell) = screen.cell(row, c)
                && cell.has_contents()
            {
                return true;
            }
        }
        false
    }

    fn rebase_line_refs(&mut self, evicted: usize) {
        if evicted == 0 {
            return;
        }
        self.search_state.rebase_after_eviction(evicted);
        self.copy_state.rebase_after_eviction(evicted);
    }

    pub fn total_timeline_len(&self) -> usize {
        self.row_timestamps.len()
    }

    pub fn get_timeline_timestamp(&self, global_idx: usize) -> Option<DateTime<Local>> {
        self.row_timestamps.get(global_idx).copied().flatten()
    }

    pub fn clear_history(&mut self) {
        let (rows, cols) = self.get_vt100_size();

        self.vt100_parser = new_parser(rows, cols, self.max_history_lines);
        self.row_timestamps = VecDeque::from(vec![None; rows as usize]);
        self.search_text = None;

        self.max_content_scrollback = 0;
        self.pan_offset_y = 0;

        self.scroller.scroll_to_bottom();
        self.scroller.max_scroll_y = 0;
        self.scroller.offset_x = 0;
        self.scroller.pan_right_explored = 0;
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
        let vt_cols = vt100_size(self.vt100_parser.screen()).1 as usize;
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
        let vt_rows = vt100_size(self.vt100_parser.screen()).0 as usize;
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

    pub fn refresh_max_scrollback(&mut self) {
        self.max_content_scrollback = self.vt100_parser.screen().scrollback_len();
        self.scroller.max_scroll_y = self.max_vertical_scroll();
    }

    pub fn set_vt100_size(&mut self, rows: u16, cols: u16) {
        let (old_rows, old_cols) = self.get_vt100_size();
        if (old_rows, old_cols) == (rows, cols) || rows == 0 || cols == 0 {
            return;
        }

        self.search_text = None;
        let scrollback_before = self.max_content_scrollback;
        let cursor_row_before = self.vt100_parser.screen().cursor_position().0 as usize;
        // the parser's `set_size` (not the screen's) reports the rows it evicts
        self.vt100_parser.set_size(vt100_dim(rows), vt100_dim(cols));
        let evicted = std::mem::take(&mut self.vt100_parser.callbacks_mut().count);
        // in the alternate screen the main rows can't be measured: the next RX on the main
        // screen fixes the slot count (see `process_rx`)
        if self.vt100_parser.screen().alternate_screen() {
            return;
        }
        self.refresh_max_scrollback();
        let scrollback = self.max_content_scrollback;

        // since vt100 0.18 a smaller height pushes the top rows into scrollback (evicting the
        // oldest when it is full) and drops rows at the bottom; a taller one pulls rows back from
        // scrollback and adds rows at the bottom. The world rows keep their order, only the
        // scrollback/screen boundary moves
        if rows < old_rows {
            let evicted = evicted.min(self.row_timestamps.len());
            self.row_timestamps.drain(..evicted);
            self.rebase_line_refs(evicted);
        } else {
            // the cursor moved down by the rows inserted at the top. Those not pulled back from
            // scrollback are blank rows vt100 adds for scrollback it no longer holds (evicted)
            let cursor_row = self.vt100_parser.screen().cursor_position().0 as usize;
            let inserted = cursor_row.saturating_sub(cursor_row_before);
            let pulled = scrollback_before.saturating_sub(scrollback);
            let blank = inserted.saturating_sub(pulled);
            let at = scrollback.min(self.row_timestamps.len());
            for _ in 0..blank {
                self.row_timestamps.insert(at, None);
            }
        }
        self.row_timestamps.resize(scrollback + rows as usize, None);
    }

    /// Return (rows, cols)
    pub fn get_vt100_size(&self) -> (u16, u16) {
        vt100_size(self.vt100_parser.screen())
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
            TerminalAction::ScreenshotBytes => match self.save_screenshot_ansi() {
                Ok(filename) => {
                    self.trigger_flash();
                    UpdateEffect::Screenshot(Ok(filename))
                }
                Err(e) => UpdateEffect::Screenshot(Err(e.to_string())),
            },
            TerminalAction::ScreenshotTxt => match self.save_screenshot_txt() {
                Ok(filename) => {
                    self.trigger_flash();
                    UpdateEffect::Screenshot(Ok(filename))
                }
                Err(e) => UpdateEffect::Screenshot(Err(e.to_string())),
            },
            TerminalAction::Key(key_event) => {
                let application_cursor = self.vt100_screen().application_cursor();
                match encode_terminal_key(key_event, application_cursor) {
                    Some(bytes) => UpdateEffect::Transmit(bytes),
                    None => UpdateEffect::None,
                }
            }
            TerminalAction::Paste(text) => {
                let bracketed = self.vt100_screen().bracketed_paste();
                UpdateEffect::Transmit(terminal_paste_bytes(&text, bracketed))
            }
        }
    }

    pub fn handle_search_action(&mut self, action: SearchModeAction, raw_view: RawSideView) {
        match action {
            SearchModeAction::Start => {
                self.search_state.toggle_itself();
                if self.search_state.active {
                    self.update_search_matches(raw_view);
                }
            }
            SearchModeAction::Next => {
                if self.search_state.active
                    && let Some((row, col)) = self.search_state.next_match()
                {
                    self.copy_state.cursor.line_idx = row;
                    self.copy_state.cursor.char_idx = col;
                    self.copy_state.cursor.preferred_col = col;
                }
            }
            SearchModeAction::Previous => {
                if self.search_state.active
                    && let Some((row, col)) = self.search_state.prev_match()
                {
                    self.copy_state.cursor.line_idx = row;
                    self.copy_state.cursor.char_idx = col;
                    self.copy_state.cursor.preferred_col = col;
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
                    self.handle_input_search(key_event, raw_view);
                }
            }
            SearchModeAction::Paste(text) => {
                if self.search_state.active {
                    paste_into_input(&mut self.search_state.query, &text);
                    self.update_search_matches(raw_view);
                }
            }
            SearchModeAction::Kind => {
                self.search_state.toggle_mode();
                self.update_search_matches(raw_view);
            }
        }
    }

    pub fn handle_copy_action(
        &mut self,
        action: CopyModeAction,
        settings: &SettingsConfig,
    ) -> Option<String> {
        if let CopyModeAction::HandleKey(key) = action {
            let line_text = read_vt100_world_line_text(
                &mut self.vt100_parser,
                self.max_content_scrollback,
                self.copy_state.cursor.line_idx,
            )
            .unwrap_or_default();
            if let Some(sub_action) = dispatch_copy_key_vi(
                &mut self.vi_pending_copy,
                &line_text,
                self.copy_state.cursor.char_idx,
                key,
            ) {
                return self.handle_copy_action(sub_action, settings);
            }
            return None;
        }

        if action == CopyModeAction::Enter {
            let (c_row, c_col) = self.vt100_screen().cursor_position();
            let max_col = self.get_vt100_size().1.saturating_sub(1) as usize;
            self.copy_state.enter_at(
                self.max_content_scrollback + (c_row as usize),
                (c_col as usize).min(max_col),
            );
            self.keep_terminal_copy_cursor_in_view();
            return None;
        }

        // adapter for the TextSource trait
        let mut source = TerminalModeTextSource {
            parser: &mut self.vt100_parser,
            max_content_scrollback: self.max_content_scrollback,
            raw_view: settings.raw_side.view,
            view_height: self.scroller.view_height,
        };

        let result = self.copy_state.apply_action(action, &mut source);
        match result {
            CopyActionResult::Moved => {
                self.keep_terminal_copy_cursor_in_view();
                None
            }
            CopyActionResult::Yanked(text) => Some(text),
            _ => None,
        }
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

    pub fn update_search_matches(&mut self, raw_view: RawSideView) {
        if self.search_text.is_none() {
            self.search_text = Some(self.all_text_lines());
        }
        let text = self.search_text.as_deref().unwrap_or_default();
        self.search_state
            .update_matches_from_strings(text, raw_view);
    }

    pub fn handle_input_search(&mut self, key_event: KeyEvent, raw_view: RawSideView) {
        let before = self.search_state.query.value().to_owned();
        dispatch_input_key(&mut self.search_state.query, key_event);
        if self.search_state.query.value() != before {
            self.update_search_matches(raw_view);
        }
    }

    pub fn handle_side_view_action(&mut self, action: SideViewAction) {
        match action {
            SideViewAction::PanLeft(amount) => {
                self.raw_scroller.pan_left(amount);
            }
            SideViewAction::PanRight(amount) => {
                self.raw_scroller.pan_right(amount);
            }
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

    pub fn flush_logger_if_due(&mut self) {
        let _ = self.logger.flush_if_due();
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

    #[inline]
    pub fn raw_pan_x(&self) -> usize {
        self.raw_scroller.pan_x
    }

    pub fn save_screenshot_txt(&self) -> std::io::Result<String> {
        let timestamp = Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("screen_{}.txt", timestamp);

        let text = self.vt100_parser.screen().contents();
        let mut file = File::create(&filename)?;
        file.write_all(text.as_bytes())?;

        Ok(filename)
    }

    pub fn save_screenshot_ansi(&self) -> std::io::Result<String> {
        let timestamp = Local::now().format("%Y%m%d_%H%M%S");
        let filename = format!("screen_{}.ans", timestamp);

        let contents = self.vt100_parser.screen().contents_formatted();
        let mut file = File::create(&filename)?;
        file.write_all(contents.as_bytes())?;

        Ok(filename)
    }

    pub fn trigger_flash(&mut self) {
        self.flash_until = Some(Instant::now() + Duration::from_millis(120));
    }

    pub fn has_pending_flash(&self) -> bool {
        self.flash_until.is_some()
    }

    pub fn is_flashing(&mut self) -> bool {
        if let Some(until) = self.flash_until {
            if Instant::now() < until {
                return true;
            } else {
                self.flash_until = None;
            }
        }
        false
    }

    pub fn center_raw_pan_on_byte(
        &mut self,
        raw_view: RawSideView,
        usable_panel_width: usize,
        local_byte_offset: usize,
        total_sub_bytes: usize,
    ) {
        self.raw_scroller.center_on_byte(
            raw_view,
            usable_panel_width,
            local_byte_offset,
            total_sub_bytes,
        );
    }

    pub fn sync_raw_pan_with_local_col(
        &mut self,
        raw_view: RawSideView,
        usable_panel_width: usize,
        local_byte_offset: usize,
        total_sub_bytes: usize,
    ) {
        self.raw_scroller.sync_with_cursor(
            raw_view,
            usable_panel_width,
            local_byte_offset,
            total_sub_bytes,
        );
    }

    pub fn center_on_target_match(
        &mut self,
        target_row: usize,
        target_col: usize,
        view_w: usize,
        view_h: usize,
    ) {
        let (grid_rows, grid_cols) = self.get_vt100_size();
        let max_sb = self.max_content_scrollback;
        let total_world_lines = max_sb + (grid_rows as usize);

        if total_world_lines == 0 || target_row >= total_world_lines {
            return;
        }

        let lines_from_bottom = (total_world_lines - 1).saturating_sub(target_row);
        let half_h = (grid_rows as usize).min(view_h) / 2;
        let target_scroll_y = lines_from_bottom.saturating_sub(half_h);

        self.scroller.max_scroll_y = self.max_vertical_scroll();
        self.scroller.scroll_to(target_scroll_y);
        self.reconcile_both_vertical_scroll();

        if view_w > 0 && (grid_cols as usize) > view_w {
            let max_pan_x = (grid_cols as usize).saturating_sub(view_w);
            let half_w = view_w / 2;
            self.scroller.offset_x = target_col.saturating_sub(half_w).min(max_pan_x);
        }
    }
}

// HACK: there is no function to fetch string content from history so sb everywhere
pub fn read_vt100_world_line_text(
    parser: &mut TerminalParser,
    max_sb: usize,
    world_row: usize,
) -> Option<String> {
    // this function is not a member to simlify ownership with TerminalModeTextSource
    let (rows, cols) = vt100_size(parser.screen());
    let total_world = max_sb + rows as usize;

    if world_row >= total_world {
        return None;
    }

    let current_sb = parser.screen().scrollback();

    let (view_row, target_sb) = if world_row < max_sb {
        (0, max_sb - world_row)
    } else {
        ((world_row - max_sb) as u16, 0)
    };

    if current_sb != target_sb {
        parser.screen_mut().set_scrollback(target_sb);
    }

    // vt100's own row text, like search and the selection read it: the gaps a tab or a cursor
    // move leave are spaces, so a char index is a screen column (#33). `rows()` is lazy but builds
    // every row before `view_row`: one row for history (always at the top), a screen at most
    let line_str = parser
        .screen()
        .rows(0, cols)
        .nth(view_row as usize)
        .unwrap_or_default();
    if current_sb != target_sb {
        parser.screen_mut().set_scrollback(current_sb);
    }

    Some(line_str)
}

/// Bytes sent to the remote for a key typed in terminal mode (legacy xterm encoding). When the
/// remote turned on application cursor mode (DECCKM, `ESC[?1h`), unmodified arrows, Home and End
/// are sent as `ESC O x` instead of `ESC [ x`, like xterm does. Modified ones (`ESC[1;5A`) are the
/// same in both modes
fn encode_terminal_key(key_event: KeyEvent, application_cursor: bool) -> Option<Vec<u8>> {
    let key_event = to_terminput_key(key_event).ok()?;
    let mut buf = [0u8; 32];
    let len = terminput::Event::Key(key_event)
        .encode(&mut buf, Encoding::Xterm)
        .ok()?;
    let mut bytes = buf[..len].to_vec();
    if application_cursor
        && let [
            0x1b,
            introducer @ b'[',
            b'A' | b'B' | b'C' | b'D' | b'H' | b'F',
        ] = bytes.as_mut_slice()
    {
        *introducer = b'O';
    }
    Some(bytes)
}

/// Paste sent to the remote in one write. Line breaks are sent as `\r`, like the Enter key, and
/// the text is wrapped in paste markers when the remote asked for them (`ESC[?2004h`)
fn terminal_paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() + 12);
    if bracketed {
        bytes.extend_from_slice(b"\x1b[200~");
    }
    let mut prev_cr = false;
    for &b in text.as_bytes() {
        match b {
            b'\n' if prev_cr => {}
            b'\n' => bytes.push(b'\r'),
            _ => bytes.push(b),
        }
        prev_cr = b == b'\r';
    }
    if bracketed {
        bytes.extend_from_slice(b"\x1b[201~");
    }
    bytes
}

struct TerminalModeTextSource<'a> {
    pub parser: &'a mut TerminalParser,
    pub max_content_scrollback: usize,
    pub raw_view: RawSideView,
    pub view_height: usize,
}

impl<'a> TextSource for TerminalModeTextSource<'a> {
    fn total_lines(&mut self) -> usize {
        let (rows, _) = vt100_size(self.parser.screen());
        self.max_content_scrollback + (rows as usize)
    }

    fn get_line_text(&mut self, line_idx: usize) -> Option<String> {
        read_vt100_world_line_text(self.parser, self.max_content_scrollback, line_idx)
    }

    #[inline]
    fn raw_view(&mut self) -> RawSideView {
        self.raw_view
    }

    fn view_height(&mut self) -> usize {
        self.view_height
    }
}

#[cfg(test)]
mod tests {
    //! Bytes sent in terminal mode: typed keys (#31) and bracketed paste (#28).

    use super::*;
    use crate::configs::TerminalLogConfig;
    use crossterm::event::{KeyCode, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn sent(code: KeyCode, modifiers: KeyModifiers, application_cursor: bool) -> Vec<u8> {
        encode_terminal_key(KeyEvent::new(code, modifiers), application_cursor).unwrap()
    }

    /// Normal cursor mode sends the same bytes as before #31 (captured from the old
    /// `handle_terminal_mode` in inputs.rs).
    /// How: arrows, Home, End, a modified arrow, a char, Enter, F1 and PageUp.
    #[test]
    fn keys_in_normal_cursor_mode() {
        let none = KeyModifiers::NONE;
        assert_eq!(sent(KeyCode::Up, none, false), b"\x1b[A");
        assert_eq!(sent(KeyCode::Down, none, false), b"\x1b[B");
        assert_eq!(sent(KeyCode::Right, none, false), b"\x1b[C");
        assert_eq!(sent(KeyCode::Left, none, false), b"\x1b[D");
        assert_eq!(sent(KeyCode::Home, none, false), b"\x1b[H");
        assert_eq!(sent(KeyCode::End, none, false), b"\x1b[F");
        assert_eq!(
            sent(KeyCode::Up, KeyModifiers::CONTROL, false),
            b"\x1b[1;5A"
        );
        assert_eq!(sent(KeyCode::Char('a'), none, false), b"a");
        assert_eq!(sent(KeyCode::Enter, none, false), b"\r");
        assert_eq!(sent(KeyCode::F(1), none, false), b"\x1bOP");
        assert_eq!(sent(KeyCode::PageUp, none, false), b"\x1b[5~");
    }

    /// Application cursor mode only changes unmodified arrows, Home and End to `ESC O x`.
    /// How: the same keys with the mode on.
    #[test]
    fn keys_in_application_cursor_mode() {
        let none = KeyModifiers::NONE;
        assert_eq!(sent(KeyCode::Up, none, true), b"\x1bOA");
        assert_eq!(sent(KeyCode::Down, none, true), b"\x1bOB");
        assert_eq!(sent(KeyCode::Right, none, true), b"\x1bOC");
        assert_eq!(sent(KeyCode::Left, none, true), b"\x1bOD");
        assert_eq!(sent(KeyCode::Home, none, true), b"\x1bOH");
        assert_eq!(sent(KeyCode::End, none, true), b"\x1bOF");
        assert_eq!(sent(KeyCode::Up, KeyModifiers::CONTROL, true), b"\x1b[1;5A");
        assert_eq!(sent(KeyCode::Char('a'), none, true), b"a");
        assert_eq!(sent(KeyCode::PageUp, none, true), b"\x1b[5~");
    }

    fn terminal(rows: u16) -> TerminalMode {
        TerminalMode::new(
            100,
            rows,
            80,
            TerminalLogger::new(TerminalLogConfig::default()),
        )
    }

    /// Feeds one line per chunk, 1 ms apart so every row gets a distinct timestamp.
    fn feed_lines(tm: &mut TerminalMode, lines: &[&str]) {
        for (i, line) in lines.iter().enumerate() {
            let sep = if i + 1 < lines.len() { "\r\n" } else { "" };
            tm.process_rx(TextEncoding::Utf8, format!("{line}{sep}").as_bytes());
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Timestamps of every world row, history first (top to bottom).
    fn timeline(tm: &TerminalMode) -> Vec<Option<DateTime<Local>>> {
        (0..tm.total_timeline_len())
            .map(|i| tm.get_timeline_timestamp(i))
            .collect()
    }

    fn terminal_with(history: usize, rows: u16) -> TerminalMode {
        TerminalMode::new(
            history,
            rows,
            80,
            TerminalLogger::new(TerminalLogConfig::default()),
        )
    }

    /// Feeds each line as its own chunk, 1 ms apart, and returns each line's text with the
    /// timestamp of the row the cursor is on right after it arrived.
    fn feed_and_stamp(
        tm: &mut TerminalMode,
        lines: &[&str],
    ) -> Vec<(String, Option<DateTime<Local>>)> {
        let mut stamps = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let sep = if i == 0 { "" } else { "\r\n" };
            tm.process_rx(TextEncoding::Utf8, format!("{sep}{line}").as_bytes());
            let cursor_world =
                tm.get_max_scrollback() + tm.vt100_screen().cursor_position().0 as usize;
            stamps.push((line.to_string(), tm.get_timeline_timestamp(cursor_world)));
            std::thread::sleep(Duration::from_millis(1));
        }
        stamps
    }

    /// Checks every world row with text has the timestamp its line got when it arrived.
    fn assert_rows_keep_their_stamps(
        tm: &mut TerminalMode,
        stamps: &[(String, Option<DateTime<Local>>)],
    ) {
        let world = tm.get_max_scrollback() + tm.get_vt100_size().0 as usize;
        assert_eq!(
            tm.total_timeline_len(),
            world,
            "one timestamp slot per world row"
        );
        for row in 0..world {
            let text =
                read_vt100_world_line_text(&mut tm.vt100_parser, tm.max_content_scrollback, row)
                    .unwrap();
            if let Some((_, ts)) = stamps.iter().find(|(line, _)| *line == text.trim_end()) {
                assert!(ts.is_some(), "{text:?} was never stamped");
                assert_eq!(tm.get_timeline_timestamp(row), *ts, "row {row} {text:?}");
            }
        }
    }

    /// Text of a world row, without the trailing spaces.
    fn row_text(tm: &mut TerminalMode, row: usize) -> String {
        read_vt100_world_line_text(&mut tm.vt100_parser, tm.max_content_scrollback, row)
            .unwrap_or_default()
            .trim_end()
            .to_string()
    }

    /// Search text is built once and reused while typing, and rebuilt after anything that
    /// changes the terminal's text: RX, resize, clear (#12).
    /// How: count the matches of "hit" before and after each of those.
    #[test]
    fn search_text_follows_terminal_changes() {
        let mut tm = terminal(3);
        feed_lines(&mut tm, &["hit", "miss"]);
        tm.search_state.query = tui_input::Input::new("hit".to_string());
        let matches = |tm: &mut TerminalMode| {
            tm.update_search_matches(RawSideView::default());
            tm.search_state.matches.len()
        };
        assert_eq!(matches(&mut tm), 1);
        assert_eq!(matches(&mut tm), 1);

        tm.process_rx(TextEncoding::Utf8, b"\r\nhit again");
        assert_eq!(matches(&mut tm), 2);

        // a shrink of 3 cols cuts "hit again" to "hit": still 2 rows, and nothing stale
        tm.set_vt100_size(3, 3);
        assert_eq!(matches(&mut tm), 2);

        tm.clear_history();
        assert_eq!(matches(&mut tm), 0);
    }

    /// Once scrollback is full each new row evicts the oldest: the copy cursor, the selection
    /// anchor and the search matches stay on their rows instead of sliding (#34).
    /// How: 3 rows of history, 2 on screen, cursor and anchor on "l4", a match on "l5", then
    /// 2 more lines; then enough lines to evict the match's row too.
    #[test]
    fn line_refs_follow_rows_when_scrollback_is_full() {
        let mut tm = terminal_with(3, 2);
        feed_and_stamp(&mut tm, &["l0", "l1", "l2", "l3", "l4", "l5", "l6"]);
        let l4 = (0..5).find(|&r| row_text(&mut tm, r) == "l4").unwrap();
        tm.copy_state.enter_at(l4, 1);
        tm.copy_state.selection_anchor = Some(tm.copy_state.cursor);
        tm.search_state.query = tui_input::Input::new("l5".to_string());
        tm.update_search_matches(RawSideView::default());
        assert_eq!(tm.search_state.matches.len(), 1);

        tm.process_rx(TextEncoding::Utf8, b"\r\nl7\r\nl8");
        let cursor_row = tm.copy_state.cursor.line_idx;
        assert_eq!(row_text(&mut tm, cursor_row), "l4");
        assert_eq!(tm.copy_state.cursor.char_idx, 1);
        let anchor_row = tm.copy_state.selection_anchor.unwrap().line_idx;
        assert_eq!(row_text(&mut tm, anchor_row), "l4");
        let match_row = tm.search_state.matches[0].line_idx;
        assert_eq!(row_text(&mut tm, match_row), "l5");

        // "l5" and "l4" scroll out of the history: the match goes, the cursor stays on row 0
        tm.process_rx(TextEncoding::Utf8, b"\r\nl9\r\nl10\r\nl11");
        assert!(tm.search_state.matches.is_empty());
        assert_eq!(tm.copy_state.cursor.line_idx, 0);
    }

    /// One chunk with several lines stamps every row it writes, the first one included (#13).
    /// How: "a", "b", "c" in a single chunk on a 5 row screen; the 2 rows below stay unstamped.
    #[test]
    fn one_chunk_stamps_every_row_it_writes() {
        let mut tm = terminal(5);
        tm.process_rx(TextEncoding::Utf8, b"a\r\nb\r\nc");
        let timeline = timeline(&tm);
        assert!(timeline[..3].iter().all(Option::is_some), "{timeline:?}");
        assert!(timeline[3..].iter().all(Option::is_none), "{timeline:?}");
    }

    /// Once scrollback is full, every new row drops the oldest one: timestamps stay on their
    /// rows instead of drifting (#13).
    /// How: 3 rows of history, 2 on screen, 8 lines one chunk each.
    #[test]
    fn timestamps_follow_rows_with_full_scrollback() {
        let mut tm = terminal_with(3, 2);
        let stamps = feed_and_stamp(&mut tm, &["l0", "l1", "l2", "l3", "l4", "l5", "l6", "l7"]);
        assert_eq!(tm.get_max_scrollback(), 3);
        assert_rows_keep_their_stamps(&mut tm, &stamps);
    }

    /// Without scrollback, rows scrolled off the top take their timestamps with them (#13).
    /// How: `history_size = 0`, 2 rows, 5 lines.
    #[test]
    fn timestamps_follow_rows_without_scrollback() {
        let mut tm = terminal_with(0, 2);
        let stamps = feed_and_stamp(&mut tm, &["l0", "l1", "l2", "l3", "l4"]);
        assert_eq!(tm.get_max_scrollback(), 0);
        assert_rows_keep_their_stamps(&mut tm, &stamps);
    }

    /// `CSI 3 J` clears scrollback: its timestamps go too, the screen keeps its own (#13).
    /// How: 6 lines on 3 rows, then `ESC[3J`.
    #[test]
    fn clear_scrollback_drops_its_timestamps() {
        let mut tm = terminal(3);
        let stamps = feed_and_stamp(&mut tm, &["l0", "l1", "l2", "l3", "l4", "l5"]);
        tm.process_rx(TextEncoding::Utf8, b"\x1b[3J");
        assert_eq!(tm.get_max_scrollback(), 0);
        assert_rows_keep_their_stamps(&mut tm, &stamps);
    }

    /// The alternate screen (vim, less) never touches the main screen's timestamps (#13).
    /// How: 4 lines, then 10 lines in the alternate screen, then back.
    #[test]
    fn alternate_screen_keeps_main_timestamps() {
        let mut tm = terminal(3);
        let stamps = feed_and_stamp(&mut tm, &["l0", "l1", "l2", "l3"]);
        let before = timeline(&tm);
        tm.process_rx(TextEncoding::Utf8, b"\x1b[?1049h");
        for i in 0..10 {
            tm.process_rx(TextEncoding::Utf8, format!("alt{i}\r\n").as_bytes());
        }
        tm.process_rx(TextEncoding::Utf8, b"\x1b[?1049l");
        assert_eq!(timeline(&tm), before);
        assert_rows_keep_their_stamps(&mut tm, &stamps);
    }

    /// The max scrollback is the number of rows scrolled off, read with `scrollback_len` instead
    /// of the old set/read/restore offset trick, and the view offset is left alone (#29).
    /// How: 6 lines on a 3 row screen, scrolled back by 1.
    #[test]
    fn max_scrollback_counts_scrolled_rows() {
        let mut tm = terminal(3);
        feed_lines(&mut tm, &["1", "2", "3", "4", "5", "6"]);
        assert_eq!(tm.get_max_scrollback(), 3);
        tm.vt100_parser.screen_mut().set_scrollback(1);
        tm.refresh_max_scrollback();
        assert_eq!(tm.get_max_scrollback(), 3);
        assert_eq!(tm.vt100_screen().scrollback(), 1);
    }

    /// vt100 0.18 pushes the top rows into scrollback when the height shrinks (cursor at the
    /// bottom) and pulls them back when it grows: the timestamps follow their rows (#29).
    /// How: 4 lines on 4 rows, shrink to 2, then grow back to 4, comparing the timestamps of
    /// the whole world (history + screen) with the ones before.
    #[test]
    fn resize_moves_timestamps_with_rows() {
        let mut tm = terminal(4);
        feed_lines(&mut tm, &["a", "b", "c", "d"]);
        let before = timeline(&tm);
        assert_eq!(before.len(), 4);
        assert!(before.iter().all(Option::is_some));

        tm.set_vt100_size(2, 80);
        assert_eq!(tm.get_max_scrollback(), 2);
        assert_eq!(timeline(&tm), before);

        tm.set_vt100_size(4, 80);
        assert_eq!(tm.get_max_scrollback(), 0);
        assert_eq!(timeline(&tm), before);
    }

    /// Shrinking with a full scrollback pushes the top rows in and evicts as many of the oldest:
    /// timestamps leave from the top, not the bottom.
    /// How: 3 rows of history, 4 on screen, 8 lines (scrollback full), shrink to 2.
    #[test]
    fn shrink_with_full_scrollback_evicts_oldest_timestamps() {
        let mut tm = terminal_with(3, 4);
        let lines = ["l0", "l1", "l2", "l3", "l4", "l5", "l6", "l7"];
        let stamps = feed_and_stamp(&mut tm, &lines);
        assert_eq!(tm.get_max_scrollback(), 3);
        tm.set_vt100_size(2, 80);
        assert_eq!(tm.get_max_scrollback(), 3);
        assert_rows_keep_their_stamps(&mut tm, &stamps);
    }

    /// Growing past what scrollback still holds makes vt100 insert blank rows at the top (for the
    /// rows it evicted): they get empty slots there, the other rows keep theirs (#13b).
    /// How: 3 rows of history, 2 on screen, 8 lines, grow to 7 (3 rows pulled back, 2 blank).
    #[test]
    fn grow_past_scrollback_inserts_blank_rows_at_top() {
        let mut tm = terminal_with(3, 2);
        let lines = ["l0", "l1", "l2", "l3", "l4", "l5", "l6", "l7"];
        let stamps = feed_and_stamp(&mut tm, &lines);
        tm.set_vt100_size(7, 80);
        assert_eq!(tm.get_max_scrollback(), 0);
        assert_eq!(timeline(&tm)[..2], [None, None]);
        assert_rows_keep_their_stamps(&mut tm, &stamps);
    }

    /// Without scrollback every row inserted at the top on grow is blank (#13b).
    /// How: `history_size = 0`, 2 rows, 5 lines, grow to 4.
    #[test]
    fn grow_without_scrollback_inserts_blank_rows_at_top() {
        let mut tm = terminal_with(0, 2);
        let stamps = feed_and_stamp(&mut tm, &["l0", "l1", "l2", "l3", "l4"]);
        tm.set_vt100_size(4, 80);
        assert_eq!(timeline(&tm)[..2], [None, None]);
        assert_rows_keep_their_stamps(&mut tm, &stamps);
    }

    /// With the cursor at the top, shrinking drops rows from the bottom (nothing goes to
    /// scrollback), like before 0.18: the bottom timestamps are dropped.
    /// How: 1 line on 4 rows, shrink to 2.
    #[test]
    fn resize_with_cursor_at_top_drops_bottom_rows() {
        let mut tm = terminal(4);
        feed_lines(&mut tm, &["a"]);
        let before = timeline(&tm);
        tm.set_vt100_size(2, 80);
        assert_eq!(tm.get_max_scrollback(), 0);
        assert_eq!(timeline(&tm), before[..2]);
    }

    /// Copy mode reads a row like vt100 shows it: the gaps a tab or a cursor move leave are
    /// spaces, so the copy cursor's char index is the screen column search and the selection use.
    /// How: "a\\tb" and "ESC[5C" then "c", read from the screen and once scrolled into history.
    #[test]
    fn copy_mode_row_text_keeps_gaps() {
        let mut tm = terminal(3);
        tm.process_rx(TextEncoding::Utf8, b"a\tb\r\n\x1b[5Cc");
        let read = |tm: &mut TerminalMode, row| {
            read_vt100_world_line_text(&mut tm.vt100_parser, tm.max_content_scrollback, row)
        };
        assert_eq!(read(&mut tm, 0).as_deref(), Some("a       b"));
        assert_eq!(read(&mut tm, 1).as_deref(), Some("     c"));

        tm.process_rx(TextEncoding::Utf8, b"\r\n\r\n\r\n");
        assert_eq!(tm.get_max_scrollback(), 2);
        assert_eq!(read(&mut tm, 0).as_deref(), Some("a       b"));
        assert_eq!(read(&mut tm, 1).as_deref(), Some("     c"));
    }

    /// The mode follows what the remote sends: `ESC[?1h` turns it on, `ESC[?1l` off.
    /// How: an Up key through `update_action_terminal` before, during and after the mode.
    #[test]
    fn cursor_mode_follows_remote() {
        let mut tm = TerminalMode::new(
            100,
            24,
            80,
            TerminalLogger::new(TerminalLogConfig::default()),
        );
        let up = |tm: &mut TerminalMode| match tm
            .update_action_terminal(TerminalAction::Key(key(KeyCode::Up)))
        {
            UpdateEffect::Transmit(bytes) => bytes,
            _ => panic!("expected a transmit"),
        };
        assert_eq!(up(&mut tm), b"\x1b[A");
        tm.process_rx(TextEncoding::Utf8, b"\x1b[?1h");
        assert_eq!(up(&mut tm), b"\x1bOA");
        tm.process_rx(TextEncoding::Utf8, b"\x1b[?1l");
        assert_eq!(up(&mut tm), b"\x1b[A");
    }

    /// LF, CRLF and CR all become one `\r`, like pressing Enter.
    /// How: one paste with each kind of line break.
    #[test]
    fn terminal_paste_line_breaks_become_cr() {
        assert_eq!(terminal_paste_bytes("a\nb\r\nc\rd", false), b"a\rb\rc\rd");
    }

    /// The paste markers are only added when the remote enabled bracketed paste.
    /// How: same text with the remote flag off then on.
    #[test]
    fn terminal_paste_markers_follow_remote_mode() {
        assert_eq!(terminal_paste_bytes("ls", false), b"ls");
        assert_eq!(terminal_paste_bytes("ls", true), b"\x1b[200~ls\x1b[201~");
    }

    /// Non-ASCII text is sent as its UTF-8 bytes, like typed chars.
    /// How: a paste with an accented char.
    #[test]
    fn terminal_paste_keeps_utf8() {
        assert_eq!(terminal_paste_bytes("é", false), "é".as_bytes());
    }

    /// MAX-72: keys that only move the query cursor don't re-run the search (same as line
    /// mode); a key that changes the query does.
    #[test]
    fn query_cursor_keys_keep_the_current_match() {
        let mut tm = terminal(3);
        feed_lines(&mut tm, &["hit a", "hit b"]);
        tm.search_state.active = true;
        tm.search_state.query = tui_input::Input::new("hit".to_string());
        tm.update_search_matches(RawSideView::default());
        tm.search_state.next_match();
        tm.search_state.target_jump = None;

        tm.handle_input_search(key(KeyCode::Left), RawSideView::default());
        assert_eq!(tm.search_state.current_match, 1);
        assert_eq!(tm.search_state.target_jump, None);

        tm.handle_input_search(key(KeyCode::Right), RawSideView::default());
        tm.handle_input_search(key(KeyCode::Char(' ')), RawSideView::default());
        assert_eq!(tm.search_state.current_match, 0);
        assert!(tm.search_state.target_jump.is_some());
    }
}
