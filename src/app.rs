use std::time::Duration;

use crate::{
    config::{AppConfig, InputMode, WrapMode},
    input::{Action, AppAction, HistoryAction},
    logger::{LineLogger, TerminalLogger},
    notification::{NotificationManager, ToastLevel},
    serial::SerialConfig,
    types::{
        ErrorDialog, FilePathDialog, FocusTarget, Overlay, OverlayResponse, SettingKey,
        ViewportSize,
    },
};

pub mod line_mode;
pub mod terminal_mode;

pub use line_mode::LineMode;
pub use terminal_mode::TerminalMode;

pub struct App {
    app_config: AppConfig,
    pub serial_config: SerialConfig,
    prefix_active: bool,
    overlay: Option<Overlay>,
    pub notifications: NotificationManager,
    pub viewport: ViewportSize,
    pub line_mode: LineMode,
    pub terminal_mode: TerminalMode,
    pub is_window_focused: bool,
}

// Effects outside which can only be handled by main
pub enum UpdateEffect {
    None,
    Transmit(Vec<u8>),
    Reconfigure(SerialConfig),
    Quit,
}

impl App {
    pub fn new(app_config: AppConfig, serial_config: SerialConfig) -> Self {
        let line_logger = LineLogger::new(app_config.line_mode.logging.to_owned());
        let terminal_logger = TerminalLogger::new(app_config.terminal_mode.logging.to_owned());

        let line_view = LineMode::new(app_config.line_mode.max_history_lines, line_logger);
        let terminal_view = TerminalMode::new(
            app_config.terminal_mode.history_size,
            app_config.terminal_mode.rows,
            app_config.terminal_mode.cols,
            terminal_logger,
        );

        Self {
            prefix_active: false,
            app_config,
            serial_config,
            overlay: None,
            notifications: NotificationManager::new(),
            viewport: ViewportSize::default(),
            line_mode: line_view,
            terminal_mode: terminal_view,
            is_window_focused: true,
        }
    }

    pub fn update_viewport(&mut self, viewport: ViewportSize) {
        match self.app_config.input_mode {
            InputMode::Line => {
                self.line_mode
                    .update_viewport(self.config().line_mode.wrap_mode, viewport);
            }

            InputMode::Terminal => {
                self.terminal_mode.update_viewport(viewport);
            }
        }
    }

    pub fn overlay(&self) -> Option<&Overlay> {
        self.overlay.as_ref()
    }

    pub fn open_overlay(&mut self, overlay: Overlay) {
        self.overlay = Some(overlay);
    }

    pub fn close_overlay(&mut self) {
        self.overlay = None;
    }

