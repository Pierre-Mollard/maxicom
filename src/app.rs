/// State machine, updates and queue handling
/// Mainly local functional code (core feature)
use std::collections::VecDeque;

use crate::{config::AppConfig, input::Action};

pub struct App {
    /// Completed lines of device output (scrollback) String because already processed
    local_history: VecDeque<String>,
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

impl App {
    pub fn new(app_config: AppConfig) -> Self {
        Self {
            local_history: VecDeque::with_capacity(app_config.max_history_lines),
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

    /// Append a character to the input line.
    pub fn append_input_char(&mut self, c: char) {
        self.partial_user_line.push(c);
    }

    /// Remove the last character from the input line.
    pub fn delete_last_input_char(&mut self) {
        self.partial_user_line.pop();
    }

    /// Commit the current input line (move it outside of self)
    pub fn commit_input_line(&mut self) -> String {
        std::mem::take(&mut self.partial_user_line)
    }

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
                let line = self.commit_input_line();

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
}
