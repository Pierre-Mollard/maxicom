use chrono::{DateTime, Local};
use crossterm::event::KeyEvent;
use std::{collections::VecDeque, io, path::PathBuf};
use tui_input::Input;

use crate::{
    app::{
        UpdateEffect,
        data_processing::{
            build_display_to_raw_byte_map, entry_display_text, entry_display_to_wire_map,
        },
        raw_scroller::RawSideScroller,
        scroller::{ScrollStatus, ViewportScroller},
        search::SearchState,
        selection::{CopyActionResult, CopyModeState, TextSource},
        types::{LogEntry, ViewportSize},
    },
    configs::{
        LineDisplayMode, RawSideView, RxNewlineMode, SettingsConfig, TextEncoding, WrapMode,
    },
    inputs::{
        CopyModeAction, LineInputAction, SearchModeAction, SideViewAction, ViMode, ViState,
        dispatch_input_key, dispatch_input_key_vi, local_dispatchers::dispatch_copy_key_vi,
        paste_into_input, vi::ViPending,
    },
    logger::LineLogger,
};

pub struct LineMode {
    /// Completed lines of device output (scrollback) String because already processed
    local_history: VecDeque<LogEntry>,
    /// Completed lines of device input sent
    local_send_history: VecDeque<String>,
    /// None = active editing, Some(i) browsing #i history line
    history_index: Option<usize>,
    /// In-progress line when user press up
    temp_input: Input,
    /// Max line history
    max_lines: usize,
    /// Max sent lines kept for history up/down
    max_send_lines: usize,
    /// Current line being received from device output as bytes
    partial_line: Vec<u8>,
    partial_line_timestamp: Option<DateTime<Local>>,
    /// Current line being typed by user as futur device input
    user_input: Input,
    vi_state_prompt: ViState,
    /// Waiting to receive carriage return
    pending_carriage_return: bool,
    logger: LineLogger,
    search_state: SearchState,
    copy_state: CopyModeState,
    scroller: ViewportScroller,
    /// Rows of the scrolled-to entry (`scroller.offset_y` entries from the bottom) hidden below
    /// the view: with it, scrolling moves by rows through entries taller than the view (#36)
    rows_below: usize,
    /// Rows to scroll (up > 0), applied by the render with the wrapped layout it computes
    pending_scroll_rows: isize,
    /// Rows each history entry takes on screen, aligned with `local_history` (0 = not counted
    /// yet), counted by the render for the scrollbar and the scroll status (#5d)
    row_heights: VecDeque<u32>,
    /// What `row_heights` were counted for (view width, wrap and display modes...): the render
    /// gives a new key when that changes, which resets every height to "not counted"
    row_heights_key: u64,
    /// Scroll position in rows, from the render (#5d)
    row_scroll_status: ScrollStatus,
    raw_scroller: RawSideScroller,
    vi_state_copymode: Option<ViPending>,
    evicted_lines: usize,
}

impl LineMode {
    pub fn new(max_history_lines: usize, max_send_lines: usize, logger: LineLogger) -> Self {
        let search_state = SearchState::default();
        let copy_state = CopyModeState::default();
        let scroller = ViewportScroller::default();
        let raw_scroller = RawSideScroller::default();
        LineMode {
            // grown on demand, the configured size is only a limit
            local_history: VecDeque::new(),
            local_send_history: VecDeque::new(),
            history_index: None,
            temp_input: Input::default(),
            max_lines: max_history_lines,
            max_send_lines,
            partial_line: Vec::new(),
            user_input: Input::default(),
            vi_state_prompt: ViState::default(),
            partial_line_timestamp: None,
            pending_carriage_return: false,
            logger,
            search_state,
            copy_state,
            scroller,
            rows_below: 0,
            pending_scroll_rows: 0,
            row_heights: VecDeque::new(),
            row_heights_key: 0,
            row_scroll_status: ScrollStatus::default(),
            raw_scroller,
            vi_state_copymode: None,
            evicted_lines: 0,
        }
    }

    #[inline]
    pub fn input(&self) -> &Input {
        &self.user_input
    }

    pub fn partial_line_timestamp(&self) -> Option<DateTime<Local>> {
        self.partial_line_timestamp
    }

