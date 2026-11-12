use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::{
    app::{
        mash::{Mash, MashStep},
        plots::Plot,
        scroller::ScrollStatus,
        search::{
            SEARCH_HISTORY_SIZE, SearchDirection, SearchHost, SearchModeKind, SearchState,
            SearchView, word_at,
        },
        selection::{ClickCounter, CopySide, LogicalPosition, MouseUnit},
        term_answers::answer_bytes,
        triggers::Trigger,
        types::{FocusTarget, FrameAreas, FrameMap, ViewportSize},
    },
    capture::{
        CaptureDevice, CaptureLine, Recorder, RecordingWriter, ScreenshotFormat, Session, Stream,
    },
    commands::{CommandError, Invocation, Target},
    configs::{
        DualBufferMode, KeyBindingAction, KeyResolver, LinksConfig, Mode, PlotsConfig, RawSideView,
        SettingsConfig, StatusBarLayout, TextEncoding, WrapMode,
        settings::{PrefixPlace, PrefixWrapBehavior, SPLIT_PERCENT_MAX, SPLIT_PERCENT_MIN},
    },
    inputs::{
        Action, AppAction, ClickHoldAction, CopyModeAction, HistoryAction, LineInputAction,
        MarkAction, PendingMark, ScrollbarAction, SearchModeAction, SideHandleAction,
        SideViewAction, TerminalAction,
    },
    link::{
        Address, LinkConfig, LinkKind, LinkState,
        replay::Replay,
        serial::{LineControl, LineSettings},
    },
    notification::{NotificationManager, ToastLevel},
    overlays::{MarkBrowser, Overlay, OverlayResponse, PopupTarget, ScreenshotDialog, SettingKey},
    paths::{create_parent_dirs, expand_home},
    transfer::{FileTransfer, Step, modem::Protocol},
};

/// How soon after the window gains focus a press is the one that focused it
const FOCUS_CLICK: Duration = Duration::from_millis(200);

pub mod capture;
pub mod data_processing;
pub mod device;
pub mod line_mode;
pub mod marks;
pub mod mash;
pub mod plots;
pub mod raw_scroller;
pub mod scroller;
pub mod search;
pub mod selection;
pub mod term_answers;
pub mod terminal_mode;
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

/// How long the line a mark jump lands on flashes (MAX-164)
const MARK_FLASH: Duration = Duration::from_millis(400);

pub struct App {
    settings: SettingsConfig,
    pub key_resolver: KeyResolver,
    /// The link: its config, the device while open, the open running (MAX-168)
    link: LinkState,
    /// The missing device's popup waits for the restore question's answer (MAX-148)
    disconnected_after_restore: bool,
    /// Started on the directory, no device picked yet: the link waits detached (MAX-181)
    device_unpicked: bool,
    /// The disconnected popup was shown and the link hasn't opened since (MAX-181)
    link_lost: bool,
    /// The read error that closed the link, said in its popup; none for a plain close
    /// (MAX-199)
    lost_reason: Option<String>,
    /// A failed write was said: quiet until a write goes through or the link opens again
    /// (MAX-199)
    write_failing: bool,
    /// The directory's port test running (MAX-181)
    link_test: Option<device::LinkTest>,
    prefix_active: bool,
    /// An add or jump mark key waits for its letter (MAX-164)
    pending_mark: Option<PendingMark>,
    /// The line a mark jump landed on, in its mode's view, flashed until then (MAX-164)
    mark_flash: Option<(Mode, usize, Instant)>,
    overlay: Option<Overlay>,
    pub notifications: NotificationManager,
    pub viewport: ViewportSize,
    frame_areas: FrameAreas,
    frame_map: FrameMap,
    clicks: ClickCounter,
    /// A plain press on the terminal's text, out of copy mode: a drag from it enters copy mode
    /// there, a release drops it, so a click only focuses
    pending_press: Option<(LogicalPosition, CopySide)>,
    /// The screen cell of a drag in the history while the button is held, and when the drag
    /// scroll last ran (MAX-126)
    drag_hold: Option<(u16, u16)>,
    last_drag_scroll: Option<Instant>,
    scrollbar_hold: Option<ScrollbarHold>,
    side_handle_held: bool,
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
    /// The searches Enter ran, oldest first, for Up / Down in the bar (MAX-218)
    search_history: VecDeque<String>,
    pub line_mode: LineMode,
    pub terminal_mode: TerminalMode,
    recorder: Recorder,
    /// What is received while the restore dialog asks, fed after the answer (MAX-148)
    held_rx: Option<Vec<u8>>,
    /// `--restore` / `--no-restore`, for the start and each device switched to (MAX-187)
    restore_flag: Option<bool>,
    /// The replies to the remote's queries, for the loop to send (MAX-151)
    query_answers: Vec<u8>,
    /// The rules of `triggers.toml` (MAX-6)
    triggers: Vec<Trigger>,
    /// The links of `links.toml`, or why it didn't load, and this machine's name, which picks
    /// the ones for it (MAX-185)
    links: Result<LinksConfig, String>,
    host: Option<String>,
    /// The text `mash` sends until another comes in (MAX-6)
    mash: Option<Mash>,
    /// The plots of `plots.toml`, the one shown and when its clock started (MAX-222)
    plots: PlotsConfig,
    plot: Option<Plot>,
    plot_started: Instant,
    /// Where the session backups are, for the session browser (MAX-148)
    sessions_dir: Option<PathBuf>,
    /// The session the history was filled from, for the session browser's mark (MAX-202)
    restored_session: Option<String>,
    pub is_window_focused: bool,
    /// When the window last gained focus, until the next press: one this soon after only
    /// focused it
    focused_at: Option<Instant>,
    /// When a press came while the window was unfocused: a focus gained just after is that
    /// press's, not a new one
    unfocused_press_at: Option<Instant>,
    pointer: Option<(u16, u16)>,
    /// The last text copied, which a right-click pastes (MAX-144)
    paste_buffer: Option<String>,
    /// The last frame's size, for the parts a small screen hides (MAX-136, MAX-181)
    screen_size: Option<(u16, u16)>,
    pub stats: Stats,
    /// The file `send-file` or `upload` sends, a step per loop turn (MAX-12, MAX-162)
    transfer: Option<FileTransfer>,
    /// A write was refused while a protocol transfer owns the link, and said: once per
    /// transfer (MAX-162)
    busy_said: bool,
    /// After a protocol transfer was cancelled, until when the other side's ACK or NAK gets
    /// the CANs again (MAX-162)
    recancel_until: Option<Instant>,
    /// The replay running, while the link is one (MAX-166)
    replay: Option<ReplayRun>,
}

/// The live link a replay replaced (MAX-166): its settings, the mode, the encoding, the
/// histories of both modes, the counts and its session backup, put back as they were when the
/// replay stops (MAX-171: the backup kept here only, so it can't stay behind)
struct ReplacedLink {
    link_config: LinkConfig,
    mode: Mode,
    encoding: TextEncoding,
    line_mode: LineMode,
    terminal_mode: TerminalMode,
    stats: Stats,
    session: Option<RecordingWriter>,
}

