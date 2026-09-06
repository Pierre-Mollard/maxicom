use chrono::{DateTime, Local};
/// State machine, updates and queue handling
/// Mainly local functional code (core feature)
use std::collections::VecDeque;
use vt100::Parser as Vt100Parser;

use crate::{
    config::{AppConfig, InputMode, RxNewlineMode, TxNewlineSymbol, WrapMode},
    input::{Action, AppAction, HistoryAction, LineInputAction, OverlayAction, SettingKey},
    logger::{FileLogger, LogConfig},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    Help,
    Settings,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewportSize {
    pub cols: usize,
    pub rows: usize,
}

impl ViewportSize {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols: cols as usize,
            rows: rows as usize,
        }
    }
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

pub struct LineViewState {
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
    pub scroll_from_bottom: usize,
    /// Offset to display when lines are not wrapped
    pub x_offset: usize,
    /// TODO: make in terminal with byte stream
    logger: FileLogger,
}

pub struct TerminalViewState {
    pub vt100_parser: vt100::Parser,
    pub scrollback_offset: usize,
    pub pan_offset: usize,
}

pub struct App {
    app_config: AppConfig,
    prefix_active: bool,
    overlay: Option<Overlay>,
    pub viewport: ViewportSize,
    pub line_view: LineViewState,
    pub terminal_view: TerminalViewState,
}

pub enum UpdateEffect {
    None,
    Transmit(Vec<u8>),
    Quit,
}

impl LineViewState {
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

    fn update_action_lineinput(
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

    // FIXME: wont work when wrap mode is enable because one line is not always one height
    pub fn clamp_viewport(&mut self, viewport: ViewportSize, wrap: WrapMode) {
        let max_scroll = self.local_history.len().saturating_sub(viewport.rows);

        self.scroll_from_bottom = self.scroll_from_bottom.min(max_scroll);

        match wrap {
            WrapMode::Wrap => {
                self.x_offset = 0;
            }
            WrapMode::Truncate => {} //TODO: clamp
        }
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_from_bottom = self.scroll_from_bottom.saturating_add(lines);
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_from_bottom = self.scroll_from_bottom.saturating_sub(lines);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_from_bottom = 0;
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll_from_bottom = usize::MAX;
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll_from_bottom
    }

    pub fn pan_to_right(&mut self, cols: usize) {
        self.x_offset = self.x_offset.saturating_add(cols);
    }

    pub fn pan_to_left(&mut self, cols: usize) {
        self.x_offset = self.x_offset.saturating_sub(cols);
    }

    pub fn x_offset(&self) -> usize {
        self.x_offset
    }
}

impl TerminalViewState {
    pub fn clamp_viewport(&mut self, viewport: ViewportSize) {
        self.clamp_scrollback();
        self.clamp_pan(viewport.cols);
    }

    fn clamp_scrollback(&mut self) {
        self.vt100_parser
            .screen_mut()
            .set_scrollback(self.scrollback_offset);
        self.scrollback_offset = self.vt100_parser.screen().scrollback();
    }

    fn clamp_pan(&mut self, visible_cols: usize) {
        let vt_cols = self.vt100_parser.screen().size().1 as usize;
        let max_pan = vt_cols.saturating_sub(visible_cols);
        self.pan_offset = self.pan_offset.min(max_pan);
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scrollback_offset = self.scrollback_offset.saturating_add(lines);
        self.clamp_scrollback();
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scrollback_offset = self.scrollback_offset.saturating_sub(lines);
        self.clamp_scrollback();
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scrollback_offset = 0;
        self.clamp_scrollback();
    }

    pub fn scroll_to_top(&mut self) {
        self.scrollback_offset = usize::MAX;
        self.clamp_scrollback();
    }

    pub fn pan_to_right(&mut self, cols: usize, visible_cols: usize) {
        self.pan_offset = self.pan_offset.saturating_add(cols);
        self.clamp_pan(visible_cols);
    }

    pub fn pan_to_left(&mut self, cols: usize, visible_cols: usize) {
        self.pan_offset = self.pan_offset.saturating_sub(cols);
        self.clamp_pan(visible_cols);
    }

