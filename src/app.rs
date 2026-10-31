use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::{
    app::{
        mash::{Mash, MashStep},
        scroller::ScrollStatus,
        selection::{ClickCounter, CopySide},
        term_answers::answer_bytes,
        transfer::FileTransfer,
        triggers::Trigger,
        types::{FocusTarget, FrameAreas, FrameMap, LogEntry, ViewportSize},
    },
    capture::{
        JournalReader, JournalText, RESTORE_RAW_TAIL, Recorder, ScreenshotFormat, Session,
        SessionDevice, SessionLine, Stream, create_parent_dirs, expand_home, find_last_session,
        read_tail_bytes, read_tail_lines, restore_separator,
    },
    commands::{CommandError, Invocation, Target},
    configs::{
        DualBufferMode, KeyBindingAction, KeyResolver, Mode, SettingsConfig, StatusBarLayout,
        TextEncoding, WrapMode,
        settings::{PrefixWrapBehavior, RestoreMode},
    },
    inputs::{
        Action, AppAction, ClickHoldAction, CopyModeAction, HistoryAction, LineInputAction,
        ScrollbarAction, SearchModeAction, SideViewAction, TerminalAction,
    },
    link::{
        Address, LinkConfig, LinkKind, LinkState,
        replay::Replay,
        serial::{LineControl, LineSettings},
    },
    notification::{NotificationManager, ToastLevel},
    overlays::{
        ErrorDialog, FilePathDialog, Overlay, OverlayResponse, PopupTarget, ScreenshotDialog,
        SessionBrowser, SettingKey,
    },
};

pub mod data_processing;
pub mod device;
pub mod line_mode;
pub mod mash;
pub mod raw_scroller;
pub mod scroller;
pub mod search;
pub mod selection;
pub mod term_answers;
pub mod terminal_mode;
pub mod transfer;
pub mod triggers;
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

#[derive(Default)]
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

/// The most kept while the restore dialog asks: past it, the dialog closes without a restore
const HELD_RX_MAX: usize = 4 << 20;
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
    /// The link: its config, the device while open, the open running (MAX-168)
    link: LinkState,
    /// The missing device's popup waits for the restore question's answer (MAX-148)
    disconnected_after_restore: bool,
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
    /// The commands of a command line chain still to run, one after the other (MAX-12), and
    /// when they go on after a `sleep` or a `break`
    pending_commands: VecDeque<Invocation>,
    commands_resume_at: Option<Instant>,
    /// The command lines that ran, the oldest first (MAX-12)
    command_history: VecDeque<String>,
    pub line_mode: LineMode,
    pub terminal_mode: TerminalMode,
    recorder: Recorder,
    /// What is received while the restore dialog asks, fed after the answer (MAX-148)
    held_rx: Option<Vec<u8>>,
    /// The replies to the remote's queries, for the loop to send (MAX-151)
    query_answers: Vec<u8>,
    /// The rules of `triggers.toml` (MAX-6)
    triggers: Vec<Trigger>,
    /// The text `mash` sends until another comes in (MAX-6)
    mash: Option<Mash>,
    /// Where the session backups are, for the session browser (MAX-148)
    sessions_dir: Option<PathBuf>,
    pub is_window_focused: bool,
    pointer: Option<(u16, u16)>,
    /// The last text copied, which a right-click pastes (MAX-144)
    paste_buffer: Option<String>,
    /// The last frame was smaller than `compact_cols` x `compact_rows` (MAX-136)
    compact: bool,
    pub stats: Stats,
    /// The file `send-file` sends, a chunk per loop turn (MAX-12)
    transfer: Option<FileTransfer>,
    /// What a replay started while running set aside, to go back to (MAX-166)
    replaced_link: Option<Box<ReplacedLink>>,
    replay: ReplayState,
}

/// The live link a replay replaced (MAX-166): its settings, the mode, the encoding, the
/// histories of both modes and the counts, put back as they were when the replay stops
struct ReplacedLink {
    link_config: LinkConfig,
    mode: Mode,
    encoding: TextEncoding,
    line_mode: LineMode,
    terminal_mode: TerminalMode,
    stats: Stats,
}

/// The replay running (MAX-166), new for each one: the line settings of the session it plays,
/// when it plays one recorded on a serial port, and whether it read its whole file or was
/// stopped without a link to go back to. Meaningless while the link isn't a replay
#[derive(Debug, Default)]
struct ReplayState {
    line: Option<SessionLine>,
    ended: bool,
}

// Effects outside which can only be handled by main
pub enum UpdateEffect {
    None,
    Transmit(Vec<u8>),
    Reconfigure(LineSettings),
    LineControl(LineControl),
    Open(Address),
    /// `session` is what a session backup was recorded on, from its meta file: the replay
    /// decodes with its encoding and shows its line settings (MAX-166)
    Replay {
        path: PathBuf,
        delay: Duration,
        session: Option<SessionDevice>,
    },
    StopReplay,
    CopyToClipboard(String),
    CopyToClipboardAs {
        text: String,
        what: String,
    },
    ToggleAttach,
    ReloadConfig,
    Quit,
}

impl App {
    pub fn new(
        settings: SettingsConfig,
        key_resolver: KeyResolver,
        link_config: LinkConfig,
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

        let mut app = Self {
            prefix_active: false,
            settings,
            key_resolver,
            link: LinkState::new(link_config),
            disconnected_after_restore: false,
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
            pending_commands: VecDeque::new(),
            commands_resume_at: None,
            command_history: VecDeque::new(),
            line_mode: line_view,
            terminal_mode: terminal_view,
            recorder: Recorder::default(),
            held_rx: None,
            query_answers: Vec::new(),
            triggers: Vec::new(),
            mash: None,
            sessions_dir: None,
            is_window_focused: true,
            pointer: None,
            paste_buffer: None,
            compact: false,
            stats: Stats {
                received_bytes: 0,
                sent_bytes: 0,
            },
            transfer: None,
            replaced_link: None,
            replay: ReplayState::default(),
        };
        app.warn_shadowed_keys();
        app
    }

    /// The file `send-file` sends (MAX-12)
    pub fn transfer(&self) -> Option<&FileTransfer> {
        self.transfer.as_ref()
    }

    /// The next chunk of the file being sent, for the loop to write; none while it isn't time.
    /// The end of the file, or a read error, ends the transfer with a notice
    pub fn next_transfer_chunk(&mut self, now: Instant) -> Option<Vec<u8>> {
        let transfer = self.transfer.as_mut()?;
        match transfer.next_chunk(now) {
            Ok(Some(chunk)) if chunk.is_empty() => None,
            Ok(Some(chunk)) => Some(chunk),
            Ok(None) => {
                let transfer = self.transfer.take()?;
                self.notify_success(format!(
                    "Sent {} ({} bytes)",
                    transfer.name(),
                    transfer.size()
                ));
                None
            }
            Err(e) => {
                self.stop_transfer(&format!("read error: {e}"));
                None
            }
        }
    }

    /// Ends the file being sent, the link can't take it (`why`)
    pub fn stop_transfer(&mut self, why: &str) {
        if let Some(transfer) = self.transfer.take() {
            self.notify_error(format!(
                "Stopped sending {} at {}%: {why}",
                transfer.name(),
                transfer.percent()
            ));
        }
    }

    pub fn command_history(&self) -> &VecDeque<String> {
        &self.command_history
    }

    /// A command line that ran, trimmed, unless it is the last one; the oldest dropped past
    /// `command_line.history_size`, read now so a reload applies at once
    fn push_command_history(&mut self, line: String) {
        let line = line.trim();
        if line.is_empty() || self.command_history.back().is_some_and(|last| last == line) {
            return;
        }
        self.command_history.push_back(line.to_string());
        let size = self.settings.command_line.history_size;
        while self.command_history.len() > size {
            self.command_history.pop_front();
        }
    }

    /// The next action of a command line chain (MAX-12). Each is built when its turn comes, on
    /// the state the previous ones left, and the loop applies its effect before the next
    /// A command that refuses (the link can't do it) stops the chain, with the error. None
    /// either while the chain is paused (before `now`)
    pub fn next_command_action(&mut self, now: Instant) -> Option<Action> {
        if let Some(resume_at) = self.commands_resume_at {
            if now < resume_at {
                return None;
            }
            self.commands_resume_at = None;
        }
        let invocation = self.pending_commands.pop_front()?;
        let built = match invocation.spec.target {
            Target::Action(action) => Ok(action.to_action(self)),
            Target::Build(build) => build(self, &invocation.args),
        };
        built
            .map_err(|reason| {
                let error = CommandError::Failed {
                    command: invocation.spec.name,
                    reason,
                };
                self.fail_commands(error.to_string());
            })
            .ok()
    }

    /// A command failed with `message`, here or where the loop applied its effect: the rest of
    /// its chain is dropped, the error says how much
    pub fn fail_commands(&mut self, message: String) {
        match self.drop_pending_commands() {
            0 => self.notify_error(message),
            n => self.notify_error(format!("{message} ({n} more not run)")),
        }
    }

    /// Drops the rest of a command line chain, quietly: how many commands it had
    pub fn drop_pending_commands(&mut self) -> usize {
        self.commands_resume_at = None;
        std::mem::take(&mut self.pending_commands).len()
    }

    /// A command line to run, typed or bound to a key (MAX-12): after the commands waiting, but
    /// a line starting with `cancel` stops them instead of waiting behind them
    fn queue_commands(&mut self, mut invocations: Vec<Invocation>) {
        let cancels = invocations.first().is_some_and(|invocation| {
            matches!(
                invocation.spec.target,
                Target::Action(KeyBindingAction::CancelCommands)
            )
        });
        if cancels {
            invocations.remove(0);
            self.cancel();
        }
        self.pending_commands.extend(invocations);
    }

    /// `cancel` (MAX-12): stops the file being sent and drops the commands waiting
    fn cancel(&mut self) {
        let mut stopped = Vec::new();
        if self.mash.take().is_some() {
            stopped.push("Stopped mashing".to_string());
        }
        if let Some(transfer) = self.transfer.take() {
            stopped.push(format!(
                "Stopped sending {} at {}%",
                transfer.name(),
                transfer.percent()
            ));
        }
        match self.drop_pending_commands() {
            0 => {}
            1 => stopped.push("dropped 1 waiting command".to_string()),
            n => stopped.push(format!("dropped {n} waiting commands")),
        }
        if stopped.is_empty() {
            self.notify_info("Nothing to cancel");
        } else {
            let mut message = stopped.join(", ");
            message[..1].make_ascii_uppercase();
            self.notify_warn(message);
        }
    }

