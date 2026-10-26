use std::time::{Duration, Instant};

use crate::{
    app::{
        scroller::ScrollStatus,
        selection::{ClickCounter, CopySide},
        types::{FocusTarget, FrameAreas, FrameMap, ViewportSize},
    },
    configs::{
        DualBufferMode, KeyResolver, Mode, SettingsConfig, WrapMode, settings::PrefixWrapBehavior,
    },
    inputs::{
        Action, AppAction, CopyModeAction, HistoryAction, LineInputAction, SearchModeAction,
        SideViewAction,
    },
    logger::{LineLogger, TerminalLogger},
    notification::{NotificationManager, ToastLevel},
    overlays::{ErrorDialog, FilePathDialog, Overlay, OverlayResponse, PopupTarget, SettingKey},
    serial::SerialConfig,
};

pub mod data_processing;
pub mod line_mode;
pub mod raw_scroller;
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

/// How often a drag held at the history's edge scrolls it (MAX-126)
const DRAG_SCROLL_EVERY: Duration = Duration::from_millis(50);

pub struct App {
    settings: SettingsConfig,
    pub key_resolver: KeyResolver,
    pub serial_config: SerialConfig,
    prefix_active: bool,
    overlay: Option<Overlay>,
    pub notifications: NotificationManager,
    pub viewport: ViewportSize,
    frame_areas: FrameAreas,
    frame_map: FrameMap,
    clicks: ClickCounter,
    /// The screen cell of a drag in the history while the button is held, and when the drag
    /// scroll last ran (MAX-126)
    drag_hold: Option<(u16, u16)>,
    last_drag_scroll: Option<Instant>,
    pub line_mode: LineMode,
    pub terminal_mode: TerminalMode,
    pub is_window_focused: bool,
    pointer: Option<(u16, u16)>,
    pub stats: Stats,
}

// Effects outside which can only be handled by main
pub enum UpdateEffect {
    None,
    Transmit(Vec<u8>),
    Reconfigure(SerialConfig),
    CopyToClipboard(String),
    Screenshot(Result<String, String>),
    ToggleAttachSerial,
    ReloadConfig,
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

