use chrono::{DateTime, Local};
use std::{collections::VecDeque, io, path::PathBuf};
use tui_input::{Input, backend::crossterm::EventHandler};

use crate::{
    app::UpdateEffect,
    config::{RxNewlineMode, TextEncoding, TxNewlineSymbol, WrapMode},
    input::LineInputAction,
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
}

impl LineMode {
    pub fn new(max_history_lines: usize, logger: LineLogger) -> Self {
        let search_state = SearchState::default();
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
        }
    }

    #[inline]
    pub fn input_buffer(&self) -> &str {
        self.user_input.value()
    }

    #[inline]
    pub fn input(&self) -> &Input {
        &self.user_input
    }

    pub fn partial_line_timestamp(&self) -> Option<DateTime<Local>> {
        self.partial_line_timestamp
    }

    pub fn feed_rx(&mut self, rx_eol_mode: RxNewlineMode, encoding: TextEncoding, bytes: &[u8]) {
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
                    self.commit_partial_line(encoding);
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

    fn commit_partial_line(&mut self, encoding: TextEncoding) {
        let line = encoding.decode(&self.partial_line);
        let trimmed = line.trim_end_matches(['\r', '\n']).to_string();
        self.push_history(&trimmed);
        self.partial_line.clear();
    }

    pub fn push_history(&mut self, line: &str) {
        let timestamp = self
            .partial_line_timestamp
            .take()
            .unwrap_or_else(Local::now);
        let entry = LogEntry {
            text: line.to_string(),
            timestamp,
        };

        let _ = self.logger.log_entry(&entry);

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
        action: LineInputAction,
    ) -> UpdateEffect {
        match action {
            LineInputAction::ToggleSearch => {
                self.search_state.toggle(!self.search_state.active);
                if self.search_state.active {
                    self.search_state.update_matches(&self.local_history);
                }
                UpdateEffect::None
            }
            LineInputAction::SearchPrevious => {
                if self.search_state.active
                    && let Some(target_entry_idx) = self.search_state.prev_match()
                {
                    self.search_state.target_jump_index = Some(target_entry_idx);
                }
                UpdateEffect::None
            }
            LineInputAction::SearchNext => {
                if self.search_state.active
                    && let Some(target_entry_idx) = self.search_state.next_match()
                {
                    self.search_state.target_jump_index = Some(target_entry_idx);
                }
                UpdateEffect::None
            }
            LineInputAction::Submit => {
                let text = self.user_input.value().to_string();
                self.user_input.reset();
                self.temp_input.reset();
                self.history_index = None;

                if !text.is_empty() && self.local_send_history.back() != Some(&text) {
                    self.local_send_history.push_back(text.clone());
                }

                if local_echo {
                    self.push_history(&text);
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
            // TODO: input prompt handle emac/vi shortcuts + clear when out history
            LineInputAction::HandleKey(key_event) => {
                if !self.search_state.active {
                    self.history_index = None;
                    self.user_input
                        .handle_event(&crossterm::event::Event::Key(key_event));
                } else {
                    self.search_state
                        .query
                        .handle_event(&crossterm::event::Event::Key(key_event));
                    self.search_state.update_matches(&self.local_history);
                    if let Some(&first_match_idx) = self.search_state.matches.first() {
                        self.search_state.current_match = 0;
                        self.search_state.target_jump_index = Some(first_match_idx.line_idx);
                    }
                }
                UpdateEffect::None
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMatch {
    pub line_idx: usize,
    pub occurrence_in_line: usize,
}

#[derive(Default)]
pub struct SearchState {
    pub active: bool,
    pub query: Input,
    pub matches: Vec<SearchMatch>,
    pub current_match: usize,
    pub target_jump_index: Option<usize>,
}

impl SearchState {
    pub fn update_matches(&mut self, history: &VecDeque<LogEntry>) {
        self.matches.clear();
        if self.query_str().is_empty() {
            self.current_match = 0;
            return;
        }

        let q = self.query_str().to_lowercase();
        for (line_idx, entry) in history.iter().enumerate() {
            let line_lower = entry.text.to_lowercase();
            for (occurrence, _) in line_lower.match_indices(&q).enumerate() {
                self.matches.push(SearchMatch {
                    line_idx,
                    occurrence_in_line: occurrence,
                });
            }
        }

        if self.current_match >= self.matches.len() {
            self.current_match = self.matches.len().saturating_sub(1);
        }
    }

    pub fn next_match(&mut self) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        self.current_match = (self.current_match + 1) % self.matches.len();
        Some(self.matches[self.current_match].line_idx)
    }

    pub fn prev_match(&mut self) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        if self.current_match == 0 {
            self.current_match = self.matches.len().saturating_sub(1);
        } else {
            self.current_match -= 1;
        }
        Some(self.matches[self.current_match].line_idx)
    }

    pub fn matches_count(&self) -> usize {
        self.matches.len()
    }

    pub fn current_match_display(&self) -> usize {
        if self.matches.is_empty() {
            0
        } else {
            self.current_match + 1
        }
    }

    pub fn toggle(&mut self, active: bool) {
        self.active = active;
        if !active {
            self.target_jump_index = None;
        }
    }

    pub fn active_occurrence_for_line(&self, line_idx: usize) -> Option<usize> {
        if let Some(m) = self.matches.get(self.current_match) {
            if m.line_idx == line_idx {
                return Some(m.occurrence_in_line);
            }
        }
        None
    }

    #[inline]
    pub fn query_str(&self) -> &str {
        self.query.value()
    }
}