    pub fn feed_rx(
        &mut self,
        rx_eol_mode: RxNewlineMode,
        encoding: TextEncoding,
        timestamp_f: &str,
        bytes: &[u8],
    ) {
        let now = Local::now();
        for &b in bytes {
            if self.partial_line.is_empty() {
                self.partial_line_timestamp = Some(now);
            }

            match b {
                b'\r' => match rx_eol_mode {
                    RxNewlineMode::CrLf | RxNewlineMode::Auto => {
                        self.pending_carriage_return = true;
                        self.partial_line.push(b'\r');
                    }
                    RxNewlineMode::LfOnly => {
                        self.partial_line.push(b'\r');
                    }
                },
                b'\n' => {
                    self.partial_line.push(b'\n');
                    self.pending_carriage_return = false;
                    self.commit_partial_line(encoding, timestamp_f);
                }
                byte => {
                    if self.pending_carriage_return {
                        self.pending_carriage_return = false;
                        self.partial_line.clear();
                        self.partial_line_timestamp = Some(now);
                    }
                    self.partial_line.push(byte);
                }
            }
        }

        self.rebase_line_refs();
    }

    fn commit_partial_line(&mut self, encoding: TextEncoding, timestamp_f: &str) {
        let line = encoding.decode(&self.partial_line);
        let timestamp = self.take_line_timestamp();
        let entry = LogEntry::received(line, &self.partial_line, timestamp);
        self.push_entry(entry, timestamp_f);
        self.partial_line.clear();
    }

    pub fn push_history(&mut self, line: String, timestamp_f: &str) {
        let entry = LogEntry::new(line, self.take_line_timestamp());
        self.push_entry(entry, timestamp_f);
        self.rebase_line_refs();
    }

    fn take_line_timestamp(&mut self) -> DateTime<Local> {
        self.partial_line_timestamp
            .take()
            .unwrap_or_else(Local::now)
    }

    /// Push without rebasing line indexes, the caller rebases once for a whole batch
    fn push_entry(&mut self, entry: LogEntry, timestamp_f: &str) {
        let _ = self.logger.log_entry(&entry, timestamp_f);
        self.local_history.push_back(entry);
        self.row_heights.push_back(0);

        // A view scrolled up (by entries, by rows of the last entry, or by a scroll not drawn
        // yet) keeps showing the same rows: its position counts from the bottom, so it moves up
        // by the new entry. Only the live view follows the new lines (MAX-124)
        let is_scrolled =
            self.scroller.offset_y > 0 || self.rows_below > 0 || self.pending_scroll_rows > 0;
        if is_scrolled {
            self.scroller.offset_y = self.scroller.offset_y.saturating_add(1);
        }
        self.scroller.on_rx_received();
        while self.local_history.len() > self.max_lines {
            self.local_history.pop_front();
            self.row_heights.pop_front();
            self.evicted_lines += 1;
        }
    }

    fn rebase_line_refs(&mut self) {
        let evicted = std::mem::take(&mut self.evicted_lines);
        if evicted == 0 {
            return;
        }
        self.search_state.rebase_after_eviction(evicted);
        self.copy_state.rebase_after_eviction(evicted);
    }

    pub fn clear_history(&mut self) {
        self.local_history.clear();
        self.row_heights.clear();
    }

    /// Read-only slice/iterator of completed lines
    #[inline]
    pub fn history(&self) -> &VecDeque<LogEntry> {
        &self.local_history
    }

    /// Read-only view of current incoming RX slice
    #[inline]
    pub fn partial_rx(&self) -> &[u8] {
        &self.partial_line
    }

    pub fn history_up(&mut self) {
        if self.local_send_history.is_empty() {
            return;
        }

        match self.history_index {
            None => {
                self.temp_input = self.user_input.clone();
                let last = self.local_send_history.len() - 1;
                self.history_index = Some(last);
                let text = self.local_send_history[last].clone();
                self.user_input = Input::new(text);
            }
            Some(i) if i > 0 => {
                let next = i - 1;
                self.history_index = Some(next);
                let text = self.local_send_history[next].clone();
                self.user_input = Input::new(text);
            }
            _ => {}
        }
    }

    pub fn history_down(&mut self) {
        match self.history_index {
            Some(i) if i + 1 < self.local_send_history.len() => {
                let next = i + 1;
                self.history_index = Some(next);
                let text = self.local_send_history[next].clone();
                self.user_input = Input::new(text);
            }
            Some(_) => {
                self.history_index = None;
                self.user_input = std::mem::take(&mut self.temp_input);
            }
            None => {}
        }
    }