        let line_view = LineMode::new(
            settings.line_mode.history_size,
            settings.line_mode.send_history_size,
            line_logger,
        );
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
            frame_areas: FrameAreas::default(),
            frame_map: FrameMap::default(),
            clicks: ClickCounter::default(),
            drag_hold: None,
            last_drag_scroll: None,
            line_mode: line_view,
            terminal_mode: terminal_view,
            is_window_focused: true,
            pointer: None,
            stats: Stats {
                received_bytes: 0,
                sent_bytes: 0,
            },
        }
    }

    /// Where the last frame drew the history and the raw panel, set by `render`
    pub fn frame_areas(&self) -> FrameAreas {
        self.frame_areas
    }

    pub fn set_frame_areas(&mut self, areas: FrameAreas) {
        self.frame_areas = areas;
    }

    /// What the last frame showed where, for mouse clicks (MAX-113)
    pub fn pointer(&self) -> Option<(u16, u16)> {
        self.pointer
    }

    pub fn frame_map(&self) -> &FrameMap {
        &self.frame_map
    }

    /// For the renderers to fill, cleared by `render` at the start of each frame
    pub fn frame_map_mut(&mut self) -> &mut FrameMap {
        &mut self.frame_map
    }

    pub fn update_viewport(&mut self, viewport: ViewportSize) {
        self.viewport = viewport;
        match self.settings.mode {
            Mode::Line => {
                self.line_mode
                    .update_viewport(self.settings().line_mode.wrap_mode, viewport);
            }

            Mode::Terminal => {
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
            match self.settings().mode {
                Mode::Line => InputContext::LineInput,
                Mode::Terminal => InputContext::Terminal,
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

        // a mouse move (ignored, or a hover) between the prefix and its key keeps it (MAX-131)
        let keeps_prefix = matches!(
            action,
            Action::App(AppAction::SetPrefix(true) | AppAction::Hover(..)) | Action::Ignore
        );
        if !keeps_prefix && !is_repeatable_history {
            self.prefix_active = false;
        }

        match action {
            Action::App(action) => self.update_action_app(action),
            Action::TransmitRaw(bytes) => {
                if self.settings.local_echo && self.settings.mode == Mode::Line {
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    self.line_mode
                        .push_history(text, &self.settings.timestamp_format);
                }
                UpdateEffect::Transmit(bytes)
            }
            Action::LineInput(action) => {
                // a click in the input leaves the history for it (MAX-113)
                if let LineInputAction::ClickAt(_) = action {
                    if self.is_copy_mode_active() {
                        self.update_action_copy_mode(CopyModeAction::Exit);
                    }
                    if self.is_search_active() {
                        self.update_action_search_mode(SearchModeAction::Cancel);
                    }
                }
                let is_interrupt = matches!(action, LineInputAction::Interrupt);
                let effect = self
                    .line_mode
                    .update_action_lineinput(&self.settings, action);
                if is_interrupt && matches!(effect, UpdateEffect::Transmit(_)) {
                    self.notify_info("^C sent");
                }
                effect
            }
            Action::Terminal(action) => self.terminal_mode.update_action_terminal(action),
            Action::History(action) => self.update_action_history(action),
            Action::CopyMode(action) => self.update_action_copy_mode(action),
            Action::Search(action) => self.update_action_search_mode(action),
            Action::Overlay(action) => {
                if let Some(mut overlay) = self.overlay.take() {
                    let response = overlay.handle_action(action);
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
                        OverlayResponse::ReconnectSerial => UpdateEffect::ToggleAttachSerial,
                        OverlayResponse::Run(action) => {
                            let action = action.to_action(self);
                            self.update(action)
                        }
                        OverlayResponse::StartCapture(path) => {
                            let result = match self.settings.mode {
                                Mode::Line => self.line_mode.start_logging(path),
                                Mode::Terminal => self.terminal_mode.start_logging(path),
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
        }
    }

    pub fn feed_rx(&mut self, chunk: &[u8]) {
        let settings = &self.settings;
        let feed_line = settings.dual_mode_buffering == DualBufferMode::AlwaysBoth
            || settings.mode == Mode::Line;
        let feed_terminal = settings.dual_mode_buffering == DualBufferMode::AlwaysBoth
            || settings.mode == Mode::Terminal;

        if feed_line {
            self.line_mode.feed_rx(
                settings.line_mode.rx_eol,
                settings.encoding,
                &settings.timestamp_format,
                chunk,
            );
        }
        if feed_terminal {
            self.terminal_mode.process_rx(settings.encoding, chunk);
        }
    }

    /// Returns true when something on screen changed with time alone (needs a redraw)
    pub fn tick(&mut self) -> bool {
        self.line_mode.flush_logger_if_due();
        self.terminal_mode.flush_logger_if_due();

        let toasts_expired = self.notifications.tick();
        let drag_scrolled = self.drag_scroll(Instant::now());
        toasts_expired || self.terminal_mode.has_pending_flash() || drag_scrolled
    }

    /// A drag held on the top or bottom row of the history (or the raw panel), or past it,
    /// scrolls it that way every `DRAG_SCROLL_EVERY`, one row more per row past, and the
    /// selection follows onto the rows coming into view, as in tmux (MAX-126). The edge row
    /// counts because the history often touches the window's edge, where the pointer stops.
    /// Returns true when it scrolled
    pub fn drag_scroll(&mut self, now: Instant) -> bool {
        let Some((x, y)) = self.drag_hold else {
            return false;
        };
        let Some(side) = self.copy_side() else {
            self.drag_hold = None;
            return false;
        };
        if self
            .last_drag_scroll
            .is_some_and(|last| now.duration_since(last) < DRAG_SCROLL_EVERY)
        {
            return false;
        }
        let area = match side {
            CopySide::Text => self.frame_map.text_area,
            CopySide::Raw => self.frame_map.raw_area,
        };
        if area.is_empty() {
            return false;
        }
        let (top, bottom) = (area.top(), area.bottom() - 1);
        let scroll = if y <= top {
            HistoryAction::ScrollUp(usize::from(top - y) + 1)
        } else if y >= bottom {
            HistoryAction::ScrollDown(usize::from(y - bottom) + 1)
        } else {
            return false;
        };
        // onto the edge row the last scroll brought in, then the next one
        if let Some(pos) = self.frame_map.copy_position(side, x, y, true) {
            self.update_action_copy_mode(CopyModeAction::MouseDrag(pos, (x, y)));
        }
        self.update_action_history(scroll);
        self.last_drag_scroll = Some(now);
        true
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
            AppAction::Quit => {
                self.overlay = Some(Overlay::ConfirmQuit);
                UpdateEffect::None
            }
            AppAction::DetachSerial => UpdateEffect::ToggleAttachSerial,
            AppAction::ReloadConfig => UpdateEffect::ReloadConfig,
            AppAction::SideView(action) => self.update_side_view(action),
            // the menu's selection follows the pointer, the other parts are only drawn
            // highlighted (`ui::render`)
            AppAction::Hover(x, y) => {
                self.pointer = Some((x, y));
                if let Some(Overlay::ActionMenu(menu)) = &mut self.overlay
                    && let Some((_, PopupTarget::MenuItem(idx))) =
                        self.frame_map.popup_target_at(x, y)
                {
                    menu.selected = idx;
                }
                UpdateEffect::None
            }
        }
    }

    fn update_side_view(&mut self, action: SideViewAction) -> UpdateEffect {
        let raw_view = self.settings.raw_side.view;
        match self.settings.mode {
            Mode::Terminal => {
                self.terminal_mode.handle_side_view_action(action, raw_view);
            }
            Mode::Line => {
                self.line_mode.handle_side_view_action(action, raw_view);
            }
        }

        UpdateEffect::None
    }

    fn update_action_copy_mode(&mut self, action: CopyModeAction) -> UpdateEffect {
        // a click in the history leaves the search for copy mode there (MAX-113)
        if matches!(action, CopyModeAction::MousePress(..)) && self.is_search_active() {
            self.update_action_search_mode(SearchModeAction::Cancel);
        }
        // quick presses on the same cell select its word, then its line (MAX-126)
        let action = match action {
            CopyModeAction::MousePress(pos, side, _) => {
                let col = match side {
                    CopySide::Text => pos.char_idx,
                    CopySide::Raw => pos.byte_idx,
                };
                let unit = self.clicks.press(Instant::now(), (pos.line_idx, col), side);
                self.drag_hold = None;
                CopyModeAction::MousePress(pos, side, unit)
            }
            CopyModeAction::MouseDrag(_, at) => {
                self.clicks.reset();
                self.drag_hold = Some(at);
                action
            }
            CopyModeAction::MouseRelease => {
                self.drag_hold = None;
                action
            }
            action => action,
        };
        match self.settings.mode {
            Mode::Terminal => {
                if let Some(text) = self
                    .terminal_mode
                    .handle_copy_action(action, &self.settings)
                {
                    return UpdateEffect::CopyToClipboard(text);
                }
            }
            Mode::Line => {
                if let Some(text) = self.line_mode.handle_copy_action(action, &self.settings) {
                    return UpdateEffect::CopyToClipboard(text);
                }
            }
        }

        UpdateEffect::None
    }

    fn update_action_search_mode(&mut self, action: SearchModeAction) -> UpdateEffect {
        match self.settings.mode {
            Mode::Terminal => {
                self.terminal_mode
                    .handle_search_action(action, self.settings.raw_side.view);
            }
            Mode::Line => {
                self.line_mode.handle_search_action(action, &self.settings);
            }
        }

        UpdateEffect::None
    }

    fn update_action_history(&mut self, action: HistoryAction) -> UpdateEffect {
        match (&self.settings.mode, action) {
            (Mode::Line, HistoryAction::ScrollUp(lines)) => {
                self.line_mode.scroll_up(lines);
            }
            (Mode::Line, HistoryAction::ScrollDown(lines)) => {
                self.line_mode.scroll_down(lines);
            }
            (Mode::Line, HistoryAction::ScrollUpPage) => {
                self.line_mode.scroll_up_page();
            }
            (Mode::Line, HistoryAction::ScrollDownPage) => {
                self.line_mode.scroll_down_page();
            }
            (Mode::Line, HistoryAction::ScrollToBottom) => {
                self.line_mode.scroll_to_bottom();
            }
            (Mode::Line, HistoryAction::ScrollToTop) => {
                self.line_mode.scroll_to_top();
            }
            (Mode::Line, HistoryAction::Clear) => {
                self.line_mode.clear_history();
            }
            (Mode::Line, HistoryAction::PanLeft(cols)) => {
                self.line_mode.pan_to_left(cols);
            }
            (Mode::Line, HistoryAction::PanRight(cols)) => {
                self.line_mode.pan_to_right(cols);
            }
            (Mode::Terminal, HistoryAction::ScrollUp(lines)) => {
                self.terminal_mode.scroll_up(lines);
            }
            (Mode::Terminal, HistoryAction::ScrollDown(lines)) => {
                self.terminal_mode.scroll_down(lines);
            }
            (Mode::Terminal, HistoryAction::ScrollUpPage) => {
                self.terminal_mode.scroll_up_page();
            }
            (Mode::Terminal, HistoryAction::ScrollDownPage) => {
                self.terminal_mode.scroll_down_page();
            }
            (Mode::Terminal, HistoryAction::ScrollToBottom) => {
                self.terminal_mode.scroll_to_bottom();
            }
            (Mode::Terminal, HistoryAction::ScrollToTop) => {
                self.terminal_mode.scroll_to_top();
            }
            (Mode::Terminal, HistoryAction::PanRight(cols)) => {
                self.terminal_mode.pan_to_right(cols, self.viewport.cols);
            }
            (Mode::Terminal, HistoryAction::PanLeft(cols)) => {
                self.terminal_mode.pan_to_left(cols, self.viewport.cols);
            }
            (Mode::Terminal, HistoryAction::Clear) => self.terminal_mode.clear_history(),
        }

        UpdateEffect::None
    }

    /// Applies reloaded config files (MAX-112): their settings win over the session toggles,
    /// except the ones only read at startup (history sizes, emulator size, logging), which keep
    /// their running value and are named in the notice
    pub fn apply_reload(&mut self, mut settings: SettingsConfig, key_resolver: KeyResolver) {
        let old = &self.settings;
        let mut restart_needed = Vec::new();
        let mut keep = |name: &'static str, is_changed: bool| {
            if is_changed {
                restart_needed.push(name);
            }
        };
        let (line, new_line) = (&old.line_mode, &mut settings.line_mode);
        keep(
            "line_mode.history_size",
            line.history_size != new_line.history_size,
        );
        keep(
            "line_mode.send_history_size",
            line.send_history_size != new_line.send_history_size,
        );
        keep("line_mode.logging", line.logging != new_line.logging);
        new_line.history_size = line.history_size;
        new_line.send_history_size = line.send_history_size;
        new_line.logging = line.logging.clone();

        let (term, new_term) = (&old.terminal_mode, &mut settings.terminal_mode);
        keep(
            "terminal_mode.history_size",
            term.history_size != new_term.history_size,
        );
        keep("terminal_mode.rows", term.rows != new_term.rows);
        keep("terminal_mode.cols", term.cols != new_term.cols);
        keep("terminal_mode.logging", term.logging != new_term.logging);
        new_term.history_size = term.history_size;
        new_term.rows = term.rows;
        new_term.cols = term.cols;
        new_term.logging = term.logging.clone();

        settings.refresh_timestamp_width();
        self.settings = settings;
        self.key_resolver = key_resolver;
        self.prefix_active = false;
        self.update_viewport(self.viewport);

        if restart_needed.is_empty() {
            self.notify_info("Config reloaded");
        } else {
            self.notify_warn(format!(
                "Config reloaded; restart to apply: {}",
                restart_needed.join(", ")
            ));
        }
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
            SettingKey::Mode => {
                self.settings.mode = match self.settings.mode {
                    Mode::Line => Mode::Terminal,
                    Mode::Terminal => Mode::Line,
                };
                self.update_viewport(self.viewport);
            }
            SettingKey::DisplayTimestamp => {
                self.settings.show_timestamps = !self.settings.show_timestamps;
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
                    PrefixWrapBehavior::Prefix => PrefixWrapBehavior::Timestamp,
                    PrefixWrapBehavior::Timestamp => PrefixWrapBehavior::None,
                }
            }
            SettingKey::DisplayMode => {
                self.settings.line_mode.display_mode.toggle();
                self.notify_info(format!(
                    "Display Mode is {}",
                    self.settings.line_mode.display_mode.as_str()
                ));
            }
            SettingKey::Mouse => {
                self.settings.ui.mouse = !self.settings.ui.mouse;
                let state = if self.settings.ui.mouse { "ON" } else { "OFF" };
                self.notify_info(format!("Mouse capture {state}"));
            }
            SettingKey::DisplayRaw => {
                self.settings.raw_side.view.toggle();
                self.notify_info(format!(
                    "Raw SideView is {}",
                    self.settings.raw_side.view.as_str()
                ));
            }
        }
    }

    fn toggle_logging(&mut self) {
        let (is_logging, active_path) = match self.settings.mode {
            Mode::Line => (
                self.line_mode.is_logging(),
                self.line_mode
                    .logger_active_path()
                    .to_string_lossy()
                    .into_owned(),
            ),
            Mode::Terminal => (
                self.terminal_mode.is_logging(),
                self.terminal_mode
                    .logger_active_path()
                    .to_string_lossy()
                    .into_owned(),
            ),
        };

        if is_logging {
            match self.settings.mode {
                Mode::Line => self.line_mode.stop_logging(),
                Mode::Terminal => self.terminal_mode.stop_logging(),
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

    #[inline]
    pub fn settings_mut(&mut self) -> &mut SettingsConfig {
        &mut self.settings
    }

    pub fn is_prefix_active(&self) -> bool {
        self.prefix_active
    }

    pub fn is_logging_active(&self) -> bool {
        match self.settings().mode {
            Mode::Line => self.line_mode.is_logging(),
            Mode::Terminal => self.terminal_mode.is_logging(),
        }
    }

    pub fn focus_target(&self) -> FocusTarget {
        if self.overlay.is_some() {
            return FocusTarget::Overlay;
        }

        match self.settings().mode {
            Mode::Terminal => FocusTarget::Terminal,
            Mode::Line => {
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
        match self.settings.mode {
            Mode::Line => self.line_mode.copy_state().active,
            Mode::Terminal => self.terminal_mode.copy_state().active,
        }
    }

    pub fn copy_side(&self) -> Option<CopySide> {
        let copy_state = match self.settings.mode {
            Mode::Line => self.line_mode.copy_state(),
            Mode::Terminal => self.terminal_mode.copy_state(),
        };
        copy_state.active.then_some(copy_state.side)
    }

    #[inline]
    pub fn is_search_active(&self) -> bool {
        match self.settings.mode {
            Mode::Line => self.line_mode.search().active,
            Mode::Terminal => self.terminal_mode.search().active,
        }
    }

    #[inline]
    pub fn scroll_status(&self) -> ScrollStatus {
        match self.settings().mode {
            Mode::Terminal => self.terminal_mode.scroller().status(),
            Mode::Line => self.line_mode.scroll_status(),
        }
    }

    #[inline]
    pub fn pan_left_status(&self) -> Option<usize> {
        match self.settings().mode {
            Mode::Terminal => self.terminal_mode.can_pan_left(),
            Mode::Line => self.line_mode.can_pan_left(),
        }
    }

    #[inline]
    pub fn pan_right_status(&self) -> Option<(usize, usize)> {
        match self.settings().mode {
            Mode::Terminal => self.terminal_mode.can_pan_right(),
            Mode::Line => self.line_mode.can_pan_right(),
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

#[cfg(test)]
mod tests {
    //! App-level wiring of the viewport (#35) and of a config reload (MAX-112).

    use super::*;
    use crate::configs::KeyBindingsConfig;

    fn app() -> App {
        App::new(
            SettingsConfig::default(),
            KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap(),
            SerialConfig::default(),
        )
    }

    /// Setting toggles that re-lay out the view reuse the last size `render` gave, not a
    /// default one.
    /// How: a 100x30 viewport in line mode, then the wrap toggle and the mode toggle (into
    /// terminal mode); both scrollers must see 100x30.
    #[test]
    fn setting_toggles_keep_the_viewport() {
        let mut app = app();
        app.settings.mode = Mode::Line;
        app.update_viewport(ViewportSize::new(100, 30));

        app.toggle_setting(SettingKey::LineWrap);
        let scroller = app.line_mode.scroller();
        assert_eq!((scroller.view_width, scroller.view_height), (100, 30));

        app.toggle_setting(SettingKey::Mode);
        assert_eq!(app.settings.mode, Mode::Terminal);
        let scroller = app.terminal_mode.scroller();
        assert_eq!((scroller.view_width, scroller.view_height), (100, 30));
    }

    fn toasts(app: &App) -> Vec<String> {
        app.notifications
            .active_toasts()
            .iter()
            .map(|toast| toast.message.clone())
            .collect()
    }

    /// The reload action asks `main` for the I/O.
    #[test]
    fn reload_action_gives_the_effect() {
        let mut app = app();
        let effect = app.update(Action::App(AppAction::ReloadConfig));
        assert!(matches!(effect, UpdateEffect::ReloadConfig));
    }

    /// A reload takes the file's settings over the session toggles, the new keymap, and the
    /// new timestamp format's width; the view is laid out again and the prefix dropped.
    #[test]
    fn reload_takes_the_files_settings() {
        let mut app = app();
        // only the terminal view is laid out yet: the reload into line mode lays out that one
        app.settings.mode = Mode::Terminal;
        app.update_viewport(ViewportSize::new(100, 30));
        app.toggle_setting(SettingKey::LocalEcho);
        app.toggle_setting(SettingKey::DisplayTimestamp);
        app.update(Action::App(AppAction::SetPrefix(true)));

        let mut settings = SettingsConfig::default();
        settings.mode = Mode::Line;
        settings.line_mode.wrap_mode = WrapMode::Truncate;
        settings.timestamp_format = "%H:%M".to_string();
        let keymap = KeyBindingsConfig {
            prefix: "ctrl-b".to_string(),
            ..Default::default()
        };
        app.apply_reload(settings, KeyResolver::try_new(&keymap).unwrap());

        assert_eq!(app.settings.mode, Mode::Line);
        assert!(!app.settings.local_echo);
        assert!(!app.settings.show_timestamps);
        assert_eq!(app.settings.timestamp_width(), 5);
        assert_eq!(app.key_resolver.prefix.to_string(), "Ctrl-b");
        assert!(!app.is_prefix_active());
        let scroller = app.line_mode.scroller();
        assert_eq!((scroller.view_width, scroller.view_height), (100, 30));
        assert_eq!(toasts(&app), ["Config reloaded"]);
    }

    /// Settings only read at startup keep their running value and are named in a notice;
    /// the cheap ones (inactivity timeout, RX buffer) are taken.
    #[test]
    fn reload_keeps_startup_only_settings() {
        let mut app = app();
        let mut settings = SettingsConfig::default();
        settings.line_mode.history_size = 7;
        settings.terminal_mode.rows = 50;
        settings.terminal_mode.logging.path = "other.log".into();
        settings.inactivity_detach_timeout = Duration::from_secs(9);
        settings.rx_buffer_size = 512;
        settings.encoding.toggle();
        app.apply_reload(
            settings,
            KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap(),
        );

        let defaults = SettingsConfig::default();
        assert_eq!(
            app.settings.line_mode.history_size,
            defaults.line_mode.history_size
        );
        assert_eq!(app.settings.terminal_mode.rows, defaults.terminal_mode.rows);
        assert_eq!(
            app.settings.terminal_mode.logging,
            defaults.terminal_mode.logging
        );
        assert_eq!(
            app.settings.inactivity_detach_timeout,
            Duration::from_secs(9)
        );
        assert_eq!(app.settings.rx_buffer_size, 512);
        assert_ne!(app.settings.encoding, defaults.encoding);
        assert_eq!(
            toasts(&app),
            [
                "Config reloaded; restart to apply: line_mode.history_size, terminal_mode.rows, \
              terminal_mode.logging"
            ]
        );
    }

    /// The mouse toggle flips the capture setting and says so (MAX-113).
    #[test]
    fn mouse_toggle_flips_the_setting() {
        let mut app = app();
        assert!(app.settings.ui.mouse);
        app.toggle_setting(SettingKey::Mouse);
        assert!(!app.settings.ui.mouse);
        assert_eq!(toasts(&app), ["Mouse capture OFF"]);
    }
}
