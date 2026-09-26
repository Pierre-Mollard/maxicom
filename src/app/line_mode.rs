use chrono::{DateTime, Local};
use crossterm::event::KeyEvent;
use std::{collections::VecDeque, io, path::PathBuf};
use tui_input::Input;

use crate::{
    app::{
        UpdateEffect,
        search::{SearchState, dispatch_input_key},
        selection::CopyModeState,
    },
    config::{RxNewlineMode, TextEncoding, TxNewlineSymbol, WrapMode},
    input::{CopyModeAction, LineInputAction},
    logger::LineLogger,
    types::{LogEntry, ViewportSize},
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
    /// Waiting to receive carriage return
    pending_carriage_return: bool,
    /// Number of visual lines above newest output
    scroll_from_bottom: usize,
    /// Offset to display when lines are not wrapped
    x_offset: usize,
    new_content_missed: bool,
    logger: LineLogger,
    max_scroll: usize,
    max_pan: usize,
    pan_right_explored_amount: usize,
    view_height: usize,
    view_width: usize,
    search_state: SearchState,
    copy_state: CopyModeState,
}

impl LineMode {
    pub fn new(max_history_lines: usize, logger: LineLogger) -> Self {
        let search_state = SearchState::default();
        let copy_state = CopyModeState::default();
        LineMode {
            local_history: VecDeque::with_capacity(max_history_lines),
            local_send_history: VecDeque::with_capacity(max_history_lines),
            history_index: None,
            temp_input: Input::default(),
            max_lines: max_history_lines,
            partial_line: Vec::new(),
            user_input: Input::default(),
            partial_line_timestamp: None,
            pending_carriage_return: false,
            scroll_from_bottom: 0,
            x_offset: 0,
            max_scroll: 0,
            max_pan: 0,
            new_content_missed: false,
            logger,
            pan_right_explored_amount: 0,
            view_width: 0,
            view_height: 0,
            search_state,
            copy_state,
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
                    }
                    RxNewlineMode::LfOnly => {}
                },
                b'\n' => {
                    self.pending_carriage_return = false;
                    self.commit_partial_line(encoding, timestamp_f);
                }
                0x08 | 0x7F => {
                    self.partial_line.pop();
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
        let trimmed = line.trim_end_matches(['\r', '\n']).to_string();
        self.push_history(&trimmed, timestamp_f);
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

        if self.scroll_offset() > 0 {
            self.new_content_missed = true;
        }

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
        local_echo: bool,
        tx_eol_mode: TxNewlineSymbol,
        timestamp_f: &str,
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

                if local_echo {
                    self.push_history(&text, timestamp_f);
                }

                // consumming string!
                let mut bytes = text.into_bytes();
                bytes.extend_from_slice(tx_eol_mode.as_bytes());

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
            LineInputAction::HandleKey(key_event) => {
                self.history_index = None;
                dispatch_input_key(&mut self.user_input, key_event);
                UpdateEffect::None
            }
        }
    }

    // TODO: in line mode copy is only on visible text.... (and cursor is not spawning at latest
    // receive position)
    pub fn handle_copy_action(&mut self, action: CopyModeAction) -> Option<String> {
        let total_visual_rows = self.max_scroll + self.view_height;
        let content_width = self.view_width;

        if total_visual_rows == 0 {
            if action == CopyModeAction::Exit {
                self.copy_state.exit();
            }
            return None;
        }

        let max_row = total_visual_rows.saturating_sub(1);
        let max_col = content_width + self.max_pan;

        match action {
            CopyModeAction::Enter => {
                let current_top = self.max_scroll.saturating_sub(self.scroll_from_bottom);
                let current_bottom =
                    (current_top + self.view_height.saturating_sub(1)).min(max_row);

                self.copy_state.enter_at(current_bottom, 0);

                self.keep_copy_cursor_in_view();
                return None;
            }
            CopyModeAction::Exit => {
                self.copy_state.exit();
                return None;
            }
            CopyModeAction::MoveUp => {
                self.copy_state.cursor.row = self.copy_state.cursor.row.saturating_sub(1);
                self.keep_copy_cursor_in_view();
            }
            CopyModeAction::MoveDown => {
                self.copy_state.cursor.row = (self.copy_state.cursor.row + 1).min(max_row);
                self.keep_copy_cursor_in_view();
            }
            CopyModeAction::MoveLeft => {
                self.copy_state.cursor.col = self.copy_state.cursor.col.saturating_sub(1);
                self.keep_copy_cursor_in_view();
            }
            CopyModeAction::MoveRight => {
                self.copy_state.cursor.col = (self.copy_state.cursor.col + 1).min(max_col);
                self.keep_copy_cursor_in_view();
            }
            CopyModeAction::PageUp => {
                let jump = self.view_height.saturating_sub(1);
                self.copy_state.cursor.row = self.copy_state.cursor.row.saturating_sub(jump);
                self.keep_copy_cursor_in_view();
            }
            CopyModeAction::PageDown => {
                let jump = self.view_height.saturating_sub(1);
                self.copy_state.cursor.row = (self.copy_state.cursor.row + jump).min(max_row);
                self.keep_copy_cursor_in_view();
            }
            CopyModeAction::StartOfLine => {
                self.copy_state.cursor.col = 0;
            }
            CopyModeAction::EndOfLine => {
                self.copy_state.cursor.col = max_col;
            }
            CopyModeAction::ToggleSelection => {
                self.copy_state.toggle_selection();
            }
            CopyModeAction::Yank => {
                self.copy_state.pending_yank = true;
            }
        }
        None
    }

    fn keep_copy_cursor_in_view(&mut self) {
        let cursor_row = self.copy_state.cursor.row;
        let cursor_col = self.copy_state.cursor.col;
        let view_h = self.view_height;
        let view_w = self.view_width;

        let current_top = self.max_scroll.saturating_sub(self.scroll_from_bottom);
        let current_bottom = current_top + view_h.saturating_sub(1);

        if cursor_row < current_top {
            let diff = current_top - cursor_row;
            self.scroll_up(diff);
        } else if cursor_row > current_bottom {
            let diff = cursor_row - current_bottom;
            self.scroll_down(diff);
        }

        if view_w > 0 && self.max_pan > 0 {
            if cursor_col < self.x_offset {
                self.x_offset = cursor_col;
            } else if cursor_col >= self.x_offset + view_w {
                self.x_offset = (cursor_col + 1).saturating_sub(view_w).min(self.max_pan);
            }
        } else {
            self.x_offset = 0;
        }
    }

    pub fn update_search_matches(&mut self) {
        self.search_state
            .update_matches_from_entries(&self.local_history);
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

    pub fn update_viewport(&mut self, wrap_mode: WrapMode, viewport: ViewportSize) {
        self.view_height = viewport.rows;
        self.view_width = viewport.cols;
        self.scroll_from_bottom = self.scroll_from_bottom.min(self.max_scroll);
        self.x_offset = match wrap_mode {
            WrapMode::Wrap => 0,
            WrapMode::Truncate => self.x_offset.min(self.max_pan),
        };
    }

    pub fn update_limits(&mut self, max_scroll: usize, max_pan: usize) {
        self.max_scroll = max_scroll;
        self.max_pan = max_pan;
        self.scroll_from_bottom = self.scroll_from_bottom.min(self.max_scroll);
        self.x_offset = self.x_offset.min(self.max_pan);
    }

    pub fn scroll_to(&mut self, lines: usize) {
        self.scroll_from_bottom = lines;
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_from_bottom = self.scroll_from_bottom.saturating_add(lines);
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_from_bottom = self.scroll_from_bottom.saturating_sub(lines);
        if self.scroll_offset() == 0 {
            self.new_content_missed = false;
        }
    }

    pub fn scroll_up_page(&mut self) {
        self.scroll_from_bottom = self
            .scroll_from_bottom
            .saturating_add(self.view_height.saturating_sub(1));
    }

    pub fn scroll_down_page(&mut self) {
        self.scroll_from_bottom = self
            .scroll_from_bottom
            .saturating_sub(self.view_height.saturating_sub(1));
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_from_bottom = 0;
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll_from_bottom = usize::MAX;
    }

    pub fn pan_to_right(&mut self, cols: usize) {
        self.x_offset = self.x_offset.saturating_add(cols);
        self.pan_right_explored_amount = self.pan_right_explored_amount.max(self.x_offset);
    }

    pub fn pan_to_left(&mut self, cols: usize) {
        self.x_offset = self.x_offset.saturating_sub(cols);
    }

    #[inline]
    pub fn scroll_offset(&self) -> usize {
        self.scroll_from_bottom
    }

    #[inline]
    pub fn x_offset(&self) -> usize {
        self.x_offset
    }

    /// Return Some(pan possible, max pan explored since start)
    pub fn can_pan_right(&self) -> Option<(usize, usize)> {
        if self.max_pan > 0 {
            Some((
                self.max_pan.saturating_sub(self.x_offset),
                self.pan_right_explored_amount,
            ))
        } else {
            None
        }
    }

    /// Return Some(pan possible)
    pub fn can_pan_left(&self) -> Option<usize> {
        if self.x_offset > 0 {
            Some(self.x_offset)
        } else {
            None
        }
    }

    pub fn is_new_content_missed(&self) -> bool {
        self.new_content_missed
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
    pub fn copy_state_mut(&mut self) -> &mut CopyModeState {
        &mut self.copy_state
    }
}
