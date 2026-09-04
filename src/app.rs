use chrono::{DateTime, Local};
/// State machine, updates and queue handling
/// Mainly local functional code (core feature)
use std::collections::VecDeque;
use vt100::Parser as Vt100Parser;

use crate::{
    config::{AppConfig, InputMode, RxNewlineMode, WrapMode},
    input::{Action, AppAction, HistoryAction, LineInputAction, OverlayAction, SettingKey},
    logger::{FileLogger, LogConfig},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    Help,
    Settings,
}

pub struct LogEntry {
    pub text: String,
    pub timestamp: DateTime<Local>,
}

impl LogEntry {
    // TODO : make format config
    pub fn formatted_time(&self) -> String {
        let time = self.timestamp.format("%H:%M:%S%.3f").to_string();
        format!("[{time}]")
    }

    pub fn formatted_time_builder(timestamp: DateTime<Local>) -> String {
        let time = timestamp.format("%H:%M:%S%.3f").to_string();
        format!("[{time}]")
    }
}

pub struct App {
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
    /// 0 = pinned to newest line (autoscroll)
    scroll_offset: usize,
    /// App config
    app_config: AppConfig,
    prefix_active: bool,
    overlay: Option<Overlay>,
    logger: FileLogger,
    pending_carriage_return: bool,
    pub vt100_parser: vt100::Parser,
    term_size: (u16, u16),
}

pub enum UpdateEffect {
    None,
    Transmit(Vec<u8>),
    Quit,
}

impl App {
    pub fn new(app_config: AppConfig, log_config: LogConfig) -> Self {
        let vt100_parser = Vt100Parser::new(24, 80, 1000);
        // TODO: configure size of scrollback history with vt100
        Self {
            local_history: VecDeque::with_capacity(app_config.max_history_lines),
            local_send_history: VecDeque::with_capacity(app_config.max_history_lines),
            history_index: None,
            temp_input: String::new(),
            max_lines: app_config.max_history_lines,
            partial_line: Vec::new(),
            partial_user_line: String::new(),
            partial_line_timestamp: None,
            scroll_offset: 0,
            prefix_active: false,
            app_config,
            overlay: None,
            logger: FileLogger::new(log_config),
            pending_carriage_return: false,
            vt100_parser,
            term_size: (40, 80),
        }
    }

    pub fn vt100_screen(&self) -> &vt100::Screen {
        self.vt100_parser.screen()
    }