    /// The rest of a command line chain waits until `until` (a `sleep`, the end of a `break`,
    /// an `open` at the latest)
    pub fn pause_commands(&mut self, until: Instant) {
        self.commands_resume_at = Some(until);
    }

    /// The rest of a command line chain goes on now: the link `open` waited for is open
    pub fn resume_commands(&mut self) {
        self.commands_resume_at = None;
    }

    /// Whether the link is open: the commands that need it refuse otherwise (MAX-12)
    pub fn is_link_open(&self) -> bool {
        self.link.is_open()
    }

    /// The link's address and line settings (MAX-168)
    pub fn link_config(&self) -> &LinkConfig {
        self.link.config()
    }

    /// The link itself, for the other modules' tests to set its address (MAX-168)
    #[cfg(test)]
    pub fn link_mut(&mut self) -> &mut LinkState {
        &mut self.link
    }

    /// A new session backup, for the port `open` switched to: the backups go by port (MAX-12)
    pub fn restart_journal(&mut self) {
        self.start_journal(self.sessions_dir.clone());
    }

    /// What the current link can do, taken from its address (MAX-166)
    pub fn link_kind(&self) -> LinkKind {
        self.link.kind()
    }

    /// The line settings the status bar and the settings dialog show (MAX-166): a serial port's,
    /// a replayed serial session's from its meta; none for a TCP link nor a plain file
    pub fn line_settings(&self) -> Option<SessionLine> {
        match self.link_kind() {
            LinkKind::Serial => Some(SessionLine::of(&self.link.config().line)),
            LinkKind::Tcp => None,
            LinkKind::Replay => self.recorded_line(),
        }
    }

    /// The line settings of the session the replay plays, from its meta (MAX-166): shown, never
    /// set. None for a plain file, a session recorded over TCP, or a link that isn't a replay
    pub fn recorded_line(&self) -> Option<SessionLine> {
        (self.link_kind() == LinkKind::Replay)
            .then_some(self.replay.line)
            .flatten()
    }

    /// The replay of the file at `path` takes the link's place, on a reset terminal in terminal
    /// mode (MAX-166). A live link is set aside with its encoding, histories and counts, its
    /// journal paused; what it was sending stops. A replay replacing a replay keeps the live
    /// link. A session backup (`session`) decodes with its own encoding, a plain file with the
    /// live link's
    pub fn start_replay(&mut self, path: PathBuf, session: Option<&SessionDevice>) {
        self.stop_transfer("a replay took the link's place");
        self.stop_mash("a replay took the link's place");
        let (line_mode, terminal_mode, stats) = self.clear_screen_for_replay();
        let live_encoding = match &self.replaced_link {
            Some(replaced) => replaced.encoding,
            None => self.settings.encoding,
        };
        if self.link_kind().keeps_sessions() {
            self.recorder.pause_journal();
            self.replaced_link = Some(Box::new(ReplacedLink {
                link_config: self.link.config().clone(),
                mode: self.settings.mode,
                encoding: self.settings.encoding,
                line_mode,
                terminal_mode,
                stats,
            }));
        }
        self.link.set_address(Address::Replay(path));
        self.settings.mode = Mode::Terminal;
        self.settings.encoding = session.map_or(live_encoding, |device| device.encoding);
        self.replay = ReplayState {
            line: session.and_then(|device| device.line),
            ended: false,
        };
    }

    /// Fresh histories of both modes and fresh counts for a replay to play on, the ones they
    /// replace given back (MAX-166)
    fn clear_screen_for_replay(&mut self) -> (LineMode, TerminalMode, Stats) {
        let line_mode = LineMode::new(
            self.settings.line_mode.history_size,
            self.settings.line_mode.send_history_size,
        );
        let terminal_mode = TerminalMode::new(
            self.settings.terminal_mode.history_size,
            self.settings.terminal_mode.rows,
            self.settings.terminal_mode.cols,
        );
        (
            std::mem::replace(&mut self.line_mode, line_mode),
            std::mem::replace(&mut self.terminal_mode, terminal_mode),
            std::mem::take(&mut self.stats),
        )
    }

    /// The replay reads again from the start, on a clean screen, not ended (MAX-166)
    fn restart_replay_screen(&mut self) {
        self.clear_screen_for_replay();
        self.replay.ended = false;
    }

    /// The replay reading, for its progress and its controls (MAX-166)
    pub fn replay(&self) -> Option<&Replay> {
        self.link.replay()
    }

    /// The replay read its whole file: said once, its screen left as it is (MAX-166)
    pub fn end_replay(&mut self) {
        self.replay.ended = true;
        self.notify_info("Replay finished");
    }

    pub fn is_replay_ended(&self) -> bool {
        self.replay.ended
    }

    /// Puts back the live link the replay replaced, for the loop to open, and resumes its
    /// journal (MAX-166). False when there is none: a replay from the command line
    pub fn stop_replay(&mut self) -> bool {
        let Some(replaced) = self.replaced_link.take() else {
            self.replay.ended = true;
            return false;
        };
        self.link.set_config(replaced.link_config);
        self.settings.mode = replaced.mode;
        self.settings.encoding = replaced.encoding;
        self.line_mode = replaced.line_mode;
        self.terminal_mode = replaced.terminal_mode;
        self.stats = replaced.stats;
        self.recorder.resume_journal();
        true
    }

