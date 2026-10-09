use std::time::Duration;

use crate::{
    app::{
        scroller::ScrollStatus,
        types::{FocusTarget, ViewportSize},
    },
    configs::{InputMode, KeyResolver, SettingsConfig, WrapMode, settings::PrefixWrapBehavior},
    inputs::{Action, AppAction, CopyModeAction, HistoryAction, SearchModeAction},
    logger::{LineLogger, TerminalLogger},
    notification::{NotificationManager, ToastLevel},
    overlays::{ErrorDialog, FilePathDialog, Overlay, OverlayResponse, SettingKey},
    serial::SerialConfig,
};

pub mod line_mode;
pub mod scroller;
pub mod search;
pub mod selection;
pub mod terminal_mode;
pub mod types;

pub use line_mode::LineMode;
pub use terminal_mode::TerminalMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputContext {
    Overlay,
    Prefix,
    CopyMode,
    Search,
    LineInput,
    Terminal,
}

pub struct Stats {
    received_bytes: usize,
    sent_bytes: usize,
}

impl Stats {
    pub fn update_rx(&mut self, count: usize) {
        self.received_bytes = self.received_bytes.saturating_add(count);
    }
    pub fn update_tx(&mut self, count: usize) {
        self.sent_bytes = self.sent_bytes.saturating_add(count);
    }
    pub fn rx(&self) -> usize {
        self.received_bytes
    }
    pub fn tx(&self) -> usize {
        self.sent_bytes
    }
}

pub struct App {
    settings: SettingsConfig,
    pub key_resolver: KeyResolver,
    pub serial_config: SerialConfig,
    prefix_active: bool,
    overlay: Option<Overlay>,
    pub notifications: NotificationManager,
    pub viewport: ViewportSize,
    pub line_mode: LineMode,
    pub terminal_mode: TerminalMode,
    pub is_window_focused: bool,
    pub stats: Stats,
}

// Effects outside which can only be handled by main
pub enum UpdateEffect {
    None,
    Transmit(Vec<u8>),
    Reconfigure(SerialConfig),
    CopyToClipboard(String),
    Quit,
}

impl App {
    pub fn new(
        settings: SettingsConfig,
        key_resolver: KeyResolver,
        serial_config: SerialConfig,
    ) -> Self {
        let line_logger = LineLogger::new(settings.line_mode.logging.to_owned());
        let terminal_logger = TerminalLogger::new(settings.terminal_mode.logging.to_owned());

        let line_view = LineMode::new(settings.line_mode.max_history_lines, line_logger);
        let terminal_view = TerminalMode::new(
            settings.terminal_mode.history_size,
            settings.terminal_mode.rows,
            settings.terminal_mode.cols,
            terminal_logger,
        );

        Self {
            prefix_active: false,
            settings,
            key_resolver,
            serial_config,
            overlay: None,
            notifications: NotificationManager::new(),
            viewport: ViewportSize::default(),
            line_mode: line_view,
            terminal_mode: terminal_view,
            is_window_focused: true,
            stats: Stats {
                received_bytes: 0,
                sent_bytes: 0,
            },
        }
    }

    pub fn update_viewport(&mut self, viewport: ViewportSize) {
        match self.settings.input_mode {
            InputMode::Line => {
                self.line_mode
                    .update_viewport(self.settings().line_mode.wrap_mode, viewport);
            }

            InputMode::Terminal => {
                self.terminal_mode.update_viewport(
                    viewport,
                    self.settings().terminal_mode.dimension_mode,
                    self.settings().terminal_mode.rows,
                    self.settings().terminal_mode.cols,
                );
            }
        }
    }