    // TODO: handle resizing terminal 2D because now it is like minicom data loss...
    // TODO: add command to enable sending the 'resize' command to a linux remote
    pub fn resize_vt100(&mut self, rows: u16, cols: u16) {
        if rows > 0 && cols > 0 && self.term_size != (rows, cols) {
            self.term_size = (rows, cols);
            self.vt100_parser.screen_mut().set_size(rows, cols);
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

    pub fn overlay(&self) -> Option<Overlay> {
        self.overlay
    }

    pub fn set_overlay(&mut self, overlay: Option<Overlay>) {
        self.overlay = overlay;
    }

    pub fn partial_line_timestamp(&self) -> Option<DateTime<Local>> {
        self.partial_line_timestamp
    }

    // TODO:  Using the `ansi-to-tui` or `ratatui::text::Line::from` ANSI helpers to save some
    // colors and convert to ratatui

    // TODO: handle interactive mode like VT100 (huge change)
    // TODO: save as raw capture stream when in interactive mode
    pub fn feed_rx(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if self.partial_line.is_empty() {
                self.partial_line_timestamp = Some(Local::now());
            }

            match b {
                b'\r' => match self.app_config.rx_eol_mode {
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

        // Enforce maximum scrollback size.
        while self.local_history.len() > self.max_lines {
            self.local_history.pop_front();
        }

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

        // TODO: add limite to the Rx queue as well (check if commit partial line or here)
        self.local_history.push_back(entry);
    }

    /// Append a character to the input line.
    pub fn append_input_char(&mut self, c: char) {
        self.partial_user_line.push(c);
    }

    /// Remove the last character from the input line.
    pub fn delete_last_input_char(&mut self) {
        self.partial_user_line.pop();
    }

    pub fn update(&mut self, action: Action) -> UpdateEffect {
        match action {
            Action::TransmitRaw(bytes) => {
                if self.app_config.local_echo {
                    let text = String::from_utf8_lossy(&bytes);
                    self.push_history(&text);
                }
                UpdateEffect::Transmit(bytes)
            }
            Action::LineInput(action) => self.update_action_lineinput(action),
            Action::History(action) => self.update_action_history(action),
            Action::Overlay(action) => self.update_action_overlay(action),
            Action::App(action) => self.update_action_app(action),
            Action::Ignore => UpdateEffect::None,
        }
    }

    fn update_action_app(&mut self, action: AppAction) -> UpdateEffect {
        match action {
            AppAction::SetPrefix(state) => {
                self.prefix_active = state;
                UpdateEffect::None
            }
            AppAction::Quit => UpdateEffect::Quit,
        }
    }

    fn update_action_lineinput(&mut self, action: LineInputAction) -> UpdateEffect {
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

                if self.app_config.local_echo {
                    self.push_history(&line);
                }

                // consumming string!
                let mut bytes = line.into_bytes();
                bytes.extend_from_slice(self.app_config.tx_eol_mode.as_bytes());

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

    fn update_action_history(&mut self, action: HistoryAction) -> UpdateEffect {
        match action {
            HistoryAction::ScrollUp(nb) => {
                self.scroll_up(nb);
                UpdateEffect::None
            }
            HistoryAction::ScrollDown(nb) => {
                self.scroll_down(nb);
                UpdateEffect::None
            }
            HistoryAction::ScrollToBottom => {
                self.scroll_to_bottom();
                UpdateEffect::None
            }
            HistoryAction::ScrollToTop => {
                self.scroll_to_top();
                UpdateEffect::None
            }
            HistoryAction::Clear => {
                self.clear_history();
                UpdateEffect::None
            }
        }
    }

    fn update_action_overlay(&mut self, action: OverlayAction) -> UpdateEffect {
        match action {
            OverlayAction::Open(overlay) => {
                self.prefix_active = false;
                self.overlay = Some(overlay);
                UpdateEffect::None
            }
            OverlayAction::Close => {
                self.overlay = None;
                UpdateEffect::None
            }
            OverlayAction::SelectNext => UpdateEffect::None,
            OverlayAction::SelectPrev => UpdateEffect::None,
            OverlayAction::Confirm => UpdateEffect::None,
            OverlayAction::ToggleSetting(setting_key) => match setting_key {
                SettingKey::LocalEcho => {
                    self.app_config.local_echo = !self.app_config.local_echo;
                    UpdateEffect::None
                }
                SettingKey::LineWrap => {
                    self.app_config.wrap_mode = match self.app_config.wrap_mode {
                        WrapMode::Wrap => WrapMode::Truncate,
                        WrapMode::Truncate => WrapMode::Wrap,
                    };
                    UpdateEffect::None
                }
                SettingKey::InputMode => {
                    // TODO: these prefix reset should not be linked to the
                    // actions because it will change bindings maybe later to not using prefix and some
                    // reverse
                    self.prefix_active = false;
                    self.app_config.input_mode = match self.app_config.input_mode {
                        InputMode::Line => InputMode::Terminal,
                        InputMode::Terminal => InputMode::Line,
                    };
                    UpdateEffect::None
                }
            },
        }
    }

    /// Read-only slice/iterator of completed lines
    pub fn history(&self) -> &VecDeque<LogEntry> {
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

    /// Read-only accessor if outside modules need it
    pub fn config(&self) -> &AppConfig {
        &self.app_config
    }

    pub fn update_vt100_scroll(&mut self) {
        if self.app_config.input_mode == InputMode::Terminal {
            self.vt100_parser
                .screen_mut()
                .set_scrollback(self.scroll_offset);
            self.scroll_offset = self.vt100_parser.screen().scrollback(); // in case clamping
        }
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(lines);
        self.update_vt100_scroll();
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
        self.update_vt100_scroll();
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.update_vt100_scroll();
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = usize::MAX;
        self.update_vt100_scroll();
    }

    pub fn clamp_scroll(&mut self, visible_height: usize) {
        let max_scroll = self.local_history.len().saturating_sub(visible_height);
        if self.scroll_offset > max_scroll {
            self.scroll_offset = max_scroll;
        }
        self.update_vt100_scroll();
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub fn is_prefix_active(&self) -> bool {
        self.prefix_active
    }

    pub fn clear_history(&mut self) {
        self.local_history.clear();
    }
}
