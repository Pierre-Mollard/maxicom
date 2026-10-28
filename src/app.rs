use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::{
    app::{
        scroller::ScrollStatus,
        selection::{ClickCounter, CopySide},
        types::{FocusTarget, FrameAreas, FrameMap, LogEntry, ViewportSize},
    },
    configs::{
        DualBufferMode, KeyBindingAction, KeyResolver, Mode, SettingsConfig, StatusBarLayout,
        WrapMode, settings::PrefixWrapBehavior,
    },
    inputs::{
        Action, AppAction, ClickHoldAction, CopyModeAction, HistoryAction, LineInputAction,
        ScrollbarAction, SearchModeAction, SideViewAction, TerminalAction,
    },
    logger::{
        JournalReader, JournalText, PastSession, RESTORE_RAW_TAIL, Recorder, ScreenshotFormat,
        SessionDevice, Stream, create_parent_dirs, expand_home, read_tail_bytes, read_tail_lines,
        restore_separator,
    },
    notification::{NotificationManager, ToastLevel},
    overlays::{
        ErrorDialog, FilePathDialog, Overlay, OverlayResponse, PopupTarget, ScreenshotDialog,
        SettingKey,
    },
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
/// How long the scrollbar's `▲` or `▼` (MAX-142), or a click target that repeats (MAX-143), is
/// held before it repeats
const SCROLLBAR_REPEAT_AFTER: Duration = Duration::from_millis(300);

/// What the left button holds on the scrollbar since its press (MAX-142): an arrow, or the thumb
/// by one of its rows
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScrollbarHold {
    Up,
    Down,
    Thumb { grab: usize },
}

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
    scrollbar_hold: Option<ScrollbarHold>,
    /// When a held scrollbar arrow scrolls again
    scrollbar_repeat_at: Option<Instant>,
    /// The action and cells of the click target the left button holds (MAX-143), and when it
    /// runs again
    click_hold: Option<(KeyBindingAction, ratatui::layout::Rect)>,
    click_repeat_at: Option<Instant>,
    pub line_mode: LineMode,
    pub terminal_mode: TerminalMode,
    recorder: Recorder,
    pub is_window_focused: bool,
    pointer: Option<(u16, u16)>,
    /// The last text copied, which a right-click pastes (MAX-144)
    paste_buffer: Option<String>,
    /// The last frame was smaller than `compact_cols` x `compact_rows` (MAX-136)
    compact: bool,
    pub stats: Stats,
}