    pub fn update(&mut self, action: Action) -> UpdateEffect {
        let effect = match action {
            Action::TransmitRaw(bytes) => {
                if self.app_config.local_echo {
                    let text = String::from_utf8_lossy(&bytes);
                    if self.app_config.input_mode == InputMode::Line {
                        self.line_mode.push_history(&text);
                    }
                }
                UpdateEffect::Transmit(bytes)
            }
            Action::LineInput(action) => self.line_mode.update_action_lineinput(
                self.app_config.local_echo,
                self.app_config.line_mode.tx_eol,
                action,
            ),
            Action::Terminal(action) => self.terminal_mode.update_action_terminal(action),
            Action::History(action) => self.update_action_history(action),
            Action::App(action) => self.update_action_app(action),
            Action::Ignore => UpdateEffect::None,
            Action::OpenOverlay(overlay) => {
                self.overlay = Some(overlay);
                UpdateEffect::None
            }
            Action::OverlayKey(key) => {
                if let Some(mut overlay) = self.overlay.take() {
                    let response = overlay.handle_key(key);
                    match response {
                        OverlayResponse::Handled => {
                            self.overlay = Some(overlay); // overlay taken, so give back
                            UpdateEffect::None
                        }
                        OverlayResponse::Close => UpdateEffect::None,
                        OverlayResponse::Quit => UpdateEffect::Quit,
                        OverlayResponse::ToggleSetting(key) => {
                            self.toggle_setting(key);
                            self.overlay = Some(overlay);
                            UpdateEffect::None
                        }
                        OverlayResponse::ResizeTerminal { rows, cols } => {
                            self.terminal_mode.set_vt100_size(rows, cols);
                            UpdateEffect::None
                        }
                        OverlayResponse::ReconfigureSerial(new_cfg) => {
                            self.serial_config = new_cfg.clone();
                            UpdateEffect::Reconfigure(new_cfg)
                        }
                        OverlayResponse::StartCapture(path) => {
                            let result = match self.app_config.input_mode {
                                InputMode::Line => self.line_mode.start_logging(path),
                                InputMode::Terminal => self.terminal_mode.start_logging(path),
                            };

                            match result {
                                Ok(_) => {
                                    self.notify_success("Started recording");
                                }
                                Err(e) => {
                                    self.open_overlay(Overlay::Error(
                                        ErrorDialog::new(
                                            "Capture Failed",
                                            "Could not open file for writing",
                                        )
                                        .with_details(e),
                                    ));
                                }
                            }
                            UpdateEffect::None
                        }
                    }
                } else {
                    UpdateEffect::None
                }
            }
        };

        self.notifications.tick();
        effect
    }

    fn update_action_app(&mut self, action: AppAction) -> UpdateEffect {
        match action {
            AppAction::SetPrefix(state) => {
                self.prefix_active = state;
                UpdateEffect::None
            }
            AppAction::FocusGained(is_focused) => {
                self.is_window_focused = is_focused;
                UpdateEffect::None
            }
            AppAction::ToggleSetting(setting) => {
                self.toggle_setting(setting);
                UpdateEffect::None
            }
            AppAction::ToggleLogging => {
                self.toggle_logging();
                UpdateEffect::None
            }
            AppAction::Quit => UpdateEffect::Quit,
        }
    }

    fn update_action_history(&mut self, action: HistoryAction) -> UpdateEffect {
        match (&self.app_config.input_mode, action) {
            (InputMode::Line, HistoryAction::ScrollUp(lines)) => {
                self.line_mode.scroll_up(lines);
            }
            (InputMode::Line, HistoryAction::ScrollDown(lines)) => {
                self.line_mode.scroll_down(lines);
            }
            (InputMode::Line, HistoryAction::ScrollUpPage) => {
                self.line_mode.scroll_up_page();
            }
            (InputMode::Line, HistoryAction::ScrollDownPage) => {
                self.line_mode.scroll_down_page();
            }
            (InputMode::Line, HistoryAction::ScrollToBottom) => {
                self.line_mode.scroll_to_bottom();
            }
            (InputMode::Line, HistoryAction::ScrollToTop) => {
                self.line_mode.scroll_to_top();
            }
            (InputMode::Line, HistoryAction::Clear) => {
                self.line_mode.clear_history();
            }
            (InputMode::Line, HistoryAction::PanLeft(cols)) => {
                self.line_mode.pan_to_left(cols);
            }
            (InputMode::Line, HistoryAction::PanRight(cols)) => {
                self.line_mode.pan_to_right(cols);
            }
            (InputMode::Terminal, HistoryAction::ScrollUp(lines)) => {
                self.terminal_mode.scroll_up(lines);
            }
            (InputMode::Terminal, HistoryAction::ScrollDown(lines)) => {
                self.terminal_mode.scroll_down(lines);
            }
            (InputMode::Terminal, HistoryAction::ScrollUpPage) => {
                self.terminal_mode.scroll_up_page();
            }
            (InputMode::Terminal, HistoryAction::ScrollDownPage) => {
                self.terminal_mode.scroll_down_page();
            }
            (InputMode::Terminal, HistoryAction::ScrollToBottom) => {
                self.terminal_mode.scroll_to_bottom();
            }
            (InputMode::Terminal, HistoryAction::ScrollToTop) => {
                self.terminal_mode.scroll_to_top();
            }
            (InputMode::Terminal, HistoryAction::PanRight(cols)) => {
                self.terminal_mode.pan_to_right(cols, self.viewport.cols);
            }
            (InputMode::Terminal, HistoryAction::PanLeft(cols)) => {
                self.terminal_mode.pan_to_left(cols, self.viewport.cols);
            }
            (InputMode::Terminal, HistoryAction::Clear) => todo!(),
        }

        UpdateEffect::None
    }

