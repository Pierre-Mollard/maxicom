//! Events (MAX-165): what happens in the session without anyone asking at that moment, the
//! link lost, a transfer done, a session restored, published with `App::emit`: the user's toast
//! and the notification history, and the API's subscribers (MAX-284), who get it as JSON
//! from the log. A result answers an action instead (`commands::report`). Each event says its
//! own note, so its wording lives here and every listener reads the same
use std::collections::VecDeque;

use jiff::Timestamp;
use serde::Serialize;

use crate::{
    commands::{Origin, report::Note},
    link::serial::FlowControl,
    notification::ToastLevel,
};

/// Something that happened in the session on its own, with what its note says. In JSON its
/// kind's name (`link-opened`) and its fields (MAX-284)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Event {
    /// The link opened, `label` its name or address
    LinkOpened { label: String, how: Opened },
    /// The link opened while another program holds it too (a server taking several clients)
    LinkShared { label: String, holder: String },
    /// The link didn't open at start, `error` why
    LinkNotOpened { error: String },
    /// A detached link didn't attach again, `error` why
    ReattachFailed { error: String },
    /// The link closed under a read or a write, `link` its kind's name
    LinkLost { link: &'static str },
    /// The link was quiet for its inactivity timeout and got detached
    LinkIdle { secs: u64 },
    /// A write failed, the link kept: said once until it writes again
    WriteFailed { link: &'static str, error: String },
    /// A read failed with an error nothing handles: the loop ends
    ReadFailed { link: &'static str },
    /// A break failed at its end
    BreakFailed { error: String },
    /// The far end said something wrong (an RFC 2217 server's answer)
    LinkWarning { text: String },
    /// A file sent, `protocol` the one it went with, none for a raw send
    Sent {
        name: String,
        size: u64,
        protocol: Option<&'static str>,
    },
    /// A file of a download came in, named as it comes (a YMODEM batch)
    FileReceived { name: String, size: u64 },
    /// A download ended with no file: the sender sent none
    NothingReceived,
    /// A download ended with its one file, in `folder`
    ReceivedFile {
        name: String,
        size: u64,
        folder: String,
    },
    /// A download ended with `count` files of `size` bytes in all, in `folder`
    ReceivedFiles {
        count: usize,
        size: u64,
        folder: String,
    },
    /// A transfer stopped before its end: `stopped` says which and where, `why` the cause
    TransferStopped { stopped: String, why: String },
    /// A trigger's text came in and its command line runs; a silent one says nothing
    TriggerFired {
        name: String,
        run: String,
        silent: bool,
    },
    /// Bytes the terminal doesn't handle came in: the kinds most seen with their counts, and
    /// how many kinds more
    Unhandled {
        kinds: Vec<(String, usize)>,
        more: usize,
    },
    /// The device copied to the user's clipboard (OSC 52), `base64_len` long as it sent it
    DeviceCopied { base64_len: usize },
    /// The device answered like a shell (a terminal query) in line mode: `key` switches to
    /// terminal mode, for its line editing and history
    ShellSeen { key: String },
    /// A replay read its whole file
    ReplayFinished,
    /// The session folder was full: its oldest sessions removed, `message` says which
    SessionsPruned { message: String },
    /// The session backup didn't start, `error` why: the app runs on without it
    BackupOff { error: String },
    /// A past session restored into the history, `span` when it ran
    SessionRestored { span: String },
    /// A past session not restored, `error` why
    RestoreFailed { error: String },
    /// Too much came in while the restore dialog asked: no restore
    RestoreOverflowed,
    /// A full recording stopped: the device it records changed
    RecordingStopped,
    /// A group of sessions loaded (MAX-202), `message` saying what didn't fit, a warning when
    /// line mode misses something too
    GroupLoaded { message: String, warn: bool },
    /// A config file read with something wrong in it: at start, on a reload, a profile laid,
    /// `links.toml`, a key no one can reach, a hook that doesn't parse
    ConfigWarning { text: String },
    /// The device's clipboard copy (OSC 52) didn't make it to the clipboard, `error` why
    DeviceCopyFailed { error: String },
    /// The screen is too small for a part of the layout: `text` says which
    ScreenWarning { text: String },
    // what reaches the device or the link (MAX-284), silent: the one who did it was told by
    // its result. An action names its `cause`; what comes of it later (`open-failed`,
    // `mash-done`) doesn't, as `link-opened`
    /// Line settings set: the baud rate, the framing (`8N1`) and the flow control
    LineSettings {
        baud: u32,
        framing: String,
        flow: FlowControl,
        cause: Cause,
    },
    /// A control line set, `line` DTR or RTS
    LineLevel {
        line: &'static str,
        on: bool,
        cause: Cause,
    },
    /// A break started, held `ms` long
    BreakSent { ms: u64, cause: Cause },
    /// The link detached by hand
    LinkDetached { cause: Cause },
    /// The link opening, `label` the one opened (`open`, the directory, an attach): what it
    /// gives comes as its own event
    LinkOpening { label: String, cause: Cause },
    /// A setting changed, `setting` its name as `get` and `set` know it, `value` what `get`
    /// gives now
    SettingChanged {
        setting: &'static str,
        value: SettingValue,
        cause: Cause,
    },
    /// An `open` (or a pick in the directory) failed: `error` says why, naming what it tried;
    /// `back` the link it went back to, none before the first pick
    OpenFailed { error: String, back: Option<String> },
    /// A replay in place of the link: a file, or a group of `sessions` loaded at once, `path`
    /// the newest's
    ReplayStarted {
        path: String,
        sessions: usize,
        cause: Cause,
    },
    /// The replay stopped: the live link it set aside opens again, said by `link-opening`
    ReplayStopped { cause: Cause },
    /// The replay paused, resumed, restarted or its delay set (`delay_ms`)
    ReplayControl {
        control: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        delay_ms: Option<u64>,
        cause: Cause,
    },
    /// A file transfer started, `direction` send or receive: a protocol's waits for the other
    /// side, its end comes as its own event
    TransferStarted {
        direction: &'static str,
        name: String,
        size: Option<u64>,
        protocol: Option<&'static str>,
        cause: Cause,
    },
    /// A mash started: its text sent again and again until `until` comes in
    MashStarted { until: String, cause: Cause },
    /// The mash's text came in: it ended
    MashDone { until: String },
    /// The mash ended before its text came in, `why`
    MashStopped { why: String },
}

/// A setting's value in an event (MAX-284): on or off, or its word
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum SettingValue {
    /// an on/off setting
    On(bool),
    /// the others: a config word, a plot's name
    Word(String),
}

/// Who did what an event says (MAX-284): its origin's kind as `status` names it, with the
/// trigger's or the profile hook's name
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Cause {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

impl Cause {
    /// The link itself, not an origin: what an open sets on its own (the DTR and RTS levels
    /// of its line)
    pub fn link() -> Self {
        Self {
            kind: "link",
            name: None,
        }
    }
}

impl From<&Origin> for Cause {
    fn from(origin: &Origin) -> Self {
        Self {
            kind: origin.kind(),
            name: origin.name().map(str::to_string),
        }
    }
}

/// How the link came to open, as its note says it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Opened {
    /// At start
    Connected,
    /// Attached again, or reopened after it was lost
    Reconnected,
    /// Switched to, by `open` or the directory
    Switched,
}

/// Every event's kind, as `kind()` names it: what a subscriber may ask for (MAX-284)
pub const KINDS: [&str; 45] = [
    "link-opened",
    "link-shared",
    "link-not-opened",
    "reattach-failed",
    "link-lost",
    "link-idle",
    "write-failed",
    "read-failed",
    "break-failed",
    "link-warning",
    "sent",
    "file-received",
    "nothing-received",
    "received-file",
    "received-files",
    "transfer-stopped",
    "trigger-fired",
    "unhandled",
    "device-copied",
    "shell-seen",
    "replay-finished",
    "sessions-pruned",
    "backup-off",
    "session-restored",
    "restore-failed",
    "restore-overflowed",
    "recording-stopped",
    "group-loaded",
    "config-warning",
    "device-copy-failed",
    "screen-warning",
    "line-settings",
    "line-level",
    "break-sent",
    "link-detached",
    "link-opening",
    "setting-changed",
    "open-failed",
    "replay-started",
    "replay-stopped",
    "replay-control",
    "transfer-started",
    "mash-started",
    "mash-done",
    "mash-stopped",
];

impl Event {
    /// Its kind's name, the same as in its JSON
    pub fn kind(&self) -> &'static str {
        match self {
            Self::LinkOpened { .. } => "link-opened",
            Self::LinkShared { .. } => "link-shared",
            Self::LinkNotOpened { .. } => "link-not-opened",
            Self::ReattachFailed { .. } => "reattach-failed",
            Self::LinkLost { .. } => "link-lost",
            Self::LinkIdle { .. } => "link-idle",
            Self::WriteFailed { .. } => "write-failed",
            Self::ReadFailed { .. } => "read-failed",
            Self::BreakFailed { .. } => "break-failed",
            Self::LinkWarning { .. } => "link-warning",
            Self::Sent { .. } => "sent",
            Self::FileReceived { .. } => "file-received",
            Self::NothingReceived => "nothing-received",
            Self::ReceivedFile { .. } => "received-file",
            Self::ReceivedFiles { .. } => "received-files",
            Self::TransferStopped { .. } => "transfer-stopped",
            Self::TriggerFired { .. } => "trigger-fired",
            Self::Unhandled { .. } => "unhandled",
            Self::DeviceCopied { .. } => "device-copied",
            Self::ShellSeen { .. } => "shell-seen",
            Self::ReplayFinished => "replay-finished",
            Self::SessionsPruned { .. } => "sessions-pruned",
            Self::BackupOff { .. } => "backup-off",
            Self::SessionRestored { .. } => "session-restored",
            Self::RestoreFailed { .. } => "restore-failed",
            Self::RestoreOverflowed => "restore-overflowed",
            Self::RecordingStopped => "recording-stopped",
            Self::GroupLoaded { .. } => "group-loaded",
            Self::ConfigWarning { .. } => "config-warning",
            Self::DeviceCopyFailed { .. } => "device-copy-failed",
            Self::ScreenWarning { .. } => "screen-warning",
            Self::LineSettings { .. } => "line-settings",
            Self::LineLevel { .. } => "line-level",
            Self::BreakSent { .. } => "break-sent",
            Self::LinkDetached { .. } => "link-detached",
            Self::LinkOpening { .. } => "link-opening",
            Self::SettingChanged { .. } => "setting-changed",
            Self::OpenFailed { .. } => "open-failed",
            Self::ReplayStarted { .. } => "replay-started",
            Self::ReplayStopped { .. } => "replay-stopped",
            Self::ReplayControl { .. } => "replay-control",
            Self::TransferStarted { .. } => "transfer-started",
            Self::MashStarted { .. } => "mash-started",
            Self::MashDone { .. } => "mash-done",
            Self::MashStopped { .. } => "mash-stopped",
        }
    }

    /// What the user is shown: the level, the box (the link's or the transfer's for what
    /// follows on from its last message, MAX-228) and the text; none for a silent one
    pub fn note(&self) -> Option<Note> {
        let link = |level, text: String| Note::new(level, text).in_slot("link");
        let transfer = |level, text: String| Note::new(level, text).in_slot("transfer");
        let note = match self {
            Self::LinkOpened { label, how } => {
                let done = match how {
                    Opened::Connected => "Connected to",
                    Opened::Reconnected => "Reconnected to",
                    Opened::Switched => "Opened",
                };
                link(ToastLevel::Success, format!("{done} {label}"))
            }
            // in a box of its own, as the link's box says it opened right after (MAX-186)
            Self::LinkShared { label, holder } => {
                Note::warn(format!("{label} is open in {holder} too"))
            }
            Self::LinkNotOpened { error } => link(ToastLevel::Warn, error.clone()),
            Self::ReattachFailed { error } => {
                link(ToastLevel::Error, format!("Failed to reattach: {error}"))
            }
            Self::LinkLost { link: kind } => link(ToastLevel::Warn, format!("{kind} lost")),
            Self::LinkIdle { secs } => link(
                ToastLevel::Warn,
                format!("Device idle for {secs}s: port detached"),
            ),
            Self::WriteFailed { link: kind, error } => {
                link(ToastLevel::Error, format!("{kind}: {error}"))
            }
            Self::ReadFailed { link: kind } => {
                link(ToastLevel::Error, format!("{kind} read error"))
            }
            Self::BreakFailed { error } => Note::new(ToastLevel::Error, format!("break: {error}")),
            Self::LinkWarning { text } => Note::warn(text.clone()),
            Self::Sent {
                name,
                size,
                protocol,
            } => {
                let protocol = protocol.map(|p| format!(", {p}")).unwrap_or_default();
                transfer(
                    ToastLevel::Success,
                    format!("Sent {name} ({size} bytes{protocol})"),
                )
            }
            // by name as they come: the folder is said once, at the end (MAX-228)
            Self::FileReceived { name, size } => transfer(
                ToastLevel::Success,
                format!("Received {name} ({size} bytes)"),
            ),
            Self::NothingReceived => transfer(
                ToastLevel::Info,
                "Nothing received: the sender sent no file".to_string(),
            ),
            Self::ReceivedFile { name, size, folder } => transfer(
                ToastLevel::Success,
                format!("Received {name} ({size} bytes) into {folder}"),
            ),
            Self::ReceivedFiles {
                count,
                size,
                folder,
            } => transfer(
                ToastLevel::Success,
                format!("Received {count} files ({size} bytes) into {folder}"),
            ),
            Self::TransferStopped { stopped, why } => {
                transfer(ToastLevel::Error, format!("{stopped}: {why}"))
            }
            // a silent one leaves it to its command line (`notify`, MAX-228)
            Self::TriggerFired { name, run, silent } => {
                return (!silent).then(|| Note::info(format!("Trigger {name}: {run}")));
            }
            // in a box of its own, its counts going on while it shows (`App::unhandled`)
            Self::Unhandled { kinds, more } => {
                let mut parts: Vec<String> = (kinds.iter())
                    .map(|(label, count)| match count {
                        1 => label.clone(),
                        _ => format!("{label} ×{count}"),
                    })
                    .collect();
                match more {
                    0 => {}
                    1 => parts.push("1 more kind".to_string()),
                    more => parts.push(format!("{more} more kinds")),
                }
                Note::info(format!("Unhandled: {}", parts.join(", "))).in_slot("unhandled")
            }
            // said every time, `notify_copy` or not, as the user didn't ask for it (MAX-275)
            Self::DeviceCopied { base64_len } => {
                let bytes = base64_len / 4 * 3;
                let text = format!("The device copied about {bytes} bytes to your clipboard");
                Note::info(text).in_slot("copy")
            }
            Self::ShellSeen { key } => Note::info(format!(
                "The device looks like a shell (it sent a terminal query): {key} switches to \
                 terminal mode for its line editing and history"
            ))
            .in_slot("shell"),
            Self::ReplayFinished => Note::info("Replay finished"),
            Self::SessionsPruned { message } => Note::info(message.clone()),
            Self::BackupOff { error } => Note::warn(format!("Session backup off: {error}")),
            Self::SessionRestored { span } => Note::info(format!("Restored the session of {span}")),
            Self::RestoreFailed { error } => Note::warn(format!("Session not restored: {error}")),
            Self::RestoreOverflowed => {
                Note::warn("Session not restored: too much data came in while asking")
            }
            Self::RecordingStopped => Note::warn("Full recording stopped: the device changed"),
            Self::GroupLoaded { message, warn } => match warn {
                true => Note::warn(message.clone()),
                false => Note::info(message.clone()),
            },
            Self::ConfigWarning { text } | Self::ScreenWarning { text } => Note::warn(text.clone()),
            Self::DeviceCopyFailed { error } => {
                Note::new(ToastLevel::Error, format!("Clipboard error: {error}")).in_slot("copy")
            }
            Self::LineSettings { .. }
            | Self::LineLevel { .. }
            | Self::BreakSent { .. }
            | Self::LinkDetached { .. }
            | Self::LinkOpening { .. }
            | Self::SettingChanged { .. }
            | Self::OpenFailed { .. }
            | Self::ReplayStarted { .. }
            | Self::ReplayStopped { .. }
            | Self::ReplayControl { .. }
            | Self::TransferStarted { .. }
            | Self::MashStarted { .. }
            | Self::MashDone { .. }
            | Self::MashStopped { .. } => return None,
        };
        Some(note)
    }
}

/// An event as the log keeps it: its number, counted from 1 since the start, and its time
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Logged {
    pub seq: u64,
    pub time: Timestamp,
    pub event: Event,
}

/// The last events emitted (MAX-284), for the API's subscribers: a client that comes back asks
/// from the last number it saw
#[derive(Debug, Default)]
pub struct EventLog {
    events: VecDeque<Logged>,
    last: u64,
}

/// The events the log keeps
const LOG_SIZE: usize = 1000;

impl EventLog {
    pub fn push(&mut self, event: Event) {
        self.last += 1;
        if self.events.len() == LOG_SIZE {
            self.events.pop_front();
        }
        self.events.push_back(Logged {
            seq: self.last,
            time: Timestamp::now(),
            event,
        });
    }

    /// The number of the last event, 0 before the first
    pub fn last(&self) -> u64 {
        self.last
    }

    /// The number of the oldest event kept: past `last` when none is
    pub fn oldest(&self) -> u64 {
        self.events
            .front()
            .map_or(self.last + 1, |logged| logged.seq)
    }

    /// The events kept after number `seq`, oldest first
    pub fn after(&self, seq: u64) -> impl Iterator<Item = &Logged> {
        let skip = seq.saturating_sub(self.oldest().saturating_sub(1));
        self.events
            .iter()
            .skip(usize::try_from(skip).unwrap_or(usize::MAX))
    }
}

#[cfg(test)]
#[path = "tests/event.rs"]
mod tests;
