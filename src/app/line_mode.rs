use chrono::{DateTime, Local};
use crossterm::event::KeyEvent;
use std::{collections::VecDeque, io, path::PathBuf};
use tui_input::Input;

use crate::{
    app::{
        UpdateEffect,
        data_processing::displayable_line_string,
        scroller::ViewportScroller,
        search::SearchState,
        selection::{CopyActionResult, CopyModeState, TextSource},
        types::{LogEntry, ViewportSize},
    },
    configs::{LineDisplayMode, RxNewlineMode, SettingsConfig, TextEncoding, WrapMode},
    inputs::{
        CopyModeAction, LineInputAction, SearchModeAction, ViMode, ViState, dispatch_input_key,
        dispatch_input_key_vi, local_dispatchers::dispatch_copy_key_vi, vi::ViPending,
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
    vi_state_copymode: Option<ViPending>,
}

impl LineMode {
    pub fn new(max_history_lines: usize, logger: LineLogger) -> Self {
        let search_state = SearchState::default();
        let copy_state = CopyModeState::default();
        let scroller = ViewportScroller::default();
        LineMode {
            local_history: VecDeque::with_capacity(max_history_lines),
            local_send_history: VecDeque::with_capacity(max_history_lines),
            history_index: None,
            temp_input: Input::default(),
            max_lines: max_history_lines,
            partial_line: Vec::new(),
            user_input: Input::default(),
            vi_state_prompt: ViState::default(),
            partial_line_timestamp: None,
            pending_carriage_return: false,
            logger,
            search_state,
            copy_state,
            scroller,
            vi_state_copymode: None,
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
        for &b in bytes {
            if self.partial_line.is_empty() {
                self.partial_line_timestamp = Some(Local::now());
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
                        self.partial_line_timestamp = Some(Local::now());
                    }
                    self.partial_line.push(byte);
                }
            }
        }
    }

    fn commit_partial_line(&mut self, encoding: TextEncoding, timestamp_f: &str) {
        let line = encoding.decode(&self.partial_line);
        self.push_history(&line, timestamp_f);
        self.partial_line.clear();
    }

    pub fn push_history(&mut self, line: &str, timestamp_f: &str) {
        let timestamp = self
            .partial_line_timestamp
            .take()
            .unwrap_or_else(Local::now);
        let entry = LogEntry {
            text: line.to_string(),
            timestamp,
        };

        let _ = self.logger.log_entry(&entry, timestamp_f);

        self.local_history.push_back(entry);

        self.scroller.on_rx_received();

        // Enforce maximum scrollback size.
        while self.local_history.len() > self.max_lines {
            self.local_history.pop_front();
        }
    }

    pub fn clear_history(&mut self) {
        self.local_history.clear();
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
                let text = self.user_input.value().to_string();
                self.user_input.reset();
                self.temp_input.reset();
                self.history_index = None;

                if !text.is_empty() && self.local_send_history.back() != Some(&text) {
                    self.local_send_history.push_back(text.clone());
                }

                if settings.local_echo {
                    self.push_history(&text, &settings.timestamp_format);
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
                .map(|e| displayable_line_string(&e.text, display_mode))
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

    fn sync_scroll_with_cursor(&mut self) {
        let total_lines = self.local_history.len();
        if total_lines == 0 {
            return;
        }

        let line_from_bottom = (total_lines - 1).saturating_sub(self.copy_state.cursor.line_idx);
        let view_h = self.scroller.view_height;
        let half = view_h / 2;

        let target_scroll = line_from_bottom.saturating_sub(half);
        self.scroller.scroll_to(target_scroll);
    }

    fn sync_pan_with_cursor(&mut self, display_mode: LineDisplayMode, gutter_width: usize) {
        let view_w = self.scroller.view_width;
        if view_w == 0 {
            return;
        }

        let cursor_char_idx = self.copy_state.cursor.char_idx;
        let content_width =
            if let Some(entry) = self.local_history.get(self.copy_state.cursor.line_idx) {
                let display_text = displayable_line_string(&entry.text, display_mode);
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

    pub fn update_search_matches(&mut self, display_mode: LineDisplayMode) {
        self.search_state
            .update_matches_from_entries(&self.local_history, display_mode);
    }

    pub fn handle_search_action(
        &mut self,
        action: SearchModeAction,
        display_mode: LineDisplayMode,
    ) {
        match action {
            SearchModeAction::Start => {
                self.search_state.toggle_itself();
                if self.search_state.active {
                    self.update_search_matches(display_mode);
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
                    self.handle_input_search(key_event, display_mode);
                }
            }
        }
    }

    pub fn handle_input_search(&mut self, key_event: KeyEvent, display_mode: LineDisplayMode) {
        dispatch_input_key(&mut self.search_state.query, key_event);
        self.update_search_matches(display_mode);
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
    pub fn scroll_up(&mut self, lines: usize) {
        self.scroller.scroll_up(lines);
    }
    #[inline]
    pub fn scroll_down(&mut self, lines: usize) {
        self.scroller.scroll_down(lines);
    }
    #[inline]
    pub fn scroll_up_page(&mut self) {
        self.scroller.scroll_up_page();
    }
    #[inline]
    pub fn scroll_down_page(&mut self) {
        self.scroller.scroll_down_page();
    }
    #[inline]
    pub fn scroll_to_bottom(&mut self) {
        self.scroller.scroll_to_bottom();
    }
    #[inline]
    pub fn scroll_to_top(&mut self) {
        self.scroller.scroll_to_top();
    }
    #[inline]
    pub fn scroll_to(&mut self, lines: usize) {
        self.scroller.scroll_to(lines);
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

struct LineModeTextSource<'a> {
    history: &'a VecDeque<LogEntry>,
    display_mode: LineDisplayMode,
    view_height: usize,
}

impl<'a> TextSource for LineModeTextSource<'a> {
    fn total_lines(&mut self) -> usize {
        self.history.len()
    }

    fn get_line_text(&mut self, line_idx: usize) -> Option<String> {
        self.history
            .get(line_idx)
            .map(|e| displayable_line_string(&e.text, self.display_mode))
    }

    fn view_height(&mut self) -> usize {
        self.view_height
    }
}
