use std::path::{Path, PathBuf};

/// Module to reduce inter modules exchange or something
use crossterm::event::KeyCode;

use crate::{
    capture::{CaptureDevice, Session, Stream},
    commands::Invocation,
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
    Marks(MarkBrowser),
    Directory(Directory),
    LinkForm(Box<LinkForm>),
    DeleteLink(SavedLink),
}

/// A part of a popup the mouse picks (MAX-113)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupTarget {
    HelpTab(HelpCategory),
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
    MarkRow(usize),
    MarkJump,
    MarkDelete,
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
    HandledNotify {
        message: String,
        warn: bool,
    },
    HandledCopy {
        text: String,
        what: String,
    },
    ToggleTrigger(Option<String>),
    Open(Address),
    OpenLink(LinkConfig),
    NameLink(Option<Address>),
    NewLink,
    EditLink(SavedLink),
    AskDeleteLink(SavedLink),
    DeleteLink(SavedLink),
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
        )
    }

    /// A plain letter key goes to the popup before the popup keys (`q`, `j`...): the marks
    /// popup jumps to the mark of that letter (MAX-164)
    pub fn takes_letters(&self) -> bool {
        matches!(self, Overlay::Marks(_))
    }

    pub fn closes_on_click_outside(&self) -> bool {
        !matches!(
            self,
            Overlay::DeviceDisconnected
                | Overlay::DeviceDetached
                | Overlay::NoDevice
                | Overlay::RestoreSession(_)
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
            Overlay::Marks(browser) => browser.handle_action(action),
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
    AddCr,
    AddLf,
    TxEol,
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
