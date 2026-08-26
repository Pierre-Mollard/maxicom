/// State machine, updates and queue handling
/// Mainly local functional code (core feature)
use std::collections::VecDeque;

use crate::{config::AppConfig, input::Action};

pub struct App {
    /// Completed lines of device output (scrollback) String because already processed
    local_history: VecDeque<String>,
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
    /// Current line being typed by user as futur device input
    partial_user_line: String,
    /// App config //TODO: voir si pas meilleur reuse
    app_config: AppConfig,
}

pub enum UpdateEffect {
    None,
    Transmit(Vec<u8>),
    Quit,
}

// TODO: handle mode and different actions possible in each
impl App {
    pub fn new(app_config: AppConfig) -> Self {
        Self {
            local_history: VecDeque::with_capacity(app_config.max_history_lines),
            local_send_history: VecDeque::with_capacity(app_config.max_history_lines),
            history_index: None,
            temp_input: String::new(),
            max_lines: app_config.max_history_lines,
            partial_line: Vec::new(),
            partial_user_line: String::new(),
            app_config,
        }
    }

    pub fn feed_rx(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.partial_line.push(b);

            if b == b'\n' {
                self.commit_partial_line();
            }
        }
    }

    fn commit_partial_line(&mut self) {
        // TODO: handle different formats, right now error symbol
        let line = String::from_utf8_lossy(&self.partial_line);

        let line = line.trim_end_matches(['\r', '\n']);
        self.local_history.push_back(line.to_string());

        // Enforce maximum scrollback size.
        while self.local_history.len() > self.max_lines {
            self.local_history.pop_front();
        }

        self.partial_line.clear();
    }

    pub fn push_history(&mut self, line: &str) {
        self.local_history.push_back(line.to_string());
    }

    pub fn push_send_history(&mut self, line: &str) {
        self.local_send_history.push_back(line.to_string());
    }

    /// Append a character to the input line.
    pub fn append_input_char(&mut self, c: char) {
        self.partial_user_line.push(c);
    }

    /// Remove the last character from the input line.
    pub fn delete_last_input_char(&mut self) {
        self.partial_user_line.pop();
    }

    // TODO: mode just pass through where ^N or ^P or ^A etc. are sent like that and history is
    // handled by target
    pub fn update(&mut self, action: Action) -> UpdateEffect {
        match action {
            Action::InsertChar(c) => {
                self.append_input_char(c);
                UpdateEffect::None
            }
            Action::DeleteBack => {
                self.delete_last_input_char();
                UpdateEffect::None
            }
            Action::Submit => {
                let line = std::mem::take(&mut self.partial_user_line);

                if !line.is_empty() && self.local_send_history.back() != Some(&line) {
                    self.local_send_history.push_back(line.clone());
                }

                if self.app_config.local_echo {
                    self.push_history(&line);
                }

                // consumming string!
                let mut bytes = line.into_bytes();
                bytes.extend_from_slice(self.app_config.eol_mode.as_bytes());
                UpdateEffect::Transmit(bytes)
            }
            Action::Quit => UpdateEffect::Quit,
            Action::Ignore => UpdateEffect::None,
        }
    }

    /// Read-only slice/iterator of completed lines
    pub fn history(&self) -> &VecDeque<String> {
        &self.local_history
    }

    /// Read-only view of current incoming RX slice
    pub fn partial_rx(&self) -> &[u8] {
        &self.partial_line
    }

    /// Read-only view of user input buffer
    pub fn input_buffer(&self) -> &str {
        &self.partial_user_line
    }

    /// Read-only should display the recevied bytes before a full line
    pub fn display_partial_incoming(&self) -> bool {
        self.app_config.display_partial_rx
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
}
