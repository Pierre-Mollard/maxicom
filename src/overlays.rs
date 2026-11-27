use std::path::{Path, PathBuf};

/// Module to reduce inter modules exchange or something
use crossterm::event::KeyCode;

use crate::{
    app::types::SidePanel,
    capture::{CaptureDevice, Session, Stream},
    commands::{Invocation, jobs::JobId},
    configs::links::SavedLink,
    configs::{AppConfig, KeyBindingAction, SettingsConfig, keybindings::KeyBindingDialogAction},
    inputs::DialogAction,
    link::{Address, LinkConfig, serial::LineSettings},
};

// Re-exports
pub use command_line::CommandLine;
pub use directory::{Directory, DirectoryTab};
pub use help::{HelpCategory, HelpDialog};
pub use link_form::{LinkForm, LinkFormField};
pub use marks::{MarkBrowser, MarkRow};
pub use menu::{ActionMenu, MenuRun};
pub use misc::{ErrorDialog, FilePathDialog, ScreenshotDialog};
pub use notices::NoticeBrowser;
pub use profiles::ProfileBrowser;
pub use query::QueryView;
pub use reconfigure::{LineSettingField, LineSettingsDialog};
pub use resize::{ResizeDialog, ResizeField};
pub use sessions::SessionBrowser;
pub use settings::SettingsDialog;
pub use triggers::TriggerBrowser;

pub mod command_line;
pub mod directory;
pub mod help;
pub mod link_form;
pub mod marks;
pub mod menu;
pub mod misc;
pub mod notices;
pub mod profiles;
pub mod query;
pub mod reconfigure;
pub mod resize;
pub mod sessions;
pub mod settings;
pub mod triggers;

#[derive(Debug, Clone)]
pub enum Overlay {
    Help(HelpDialog),
    Settings(SettingsDialog),
    TerminalSize(ResizeDialog),
    LinkSettings(LineSettingsDialog),
    FilePath(FilePathDialog),
    Screenshot(ScreenshotDialog),
    DeviceDisconnected,
    DeviceDetached,
    NoDevice,
    Error(ErrorDialog),
    ConfirmQuit,
    ActionMenu(ActionMenu),
    RestoreSession(Box<Session>),
    Sessions(Box<SessionBrowser>),
    CommandLine(Box<CommandLine>),
    Triggers(TriggerBrowser),
    Profiles(ProfileBrowser),
    Marks(MarkBrowser),
    Notices(NoticeBrowser),
    /// A query's answer (MAX-165)
    Query(QueryView),
    Directory(Directory),
    LinkForm(Box<LinkForm>),
    DeleteLink(SavedLink),
    /// A group replay loading (MAX-272): its state is the replay's. Close stops it
    Loading,
    /// A client's command the `[api]` levels ask the user about (MAX-287), by its job: the
    /// app's first ask
    ClientAsk(JobId),
}

/// The user's answer to a client's command asked about (MAX-287)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskAnswer {
    /// Runs it, this once
    Send,
    /// Runs it, and the client's next commands of the same access without asking, until it
    /// disconnects
    Always,
    /// Refuses it: its chain stops
    Refuse,
}

/// A part of a popup the mouse picks (MAX-113)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupTarget {
    HelpTab(HelpCategory),
    /// The help's `/` hint: types a filter (MAX-274)
    HelpFilter,
    Setting(SettingKey),
    LineField(LineSettingField),
    LinePrev(LineSettingField),
    LineNext(LineSettingField),
    ResizeField(ResizeField),
    Hint(KeyBindingDialogAction),
    MenuItem(usize),
    CaptureStream,
    CaptureSession,
    ScreenshotFormat,
    SessionRow(usize),
    SessionReplay,
    SessionBackToLive,
    SessionDelete,
    SessionDeleteAll,
    SessionName,
    SessionView,
    SessionCopyLog,
    SessionCopyFolder,
    SessionReload,
    SessionConfirm,
    SessionCancel,
    Completion(usize),
    TriggerRow(usize),
    TriggerToggle,
    TriggerToggleAll,
    TriggerCopyPath,
    ProfileRow(usize),
    ProfileToggle,
    /// Up the stack (`true`), or down
    ProfileMove(bool),
    ProfileCopyPath,
    MarkRow(usize),
    MarkJump,
    MarkDelete,
    NoticeRow(usize),
    NoticeCopy,
    /// A line of a query's answer (MAX-165)
    QueryRow(usize),
    /// The query popup's Copy button: the selected line
    QueryCopy,
    /// The query popup's Copy all button: the whole answer
    QueryCopyAll,
    /// The ask popup's always button (MAX-287)
    AskAlways,
    ConfigPath(usize),
    DirectoryTab(DirectoryTab),
    DirectoryRow(usize),
    DirectoryOpen,
    DirectoryRefresh,
    DirectoryTest,
    DirectorySave,
    DirectoryNew,
    DirectoryEdit,
    DirectoryDelete,
    LinkFormField(LinkFormField),
    OpenDirectory,
}