/// The replay running (MAX-166), new for each one, there exactly while the link is a replay
/// (MAX-175): the device of the session it plays, when its meta gives it, whether it read its
/// whole file or was stopped without a link to go back to, and the live link it set aside, if
/// it was started while running
struct ReplayRun {
    device: Option<CaptureDevice>,
    ended: bool,
    /// How many sessions a group replay loaded at once (MAX-202)
    group: Option<usize>,
    replaced: Option<Box<ReplacedLink>>,
}

// Effects outside which can only be handled by main
pub enum UpdateEffect {
    None,
    Transmit(Vec<u8>),
    Reconfigure(LineSettings),
    LineControl(LineControl),
    Open(Address),
    OpenLink(LinkConfig),
    /// `session` is what a session backup was recorded on, from its meta file: the replay
    /// decodes with its encoding and shows its line settings (MAX-166)
    Replay {
        path: PathBuf,
        delay: Duration,
        session: Option<CaptureDevice>,
    },
    /// The sessions of a group to load at once, oldest first (MAX-202)
    ReplayGroup(Vec<Session>),
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
            pending_mark: None,
            mark_flash: None,
            settings,
            key_resolver,
            link: LinkState::new(link_config),
            disconnected_after_restore: false,
            device_unpicked: false,
            link_lost: false,
            lost_reason: None,
            write_failing: false,
            link_test: None,
            overlay: None,
            notifications: NotificationManager::new(),
            viewport: ViewportSize::default(),
            frame_areas: FrameAreas::default(),
            frame_map: FrameMap::default(),
            clicks: ClickCounter::default(),
            pending_press: None,
            drag_hold: None,
            last_drag_scroll: None,
            scrollbar_hold: None,
            side_handle_held: false,
            scrollbar_repeat_at: None,
            click_hold: None,
            click_repeat_at: None,
            pending_commands: VecDeque::new(),
            commands_resume_at: None,
            command_history: VecDeque::new(),
            search_history: VecDeque::new(),
            line_mode: line_view,
            terminal_mode: terminal_view,
            recorder: Recorder::default(),
            held_rx: None,
            restore_flag: None,
            query_answers: Vec::new(),
            triggers: Vec::new(),
            links: Ok(LinksConfig::default()),
            host: crate::configs::links::this_host(),
            mash: None,
            plots: PlotsConfig::default(),
            plot: None,
            plot_started: Instant::now(),
            sessions_dir: None,
            restored_session: None,
            is_window_focused: true,
            focused_at: None,
            unfocused_press_at: None,
            pointer: None,
            paste_buffer: None,
            screen_size: None,
            stats: Stats {
                received_bytes: 0,
                sent_bytes: 0,
            },
            transfer: None,
            busy_said: false,
            recancel_until: None,
            replay: None,
        };
        app.fit_mode_to_link();
        app.warn_shadowed_keys();
        app
    }

    /// The file `send-file` or `upload` sends (MAX-12, MAX-162)
    pub fn transfer(&self) -> Option<&FileTransfer> {
        self.transfer.as_ref()
    }

    /// The progress row shows while a file is sent, unless `ui.transfer_row` is off (MAX-162)
    pub fn show_transfer_row(&self) -> bool {
        self.transfer.is_some() && self.settings.ui.transfer_row
    }

    /// Whether a protocol transfer owns the link: nothing else may write to it (MAX-162)
    pub fn link_busy(&self) -> bool {
        self.transfer.as_ref().is_some_and(FileTransfer::owns_link)
    }

    /// A loop turn of the file going through, with `rx`, what the link received for a
    /// protocol (empty otherwise): the bytes to write, and the part of `rx` that is the
    /// protocol's. Each file a download completes is said; the end of the transfer, its
    /// failure or a read / write error ends it with a notice
    pub fn transfer_step(&mut self, rx: &[u8], now: Instant) -> Step {
        let idle = Step {
            out: Vec::new(),
            protocol: rx.len()..rx.len(),
        };
        let Some(transfer) = self.transfer.as_mut() else {
            return idle;
        };
        let step = match transfer.step(rx, now) {
            Ok(step) => step,
            Err(e) => {
                // a protocol's other side is told to stop too
                let out = transfer.cancel();
                let what = if transfer.is_download() {
                    "write"
                } else {
                    "read"
                };
                self.stop_transfer(&format!("{what} error: {e}"));
                return Step { out, ..idle };
            }
        };
        let received = transfer.take_received();
        let outcome = transfer.outcome();
        // by name as they come: the folder is said once, at the end (MAX-228)
        for (path, size) in received {
            let message = format!("Received {} ({size} bytes)", file_name(&path));
            self.notify_transfer(ToastLevel::Success, message);
        }
        match outcome {
            None => {}
            Some(Ok(())) => {
                let Some(transfer) = self.transfer.take() else {
                    return step;
                };
                if transfer.is_download() {
                    self.notify_received(&transfer);
                } else {
                    let protocol = transfer.protocol().map(|p| format!(", {}", p.name()));
                    let message = format!(
                        "Sent {} ({} bytes{})",
                        transfer.name(),
                        transfer.size().unwrap_or_default(),
                        protocol.unwrap_or_default()
                    );
                    self.notify_transfer(ToastLevel::Success, message);
                }
            }
            Some(Err(failure)) => self.stop_transfer(&failure.to_string()),
        }
        step
    }

    /// The end of a download, in the transfer's box: the file or how many, and the folder
    /// once (MAX-228). A YMODEM batch can hold none
    fn notify_received(&mut self, transfer: &FileTransfer) {
        let folder = transfer.folder().map(|folder| folder.display().to_string());
        let folder = folder.unwrap_or_default();
        let message = match (transfer.received_count(), transfer.last_received()) {
            (0, _) => "Nothing received: the sender sent no file".to_string(),
            (1, Some((path, size))) => {
                format!("Received {} ({size} bytes) into {folder}", file_name(path))
            }
            (count, _) => format!(
                "Received {count} files ({} bytes) into {folder}",
                transfer.received_total()
            ),
        };
        let level = match transfer.received_count() {
            0 => ToastLevel::Info,
            _ => ToastLevel::Success,
        };
        self.notify_transfer(level, message);
    }

    /// Ends the file going through, the link can't take it (`why`)
    pub fn stop_transfer(&mut self, why: &str) {
        if let Some(transfer) = self.transfer.take() {
            let message = format!("{}: {why}", stopped_text(&transfer));
            self.notify_transfer(ToastLevel::Error, message);
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
    /// either while the chain is paused (before `now`) or the restore dialog asks, as the
    /// device an `open` switched to may (MAX-187)
    pub fn next_command_action(&mut self, now: Instant) -> Option<Action> {
        if matches!(self.overlay, Some(Overlay::RestoreSession(_))) {
            return None;
        }
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
        // the transfer says it in its own box, "Cancelled", where a failure says "Stopped"
        // (MAX-228)
        let transfer_cancelled = self.transfer.is_some();
        if let Some(mut transfer) = self.transfer.take() {
            // a protocol's receiver is told to stop too
            let cans = transfer.cancel();
            if !cans.is_empty() {
                self.write_link(&cans, false);
                self.recancel_until = Some(Instant::now() + RECANCEL_FOR);
            }
            self.notify_transfer(ToastLevel::Warn, cancelled_text(&transfer));
        }
        let mut stopped = Vec::new();
        if self.mash.take().is_some() {
            stopped.push("Stopped mashing".to_string());
        }
        match self.drop_pending_commands() {
            0 => {}
            1 => stopped.push("dropped 1 waiting command".to_string()),
            n => stopped.push(format!("dropped {n} waiting commands")),
        }
        if !stopped.is_empty() {
            let mut message = stopped.join(", ");
            message[..1].make_ascii_uppercase();
            self.notify_warn(message);
        } else if !transfer_cancelled {
            self.notify_info("Nothing to cancel");
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

    /// What the current link can do, taken from its address (MAX-166)
    pub fn link_kind(&self) -> LinkKind {
        self.link.kind()
    }

    /// The line settings the status bar and the settings dialog show (MAX-166): a serial port's,
    /// a replayed serial session's from its meta; none for a TCP link nor a plain file
    pub fn line_settings(&self) -> Option<CaptureLine> {
        match self.link_kind() {
            LinkKind::Serial => Some(CaptureLine::of(&self.link.config().line)),
            LinkKind::Tcp => None,
            LinkKind::Replay => self.recorded_line(),
        }
    }

    /// The line settings of the session the replay plays, from its meta (MAX-166): shown, never
    /// set. None for a plain file, a session recorded over TCP, or a link that isn't a replay
    pub fn recorded_line(&self) -> Option<CaptureLine> {
        self.replay_device().and_then(|device| device.line)
    }

    /// The device of the session the replay plays, from its meta (MAX-166)
    fn replay_device(&self) -> Option<&CaptureDevice> {
        self.replay.as_ref().and_then(|run| run.device.as_ref())
    }

    /// How many sessions the replay loaded at once, none for a replay reading its file
    /// (MAX-202)
    pub fn replay_group(&self) -> Option<usize> {
        self.replay.as_ref().and_then(|run| run.group)
    }

    /// What the status bar calls a group replay: `ttyUSB0 · 12 sessions` (MAX-202)
    pub fn replay_group_label(&self) -> Option<String> {
        let count = crate::overlays::sessions::sessions(self.replay_group()?);
        Some(match self.replay_device() {
            Some(device) => format!("{} · {count}", device.address.tag()),
            None => count,
        })
    }

    /// The live link the replay set aside, to go back to (MAX-166)
    fn replaced_link(&self) -> Option<&ReplacedLink> {
        self.replay.as_ref().and_then(|run| run.replaced.as_deref())
    }

    /// A replay holds the place of a live link, which `toggle-attach` goes back to
    pub fn replays_over_live_link(&self) -> bool {
        self.replaced_link().is_some()
    }

    /// The replay of the file at `path` takes the link's place, on a reset terminal in terminal
    /// mode (MAX-166). A live link is set aside with its encoding, histories and counts, its
    /// session backup paused; what it was sending stops. A replay replacing a replay keeps the live
    /// link. A session backup (`session`) decodes with its own encoding, a plain file with the
    /// live link's
    pub fn start_replay(&mut self, path: PathBuf, session: Option<&CaptureDevice>) {
        self.stop_transfer("a replay took the link's place");
        self.stop_mash("a replay took the link's place");
        let (line_mode, terminal_mode, stats) = self.clear_screen();
        // a replay replacing a replay keeps the live link the first one set aside
        let replaced = match self.replay.take().and_then(|run| run.replaced) {
            Some(replaced) => Some(replaced),
            None if self.link_kind().keeps_sessions() => Some(Box::new(ReplacedLink {
                link_config: self.link.config().clone(),
                mode: self.settings.mode,
                encoding: self.settings.encoding,
                line_mode,
                terminal_mode,
                stats,
                session: self.pause_session(),
            })),
            None => None,
        };
        let live_encoding = replaced
            .as_ref()
            .map_or(self.settings.encoding, |replaced| replaced.encoding);
        self.link.set_address(Address::Replay(path));
        self.fit_mode_to_link();
        self.settings.encoding = session.map_or(live_encoding, |device| device.encoding);
        self.replay = Some(ReplayRun {
            device: session.cloned(),
            ended: false,
            group: None,
            replaced,
        });
    }

    /// Terminal mode on a link that isn't written (a replay), as line mode is for typing to it
    /// (MAX-168): at start, on a reload and when a replay starts
    fn fit_mode_to_link(&mut self) {
        if !self.link_kind().writes() {
            self.settings.mode = Mode::Terminal;
        }
    }

    /// Fresh histories of both modes and fresh counts, the ones they replace given back: for a
    /// replay to play on (MAX-166) and for a device switched to (MAX-187)
    fn clear_screen(&mut self) -> (LineMode, TerminalMode, Stats) {
        self.clear_plot();
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
        self.clear_screen();
        self.set_replay_ended(false);
    }

    fn set_replay_ended(&mut self, ended: bool) {
        if let Some(run) = &mut self.replay {
            run.ended = ended;
        }
    }

    /// The replay reading, for its progress and its controls (MAX-166)
    pub fn replay(&self) -> Option<&Replay> {
        self.link.replay()
    }

    /// The replay read its whole file: said once, its screen left as it is (MAX-166)
    pub fn end_replay(&mut self) {
        self.set_replay_ended(true);
        self.notify_info("Replay finished");
    }

    pub fn is_replay_ended(&self) -> bool {
        self.replay.as_ref().is_some_and(|run| run.ended)
    }

    /// Puts back the live link the replay replaced, for the loop to open, and resumes its
    /// session backup (MAX-166). False when there is none: a replay from the command line
    pub fn stop_replay(&mut self) -> bool {
        let Some(replaced) = self.replay.as_mut().and_then(|run| run.replaced.take()) else {
            self.set_replay_ended(true);
            return false;
        };
        self.replay = None;
        self.clear_plot();
        self.link.set_config(replaced.link_config);
        self.settings.mode = replaced.mode;
        self.settings.encoding = replaced.encoding;
        self.line_mode = replaced.line_mode;
        self.terminal_mode = replaced.terminal_mode;
        self.stats = replaced.stats;
        self.resume_session(replaced.session);
        true
    }

    /// A warning for the keymap's keys that never reach their mode section (MAX-155)
    fn warn_shadowed_keys(&mut self) {
        let shadowed = self.key_resolver.shadowed_keys();
        if !shadowed.is_empty() {
            self.notify_warn(format!("Keymap: unreachable {}", shadowed.join(", ")));
        }
    }

    /// The next frames hide the borders, the scrollbar and the status bar each while the
    /// screen is smaller than its `hide_*_below` size, the settings left as they are (MAX-136,
    /// MAX-181). Set by `render`, so the mouse sees what the last frame drew
    pub fn set_screen_size(&mut self, cols: u16, rows: u16) {
        self.screen_size = Some((cols, rows));
    }

    /// The last frame was at least `[cols, rows]`, a `0` side always; true before the first
    fn screen_fits(&self, [min_cols, min_rows]: [u16; 2]) -> bool {
        self.screen_size
            .is_none_or(|(cols, rows)| cols >= min_cols && rows >= min_rows)
    }

    pub fn show_borders(&self) -> bool {
        let ui = &self.settings.ui;
        ui.show_borders && self.screen_fits(ui.hide_borders_below)
    }

    pub fn show_scrollbar(&self) -> bool {
        let ui = &self.settings.ui;
        ui.show_scrollbar && self.screen_fits(ui.hide_scrollbar_below)
    }

    pub fn show_status_bar(&self) -> bool {
        let ui = &self.settings.ui;
        ui.show_status_bar && self.screen_fits(ui.hide_status_bar_below)
    }

    /// The status bar the screen's size hides, shown while the prefix is on over the view's
    /// bottom row, so the prefix and the link can be seen: nothing moves and the remote isn't
    /// resized (MAX-181). Not when `show_status_bar` turns it off
    pub fn status_bar_over_view(&self) -> bool {
        let ui = &self.settings.ui;
        self.prefix_active && ui.show_status_bar && !self.screen_fits(ui.hide_status_bar_below)
    }

    /// The layout in use: a line over the view, as the bar on a row of its own, else
    /// `[ui.status_bar] layout` if the borders it needs are drawn
    pub fn status_bar_layout(&self) -> StatusBarLayout {
        if self.status_bar_over_view() {
            return StatusBarLayout::Line;
        }
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
                    self.search_bar_row() as u16,
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
        } else if self.is_search_active() {
            // before copy mode: a search opened from it takes the keys (MAX-215)
            InputContext::Search
        } else if self.is_copy_mode_active() {
            InputContext::CopyMode
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
                // refused while a protocol transfer owns the link: not echoed (MAX-162)
                if self.settings.local_echo && self.settings.mode == Mode::Line && !self.link_busy()
                {
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    self.line_mode.push_history(text);
                }
                UpdateEffect::Transmit(bytes)
            }
            Action::LineInput(action) => {
                // while a protocol transfer owns the link, a line isn't sent: it stays in the
                // input, not echoed (MAX-162)
                let sends = match &action {
                    LineInputAction::Submit => true,
                    LineInputAction::Paste(text) => text.contains(['\r', '\n']),
                    _ => false,
                };
                if sends && self.link_busy() {
                    self.say_link_busy();
                    return UpdateEffect::None;
                }
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
                if is_interrupt && matches!(effect, UpdateEffect::Transmit(_)) && !self.link_busy()
                {
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
                    let response = overlay.handle_action(action, &self.settings);
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
                            self.start_capture(&path, stream, include_session);
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
                        OverlayResponse::JumpMark(letter) => {
                            self.update_mark(MarkAction::Jump(letter));
                            UpdateEffect::None
                        }
                        // the menu's "Add mark": on the line clicked, else where a mark goes
                        OverlayResponse::AddMark(line) => {
                            self.update_mark(match line {
                                Some(line) => MarkAction::AddAt(line),
                                None => MarkAction::Add(None),
                            });
                            UpdateEffect::None
                        }
                        OverlayResponse::DeleteMark(letter) => {
                            self.overlay = Some(overlay);
                            self.update_mark(MarkAction::Delete(letter));
                            UpdateEffect::None
                        }
                        OverlayResponse::ToggleTrigger(name) => {
                            self.overlay = Some(overlay);
                            self.toggle_triggers(name.as_deref());
                            UpdateEffect::None
                        }
                        OverlayResponse::TestLinks => {
                            self.overlay = Some(overlay);
                            self.test_links();
                            UpdateEffect::None
                        }
                        OverlayResponse::ArmTrigger { name, on } => {
                            self.overlay = Some(overlay);
                            let armed = (self.triggers.iter())
                                .any(|trigger| trigger.name == name && trigger.is_armed());
                            if armed != on {
                                self.arm_triggers(on, Some(&name));
                            }
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
                        OverlayResponse::ReplayGroup(sessions) => {
                            UpdateEffect::ReplayGroup(sessions)
                        }
                        OverlayResponse::StopReplay => UpdateEffect::StopReplay,
                        // the directory stays until the port opens, or says why it doesn't
                        OverlayResponse::Open(address) => {
                            self.overlay = Some(overlay);
                            UpdateEffect::Open(address)
                        }
                        OverlayResponse::OpenLink(config) => {
                            self.overlay = Some(overlay);
                            UpdateEffect::OpenLink(config)
                        }
                        OverlayResponse::NameRunningSession(name) => {
                            if let Err(err) = self.name_session(&name) {
                                self.notify_warn(format!("Session not named: {err}"));
                            }
                            if let Overlay::Sessions(browser) = &mut overlay {
                                browser.reload();
                            }
                            self.overlay = Some(overlay);
                            UpdateEffect::None
                        }
                        OverlayResponse::NameLink(address) => {
                            self.overlay = Some(overlay);
                            self.name_link(address);
                            UpdateEffect::None
                        }
                        OverlayResponse::SaveLink { link, replaces } => {
                            self.overlay = Some(overlay);
                            self.save_link(link, replaces);
                            UpdateEffect::None
                        }
                        OverlayResponse::NewLink => {
                            self.open_link_form(None);
                            UpdateEffect::None
                        }
                        OverlayResponse::EditLink(link) => {
                            self.open_link_form(Some(link));
                            UpdateEffect::None
                        }
                        OverlayResponse::AskDeleteLink(link) => {
                            self.open_overlay(Overlay::DeleteLink(link));
                            UpdateEffect::None
                        }
                        OverlayResponse::DeleteLink(link) => {
                            self.delete_link(&link);
                            UpdateEffect::None
                        }
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
            Action::Redraw | Action::Ignore => UpdateEffect::None,
        }
    }

    /// Live RX, a replay's too. The plot reads it as it comes, before the restore question
    /// holds it back (MAX-222)
    pub fn feed_rx(&mut self, chunk: &[u8]) {
        self.feed_plot(chunk);
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

    /// The saved links, at start and on a reload: a file that doesn't load is said, the app
    /// running without them (MAX-185)
    pub fn set_links(&mut self, links: Result<LinksConfig, String>) {
        if let Err(err) = &links {
            self.notify_warn(err.clone());
        }
        self.links = links;
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
                // a silent one leaves it to its command line (`notify`, MAX-228)
                if !trigger.silent {
                    self.notify_info(notice);
                }
                self.queue_commands(invocations);
            }
        }
    }

    /// Feeds both modes. The remote's queries are answered when `live` (not held while the
    /// restore dialog asked) and in terminal mode only (MAX-151)
    fn feed(&mut self, chunk: &[u8], live: bool) {
        self.follow_device();
        let settings = &self.settings;
        let timestamp_format = settings
            .capture
            .timestamp
            .then_some(settings.timestamp_format.as_str());
        self.recorder
            .record_rx(settings.add_lf, settings.encoding, timestamp_format, chunk);

        let feed_line = settings.dual_mode_buffering == DualBufferMode::AlwaysBoth
            || settings.mode == Mode::Line;
        let feed_terminal = settings.dual_mode_buffering == DualBufferMode::AlwaysBoth
            || settings.mode == Mode::Terminal;

        if feed_line {
            self.line_mode
                .feed_rx(settings.add_lf, settings.encoding, chunk);
        }
        if feed_terminal {
            (self.terminal_mode).process_device_rx(
                settings.encoding,
                chunk,
                settings.add_cr,
                settings.add_lf,
            );
            let answers = self.terminal_mode.take_answers();
            if live && self.settings.mode == Mode::Terminal {
                let bytes = answer_bytes(answers, &self.settings.terminal_mode);
                self.query_answers.extend(bytes);
            }
        }
    }

    /// The replies to send to the remote's queries, received since the last call (MAX-151),
    /// with the size reports a resize queued, in terminal mode only (MAX-159)
    pub fn take_query_answers(&mut self) -> Vec<u8> {
        // a resize comes with no RX: a report queued by it waits in the parser, not for `feed`
        let answers = self.terminal_mode.take_answers();
        if self.settings.mode == Mode::Terminal {
            let bytes = answer_bytes(answers, &self.settings.terminal_mode);
            self.query_answers.extend(bytes);
        }
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
    /// colors, else text, `.txt` added to a name without one), a bare name in `screenshot.dir`
    fn save_screenshot_as(&mut self, name: &Path) {
        let format = match name.extension() {
            Some(ext) if ext == "ans" => ScreenshotFormat::Ansi,
            _ => ScreenshotFormat::Text,
        };
        let mut path = self.settings.screenshot.dir.join(name);
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
            SettingKey::AddCr => Some(self.settings.add_cr),
            SettingKey::AddLf => Some(self.settings.add_lf),
            _ => None,
        }
    }

    fn take_screenshot(&mut self, format: ScreenshotFormat) {
        if self.settings.mode != Mode::Terminal {
            self.notify_info("Screenshots are in terminal mode");
            return;
        }
        let shot = self.terminal_mode.screenshot();
        let stem = self.settings.screenshot.stem();
        let dialog = ScreenshotDialog::new(format, &stem.to_string_lossy(), shot);
        self.open_overlay(Overlay::Screenshot(dialog));
    }

    /// Returns true when something on screen changed with time alone (needs a redraw)
    pub fn tick(&mut self) -> bool {
        self.follow_device();
        self.recorder.flush_if_due();

        let toasts_expired = self.notifications.tick();
        let now = Instant::now();
        let drag_scrolled =
            self.drag_scroll(now) || self.scrollbar_repeat(now) || self.click_repeat(now);
        let tested = self.poll_link_test();
        // a flash is redrawn until it's over, the last frame without it
        let flashing = self.terminal_mode.has_pending_flash() || self.mark_flash.is_some();
        toasts_expired || flashing || drag_scrolled || tested
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

    /// The left button is on the raw side panel's border, from its press to its release
    /// (MAX-189)
    pub fn is_side_handle_held(&self) -> bool {
        self.side_handle_held
    }

    /// The left button on the raw side panel's border, as the last frame drew it (MAX-189): a
    /// drag puts the border under the pointer, in the config's range
    fn side_handle(&mut self, action: SideHandleAction) {
        match action {
            SideHandleAction::Press => self.side_handle_held = self.frame_map.side_handle.is_some(),
            SideHandleAction::Drag(x, y) => {
                if let Some(handle) = self.frame_map.side_handle {
                    self.settings.raw_side.split_percent = handle
                        .percent_at(x, y)
                        .clamp(SPLIT_PERCENT_MIN, SPLIT_PERCENT_MAX);
                }
            }
            SideHandleAction::Release => self.side_handle_held = false,
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
                let press_came_first =
                    (self.unfocused_press_at.take()).is_some_and(|at| at.elapsed() < FOCUS_CLICK);
                self.focused_at =
                    (is_focused && !self.is_window_focused && !press_came_first).then(Instant::now);
                self.is_window_focused = is_focused;
                UpdateEffect::None
            }
            AppAction::ToggleSetting(setting) => {
                self.toggle_setting(setting);
                UpdateEffect::None
            }
            AppAction::ToggleRecording => {
                self.toggle_recording();
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
            AppAction::Notify(level, text) => {
                self.notify(level, text);
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
                // a protocol waits for the other side, which the user may start after
                let protocol = transfer.protocol().map_or("", Protocol::name);
                let message = match transfer.folder() {
                    // XMODEM's file is named here, YMODEM's come with their names
                    Some(folder) => {
                        let name = transfer.name();
                        let name = if name.is_empty() {
                            name
                        } else {
                            format!("{name} ")
                        };
                        format!(
                            "Receiving {name}with {protocol} into {}: waiting for the sender",
                            folder.display()
                        )
                    }
                    None if transfer.protocol().is_some() => format!(
                        "Sending {} ({} bytes) with {protocol}: waiting for the receiver",
                        transfer.name(),
                        transfer.size().unwrap_or_default()
                    ),
                    None => format!(
                        "Sending {} ({} bytes)",
                        transfer.name(),
                        transfer.size().unwrap_or_default()
                    ),
                };
                // the progress row says it already (MAX-228)
                if !self.settings.ui.transfer_row {
                    self.notify_transfer(ToastLevel::Info, message);
                }
                self.transfer = Some(*transfer);
                self.busy_said = false;
                self.recancel_until = None;
                UpdateEffect::None
            }
            AppAction::Reconfigure(line) => UpdateEffect::Reconfigure(line),
            AppAction::PauseCommands(duration) => {
                self.pause_commands(Instant::now() + duration);
                UpdateEffect::None
            }
            AppAction::LineControl(control) => UpdateEffect::LineControl(control),
            AppAction::Open(port) => UpdateEffect::Open(port),
            AppAction::OpenLink(config) => UpdateEffect::OpenLink(config),
            // `link-save` and `link-delete` (MAX-185): a refusal stops the command line
            AppAction::SaveLink(link) => {
                if let Err(err) = self.store_link(&link, None) {
                    self.fail_commands(format!("link-save: {err}"));
                }
                UpdateEffect::None
            }
            AppAction::DeleteLink(link) => {
                if let Err(err) = self.remove_link(&link) {
                    self.fail_commands(format!("link-delete: {err}"));
                }
                UpdateEffect::None
            }
            AppAction::NameSession(name) => {
                if let Err(err) = self.name_session(&name) {
                    self.fail_commands(format!("session-name: {err}"));
                }
                UpdateEffect::None
            }
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
            AppAction::ShowPlot(name) => {
                self.show_plot(name.as_deref());
                UpdateEffect::None
            }
            AppAction::TogglePlotFreeze => {
                self.toggle_plot_freeze();
                UpdateEffect::None
            }
            AppAction::StepPlot(forward) => {
                self.step_plot(forward);
                UpdateEffect::None
            }
            AppAction::DismissToast(id) => {
                self.notifications.dismiss(id);
                UpdateEffect::None
            }
            AppAction::DismissToasts => {
                self.notifications.dismiss_all();
                UpdateEffect::None
            }
            AppAction::OpenMarks => {
                let rows = match self.settings.mode {
                    Mode::Line => self.line_mode.mark_rows(),
                    Mode::Terminal => self.terminal_mode.mark_rows(),
                };
                self.overlay = Some(Overlay::Marks(MarkBrowser::new(rows)));
                UpdateEffect::None
            }
            AppAction::Mark(action) => {
                self.update_mark(action);
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
            AppAction::ToggleAttach if self.replaced_link().is_some() => UpdateEffect::StopReplay,
            AppAction::ToggleAttach => UpdateEffect::ToggleAttach,
            AppAction::ReloadConfig => UpdateEffect::ReloadConfig,
            AppAction::SideView(action) => self.update_side_view(action),
            AppAction::ResizeSidePanel(by) => {
                self.resize_side_panel(by);
                UpdateEffect::None
            }
            // the menu's selection follows the pointer, the other parts are only drawn
            // highlighted (`ui::render`)
            AppAction::Scrollbar(action) => {
                self.scrollbar(action, Instant::now());
                UpdateEffect::None
            }
            AppAction::SideHandle(action) => {
                self.side_handle(action);
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

    /// The side panel's share of the split, by `by` percent, kept in the config's range
    /// (MAX-189): the raw panel's or the plot's (MAX-222)
    fn resize_side_panel(&mut self, by: i16) {
        if self.settings.raw_side.view == RawSideView::Off && self.plot.is_none() {
            self.notify_warn("SideView is Off");
        }
        let percent = self.settings.raw_side.split_percent as i16 + by;
        self.settings.raw_side.split_percent =
            (percent.max(0) as u16).clamp(SPLIT_PERCENT_MIN, SPLIT_PERCENT_MAX);
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

    /// The copy action applied by the mode's copy state: the text a yank gives
    fn copy_action(&mut self, action: CopyModeAction) -> Option<String> {
        match self.settings.mode {
            Mode::Terminal => self
                .terminal_mode
                .handle_copy_action(action, &self.settings),
            Mode::Line => self.line_mode.handle_copy_action(action, &self.settings),
        }
    }

    /// A press that only gives the window focus: one while it's unfocused (the terminal sent
    /// the click first), or the first one just after it gained focus (the focus first)
    fn is_focus_click(&mut self) -> bool {
        if !self.is_window_focused {
            self.unfocused_press_at = Some(Instant::now());
            return true;
        }
        (self.focused_at.take()).is_some_and(|at| at.elapsed() < FOCUS_CLICK)
    }

    fn update_action_copy_mode(&mut self, action: CopyModeAction) -> UpdateEffect {
        // the click that focuses the window doesn't select, nor does its drag
        if matches!(action, CopyModeAction::MousePress(..)) && self.is_focus_click() {
            self.pending_press = None;
            return UpdateEffect::None;
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
                // in terminal mode a plain click only focuses: copy mode waits for a drag
                self.pending_press = (self.settings.mode == Mode::Terminal
                    && unit == MouseUnit::Char
                    && self.copy_side().is_none())
                .then_some((pos, side));
                if self.pending_press.is_some() {
                    return UpdateEffect::None;
                }
                CopyModeAction::MousePress(pos, side, unit)
            }
            CopyModeAction::MouseDrag(_, at) => {
                // the press waiting for a drag enters copy mode where it was
                if let Some((pos, side)) = self.pending_press.take() {
                    if self.is_search_active() {
                        self.close_search_for_click();
                    }
                    self.copy_action(CopyModeAction::MousePress(pos, side, MouseUnit::Char));
                }
                self.clicks.reset();
                self.drag_hold = Some(at);
                action
            }
            CopyModeAction::MouseRelease => {
                self.drag_hold = None;
                if self.pending_press.take().is_some() {
                    return UpdateEffect::None;
                }
                action
            }
            action => action,
        };
        // a click in the history leaves the search for copy mode there (MAX-113)
        if matches!(action, CopyModeAction::MousePress(..)) && self.is_search_active() {
            self.close_search_for_click();
        }
        match self.copy_action(action) {
            Some(text) => {
                self.paste_buffer = Some(text.clone());
                UpdateEffect::CopyToClipboard(text)
            }
            None => UpdateEffect::None,
        }
    }

    fn update_action_search_mode(&mut self, action: SearchModeAction) -> UpdateEffect {
        match action {
            SearchModeAction::Repeat(opposite) => {
                self.repeat_search(opposite);
                return UpdateEffect::None;
            }
            SearchModeAction::History(up) => {
                self.walk_search_history(up);
                return UpdateEffect::None;
            }
            SearchModeAction::Word(direction) => {
                self.search_word(direction);
                return UpdateEffect::None;
            }
            // a search always runs in copy mode (MAX-215): from out of it, copy mode starts
            // at the view, its cursor at the end of its line so that `?` from the live view
            // finds the newest line too, and Esc leaves it for the live view
            SearchModeAction::Start(_) | SearchModeAction::Query(_)
                if !self.is_copy_mode_active() =>
            {
                self.copy_action(CopyModeAction::Enter);
                self.copy_action(CopyModeAction::EndOfLine);
                self.search_mut().exits_copy = true;
            }
            _ => {}
        }
        let leaves_copy = matches!(action, SearchModeAction::Cancel)
            && std::mem::take(&mut self.search_mut().exits_copy);
        let confirmed = matches!(action, SearchModeAction::Confirm);
        // where a search from copy mode started, to tell a match around the history's end;
        // from the live view it goes to the oldest or newest match on purpose (MAX-218)
        let started_at = if confirmed {
            let search = self.search_mut();
            let from_live = std::mem::take(&mut search.exits_copy);
            search.origin.filter(|_| !from_live)
        } else {
            None
        };
        let view = SearchView::from(&self.settings);
        match self.settings.mode {
            Mode::Terminal => self.terminal_mode.handle_search_action(action, view),
            Mode::Line => self.line_mode.handle_search_action(action, view),
        }
        if leaves_copy {
            self.copy_action(CopyModeAction::Exit);
            self.update_action_history(HistoryAction::ScrollToBottom);
        }
        if confirmed {
            let query = self.search_mut().query_str().to_string();
            self.push_search_history(query);
            let search = self.search_mut();
            let forward = search.direction == SearchDirection::Forward;
            if started_at.is_some_and(|origin| search.went_around(origin)) {
                self.notify_search_went_around(forward);
            }
        }

        UpdateEffect::None
    }

    /// A search Enter ran, unless it is the last one; the oldest dropped past
    /// SEARCH_HISTORY_SIZE (MAX-218)
    fn push_search_history(&mut self, query: String) {
        if query.is_empty() || self.search_history.back() == Some(&query) {
            return;
        }
        self.search_history.push_back(query);
        while self.search_history.len() > SEARCH_HISTORY_SIZE {
            self.search_history.pop_front();
        }
    }

    /// Up / Down in the bar: an older / newer search in place of the query, its matches found
    fn walk_search_history(&mut self, up: bool) {
        let view = SearchView::from(&self.settings);
        let history = &self.search_history;
        match self.settings.mode {
            Mode::Terminal => {
                if self.terminal_mode.search_mut().walk_history(history, up) {
                    self.terminal_mode.update_search_matches(view);
                }
            }
            Mode::Line => {
                if self.line_mode.search_mut().walk_history(history, up) {
                    self.line_mode.update_search_matches(view);
                }
            }
        }
    }

    /// `*` in copy mode (MAX-218): the word under the cursor searched as if typed, from its
    /// first char so that the next match is another one, and kept as after Enter
    fn search_word(&mut self, direction: SearchDirection) {
        let (cursor, text) = match self.settings.mode {
            Mode::Terminal => (
                self.terminal_mode.copy_state().cursor,
                self.terminal_mode.copy_line_text(),
            ),
            Mode::Line => {
                let display_mode = self.settings.line_mode.display_mode;
                (
                    self.line_mode.copy_state().cursor,
                    self.line_mode.copy_line_text(display_mode),
                )
            }
        };
        let Some((start, word)) = word_at(&text, cursor.char_idx) else {
            self.notify_warn("No word under the cursor");
            return;
        };
        let copy_state = match self.settings.mode {
            Mode::Terminal => self.terminal_mode.copy_state_mut(),
            Mode::Line => self.line_mode.copy_state_mut(),
        };
        copy_state.jump_to(cursor.line_idx, start);
        self.search_mut().mode_kind = SearchModeKind::Text;
        self.update_action_search_mode(SearchModeAction::Start(direction));
        self.update_action_search_mode(SearchModeAction::Paste(word));
        self.update_action_search_mode(SearchModeAction::Confirm);
    }

    /// A click in the history closes the search for copy mode there: copy mode stays, also
    /// when the search started it (MAX-215)
    fn close_search_for_click(&mut self) {
        self.search_mut().exits_copy = false;
        self.update_action_search_mode(SearchModeAction::Cancel);
    }

    /// The current mode's search
    pub fn search(&self) -> &SearchState {
        match self.settings.mode {
            Mode::Terminal => self.terminal_mode.search(),
            Mode::Line => self.line_mode.search(),
        }
    }

    fn search_mut(&mut self) -> &mut SearchState {
        match self.settings.mode {
            Mode::Terminal => self.terminal_mode.search_mut(),
            Mode::Line => self.line_mode.search_mut(),
        }
    }

    /// `n` / `N` in copy mode (MAX-215), with a toast when it goes around the history's end
    fn repeat_search(&mut self, opposite: bool) {
        let wrapped = match self.settings.mode {
            Mode::Terminal => self.terminal_mode.repeat_search(opposite),
            Mode::Line => self.line_mode.repeat_search(opposite),
        };
        let forward = (self.search().direction == SearchDirection::Forward) != opposite;
        match wrapped {
            None => self.notify_warn("No match to go to"),
            Some(true) => self.notify_search_went_around(forward),
            Some(false) => {}
        }
    }

    fn notify_search_went_around(&mut self, forward: bool) {
        if forward {
            self.notify_info("Search hit the end, went on from the top");
        } else {
            self.notify_info("Search hit the top, went on from the end");
        }
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
            (Mode::Line, HistoryAction::ClearView) => self.line_mode.clear_view(),
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
            (Mode::Terminal, HistoryAction::ClearView) => self.terminal_mode.clear_view(),
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

        let (sessions, new_sessions) = (&old.sessions, &mut settings.sessions);
        keep("sessions.enabled", sessions.enabled != new_sessions.enabled);
        keep("sessions.keep", sessions.keep != new_sessions.keep);
        keep(
            "sessions.max_size",
            sessions.max_size != new_sessions.max_size,
        );
        new_sessions.enabled = sessions.enabled;
        new_sessions.keep = sessions.keep;
        new_sessions.max_size = sessions.max_size;

        settings.refresh_timestamp_width();
        self.settings = settings;
        self.fit_mode_to_link();
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
                if self.settings().mode == Mode::Terminal {
                    self.notify_warn("Line wrap: line mode only");
                }
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
            SettingKey::PrefixPlace => {
                self.settings.line_mode.prefix_place = match self.settings.line_mode.prefix_place {
                    PrefixPlace::Inset => PrefixPlace::Border,
                    PrefixPlace::Border => PrefixPlace::None,
                    PrefixPlace::None => PrefixPlace::Inset,
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
            // the bytes received from now on, what's shown stays as it came (MAX-181)
            SettingKey::AddCr => {
                self.settings.add_cr = !self.settings.add_cr;
                let state = if self.settings.add_cr { "ON" } else { "OFF" };
                self.notify_info(format!("Add CR {state}"));
            }
            SettingKey::AddLf => {
                self.settings.add_lf = !self.settings.add_lf;
                let state = if self.settings.add_lf { "ON" } else { "OFF" };
                self.notify_info(format!("Add LF {state}"));
            }
            // what Enter sends in line mode (MAX-184)
            SettingKey::TxEol => {
                self.settings.line_mode.tx_eol.toggle();
                let eol = self.settings.line_mode.tx_eol.as_str();
                self.notify_info(format!("Line Enter sends {eol}"));
            }
            // the next plot of plots.toml, off after the last, as `plot` alone (MAX-222)
            SettingKey::Plot => self.step_plot(true),
            SettingKey::DisplayRaw => {
                self.settings.raw_side.view.toggle();
                self.notify_info(format!(
                    "Raw SideView is {}",
                    self.settings.raw_side.view.as_str()
                ));
            }
        }
    }

    #[inline]
    pub fn settings(&self) -> &SettingsConfig {
        &self.settings
    }

    /// The settings, for the other modules' tests to change (MAX-168: the loop no longer sets
    /// the mode)
    #[cfg(test)]
    pub fn settings_mut(&mut self) -> &mut SettingsConfig {
        &mut self.settings
    }

    pub fn is_prefix_active(&self) -> bool {
        self.prefix_active
    }

    /// The marks of the current mode's view (MAX-164)
    pub fn marks(&self) -> &marks::Marks {
        match self.settings.mode {
            Mode::Line => self.line_mode.marks(),
            Mode::Terminal => self.terminal_mode.marks(),
        }
    }

    #[inline]
    pub fn pending_mark(&self) -> Option<PendingMark> {
        self.pending_mark
    }

    /// The marks of the current mode's view (MAX-164); a toast says what happened
    fn update_mark(&mut self, action: MarkAction) {
        self.pending_mark = None;
        let mode = self.settings.mode;
        match action {
            // the status bar's badge reads MARK meanwhile
            MarkAction::Await(pending) => self.pending_mark = Some(pending),
            MarkAction::Cancel => {}
            MarkAction::Add(_) | MarkAction::AddAt(_) => {
                let marks = self.marks();
                let letter = match action {
                    MarkAction::Add(letter) => letter,
                    _ => None,
                };
                let Some(letter) = letter.or_else(|| marks.first_free()) else {
                    self.notify_warn("all 26 marks are set: delete one first");
                    return;
                };
                let added = match (mode, action) {
                    (Mode::Line, MarkAction::AddAt(line)) => {
                        self.line_mode.add_mark_at(letter, line)
                    }
                    (Mode::Terminal, MarkAction::AddAt(row)) => {
                        self.terminal_mode.add_mark_at(letter, row)
                    }
                    (Mode::Line, _) => self.line_mode.add_mark(letter),
                    (Mode::Terminal, _) => self.terminal_mode.add_mark(letter),
                };
                match added {
                    Some(_) => self.notify_info(format!("mark {letter}")),
                    None => self.notify_warn("no line to mark yet"),
                }
            }
            MarkAction::Jump(letter) => {
                let jumped = match mode {
                    Mode::Line => self.line_mode.jump_to_mark(letter),
                    Mode::Terminal => self.terminal_mode.jump_to_mark(letter),
                };
                match jumped {
                    Some(line) => self.flash_mark(line),
                    None => self.notify_warn(format!("mark {letter} is not set")),
                }
            }
            MarkAction::Delete(letter) => {
                let removed = match mode {
                    Mode::Line => self.line_mode.remove_mark(letter),
                    Mode::Terminal => self.terminal_mode.remove_mark(letter),
                };
                match removed {
                    Some(_) => self.notify_info(format!("mark {letter} deleted")),
                    None => self.notify_warn(format!("mark {letter} is not set")),
                }
            }
            MarkAction::Adjacent(forward) => {
                let jumped = match mode {
                    Mode::Line => self.line_mode.jump_to_adjacent_mark(forward),
                    Mode::Terminal => self.terminal_mode.jump_to_adjacent_mark(forward),
                };
                match jumped.and_then(|letter| self.marks().get(letter)) {
                    Some(line) => self.flash_mark(line),
                    None => self.notify_info(if forward {
                        "no mark below"
                    } else {
                        "no mark above"
                    }),
                }
            }
        }
    }

    /// The line a jump landed on flashes, unless `[ui.marks] flash` is off (MAX-164)
    fn flash_mark(&mut self, line: usize) {
        if self.settings.ui.marks.flash {
            let until = Instant::now() + MARK_FLASH;
            self.mark_flash = Some((self.settings.mode, line, until));
        }
    }

    /// The line flashing in `mode`'s view after a mark jump, forgotten once its time is up
    pub fn mark_flash(&mut self, mode: Mode) -> Option<usize> {
        let (flash_mode, line, until) = self.mark_flash?;
        if Instant::now() >= until {
            self.mark_flash = None;
            return None;
        }
        (flash_mode == mode).then_some(line)
    }

    /// The session backup runs, what the optional `session` status bar condition shows
    pub fn has_session(&self) -> bool {
        self.recorder.has_session()
    }

    /// What the recording running saves, if one runs: what `REC` shows
    pub fn recording(&self) -> Option<Stream> {
        self.recorder.recording()
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

    /// Whether to draw the window as focused: always, with `focus_effects` off (MAX-204). The
    /// click that focuses the window is told by `is_window_focused`, whatever the setting
    #[inline]
    pub fn is_app_focused(&self) -> bool {
        self.is_window_focused || !self.settings.ui.focus_effects
    }

    /// The search bar open, or kept in copy mode with its matches (MAX-215)
    pub fn is_search_bar_shown(&self) -> bool {
        let search = self.search();
        search.active || search.shows_matches()
    }

    /// The search bar on a row of its own under the view, taken from it (MAX-218): it goes
    /// right under the history, in its bottom border when it has one free of the status bar.
    /// Without borders it would cover the view's last row, the cursor or a match on it; in
    /// terminal mode the `border` status bar has the history's border. The transfer row takes
    /// the history's last row: the search bar's row is over it (MAX-162)
    pub fn search_bar_row(&self) -> bool {
        let status_in_history_border = self.settings.mode == Mode::Terminal
            && self.show_status_bar()
            && self.status_bar_layout() == StatusBarLayout::Border;
        self.is_search_bar_shown()
            && (!self.show_borders() || status_in_history_border || self.show_transfer_row())
    }

    #[inline]
    pub fn is_copy_mode_active(&self) -> bool {
        match self.settings.mode {
            Mode::Line => self.line_mode.copy_state().active,
            Mode::Terminal => self.terminal_mode.copy_state().active,
        }
    }

    /// The left button pressed in the history or the raw panel, not released yet (MAX-191)
    pub fn is_mouse_selecting(&self) -> bool {
        let copy_state = match self.settings.mode {
            Mode::Line => self.line_mode.copy_state(),
            Mode::Terminal => self.terminal_mode.copy_state(),
        };
        copy_state.active && copy_state.mouse_drag.is_some()
    }

    /// The side of a terminal press waiting for a drag to enter copy mode
    pub fn pending_press_side(&self) -> Option<CopySide> {
        self.pending_press.map(|(_, side)| side)
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
        self.search().active
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
        self.notify(ToastLevel::Info, msg);
    }

    pub fn notify_success(&mut self, msg: impl Into<String>) {
        self.notify(ToastLevel::Success, msg);
    }

    pub fn notify_warn(&mut self, msg: impl Into<String>) {
        self.notify(ToastLevel::Warn, msg);
    }

    pub fn notify_error(&mut self, msg: impl Into<String>) {
        self.notify(ToastLevel::Error, msg);
    }

    fn notify(&mut self, level: ToastLevel, msg: impl Into<String>) {
        let msg = msg.into();
        let duration = toast_duration(level, &msg);
        self.notifications.push(msg, level, duration);
    }

    /// A file transfer's toast: its messages follow each other in one box (MAX-228)
    pub fn notify_transfer(&mut self, level: ToastLevel, msg: impl Into<String>) {
        let msg = msg.into();
        let duration = toast_duration(level, &msg);
        self.notifications
            .push_keyed("transfer", msg, level, duration);
    }

    /// The link's toast, the same way: connecting then connected, lost then reconnected,
    /// detached then attached (MAX-228)
    pub fn notify_link(&mut self, level: ToastLevel, msg: impl Into<String>) {
        let msg = msg.into();
        let duration = toast_duration(level, &msg);
        self.notifications.push_keyed("link", msg, level, duration);
    }
}

/// How long a toast stays: the time to read it (MAX-228), 2 s to notice it then 50 ms a
/// character (20 a second), so "Copied" goes in about 2 s and a line with a path stays twice
/// that. A warning stays at least 4 s and an error 6 s, however short; none more than 10 s
fn toast_duration(level: ToastLevel, message: &str) -> Duration {
    let read = 2_000 + 50 * message.chars().count() as u64;
    let floor = match level {
        ToastLevel::Info | ToastLevel::Success => 0,
        ToastLevel::Warn => 4_000,
        ToastLevel::Error => 6_000,
    };
    Duration::from_millis(read.max(floor).min(10_000))
}

/// How long after a cancel the other side's ACK or NAK gets the CANs again: lrzsz's `rb`
/// drops what it read past a good block, so CANs that came with the block's end are lost to
/// it, and only its next ACK can be answered with them (MAX-162)
const RECANCEL_FOR: Duration = Duration::from_secs(5);

/// A path's file name, for a short message; the path whole when it has none
fn file_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into(),
    )
}

/// What a cancelled transfer says: how far a send got, the partial file a download keeps
fn cancelled_text(transfer: &FileTransfer) -> String {
    if !transfer.is_download() {
        return format!("Cancelled: {} at {}%", transfer.name(), transfer.percent());
    }
    match transfer.partial() {
        Some(part) => format!("Cancelled: {}, kept as {}", transfer.name(), part.display()),
        None => "Cancelled: the download".to_string(),
    }
}

/// What a transfer stopped by a failure says, before why: how far a send got, the partial
/// file a download keeps
fn stopped_text(transfer: &FileTransfer) -> String {
    if !transfer.is_download() {
        return format!(
            "Stopped sending {} at {}%",
            transfer.name(),
            transfer.percent()
        );
    }
    match transfer.partial() {
        Some(part) => format!(
            "Stopped receiving {}, kept as {}",
            transfer.name(),
            part.display()
        ),
        None => "Stopped receiving".to_string(),
    }
}

#[cfg(test)]
#[path = "tests/app.rs"]
pub mod tests;
