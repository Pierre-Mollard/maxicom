use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crossterm::event::KeyCode;
use ratatui::layout::{Direction, Rect};

use crate::{
    app::{
        capture::GroupLoad,
        mash::{Mash, MashStep},
        plots::{Plot, plot_note},
        scroller::ScrollStatus,
        search::{
            SEARCH_HISTORY_SIZE, SearchDirection, SearchHost, SearchModeKind, SearchState,
            SearchView, word_at,
        },
        selection::{ClickCounter, CopySide, LogicalPosition, MouseUnit},
        term_answers::answer_bytes,
        terminal_mode::ShellSignal,
        triggers::Trigger,
        types::{FocusTarget, FrameAreas, FrameMap, ScrollbarMap, SidePanel, ViewportSize},
        wait_for::TextWatch,
    },
    capture::{
        CaptureDevice, CaptureLine, Recorder, RecordingWriter, ScreenshotFormat, Session, Stream,
    },
    commands::{
        Access, Origin,
        jobs::{Caller, JobId, Jobs, Wait},
        query,
        report::{Note, Report},
        value::Value,
    },
    configs::{
        AppConfig, DualBufferMode, KeyBindingAction, KeyResolver, LinksConfig, Mode, PlotsConfig,
        QueryGroup, RawSideView, SettingsConfig, StatusBarLayout, TerminalDimensionMode,
        TextEncoding, WrapMode,
        settings::{
            PrefixPlace, PrefixWrapBehavior, RawSideOrientation, SPLIT_PERCENT_MAX,
            SPLIT_PERCENT_MIN, on_off,
        },
    },
    event::{Cause, Event, EventLog, SettingValue},
    inputs::{
        Action, AppAction, ClickHoldAction, CopyModeAction, HistoryAction, LineInputAction,
        MarkAction, PendingMark, ScrollbarAction, ScrollbarId, SearchModeAction, SideHandleAction,
        SideViewAction, TerminalAction,
    },
    link::{
        Address, LinkConfig, LinkKind, LinkState,
        replay::Replay,
        serial::{LineControl, LineLevel, LineSettings},
    },
    notification::{Notice, NotificationManager, ToastLevel},
    overlays::{
        MarkBrowser, MarkRow, NoticeBrowser, Overlay, OverlayResponse, PopupTarget,
        ScreenshotDialog, SettingKey,
    },
    paths::{create_parent_dirs, expand_home},
    rx_stream::RxStream,
    transfer::{FileTransfer, Step, modem::Protocol},
    ui::{layout::side_chunks, raw_side::terminal::DumpLayout},
};

/// What brought the shell hint in the line input (MAX-255); a terminal query received says
/// it as an event (`Event::ShellSeen`)
enum ShellHint {
    /// Up or Down in the line input, after a sign of a shell
    History(ShellSignal),
    /// Tab in the line input
    Tab,
}

/// How soon after the window gains focus a press is the one that focused it
const FOCUS_CLICK: Duration = Duration::from_millis(200);

pub mod capture;
pub mod data_processing;
pub mod device;
pub mod jobs;
pub mod line_mode;
pub mod marks;
pub mod mash;
pub mod plots;
pub mod scroller;
pub mod search;
pub mod selection;
pub mod term_answers;
pub mod terminal_mode;
pub mod triggers;
pub mod types;
pub mod wait_for;

pub use line_mode::LineMode;
pub use terminal_mode::{TerminalMode, UntracedYank};

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

/// A scrollbar's scroll: a row up or down, or to a position from the top
#[derive(Debug, Clone, Copy)]
enum BarScroll {
    Up,
    Down,
    To(usize),
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
    /// Every event emitted, for the API's subscribers (MAX-284)
    pub events: EventLog,
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
    scrollbar_hold: Option<(ScrollbarId, ScrollbarHold)>,
    side_handle_held: Option<SidePanel>,
    /// The dump was sized to fit its bytes on its first showing (`fit_dump_once`)
    dump_fitted: bool,
    /// When a held scrollbar arrow scrolls again
    scrollbar_repeat_at: Option<Instant>,
    /// The action and cells of the click target the left button holds (MAX-143), and when it
    /// runs again
    click_hold: Option<(KeyBindingAction, ratatui::layout::Rect)>,
    click_repeat_at: Option<Instant>,
    /// The chains running, each a job with who started it and what it waits for (MAX-12,
    /// MAX-165)
    jobs: Jobs,
    /// The clients' commands held for the user's answer, the first one asked (MAX-287)
    asks: VecDeque<jobs::Ask>,
    /// The jobs whose held command the user let run, once (MAX-287)
    allowed_once: Vec<JobId>,
    /// The clients' jobs running, each with what it did so far (MAX-284)
    runs: HashMap<JobId, jobs::Outcome>,
    /// The clients' jobs ended, for the API to reply (MAX-284)
    finished: Vec<(JobId, jobs::Outcome)>,
    /// What the user let a client do without asking again, by its connection, until it
    /// closes (MAX-287)
    allowed_always: Vec<(u64, Access)>,
    /// The command lines that ran, the oldest first (MAX-12)
    command_history: VecDeque<String>,
    /// The searches Enter ran, oldest first, for Up / Down in the bar (MAX-218)
    search_history: VecDeque<String>,
    /// The bytes received, whatever the modes fed: the terminal raw panel's and the API's
    /// (MAX-290)
    pub rx_bytes: RxStream,
    pub line_mode: LineMode,
    pub terminal_mode: TerminalMode,
    recorder: Recorder,
    /// What is received while the restore dialog asks, fed after the answer (MAX-148)
    held_rx: Option<Vec<u8>>,
    /// `--restore` / `--no-restore`, for the start and each device switched to (MAX-187)
    restore_flag: Option<bool>,
    /// The replies to the remote's queries, for the loop to send (MAX-151)
    query_answers: Vec<u8>,
    /// `answer_groups` put aside while the answers are switched off for the session (the
    /// settings popup's `A`, `set answer-queries`), given back when switched on
    answer_groups_off: Option<Vec<QueryGroup>>,
    /// The device's last OSC 52 copy in base64, for the loop to send on (MAX-275)
    device_copy: Option<Vec<u8>>,
    /// The device was said to look like a shell: once a link, until a switch (MAX-255)
    shell_hinted: bool,
    /// What the emulator didn't use, by label with its count in first seen order, while its
    /// notification is shown (MAX-264)
    unhandled: Vec<(String, usize)>,
    /// The rules of `triggers.toml` (MAX-6)
    triggers: Vec<Trigger>,
    /// The links of `links.toml`, or why it didn't load, and this machine's name, which picks
    /// the ones for it (MAX-185)
    links: Result<LinksConfig, String>,
    host: Option<String>,
    /// The profile stack laid over the config, the one of the link being opened, and the one
    /// for the loop to lay once that link opened, when it isn't the same (MAX-240)
    profiles: Vec<String>,
    opening_profiles: Option<Vec<String>>,
    profiles_due: Option<Vec<String>>,
    /// The stack's `on_open` line, the later profile's (MAX-9), and whether a link opened
    /// since, for the loop to run it once the stack is laid
    on_open: Option<String>,
    on_open_due: bool,
    /// The `on_load` lines of the profiles just laid, for the loop to run (MAX-247)
    on_load_due: Vec<String>,
    /// Where the profiles are: next to the config file, another folder in the tests
    profiles_dir: PathBuf,
    /// The text `mash` sends until another comes in (MAX-6)
    mash: Option<Mash>,
    /// The texts the jobs' `wait-for`s wait for (MAX-219), and the next watch's number
    text_watches: Vec<TextWatch>,
    next_watch: u64,
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

/// The live link a replay replaced (MAX-166): its settings, its profile stack (MAX-240), the
/// mode, the encoding, the bytes received, the
/// histories of both modes, the counts and its session backup, put back as they were when the
/// replay stops (MAX-171: the backup kept here only, so it can't stay behind)
struct ReplacedLink {
    link_config: LinkConfig,
    profiles: Vec<String>,
    mode: Mode,
    encoding: TextEncoding,
    rx_bytes: RxStream,
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
    /// The group being loaded, a slice a loop turn, the loading popup open (MAX-272)
    loading: Option<Box<GroupLoad>>,
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
    /// The link attached or detached, `attach` / `detach`'s (MAX-219)
    SetAttached(bool),
    ReloadConfig,
    /// The host terminal cleared and every cell written again: what it shows drifted from
    /// what maxicom drew (MAX-271)
    Redraw,
    /// Every API client dropped, their jobs cancelled already (MAX-287): the loop holds the
    /// server
    DisconnectClients,
    /// The profile stack for the loop to lay at once, `profile load` / `unload`'s (MAX-240)
    LayProfiles(Vec<String>),
    Quit,
}

impl App {
    pub fn new(
        settings: SettingsConfig,
        key_resolver: KeyResolver,
        link_config: LinkConfig,
    ) -> Self {
        let rx_bytes = rx_store(&settings);
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
            events: EventLog::default(),
            viewport: ViewportSize::default(),
            frame_areas: FrameAreas::default(),
            frame_map: FrameMap::default(),
            clicks: ClickCounter::default(),
            pending_press: None,
            drag_hold: None,
            last_drag_scroll: None,
            scrollbar_hold: None,
            side_handle_held: None,
            dump_fitted: false,
            scrollbar_repeat_at: None,
            click_hold: None,
            click_repeat_at: None,
            jobs: Jobs::default(),
            asks: VecDeque::new(),
            allowed_once: Vec::new(),
            runs: HashMap::new(),
            finished: Vec::new(),
            allowed_always: Vec::new(),
            command_history: VecDeque::new(),
            search_history: VecDeque::new(),
            rx_bytes,
            line_mode: line_view,
            terminal_mode: terminal_view,
            recorder: Recorder::default(),
            held_rx: None,
            restore_flag: None,
            query_answers: Vec::new(),
            answer_groups_off: None,
            device_copy: None,
            shell_hinted: false,
            unhandled: Vec::new(),
            triggers: Vec::new(),
            links: Ok(LinksConfig::default()),
            host: crate::configs::links::this_host(),
            profiles: Vec::new(),
            opening_profiles: None,
            profiles_due: None,
            on_open: None,
            on_open_due: false,
            on_load_due: Vec::new(),
            profiles_dir: AppConfig::profiles_dir(),
            mash: None,
            text_watches: Vec::new(),
            next_watch: 0,
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
            let name = file_name(&path);
            self.emit(Event::FileReceived { name, size });
        }
        match outcome {
            None => {}
            Some(Ok(())) => {
                let Some(transfer) = self.transfer.take() else {
                    return step;
                };
                let done = if transfer.is_download() {
                    received_event(&transfer)
                } else {
                    Event::Sent {
                        name: transfer.name(),
                        size: transfer.size().unwrap_or_default(),
                        protocol: transfer.protocol().map(Protocol::name),
                    }
                };
                self.emit(done);
                self.complete_waiting(|wait| *wait == Wait::Transfer, Report::default());
            }
            Some(Err(failure)) => self.stop_transfer(&failure.to_string()),
        }
        step
    }