    pub fn update_action_lineinput(
        &mut self,
        settings: &SettingsConfig,
        action: LineInputAction,
    ) -> UpdateEffect {
        match action {
            LineInputAction::Submit => {
                self.scroller.scroll_to_bottom();

                let text = self.user_input.value().to_string();
                self.user_input.reset();
                self.temp_input.reset();
                self.history_index = None;

                if !text.is_empty() && self.local_send_history.back() != Some(&text) {
                    self.local_send_history.push_back(text.clone());
                    if self.local_send_history.len() > self.max_send_lines {
                        self.local_send_history.pop_front();
                    }
                }

                if settings.local_echo {
                    self.push_history(text.clone(), &settings.timestamp_format);
                }

                // consumming string!
                let mut bytes = text.into_bytes();
                bytes.extend_from_slice(settings.line_mode.tx_eol.as_bytes());

                UpdateEffect::Transmit(bytes)
            }
            LineInputAction::HistoryUp => {
                self.history_up();
                UpdateEffect::None
            }
            LineInputAction::HistoryDown => {
                self.history_down();
                UpdateEffect::None
            }
            LineInputAction::AutoResizeRemoteToWidth => {
                let gutter = settings.line_gutter_width();
                let cols = self.scroller.view_width.saturating_sub(gutter).max(1);
                let cmd = format!("stty cols {cols}; export COLUMNS={cols}\n");
                UpdateEffect::Transmit(cmd.into_bytes())
            }
            LineInputAction::ClearLine => {
                self.user_input.reset();
                UpdateEffect::None
            }
            LineInputAction::HandleKey(key_event) => {
                self.history_index = None;
                dispatch_input_key_vi(&mut self.user_input, &mut self.vi_state_prompt, key_event);
                UpdateEffect::None
            }
            LineInputAction::Paste(text) => {
                self.history_index = None;
                let text = text.replace("\r\n", "\n");
                let mut lines = text.split(['\r', '\n']);
                let mut current = lines.next().unwrap_or_default();
                let mut tx = Vec::new();
                for next in lines {
                    paste_into_input(&mut self.user_input, current);
                    if let UpdateEffect::Transmit(bytes) =
                        self.update_action_lineinput(settings, LineInputAction::Submit)
                    {
                        tx.extend_from_slice(&bytes);
                    }
                    current = next;
                }
                paste_into_input(&mut self.user_input, current);
                if tx.is_empty() {
                    UpdateEffect::None
                } else {
                    UpdateEffect::Transmit(tx)
                }
            }
        }
    }

    pub fn handle_copy_action(
        &mut self,
        action: CopyModeAction,
        settings: &SettingsConfig,
    ) -> Option<String> {
        let display_mode = settings.line_mode.display_mode;
        let wrap_mode = settings.line_mode.wrap_mode;
        let gutter_width = settings.line_gutter_width();

        if let CopyModeAction::HandleKey(key) = action {
            let display_text = self
                .local_history
                .get(self.copy_state.cursor.line_idx)
                .map(|e| entry_display_text(e, display_mode))
                .unwrap_or_default();
            if let Some(sub_action) = dispatch_copy_key_vi(
                &mut self.vi_state_copymode,
                &display_text,
                self.copy_state.cursor.char_idx,
                key,
            ) {
                return self.handle_copy_action(sub_action, settings);
            }
            return None;
        }

        if action == CopyModeAction::Enter {
            let max_line = self.local_history.len().saturating_sub(1);
            self.copy_state.enter_at(max_line, 0);
            self.sync_scroll_with_cursor();
            return None;
        }

        // adapter for the TextSource trait
        let mut source = LineModeTextSource {
            history: &self.local_history,
            display_mode,
            raw_view: settings.raw_side.view,
            view_height: self.scroller.view_height,
        };

        let result = self.copy_state.apply_action(action, &mut source);
        match result {
            CopyActionResult::Moved => {
                self.sync_scroll_with_cursor();

                if wrap_mode == WrapMode::Truncate {
                    self.sync_pan_with_cursor(display_mode, gutter_width);
                }
                None
            }
            CopyActionResult::Yanked(text) => Some(text),
            _ => None,
        }
    }

    /// Scrolls to the cursor's entry. While in copy mode the render centers the view on the
    /// cursor whatever this offset is; it decides where the view stays once copy mode is left.
    /// It used to subtract half the view height (rows) from an entry count, which hit 0 with
    /// tall entries near the bottom (MAX-123)
    fn sync_scroll_with_cursor(&mut self) {
        let total_lines = self.local_history.len();
        if total_lines == 0 {
            return;
        }
        let line_from_bottom = (total_lines - 1).saturating_sub(self.copy_state.cursor.line_idx);
        self.scroll_to(line_from_bottom);
    }