pub enum OverlayResponse {
    Handled,
    Close,
    ToggleSetting(SettingKey),
    ResizeTerminal {
        rows: u16,
        cols: u16,
    },
    Reconfigure(LineSettings),
    Reattach,
    StartCapture {
        path: PathBuf,
        stream: Stream,
        include_session: bool,
    },
    SaveScreenshot {
        path: PathBuf,
        contents: String,
    },
    Quit,
    Run(KeyBindingAction),
    ClosePanel(SidePanel),
    QueueCommands {
        line: String,
        invocations: Vec<Invocation>,
    },
    RestoreSession(bool),
    ReplaySession {
        path: PathBuf,
        device: Option<CaptureDevice>,
    },
    ReplayGroup(Vec<Session>),
    StopReplay,
    NameRunningSession(String),
    HandledNotify {
        message: String,
        warn: bool,
    },
    HandledCopy {
        text: String,
        what: String,
    },
    ToggleTrigger(Option<String>),
    ToggleProfile(String),
    SetProfile {
        name: String,
        on: bool,
    },
    MoveProfile {
        name: String,
        up: bool,
    },
    Open(Address),
    OpenLink(LinkConfig),
    NameLink(Option<Address>),
    NewLink,
    EditLink(SavedLink),
    AskDeleteLink(SavedLink),
    DeleteLink(SavedLink),
    AnswerAsk(AskAnswer),
    SaveLink {
        link: SavedLink,
        replaces: Option<SavedLink>,
    },
    TestLinks,
    ArmTrigger {
        name: String,
        on: bool,
    },
    JumpMark(char),
    DeleteMark(char),
    AddMark(Option<usize>),
}

impl Overlay {
    pub fn is_text_input(&self) -> bool {
        matches!(
            self,
            Overlay::FilePath(_)
                | Overlay::Screenshot(_)
                | Overlay::TerminalSize(_)
                | Overlay::CommandLine(_)
                | Overlay::LinkForm(_)
        ) || matches!(self, Overlay::Sessions(browser) if browser.naming.is_some())
            || matches!(self, Overlay::Help(help) if help.typing)
    }

    /// A plain letter key goes to the popup before the popup keys (`q`, `j`...): the marks
    /// popup jumps to the mark of that letter (MAX-164)
    pub fn takes_letters(&self) -> bool {
        matches!(self, Overlay::Marks(_))
    }

    /// The link's own popups, up while it waits (no device picked yet, detached, lost): a
    /// client's ask shows in their place, and they come back after it (MAX-285)
    pub fn is_link_wait(&self) -> bool {
        matches!(
            self,
            Overlay::NoDevice | Overlay::DeviceDetached | Overlay::DeviceDisconnected
        )
    }

    pub fn closes_on_click_outside(&self) -> bool {
        !matches!(
            self,
            Overlay::DeviceDisconnected
                | Overlay::DeviceDetached
                | Overlay::NoDevice
                | Overlay::RestoreSession(_)
                | Overlay::Loading
                | Overlay::ClientAsk(_)
        )
    }