    /// Ends the file going through, the link can't take it (`why`)
    pub fn stop_transfer(&mut self, why: &str) {
        if let Some(transfer) = self.transfer.take() {
            let stopped = stopped_text(&transfer);
            // the job waiting for it fails quietly: the event's toast says it
            let failed = Report::shown(format!("{stopped}: {why}"));
            let why = why.to_string();
            self.emit(Event::TransferStopped { stopped, why });
            self.complete_waiting(|wait| *wait == Wait::Transfer, failed);
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

    /// Whether the link is open: the commands that need it refuse otherwise (MAX-12)
    pub fn is_link_open(&self) -> bool {
        self.link.is_open()
    }

    /// The link's address and line settings (MAX-168)
    pub fn link_config(&self) -> &LinkConfig {
        self.link.config()
    }

    /// The marks of `mode`'s view by line (MAX-164), with their line's text: the terminal's
    /// read from the emulator's scrollback, put back after (MAX-283)
    pub fn mark_rows(&mut self, mode: Mode) -> Vec<MarkRow> {
        match mode {
            Mode::Line => self.line_mode.mark_rows(),
            Mode::Terminal => self.terminal_mode.mark_rows(),
        }
    }

    /// DTR and RTS as maxicom last set them since the open (MAX-283)
    pub fn line_levels(&self) -> (LineLevel, LineLevel) {
        self.link.levels()
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

    /// The line settings the status bar and the settings dialog show (MAX-166): a serial port's
    /// or an RFC 2217 link's, a replayed serial session's from its meta; none for a raw or
    /// telnet TCP link nor a plain file
    pub fn line_settings(&self) -> Option<CaptureLine> {
        match self.link_kind() {
            LinkKind::Replay => self.recorded_line(),
            kind if kind.has_line_settings() => Some(CaptureLine::of(&self.link.config().line)),
            _ => None,
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
        // the line being received ends with its link, not in the replay's bytes
        self.recorder.end_line(self.settings.encoding);
        self.stop_transfer("a replay took the link's place");
        self.stop_mash("a replay took the link's place");
        self.fail_text_watches("a replay took the link's place");
        let (rx_bytes, line_mode, terminal_mode, stats) = self.clear_screen();
        // a replay replacing a replay keeps the live link the first one set aside
        let replaced = match self.replay.take().and_then(|run| run.replaced) {
            Some(replaced) => Some(replaced),
            None if self.link_kind().keeps_sessions() => Some(Box::new(ReplacedLink {
                link_config: self.link.config().clone(),
                profiles: self.profiles.clone(),
                mode: self.settings.mode,
                encoding: self.settings.encoding,
                rx_bytes,
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
            loading: None,
            replaced,
        });
    }

    /// Terminal mode on a link that isn't written (a replay), as line mode is for typing to it
    /// (MAX-168): at start, on a reload and when a replay starts
    fn fit_mode_to_link(&mut self) {
        if !self.link_kind().writes() && self.settings.mode_enabled(Mode::Terminal) {
            self.settings.mode = Mode::Terminal;
        }
    }

    /// Fresh bytes received, histories of both modes and counts, the ones they replace given
    /// back: for a replay to play on (MAX-166) and for a device switched to (MAX-187)
    fn clear_screen(&mut self) -> (RxStream, LineMode, TerminalMode, Stats) {
        self.clear_plot();
        self.shell_hinted = false;
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
            std::mem::replace(&mut self.rx_bytes, rx_store(&self.settings)),
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
        self.emit(Event::ReplayFinished);
    }

    pub fn is_replay_ended(&self) -> bool {
        self.replay.as_ref().is_some_and(|run| run.ended)
    }

    /// Puts back the live link the replay replaced, for the loop to open, its profile stack
    /// for the loop to lay (MAX-240), and resumes its session backup (MAX-166). False when there is none: a replay from the command line
    pub fn stop_replay(&mut self) -> bool {
        let Some(replaced) = self.replay.as_mut().and_then(|run| run.replaced.take()) else {
            self.set_replay_ended(true);
            return false;
        };
        // the replay's line being cut ends with it, in its own encoding
        self.recorder.end_line(self.settings.encoding);
        self.replay = None;
        self.clear_plot();
        self.link.set_config(replaced.link_config);
        self.set_profiles_due(replaced.profiles);
        // a reload during the replay may have turned that mode off
        if self.settings.mode_enabled(replaced.mode) {
            self.settings.mode = replaced.mode;
        }
        self.settings.encoding = replaced.encoding;
        self.rx_bytes = replaced.rx_bytes;
        self.line_mode = replaced.line_mode;
        self.terminal_mode = replaced.terminal_mode;
        self.stats = replaced.stats;
        self.resume_session(replaced.session);
        true
    }

    /// A warning for the keymap's keys that never reach their mode section (MAX-155)
    pub fn warn_shadowed_keys(&mut self) {
        let shadowed = self.key_resolver.shadowed_keys();
        if !shadowed.is_empty() {
            let text = format!("Keymap: unreachable {}", shadowed.join(", "));
            self.emit(Event::ConfigWarning { text });
        }
    }

    /// Says once a link that the device looks like a shell, its history and completion in
    /// terminal mode (MAX-255), worded for what brought it. With `shell_hint`, in line mode on
    /// a written link only, never when terminal mode is turned off
    fn hint_shell(&mut self, hint: ShellHint) -> Option<Note> {
        let key = self.shell_hint_key()?;
        let msg = match hint {
            ShellHint::History(signal) => format!(
                "The device shell keeps its own history ({}): {key} switches to terminal mode \
                 to use it",
                signal.told()
            ),
            ShellHint::Tab => {
                format!("Tab completion is the device shell's: {key} switches to terminal mode")
            }
        };
        Some(Note::info(msg).in_slot("shell"))
    }

    /// The key that switches to terminal mode, when the shell hint is due: once a link, with
    /// `shell_hint`, in line mode on a written link, never when terminal mode is turned off
    fn shell_hint_key(&mut self) -> Option<String> {
        let line_mode = self.settings.mode == Mode::Line;
        if self.shell_hinted
            || !self.settings.line_mode.shell_hint
            || !line_mode
            || !self.settings.mode_enabled(Mode::Terminal)
            || !self.link_kind().writes()
        {
            return None;
        }
        self.shell_hinted = true;
        let keys = self.key_resolver.action_keys(KeyBindingAction::SKInputMode);
        let key = keys.into_iter().next();
        Some(key.unwrap_or_else(|| "toggle-input-mode".to_string()))
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

    /// An action of the user's own input, what it reports shown
    pub fn update(&mut self, action: Action) -> UpdateEffect {
        let (effect, report) = self.update_from(&Origin::Input, action);
        self.present(&Caller::input(), report);
        effect
    }

    /// An action from `origin` (MAX-165), with what it reports for the caller to present:
    /// only the user's input acts as typing, so a trigger's command neither ends a prefix the
    /// user pressed nor stops a mash
    pub fn update_from(&mut self, origin: &Origin, action: Action) -> (UpdateEffect, Report) {
        if *origin == Origin::Input {
            self.input_takes_over(&action);
        }
        let mut report = Report::default();
        let effect = match action {
            Action::App(action) => self.update_action_app(origin, action, &mut report),
            Action::TransmitRaw(bytes) => {
                // refused while a protocol transfer owns the link: not echoed (MAX-162)
                if self.settings.local_echo && !self.link_busy() {
                    match self.settings.mode {
                        Mode::Line => {
                            let text = String::from_utf8_lossy(&bytes).into_owned();
                            self.line_mode.push_history(text);
                        }
                        Mode::Terminal => self.terminal_mode.echo(&bytes),
                    }
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
                    report.notes.extend(self.say_link_busy());
                    return (UpdateEffect::None, report);
                }
                // a click in the input leaves the history for it (MAX-113), a right-click
                // paste too (MAX-144)
                if let LineInputAction::ClickAt(_) | LineInputAction::Paste(_) = action {
                    if self.is_copy_mode_active() {
                        self.update_action_copy_mode(CopyModeAction::Exit, &mut report);
                    }
                    if self.is_search_active() {
                        self.update_action_search_mode(SearchModeAction::Cancel, &mut report);
                    }
                }
                // the device's history and completion are in terminal mode: said once (MAX-255)
                match (&action, self.terminal_mode.shell_signal()) {
                    (LineInputAction::HistoryUp | LineInputAction::HistoryDown, Some(signal)) => {
                        report
                            .notes
                            .extend(self.hint_shell(ShellHint::History(signal)));
                    }
                    (LineInputAction::HandleKey(key), _)
                        if key.code == KeyCode::Tab && key.modifiers.is_empty() =>
                    {
                        report.notes.extend(self.hint_shell(ShellHint::Tab));
                    }
                    _ => {}
                }
                let is_interrupt = matches!(action, LineInputAction::Interrupt);
                let effect = self
                    .line_mode
                    .update_action_lineinput(&self.settings, action);
                if is_interrupt && matches!(effect, UpdateEffect::Transmit(_)) && !self.link_busy()
                {
                    report.notes.push(Note::info("^C sent"));
                }
                effect
            }
            Action::Terminal(TerminalAction::Screenshot(format)) => {
                report.notes.extend(self.take_screenshot(format));
                UpdateEffect::None
            }
            Action::Terminal(action) => {
                // local echo shows what is typed and pasted, not the keys sent as escape
                // sequences (arrows, F keys, Alt): they would move the cursor (MAX-270)
                let echo = self.settings.local_echo && !self.link_busy();
                let paste = match &action {
                    TerminalAction::Paste(text) if echo => Some(text.clone()),
                    _ => None,
                };
                let key = matches!(action, TerminalAction::Key(_));
                let backspace = self.settings.terminal_mode.backspace;
                let effect = self.terminal_mode.update_action_terminal(action, backspace);
                if let UpdateEffect::Transmit(bytes) = &effect {
                    if let Some(text) = paste {
                        self.terminal_mode.echo_paste(&text);
                    } else if echo && key && bytes.first() != Some(&0x1b) {
                        self.terminal_mode.echo(bytes);
                    }
                }
                effect
            }
            Action::History(action) => self.update_action_history(action),
            Action::CopyMode(action) => self.update_action_copy_mode(action, &mut report),
            Action::Search(action) => self.update_action_search_mode(action, &mut report),
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
                            // the popup shows the new value: only a warning is said (MAX-282)
                            let warning = (self.toggle_setting(origin, key))
                                .filter(|note| note.level == ToastLevel::Warn);
                            report.notes.extend(warning);
                            self.overlay = Some(overlay);
                            UpdateEffect::None
                        }
                        OverlayResponse::ResizeTerminal { rows, cols } => {
                            self.resize_terminal(rows, cols);
                            UpdateEffect::None
                        }
                        // the config changes once the port takes it (MAX-168)
                        OverlayResponse::Reconfigure(line) => UpdateEffect::Reconfigure(line),
                        OverlayResponse::Reattach => UpdateEffect::ToggleAttach,
                        OverlayResponse::Run(action) => {
                            let action = action.to_action(self);
                            let (effect, run) = self.update_from(origin, action);
                            report.extend(run);
                            effect
                        }
                        OverlayResponse::QueueCommands { line, invocations } => {
                            self.push_command_history(line);
                            report.extend(self.queue_chain(Origin::CommandLine, invocations));
                            UpdateEffect::None
                        }
                        OverlayResponse::StartCapture {
                            path,
                            stream,
                            include_session,
                        } => {
                            report.extend(self.start_capture(&path, stream, include_session));
                            UpdateEffect::None
                        }
                        OverlayResponse::HandledNotify { message, warn } => {
                            self.overlay = Some(overlay);
                            report.notes.push(match warn {
                                true => Note::warn(message),
                                false => Note::info(message),
                            });
                            UpdateEffect::None
                        }
                        OverlayResponse::JumpMark(letter) => {
                            report
                                .notes
                                .extend(self.update_mark(MarkAction::Jump(letter)));
                            UpdateEffect::None
                        }
                        OverlayResponse::ClosePanel(panel) => {
                            report.notes.extend(self.close_panel(panel));
                            UpdateEffect::None
                        }
                        // the menu's "Add mark": on the line clicked, else where a mark goes
                        OverlayResponse::AddMark(line) => {
                            let note = self.update_mark(match line {
                                Some(line) => MarkAction::AddAt(line),
                                None => MarkAction::Add(None),
                            });
                            report.notes.extend(note);
                            UpdateEffect::None
                        }
                        OverlayResponse::DeleteMark(letter) => {
                            self.overlay = Some(overlay);
                            report
                                .notes
                                .extend(self.update_mark(MarkAction::Delete(letter)));
                            UpdateEffect::None
                        }
                        OverlayResponse::ToggleTrigger(name) => {
                            self.overlay = Some(overlay);
                            report.notes.push(self.toggle_triggers(name.as_deref()));
                            UpdateEffect::None
                        }
                        // laid at once by the loop, the popup staying on the new stack
                        OverlayResponse::ToggleProfile(name) => {
                            self.overlay = Some(overlay);
                            match self.toggled_profile(&name) {
                                Ok(stack) => UpdateEffect::LayProfiles(stack),
                                Err(message) => {
                                    report.notes.push(Note::warn(message));
                                    UpdateEffect::None
                                }
                            }
                        }
                        // nothing when it's laid or off already
                        OverlayResponse::SetProfile { name, on } => {
                            self.overlay = Some(overlay);
                            let laid = self.profiles.contains(&name);
                            let toggled = (laid != on).then(|| self.toggled_profile(&name));
                            match toggled {
                                None => UpdateEffect::None,
                                Some(Ok(stack)) => UpdateEffect::LayProfiles(stack),
                                Some(Err(message)) => {
                                    report.notes.push(Note::warn(message));
                                    UpdateEffect::None
                                }
                            }
                        }
                        OverlayResponse::MoveProfile { name, up } => {
                            self.overlay = Some(overlay);
                            match self.moved_profile(&name, up) {
                                Ok(Some(stack)) => UpdateEffect::LayProfiles(stack),
                                Ok(None) => UpdateEffect::None,
                                Err(message) => {
                                    report.notes.push(Note::warn(message));
                                    UpdateEffect::None
                                }
                            }
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
                                report.notes.push(self.arm_triggers(on, Some(&name)));
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
                            report.notes.push(match self.name_session(&name) {
                                Ok(named) => named,
                                Err(err) => Note::warn(format!("Session not named: {err}")),
                            });
                            if let Overlay::Sessions(browser) = &mut overlay {
                                browser.reload();
                            }
                            self.overlay = Some(overlay);
                            UpdateEffect::None
                        }
                        OverlayResponse::NameLink(address) => {
                            self.overlay = Some(overlay);
                            report.notes.extend(self.name_link(address));
                            UpdateEffect::None
                        }
                        OverlayResponse::SaveLink { link, replaces } => {
                            self.overlay = Some(overlay);
                            report.extend(self.save_link(link, replaces));
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
                        OverlayResponse::AnswerAsk(answer) => {
                            self.answer_ask(answer);
                            UpdateEffect::None
                        }
                        OverlayResponse::AskDeleteLink(link) => {
                            self.open_overlay(Overlay::DeleteLink(link));
                            UpdateEffect::None
                        }
                        OverlayResponse::DeleteLink(link) => {
                            report.extend(self.delete_link(&link));
                            UpdateEffect::None
                        }
                        OverlayResponse::RestoreSession(yes) => {
                            if let Overlay::RestoreSession(session) = overlay {
                                self.answer_restore(&session, yes);
                            }
                            UpdateEffect::None
                        }
                        OverlayResponse::SaveScreenshot { path, contents } => {
                            report.extend(self.save_screenshot(&path, contents));
                            UpdateEffect::None
                        }
                    }
                } else {
                    UpdateEffect::None
                }
            }
            Action::Redraw | Action::Ignore => UpdateEffect::None,
        };
        (effect, report)
    }

    /// What the user's input does before its action: it ends the prefix and stops a mash
    fn input_takes_over(&mut self, action: &Action) {
        // the prefix stays after a line or pan move, the side panel's too: the key again moves
        // again (MAX-252 for the side panel)
        let is_repeatable_history = self.prefix_active
            && matches!(
                action,
                Action::History(
                    HistoryAction::ScrollUp(_)
                        | HistoryAction::ScrollDown(_)
                        | HistoryAction::PanLeft(_)
                        | HistoryAction::PanRight(_)
                ) | Action::App(AppAction::SideView(_))
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
    }

    /// Live RX, a replay's too. The plot reads it as it comes, before the restore question
    /// holds it back (MAX-222)
    pub fn feed_rx(&mut self, chunk: &[u8]) {
        self.feed_plot(chunk);
        if self.hold_rx(chunk) {
            return;
        }
        let fired = self.match_triggers(chunk);
        self.feed_marking(chunk, &fired);
        self.run_fired(&fired);
        self.watch_mash(chunk);
        self.watch_texts(chunk);
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
                let failed = Report::failed(format!("mash: '{until}' didn't come in"));
                self.complete_waiting(|wait| *wait == Wait::Mash, failed);
                None
            }
        }
    }

    /// Ends the mash before its text came in (`why` shown as reason), and the rest of its line
    pub fn stop_mash(&mut self, why: &str) {
        if self.mash.take().is_some() {
            let why = why.to_string();
            let failed = Report::failed(format!("Mash stopped: {why}"));
            self.emit(Event::MashStopped { why });
            self.complete_waiting(|wait| *wait == Wait::Mash, failed);
        }
    }

    /// The text the mash waits for came in: it ends, the rest of its line goes on
    fn watch_mash(&mut self, chunk: &[u8]) {
        if self
            .mash
            .as_mut()
            .is_some_and(|mash| mash.until_seen(chunk))
        {
            let until =
                (self.mash.take()).map_or_else(String::new, |mash| mash.until_text().to_string());
            let done = format!("Mash done: '{until}' came in");
            self.emit(Event::MashDone { until });
            let done = Report::from(Note::new(ToastLevel::Success, done));
            self.complete_waiting(|wait| *wait == Wait::Mash, done);
        }
    }

    /// The rules of `triggers.toml`, at startup and on a reload (MAX-6)
    /// The triggers of the config and its profiles, each keeping the count and the last fire
    /// of the one of the same name it replaces (MAX-220): a reload or a profile lay doesn't
    /// reset them
    pub fn set_triggers(&mut self, mut triggers: Vec<Trigger>) {
        for trigger in &mut triggers {
            if let Some(old) = self.triggers.iter().find(|old| old.name == trigger.name) {
                trigger.hits = old.hits;
                trigger.last_hit = old.last_hit;
            }
        }
        self.triggers = triggers;
    }

    /// The saved links, at start and on a reload: a file that doesn't load is said, the app
    /// running without them (MAX-185)
    pub fn set_links(&mut self, links: Result<LinksConfig, String>) {
        if let Err(err) = &links {
            let text = err.clone();
            self.emit(Event::ConfigWarning { text });
        }
        self.links = links;
    }

    pub fn triggers(&self) -> &[Trigger] {
        &self.triggers
    }

    /// Toggles the trigger named, or all of them: armed when none is, else disarmed (MAX-6)
    fn toggle_triggers(&mut self, name: Option<&str>) -> Note {
        let named = (self.triggers.iter()).filter(|t| name.is_none_or(|name| name == t.name));
        let on = !named.into_iter().any(Trigger::is_armed);
        self.arm_triggers(on, name)
    }

    /// Arms or disarms the triggers, or the one named, with a notice of what is armed after
    /// (MAX-6)
    fn arm_triggers(&mut self, on: bool, name: Option<&str>) -> Note {
        if self.triggers.is_empty() {
            return Note::warn("triggers.toml has no trigger");
        }
        for trigger in &mut self.triggers {
            if name.is_none_or(|name| name == trigger.name) {
                trigger.arm(on);
            }
        }
        let armed = self.armed_triggers().join(", ");
        if armed.is_empty() {
            Note::info("Triggers disarmed")
        } else {
            Note::info(format!("Triggers armed: {armed}"))
        }
    }

    /// The counts of the triggers back to zero, or the one named's, with a notice (MAX-220)
    fn reset_trigger_hits(&mut self, name: Option<&str>) -> Note {
        for trigger in &mut self.triggers {
            if name.is_none_or(|name| name == trigger.name) {
                trigger.reset_hits();
            }
        }
        match name {
            Some(name) => Note::info(format!("Trigger {name}: count reset")),
            None => Note::info("Trigger counts reset"),
        }
    }

    /// The names of the armed triggers
    pub fn armed_triggers(&self) -> Vec<&str> {
        (self.triggers.iter())
            .filter(|trigger| trigger.is_armed())
            .map(|trigger| trigger.name.as_str())
            .collect()
    }

    /// The armed triggers whose text ends in these live bytes, with where it ends in them
    /// (MAX-6). A replay's bytes never fire one, nor what a restore or the restore question
    /// held back (fed apart)
    fn match_triggers(&mut self, chunk: &[u8]) -> Vec<(usize, usize)> {
        if !self.link_kind().writes() {
            return Vec::new();
        }
        (self.triggers.iter_mut().enumerate())
            .filter_map(|(i, trigger)| Some((i, trigger.fires(chunk)?)))
            .collect()
    }

    /// Feeds `chunk`, cut where a `fired` trigger with a mark has its text end, to put the
    /// mark on the line being received there (MAX-220)
    fn feed_marking(&mut self, chunk: &[u8], fired: &[(usize, usize)]) {
        let mut marks: Vec<(usize, char)> = (fired.iter())
            .filter_map(|&(i, end)| Some((end, self.triggers[i].mark?)))
            .collect();
        marks.sort_by_key(|&(end, _)| end);
        let mut from = 0;
        for (end, letter) in marks {
            if end > from {
                self.feed(&chunk[from..end], true);
                from = end;
            }
            self.mark_receiving(letter);
        }
        if from < chunk.len() || from == 0 {
            self.feed(&chunk[from..], true);
        }
    }

    /// Mark `letter` on the line being received, in each mode fed (MAX-220)
    fn mark_receiving(&mut self, letter: char) {
        let (feed_line, feed_terminal) = self.fed_modes();
        if feed_line {
            self.line_mode.mark_receiving(letter);
        }
        if feed_terminal {
            self.terminal_mode.mark_cursor_row(letter);
        }
    }

    /// The `fired` triggers run their chain, queued with the trigger as its origin, with a
    /// notice (MAX-6), once their chunk is fed
    fn run_fired(&mut self, fired: &[(usize, usize)]) {
        for &(i, _) in fired {
            let trigger = &self.triggers[i];
            let fired = Event::TriggerFired {
                name: trigger.name.clone(),
                run: trigger.run.clone(),
                silent: trigger.silent,
            };
            let invocations = trigger.invocations.clone();
            let origin = Origin::Trigger(trigger.name.clone());
            self.emit(fired);
            for note in self.queue_chain(origin, invocations).notes {
                self.show_note(note);
            }
        }
    }

    /// Which modes the received bytes go to: the one shown, both with `always_both`, never
    /// a disabled one
    pub fn fed_modes(&self) -> (bool, bool) {
        let both = self.settings.dual_mode_buffering == DualBufferMode::AlwaysBoth;
        let fed = |mode| self.settings.mode_enabled(mode) && (both || self.settings.mode == mode);
        (fed(Mode::Line), fed(Mode::Terminal))
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

        // kept whatever the modes fed (MAX-290)
        self.rx_bytes.push(chunk);
        let (feed_line, feed_terminal) = self.fed_modes();
        if feed_terminal {
            self.sync_unhandled();
        }
        let settings = &self.settings;
        if feed_line {
            self.line_mode
                .feed_rx(settings.add_lf, settings.encoding, chunk);
        }
        if feed_terminal {
            (self.terminal_mode).process_device_rx(
                &self.rx_bytes,
                settings.encoding,
                chunk,
                settings.add_cr,
                settings.add_lf,
                settings.terminal_mode.byte_trace,
            );
            let answers = self.terminal_mode.take_answers();
            if live && self.settings.mode == Mode::Terminal {
                let bytes = answer_bytes(answers, &self.settings.terminal_mode);
                self.query_answers.extend(bytes);
            }
            // unanswered in line mode, the device may give up on its line editing: said now
            if live
                && self.terminal_mode.shell_signal() == Some(ShellSignal::Query)
                && let Some(key) = self.shell_hint_key()
            {
                self.emit(Event::ShellSeen { key });
            }
            if !self.settings.terminal_mode.notify_unhandled.is_empty() {
                self.emit_unhandled();
            } else {
                // collected for the markers only: nothing left for later (MAX-253)
                self.terminal_mode.take_dropped();
            }
            // a live link's, when allowed: never a replay's nor a held one's (MAX-275)
            let copy = self.terminal_mode.take_device_copy();
            if let Some(copy) = copy
                && live
                && self.link_kind().writes()
                && self.settings.terminal_mode.device_clipboard
            {
                self.device_copy = Some(copy);
            }
        }
    }

    /// Terminal mode collects what it doesn't handle while it's notified or marked, with the
    /// tokenizer when a group needs it or anything is marked, and marks the groups of
    /// `mark_unhandled`: the settings rule each read, as a link switched to has a fresh
    /// terminal (MAX-264, MAX-253)
    pub fn sync_unhandled(&mut self) {
        let terminal = &self.settings.terminal_mode;
        let (notify, marks) = (&terminal.notify_unhandled, &terminal.mark_unhandled);
        let collect = !notify.is_empty() || !marks.is_empty();
        let tokens = !marks.is_empty() || notify.iter().any(|group| group.needs_tokens());
        if self.terminal_mode.collecting_dropped() != (collect, tokens) {
            self.terminal_mode.collect_dropped(collect, tokens);
        }
        if self.terminal_mode.marked_groups() != marks.as_slice() {
            let marks = marks.clone();
            self.terminal_mode.mark_dropped(&marks);
        }
    }

    /// What the emulator didn't use in the last read, counted with what its notification
    /// already shows: `Unhandled: CSI ?2026h ×3, NUL ×9`, the kinds seen most first past the
    /// first few, only the groups of `notify_unhandled`; a new count once the notification is
    /// gone
    /// (MAX-264)
    fn emit_unhandled(&mut self) {
        let dropped = self.terminal_mode.take_dropped();
        let groups = &self.settings.terminal_mode.notify_unhandled;
        let mut new = (dropped.iter())
            .filter(|dropped| groups.contains(&dropped.group()))
            .peekable();
        if new.peek().is_none() {
            return;
        }
        let shown =
            (self.notifications.active_toasts().iter()).any(|toast| toast.key == Some("unhandled"));
        if !shown {
            self.unhandled.clear();
        }
        for label in new.map(|dropped| dropped.label()) {
            match self.unhandled.iter_mut().find(|(seen, _)| *seen == label) {
                Some((_, count)) => *count += 1,
                None => self.unhandled.push((label, 1)),
            }
        }
        const SHOWN: usize = 6;
        let mut kinds: Vec<&(String, usize)> = self.unhandled.iter().collect();
        if kinds.len() > SHOWN {
            kinds.sort_by_key(|&(_, count)| std::cmp::Reverse(*count));
        }
        let more = kinds.len().saturating_sub(SHOWN);
        let kinds = kinds.into_iter().take(SHOWN).cloned().collect();
        self.emit(Event::Unhandled { kinds, more });
    }

    /// The device's last OSC 52 copy to send on, base64, since the last call (MAX-275)
    pub fn take_device_copy(&mut self) -> Option<Vec<u8>> {
        self.device_copy.take()
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
    fn save_screenshot(&mut self, path: &Path, contents: String) -> Report {
        let path = expand_home(path);
        let saved = create_parent_dirs(&path).and_then(|()| std::fs::write(&path, contents));
        match saved {
            Ok(()) => Report::from(Note::new(
                ToastLevel::Success,
                format!("Screenshot saved to {}", path.display()),
            )),
            Err(e) => Report::failed(format!("Failed to save screenshot: {e}")),
        }
    }

    /// `screenshot <name>` (MAX-12): saved at once, its format from its extension (`.ans` the
    /// colors, else text, `.txt` added to a name without one), a bare name in `screenshot.dir`
    fn save_screenshot_as(&mut self, name: &Path) -> Report {
        let format = match name.extension() {
            Some(ext) if ext == "ans" => ScreenshotFormat::Ansi,
            _ => ScreenshotFormat::Text,
        };
        let mut path = self.settings.screenshot.dir.join(name);
        if path.extension().is_none() {
            path.set_extension(format.extension());
        }
        let contents = self.terminal_mode.screenshot().into_contents(format);
        self.save_screenshot(&path, contents)
    }

    /// Whether an on/off setting is on, for `set` (MAX-12); none for the ones with more values
    pub fn setting_is_on(&self, key: SettingKey) -> Option<bool> {
        match key {
            SettingKey::LocalEcho => Some(self.settings.local_echo),
            SettingKey::DisplayTimestamp => Some(self.settings.show_timestamps),
            SettingKey::Mouse => Some(self.settings.ui.mouse),
            SettingKey::AnswerQueries => {
                Some(!self.settings.terminal_mode.answer_groups.is_empty())
            }
            SettingKey::ByteTrace => Some(self.settings.terminal_mode.byte_trace),
            SettingKey::AddCr => Some(self.settings.add_cr),
            SettingKey::AddLf => Some(self.settings.add_lf),
            _ => None,
        }
    }

    fn take_screenshot(&mut self, format: ScreenshotFormat) -> Option<Note> {
        if self.settings.mode != Mode::Terminal {
            return Some(Note::info("Screenshots are in terminal mode"));
        }
        let shot = self.terminal_mode.screenshot();
        let stem = self.settings.screenshot.stem();
        let dialog = ScreenshotDialog::new(format, &stem.to_string_lossy(), shot);
        self.open_overlay(Overlay::Screenshot(dialog));
        None
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
        let loaded = self.load_group_slice(now);
        // the line heights counted a slice a frame, the busy indicator turning (MAX-272)
        let busy = self.busy().is_some();
        // a flash is redrawn until it's over, the last frame without it
        let flashing = self.terminal_mode.has_pending_flash() || self.mark_flash.is_some();
        toasts_expired || flashing || drag_scrolled || tested || loaded || busy
    }

    /// Work left for the next frames, with since when, for the busy indicator: for now the line
    /// heights of a long history, in line mode (MAX-272), maybe more later
    pub fn busy(&self) -> Option<(&'static str, Instant)> {
        let since = (self.settings.mode == Mode::Line)
            .then(|| self.line_mode.counting_since())
            .flatten()?;
        Some(("Counting lines", since))
    }

    /// The left button is on a scrollbar, from its press to its release (MAX-142)
    pub fn is_scrollbar_held(&self) -> bool {
        self.scrollbar_hold.is_some()
    }

    /// The scrollbar the left button holds (MAX-252)
    pub fn scrollbar_held(&self) -> Option<ScrollbarId> {
        self.scrollbar_hold.map(|(id, _)| id)
    }

    /// The left button on a scrollbar, as the last frame drew it (MAX-142): `▲` or `▼` scrolls
    /// a row, then again every `DRAG_SCROLL_EVERY` while held; a press on the thumb grabs it by
    /// that row, on the rest of the track it puts the thumb's middle there first; a drag then
    /// moves the thumb with the pointer
    fn scrollbar(&mut self, action: ScrollbarAction, now: Instant) {
        let id = match action {
            ScrollbarAction::Press(id, _) => Some(id),
            _ => self.scrollbar_held(),
        };
        let Some((id, bar)) = id.and_then(|id| Some((id, self.frame_map.scrollbar_of(id)?))) else {
            self.scrollbar_hold = None;
            return;
        };
        let to_thumb = |first: isize| BarScroll::To(bar.position_at(first));
        match action {
            ScrollbarAction::Press(_, row) => {
                let (hold, scroll) = if row == 0 {
                    (ScrollbarHold::Up, BarScroll::Up)
                } else if row + 1 >= bar.area.height {
                    (ScrollbarHold::Down, BarScroll::Down)
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
                self.scrollbar_hold = Some((id, hold));
                self.scrollbar_repeat_at = Some(now + SCROLLBAR_REPEAT_AFTER);
                self.scroll_bar(id, bar, scroll);
            }
            ScrollbarAction::Drag(row) => {
                if let Some((_, ScrollbarHold::Thumb { grab })) = self.scrollbar_hold {
                    let first = row as isize - 1 - grab as isize;
                    self.scroll_bar(id, bar, to_thumb(first));
                }
            }
            ScrollbarAction::Release => self.scrollbar_hold = None,
        }
    }

    /// Scrolls what scrollbar `id` runs along: the history, or the dump (MAX-252)
    fn scroll_bar(&mut self, id: ScrollbarId, bar: ScrollbarMap, scroll: BarScroll) {
        match (id, scroll) {
            (ScrollbarId::History, BarScroll::Up) => {
                self.update_action_history(HistoryAction::ScrollUp(1));
            }
            (ScrollbarId::History, BarScroll::Down) => {
                self.update_action_history(HistoryAction::ScrollDown(1));
            }
            (ScrollbarId::History, BarScroll::To(position)) => {
                self.update_action_history(HistoryAction::ScrollTo(bar.max - position));
            }
            (ScrollbarId::Dump, scroll) => {
                let (dump, rx) = (self.terminal_mode.dump_mut(), &self.rx_bytes);
                match scroll {
                    BarScroll::Up => dump.scroll_up(rx, 1),
                    BarScroll::Down => dump.scroll_down(rx, 1),
                    BarScroll::To(position) => dump.scroll_to(rx, position),
                }
            }
        }
    }

    /// The left button is on a side panel's border, from its press to its release (MAX-189)
    pub fn is_side_handle_held(&self) -> bool {
        self.side_handle_held.is_some()
    }

    /// The left button on a side panel's border, as the last frame drew it (MAX-189): a drag
    /// puts the border under the pointer, in the config's range, that panel's only (MAX-230)
    fn side_handle(&mut self, action: SideHandleAction) {
        match action {
            SideHandleAction::Press(panel) => {
                self.side_handle_held = self.frame_map.side_handle(panel).map(|_| panel);
            }
            SideHandleAction::Drag(x, y) => {
                let held = self.side_handle_held;
                if let Some(handle) = held.and_then(|panel| self.frame_map.side_handle(panel)) {
                    *self.split_percent_mut(handle.panel) = handle
                        .percent_at(x, y)
                        .clamp(SPLIT_PERCENT_MIN, SPLIT_PERCENT_MAX);
                }
            }
            SideHandleAction::Release => self.side_handle_held = None,
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
        let (id, scroll) = match self.scrollbar_hold {
            Some((id, ScrollbarHold::Up)) => (id, BarScroll::Up),
            Some((id, ScrollbarHold::Down)) => (id, BarScroll::Down),
            _ => return false,
        };
        if self.scrollbar_repeat_at.is_some_and(|at| now < at) {
            return false;
        }
        let Some(bar) = self.frame_map.scrollbar_of(id) else {
            return false;
        };
        self.scrollbar_repeat_at = Some(now + DRAG_SCROLL_EVERY);
        self.scroll_bar(id, bar, scroll);
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
        let (up, lines) = if y <= top {
            (true, usize::from(top - y) + 1)
        } else if y >= bottom {
            (false, usize::from(y - bottom) + 1)
        } else {
            return false;
        };
        // onto the edge row the last scroll brought in, then the next one
        if let Some(pos) = self.frame_map.copy_position(side, x, y, true) {
            let mut report = Report::default();
            self.update_action_copy_mode(CopyModeAction::MouseDrag(pos, (x, y)), &mut report);
            self.present(&Caller::input(), report);
        }
        // terminal mode's dump scrolls on its own (MAX-252)
        if side == CopySide::Raw && self.settings.mode == Mode::Terminal {
            self.update_side_view(if up {
                SideViewAction::ScrollUp(lines)
            } else {
                SideViewAction::ScrollDown(lines)
            });
        } else {
            self.update_action_history(if up {
                HistoryAction::ScrollUp(lines)
            } else {
                HistoryAction::ScrollDown(lines)
            });
        }
        self.last_drag_scroll = Some(now);
        true
    }

    /// An app action from `origin`, what it reports added to `report`
    fn update_action_app(
        &mut self,
        origin: &Origin,
        action: AppAction,
        report: &mut Report,
    ) -> UpdateEffect {
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
                if is_focused != self.is_window_focused {
                    self.terminal_mode.report_focus(is_focused);
                }
                self.is_window_focused = is_focused;
                UpdateEffect::None
            }
            AppAction::ToggleSetting(setting) => {
                report.notes.extend(self.toggle_setting(origin, setting));
                UpdateEffect::None
            }
            AppAction::SetSetting(setting, word) => {
                (report.notes).extend(self.set_word_setting(origin, setting, word));
                UpdateEffect::None
            }
            AppAction::ToggleRecording => {
                report.notes.extend(self.toggle_recording());
                UpdateEffect::None
            }
            AppAction::ScrollTo(index) => {
                match self.settings.mode {
                    Mode::Line => self.line_mode.go_to_line(index),
                    Mode::Terminal => self.terminal_mode.go_to_row(index),
                }
                UpdateEffect::None
            }
            // copy mode afresh: one on the raw side or holding a search's matches ends first
            AppAction::Select(from, to) => {
                if self.is_copy_mode_active() {
                    self.update_action_copy_mode(CopyModeAction::Exit, report);
                }
                self.copy_action(CopyModeAction::Enter, report);
                self.copy_action(CopyModeAction::SelectArea(from, to), report);
                UpdateEffect::None
            }
            AppAction::SelectClear if self.is_copy_mode_active() => {
                self.update_action_copy_mode(CopyModeAction::Exit, report)
            }
            AppAction::SelectClear => UpdateEffect::None,
            AppAction::ResizeTerminal { rows, cols } => {
                self.resize_terminal(rows, cols);
                UpdateEffect::None
            }
            AppAction::StartRecording { path, stream } => {
                report.extend(self.start_capture(&path, stream, false));
                UpdateEffect::None
            }
            AppAction::OpenOverlay(overlay) => {
                self.overlay = Some(overlay);
                UpdateEffect::None
            }
            AppAction::CloseOverlay => {
                self.close_overlay();
                UpdateEffect::None
            }
            AppAction::Quit => {
                self.overlay = Some(Overlay::ConfirmQuit);
                UpdateEffect::None
            }
            AppAction::OpenSessions => {
                report.notes.extend(self.open_session_browser());
                UpdateEffect::None
            }
            AppAction::Notify(level, text) => {
                report.notes.push(Note::new(level, text));
                UpdateEffect::None
            }
            AppAction::StartMash(mash) => {
                let until = mash.until_text();
                report
                    .notes
                    .push(Note::info(format!("Mashing until '{until}'")));
                // the rest of the chain waits for its text, its own timeout at most
                report.wait = Some(Wait::Mash);
                let (until, cause) = (until.to_string(), Cause::from(origin));
                self.emit(Event::MashStarted { until, cause });
                self.mash = Some(*mash);
                UpdateEffect::None
            }
            AppAction::WaitFor(mut watch) => {
                // the chain waits for one of its texts, failed at its time
                watch.id = self.next_watch;
                self.next_watch += 1;
                report
                    .notes
                    .push(Note::info(format!("Waiting for {}", watch.texts_shown())));
                report.wait = Some(Wait::Text {
                    watch: watch.id,
                    until: watch.until,
                });
                self.text_watches.push(*watch);
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
                    report.notes.push(Note::info(message).in_slot("transfer"));
                }
                // the rest of the chain waits for the file's end (MAX-165)
                report.wait = Some(Wait::Transfer);
                let direction = if transfer.is_download() {
                    "receive"
                } else {
                    "send"
                };
                self.emit(Event::TransferStarted {
                    direction,
                    name: transfer.name(),
                    size: transfer.size(),
                    protocol: transfer.protocol().map(Protocol::name),
                    cause: Cause::from(origin),
                });
                self.transfer = Some(*transfer);
                self.busy_said = false;
                self.recancel_until = None;
                UpdateEffect::None
            }
            AppAction::Reconfigure(line) => UpdateEffect::Reconfigure(line),
            AppAction::PauseCommands(duration) => {
                report.wait = Some(Wait::Until(Instant::now() + duration));
                UpdateEffect::None
            }
            AppAction::LineControl(control) => UpdateEffect::LineControl(control),
            AppAction::Open(port) => UpdateEffect::Open(port),
            AppAction::OpenLink(config) => UpdateEffect::OpenLink(config),
            // `link-save` and `link-delete` (MAX-185): a refusal stops the chain
            AppAction::SaveLink(link) => {
                match self.store_link(&link, None) {
                    Ok(notes) => report.notes.extend(notes),
                    Err(err) => report.fail(format!("link-save: {err}")),
                }
                UpdateEffect::None
            }
            AppAction::DeleteLink(link) => {
                match self.remove_link(&link) {
                    Ok(note) => report.notes.push(note),
                    Err(err) => report.fail(format!("link-delete: {err}")),
                }
                UpdateEffect::None
            }
            AppAction::NameSession(name) => {
                match self.name_session(&name) {
                    Ok(named) => report.notes.push(named),
                    Err(err) => report.fail(format!("session-name: {err}")),
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
                report.extend(self.control_replay(origin, control));
                UpdateEffect::None
            }
            AppAction::SetEncoding(encoding) => {
                let before = query::setting(self, SettingKey::TextEncoding);
                self.settings.encoding = encoding;
                self.publish_setting(origin, SettingKey::TextEncoding, before);
                let encoding = encoding.as_str();
                let text = format!("Rx Text Encoding is {encoding}");
                report.notes.push(setting_note("setting:encoding", text));
                UpdateEffect::None
            }
            AppAction::SaveScreenshot(name) => {
                report.extend(self.save_screenshot_as(&name));
                UpdateEffect::None
            }
            AppAction::QueueCommands(invocations) => {
                report.extend(self.queue_chain(Origin::KeyBinding, invocations));
                UpdateEffect::None
            }
            AppAction::CancelCommands => {
                report.extend(self.cancel(origin));
                UpdateEffect::None
            }
            AppAction::ToggleTriggers => {
                report.notes.push(self.toggle_triggers(None));
                UpdateEffect::None
            }
            AppAction::ArmTriggers { on, name } => {
                report.notes.push(self.arm_triggers(on, name.as_deref()));
                UpdateEffect::None
            }
            AppAction::ResetTriggers(name) => {
                report.notes.push(self.reset_trigger_hits(name.as_deref()));
                UpdateEffect::None
            }
            AppAction::ShowPlot(name) => {
                let before = query::setting(self, SettingKey::Plot);
                report.notes.extend(self.show_plot(name.as_deref()));
                self.publish_setting(origin, SettingKey::Plot, before);
                UpdateEffect::None
            }
            AppAction::ShowProfiles => {
                report.notes.push(self.profiles_note());
                UpdateEffect::None
            }
            AppAction::LayProfiles(profiles) => UpdateEffect::LayProfiles(profiles),
            AppAction::TogglePlotFreeze => {
                report.notes.push(self.toggle_plot_freeze());
                UpdateEffect::None
            }
            AppAction::StepPlot(forward) => {
                let before = query::setting(self, SettingKey::Plot);
                report.notes.extend(self.step_plot(forward));
                self.publish_setting(origin, SettingKey::Plot, before);
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
                let rows = self.mark_rows(self.settings.mode);
                self.overlay = Some(Overlay::Marks(MarkBrowser::new(rows)));
                UpdateEffect::None
            }
            AppAction::Answer(answer) => {
                report.answer = Some(*answer);
                UpdateEffect::None
            }
            AppAction::OpenNotices => {
                let notices = self.notifications.history().to_vec();
                self.overlay = Some(Overlay::Notices(NoticeBrowser::new(notices)));
                UpdateEffect::None
            }
            AppAction::Mark(action) => {
                report.notes.extend(self.update_mark(action));
                UpdateEffect::None
            }
            AppAction::PasteBuffer => {
                let Some(text) = self.paste_buffer.clone() else {
                    return UpdateEffect::None;
                };
                let count = text.chars().count();
                report
                    .notes
                    .push(Note::info(format!("Pasted {count} chars")));
                let paste = match self.settings.mode {
                    Mode::Terminal => Action::Terminal(TerminalAction::Paste(text)),
                    Mode::Line => Action::LineInput(LineInputAction::Paste(text)),
                };
                let (effect, pasted) = self.update_from(&Origin::Input, paste);
                report.extend(pasted);
                effect
            }
            // the live link a replay replaced comes back with the attach key too (MAX-166)
            AppAction::ToggleAttach if self.replaced_link().is_some() => UpdateEffect::StopReplay,
            AppAction::ToggleAttach => UpdateEffect::ToggleAttach,
            AppAction::SetAttached(true) if self.replaced_link().is_some() => {
                UpdateEffect::StopReplay
            }
            AppAction::SetAttached(attach) => UpdateEffect::SetAttached(attach),
            AppAction::ReloadConfig => UpdateEffect::ReloadConfig,
            AppAction::Redraw => UpdateEffect::Redraw,
            AppAction::DisconnectClients => {
                self.drop_clients();
                UpdateEffect::DisconnectClients
            }
            AppAction::SideView(action) => self.update_side_view(action),
            AppAction::ResizeSidePanel(by) => {
                report.notes.extend(self.resize_side_panel(by));
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
    /// The emulator's size, the resize dialog's or `resize`'s (MAX-219)
    fn resize_terminal(&mut self, rows: u16, cols: u16) {
        self.settings.terminal_mode.cols = cols;
        self.settings.terminal_mode.rows = rows;
        self.terminal_mode.set_vt100_size(rows, cols);
    }

    fn resize_side_panel(&mut self, by: i16) -> Option<Note> {
        let off = self.settings.raw_side.view == RawSideView::Off && self.plot.is_none();
        let note = off.then(|| Note::warn("SideView is Off"));
        // the plot's while one is shown, the raw panel's otherwise (MAX-230)
        let panel = match self.plot {
            Some(_) => SidePanel::Plot,
            None => SidePanel::Raw,
        };
        let split = *self.split_percent_mut(panel);
        // the big steps fit the dump's lines in terminal mode
        let to = match panel {
            SidePanel::Raw if by.abs() > 1 => self.dump_resize(split, by.signum()),
            _ => None,
        };
        let to = to.unwrap_or_else(|| (split as i16 + by).max(0) as u16);
        *self.split_percent_mut(panel) = to.clamp(SPLIT_PERCENT_MIN, SPLIT_PERCENT_MAX);
        note
    }

    /// Terminal mode's raw panel on the right, from `split` percent: growing (`dir` 1), the
    /// first size where the dump shows a group of bytes more a line; shrinking (-1), the last
    /// size below where a group just fits, with no padding left. By 1 % when there's none
    fn dump_resize(&self, split: u16, dir: i16) -> Option<u16> {
        if !self.dump_on_right() {
            return None;
        }
        let handle = self.frame_map.side_handle(SidePanel::Raw)?;
        let groups = |percent| self.dump_groups(handle.split, percent);
        let found = if dir > 0 {
            let now = groups(split);
            (split + 1..=SPLIT_PERCENT_MAX).find(|&percent| groups(percent) > now)
        } else {
            (SPLIT_PERCENT_MIN + 1..split)
                .rev()
                .find(|&percent| groups(percent) > groups(percent - 1))
        };
        Some(found.unwrap_or_else(|| split.saturating_add_signed(dir)))
    }

    /// Terminal mode's raw panel, on the right: the dump's lines can size it
    fn dump_on_right(&self) -> bool {
        self.settings.mode == Mode::Terminal
            && self.settings.raw_side.orientation == RawSideOrientation::Right
    }

    /// The groups of bytes a dump line shows with the raw panel at `percent` of `split`
    fn dump_groups(&self, split: Rect, percent: u16) -> usize {
        let (_, panel) = side_chunks(split, Direction::Horizontal, percent);
        // the left border, the scrollbar's column
        let chrome = 1 + u16::from(self.show_scrollbar());
        let width = panel.width.saturating_sub(chrome).into();
        let end = self.rx_bytes.end();
        DumpLayout::groups(width, self.settings.raw_side.view, end)
    }

    /// The first time the dump shows on the right, its panel takes the size that just fits
    /// one group of bytes a line, when one fits in the config's range; `split` is the room it
    /// shares with the screen. True when the size changed, to lay the frame out again
    pub fn fit_dump_once(&mut self, split: Rect) -> bool {
        if self.dump_fitted || !self.dump_on_right() {
            return false;
        }
        self.dump_fitted = true;
        let fits = (SPLIT_PERCENT_MIN..=SPLIT_PERCENT_MAX)
            .find(|&percent| self.dump_groups(split, percent) > 0);
        let Some(percent) = fits else {
            return false;
        };
        let changed = self.settings.raw_side.split_percent != percent;
        self.settings.raw_side.split_percent = percent;
        changed
    }

    /// The right-click menu's "Close": the raw view off, as `Ctrl-a B` says it, or the plot
    /// off (MAX-230)
    fn close_panel(&mut self, panel: SidePanel) -> Option<Note> {
        match panel {
            SidePanel::Raw => {
                self.settings.raw_side.view = RawSideView::Off;
                let view = self.settings.raw_side.view.as_str();
                Some(setting_note(
                    "setting:raw",
                    format!("Raw SideView is {view}"),
                ))
            }
            SidePanel::Plot => self.show_plot(None),
        }
    }

    /// A side panel's share of its split, in the settings (MAX-230)
    fn split_percent_mut(&mut self, panel: SidePanel) -> &mut u16 {
        match panel {
            SidePanel::Raw => &mut self.settings.raw_side.split_percent,
            SidePanel::Plot => &mut self.settings.plot_side.split_percent,
        }
    }

    fn update_side_view(&mut self, action: SideViewAction) -> UpdateEffect {
        let raw_view = self.settings.raw_side.view;
        match (self.settings.mode, action) {
            (Mode::Terminal, action) => {
                (self.terminal_mode).handle_side_view_action(&self.rx_bytes, action, raw_view);
            }
            // line mode's raw panel scrolls with the history
            (Mode::Line, SideViewAction::ScrollUp(lines)) => {
                return self.update_action_history(HistoryAction::ScrollUp(lines));
            }
            (Mode::Line, SideViewAction::ScrollDown(lines)) => {
                return self.update_action_history(HistoryAction::ScrollDown(lines));
            }
            (Mode::Line, SideViewAction::PageUp) => {
                return self.update_action_history(HistoryAction::ScrollUpPage);
            }
            (Mode::Line, SideViewAction::PageDown) => {
                return self.update_action_history(HistoryAction::ScrollDownPage);
            }
            (Mode::Line, action) => {
                self.line_mode.handle_side_view_action(action, raw_view);
            }
        }

        UpdateEffect::None
    }

    /// Copy mode's scroll, the cursor staying: the dump scrolls while the cursor is in it
    /// (MAX-252), else the history
    fn copy_scroll(&mut self, scroll: HistoryAction) -> UpdateEffect {
        if self.settings.mode == Mode::Line || self.terminal_mode.dump_copy().is_none() {
            return self.update_action_history(scroll);
        }
        let (dump, rx) = (self.terminal_mode.dump_mut(), &self.rx_bytes);
        let page = dump.page();
        match scroll {
            HistoryAction::ScrollUp(lines) => dump.scroll_up(rx, lines),
            HistoryAction::ScrollDown(lines) => dump.scroll_down(rx, lines),
            HistoryAction::ScrollUpPage => dump.scroll_up(rx, page),
            HistoryAction::ScrollDownPage => dump.scroll_down(rx, page),
            _ => {}
        }
        UpdateEffect::None
    }

    /// The copy action applied by the mode's copy state: the text a yank gives, what it says
    /// added to `report`
    fn copy_action(&mut self, action: CopyModeAction, report: &mut Report) -> Option<String> {
        match self.settings.mode {
            Mode::Terminal => {
                let text =
                    (self.terminal_mode).handle_copy_action(&self.rx_bytes, action, &self.settings);
                // `Y` couldn't give the bytes received (MAX-256)
                if let Some(why) = self.terminal_mode.take_untraced_yank() {
                    let why = match why {
                        UntracedYank::TraceOff => "the byte trace is off",
                        UntracedYank::NotTraced => "no byte trace for this selection",
                    };
                    let text = format!("Copied the screen's bytes, not the ones received: {why}");
                    report.notes.push(Note::warn(text));
                }
                text
            }
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

    /// A copy mode action, what it says added to `report`
    fn update_action_copy_mode(
        &mut self,
        action: CopyModeAction,
        report: &mut Report,
    ) -> UpdateEffect {
        if let CopyModeAction::ScrollView(scroll) = action {
            return self.copy_scroll(scroll);
        }
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
                        self.close_search_for_click(report);
                    }
                    let press = CopyModeAction::MousePress(pos, side, MouseUnit::Char);
                    self.copy_action(press, report);
                }
                self.clicks.reset();
                self.drag_hold = Some(at);
                action
            }
            CopyModeAction::MouseRelease => {
                self.drag_hold = None;
                // a click with no drag: on a link's text, the link is copied (MAX-277)
                if let Some((pos, side)) = self.pending_press.take() {
                    let link = (side == CopySide::Text)
                        .then(|| self.terminal_mode.link_at(pos.line_idx, pos.char_idx));
                    return match link.flatten() {
                        Some(uri) => self.copy_link(uri),
                        None => UpdateEffect::None,
                    };
                }
                action
            }
            action => action,
        };
        // a click in the history leaves the search for copy mode there (MAX-113)
        if matches!(action, CopyModeAction::MousePress(..)) && self.is_search_active() {
            self.close_search_for_click(report);
        }
        let copied = self.copy_action(action, report);
        if let Some(link) = self.terminal_mode.take_link_asked() {
            return match link {
                Some(uri) => self.copy_link(uri),
                None => {
                    let note = Note::warn("No link under the cursor").in_slot("copy");
                    report.notes.push(note);
                    UpdateEffect::None
                }
            };
        }
        match copied {
            Some(text) => {
                self.paste_buffer = Some(text.clone());
                UpdateEffect::CopyToClipboard(text)
            }
            None => UpdateEffect::None,
        }
    }

    /// A link the device drew, copied: its URI to the clipboard and the paste buffer, said as
    /// `Copied the link: <URI>` (MAX-277). maxicom never opens it
    fn copy_link(&mut self, uri: String) -> UpdateEffect {
        self.paste_buffer = Some(uri.clone());
        UpdateEffect::CopyToClipboardAs {
            text: uri,
            what: "link".to_string(),
        }
    }

    /// A search action, what it says added to `report`
    fn update_action_search_mode(
        &mut self,
        action: SearchModeAction,
        report: &mut Report,
    ) -> UpdateEffect {
        match action {
            SearchModeAction::Repeat(opposite) => {
                report.notes.extend(self.repeat_search(opposite));
                return UpdateEffect::None;
            }
            SearchModeAction::History(up) => {
                self.walk_search_history(up);
                return UpdateEffect::None;
            }
            SearchModeAction::Word(direction) => {
                self.search_word(direction, report);
                return UpdateEffect::None;
            }
            // a search always runs in copy mode (MAX-215): from out of it, copy mode starts
            // at the view, its cursor at the end of its line so that `?` from the live view
            // finds the newest line too, and Esc leaves it for the live view
            SearchModeAction::Start(_) | SearchModeAction::Query(_)
                if !self.is_copy_mode_active() =>
            {
                self.copy_action(CopyModeAction::Enter, report);
                self.copy_action(CopyModeAction::EndOfLine, report);
                self.search_mut().exits_copy = true;
            }
            _ => {}
        }
        let leaves_copy = matches!(action, SearchModeAction::Cancel)
            && std::mem::take(&mut self.search_mut().exits_copy);
        let confirmed = matches!(action, SearchModeAction::Confirm);
        // no dump to show a raw search's matches in terminal mode (MAX-252)
        let opens = matches!(
            action,
            SearchModeAction::Start(_) | SearchModeAction::Query(_) | SearchModeAction::Kind
        );
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
            Mode::Terminal => {
                (self.terminal_mode.searching(&self.rx_bytes)).handle_search_action(action, view)
            }
            Mode::Line => self.line_mode.handle_search_action(action, view),
        }
        if opens
            && self.settings.mode == Mode::Terminal
            && self.terminal_mode.dump_search()
            && self.settings.raw_side.view == RawSideView::Off
        {
            let text = "A raw search needs the raw panel: no bytes shown to search";
            report.notes.push(Note::warn(text));
        }
        if leaves_copy {
            self.copy_action(CopyModeAction::Exit, report);
            self.update_action_history(HistoryAction::ScrollToBottom);
        }
        if confirmed {
            let query = self.search_mut().query_str().to_string();
            self.push_search_history(query);
            let search = self.search_mut();
            let forward = search.direction == SearchDirection::Forward;
            if started_at.is_some_and(|origin| search.went_around(origin)) {
                report.notes.push(went_around_note(forward));
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
                    (self.terminal_mode.searching(&self.rx_bytes)).update_search_matches(view);
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
    fn search_word(&mut self, direction: SearchDirection, report: &mut Report) {
        let (cursor, text) = match self.settings.mode {
            Mode::Terminal => (
                self.terminal_mode.copy_state().cursor,
                self.terminal_mode.copy_line_text(),
            ),
            Mode::Line => (
                self.line_mode.copy_state().cursor,
                self.line_mode.copy_line_text(&self.settings),
            ),
        };
        let Some((start, word)) = word_at(&text, cursor.char_idx) else {
            report.notes.push(Note::warn("No word under the cursor"));
            return;
        };
        let copy_state = match self.settings.mode {
            Mode::Terminal => self.terminal_mode.copy_state_mut(),
            Mode::Line => self.line_mode.copy_state_mut(),
        };
        copy_state.jump_to(cursor.line_idx, start);
        self.search_mut().mode_kind = SearchModeKind::Text;
        self.update_action_search_mode(SearchModeAction::Start(direction), report);
        self.update_action_search_mode(SearchModeAction::Paste(word), report);
        self.update_action_search_mode(SearchModeAction::Confirm, report);
    }

    /// A click in the history closes the search for copy mode there: copy mode stays, also
    /// when the search started it (MAX-215)
    fn close_search_for_click(&mut self, report: &mut Report) {
        self.search_mut().exits_copy = false;
        self.update_action_search_mode(SearchModeAction::Cancel, report);
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

    /// `n` / `N` in copy mode (MAX-215), with a note when it goes around the history's end
    fn repeat_search(&mut self, opposite: bool) -> Option<Note> {
        let wrapped = match self.settings.mode {
            Mode::Terminal => {
                (self.terminal_mode.searching(&self.rx_bytes)).repeat_search(opposite)
            }
            Mode::Line => self.line_mode.repeat_search(opposite),
        };
        let forward = (self.search().direction == SearchDirection::Forward) != opposite;
        match wrapped {
            None => Some(Note::warn("No match to go to")),
            Some(true) => Some(went_around_note(forward)),
            Some(false) => None,
        }
    }

    /// `clear-history`: the history of the mode shown, and in both modes the bytes received
    /// with their trace (MAX-290), the offsets counting on
    fn clear_history(&mut self) {
        match self.settings.mode {
            Mode::Line => self.line_mode.clear_history(),
            Mode::Terminal => self.terminal_mode.clear_history(),
        }
        self.rx_bytes.clear();
        self.terminal_mode.dump_mut().clear();
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
            (_, HistoryAction::Clear) => self.clear_history(),
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
    /// their running value and are named in the note it gives
    pub fn apply_reload(
        &mut self,
        mut settings: SettingsConfig,
        key_resolver: KeyResolver,
    ) -> Note {
        let old = &self.settings;
        let mut restart_needed = Vec::new();
        let mut keep = |name: &'static str, is_changed: bool| {
            if is_changed {
                restart_needed.push(name);
            }
        };
        keep("rx_bytes", old.rx_bytes != settings.rx_bytes);
        settings.rx_bytes = old.rx_bytes;
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
        // the file's `answer_groups` rule now, what the switch kept aside goes
        self.answer_groups_off = None;
        self.fit_mode_to_link();
        self.key_resolver = key_resolver;
        self.prefix_active = false;
        self.update_viewport(self.viewport);

        if restart_needed.is_empty() {
            Note::info("Config reloaded")
        } else {
            let restart = restart_needed.join(", ");
            Note::warn(format!("Config reloaded; restart to apply: {restart}"))
        }
    }

    /// The setting `key` cycled by `origin`, published when it changed (MAX-284): a mode
    /// `disabled_mode` keeps from switching isn't
    fn toggle_setting(&mut self, origin: &Origin, key: SettingKey) -> Option<Note> {
        let before = query::setting(self, key);
        let note = self.cycle_setting(key);
        self.publish_setting(origin, key, before);
        note
    }

    /// `key` published with `origin` as its cause, when `get` gives other than `before`
    fn publish_setting(&mut self, origin: &Origin, key: SettingKey, before: Value) {
        let value = match query::setting(self, key) {
            value if value == before => return,
            Value::Bool(on) => SettingValue::On(on),
            Value::Text(word) => SettingValue::Word(word),
            value => unreachable!("a setting is on/off or a word, not {value:?}"),
        };
        let (setting, cause) = (key.name(), Cause::from(origin));
        self.emit(Event::SettingChanged {
            setting,
            value,
            cause,
        });
    }

    fn cycle_setting(&mut self, key: SettingKey) -> Option<Note> {
        let mut note = None;
        match key {
            SettingKey::LocalEcho => {
                self.settings.local_echo = !self.settings.local_echo;
            }
            SettingKey::LineWrap
            | SettingKey::IdentWrap
            | SettingKey::PrefixPlace
            | SettingKey::DisplayMode
            | SettingKey::DimensionMode
            | SettingKey::Backspace
            | SettingKey::TxEol
            | SettingKey::DisplayRaw => {
                self.step_word_setting(key);
                note = self.word_setting_applied(key);
            }
            // no switching to the mode `disabled_mode` turns off (MAX-260)
            SettingKey::Mode if !self.settings.mode_enabled(self.settings.mode.other()) => {
                let other = self.settings.mode.other().as_str();
                note = Some(Note::warn(format!(
                    "The {other} mode is disabled (disabled_mode)"
                )));
            }
            SettingKey::Mode => {
                self.settings.mode = self.settings.mode.other();
                self.update_viewport(self.viewport);
            }
            SettingKey::DisplayTimestamp => {
                self.settings.show_timestamps = !self.settings.show_timestamps;
            }
            SettingKey::TextEncoding => {
                self.settings.encoding.toggle();
                let encoding = self.settings.encoding.as_str();
                note = Some(setting_note(
                    "setting:encoding",
                    format!("Rx Text Encoding is {encoding}"),
                ));
            }
            SettingKey::Mouse => {
                self.settings.ui.mouse = !self.settings.ui.mouse;
                let state = on_off(self.settings.ui.mouse);
                note = Some(setting_note(
                    "setting:mouse",
                    format!("Mouse capture {state}"),
                ));
            }
            // off empties `answer_groups`, its list kept aside for on, all groups when none was
            SettingKey::AnswerQueries => {
                let groups = &mut self.settings.terminal_mode.answer_groups;
                if groups.is_empty() {
                    *groups = (self.answer_groups_off.take())
                        .filter(|kept| !kept.is_empty())
                        .unwrap_or_else(|| QueryGroup::ALL.to_vec());
                } else {
                    self.answer_groups_off = Some(std::mem::take(groups));
                }
                let state = on_off(!self.settings.terminal_mode.answer_groups.is_empty());
                note = Some(setting_note(
                    "setting:answers",
                    format!("Query answers {state}"),
                ));
            }
            // from the bytes received now on; off gives the memory back (MAX-256)
            SettingKey::ByteTrace => {
                let terminal = &mut self.settings.terminal_mode;
                terminal.byte_trace = !terminal.byte_trace;
                let state = on_off(terminal.byte_trace);
                if !terminal.byte_trace {
                    self.terminal_mode.dump_mut().forget_trace();
                }
                note = Some(setting_note(
                    "setting:byte-trace",
                    format!("Byte trace {state}"),
                ));
            }
            // the bytes received from now on, what's shown stays as it came (MAX-181)
            SettingKey::AddCr => {
                self.settings.add_cr = !self.settings.add_cr;
                let state = on_off(self.settings.add_cr);
                note = Some(setting_note("setting:add-cr", format!("Add CR {state}")));
            }
            SettingKey::AddLf => {
                self.settings.add_lf = !self.settings.add_lf;
                let state = on_off(self.settings.add_lf);
                note = Some(setting_note("setting:add-lf", format!("Add LF {state}")));
            }
            // the next plot of plots.toml, off after the last, as `plot` alone (MAX-222)
            // as `plot`: the next one, off after the last
            SettingKey::Plot if self.plots.plots.is_empty() => {
                note = Some(plot_note(ToastLevel::Warn, "plots.toml has no plot"));
            }
            SettingKey::Plot => {
                let name = self.next_plot().map(String::from);
                note = self.show_plot(name.as_deref());
            }
        }
        note
    }

    /// `set <setting> <word>` (MAX-219): a setting of more than two values set to the one
    /// `get` names, with what follows as the settings popup's cycle; its event when it changed
    fn set_word_setting(&mut self, origin: &Origin, key: SettingKey, word: &str) -> Option<Note> {
        let before = query::setting(self, key);
        if before == Value::text(word) {
            return None;
        }
        let value = toml::Value::String(word.to_string());
        let settings = &mut self.settings;
        let set = match key {
            SettingKey::LineWrap => value.try_into().map(|v| settings.line_mode.wrap_mode = v),
            SettingKey::IdentWrap => value.try_into().map(|v| settings.line_mode.wrap_indent = v),
            SettingKey::PrefixPlace => {
                (value.try_into()).map(|v| settings.line_mode.prefix_place = v)
            }
            SettingKey::DisplayMode => {
                (value.try_into()).map(|v| settings.line_mode.display_mode = v)
            }
            SettingKey::DisplayRaw => value.try_into().map(|v| settings.raw_side.view = v),
            SettingKey::DimensionMode => {
                (value.try_into()).map(|v| settings.terminal_mode.dimension_mode = v)
            }
            SettingKey::TxEol => value.try_into().map(|v| settings.line_mode.tx_eol = v),
            SettingKey::Backspace => value
                .try_into()
                .map(|v| settings.terminal_mode.backspace = v),
            _ => unreachable!("{} isn't a setting of words", key.name()),
        };
        set.expect("`set` checks the word against the setting's values");
        let note = self.word_setting_applied(key);
        self.publish_setting(origin, key, before);
        note
    }

    /// A setting of more than two values to its next one, as the settings popup cycles them
    fn step_word_setting(&mut self, key: SettingKey) {
        let line_mode = &mut self.settings.line_mode;
        match key {
            SettingKey::LineWrap => {
                line_mode.wrap_mode = match line_mode.wrap_mode {
                    WrapMode::Word => WrapMode::Char,
                    WrapMode::Char => WrapMode::Truncate,
                    WrapMode::Truncate => WrapMode::Word,
                };
            }
            SettingKey::IdentWrap => {
                line_mode.wrap_indent = match line_mode.wrap_indent {
                    PrefixWrapBehavior::None => PrefixWrapBehavior::Prefix,
                    PrefixWrapBehavior::Prefix => PrefixWrapBehavior::Timestamp,
                    PrefixWrapBehavior::Timestamp => PrefixWrapBehavior::None,
                }
            }
            SettingKey::PrefixPlace => {
                line_mode.prefix_place = match line_mode.prefix_place {
                    PrefixPlace::Inset => PrefixPlace::Border,
                    PrefixPlace::Border => PrefixPlace::None,
                    PrefixPlace::None => PrefixPlace::Inset,
                }
            }
            SettingKey::DisplayMode => line_mode.display_mode.toggle(),
            SettingKey::TxEol => line_mode.tx_eol.toggle(),
            SettingKey::DimensionMode => self.settings.terminal_mode.dimension_mode.toggle(),
            SettingKey::Backspace => self.settings.terminal_mode.backspace.toggle(),
            SettingKey::DisplayRaw => self.settings.raw_side.view.toggle(),
            _ => unreachable!("{} isn't a setting of words", key.name()),
        }
    }

    /// What follows a setting of more than two values changed, cycled or set: the view laid
    /// out again for the ones that size it, and the note saying the new value
    fn word_setting_applied(&mut self, key: SettingKey) -> Option<Note> {
        match key {
            SettingKey::LineWrap => {
                self.update_viewport(self.viewport);
                (self.settings.mode == Mode::Terminal)
                    .then(|| Note::warn("Line wrap: line mode only"))
            }
            SettingKey::IdentWrap | SettingKey::PrefixPlace => None,
            SettingKey::DisplayMode => {
                let mode = self.settings.line_mode.display_mode.as_str();
                Some(setting_note(
                    "setting:display",
                    format!("Display Mode is {mode}"),
                ))
            }
            SettingKey::DimensionMode => {
                self.update_viewport(self.viewport);
                let dimension_mode = self.settings.terminal_mode.dimension_mode;
                let text = format!("Terminal Size Mode is {}", dimension_mode.as_str());
                // the one sized from the view that cuts lines, unlike `auto` (MAX-282)
                Some(match dimension_mode {
                    TerminalDimensionMode::MatchView => Note::warn(format!(
                        "{text}: lines wider than the view can be cut, in the history too"
                    ))
                    .in_slot("setting:size"),
                    _ => setting_note("setting:size", text),
                })
            }
            // what the Backspace key sends in terminal mode (MAX-270)
            SettingKey::Backspace => {
                let byte = self.settings.terminal_mode.backspace.as_str();
                Some(setting_note(
                    "setting:backspace",
                    format!("Backspace sends {byte}"),
                ))
            }
            // what Enter sends in line mode (MAX-184)
            SettingKey::TxEol => {
                let eol = self.settings.line_mode.tx_eol.as_str();
                Some(setting_note(
                    "setting:enter",
                    format!("Line Enter sends {eol}"),
                ))
            }
            SettingKey::DisplayRaw => {
                let view = self.settings.raw_side.view.as_str();
                Some(setting_note(
                    "setting:raw",
                    format!("Raw SideView is {view}"),
                ))
            }
            _ => unreachable!("{} isn't a setting of words", key.name()),
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
    fn update_mark(&mut self, action: MarkAction) -> Option<Note> {
        self.pending_mark = None;
        let mode = self.settings.mode;
        match action {
            // the status bar's badge reads MARK meanwhile
            MarkAction::Await(pending) => {
                self.pending_mark = Some(pending);
                None
            }
            MarkAction::Cancel => None,
            MarkAction::Add(_) | MarkAction::AddAt(_) => {
                let marks = self.marks();
                let letter = match action {
                    MarkAction::Add(letter) => letter,
                    _ => None,
                };
                let Some(letter) = letter.or_else(|| marks.first_free()) else {
                    return Some(Note::warn("all 26 marks are set: delete one first"));
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
                Some(match added {
                    Some(_) => Note::info(format!("mark {letter}")),
                    None => Note::warn("no line to mark yet"),
                })
            }
            MarkAction::Jump(letter) => {
                let jumped = match mode {
                    Mode::Line => self.line_mode.jump_to_mark(letter),
                    Mode::Terminal => self.terminal_mode.jump_to_mark(letter),
                };
                match jumped {
                    Some(line) => {
                        self.flash_mark(line);
                        None
                    }
                    None => Some(Note::warn(format!("mark {letter} is not set"))),
                }
            }
            MarkAction::Delete(letter) => {
                let removed = match mode {
                    Mode::Line => self.line_mode.remove_mark(letter),
                    Mode::Terminal => self.terminal_mode.remove_mark(letter),
                };
                Some(match removed {
                    Some(_) => Note::info(format!("mark {letter} deleted")),
                    None => Note::warn(format!("mark {letter} is not set")),
                })
            }
            MarkAction::Adjacent(forward) => {
                let jumped = match mode {
                    Mode::Line => self.line_mode.jump_to_adjacent_mark(forward),
                    Mode::Terminal => self.terminal_mode.jump_to_adjacent_mark(forward),
                };
                match jumped.and_then(|letter| self.marks().get(letter)) {
                    Some(line) => {
                        self.flash_mark(line);
                        None
                    }
                    None => Some(Note::info(if forward {
                        "no mark below"
                    } else {
                        "no mark above"
                    })),
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

    /// The chains running as jobs (MAX-283), for `status`
    pub fn jobs(&self) -> &Jobs {
        &self.jobs
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

    /// The presenter's own error toast (`present`, `fail_commands`): a toast comes from
    /// `present` or `emit` only (MAX-165)
    fn notify_error(&mut self, msg: impl Into<String>) {
        self.notify(ToastLevel::Error, msg);
    }

    /// An event published (MAX-165): its note shown, unless it is a silent one, and logged for
    /// the API's subscribers (MAX-284)
    pub fn emit(&mut self, event: Event) {
        if let Some(note) = event.note() {
            self.show_note(note);
        }
        self.events.push(event);
    }

    /// What an action reported (MAX-165), shown: in its slot's box when it has one
    pub fn show_note(&mut self, note: Note) {
        match note.slot {
            Some(slot) => self.notify_keyed(slot, note.level, note.text),
            None => self.notify(note.level, note.text),
        }
    }

    fn notify(&mut self, level: ToastLevel, msg: impl Into<String>) {
        let msg = msg.into();
        let duration = toast_duration(level, &msg);
        let line = (self.notifications.push(msg, level, duration)).map(Notice::line);
        self.write_notice(line);
    }

    /// A toast in the box of `key`: it takes the place of the box's last message, its time
    /// renewed, so what follows on from it, or is cycled through, shows in one box (MAX-234)
    fn notify_keyed(&mut self, key: &'static str, level: ToastLevel, msg: impl Into<String>) {
        let msg = msg.into();
        let duration = toast_duration(level, &msg);
        let line = (self.notifications.push_keyed(key, msg, level, duration)).map(Notice::line);
        self.write_notice(line);
    }

    /// A new line of the notification history into the live session's `.notices` file, the
    /// paused one's while a replay runs (MAX-276). A failed write isn't said: its toast would
    /// fail the same way
    fn write_notice(&mut self, line: Option<String>) {
        if let Some(line) = line
            && let Some(session) = self.live_session_mut()
        {
            let _ = session.write_notice(&line);
        }
    }

    /// A copy made, its note: what it was (`Copied the <what>: <text>`) or how many chars, in
    /// the copy's box; none with `notify_copy` off, a clipboard error still shows (MAX-269)
    pub fn copied_note(&self, text: &str, what: Option<&str>) -> Option<Note> {
        if !self.settings.ui.notify_copy {
            return None;
        }
        let msg = match what {
            Some(what) => format!("Copied the {what}: {text}"),
            None => format!("Copied {} chars to clipboard", text.chars().count()),
        };
        Some(Note::new(ToastLevel::Success, msg).in_slot("copy"))
    }
}

/// A search that went around the history's end, said which way (MAX-218)
/// An empty store of the bytes received, `rx_bytes` long (MAX-290)
fn rx_store(settings: &SettingsConfig) -> RxStream {
    RxStream::new(usize::try_from(settings.rx_bytes.0).unwrap_or(usize::MAX))
}

fn went_around_note(forward: bool) -> Note {
    if forward {
        Note::info("Search hit the end, went on from the top")
    } else {
        Note::info("Search hit the top, went on from the end")
    }
}

/// A setting's note: cycling it shows one box with its last value (MAX-234)
fn setting_note(key: &'static str, text: impl Into<String>) -> Note {
    Note::info(text).in_slot(key)
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
/// The end of a download (MAX-228): the file or how many, and the folder once. A YMODEM
/// batch can hold none
fn received_event(transfer: &FileTransfer) -> Event {
    let folder = transfer.folder().map(|folder| folder.display().to_string());
    let folder = folder.unwrap_or_default();
    match (transfer.received_count(), transfer.last_received()) {
        (0, _) => Event::NothingReceived,
        (1, Some((path, size))) => Event::ReceivedFile {
            name: file_name(path),
            size: *size,
            folder,
        },
        (count, _) => Event::ReceivedFiles {
            count,
            size: transfer.received_total(),
            folder,
        },
    }
}

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
