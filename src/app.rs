/// State machine, updates and queue handling
/// Mainly local functional code (core feature)
use std::collections::VecDeque;
use vt100::Parser as Vt100Parser;

use crate::{
    config::{AppConfig, InputMode, SettingKey, WrapMode},
    input::{Action, AppAction, HistoryAction, OverlayAction},
    logger::{FileLogger, LogConfig},
    types::{Overlay, ViewportSize},
};

pub mod line_mode;
pub mod terminal_mode;

pub use line_mode::LineMode;
pub use terminal_mode::TerminalMode;

pub struct App {
    app_config: AppConfig,
    prefix_active: bool,
    overlay: Option<Overlay>,
    pub viewport: ViewportSize,
    pub line_mode: LineMode,
    pub terminal_mode: TerminalMode,
}

pub enum UpdateEffect {
    None,
    Transmit(Vec<u8>),
    Quit,
}

impl App {
    pub fn new(app_config: AppConfig, log_config: LogConfig) -> Self {
        // TODO: configure size of scrollback history with vt100
        let scrollback_max_possible = 10000;
        let vt100_parser = Vt100Parser::new(24, 80, scrollback_max_possible);

        let line_view = LineMode::new(app_config.max_history_lines, log_config);
        let terminal_view = TerminalMode::new(vt100_parser);

        Self {
            prefix_active: false,
            app_config,
            overlay: None,
            viewport: ViewportSize::default(),
            line_mode: line_view,
            terminal_mode: terminal_view,
        }
    }

    pub fn update_viewport(&mut self, viewport: ViewportSize) {
        match self.app_config.input_mode {
            InputMode::Line => {
                self.line_mode.clamp_wrap(self.config().wrap_mode);
            }

            InputMode::Terminal => {
                self.terminal_mode.update_viewport(viewport);
            }
        }
    }

    pub fn vt100_screen(&self) -> &vt100::Screen {
        self.terminal_mode.vt100_parser.screen()
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
                        self.line_mode.push_history(&text);
                    }
                }
                UpdateEffect::Transmit(bytes)
            }
            Action::LineInput(action) => self.line_mode.update_action_lineinput(
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
                self.line_mode.scroll_up(lines);
            }
            (InputMode::Line, HistoryAction::ScrollDown(lines)) => {
                self.line_mode.scroll_down(lines);
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