    pub fn handle_action(
        &mut self,
        action: DialogAction,
        settings: &SettingsConfig,
    ) -> OverlayResponse {
        match self {
            Overlay::Help(dialog) => dialog.handle_action(action, settings),
            Overlay::LinkSettings(dialog) => dialog.handle_action(action),
            Overlay::TerminalSize(dialog) => dialog.handle_action(action),
            Overlay::FilePath(dialog) => dialog.handle_action(action),
            Overlay::Screenshot(dialog) => dialog.handle_action(action),
            Overlay::ActionMenu(menu) => menu.handle_action(action),
            Overlay::Sessions(browser) => browser.handle_action(action),
            Overlay::CommandLine(command_line) => command_line.handle_action(action),
            Overlay::Triggers(browser) => browser.handle_action(action),
            Overlay::Profiles(browser) => browser.handle_action(action),
            Overlay::Marks(browser) => browser.handle_action(action),
            Overlay::Notices(browser) => browser.handle_action(action),
            Overlay::Query(view) => view.handle_action(action),
            Overlay::Directory(directory) => directory.handle_action(action),
            Overlay::LinkForm(form) => form.handle_action(action),
            Overlay::Error(_) => match action {
                DialogAction::Close | DialogAction::Confirm => OverlayResponse::Close,
                _ => OverlayResponse::Handled,
            },
            Overlay::RestoreSession(_) => match action {
                DialogAction::Confirm => OverlayResponse::RestoreSession(true),
                DialogAction::Close => OverlayResponse::RestoreSession(false),
                DialogAction::HandleKey(key) => match key.code {
                    KeyCode::Char('y' | 'Y') => OverlayResponse::RestoreSession(true),
                    KeyCode::Char('n' | 'N') => OverlayResponse::RestoreSession(false),
                    _ => OverlayResponse::Handled,
                },
                _ => OverlayResponse::Handled,
            },
            // a saved link deleted, asked first; no goes back to the directory (MAX-185)
            Overlay::DeleteLink(link) => match action {
                DialogAction::Confirm => OverlayResponse::DeleteLink(link.clone()),
                DialogAction::Close => OverlayResponse::Run(KeyBindingAction::Directory),
                DialogAction::HandleKey(key) => match key.code {
                    KeyCode::Char('y' | 'Y') => OverlayResponse::DeleteLink(link.clone()),
                    KeyCode::Char('n' | 'N') => OverlayResponse::Run(KeyBindingAction::Directory),
                    _ => OverlayResponse::Handled,
                },
                _ => OverlayResponse::Handled,
            },
            // a client's command: y runs it, a always for that client, n or close refuses
            // (MAX-287)
            Overlay::ClientAsk(_) => match action {
                DialogAction::Confirm => OverlayResponse::AnswerAsk(AskAnswer::Send),
                DialogAction::Close => OverlayResponse::AnswerAsk(AskAnswer::Refuse),
                DialogAction::Pick(PopupTarget::AskAlways) => {
                    OverlayResponse::AnswerAsk(AskAnswer::Always)
                }
                DialogAction::HandleKey(key) => match key.code {
                    KeyCode::Char('y' | 'Y') => OverlayResponse::AnswerAsk(AskAnswer::Send),
                    KeyCode::Char('a' | 'A') => OverlayResponse::AnswerAsk(AskAnswer::Always),
                    KeyCode::Char('n' | 'N') => OverlayResponse::AnswerAsk(AskAnswer::Refuse),
                    _ => OverlayResponse::Handled,
                },
                _ => OverlayResponse::Handled,
            },
            Overlay::ConfirmQuit => match action {
                DialogAction::Confirm => OverlayResponse::Quit,
                DialogAction::Close => OverlayResponse::Close,
                DialogAction::HandleKey(key) => match key.code {
                    KeyCode::Char('y' | 'Y') => OverlayResponse::Quit,
                    KeyCode::Char('n' | 'N') => OverlayResponse::Close,
                    _ => OverlayResponse::Handled,
                },
                _ => OverlayResponse::Handled,
            },
            // the directory from the link's popups, to pick another device (MAX-181)
            Overlay::DeviceDisconnected | Overlay::DeviceDetached if opens_directory(&action) => {
                OverlayResponse::Run(KeyBindingAction::Directory)
            }
            Overlay::DeviceDisconnected => match action {
                DialogAction::Close | DialogAction::Confirm => OverlayResponse::Quit,
                _ => OverlayResponse::Handled,
            },
            Overlay::NoDevice => match action {
                DialogAction::Confirm => OverlayResponse::Run(KeyBindingAction::Directory),
                DialogAction::Close => OverlayResponse::Quit,
                _ => OverlayResponse::Handled,
            },
            Overlay::DeviceDetached => match action {
                DialogAction::Confirm => OverlayResponse::Reattach,
                DialogAction::Close => OverlayResponse::Quit,
                _ => OverlayResponse::Handled,
            },
            Overlay::Settings(dialog) => dialog.handle_action(action, settings),
            // stops the load and its replay: back to the live link (MAX-272)
            Overlay::Loading => match action {
                DialogAction::Close => OverlayResponse::StopReplay,
                _ => OverlayResponse::Handled,
            },
        }
    }
}