    fn toggle_setting(&mut self, key: SettingKey) {
        match key {
            SettingKey::LocalEcho => {
                self.app_config.local_echo = !self.app_config.local_echo;
            }
            SettingKey::LineWrap => {
                self.app_config.line_mode.wrap_mode = match self.app_config.line_mode.wrap_mode {
                    WrapMode::Wrap => WrapMode::Truncate,
                    WrapMode::Truncate => WrapMode::Wrap,
                };
                self.update_viewport(self.viewport);
            }
            SettingKey::InputMode => {
                self.app_config.input_mode = match self.app_config.input_mode {
                    InputMode::Line => InputMode::Terminal,
                    InputMode::Terminal => InputMode::Line,
                };
                self.update_viewport(self.viewport);
            }
            SettingKey::DisplayTimestamp => {
                self.app_config.display_live_timestamp = !self.app_config.display_live_timestamp;
            }
        }
    }

    fn toggle_logging(&mut self) {
        let (is_logging, active_path) = match self.app_config.input_mode {
            InputMode::Line => (
                self.line_mode.is_logging(),
                self.line_mode
                    .logger_active_path()
                    .to_string_lossy()
                    .into_owned(),
            ),
            InputMode::Terminal => (
                self.terminal_mode.is_logging(),
                self.terminal_mode
                    .logger_active_path()
                    .to_string_lossy()
                    .into_owned(),
            ),
        };

        if is_logging {
            match self.app_config.input_mode {
                InputMode::Line => self.line_mode.stop_logging(),
                InputMode::Terminal => self.terminal_mode.stop_logging(),
            }
            self.notify_info("Logging stopped");
        } else {
            self.open_overlay(Overlay::FilePath(FilePathDialog::new(&active_path)));
        }
    }

    /// Read-only accessor if outside modules need it
    pub fn config(&self) -> &AppConfig {
        &self.app_config
    }

    pub fn is_prefix_active(&self) -> bool {
        self.prefix_active
    }

    pub fn focus_target(&self) -> FocusTarget {
        if self.overlay.is_some() {
            return FocusTarget::Overlay;
        }

        match self.config().input_mode {
            InputMode::Terminal => FocusTarget::Terminal,
            InputMode::Line => {
                if self.line_mode.is_search_mode() {
                    FocusTarget::LineHistory
                } else {
                    FocusTarget::LineInput
                }
            }
        }
    }

    #[inline]
    pub fn is_app_focused(&self) -> bool {
        self.is_window_focused
    }

    pub fn notify_info(&mut self, msg: impl Into<String>) {
        self.notifications
            .push(msg, ToastLevel::Info, Duration::from_secs(3));
    }

    pub fn notify_success(&mut self, msg: impl Into<String>) {
        self.notifications
            .push(msg, ToastLevel::Success, std::time::Duration::from_secs(3));
    }

    pub fn notify_warn(&mut self, msg: impl Into<String>) {
        self.notifications
            .push(msg, ToastLevel::Warn, std::time::Duration::from_secs(4));
    }

    pub fn notify_error(&mut self, msg: impl Into<String>) {
        self.notifications
            .push(msg, ToastLevel::Error, std::time::Duration::from_secs(5));
    }
}