    pub fn pan_offset(&self) -> usize {
        self.pan_offset
    }
}

impl App {
    pub fn new(app_config: AppConfig, log_config: LogConfig) -> Self {
        // TODO: configure size of scrollback history with vt100
        let vt100_parser = Vt100Parser::new(24, 80, 1000);

        let line_view = LineViewState {
            local_history: VecDeque::with_capacity(app_config.max_history_lines),
            local_send_history: VecDeque::with_capacity(app_config.max_history_lines),
            history_index: None,
            temp_input: String::new(),
            max_lines: app_config.max_history_lines,
            partial_line: Vec::new(),
            partial_user_line: String::new(),
            partial_line_timestamp: None,
            pending_carriage_return: false,
            scroll_from_bottom: 0,
            x_offset: 0,
            logger: FileLogger::new(log_config),
        };

        let terminal_view = TerminalViewState {
            vt100_parser,
            scrollback_offset: 0,
            pan_offset: 0,
        };

        Self {
            prefix_active: false,
            app_config,
            overlay: None,
            viewport: ViewportSize::default(),
            line_view,
            terminal_view,
        }
    }

    pub fn update_viewport(&mut self, viewport: ViewportSize) {
        match self.app_config.input_mode {
            InputMode::Line => {
                self.line_view
                    .clamp_viewport(viewport, self.config().wrap_mode);
            }

            InputMode::Terminal => {
                self.terminal_view.clamp_viewport(viewport);
            }
        }
    }

    pub fn vt100_screen(&self) -> &vt100::Screen {
        self.terminal_view.vt100_parser.screen()
    }

    // TODO: handle resizing terminal 2D because now it is like minicom data loss...
    // TODO: add command to enable sending the 'resize' command to a linux remote
    // NOTE: this will change the canvas size / only do this when the remote change because data
    // will be lost

    pub fn overlay(&self) -> Option<Overlay> {
        self.overlay
    }

    pub fn update(&mut self, action: Action) -> UpdateEffect {
        match action {
            Action::TransmitRaw(bytes) => {
                if self.app_config.local_echo {
                    let text = String::from_utf8_lossy(&bytes);
                    if self.app_config.input_mode == InputMode::Line {
                        self.line_view.push_history(&text);
                    }
                }
                UpdateEffect::Transmit(bytes)
            }
            Action::LineInput(action) => self.line_view.update_action_lineinput(
                self.app_config.local_echo,
                self.app_config.tx_eol_mode,
                action,
            ),
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

    fn update_action_history(&mut self, action: HistoryAction) -> UpdateEffect {
        match (&self.app_config.input_mode, action) {
            (InputMode::Line, HistoryAction::ScrollUp(lines)) => {
                self.line_view.scroll_up(lines);
            }
            (InputMode::Line, HistoryAction::ScrollDown(lines)) => {
                self.line_view.scroll_down(lines);
            }
            (InputMode::Line, HistoryAction::ScrollToBottom) => {
                self.line_view.scroll_to_bottom();
            }
            (InputMode::Line, HistoryAction::ScrollToTop) => {
                self.line_view.scroll_to_top();
            }
            (InputMode::Line, HistoryAction::Clear) => {
                self.line_view.clear_history();
            }
            (InputMode::Line, HistoryAction::PanLeft(cols)) => {
                self.line_view.pan_to_left(cols);
            }
            (InputMode::Line, HistoryAction::PanRight(cols)) => {
                self.line_view.pan_to_right(cols);
            }
            (InputMode::Terminal, HistoryAction::ScrollUp(lines)) => {
                self.terminal_view.scroll_up(lines);
            }
            (InputMode::Terminal, HistoryAction::ScrollDown(lines)) => {
                self.terminal_view.scroll_down(lines);
            }
            (InputMode::Terminal, HistoryAction::ScrollToBottom) => {
                self.terminal_view.scroll_to_bottom();
            }
            (InputMode::Terminal, HistoryAction::ScrollToTop) => {
                self.terminal_view.scroll_to_top();
            }
            (InputMode::Terminal, HistoryAction::PanRight(cols)) => {
                self.terminal_view.pan_to_right(cols, self.viewport.cols);
            }
            (InputMode::Terminal, HistoryAction::PanLeft(cols)) => {
                self.terminal_view.pan_to_left(cols, self.viewport.cols);
            }
            (InputMode::Terminal, HistoryAction::Clear) => todo!(),
        }

        UpdateEffect::None
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
                    self.update_viewport(self.viewport);
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
                    self.update_viewport(self.viewport);
                    UpdateEffect::None
                }
            },
        }
    }

    /// Read-only accessor if outside modules need it
    pub fn config(&self) -> &AppConfig {
        &self.app_config
    }

    pub fn is_prefix_active(&self) -> bool {
        self.prefix_active
    }
}