/// All settings types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKey {
    LocalEcho,
    Mode,
    LineWrap,
    IdentWrap,
    PrefixPlace,
    DisplayTimestamp,
    TextEncoding,
    DisplayMode,
    DisplayRaw,
    Mouse,
    DimensionMode,
    AnswerQueries,
    ByteTrace,
    AddCr,
    AddLf,
    TxEol,
    Backspace,
    Plot,
}

impl SettingKey {
    /// Every setting, in the settings popup's order (MAX-283)
    pub const ALL: [Self; 18] = [
        Self::Mode,
        Self::LocalEcho,
        Self::DisplayTimestamp,
        Self::TextEncoding,
        Self::Mouse,
        Self::DisplayRaw,
        Self::Plot,
        Self::AddCr,
        Self::AddLf,
        Self::TxEol,
        Self::LineWrap,
        Self::IdentWrap,
        Self::PrefixPlace,
        Self::DisplayMode,
        Self::DimensionMode,
        Self::AnswerQueries,
        Self::ByteTrace,
        Self::Backspace,
    ];

    /// Their names, as `get` and `set` take them
    pub const NAMES: [&'static str; 18] = {
        let mut names = [""; 18];
        let mut i = 0;
        while i < names.len() {
            names[i] = Self::ALL[i].name();
            i += 1;
        }
        names
    };

    /// Its name for the commands (MAX-283): `set`'s, a command's (`mode`, `encoding`), else
    /// its toggle's or its popup label's
    pub const fn name(self) -> &'static str {
        match self {
            Self::LocalEcho => "local-echo",
            Self::Mode => "mode",
            Self::LineWrap => "line-wrap",
            Self::IdentWrap => "indent-wrap",
            Self::PrefixPlace => "prefix-place",
            Self::DisplayTimestamp => "timestamps",
            Self::TextEncoding => "encoding",
            Self::DisplayMode => "display-mode",
            Self::DisplayRaw => "display-raw",
            Self::Mouse => "mouse",
            Self::DimensionMode => "size-mode",
            Self::AnswerQueries => "answer-queries",
            Self::ByteTrace => "byte-trace",
            Self::AddCr => "add-cr",
            Self::AddLf => "add-lf",
            Self::TxEol => "tx-eol",
            Self::Backspace => "backspace",
            Self::Plot => "plot",
        }
    }

    /// The values `set` takes for it (MAX-219), as `get` gives them: `on`/`off` for a switch;
    /// none for the ones of their own command (`mode`, `encoding`, `plot`)
    pub const fn values(self) -> &'static [&'static str] {
        match self {
            Self::LocalEcho
            | Self::DisplayTimestamp
            | Self::Mouse
            | Self::AnswerQueries
            | Self::ByteTrace
            | Self::AddCr
            | Self::AddLf => &["on", "off"],
            Self::LineWrap => &["word", "char", "truncate"],
            Self::IdentWrap => &["none", "prefix", "timestamp"],
            Self::PrefixPlace => &["inset", "border", "none"],
            Self::DisplayMode => &["pretty", "clean", "raw", "hex", "octal", "binary"],
            Self::DisplayRaw => &["off", "hex", "octal", "binary"],
            Self::DimensionMode => &["auto", "grow_only", "fixed_cols", "match_view", "fixed"],
            Self::TxEol => &["crlf", "cr", "lf"],
            Self::Backspace => &["del", "bs"],
            Self::Mode | Self::TextEncoding | Self::Plot => &[],
        }
    }

    /// The setting of that name
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.name() == name)
    }
}

/// `d`, or a click on its hint, in the link's popups (MAX-181)
fn opens_directory(action: &DialogAction) -> bool {
    match action {
        DialogAction::Pick(PopupTarget::OpenDirectory) => true,
        DialogAction::HandleKey(key) => key.code == KeyCode::Char('d'),
        _ => false,
    }
}

/// The path of `what` to the clipboard, for a look at the files out of the app (MAX-154)
pub fn copy_path(path: &Path, what: &str) -> OverlayResponse {
    OverlayResponse::HandledCopy {
        text: path.display().to_string(),
        what: format!("{what} path"),
    }
}

/// The `index`th of the config folder and the files out of it, as the help and the settings
/// list them (MAX-163)
pub fn copy_config_path(settings: &SettingsConfig, index: usize) -> OverlayResponse {
    match AppConfig::config_paths(settings).into_iter().nth(index) {
        Some((what, path)) => copy_path(&path, what),
        None => OverlayResponse::Handled,
    }
}