    fn sync_pan_with_cursor(&mut self, display_mode: LineDisplayMode, gutter_width: usize) {
        let view_w = self.scroller.view_width;
        if view_w == 0 {
            return;
        }

        let cursor_char_idx = self.copy_state.cursor.char_idx;
        let content_width =
            if let Some(entry) = self.local_history.get(self.copy_state.cursor.line_idx) {
                let display_text = entry_display_text(entry, display_mode);
                let chars_before: String = display_text.chars().take(cursor_char_idx).collect();
                unicode_width::UnicodeWidthStr::width(chars_before.as_str())
            } else {
                cursor_char_idx
            };

        let total_cursor_col = gutter_width + content_width;

        if total_cursor_col < self.scroller.offset_x + gutter_width {
            self.scroller.offset_x = total_cursor_col.saturating_sub(gutter_width);
        } else if total_cursor_col >= self.scroller.offset_x + view_w {
            self.scroller.offset_x = total_cursor_col - view_w + 1;
        }

        self.scroller.offset_x = self.scroller.offset_x.min(self.scroller.max_pan_x);
    }

    pub fn update_search_matches(&mut self, display_mode: LineDisplayMode, raw_view: RawSideView) {
        self.search_state
            .update_matches_from_entries(&self.local_history, display_mode, raw_view);
    }

    pub fn handle_search_action(&mut self, action: SearchModeAction, settings: &SettingsConfig) {
        let display_mode = settings.line_mode.display_mode;
        let raw_view = settings.raw_side.view;
        match action {
            SearchModeAction::Start => {
                self.search_state.toggle_itself();
                if self.search_state.active {
                    self.update_search_matches(display_mode, raw_view);
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
                    self.handle_input_search(key_event, display_mode, raw_view);
                }
            }
            SearchModeAction::Paste(text) => {
                if self.search_state.active {
                    paste_into_input(&mut self.search_state.query, &text);
                    self.update_search_matches(display_mode, raw_view);
                }
            }
            SearchModeAction::Kind => {
                self.search_state.toggle_mode();
                self.update_search_matches(display_mode, raw_view);
            }
        }
    }