// Effects outside which can only be handled by main
pub enum UpdateEffect {
    None,
    Transmit(Vec<u8>),
    Reconfigure(SerialConfig),
    CopyToClipboard(String),
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
        let line_view = LineMode::new(
            settings.line_mode.history_size,
            settings.line_mode.send_history_size,
        );
        let terminal_view = TerminalMode::new(
            settings.terminal_mode.history_size,
            settings.terminal_mode.rows,
            settings.terminal_mode.cols,
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
            scrollbar_hold: None,
            scrollbar_repeat_at: None,
            click_hold: None,
            click_repeat_at: None,
            line_mode: line_view,
            terminal_mode: terminal_view,
            recorder: Recorder::default(),
            is_window_focused: true,
            pointer: None,
            paste_buffer: None,
            compact: false,
            stats: Stats {
                received_bytes: 0,
                sent_bytes: 0,
            },
        }
    }

    /// Draws the next frames compact while the screen is smaller than `compact_cols` x
    /// `compact_rows` (a `0` never is): no borders, status bar or scrollbar, the settings left
    /// as they are (MAX-136). Set by `render`, so the mouse sees what the last frame drew
    pub fn set_screen_size(&mut self, cols: u16, rows: u16) {
        let ui = &self.settings.ui;
        self.compact = cols < ui.compact_cols || rows < ui.compact_rows;
    }

    pub fn show_borders(&self) -> bool {
        self.settings.ui.show_borders && !self.compact
    }

    pub fn show_status_bar(&self) -> bool {
        self.settings.ui.show_status_bar && !self.compact
    }

    pub fn show_scrollbar(&self) -> bool {
        self.settings.ui.show_scrollbar && !self.compact
    }

    pub fn status_bar_layout(&self) -> StatusBarLayout {
        self.settings.ui.status_bar_layout(self.show_borders())
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
                    self.line_mode.push_history(text);
                }
                UpdateEffect::Transmit(bytes)
            }
            Action::LineInput(action) => {
                // a click in the input leaves the history for it (MAX-113), a right-click
                // paste too (MAX-144)
                if let LineInputAction::ClickAt(_) | LineInputAction::Paste(_) = action {
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
            Action::Terminal(TerminalAction::Screenshot(format)) => {
                self.take_screenshot(format);
                UpdateEffect::None
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
                        OverlayResponse::StartCapture {
                            path,
                            stream,
                            include_session,
                        } => {
                            let path = expand_home(&path);
                            match self.recorder.start(stream, path, include_session) {
                                Ok(_) => {
                                    self.notify_success(format!(
                                        "Started recording {}",
                                        stream.as_str()
                                    ));
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
                        OverlayResponse::SaveScreenshot { path, contents } => {
                            let path = expand_home(&path);
                            let saved = create_parent_dirs(&path)
                                .and_then(|()| std::fs::write(&path, contents));
                            match saved {
                                Ok(()) => self.notify_success(format!(
                                    "Screenshot saved to {}",
                                    path.display()
                                )),
                                Err(e) => {
                                    self.notify_error(format!("Failed to save screenshot: {e}"))
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
        let timestamp_format = settings
            .capture
            .timestamp
            .then_some(settings.timestamp_format.as_str());
        self.recorder.record_rx(
            settings.line_mode.rx_eol,
            settings.encoding,
            timestamp_format,
            chunk,
        );

        let feed_line = settings.dual_mode_buffering == DualBufferMode::AlwaysBoth
            || settings.mode == Mode::Line;
        let feed_terminal = settings.dual_mode_buffering == DualBufferMode::AlwaysBoth
            || settings.mode == Mode::Terminal;

        if feed_line {
            self.line_mode
                .feed_rx(settings.line_mode.rx_eol, settings.encoding, chunk);
        }
        if feed_terminal {
            self.terminal_mode.process_rx(settings.encoding, chunk);
        }
    }

    /// Takes the terminal screen now, in both formats, and opens the dialog asking where to
    /// save it and which format, `format` chosen first, so a fast screen is caught. Line mode
    /// doesn't show that screen, so it is not taken there (MAX-147)
    fn take_screenshot(&mut self, format: ScreenshotFormat) {
        if self.settings.mode != Mode::Terminal {
            self.notify_info("Screenshots are in terminal mode");
            return;
        }
        let shot = self.terminal_mode.screenshot();
        let stem = self.settings.capture.screenshot_stem();
        let dialog = ScreenshotDialog::new(format, &stem.to_string_lossy(), shot);
        self.open_overlay(Overlay::Screenshot(dialog));
    }

    /// Puts a past session in the history of both modes, before any live data (MAX-148): the
    /// last lines of its text in line mode, read back with their times and marks, the end of
    /// its raw bytes through the terminal, then a separator where the live data starts. Not
    /// recorded nor counted: the new session starts empty
    // TODO(MAX-148): called at startup (step 4)
    #[allow(dead_code)]
    pub fn restore_session(&mut self, session: &PastSession) -> std::io::Result<()> {
        let text = read_tail_lines(&session.text_path, self.settings.line_mode.history_size)?;
        let raw = read_tail_bytes(&session.raw_path, RESTORE_RAW_TAIL)?;
        let separator = restore_separator(session);

        let text = String::from_utf8_lossy(&text);
        let mut reader = JournalReader::new(&session.text, session.started);
        let lines = text.lines().map(|line| {
            let line = reader.parse(line);
            LogEntry::restored(line.text.to_string(), line.time)
        });
        let separator_line = LogEntry::restored(separator.clone(), None);
        self.line_mode
            .restore_entries(lines.chain(std::iter::once(separator_line)));
        self.terminal_mode
            .restore(self.settings.encoding, &raw, &separator);
        Ok(())
    }

    /// Records what was sent to the text streams, in line mode only: in terminal mode the keys
    /// go one by one and the remote echoes them, so the received text has them (MAX-147)
    pub fn record_tx(&mut self, bytes: &[u8]) {
        let settings = &self.settings;
        if settings.mode != Mode::Line {
            return;
        }
        let capture = &settings.capture;
        let timestamp_format = capture
            .timestamp
            .then_some(settings.timestamp_format.as_str());
        // the journal records them as it started, whatever `record_tx` is now (MAX-148)
        let mark = capture.record_tx.then_some(capture.tx_mark.as_str());
        self.recorder
            .record_tx(settings.encoding, timestamp_format, mark, bytes);
    }

    /// Returns true when something on screen changed with time alone (needs a redraw)
    pub fn tick(&mut self) -> bool {
        self.recorder.flush_if_due();

        let toasts_expired = self.notifications.tick();
        let now = Instant::now();
        let drag_scrolled =
            self.drag_scroll(now) || self.scrollbar_repeat(now) || self.click_repeat(now);
        toasts_expired || self.terminal_mode.has_pending_flash() || drag_scrolled
    }

    /// The left button is on the scrollbar, from its press to its release (MAX-142)
    pub fn is_scrollbar_held(&self) -> bool {
        self.scrollbar_hold.is_some()
    }

    /// The left button on the history's scrollbar, as the last frame drew it (MAX-142): `▲` or
    /// `▼` scrolls a row, then again every `DRAG_SCROLL_EVERY` while held; a press on the thumb
    /// grabs it by that row, on the rest of the track it puts the thumb's middle there first;
    /// a drag then moves the thumb with the pointer
    fn scrollbar(&mut self, action: ScrollbarAction, now: Instant) {
        let Some(bar) = self.frame_map.scrollbar else {
            self.scrollbar_hold = None;
            return;
        };
        let to_thumb = |first: isize| HistoryAction::ScrollTo(bar.max - bar.position_at(first));
        match action {
            ScrollbarAction::Press(row) => {
                let (hold, scroll) = if row == 0 {
                    (ScrollbarHold::Up, HistoryAction::ScrollUp(1))
                } else if row + 1 >= bar.area.height {
                    (ScrollbarHold::Down, HistoryAction::ScrollDown(1))
                } else {
                    let in_track = usize::from(row - 1);
                    let (first, length) = bar.thumb();
                    let grab = if (first..first + length).contains(&in_track) {
                        in_track - first
                    } else {
                        length / 2
                    };
                    let scroll = to_thumb(in_track as isize - grab as isize);
                    (ScrollbarHold::Thumb { grab }, scroll)
                };
                self.scrollbar_hold = Some(hold);
                self.scrollbar_repeat_at = Some(now + SCROLLBAR_REPEAT_AFTER);
                self.update_action_history(scroll);
            }
            ScrollbarAction::Drag(row) => {
                if let Some(ScrollbarHold::Thumb { grab }) = self.scrollbar_hold {
                    let first = row as isize - 1 - grab as isize;
                    self.update_action_history(to_thumb(first));
                }
            }
            ScrollbarAction::Release => self.scrollbar_hold = None,
        }
    }

    /// The left button is on a click target that repeats, from its press to its release or
    /// until the pointer leaves it (MAX-143)
    pub fn is_click_held(&self) -> bool {
        self.click_hold.is_some()
    }

    pub fn has_paste_buffer(&self) -> bool {
        self.paste_buffer.is_some()
    }

    /// The left button on a click target that repeats (MAX-143): its action runs on the press,
    /// then again every `DRAG_SCROLL_EVERY` while held there, as the scrollbar's arrows do
    fn click_hold(&mut self, action: ClickHoldAction, now: Instant) {
        match action {
            ClickHoldAction::Press(key_action, area) => {
                self.click_hold = Some((key_action, area));
                self.click_repeat_at = Some(now + SCROLLBAR_REPEAT_AFTER);
                let action = key_action.to_action(self);
                self.update(action);
            }
            ClickHoldAction::Drag(x, y) => {
                let on_it = self
                    .click_hold
                    .is_some_and(|(_, area)| area.contains(ratatui::layout::Position::new(x, y)));
                if !on_it {
                    self.click_hold = None;
                }
            }
            ClickHoldAction::Release => self.click_hold = None,
        }
    }

    /// A held click target runs again; returns true when it did
    pub fn click_repeat(&mut self, now: Instant) -> bool {
        let Some((key_action, _)) = self.click_hold else {
            return false;
        };
        if self.click_repeat_at.is_some_and(|at| now < at) {
            return false;
        }
        self.click_repeat_at = Some(now + DRAG_SCROLL_EVERY);
        let action = key_action.to_action(self);
        self.update(action);
        true
    }

    /// A scrollbar arrow held scrolls again; returns true when it did
    pub fn scrollbar_repeat(&mut self, now: Instant) -> bool {
        let scroll = match self.scrollbar_hold {
            Some(ScrollbarHold::Up) => HistoryAction::ScrollUp(1),
            Some(ScrollbarHold::Down) => HistoryAction::ScrollDown(1),
            _ => return false,
        };
        if self.scrollbar_repeat_at.is_some_and(|at| now < at) {
            return false;
        }
        self.scrollbar_repeat_at = Some(now + DRAG_SCROLL_EVERY);
        self.update_action_history(scroll);
        true
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
            AppAction::ToggleRecording(stream) => {
                self.toggle_recording(stream);
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
            AppAction::PasteBuffer => {
                let Some(text) = self.paste_buffer.clone() else {
                    return UpdateEffect::None;
                };
                self.notify_info(format!("Pasted {} chars", text.chars().count()));
                match self.settings.mode {
                    Mode::Terminal => self.update(Action::Terminal(TerminalAction::Paste(text))),
                    Mode::Line => self.update(Action::LineInput(LineInputAction::Paste(text))),
                }
            }
            AppAction::DetachSerial => UpdateEffect::ToggleAttachSerial,
            AppAction::ReloadConfig => UpdateEffect::ReloadConfig,
            AppAction::SideView(action) => self.update_side_view(action),
            // the menu's selection follows the pointer, the other parts are only drawn
            // highlighted (`ui::render`)
            AppAction::Scrollbar(action) => {
                self.scrollbar(action, Instant::now());
                UpdateEffect::None
            }
            AppAction::ClickHold(action) => {
                self.click_hold(action, Instant::now());
                UpdateEffect::None
            }
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
        let copied = match self.settings.mode {
            Mode::Terminal => self
                .terminal_mode
                .handle_copy_action(action, &self.settings),
            Mode::Line => self.line_mode.handle_copy_action(action, &self.settings),
        };
        match copied {
            Some(text) => {
                self.paste_buffer = Some(text.clone());
                UpdateEffect::CopyToClipboard(text)
            }
            None => UpdateEffect::None,
        }
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
            (Mode::Line, HistoryAction::ScrollTo(rows)) => self.line_mode.scroll_to_row(rows),
            (Mode::Terminal, HistoryAction::ScrollTo(rows)) => {
                self.terminal_mode.scroll_to_row(rows)
            }
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
        new_line.history_size = line.history_size;
        new_line.send_history_size = line.send_history_size;

        let (term, new_term) = (&old.terminal_mode, &mut settings.terminal_mode);
        keep(
            "terminal_mode.history_size",
            term.history_size != new_term.history_size,
        );
        keep("terminal_mode.rows", term.rows != new_term.rows);
        keep("terminal_mode.cols", term.cols != new_term.cols);
        new_term.history_size = term.history_size;
        new_term.rows = term.rows;
        new_term.cols = term.cols;

        let (capture, new_capture) = (&old.capture, &mut settings.capture);
        keep("capture.journal", capture.journal != new_capture.journal);
        keep(
            "capture.journal_keep",
            capture.journal_keep != new_capture.journal_keep,
        );
        new_capture.journal = capture.journal;
        new_capture.journal_keep = capture.journal_keep;

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
            SettingKey::DimensionMode => {
                self.settings.terminal_mode.dimension_mode.toggle();
                self.update_viewport(self.viewport);
                self.notify_info(format!(
                    "Terminal Size Mode is {}",
                    self.settings.terminal_mode.dimension_mode.as_str()
                ));
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

    /// The stream the recording toggle starts in each mode: text in line mode, raw in terminal
    /// mode
    fn mode_stream(&self) -> Stream {
        match self.settings.mode {
            Mode::Line => Stream::Text,
            Mode::Terminal => Stream::Raw,
        }
    }

    /// Stops every recording, or opens the recording dialog on the mode's stream when none is on
    fn toggle_logging(&mut self) {
        if self.recorder.is_any_active() {
            self.recorder.stop(Stream::Text);
            self.recorder.stop(Stream::Raw);
            self.notify_info("Recording stopped");
        } else {
            self.open_capture_dialog(self.mode_stream());
        }
    }

    fn toggle_recording(&mut self, stream: Stream) {
        if self.recorder.is_active(stream) {
            self.recorder.stop(stream);
            self.notify_info(format!("Stopped recording {}", stream.as_str()));
        } else {
            self.open_capture_dialog(stream);
        }
    }

    /// The dialog on `stream`, each stream's path made from `[capture]` now
    fn open_capture_dialog(&mut self, stream: Stream) {
        let capture = &self.settings.capture;
        let path = |name| capture.path_for(name).to_string_lossy().into_owned();
        let mut dialog =
            FilePathDialog::new(stream, path(&capture.text_name), path(&capture.raw_name));
        if self.recorder.has_journal() {
            dialog = dialog.with_session_backup();
        }
        self.open_overlay(Overlay::FilePath(dialog));
    }

    /// Starts the session backup in `dir` when `capture.journal` is on. Without it, a toast
    /// says so and the app runs on (MAX-147)
    pub fn start_journal(&mut self, dir: Option<PathBuf>) {
        let capture = &self.settings.capture;
        if !capture.journal {
            return;
        }
        let device = SessionDevice::new(&self.serial_config, self.settings.encoding);
        // the journal's text is written as the capture settings say now, reloads aside
        let text = JournalText {
            timestamp: capture
                .timestamp
                .then(|| self.settings.timestamp_format.clone()),
            tx_mark: capture.record_tx.then(|| capture.tx_mark.clone()),
        };
        let result = match dir {
            Some(dir) => self
                .recorder
                .start_journal(&dir, &device, &text, capture.journal_keep),
            None => Err(std::io::Error::other("no cache folder")),
        };
        if let Err(err) = result {
            self.notify_warn(format!("Session backup off: {err}"));
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

    /// The session backup runs, what the optional `journal` status bar condition shows
    pub fn has_journal(&self) -> bool {
        self.recorder.has_journal()
    }

    /// Some recording is on, what `REC` shows
    pub fn is_logging_active(&self) -> bool {
        self.recorder.is_any_active()
    }

    pub fn is_recording(&self, stream: Stream) -> bool {
        self.recorder.is_active(stream)
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

    /// Data arrived while the view was scrolled up, not seen yet (MAX-141)
    pub fn is_new_content_missed(&self) -> bool {
        match self.settings().mode {
            Mode::Terminal => self.terminal_mode.is_new_content_missed(),
            Mode::Line => self.line_mode.is_new_content_missed(),
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
    use crate::{configs::KeyBindingsConfig, inputs::DialogAction};

    fn app() -> App {
        App::new(
            SettingsConfig::default(),
            KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap(),
            SerialConfig::default(),
        )
    }

    /// MAX-147: a text recording gets what is received in terminal mode too, even when line
    /// mode isn't fed (it used to go quiet there).
    /// How: `ActiveOnly` buffering in terminal mode, a text recording, one line received.
    #[test]
    fn text_recording_follows_rx_in_terminal_mode() {
        let mut app = app();
        app.settings.mode = Mode::Terminal;
        app.settings.dual_mode_buffering = DualBufferMode::ActiveOnly;
        app.settings.capture.timestamp = false;
        let path =
            std::env::temp_dir().join(format!("maxicom_test_{}_app_text.log", std::process::id()));
        let _ = std::fs::remove_file(&path);
        app.recorder
            .start(Stream::Text, path.clone(), false)
            .unwrap();

        app.feed_rx(b"seen\r\n");
        app.recorder.stop(Stream::Text);
        assert!(app.line_mode.history().is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "seen\n");
        let _ = std::fs::remove_file(&path);
    }

    /// MAX-147: sent lines are recorded in line mode after `capture.tx_mark`, not in terminal
    /// mode nor with `capture.record_tx` off.
    /// How: a text recording, one send in each case, then one with the mark "TX: " and one
    /// with no mark.
    #[test]
    fn tx_is_recorded_in_line_mode_only() {
        let mut app = app();
        app.settings.capture.timestamp = false;
        let path =
            std::env::temp_dir().join(format!("maxicom_test_{}_app_tx.log", std::process::id()));
        let _ = std::fs::remove_file(&path);
        app.recorder
            .start(Stream::Text, path.clone(), false)
            .unwrap();

        app.settings.mode = Mode::Line;
        app.record_tx(b"line\r\n");
        app.settings.mode = Mode::Terminal;
        app.record_tx(b"terminal\r");
        app.settings.mode = Mode::Line;
        app.settings.capture.record_tx = false;
        app.record_tx(b"off\r\n");
        app.settings.capture.record_tx = true;
        app.settings.capture.tx_mark = "TX: ".to_string();
        app.record_tx(b"marked\r\n");
        app.settings.capture.tx_mark = String::new();
        app.record_tx(b"bare\r\n");
        app.recorder.stop(Stream::Text);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "> line\nTX: marked\nbare\n"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// MAX-147: each stream's action opens the dialog on it or stops it alone; the
    /// `toggle_logging` action (the `REC` click) stops both, and `REC` is on with either.
    /// How: text and raw recordings to temp files, then each action in turn.
    #[test]
    fn recording_actions_per_stream() {
        let mut app = app();
        app.settings.mode = Mode::Line;
        let temp = |name: &str| {
            std::env::temp_dir().join(format!("maxicom_test_{}_{name}", std::process::id()))
        };
        let open_dialog = |app: &App| match app.overlay() {
            Some(Overlay::FilePath(dialog)) => Some(dialog.stream),
            _ => None,
        };

        app.update(Action::App(AppAction::ToggleRecording(Stream::Raw)));
        assert_eq!(open_dialog(&app), Some(Stream::Raw));
        app.close_overlay();
        app.update(Action::App(AppAction::ToggleLogging));
        assert_eq!(open_dialog(&app), Some(Stream::Text));
        app.close_overlay();

        app.recorder
            .start(Stream::Text, temp("act.log"), false)
            .unwrap();
        app.recorder
            .start(Stream::Raw, temp("act.raw"), false)
            .unwrap();
        app.update(Action::App(AppAction::ToggleRecording(Stream::Raw)));
        assert!(app.is_logging_active() && !app.is_recording(Stream::Raw));
        app.recorder
            .start(Stream::Raw, temp("act.raw"), false)
            .unwrap();
        app.update(Action::App(AppAction::ToggleLogging));
        assert!(!app.is_logging_active());
        assert!(app.overlay().is_none());
        let _ = std::fs::remove_file(temp("act.log"));
        let _ = std::fs::remove_file(temp("act.raw"));
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
        settings.capture.journal = false;
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
        assert!(app.settings.capture.journal);
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
              capture.journal"
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

    /// MAX-147: a screenshot action takes the screen at once, then opens its dialog prefilled
    /// with `[capture]`'s folder, name and the format's extension; Enter saves that screen
    /// there, creating the folders.
    /// How: a temp `screenshot_dir` that doesn't exist, a fixed name, "hi" on the screen at
    /// the action, " later" received while the dialog is open.
    #[test]
    fn screenshot_dialog_prefills_and_saves() {
        let mut app = app();
        app.settings.mode = Mode::Terminal;
        let dir = std::env::temp_dir().join(format!("maxicom_test_{}_shots", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        app.settings.capture.screenshot_dir = dir.join("new");
        app.settings.capture.screenshot_name = "shot".to_string();
        app.terminal_mode.process_rx(app.settings.encoding, b"hi");

        app.update(Action::Terminal(TerminalAction::Screenshot(
            ScreenshotFormat::Text,
        )));
        let path = dir.join("new").join("shot.txt");
        match app.overlay() {
            Some(Overlay::Screenshot(dialog)) => {
                assert_eq!(dialog.file_input.value(), path.to_string_lossy())
            }
            _ => panic!("no screenshot dialog"),
        }
        app.terminal_mode
            .process_rx(app.settings.encoding, b" later");
        app.update(Action::Overlay(DialogAction::Confirm));
        assert!(app.overlay().is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim_end(), "hi");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MAX-147: line mode doesn't show the terminal screen, so a screenshot there opens no
    /// dialog and doesn't flash.
    #[test]
    fn screenshot_is_not_taken_in_line_mode() {
        let mut app = app();
        app.settings.mode = Mode::Line;
        app.update(Action::Terminal(TerminalAction::Screenshot(
            ScreenshotFormat::Ansi,
        )));
        assert!(app.overlay().is_none());
        assert!(!app.terminal_mode.has_pending_flash());
    }

    /// MAX-148: a restore puts the session's last lines in line mode with their times (a sent
    /// line without its mark), then the separator without a time; the terminal gets the raw
    /// bytes, back on the main screen. Nothing goes to the new session's journal or the stats.
    /// How: a session with "> " marks and times, ending in the alternate screen; an app with
    /// its own journal restores it.
    #[test]
    fn restore_fills_both_modes_and_records_nothing() {
        let base =
            std::env::temp_dir().join(format!("maxicom_test_{}_restore", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let text = JournalText {
            timestamp: Some("[%H:%M:%S%.3f]".to_string()),
            tx_mark: Some("> ".to_string()),
        };
        let session = crate::logger::write_test_session(
            &base.join("old"),
            "/dev/ttyUSB0",
            &text,
            b"one\r\ntwo\r\n\x1b[?1049h\x1b[?1hvim",
            b"cmd\r\n",
        );

        let mut app = app();
        app.terminal_mode.set_vt100_size(24, 80);
        app.start_journal(Some(base.join("new")));
        app.restore_session(&session).unwrap();

        let history: Vec<_> = app.line_mode.history().iter().collect();
        let texts: Vec<_> = history.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts[..3], ["one", "two", "cmd"]);
        assert!(texts[3].starts_with("── restored: ttyUSB0, "), "{texts:?}");
        assert!(history[..3].iter().all(|e| e.timestamp.is_some()));
        assert_eq!(history[3].timestamp, None);

        let screen = app.terminal_mode.vt100_screen();
        assert!(!screen.alternate_screen() && !screen.application_cursor());
        assert!(screen.contents().contains("one\ntwo\n── restored"));
        assert!(screen.contents().contains("── restored: ttyUSB0, "));
        assert_eq!(app.stats.rx(), 0);

        drop(app);
        for entry in std::fs::read_dir(base.join("new")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|ext| ext != "meta") {
                assert_eq!(std::fs::read(&path).unwrap(), b"", "{path:?}");
            }
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}