    /// A warning for the keymap's keys that never reach their mode section (MAX-155)
    fn warn_shadowed_keys(&mut self) {
        let shadowed = self.key_resolver.shadowed_keys();
        if !shadowed.is_empty() {
            self.notify_warn(format!("Keymap: unreachable {}", shadowed.join(", ")));
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

        // the user takes over: a mash stops at the first key typed for the device (MAX-6)
        let typed = matches!(
            action,
            Action::Terminal(TerminalAction::Key(_) | TerminalAction::Paste(_))
                | Action::LineInput(
                    LineInputAction::HandleKey(_)
                        | LineInputAction::Paste(_)
                        | LineInputAction::Submit
                        | LineInputAction::Interrupt
                )
        );
        if typed {
            self.stop_mash("a key was typed");
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
                        // the config changes once the port takes it (MAX-168)
                        OverlayResponse::Reconfigure(line) => UpdateEffect::Reconfigure(line),
                        OverlayResponse::Reattach => UpdateEffect::ToggleAttach,
                        OverlayResponse::Run(action) => {
                            let action = action.to_action(self);
                            self.update(action)
                        }
                        OverlayResponse::QueueCommands { line, invocations } => {
                            self.push_command_history(line);
                            self.queue_commands(invocations);
                            UpdateEffect::None
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
                        OverlayResponse::HandledNotify { message, warn } => {
                            self.overlay = Some(overlay);
                            if warn {
                                self.notify_warn(message);
                            } else {
                                self.notify_info(message);
                            }
                            UpdateEffect::None
                        }
                        OverlayResponse::ToggleTrigger(name) => {
                            self.overlay = Some(overlay);
                            self.toggle_triggers(name.as_deref());
                            UpdateEffect::None
                        }
                        OverlayResponse::HandledCopy { text, what } => {
                            self.overlay = Some(overlay);
                            self.paste_buffer = Some(text.clone());
                            UpdateEffect::CopyToClipboardAs { text, what }
                        }
                        // the browser closes on the replay, which takes the screen
                        OverlayResponse::ReplaySession { path, device } => UpdateEffect::Replay {
                            path,
                            delay: Duration::ZERO,
                            session: device,
                        },
                        OverlayResponse::StopReplay => UpdateEffect::StopReplay,
                        OverlayResponse::RestoreSession(yes) => {
                            if let Overlay::RestoreSession(session) = overlay {
                                self.answer_restore(&session, yes);
                            }
                            UpdateEffect::None
                        }
                        OverlayResponse::SaveScreenshot { path, contents } => {
                            self.save_screenshot(&path, contents);
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
        if self.hold_rx(chunk) {
            return;
        }
        self.feed(chunk, true);
        self.fire_triggers(chunk);
        self.watch_mash(chunk);
    }

    pub fn mash(&self) -> Option<&Mash> {
        self.mash.as_ref()
    }

    /// What the mash sends now, for the loop to write; none while it isn't time. Its timeout
    /// stops it, and the rest of its line
    pub fn next_mash_bytes(&mut self, now: Instant) -> Option<Vec<u8>> {
        match self.mash.as_mut()?.step(now) {
            MashStep::Wait => None,
            MashStep::Send(bytes) => Some(bytes.to_vec()),
            MashStep::TimedOut => {
                let until = self.mash.take()?.until_text().to_string();
                self.fail_commands(format!("mash: '{until}' didn't come in"));
                None
            }
        }
    }

    /// Ends the mash before its text came in (`why` shown as reason), and the rest of its line
    pub fn stop_mash(&mut self, why: &str) {
        if self.mash.take().is_some() {
            self.fail_commands(format!("Mash stopped: {why}"));
        }
    }

    /// The text the mash waits for came in: it ends, the rest of its line goes on
    fn watch_mash(&mut self, chunk: &[u8]) {
        if self
            .mash
            .as_mut()
            .is_some_and(|mash| mash.until_seen(chunk))
        {
            let until = self.mash.take().map(|mash| mash.until_text().to_string());
            self.notify_success(format!(
                "Mash done: '{}' came in",
                until.unwrap_or_default()
            ));
            self.resume_commands();
        }
    }

    /// The rules of `triggers.toml`, at startup and on a reload (MAX-6)
    pub fn set_triggers(&mut self, triggers: Vec<Trigger>) {
        self.triggers = triggers;
    }

    pub fn triggers(&self) -> &[Trigger] {
        &self.triggers
    }

    /// Toggles the trigger named, or all of them: armed when none is, else disarmed (MAX-6)
    fn toggle_triggers(&mut self, name: Option<&str>) {
        let named = (self.triggers.iter()).filter(|t| name.is_none_or(|name| name == t.name));
        let on = !named.into_iter().any(Trigger::is_armed);
        self.arm_triggers(on, name);
    }

    /// Arms or disarms the triggers, or the one named, with a notice of what is armed after
    /// (MAX-6)
    fn arm_triggers(&mut self, on: bool, name: Option<&str>) {
        if self.triggers.is_empty() {
            self.notify_warn("triggers.toml has no trigger");
            return;
        }
        for trigger in &mut self.triggers {
            if name.is_none_or(|name| name == trigger.name) {
                trigger.arm(on);
            }
        }
        let armed = self.armed_triggers().join(", ");
        if armed.is_empty() {
            self.notify_info("Triggers disarmed");
        } else {
            self.notify_info(format!("Triggers armed: {armed}"));
        }
    }

    /// The names of the armed triggers
    pub fn armed_triggers(&self) -> Vec<&str> {
        (self.triggers.iter())
            .filter(|trigger| trigger.is_armed())
            .map(|trigger| trigger.name.as_str())
            .collect()
    }

    /// The armed triggers whose text ends in these live bytes run their command line, queued
    /// as a typed one, with a notice (MAX-6). A replay's bytes never fire one, nor what a
    /// restore or the restore question held back (fed apart)
    fn fire_triggers(&mut self, chunk: &[u8]) {
        if !self.link_kind().writes() {
            return;
        }
        for i in 0..self.triggers.len() {
            if self.triggers[i].fires(chunk) {
                let trigger = &self.triggers[i];
                let notice = format!("Trigger {}: {}", trigger.name, trigger.run);
                let invocations = trigger.invocations.clone();
                self.notify_info(notice);
                self.queue_commands(invocations);
            }
        }
    }

    /// Feeds both modes. The remote's queries are answered when `live` (not held while the
    /// restore dialog asked) and in terminal mode only (MAX-151)
    fn feed(&mut self, chunk: &[u8], live: bool) {
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
            let answers = self.terminal_mode.take_answers();
            if live && self.settings.mode == Mode::Terminal {
                let bytes = answer_bytes(answers, &self.settings.terminal_mode);
                self.query_answers.extend(bytes);
            }
        }
    }

    /// The replies to send to the remote's queries, received since the last call (MAX-151)
    pub fn take_query_answers(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.query_answers)
    }

    /// Takes the terminal screen now, in both formats, and opens the dialog asking where to
    /// save it and which format, `format` chosen first, so a fast screen is caught. Line mode
    /// doesn't show that screen, so it is not taken there (MAX-147)
    fn save_screenshot(&mut self, path: &Path, contents: String) {
        let path = expand_home(path);
        let saved = create_parent_dirs(&path).and_then(|()| std::fs::write(&path, contents));
        match saved {
            Ok(()) => self.notify_success(format!("Screenshot saved to {}", path.display())),
            Err(e) => self.notify_error(format!("Failed to save screenshot: {e}")),
        }
    }

    /// `screenshot <name>` (MAX-12): saved at once, its format from its extension (`.ans` the
    /// colors, else text, `.txt` added to a name without one), a bare name in `screenshot_dir`
    fn save_screenshot_as(&mut self, name: &Path) {
        let format = match name.extension() {
            Some(ext) if ext == "ans" => ScreenshotFormat::Ansi,
            _ => ScreenshotFormat::Text,
        };
        let mut path = self.settings.capture.screenshot_dir.join(name);
        if path.extension().is_none() {
            path.set_extension(format.extension());
        }
        let contents = self.terminal_mode.screenshot().into_contents(format);
        self.save_screenshot(&path, contents);
    }

    /// Whether an on/off setting is on, for `set` (MAX-12); none for the ones with more values
    pub fn setting_is_on(&self, key: SettingKey) -> Option<bool> {
        match key {
            SettingKey::LocalEcho => Some(self.settings.local_echo),
            SettingKey::DisplayTimestamp => Some(self.settings.show_timestamps),
            SettingKey::Mouse => Some(self.settings.ui.mouse),
            SettingKey::AnswerQueries => Some(self.settings.terminal_mode.answer_queries),
            _ => None,
        }
    }

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

    /// Restores the last session on this device found in `dir` when `restore` (`--restore` /
    /// `--no-restore`) or else `capture.restore` says so, with a toast either way. With `ask`,
    /// a dialog asks first, and what is received meanwhile waits for the answer. To call
    /// before `start_journal`, whose pruning could remove it (MAX-148)
    pub fn restore_last_session(&mut self, dir: Option<&Path>, restore: Option<bool>) {
        let capture = &self.settings.capture;
        let mode = match restore {
            Some(true) => RestoreMode::Always,
            Some(false) => RestoreMode::Never,
            None => capture.restore,
        };
        let Some(dir) = dir.filter(|_| mode != RestoreMode::Never) else {
            return;
        };
        let max_age = (!capture.restore_max_age.is_zero()).then_some(capture.restore_max_age);
        let Some(session) = find_last_session(dir, &self.session_device(), max_age) else {
            return;
        };
        if mode == RestoreMode::Ask {
            self.held_rx = Some(Vec::new());
            self.open_overlay(Overlay::RestoreSession(Box::new(session)));
        } else {
            self.restore_now(&session);
        }
    }

    /// Keeps `chunk` while the restore dialog is open, so the restored data comes before it.
    /// When the dialog was replaced by another popup, or too much came in, what was kept is
    /// fed without a restore, then `chunk` is not kept
    fn hold_rx(&mut self, chunk: &[u8]) -> bool {
        let Some(held) = &mut self.held_rx else {
            return false;
        };
        let asking = matches!(self.overlay, Some(Overlay::RestoreSession(_)));
        if asking && held.len() + chunk.len() <= HELD_RX_MAX {
            held.extend_from_slice(chunk);
            return true;
        }
        if asking {
            self.close_overlay();
            self.notify_warn("Session not restored: too much data came in while asking");
        }
        self.release_held_rx();
        false
    }

    fn release_held_rx(&mut self) {
        if let Some(held) = self.held_rx.take() {
            self.feed(&held, false);
        }
    }

    /// The answer of the restore dialog: the session first when `yes`, then what came in
    fn answer_restore(&mut self, session: &Session, yes: bool) {
        if yes {
            self.restore_now(session);
        }
        self.release_held_rx();
    }

    fn restore_now(&mut self, session: &Session) {
        match self.restore_session(session) {
            Ok(()) => self.notify_info(format!("Restored the session of {}", session.time_span())),
            Err(err) => self.notify_warn(format!("Session not restored: {err}")),
        }
    }

    /// The session browser on the session folder, the session this device would restore marked
    /// (MAX-148)
    fn open_session_browser(&mut self) {
        let Some(dir) = self.sessions_dir.clone() else {
            self.notify_warn("No session folder");
            return;
        };
        let capture = &self.settings.capture;
        let max_age = (!capture.restore_max_age.is_zero()).then_some(capture.restore_max_age);
        let restorable =
            find_last_session(&dir, &self.session_device(), max_age).map(|session| session.stem);
        let mut browser = SessionBrowser::new(dir, restorable);
        if self.replaced_link.is_some() {
            let raw = self.recorder.paused_journal_raw().map(Path::to_path_buf);
            browser = browser.with_live(raw);
        }
        self.open_overlay(Overlay::Sessions(Box::new(browser)));
    }

    /// What this session runs on, for its journal and to find a past one to restore
    fn session_device(&self) -> SessionDevice {
        SessionDevice::new(self.link.config(), self.settings.encoding)
    }

    /// Puts a past session in the history of both modes, before any live data (MAX-148): the
    /// last lines of its text in line mode, read back with their times and marks, the end of
    /// its raw bytes through the terminal, then a separator where the live data starts. Not
    /// recorded nor counted: the new session starts empty
    pub fn restore_session(&mut self, session: &Session) -> std::io::Result<()> {
        // a session from `find_last_session` has all three
        let (Some(text_path), Some(raw_path), Some(meta), Some(started)) = (
            session.text_path(),
            session.raw_path(),
            &session.meta,
            session.started,
        ) else {
            return Err(std::io::Error::other("not a complete session"));
        };
        let text = read_tail_lines(text_path, self.settings.line_mode.history_size)?;
        let raw = read_tail_bytes(raw_path, RESTORE_RAW_TAIL)?;
        let separator = restore_separator(session);

        let text = String::from_utf8_lossy(&text);
        let mut reader = JournalReader::new(&meta.text, started);
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
            AppAction::OpenSessions => {
                self.open_session_browser();
                UpdateEffect::None
            }
            AppAction::StartMash(mash) => {
                self.notify_info(format!("Mashing until '{}'", mash.until_text()));
                // the rest of the line waits for its text, its timeout at most
                self.pause_commands(mash.deadline());
                self.mash = Some(*mash);
                UpdateEffect::None
            }
            AppAction::StartTransfer(transfer) => {
                self.notify_info(format!(
                    "Sending {} ({} bytes)",
                    transfer.name(),
                    transfer.size()
                ));
                self.transfer = Some(*transfer);
                UpdateEffect::None
            }
            AppAction::Reconfigure(line) => UpdateEffect::Reconfigure(line),
            AppAction::PauseCommands(duration) => {
                self.pause_commands(Instant::now() + duration);
                UpdateEffect::None
            }
            AppAction::LineControl(control) => UpdateEffect::LineControl(control),
            AppAction::Open(port) => UpdateEffect::Open(port),
            AppAction::Replay { path, delay } => UpdateEffect::Replay {
                path,
                delay,
                session: None,
            },
            AppAction::StopReplay => UpdateEffect::StopReplay,
            AppAction::ControlReplay(control) => {
                self.control_replay(control);
                UpdateEffect::None
            }
            AppAction::SetEncoding(encoding) => {
                self.settings.encoding = encoding;
                self.notify_info(format!("Rx Text Encoding is {}", encoding.as_str()));
                UpdateEffect::None
            }
            AppAction::SaveScreenshot(name) => {
                self.save_screenshot_as(&name);
                UpdateEffect::None
            }
            AppAction::QueueCommands(invocations) => {
                self.queue_commands(invocations);
                UpdateEffect::None
            }
            AppAction::CancelCommands => {
                self.cancel();
                UpdateEffect::None
            }
            AppAction::ToggleTriggers => {
                self.toggle_triggers(None);
                UpdateEffect::None
            }
            AppAction::ArmTriggers { on, name } => {
                self.arm_triggers(on, name.as_deref());
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
            // the live link a replay replaced comes back with the attach key too (MAX-166)
            AppAction::DetachSerial if self.replaced_link.is_some() => UpdateEffect::StopReplay,
            AppAction::DetachSerial => UpdateEffect::ToggleAttach,
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
        self.warn_shadowed_keys();
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
            SettingKey::AnswerQueries => {
                let terminal = &mut self.settings.terminal_mode;
                terminal.answer_queries = !terminal.answer_queries;
                let state = if terminal.answer_queries { "ON" } else { "OFF" };
                self.notify_info(format!("Query answers {state}"));
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
        // the browser lists the past sessions even with the backup off
        self.sessions_dir.clone_from(&dir);
        let capture = &self.settings.capture;
        if !capture.journal {
            return;
        }
        let device = self.session_device();
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

    /// An app whose link is open
    fn app() -> App {
        let mut app = App::new(
            SettingsConfig::default(),
            KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap(),
            LinkConfig::default(),
        );
        set_link_open(&mut app, true);
        app
    }

    /// The line settings the port took, as the link sets them
    fn set_line(app: &mut App, line: LineSettings) {
        let config = LinkConfig {
            line,
            ..app.link_config().clone()
        };
        app.link.set_config(config);
    }

    /// Opens or closes the app's link: a device that reads and writes nothing stands for an
    /// open link
    fn set_link_open(app: &mut App, open: bool) {
        app.link.start(open.then(|| {
            let path =
                std::env::temp_dir().join(format!("maxicom_test_{}_open_link", std::process::id()));
            std::fs::write(&path, b"").unwrap();
            let replay = crate::link::replay::Replay::open(&path, Duration::ZERO).unwrap();
            crate::link::ByteSource::Replay(replay)
        }));
    }

    /// MAX-12: the command line runs its chain one action at a time, in order, each effect
    /// returned for the loop to apply, and closes; a line that doesn't parse runs nothing,
    /// warns and stays open.
    /// How: a chain toggling a setting then giving two effects, then a chain with a typo.
    #[test]
    fn command_line_runs_its_chain_in_order() {
        let mut app = app();
        let run_line = |app: &mut App, line: &str| {
            app.update(KeyBindingAction::CommandLine.to_action(app));
            app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
            app.update(Action::Overlay(DialogAction::Confirm))
        };
        let echo = app.settings.local_echo;
        let effect = run_line(&mut app, "toggle-local-echo; reload-config; detach-serial");
        assert!(matches!(effect, UpdateEffect::None));
        assert!(app.overlay().is_none());
        let mut effects = Vec::new();
        while let Some(action) = app.next_command_action(Instant::now()) {
            effects.push(app.update(action));
        }
        assert!(matches!(
            effects[..],
            [
                UpdateEffect::None,
                UpdateEffect::ReloadConfig,
                UpdateEffect::ToggleAttach
            ]
        ));
        assert_ne!(app.settings.local_echo, echo);

        run_line(&mut app, "toggle-local-echo; nope");
        assert!(matches!(app.overlay(), Some(Overlay::CommandLine(_))));
        assert!(app.next_command_action(Instant::now()).is_none());
        assert_ne!(app.settings.local_echo, echo);
        let toast = app.notifications.active_toasts().last().unwrap();
        assert_eq!(toast.message, "unknown command 'nope'");
    }

    /// MAX-12: `send` and `send-hex` transmit their bytes, in the chain's order. A command that
    /// refuses (nothing to send, a replay) stops the chain with the error.
    /// How: chains run on a live port, then on a replay (`file://`).
    #[test]
    fn command_line_send() {
        let mut app = app();
        let run_line = |app: &mut App, line: &str| {
            app.update(KeyBindingAction::CommandLine.to_action(app));
            app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
            app.update(Action::Overlay(DialogAction::Confirm));
            let mut effects = Vec::new();
            while let Some(action) = app.next_command_action(Instant::now()) {
                effects.push(app.update(action));
            }
            effects
        };
        let last_error = |app: &App| {
            app.notifications
                .active_toasts()
                .last()
                .unwrap()
                .message
                .clone()
        };

        let effects = run_line(&mut app, "send-hex 0d0a; send AT\\r");
        assert!(matches!(
            &effects[..],
            [UpdateEffect::Transmit(crlf), UpdateEffect::Transmit(at)]
                if crlf == b"\r\n" && at == b"AT\r"
        ));

        let effects = run_line(&mut app, "send ''; help; settings");
        assert!(effects.is_empty());
        assert_eq!(last_error(&app), "send: nothing to send (2 more not run)");
        assert!(app.overlay().is_none());

        app.link
            .set_address(Address::parse("file:///tmp/capture.raw"));
        let effects = run_line(&mut app, "help; send x");
        assert_eq!(effects.len(), 1);
        assert!(matches!(app.overlay(), Some(Overlay::Help(_))));
        assert_eq!(last_error(&app), "send: a replay has no link to send to");
        assert!(app.next_command_action(Instant::now()).is_none());
        assert!(run_line(&mut app, "send-hex 0d0a").is_empty());
        assert_eq!(
            last_error(&app),
            "send-hex: a replay has no link to send to"
        );
    }

    /// MAX-6: an armed trigger whose text comes in runs its command line as a typed one, with
    /// a notice, once (one shot); a disarmed one doesn't. A replay's bytes and what the restore
    /// question held back never fire one.
    /// How: an armed `u-boot` rule and a disarmed `login` one, the U-Boot line fed live, again,
    /// then as a replay and as held bytes.
    #[test]
    fn triggers_fire_on_live_bytes() {
        let config: crate::configs::TriggersConfig = toml::from_str(
            r#"
            [u-boot]
            match = "Hit any key to stop autoboot"
            run = "send ' '"
            armed = true
            [login]
            match = "login: "
            run = "send root\r"
            "#,
        )
        .unwrap();
        let mut app = app();
        app.set_triggers(config.compile().unwrap());
        let effects = |app: &mut App| {
            let mut effects = Vec::new();
            while let Some(action) = app.next_command_action(Instant::now()) {
                effects.push(app.update(action));
            }
            effects
        };
        let uboot = b"U-Boot 2024.01\r\nHit any key to stop autoboot:  3";

        app.feed_rx(b"board login: ");
        assert!(effects(&mut app).is_empty());

        app.feed_rx(uboot);
        assert!(matches!(&effects(&mut app)[..], [UpdateEffect::Transmit(b)] if b == b" "));
        let toast = &app.notifications.active_toasts().last().unwrap().message;
        assert_eq!(toast, "Trigger u-boot: send ' '");
        assert!(app.command_history.is_empty());

        // one shot: disarmed now
        app.feed_rx(uboot);
        assert!(effects(&mut app).is_empty());

        let reload = |app: &mut App| {
            app.set_triggers(config.compile().unwrap());
        };
        reload(&mut app);
        app.held_rx = Some(uboot.to_vec());
        app.release_held_rx();
        assert!(effects(&mut app).is_empty());

        reload(&mut app);
        app.link
            .set_address(Address::parse("file:///tmp/capture.raw"));
        app.feed_rx(uboot);
        assert!(effects(&mut app).is_empty());
    }

    /// MAX-6: the toggle arms every trigger when none is armed, else disarms them all; the
    /// `trigger` command arms, disarms or toggles all or one by name, refuses a name that
    /// isn't one, and each change says what is armed. With no trigger at all, both say so.
    /// How: the default triggers (`login`, `u-boot`), then none.
    #[test]
    fn arming_triggers() {
        let mut app = app();
        app.set_triggers(crate::configs::TriggersConfig::default().compile().unwrap());
        let last_toast = |app: &App| {
            let toasts = app.notifications.active_toasts();
            toasts.last().unwrap().message.clone()
        };
        let run = |app: &mut App, line: &str| {
            app.update(Action::App(AppAction::QueueCommands(
                crate::commands::parse(line).unwrap(),
            )));
            while let Some(action) = app.next_command_action(Instant::now()) {
                app.update(action);
            }
        };

        app.update(KeyBindingAction::ToggleTriggers.to_action(&app));
        assert_eq!(app.armed_triggers(), ["login", "u-boot"]);
        assert_eq!(last_toast(&app), "Triggers armed: login, u-boot");
        run(&mut app, "trigger off login");
        assert_eq!(app.armed_triggers(), ["u-boot"]);
        // one armed: the toggle disarms
        app.update(KeyBindingAction::ToggleTriggers.to_action(&app));
        assert!(app.armed_triggers().is_empty());
        assert_eq!(last_toast(&app), "Triggers disarmed");
        run(&mut app, "trigger on u-boot");
        assert_eq!(app.armed_triggers(), ["u-boot"]);
        run(&mut app, "trigger on");
        assert_eq!(app.armed_triggers(), ["login", "u-boot"]);
        // toggled by name, or all: none armed arms them all
        run(&mut app, "trigger toggle u-boot");
        assert_eq!(app.armed_triggers(), ["login"]);
        run(&mut app, "trigger toggle u-boot");
        assert_eq!(app.armed_triggers(), ["login", "u-boot"]);
        run(&mut app, "trigger toggle");
        assert!(app.armed_triggers().is_empty());
        run(&mut app, "trigger toggle");
        assert_eq!(app.armed_triggers(), ["login", "u-boot"]);

        run(&mut app, "trigger off uboot; trigger off");
        assert_eq!(
            last_toast(&app),
            "trigger: no trigger named 'uboot' (1 more not run)"
        );
        assert_eq!(app.armed_triggers(), ["login", "u-boot"]);

        // the command line completes their names
        app.update(KeyBindingAction::CommandLine.to_action(&app));
        app.update(Action::Overlay(DialogAction::Paste(
            "trigger on u".to_string(),
        )));
        let Some(Overlay::CommandLine(command_line)) = app.overlay() else {
            panic!("the command line");
        };
        assert_eq!(command_line.completion.candidates[0].value, "u-boot");
        app.update(Action::Overlay(DialogAction::Close));

        app.set_triggers(Vec::new());
        run(&mut app, "trigger on");
        assert_eq!(last_toast(&app), "trigger: triggers.toml has no trigger");
        app.update(KeyBindingAction::ToggleTriggers.to_action(&app));
        assert_eq!(last_toast(&app), "triggers.toml has no trigger");
    }

    /// MAX-6: `mash` sends its text at once then every interval, the rest of its line
    /// waiting; its text coming in ends it and the line goes on. The timeout, a key typed for
    /// the device and `cancel` stop it and the rest of its line. It refuses a second mash, a
    /// replay and a closed link.
    /// How: a space every 50 ms until `=> `, a second at most, the timeout's clock passed by
    /// hand.
    #[test]
    fn mash_until_its_text() {
        let mut app = app();
        let last_toast = |app: &App| {
            let toasts = app.notifications.active_toasts();
            toasts.last().unwrap().message.clone()
        };
        let queue = |app: &mut App, line: &str| {
            let invocations = crate::commands::parse(line).unwrap();
            app.update(Action::App(AppAction::QueueCommands(invocations)));
        };
        // the chain's actions built at `now`, their effects
        let run = |app: &mut App, now: Instant| {
            let mut effects = Vec::new();
            while let Some(action) = app.next_command_action(now) {
                effects.push(app.update(action));
            }
            effects
        };

        queue(&mut app, "mash ' ' '=> ' 50 1000; send x");
        assert!(matches!(
            &run(&mut app, Instant::now())[..],
            [UpdateEffect::None]
        ));
        assert!(app.mash().is_some());
        assert_eq!(last_toast(&app), "Mashing until '=> '");
        assert_eq!(app.next_mash_bytes(Instant::now()), Some(b" ".to_vec()));
        assert_eq!(app.next_mash_bytes(Instant::now()), None);
        // the line waits
        assert!(run(&mut app, Instant::now()).is_empty());
        app.feed_rx(b"Hit any key to stop autoboot:  0 \r\n=");
        assert!(app.mash().is_some());
        app.feed_rx(b"> ");
        assert!(app.mash().is_none());
        assert_eq!(last_toast(&app), "Mash done: '=> ' came in");
        let effects = run(&mut app, Instant::now());
        assert!(matches!(&effects[..], [UpdateEffect::Transmit(x)] if x == b"x"));

        // the timeout, then a key, then `cancel`
        queue(&mut app, "mash ' ' '=> ' 50 1000; send x");
        run(&mut app, Instant::now());
        // past the deadline, set when it was built
        let late = Instant::now() + Duration::from_millis(1000);
        assert_eq!(app.next_mash_bytes(late), None);
        assert!(app.mash().is_none());
        assert_eq!(
            last_toast(&app),
            "mash: '=> ' didn't come in (1 more not run)"
        );
        assert!(run(&mut app, late).is_empty());

        queue(&mut app, "mash ' ' '=> '; send x");
        run(&mut app, Instant::now());
        let key = crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Char('a'));
        app.update(Action::LineInput(LineInputAction::HandleKey(key)));
        assert!(app.mash().is_none());
        assert_eq!(
            last_toast(&app),
            "Mash stopped: a key was typed (1 more not run)"
        );

        queue(&mut app, "mash ' ' '=> '");
        run(&mut app, Instant::now());
        queue(&mut app, "cancel");
        run(&mut app, Instant::now());
        assert!(app.mash().is_none());
        assert_eq!(last_toast(&app), "Stopped mashing");

        // refused
        queue(&mut app, "mash ' ' '=> '; mash x y");
        run(&mut app, Instant::now());
        assert_eq!(last_toast(&app), "Mashing until '=> '");
        app.update(Action::App(AppAction::CancelCommands));
        set_link_open(&mut app, false);
        queue(&mut app, "mash ' ' '=> '");
        run(&mut app, Instant::now());
        assert!(last_toast(&app).starts_with("mash: the port isn't open"));
        set_link_open(&mut app, true);
        app.link
            .set_address(Address::parse("file:///tmp/capture.raw"));
        queue(&mut app, "mash ' ' '=> '");
        run(&mut app, Instant::now());
        assert_eq!(last_toast(&app), "mash: a replay has no link to send to");
    }

    /// A pty pair: maxicom opens the `path` side as its port, the test reads and writes the
    /// `device` side, as `src/bin/fake_device.rs` does through socat
    #[cfg(target_os = "linux")]
    struct Pty {
        device: std::fs::File,
        path: String,
    }

    #[cfg(target_os = "linux")]
    impl Pty {
        fn open() -> Self {
            use std::os::fd::FromRawFd;
            unsafe {
                let fd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK);
                assert!(fd >= 0, "posix_openpt");
                assert_eq!(libc::grantpt(fd), 0);
                assert_eq!(libc::unlockpt(fd), 0);
                let mut name = [0 as libc::c_char; 64];
                assert_eq!(libc::ptsname_r(fd, name.as_mut_ptr(), name.len()), 0);
                let path = std::ffi::CStr::from_ptr(name.as_ptr()).to_str().unwrap();
                Self {
                    device: std::fs::File::from_raw_fd(fd),
                    path: path.to_string(),
                }
            }
        }

        /// What came out of the port, `len` bytes, waiting `within` at most: what is there
        /// already with no wait
        fn received(&mut self, len: usize, within: Duration) -> Vec<u8> {
            use std::io::Read;
            let deadline = Instant::now() + within;
            let mut bytes = Vec::new();
            let mut buf = [0; 256];
            while bytes.len() < len {
                match self.device.read(&mut buf) {
                    Ok(n) => bytes.extend_from_slice(&buf[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if Instant::now() >= deadline {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("{e}"),
                }
            }
            bytes
        }
    }

    /// MAX-12: command lines on a real tty, their effects applied as the main loop does: the
    /// bytes of `send` and `send-hex` reach the other end in order, `baud` and `framing`
    /// change the port's settings, and a setting or a line the port refuses stops the rest of
    /// the line.
    /// How: a pty pair in place of socat and `fake_device`; a pty refuses parity, 7 bits and
    /// the modem lines.
    #[cfg(target_os = "linux")]
    #[test]
    fn commands_on_the_wire() {
        use crate::link::serial::{CharSize, LineControl, SerialDevice, StopBits};
        let mut pty = Pty::open();
        let mut app = app();
        app.link.set_address(Address::Serial(pty.path.clone()));
        let mut device = SerialDevice::open(&pty.path, &app.link_config().line).unwrap();
        // a pty has no modem lines
        assert!(device.control(LineControl::Dtr(true)).is_err());
        let mut run = |app: &mut App, line: &str| {
            let invocations = crate::commands::parse(line).unwrap();
            app.update(Action::App(AppAction::QueueCommands(invocations)));
            while let Some(action) = app.next_command_action(Instant::now()) {
                match app.update(action) {
                    UpdateEffect::Transmit(bytes) => device.write(&bytes).unwrap(),
                    UpdateEffect::Reconfigure(line) => match device.reconfigure(&line) {
                        Ok(()) => set_line(app, line),
                        Err(_) => {
                            app.drop_pending_commands();
                        }
                    },
                    UpdateEffect::LineControl(control) => {
                        if let Err(e) = device.control(control) {
                            app.fail_commands(format!("{control}: {e}"));
                        }
                    }
                    UpdateEffect::None => {}
                    _ => panic!("{line}"),
                }
            }
            device.settings().unwrap()
        };

        run(&mut app, "send AT\\r\\n; send-hex 1b5b41; send 'a;b'");
        assert_eq!(pty.received(10, Duration::from_secs(1)), b"AT\r\n\x1b[Aa;b");

        let settings = run(&mut app, "baud 9600; framing 8N2");
        assert_eq!(settings.get_baud_rate().unwrap(), 9600);
        assert_eq!(settings.get_stop_bits().unwrap(), serial2::StopBits::Two);
        assert_eq!(app.link_config().line.baud, 9600);
        assert_eq!(app.link_config().line.framing.stop_bits, StopBits::Two);

        // a pty keeps 8 bits and no parity: the port refuses, the line stops there
        let settings = run(&mut app, "framing 7E1; send y");
        assert_eq!(settings.get_char_size().unwrap(), serial2::CharSize::Bits8);
        assert_eq!(app.link_config().line.framing.char_size, CharSize::Bits8);
        assert!(pty.received(1, Duration::from_millis(50)).is_empty());

        run(&mut app, "dtr on; send x");
        let toast = app
            .notifications
            .active_toasts()
            .last()
            .unwrap()
            .message
            .clone();
        assert!(toast.starts_with("DTR on: "), "{toast}");
        assert!(toast.ends_with("(1 more not run)"), "{toast}");
        assert!(pty.received(1, Duration::from_millis(50)).is_empty());
    }

    /// MAX-6: `mash` on a real tty, its steps run as the main loop does: it sends its text
    /// every interval, the device's prompt ends it and the rest of the line goes out after;
    /// with no prompt it stops at its timeout, nothing sent after.
    /// How: a pty pair, a space every 20 ms, the device answering `=> ` once it got five; then
    /// `x` every 20 ms for 150 ms with no answer.
    #[cfg(target_os = "linux")]
    #[test]
    fn mash_on_the_wire() {
        use std::io::Write;

        use crate::link::serial::SerialDevice;
        let mut pty = Pty::open();
        let mut app = app();
        app.link.set_address(Address::Serial(pty.path.clone()));
        let mut device = SerialDevice::open(&pty.path, &app.link_config().line).unwrap();
        // a turn of the main loop: what came in, the mash's step, the line's next commands
        let mut turn = |app: &mut App| {
            let mut buf = [0; 256];
            let n = device.read(&mut buf).unwrap();
            if n > 0 {
                app.feed_rx(&buf[..n]);
            }
            if let Some(bytes) = app.next_mash_bytes(Instant::now()) {
                device.write(&bytes).unwrap();
            }
            while let Some(action) = app.next_command_action(Instant::now()) {
                match app.update(action) {
                    UpdateEffect::Transmit(bytes) => device.write(&bytes).unwrap(),
                    UpdateEffect::None => {}
                    _ => panic!("unexpected effect"),
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        let queue = |app: &mut App, line: &str| {
            let invocations = crate::commands::parse(line).unwrap();
            app.update(Action::App(AppAction::QueueCommands(invocations)));
        };

        queue(&mut app, "mash ' ' '=> ' 20 2000; send ok");
        let start = Instant::now();
        let mut received = Vec::new();
        while received.len() < 5 {
            assert!(start.elapsed() < Duration::from_secs(1), "{received:?}");
            turn(&mut app);
            received.extend(pty.received(5 - received.len(), Duration::ZERO));
        }
        // five sends take four intervals at least
        assert!(start.elapsed() >= Duration::from_millis(80));
        assert_eq!(received, b"     ");
        pty.device.write_all(b"U-Boot> \r\n=> ").unwrap();
        let mut after = Vec::new();
        while !after.ends_with(b"ok") {
            assert!(start.elapsed() < Duration::from_secs(2), "{after:?}");
            turn(&mut app);
            after.extend(pty.received(1, Duration::ZERO));
        }
        assert!(app.mash().is_none());
        // a space may have gone out while the prompt came, then the rest of the line
        assert!(after == b"ok" || after == b" ok", "{after:?}");

        queue(&mut app, "mash x '=> ' 20 150; send late");
        let start = Instant::now();
        turn(&mut app);
        assert!(app.mash().is_some());
        while app.mash().is_some() {
            assert!(start.elapsed() < Duration::from_secs(1));
            turn(&mut app);
        }
        assert!(start.elapsed() >= Duration::from_millis(150));
        turn(&mut app);
        let sent = pty.received(64, Duration::from_millis(50));
        // one at once, then every 20 ms until 150 ms: 8 at most, never `late`
        assert!((6..=8).contains(&sent.len()), "{sent:?}");
        assert!(sent.iter().all(|&b| b == b'x'), "{sent:?}");
    }

    /// MAX-12: `send-file` starts a paced transfer and the chain goes on at once; its chunks
    /// come from `next_transfer_chunk` until the end of the file, with a notice. A second
    /// `send-file`, a missing file or a replay refuse; `cancel` stops it.
    /// How: a 300-byte file at 9600 baud (96 bytes per 100 ms at most).
    #[test]
    fn command_line_send_file() {
        let mut app = app();
        let mut config = app.link_config().clone();
        config.line.baud = 9600;
        app.link.set_config(config);
        let path =
            std::env::temp_dir().join(format!("maxicom_test_{}_send.txt", std::process::id()));
        std::fs::write(&path, [b'x'; 300]).unwrap();
        let file = path.display().to_string().replace('\\', "/");
        let run_line = |app: &mut App, line: &str| {
            app.update(KeyBindingAction::CommandLine.to_action(app));
            app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
            app.update(Action::Overlay(DialogAction::Confirm));
            let mut effects = Vec::new();
            while let Some(action) = app.next_command_action(Instant::now()) {
                effects.push(app.update(action));
            }
            effects
        };
        let last_toast = |app: &App| {
            app.notifications
                .active_toasts()
                .last()
                .unwrap()
                .message
                .clone()
        };

        let effects = run_line(&mut app, &format!("send-file {file}; send y"));
        assert!(matches!(
            &effects[..],
            [UpdateEffect::None, UpdateEffect::Transmit(y)] if y == b"y"
        ));
        assert_eq!(app.transfer().unwrap().percent(), 0);
        assert!(run_line(&mut app, &format!("send-file {file}")).is_empty());
        assert_eq!(
            last_toast(&app),
            "send-file: a file is being sent already (cancel stops it)"
        );

        let start = Instant::now();
        let mut sent = 0;
        for step in 1..=10 {
            if let Some(chunk) = app.next_transfer_chunk(start + Duration::from_secs(step)) {
                assert!(chunk.len() <= 96);
                sent += chunk.len();
            }
        }
        assert_eq!(sent, 300);
        assert!(app.transfer().is_none());
        assert!(
            last_toast(&app).starts_with("Sent maxicom_test_"),
            "{}",
            last_toast(&app)
        );

        run_line(&mut app, &format!("send-file {file}"));
        app.next_transfer_chunk(start + Duration::from_secs(1));
        run_line(&mut app, "cancel");
        assert!(app.transfer().is_none());
        assert!(
            last_toast(&app).ends_with(" at 32%"),
            "{}",
            last_toast(&app)
        );

        run_line(&mut app, &format!("send-file {file}.nope; help"));
        assert!(app.overlay().is_none());
        assert!(
            last_toast(&app).contains("(1 more not run)"),
            "{}",
            last_toast(&app)
        );

        app.link
            .set_address(Address::parse("file:///tmp/capture.raw"));
        run_line(&mut app, &format!("send-file {file}"));
        assert_eq!(
            last_toast(&app),
            "send-file: a replay has no link to send to"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// MAX-12: `baud`, `framing` and `flow` give the port's settings for the loop to apply,
    /// each on the settings the one before left; `dtr`, `rts` and `break` give the line
    /// controls. `rts` refuses with hardware flow control, and all refuse on a TCP link or a
    /// replay, stopping the chain.
    /// How: the loop's part played by the test, which applies each settings change.
    #[test]
    fn command_line_serial_lines() {
        use crate::link::serial::{CharSize, FlowControl, Parity, StopBits};
        let mut app = app();
        let run_line = |app: &mut App, line: &str| {
            app.update(KeyBindingAction::CommandLine.to_action(app));
            app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
            app.update(Action::Overlay(DialogAction::Confirm));
            let mut controls = Vec::new();
            while let Some(action) = app.next_command_action(Instant::now()) {
                match app.update(action) {
                    UpdateEffect::Reconfigure(line) => set_line(app, line),
                    UpdateEffect::LineControl(control) => controls.push(control),
                    _ => {}
                }
            }
            controls
        };
        let last_error = |app: &App| {
            app.notifications
                .active_toasts()
                .last()
                .unwrap()
                .message
                .clone()
        };

        run_line(&mut app, "baud 9600; framing 7e2; flow hw");
        let config = app.link_config();
        assert_eq!(config.line.baud, 9600);
        assert_eq!(
            (
                config.line.framing.char_size,
                config.line.framing.parity,
                config.line.framing.stop_bits
            ),
            (CharSize::Bits7, Parity::Even, StopBits::Two)
        );
        assert_eq!(config.line.flow_control, FlowControl::RtsCts);

        assert!(run_line(&mut app, "rts on").is_empty());
        assert_eq!(
            last_error(&app),
            "rts: hardware flow control drives RTS (flow none first)"
        );
        let controls = run_line(&mut app, "flow none; dtr off; rts on; break; break 100");
        assert_eq!(
            controls,
            [
                LineControl::Dtr(false),
                LineControl::Rts(true),
                LineControl::Break(Duration::from_millis(250)),
                LineControl::Break(Duration::from_millis(100)),
            ]
        );
        run_line(&mut app, "framing 9x1");
        assert!(last_error(&app).starts_with("framing: '9x1' isn't a framing"));

        app.link.set_address(Address::parse("tcp://localhost:4000"));
        run_line(&mut app, "baud 115200");
        assert_eq!(
            last_error(&app),
            "baud: a TCP link has no line settings, its bridge sets them"
        );
        assert!(run_line(&mut app, "dtr on; send x").is_empty());
        assert_eq!(
            last_error(&app),
            "dtr: a TCP link has no control lines, its bridge sets them (1 more not run)"
        );
        app.link
            .set_address(Address::parse("file:///tmp/capture.raw"));
        run_line(&mut app, "break");
        assert_eq!(last_error(&app), "break: a replay has no control lines");
        assert!(run_line(&mut app, "dtr on; rts on").is_empty());
        assert_eq!(
            last_error(&app),
            "dtr: a replay has no control lines (1 more not run)"
        );
        run_line(&mut app, "baud 115200");
        assert_eq!(last_error(&app), "baud: a replay has no line settings");
        assert_eq!(app.link_config().line.baud, 9600);
    }

    /// MAX-12: `sleep` pauses the rest of the chain, which goes on once its time is up; the
    /// pause the loop sets for a break does the same. A failure drops a paused chain, pause
    /// and all.
    /// How: the actions taken one by one, at chosen times.
    #[test]
    fn command_line_sleep() {
        let mut app = app();
        let queue_line = |app: &mut App, line: &str| {
            app.update(KeyBindingAction::CommandLine.to_action(app));
            app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
            app.update(Action::Overlay(DialogAction::Confirm));
        };
        let sent = |app: &mut App, action: Option<Action>| match app.update(action.unwrap()) {
            UpdateEffect::Transmit(bytes) => bytes,
            _ => panic!("expected a transmit"),
        };

        queue_line(&mut app, "send a; sleep 100; send b");
        let now = Instant::now();
        let action = app.next_command_action(now);
        assert_eq!(sent(&mut app, action), b"a");
        let sleep = app.next_command_action(now).unwrap();
        app.update(sleep);
        assert!(app.next_command_action(Instant::now()).is_none());
        let later = Instant::now() + Duration::from_millis(150);
        let action = app.next_command_action(later);
        assert_eq!(sent(&mut app, action), b"b");
        assert!(app.next_command_action(later).is_none());

        // the loop pauses the chain for a break
        queue_line(&mut app, "send c");
        app.pause_commands(later + Duration::from_millis(100));
        assert!(app.next_command_action(later).is_none());
        app.fail_commands("break: failed".to_string());
        assert!(
            app.next_command_action(later + Duration::from_secs(1))
                .is_none()
        );
        queue_line(&mut app, "send d");
        let action = app.next_command_action(Instant::now());
        assert_eq!(sent(&mut app, action), b"d");
        assert!(parse_fails(&mut app, "sleep"));
        assert!(parse_fails(&mut app, "sleep 60001"));
    }

    /// MAX-12: a line starting with `cancel` runs it at once, not behind the commands waiting:
    /// it drops them, pause and all, and stops the file being sent, then the rest of its line
    /// runs.
    /// How: a chain paused by `sleep`, then a transfer and a paused chain together.
    #[test]
    fn command_line_cancel() {
        use crate::app::transfer::FileTransfer;
        let mut app = app();
        let queue_line = |app: &mut App, line: &str| {
            app.update(KeyBindingAction::CommandLine.to_action(app));
            app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
            app.update(Action::Overlay(DialogAction::Confirm));
        };
        let last_toast = |app: &App| {
            app.notifications
                .active_toasts()
                .last()
                .unwrap()
                .message
                .clone()
        };

        queue_line(&mut app, "cancel");
        assert_eq!(last_toast(&app), "Nothing to cancel");

        queue_line(&mut app, "sleep 1000; send a");
        let sleep = app.next_command_action(Instant::now()).unwrap();
        app.update(sleep);
        queue_line(&mut app, "cancel; send b");
        assert_eq!(last_toast(&app), "Dropped 1 waiting command");
        let send = app.next_command_action(Instant::now()).unwrap();
        assert!(matches!(app.update(send), UpdateEffect::Transmit(b) if b == b"b"));

        let transfer = FileTransfer::new(
            Box::new(std::io::Cursor::new(vec![b'x'; 10])),
            "fw.bin".to_string(),
            10,
            960.0,
            Duration::ZERO,
            Instant::now(),
        );
        app.update(Action::App(AppAction::StartTransfer(Box::new(transfer))));
        queue_line(&mut app, "sleep 1000; send c; send d");
        let sleep = app.next_command_action(Instant::now()).unwrap();
        app.update(sleep);
        queue_line(&mut app, "cancel");
        assert_eq!(
            last_toast(&app),
            "Stopped sending fw.bin at 0%, dropped 2 waiting commands"
        );
        assert!(app.transfer().is_none());
        assert!(
            app.next_command_action(Instant::now() + Duration::from_secs(2))
                .is_none()
        );
    }

    /// MAX-12: a command line bound to a key runs as a typed one, but stays out of the command
    /// history; it waits behind a waiting line, unless it starts with `cancel`.
    #[test]
    fn bound_line_runs_as_typed() {
        let mut app = app();
        let line = |text: &str| crate::commands::parse(text).unwrap();
        app.update(Action::App(AppAction::QueueCommands(line(
            "send a; sleep 1000; send b",
        ))));
        let now = Instant::now();
        let send = app.next_command_action(now).unwrap();
        assert!(matches!(app.update(send), UpdateEffect::Transmit(a) if a == b"a"));
        let sleep = app.next_command_action(now).unwrap();
        app.update(sleep);
        app.update(Action::App(AppAction::QueueCommands(line(
            "cancel; send c",
        ))));
        let send = app.next_command_action(Instant::now()).unwrap();
        assert!(matches!(app.update(send), UpdateEffect::Transmit(c) if c == b"c"));
        assert!(app.next_command_action(Instant::now()).is_none());
        assert!(app.command_history().is_empty());
    }

    /// The line doesn't parse: the command line stays open
    fn parse_fails(app: &mut App, line: &str) -> bool {
        app.update(KeyBindingAction::CommandLine.to_action(app));
        app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
        app.update(Action::Overlay(DialogAction::Confirm));
        let open = matches!(app.overlay(), Some(Overlay::CommandLine(_)));
        app.close_overlay();
        open
    }

    /// MAX-12: `open` gives the port for the loop to open, its settings kept; the loop pauses
    /// the line until its result, which resumes it or fails it. While the link isn't open,
    /// `send` and the control lines refuse; a replay refuses `open`.
    /// How: the loop's part played by the test.
    #[test]
    fn command_line_open() {
        let mut app = app();
        let queue_line = |app: &mut App, line: &str| {
            app.update(KeyBindingAction::CommandLine.to_action(app));
            app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
            app.update(Action::Overlay(DialogAction::Confirm));
        };
        let last_toast = |app: &App| {
            app.notifications
                .active_toasts()
                .last()
                .unwrap()
                .message
                .clone()
        };
        let now = Instant::now();

        queue_line(&mut app, "open /dev/ttyUSB9; send x");
        let open = app.next_command_action(now).unwrap();
        assert!(
            matches!(app.update(open), UpdateEffect::Open(address) if address == Address::parse("/dev/ttyUSB9"))
        );
        // the loop: the link closed, the line waiting for the open
        set_link_open(&mut app, false);
        app.pause_commands(now + Duration::from_secs(6));
        assert!(app.next_command_action(now).is_none());
        set_link_open(&mut app, true);
        app.resume_commands();
        let send = app.next_command_action(now).unwrap();
        assert!(matches!(app.update(send), UpdateEffect::Transmit(x) if x == b"x"));

        set_link_open(&mut app, false);
        queue_line(&mut app, "send y; help");
        assert!(app.next_command_action(now).is_none());
        assert_eq!(
            last_toast(&app),
            "send: the port isn't open (detached or reconnecting) (1 more not run)"
        );
        queue_line(&mut app, "dtr on");
        assert!(app.next_command_action(now).is_none());
        assert_eq!(
            last_toast(&app),
            "dtr: the port isn't open (detached or reconnecting)"
        );
        // settings wait for the port to open
        queue_line(&mut app, "baud 9600");
        assert!(app.next_command_action(now).is_some());

        set_link_open(&mut app, true);
        queue_line(&mut app, "open file:///tmp/x.raw");
        let open = app.next_command_action(now).unwrap();
        assert!(
            matches!(app.update(open), UpdateEffect::Replay { path, delay, session: None }
            if path == Path::new("/tmp/x.raw") && delay.is_zero())
        );
        app.link
            .set_address(Address::parse("file:///tmp/capture.raw"));
        queue_line(&mut app, "open /dev/ttyUSB0");
        assert!(app.next_command_action(now).is_none());
        assert_eq!(
            last_toast(&app),
            "open: a replay can't open a link (replay-stop ends it)"
        );
    }

    /// MAX-166: `replay` gives the file to replay, `~` expanded, with its delay (none by
    /// default); `replay-stop` stops a replay and refuses without one. The attach key stops a
    /// replay started while running, not one from the command line.
    /// How: the commands run on a live port, then on a replay, with and without a live link
    /// set aside.
    #[test]
    fn command_line_replay() {
        let mut app = app();
        let now = Instant::now();
        let run = |app: &mut App, line: &str| {
            app.update(KeyBindingAction::CommandLine.to_action(app));
            app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
            app.update(Action::Overlay(DialogAction::Confirm));
            let action = app.next_command_action(now)?;
            Some(app.update(action))
        };
        let home = dirs::home_dir().unwrap();
        assert!(matches!(run(&mut app, "replay ~/x.raw 50"),
            Some(UpdateEffect::Replay { path, delay, session: None })
                if path == home.join("x.raw") && delay == Duration::from_millis(50)));
        assert!(matches!(run(&mut app, "replay /tmp/x.raw"),
            Some(UpdateEffect::Replay { delay, .. }) if delay.is_zero()));
        assert!(run(&mut app, "replay-stop").is_none());
        let last_toast = app.notifications.active_toasts().last().unwrap();
        assert_eq!(last_toast.message, "replay-stop: no replay runs");

        app.start_replay(PathBuf::from("/tmp/x.raw"), None);
        assert!(matches!(
            run(&mut app, "replay-stop"),
            Some(UpdateEffect::StopReplay)
        ));
        let detach = Action::App(AppAction::DetachSerial);
        assert!(matches!(app.update(detach), UpdateEffect::StopReplay));
        app.stop_replay();
        app.link.set_address(Address::parse("file:///tmp/x.raw"));
        let detach = Action::App(AppAction::DetachSerial);
        assert!(matches!(app.update(detach), UpdateEffect::ToggleAttach));
    }

    /// MAX-12: `set` turns an on/off setting on or off (nothing when it is so already),
    /// `encoding` sets the encoding, `search` opens the search bar on its text with its matches,
    /// `screenshot` saves the terminal at once in `screenshot_dir`, as text or with its colors
    /// (`.ans`), and refuses in line mode.
    /// How: each command run, then the state it left.
    #[test]
    fn command_line_set_encoding_search_screenshot() {
        let mut app = app();
        app.settings.mode = Mode::Line;
        let run_line = |app: &mut App, line: &str| {
            app.update(KeyBindingAction::CommandLine.to_action(app));
            app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
            app.update(Action::Overlay(DialogAction::Confirm));
            while let Some(action) = app.next_command_action(Instant::now()) {
                app.update(action);
            }
        };
        let last_toast = |app: &App| {
            app.notifications
                .active_toasts()
                .last()
                .unwrap()
                .message
                .clone()
        };

        let echo = app.settings.local_echo;
        let on_off = |on: bool| if on { "on" } else { "off" };
        run_line(&mut app, &format!("set local-echo {}", on_off(echo)));
        assert_eq!(app.settings.local_echo, echo);
        run_line(&mut app, &format!("set local-echo {}", on_off(!echo)));
        assert_eq!(app.settings.local_echo, !echo);
        run_line(
            &mut app,
            "set mouse off; set timestamps on; set answer-queries 0",
        );
        assert!(!app.settings.ui.mouse && app.settings.show_timestamps);
        assert!(!app.settings.terminal_mode.answer_queries);
        assert!(parse_fails(&mut app, "set line-wrap on"));

        run_line(&mut app, "encoding cp437");
        assert_eq!(app.settings.encoding, crate::configs::TextEncoding::Cp437);
        assert_eq!(last_toast(&app), "Rx Text Encoding is CP437");
        run_line(&mut app, "encoding utf8");

        app.feed_rx(b"hello world\r\nbye\r\n");
        run_line(&mut app, "search world");
        let search = app.line_mode.search();
        assert!(search.active);
        assert_eq!(search.query_str(), "world");
        assert_eq!(search.matches.len(), 1);

        run_line(&mut app, "screenshot shot");
        assert_eq!(
            last_toast(&app),
            "screenshot: screenshots are in terminal mode"
        );
        let dir =
            std::env::temp_dir().join(format!("maxicom_test_{}_screenshots", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        app.settings.capture.screenshot_dir = dir.clone();
        app.settings.mode = Mode::Terminal;
        app.feed_rx(b"\x1b[31mred\x1b[0m");
        run_line(&mut app, "screenshot shot; screenshot shot.ans");
        let text = std::fs::read_to_string(dir.join("shot.txt")).unwrap();
        let ansi = std::fs::read_to_string(dir.join("shot.ans")).unwrap();
        assert!(text.contains("red") && !text.contains('\x1b'));
        assert!(ansi.contains("\x1b["));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// MAX-12: the lines that ran are kept for the next command lines, trimmed, not twice in a
    /// row, the oldest dropped past `command_line.history_size`; a line that doesn't parse
    /// isn't kept. The next command line recalls them with Up.
    /// How: a history of 2, five lines run, one with a typo.
    #[test]
    fn command_line_history() {
        let mut app = app();
        app.settings.command_line.history_size = 2;
        for line in ["help ", "help", "nope", "settings", "toggle-mouse"] {
            app.update(KeyBindingAction::CommandLine.to_action(&app));
            app.update(Action::Overlay(DialogAction::Paste(line.to_string())));
            app.update(Action::Overlay(DialogAction::Confirm));
            while let Some(action) = app.next_command_action(Instant::now()) {
                app.update(action);
            }
            app.close_overlay();
        }
        assert_eq!(app.command_history(), &["settings", "toggle-mouse"]);

        app.update(KeyBindingAction::CommandLine.to_action(&app));
        let up = crossterm::event::KeyEvent::from(crossterm::event::KeyCode::Up);
        app.update(Action::Overlay(DialogAction::HandleKey(up)));
        app.update(Action::Overlay(DialogAction::HandleKey(up)));
        match app.overlay() {
            Some(Overlay::CommandLine(command_line)) => {
                assert_eq!(command_line.input.value(), "settings");
            }
            _ => panic!("the command line is closed"),
        }
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
    /// `toggle-logging` action (the `REC` click) stops both, and `REC` is on with either.
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
        let session = crate::capture::tests::write_test_session(
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

    /// MAX-166: a replay started while running takes the link's place, on fresh histories in
    /// terminal mode; the live link is set aside with its histories, counts and mode, its
    /// journal paused, and stopping the replay puts it all back, the journal going on in the
    /// same files. A replay replacing a replay keeps the live link; a replay from the command
    /// line has none to go back to: its stop ends it. A new replay isn't ended.
    /// How: "live" received in line mode with a journal, a replay, "replayed" fed, a second
    /// replay, the stop, "back" received; then an app on an ended replay from the start.
    #[test]
    fn replay_sets_the_live_link_aside() {
        let dir = crate::capture::tests::temp_dir("replay_aside");
        let mut app = app();
        app.settings_mut().mode = Mode::Line;
        app.start_journal(Some(dir.clone()));
        app.feed_rx(b"live\r\n");
        app.stats.update_rx(6);

        app.start_replay(PathBuf::from("/tmp/a.raw"), None);
        assert_eq!(
            app.link_config().address,
            Address::parse("file:///tmp/a.raw")
        );
        assert_eq!(app.settings().mode, Mode::Terminal);
        assert!(texts(&app).is_empty() && !app.has_journal());
        assert!(!app.terminal_mode.vt100_screen().contents().contains("live"));
        assert_eq!(app.stats.rx(), 0);
        app.feed_rx(b"replayed\r\n");
        assert_eq!(texts(&app), ["replayed"]);
        app.start_replay(PathBuf::from("/tmp/b.raw"), None);
        assert!(texts(&app).is_empty());

        assert!(app.stop_replay());
        assert_eq!(app.link_config().address, Address::parse("/dev/ttyUSB0"));
        assert_eq!(app.settings().mode, Mode::Line);
        assert_eq!(texts(&app), ["live"]);
        assert!(app.terminal_mode.vt100_screen().contents().contains("live"));
        assert_eq!(app.stats.rx(), 6);
        assert!(app.has_journal() && !app.stop_replay());
        app.feed_rx(b"back\r\n");
        drop(app);
        let raw = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().is_some_and(|ext| ext == "raw"))
            .unwrap();
        assert_eq!(std::fs::read(raw).unwrap(), b"live\r\nback\r\n");
        let _ = std::fs::remove_dir_all(&dir);

        let mut app = self::app();
        app.link.set_address(Address::parse("file:///tmp/a.raw"));
        app.end_replay();
        app.start_replay(PathBuf::from("/tmp/b.raw"), None);
        assert!(!app.is_replay_ended());
        assert!(!app.stop_replay() && app.is_replay_ended());
    }

    /// MAX-166: a session's replay decodes with its meta's encoding and shows its line
    /// settings; stopping it gives the live link its own encoding back. A plain file replayed
    /// after it decodes with the live link's encoding and has no line settings, nor has a
    /// session recorded over TCP. A serial port's are its config's, a TCP link has none.
    /// How: a live app in UTF-8, a cp437 session at 9600 7E1 replayed, a plain file, the stop;
    /// then a TCP session, and the link kinds.
    #[test]
    fn session_replay_uses_its_meta() {
        let mut app = app();
        let mut session = crate::capture::tests::device("/dev/ttyACM1");
        session.encoding = crate::configs::TextEncoding::Cp437;
        session.line = Some(SessionLine {
            baud: 9600,
            framing: "7E1".parse().unwrap(),
        });
        assert_eq!(
            app.line_settings(),
            Some(SessionLine::of(&app.link_config().line))
        );
        assert_eq!(app.recorded_line(), None);

        app.start_replay(PathBuf::from("/tmp/s.raw"), Some(&session));
        assert_eq!(app.settings().encoding, TextEncoding::Cp437);
        assert_eq!(app.line_settings(), session.line);
        assert_eq!(app.recorded_line(), session.line);
        app.start_replay(PathBuf::from("/tmp/plain.raw"), None);
        assert_eq!(app.settings().encoding, TextEncoding::Utf8);
        assert_eq!(app.line_settings(), None);
        app.start_replay(PathBuf::from("/tmp/s.raw"), Some(&session));
        assert!(app.stop_replay());
        assert_eq!(app.settings().encoding, TextEncoding::Utf8);
        assert_eq!(app.recorded_line(), None);

        let tcp = crate::capture::tests::device("tcp://192.168.1.50:4000");
        app.start_replay(PathBuf::from("/tmp/t.raw"), Some(&tcp));
        assert_eq!(app.line_settings(), None);
        assert!(app.stop_replay());
        app.link
            .set_address(Address::parse("tcp://192.168.1.50:4000"));
        assert_eq!(app.line_settings(), None);
    }

    /// MAX-148: the last session restores at startup with `always` or `--restore`, not with
    /// `never`, `ask` (before its answer) or `--no-restore`, nor when older than
    /// `restore_max_age`; `0` is no limit. A session restores again on the next launch.
    /// How: one session on the app's port, then each case on a fresh app.
    #[test]
    fn last_session_restores_as_settings_say() {
        let dir =
            std::env::temp_dir().join(format!("maxicom_test_{}_restore_last", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let port = LinkConfig::default().address.to_string();
        let session = crate::capture::tests::write_test_session(
            &dir,
            &port,
            &JournalText::default(),
            b"old line\r\n",
            b"",
        );
        let restored = |mode: RestoreMode, flag: Option<bool>, max_age: u64| {
            let mut app = app();
            app.settings.capture.restore = mode;
            app.settings.capture.restore_max_age = Duration::from_secs(max_age);
            app.restore_last_session(Some(&dir), flag);
            app.line_mode.history().iter().any(|e| e.text == "old line")
        };
        assert!(restored(RestoreMode::Always, None, 3600));
        assert!(restored(RestoreMode::Always, None, 3600), "a second launch");
        assert!(!restored(RestoreMode::Never, None, 3600));
        // `ask` asks first: nothing restored before the answer
        assert!(!restored(RestoreMode::Ask, None, 3600));
        assert!(restored(RestoreMode::Never, Some(true), 3600));
        assert!(!restored(RestoreMode::Always, Some(false), 3600));

        let two_hours_ago = std::time::SystemTime::now() - Duration::from_secs(2 * 3600);
        std::fs::File::options()
            .write(true)
            .open(session.raw_path().unwrap())
            .unwrap()
            .set_modified(two_hours_ago)
            .unwrap();
        assert!(!restored(RestoreMode::Always, None, 3600));
        assert!(restored(RestoreMode::Always, None, 0));

        let mut app = app();
        app.settings.capture.restore = RestoreMode::Always;
        app.restore_last_session(None, None);
        assert!(app.line_mode.history().is_empty(), "no cache folder");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An app whose `ask` dialog is open on a session with "old line", and its folder.
    fn asking_app(name: &str) -> (App, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("maxicom_test_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::capture::tests::write_test_session(
            &dir,
            &LinkConfig::default().address.to_string(),
            &JournalText::default(),
            b"old line\r\n",
            b"",
        );
        let mut app = app();
        app.settings.mode = Mode::Line;
        app.settings.capture.restore = RestoreMode::Ask;
        app.restore_last_session(Some(&dir), None);
        assert!(matches!(app.overlay(), Some(Overlay::RestoreSession(_))));
        (app, dir)
    }

    /// The history's texts without their EOL, the separator as its start
    fn texts(app: &App) -> Vec<String> {
        let history = app.line_mode.history().iter();
        history
            .map(|e| match e.text.trim_end_matches(['\r', '\n']) {
                text if text.starts_with("── restored") => "── restored".to_string(),
                text => text.to_string(),
            })
            .collect()
    }

    /// MAX-148: with `ask`, what comes in while the dialog is open waits; Enter (or `y`)
    /// restores the session first, then feeds it; Esc (or `n`) feeds it without a restore.
    /// How: "live" received while asking, then each answer on a fresh app.
    #[test]
    fn ask_dialog_holds_rx_until_the_answer() {
        let key = |c| {
            DialogAction::HandleKey(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(c),
                crossterm::event::KeyModifiers::NONE,
            ))
        };
        for (answer, restored) in [
            (DialogAction::Confirm, true),
            (key('y'), true),
            (DialogAction::Close, false),
            (key('n'), false),
        ] {
            let (mut app, dir) = asking_app("ask_answer");
            app.feed_rx(b"live\r\n");
            assert!(app.line_mode.history().is_empty());
            app.update(Action::Overlay(answer));
            assert!(app.overlay().is_none());
            let expected: &[&str] = if restored {
                &["old line", "── restored", "live"]
            } else {
                &["live"]
            };
            assert_eq!(texts(&app), expected);
            app.feed_rx(b"after\r\n");
            assert_eq!(texts(&app).last().unwrap(), "after");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// MAX-148: the held data is never stuck: when another popup replaced the dialog, or more
    /// than the cap came in, it is fed without a restore.
    #[test]
    fn ask_dialog_never_keeps_rx_stuck() {
        let (mut app, dir) = asking_app("ask_stuck");
        app.feed_rx(b"held\r\n");
        app.open_overlay(Overlay::ConfirmQuit);
        app.feed_rx(b"next\r\n");
        assert_eq!(texts(&app), ["held", "next"]);

        let (mut app, _) = asking_app("ask_stuck");
        app.feed_rx(b"held\r\n");
        app.feed_rx(&vec![b'x'; HELD_RX_MAX]);
        assert!(app.overlay().is_none());
        assert_eq!(texts(&app), ["held"]);
        app.feed_rx(b"\r\n");
        assert_eq!(texts(&app)[1].len(), HELD_RX_MAX);
        let _ = std::fs::remove_dir_all(&dir);
    }

    const DSR: &[u8] = b"\x1b[5n";
    const DSR_READY: &[u8] = b"\x1b[0n";

    /// MAX-151: a query received in terminal mode is answered, once; in line mode it is not,
    /// even when the terminal is fed too, and it is not answered later on a switch.
    /// How: the default `always_both` buffering, DSR status received in each mode.
    #[test]
    fn queries_answered_in_terminal_mode_only() {
        let mut app = app();
        app.settings.mode = Mode::Terminal;
        app.feed_rx(DSR);
        assert_eq!(app.take_query_answers(), DSR_READY);
        assert!(app.take_query_answers().is_empty());

        app.settings.mode = Mode::Line;
        app.feed_rx(DSR);
        assert!(app.take_query_answers().is_empty());
        app.settings.mode = Mode::Terminal;
        app.feed_rx(b"x");
        assert!(app.take_query_answers().is_empty());
    }

    /// MAX-151: the settings apply to what is received after them: the popup toggle turns the
    /// answers off and on, and the answerback answers ENQ once set.
    #[test]
    fn query_answers_follow_the_settings() {
        let mut app = app();
        app.settings.mode = Mode::Terminal;
        app.toggle_setting(SettingKey::AnswerQueries);
        app.feed_rx(DSR);
        assert!(app.take_query_answers().is_empty());
        app.toggle_setting(SettingKey::AnswerQueries);
        app.feed_rx(DSR);
        assert_eq!(app.take_query_answers(), DSR_READY);

        app.feed_rx(b"\x05");
        assert!(app.take_query_answers().is_empty());
        app.settings.terminal_mode.answerback = "board-42".to_string();
        app.feed_rx(b"\x05");
        assert_eq!(app.take_query_answers(), b"board-42");
    }

    /// MAX-151: the queries of a restored session, and the ones held while the restore dialog
    /// asked, are not answered; what comes after the answer is.
    /// How: a session ending in a DSR, restored; then DSR held by the dialog, answered `n`.
    #[test]
    fn restored_and_held_queries_unanswered() {
        let base =
            std::env::temp_dir().join(format!("maxicom_test_{}_answers", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let session = crate::capture::tests::write_test_session(
            &base,
            "/dev/ttyUSB0",
            &JournalText::default(),
            b"old\r\n\x1b[5n",
            b"",
        );
        let mut app = app();
        app.settings.mode = Mode::Terminal;
        app.restore_session(&session).unwrap();
        app.feed_rx(b"x");
        assert!(app.take_query_answers().is_empty());
        let _ = std::fs::remove_dir_all(&base);

        let (mut app, dir) = asking_app("ask_answers");
        app.settings.mode = Mode::Terminal;
        app.feed_rx(DSR);
        let no = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('n'),
            crossterm::event::KeyModifiers::NONE,
        );
        app.update(Action::Overlay(DialogAction::HandleKey(no)));
        assert!(app.overlay().is_none());
        assert!(app.take_query_answers().is_empty());
        app.feed_rx(DSR);
        assert_eq!(app.take_query_answers(), DSR_READY);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