    pub fn handle_input_search(
        &mut self,
        key_event: KeyEvent,
        display_mode: LineDisplayMode,
        raw_view: RawSideView,
    ) {
        let before = self.search_state.query.value().to_owned();
        dispatch_input_key(&mut self.search_state.query, key_event);
        if self.search_state.query.value() != before {
            self.update_search_matches(display_mode, raw_view);
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
    pub fn is_prompt_vi(&self) -> bool {
        self.vi_state_prompt.mode == ViMode::Normal
    }

    pub fn update_viewport(&mut self, wrap_mode: WrapMode, viewport: ViewportSize) {
        self.scroller.update_view_size(viewport.cols, viewport.rows);
        if wrap_mode == WrapMode::Word || wrap_mode == WrapMode::Char {
            self.scroller.offset_x = 0;
        }
    }

    pub fn update_limits(&mut self, max_scroll: usize, max_pan: usize) {
        self.scroller.update_limits(max_scroll, max_pan);
    }

    pub fn update_raw_limits(&mut self, amount: usize) {
        self.raw_scroller.max_pan_x = amount;
    }

    #[inline]
    pub fn scroll_offset(&self) -> usize {
        self.scroller.offset_y
    }
    #[inline]
    pub fn x_offset(&self) -> usize {
        self.scroller.offset_x
    }
    #[inline]
    pub fn can_pan_left(&self) -> Option<usize> {
        self.scroller.can_pan_left()
    }
    #[inline]
    pub fn can_pan_right(&self) -> Option<(usize, usize)> {
        self.scroller.can_pan_right()
    }
    #[inline]
    pub fn is_new_content_missed(&self) -> bool {
        self.scroller.new_content_missed
    }

    #[inline]
    pub fn scroll_up(&mut self, rows: usize) {
        self.pending_scroll_rows = self.pending_scroll_rows.saturating_add_unsigned(rows);
    }
    #[inline]
    pub fn scroll_down(&mut self, rows: usize) {
        self.pending_scroll_rows = self.pending_scroll_rows.saturating_sub_unsigned(rows);
    }
    #[inline]
    pub fn scroll_up_page(&mut self) {
        self.scroll_up(self.scroller.view_height.saturating_sub(1));
    }
    #[inline]
    pub fn scroll_down_page(&mut self) {
        self.scroll_down(self.scroller.view_height.saturating_sub(1));
    }
    #[inline]
    pub fn scroll_to_bottom(&mut self) {
        self.scroller.scroll_to_bottom();
        self.rows_below = 0;
        self.pending_scroll_rows = 0;
    }
    /// Every row of the first entry hidden below: the render clamps that to the first row of
    /// the history at the top of the view
    #[inline]
    pub fn scroll_to_top(&mut self) {
        self.scroller.scroll_to_top();
        self.rows_below = usize::MAX;
        self.pending_scroll_rows = 0;
    }
    /// Scrolls to the last row of the entry `lines` entries from the bottom
    #[inline]
    pub fn scroll_to(&mut self, lines: usize) {
        self.scroller.scroll_to(lines);
        self.rows_below = 0;
        self.pending_scroll_rows = 0;
    }
    #[inline]
    pub fn rows_below(&self) -> usize {
        self.rows_below
    }
    /// Rows the user scrolled since the last frame, for the render to apply
    #[inline]
    pub fn take_pending_scroll_rows(&mut self) -> isize {
        std::mem::take(&mut self.pending_scroll_rows)
    }
    /// Where the render resolved the scroll to
    pub fn set_scroll_position(&mut self, entries_from_bottom: usize, rows_below: usize) {
        self.scroller.scroll_to(entries_from_bottom);
        self.rows_below = rows_below;
    }
    /// Scroll position in rows (scrollbar, `Bot` / `%`, `SCROLL +n`), as the last frame
    /// resolved it (#5d)
    pub fn scroll_status(&self) -> ScrollStatus {
        self.row_scroll_status
    }
    pub fn set_scroll_status(&mut self, status: ScrollStatus) {
        self.row_scroll_status = status;
    }
    /// The entry heights for the render to fill and read: reset to "not counted" when `key`
    /// (what they depend on) changed since the last frame. Given back with `put_row_heights`
    pub fn take_row_heights(&mut self, key: u64) -> VecDeque<u32> {
        if key != self.row_heights_key {
            self.row_heights_key = key;
            self.row_heights.iter_mut().for_each(|height| *height = 0);
        }
        std::mem::take(&mut self.row_heights)
    }
    pub fn put_row_heights(&mut self, row_heights: VecDeque<u32>) {
        self.row_heights = row_heights;
    }
    #[inline]
    pub fn pan_to_right(&mut self, cols: usize) {
        self.scroller.pan_right(cols);
    }
    #[inline]
    pub fn pan_to_left(&mut self, cols: usize) {
        self.scroller.pan_left(cols);
    }

    #[inline]
    pub fn raw_pan_x_max(&self) -> usize {
        self.raw_scroller.max_pan_x
    }
    #[inline]
    pub fn raw_pan_x(&self) -> usize {
        self.raw_scroller.pan_x
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
    #[cfg(test)]
    pub fn scroller(&self) -> &ViewportScroller {
        &self.scroller
    }

    #[inline]
    pub fn scroller_mut(&mut self) -> &mut ViewportScroller {
        &mut self.scroller
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
}

struct LineModeTextSource<'a> {
    history: &'a VecDeque<LogEntry>,
    display_mode: LineDisplayMode,
    raw_view: RawSideView,
    view_height: usize,
}

impl<'a> TextSource for LineModeTextSource<'a> {
    fn total_lines(&mut self) -> usize {
        self.history.len()
    }

    fn get_line_text(&mut self, line_idx: usize) -> Option<String> {
        self.history
            .get(line_idx)
            .map(|e| entry_display_text(e, self.display_mode).into_owned())
    }

    fn get_line_raw_bytes(&mut self, line_idx: usize) -> Option<Vec<u8>> {
        self.history
            .get(line_idx)
            .map(|entry| entry.wire_bytes().to_vec())
    }

    fn get_line_raw_map(&mut self, line_idx: usize, raw_bytes: &[u8]) -> Vec<usize> {
        match self.history.get(line_idx) {
            Some(entry) => entry_display_to_wire_map(entry, self.display_mode),
            None => build_display_to_raw_byte_map(raw_bytes, self.display_mode),
        }
    }

    #[inline]
    fn raw_view(&mut self) -> RawSideView {
        self.raw_view
    }
    #[inline]
    fn display_mode(&mut self) -> LineDisplayMode {
        self.display_mode
    }

    fn view_height(&mut self) -> usize {
        self.view_height
    }
}

#[cfg(test)]
mod tests {
    //! RX ingest: line splitting, EOL modes, history cap and known data-model bugs.

    use super::*;
    use crate::{app::search::SearchModeKind, configs::LineLogConfig};
    use crossterm::event::{KeyCode, KeyModifiers};

    fn line_mode(max_lines: usize) -> LineMode {
        LineMode::new(max_lines, 1000, LineLogger::new(LineLogConfig::default()))
    }

    fn texts(lm: &LineMode) -> Vec<&str> {
        lm.history().iter().map(|e| e.text.as_str()).collect()
    }

    const TS: &str = "[%H:%M:%S]";

    /// CRLF and LF both commit a line, and the stored text keeps its EOL.
    /// How: one chunk with two lines.
    #[test]
    fn crlf_line_is_committed_with_its_eol() {
        let mut lm = line_mode(10);
        lm.feed_rx(RxNewlineMode::Auto, TextEncoding::Utf8, TS, b"abc\r\ndef\n");
        assert_eq!(texts(&lm), ["abc\r\n", "def\n"]);
        assert!(lm.partial_rx().is_empty());
    }

    /// A line split over two chunks is joined into one entry.
    /// How: feeds "ab" then "c\n".
    #[test]
    fn line_split_across_chunks() {
        let mut lm = line_mode(10);
        lm.feed_rx(RxNewlineMode::Auto, TextEncoding::Utf8, TS, b"ab");
        assert_eq!(lm.partial_rx(), b"ab");
        lm.feed_rx(RxNewlineMode::Auto, TextEncoding::Utf8, TS, b"c\n");
        assert_eq!(texts(&lm), ["abc\n"]);
    }

    /// In Auto mode a lone CR restarts the line (progress-bar style).
    /// How: "10%\r20%\n" keeps only the last part.
    #[test]
    fn lone_cr_overwrites_line_in_auto_mode() {
        let mut lm = line_mode(10);
        lm.feed_rx(
            RxNewlineMode::Auto,
            TextEncoding::Utf8,
            TS,
            b"progress 10%\rprogress 20%\n",
        );
        assert_eq!(texts(&lm), ["progress 20%\n"]);
    }

    /// In LfOnly mode a lone CR stays inside the line.
    /// How: "a\rb\n" is stored as is.
    #[test]
    fn lone_cr_is_kept_in_lf_only_mode() {
        let mut lm = line_mode(10);
        lm.feed_rx(RxNewlineMode::LfOnly, TextEncoding::Utf8, TS, b"a\rb\n");
        assert_eq!(texts(&lm), ["a\rb\n"]);
    }

    /// History keeps only the newest max_lines entries.
    /// How: three lines with a cap of 2.
    #[test]
    fn history_is_capped() {
        let mut lm = line_mode(2);
        lm.feed_rx(RxNewlineMode::Auto, TextEncoding::Utf8, TS, b"1\n2\n3\n");
        assert_eq!(texts(&lm), ["2\n", "3\n"]);
    }

    /// Search matches keep pointing at the same line when old lines are dropped (#19).
    /// How: match on the newest line, push more lines, check the line the index points at.
    #[test]
    fn search_matches_follow_their_line_when_history_is_full() {
        let mut lm = line_mode(2);
        lm.feed_rx(RxNewlineMode::Auto, TextEncoding::Utf8, TS, b"a\ntarget\n");
        *lm.search_mut() = SearchState {
            active: true,
            query: Input::from("target"),
            ..Default::default()
        };
        lm.update_search_matches(LineDisplayMode::Pretty, RawSideView::Hex);
        assert_eq!(lm.search().matches[0].line_idx, 1);

        lm.feed_rx(RxNewlineMode::Auto, TextEncoding::Utf8, TS, b"other\n");
        let idx = lm.search().matches[0].line_idx;
        assert_eq!(lm.history()[idx].text, "target\n");

        // once its line is dropped too, the match is gone
        lm.feed_rx(RxNewlineMode::Auto, TextEncoding::Utf8, TS, b"more\n");
        assert!(lm.search().matches.is_empty());
    }

    /// The copy cursor stays on its line when old lines are dropped (#19).
    /// How: cursor on "b", stream two lines into a 3-line history.
    #[test]
    fn copy_cursor_follows_its_line_when_history_is_full() {
        let mut lm = line_mode(3);
        lm.feed_rx(RxNewlineMode::Auto, TextEncoding::Utf8, TS, b"a\nb\nc\n");
        lm.copy_state.enter_at(1, 0);
        lm.feed_rx(RxNewlineMode::Auto, TextEncoding::Utf8, TS, b"d\n");
        assert_eq!(lm.history()[lm.copy_state().cursor.line_idx].text, "b\n");
    }

    /// A large configured history doesn't reserve memory up front (#21).
    /// How: capacity of a fresh 100k-line LineMode.
    #[test]
    fn history_is_not_preallocated() {
        let lm = line_mode(100_000);
        assert!(lm.history().capacity() < 100_000);
        assert!(lm.local_send_history.capacity() < 100_000);
    }

    /// Sent lines are capped at the configured `send_history_size`, oldest dropped first (#21).
    /// How: submits 8 lines with a cap of 3, checks size, oldest kept and history up.
    #[test]
    fn send_history_is_capped() {
        let mut lm = LineMode::new(10, 3, LineLogger::new(LineLogConfig::default()));
        let settings = SettingsConfig::default();
        for i in 0..8 {
            lm.user_input = Input::from(format!("cmd {i}").as_str());
            lm.update_action_lineinput(&settings, LineInputAction::Submit);
        }
        let kept: Vec<&str> = lm.local_send_history.iter().map(String::as_str).collect();
        assert_eq!(kept, ["cmd 5", "cmd 6", "cmd 7"]);

        lm.history_up();
        assert_eq!(lm.input().value(), "cmd 7");
    }

    /// A send history size of 0 keeps nothing, and history up does nothing.
    /// How: cap 0, one submit, then history up on an empty input.
    #[test]
    fn send_history_size_zero_keeps_nothing() {
        let mut lm = LineMode::new(10, 0, LineLogger::new(LineLogConfig::default()));
        lm.user_input = Input::from("cmd");
        lm.update_action_lineinput(&SettingsConfig::default(), LineInputAction::Submit);
        assert!(lm.local_send_history.is_empty());
        lm.history_up();
        assert_eq!(lm.input().value(), "");
    }

    /// Local echo (`push_history`) rebases too, not only received chunks.
    /// How: match on line 1 of a full history, then push one echoed line.
    #[test]
    fn push_history_rebases_line_refs() {
        let mut lm = line_mode(2);
        lm.feed_rx(RxNewlineMode::Auto, TextEncoding::Utf8, TS, b"a\ntarget\n");
        lm.copy_state.enter_at(1, 0);
        lm.push_history("echo".to_string(), TS);
        assert_eq!(lm.copy_state().cursor.line_idx, 0);
    }

    /// A Latin1 line keeps its decoded text and the byte received (#20).
    /// How: E9 received in Latin1 is "é" as text and E9 on the wire.
    #[test]
    fn latin1_line_keeps_wire_bytes() {
        let mut lm = line_mode(10);
        lm.feed_rx(
            RxNewlineMode::Auto,
            TextEncoding::Latin1,
            TS,
            &[0xE9, b'\n'],
        );
        assert_eq!(lm.history()[0].text, "é\n");
        assert_eq!(lm.history()[0].wire_bytes(), [0xE9, b'\n']);
    }
    /// A multi-line paste in the prompt acts like typing it: each line break submits, all
    /// submitted lines go out in one write, and the text after the last break stays typed.
    /// How: pastes "ls\r\npwd\nec" in a prompt that already holds "a".
    #[test]
    fn paste_submits_each_line_in_one_write() {
        let mut lm = line_mode(10);
        let settings = SettingsConfig::default();
        let eol = settings.line_mode.tx_eol.as_bytes();
        lm.update_action_lineinput(&settings, LineInputAction::Paste("a".to_string()));

        let effect = lm.update_action_lineinput(
            &settings,
            LineInputAction::Paste("ls\r\npwd\nec".to_string()),
        );
        let UpdateEffect::Transmit(bytes) = effect else {
            panic!("expected one transmit");
        };
        assert_eq!(bytes, [b"als", eol, b"pwd", eol].concat());
        assert_eq!(lm.input().value(), "ec");
    }

    /// Feeds one line received as `wire` (plus LF) with `encoding`
    fn received(encoding: TextEncoding, wire: &[u8]) -> LineMode {
        let mut lm = line_mode(10);
        let mut line = wire.to_vec();
        line.push(b'\n');
        lm.feed_rx(RxNewlineMode::Auto, encoding, TS, &line);
        lm
    }

    /// The bytes raw yank and the raw panel get for an entry (`get_line_raw_bytes`) are the
    /// bytes received, whatever the encoding (#20).
    /// How: one byte that is not plain ASCII, received with each encoding.
    #[test]
    fn raw_bytes_of_entries_are_the_received_bytes() {
        let cases: [(TextEncoding, &[u8]); 5] = [
            (TextEncoding::Utf8, &[0xC3, 0xA9]),
            (TextEncoding::Latin1, &[0xE9]),
            (TextEncoding::Cp437, &[0x82]),
            (TextEncoding::Utf8, &[0xFF]),
            (TextEncoding::Ascii, &[0xE9]),
        ];
        for (encoding, wire) in cases {
            let lm = received(encoding, wire);
            let mut source = LineModeTextSource {
                history: &lm.local_history,
                display_mode: LineDisplayMode::Pretty,
                raw_view: RawSideView::Hex,
                view_height: 10,
            };
            let mut expected = wire.to_vec();
            expected.push(b'\n');
            assert_eq!(
                source.get_line_raw_bytes(0),
                Some(expected),
                "{encoding:?} {wire:02X?}"
            );
        }
    }

    /// Yanking a Latin1 line in the Hex raw view gives the bytes received (#20).
    /// How: select "xéy" received as 78 E9 79 and yank raw.
    #[test]
    fn raw_yank_gives_received_bytes() {
        let mut lm = received(TextEncoding::Latin1, b"x\xE9y");
        let mut settings = SettingsConfig::default();
        settings.raw_side.view = RawSideView::Hex;
        lm.copy_state.enter_at(0, 0);
        lm.handle_copy_action(CopyModeAction::Select, &settings);
        lm.handle_copy_action(CopyModeAction::EndOfLine, &settings);
        let yanked = lm.handle_copy_action(CopyModeAction::YankRaw, &settings);
        assert_eq!(yanked.as_deref(), Some("78 E9 79 0A"));
    }

    /// Raw search looks for the bytes received: a Latin1 "é" is found as E9, not as its
    /// decoded C3 A9, and the match's column is the "é" in the text (#20).
    /// How: raw searches for E9 and for C3 A9 on "xéy" received as 78 E9 79.
    #[test]
    fn raw_search_sees_received_bytes() {
        let mut lm = received(TextEncoding::Latin1, b"x\xE9y");
        let mut found = |query: &str| {
            lm.search_state = SearchState {
                active: true,
                query: Input::from(query),
                mode_kind: SearchModeKind::RawBytes,
                ..Default::default()
            };
            lm.update_search_matches(LineDisplayMode::Pretty, RawSideView::Hex);
            lm.search_state.matches.len()
        };
        assert_eq!(found("E9"), 1);
        assert_eq!(found("C3 A9"), 0);
        found("E9");
        let m = &lm.search_state.matches[0];
        assert_eq!((m.col_idx, m.byte_offset, m.byte_len), (1, 1, 1));
    }

    /// MAX-72: keys that only move the query cursor don't re-run the search, so the current
    /// match and the view stay; a key that changes the query does re-run it.
    /// How: two matches, Next, then Left and Right in the query; then a typed space.
    #[test]
    fn query_cursor_keys_keep_the_current_match() {
        let mut lm = line_mode(10);
        lm.feed_rx(
            RxNewlineMode::Auto,
            TextEncoding::Utf8,
            TS,
            b"hit a\nhit b\n",
        );
        lm.search_state.active = true;
        lm.search_state.query = tui_input::Input::new("hit".to_string());
        lm.update_search_matches(LineDisplayMode::Pretty, RawSideView::Hex);
        lm.search_state.next_match();
        lm.search_state.target_jump = None;

        let press = |lm: &mut LineMode, code| {
            let key = KeyEvent::new(code, KeyModifiers::NONE);
            lm.handle_input_search(key, LineDisplayMode::Pretty, RawSideView::Hex);
        };
        press(&mut lm, KeyCode::Left);
        assert_eq!(lm.search_state.current_match, 1);
        assert_eq!(lm.search_state.target_jump, None);

        press(&mut lm, KeyCode::Right);
        press(&mut lm, KeyCode::Char(' '));
        assert_eq!(lm.search_state.current_match, 0);
        assert!(lm.search_state.target_jump.is_some());
    }
}