    pub fn input_context(&self) -> InputContext {
        if self.overlay().is_some() {
            InputContext::Overlay
        } else if self.is_prefix_active() {
            InputContext::Prefix
        } else if self.is_copy_mode_active() {
            InputContext::CopyMode
        } else if self.is_search_active() {
            InputContext::Search
        } else {
            match self.settings().input_mode {
                InputMode::Line => InputContext::LineInput,
                InputMode::Terminal => InputContext::Terminal,
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

    // TODO: display mode with hexadecimal window on the side

    // TODO: capture screenshot terminal image

    // TODO: replay recorded bytes streams

    // TODO: make a cache so when connect back to fd data is there (with clear opt)

    pub fn update(&mut self, action: Action) -> UpdateEffect {
        let is_repeatable_history = self.prefix_active
            && matches!(
                action,
                Action::History(
                    HistoryAction::ScrollUp(_)
                        | HistoryAction::ScrollDown(_)
                        | HistoryAction::PanLeft(_)
                        | HistoryAction::PanRight(_)
                )
            );

        if !matches!(action, Action::App(AppAction::SetPrefix(true))) && !is_repeatable_history {
            self.prefix_active = false;
        }

        let effect = match action {
            Action::App(action) => self.update_action_app(action),
            Action::TransmitRaw(bytes) => {
                if self.settings.local_echo {
                    let text = String::from_utf8_lossy(&bytes);
                    if self.settings.input_mode == InputMode::Line {
                        self.line_mode
                            .push_history(&text, &self.settings.timestamp_format);
                    }
                }
                UpdateEffect::Transmit(bytes)
            }
            Action::LineInput(action) => self
                .line_mode
                .update_action_lineinput(&self.settings, action),
            Action::Terminal(action) => self.terminal_mode.update_action_terminal(action),
            Action::History(action) => self.update_action_history(action),
            Action::CopyMode(action) => self.update_action_copy_mode(action),
            Action::Search(action) => self.update_action_search_mode(action),
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
                            self.settings.terminal_mode.cols = cols;
                            self.settings.terminal_mode.rows = rows;
                            self.terminal_mode.set_vt100_size(rows, cols);
                            UpdateEffect::None
                        }
                        OverlayResponse::ReconfigureSerial(new_cfg) => {
                            self.serial_config = new_cfg.clone();
                            UpdateEffect::Reconfigure(new_cfg)
                        }
                        OverlayResponse::StartCapture(path) => {
                            let result = match self.settings.input_mode {
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
            Action::Ignore => UpdateEffect::None,
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
            AppAction::OpenOverlay(overlay) => {
                self.overlay = Some(overlay);
                UpdateEffect::None
            }
            AppAction::Quit => UpdateEffect::Quit,
        }
    }

    fn update_action_copy_mode(&mut self, action: CopyModeAction) -> UpdateEffect {
        match self.settings.input_mode {
            InputMode::Terminal => {
                if let Some(text) = self.terminal_mode.handle_copy_action(action) {
                    return UpdateEffect::CopyToClipboard(text);
                }
            }
            InputMode::Line => {
                if let Some(text) = self.line_mode.handle_copy_action(action, &self.settings) {
                    return UpdateEffect::CopyToClipboard(text);
                }
            }
        }

        UpdateEffect::None
    }

    fn update_action_search_mode(&mut self, action: SearchModeAction) -> UpdateEffect {
        match self.settings.input_mode {
            InputMode::Terminal => {
                self.terminal_mode.handle_search_action(action);
            }
            InputMode::Line => {
                self.line_mode
                    .handle_search_action(action, self.settings.line_mode.display_mode);
            }
        }

        UpdateEffect::None
    }

    fn update_action_history(&mut self, action: HistoryAction) -> UpdateEffect {
        match (&self.settings.input_mode, action) {
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
            (InputMode::Terminal, HistoryAction::Clear) => self.terminal_mode.clear_history(),
        }

        UpdateEffect::None
    }

    fn toggle_setting(&mut self, key: SettingKey) {
        match key {
            SettingKey::LocalEcho => {
                self.settings.local_echo = !self.settings.local_echo;
            }
            SettingKey::LineWrap => {
                self.settings.line_mode.wrap_mode = match self.settings.line_mode.wrap_mode {
                    WrapMode::Word => WrapMode::Char,
                    WrapMode::Char => WrapMode::Truncate,
                    WrapMode::Truncate => WrapMode::Word,
                };
                self.update_viewport(self.viewport);
            }
            SettingKey::InputMode => {
                self.settings.input_mode = match self.settings.input_mode {
                    InputMode::Line => InputMode::Terminal,
                    InputMode::Terminal => InputMode::Line,
                };
                self.update_viewport(self.viewport);
            }
            SettingKey::DisplayTimestamp => {
                self.settings.display_live_timestamp = !self.settings.display_live_timestamp;
            }
            SettingKey::TextEncoding => {
                self.settings.encoding.toggle();
                self.notify_info(format!(
                    "Rx Text Encoding is {}",
                    self.settings.encoding.as_str()
                ));
            }
            SettingKey::IdentWrap => {
                self.settings.line_mode.wrap_indent = match self.settings.line_mode.wrap_indent {
                    PrefixWrapBehavior::None => PrefixWrapBehavior::Prefix,
                    PrefixWrapBehavior::Prefix => PrefixWrapBehavior::None,
                }
            }
            SettingKey::DisplayMode => {
                self.settings.line_mode.display_mode.toggle();
                self.notify_info(format!(
                    "Display Mode is {}",
                    self.settings.line_mode.display_mode.as_str()
                ));
            }
        }
    }

    fn toggle_logging(&mut self) {
        let (is_logging, active_path) = match self.settings.input_mode {
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
            match self.settings.input_mode {
                InputMode::Line => self.line_mode.stop_logging(),
                InputMode::Terminal => self.terminal_mode.stop_logging(),
            }
            self.notify_info("Logging stopped");
        } else {
            self.open_overlay(Overlay::FilePath(FilePathDialog::new(&active_path)));
        }
    }

    #[inline]
    pub fn settings(&self) -> &SettingsConfig {
        &self.settings
    }

    pub fn is_prefix_active(&self) -> bool {
        self.prefix_active
    }

    pub fn is_logging_active(&self) -> bool {
        match self.settings().input_mode {
            InputMode::Line => self.line_mode.is_logging(),
            InputMode::Terminal => self.terminal_mode.is_logging(),
        }
    }

    pub fn focus_target(&self) -> FocusTarget {
        if self.overlay.is_some() {
            return FocusTarget::Overlay;
        }

        match self.settings().input_mode {
            InputMode::Terminal => FocusTarget::Terminal,
            InputMode::Line => {
                if self.line_mode.search().active || self.line_mode.copy_state().active {
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

    #[inline]
    pub fn is_copy_mode_active(&self) -> bool {
        match self.settings.input_mode {
            InputMode::Line => self.line_mode.copy_state().active,
            InputMode::Terminal => self.terminal_mode.copy_state().active,
        }
    }

    #[inline]
    pub fn is_search_active(&self) -> bool {
        match self.settings.input_mode {
            InputMode::Line => self.line_mode.search().active,
            InputMode::Terminal => self.terminal_mode.search().active,
        }
    }

    #[inline]
    pub fn scroll_status(&self) -> ScrollStatus {
        match self.settings().input_mode {
            InputMode::Terminal => self.terminal_mode.scroller().status(),
            InputMode::Line => self.line_mode.scroller().status(),
        }
    }

    #[inline]
    pub fn pan_left_status(&self) -> Option<usize> {
        match self.settings().input_mode {
            InputMode::Terminal => self.terminal_mode.can_pan_left(),
            InputMode::Line => self.line_mode.can_pan_left(),
        }
    }

    #[inline]
    pub fn pan_right_status(&self) -> Option<(usize, usize)> {
        match self.settings().input_mode {
            InputMode::Terminal => self.terminal_mode.can_pan_right(),
            InputMode::Line => self.line_mode.can_pan_right(),
        }
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
