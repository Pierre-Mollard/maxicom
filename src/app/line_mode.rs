use chrono::{DateTime, Local};
use std::collections::VecDeque;

use crate::{
    app::UpdateEffect,
    config::{RxNewlineMode, TxNewlineSymbol, WrapMode},
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
    temp_input: String,
    /// Max line history
    max_lines: usize,
    /// Current line being received from device output as bytes
    partial_line: Vec<u8>,
    partial_line_timestamp: Option<DateTime<Local>>,
    /// Current line being typed by user as futur device input
    partial_user_line: String,
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
}

impl LineMode {
    pub fn new(max_history_lines: usize, logger: LineLogger) -> Self {
        LineMode {
            local_history: VecDeque::with_capacity(max_history_lines),
            local_send_history: VecDeque::with_capacity(max_history_lines),
            history_index: None,
            temp_input: String::new(),
            max_lines: max_history_lines,
            partial_line: Vec::new(),
            partial_user_line: String::new(),
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
        }
    }

    pub fn partial_line_timestamp(&self) -> Option<DateTime<Local>> {
        self.partial_line_timestamp
    }

    // TODO:  Using the `ansi-to-tui` or `ratatui::text::Line::from` ANSI helpers to save some
    // colors and convert to ratatui in line mode (alreay in raw/terminal)

    pub fn feed_rx(&mut self, rx_eol_mode: RxNewlineMode, bytes: &[u8]) {
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
                    self.commit_partial_line();
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

    fn commit_partial_line(&mut self) {
        // TODO: handle different formats, right now error symbol
        let line = String::from_utf8_lossy(&self.partial_line);

        let line = line.trim_end_matches(['\r', '\n']).to_string();
        self.push_history(&line);

        self.partial_line.clear();
    }
    // TODO: add on/off recording with path
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

    /// Append a character to the input line.
    pub fn append_input_char(&mut self, c: char) {
        self.partial_user_line.push(c);
    }

    /// Remove the last character from the input line.
    pub fn delete_last_input_char(&mut self) {
        self.partial_user_line.pop();
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

    /// Read-only view of user input buffer
    #[inline]
    pub fn input_buffer(&self) -> &str {
        &self.partial_user_line
    }

    pub fn history_up(&mut self) {
        if self.local_send_history.is_empty() {
            return;
        }

        match self.history_index {
            None => {
                // Save what was typed before browsing history
                self.temp_input = self.partial_user_line.clone();
                let last = self.local_send_history.len() - 1;
                self.history_index = Some(last);
                self.partial_user_line = self.local_send_history[last].clone();
            }
            Some(i) if i > 0 => {
                let next = i - 1;
                self.history_index = Some(next);
                self.partial_user_line = self.local_send_history[next].clone();
            }
            _ => {} // already at oldest history entry
        }
    }

    pub fn history_down(&mut self) {
        match self.history_index {
            Some(i) if i + 1 < self.local_send_history.len() => {
                let next = i + 1;
                self.history_index = Some(next);
                self.partial_user_line = self.local_send_history[next].clone();
            }
            Some(_) => {
                // Return to what the user was typing
                self.history_index = None;
                self.partial_user_line = std::mem::take(&mut self.temp_input);
            }
            None => {} // already at live input prompt
        }
    }

    pub fn update_action_lineinput(
        &mut self,
        local_echo: bool,
        tx_eol_mode: TxNewlineSymbol,
        action: LineInputAction,
    ) -> UpdateEffect {
        match action {
            LineInputAction::InsertChar(c) => {
                self.history_index = None;
                self.append_input_char(c);
                UpdateEffect::None
            }
            LineInputAction::DeleteBack => {
                self.history_index = None;
                self.delete_last_input_char();
                UpdateEffect::None
            }
            LineInputAction::Submit => {
                let line = std::mem::take(&mut self.partial_user_line);

                self.temp_input.clear();
                self.history_index = None;

                if !line.is_empty() && self.local_send_history.back() != Some(&line) {
                    self.local_send_history.push_back(line.clone());
                }

                if local_echo {
                    self.push_history(&line);
                }

                // consumming string!
                let mut bytes = line.into_bytes();
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
        }
    }

    pub fn toggle_logging(&mut self) {
        if self.logger.is_active() {
            self.logger.stop();
        } else {
            let _ = self.logger.start();
        }
    }

    pub fn is_logging(&self) -> bool {
        self.logger.is_active()
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

    pub fn local_history(&self) -> &VecDeque<LogEntry> {
        &self.local_history
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
}
