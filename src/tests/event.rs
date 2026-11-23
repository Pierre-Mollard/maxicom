use super::*;

/// MAX-165 (E1): each link event says the note the user saw before events: the same level,
/// box and words; the shared link and a break's failure in no box, the link's others in the
/// link's box.
/// How: one event of each kind.
#[test]
fn link_events_say_their_notes() {
    let link = |level, text: &str| Note::new(level, text).in_slot("link");
    let label = "/dev/ttyUSB0".to_string();
    let opened = |how| Event::LinkOpened {
        label: label.clone(),
        how,
    };
    let cases = [
        (
            opened(Opened::Connected),
            link(ToastLevel::Success, "Connected to /dev/ttyUSB0"),
        ),
        (
            opened(Opened::Reconnected),
            link(ToastLevel::Success, "Reconnected to /dev/ttyUSB0"),
        ),
        (
            opened(Opened::Switched),
            link(ToastLevel::Success, "Opened /dev/ttyUSB0"),
        ),
        (
            Event::LinkShared {
                label: "host:23".to_string(),
                holder: "minicom".to_string(),
            },
            Note::warn("host:23 is open in minicom too"),
        ),
        (
            Event::LinkNotOpened {
                error: "no such port".to_string(),
            },
            link(ToastLevel::Warn, "no such port"),
        ),
        (
            Event::ReattachFailed {
                error: "busy".to_string(),
            },
            link(ToastLevel::Error, "Failed to reattach: busy"),
        ),
        (
            Event::LinkLost {
                link: "Serial port",
            },
            link(ToastLevel::Warn, "Serial port lost"),
        ),
        (
            Event::LinkIdle { secs: 30 },
            link(ToastLevel::Warn, "Device idle for 30s: port detached"),
        ),
        (
            Event::WriteFailed {
                link: "Serial port",
                error: "EIO".to_string(),
            },
            link(ToastLevel::Error, "Serial port: EIO"),
        ),
        (
            Event::ReadFailed { link: "TCP link" },
            link(ToastLevel::Error, "TCP link read error"),
        ),
        (
            Event::BreakFailed {
                error: "EIO".to_string(),
            },
            Note::new(ToastLevel::Error, "break: EIO"),
        ),
        (
            Event::LinkWarning {
                text: "rate not taken".to_string(),
            },
            Note::warn("rate not taken"),
        ),
    ];
    for (event, note) in cases {
        assert_eq!(event.note(), Some(note), "{event:?}");
    }
}

/// MAX-165 (E2): the transfer, trigger, terminal and replay events say the notes the user saw
/// before events, a silent trigger none; the transfer's in its box, the unhandled bytes, the
/// device's copy and the shell hint each in theirs.
/// How: one event of each kind.
#[test]
fn session_events_say_their_notes() {
    let transfer = |level, text: &str| Some(Note::new(level, text).in_slot("transfer"));
    let fired = |silent| Event::TriggerFired {
        name: "uboot".to_string(),
        run: "send ' '".to_string(),
        silent,
    };
    let cases = [
        (
            Event::Sent {
                name: "fw.bin".to_string(),
                size: 10,
                protocol: Some("YMODEM"),
            },
            transfer(ToastLevel::Success, "Sent fw.bin (10 bytes, YMODEM)"),
        ),
        (
            Event::Sent {
                name: "fw.bin".to_string(),
                size: 10,
                protocol: None,
            },
            transfer(ToastLevel::Success, "Sent fw.bin (10 bytes)"),
        ),
        (
            Event::FileReceived {
                name: "a.txt".to_string(),
                size: 3,
            },
            transfer(ToastLevel::Success, "Received a.txt (3 bytes)"),
        ),
        (
            Event::NothingReceived,
            transfer(
                ToastLevel::Info,
                "Nothing received: the sender sent no file",
            ),
        ),
        (
            Event::ReceivedFile {
                name: "a.txt".to_string(),
                size: 3,
                folder: "/tmp".to_string(),
            },
            transfer(ToastLevel::Success, "Received a.txt (3 bytes) into /tmp"),
        ),
        (
            Event::ReceivedFiles {
                count: 2,
                size: 7,
                folder: "/tmp".to_string(),
            },
            transfer(ToastLevel::Success, "Received 2 files (7 bytes) into /tmp"),
        ),
        (
            Event::TransferStopped {
                stopped: "Stopped sending fw.bin at 40%".to_string(),
                why: "the link closed".to_string(),
            },
            transfer(
                ToastLevel::Error,
                "Stopped sending fw.bin at 40%: the link closed",
            ),
        ),
        (fired(false), Some(Note::info("Trigger uboot: send ' '"))),
        (fired(true), None),
        (
            Event::Unhandled {
                kinds: vec![("NUL".to_string(), 9), ("CSI ?2026h".to_string(), 1)],
                more: 1,
            },
            Some(Note::info("Unhandled: NUL ×9, CSI ?2026h, 1 more kind").in_slot("unhandled")),
        ),
        (
            Event::DeviceCopied { base64_len: 8 },
            Some(Note::info("The device copied about 6 bytes to your clipboard").in_slot("copy")),
        ),
        (
            Event::ShellSeen {
                key: "Ctrl-a m".to_string(),
            },
            Some(
                Note::info(
                    "The device looks like a shell (it sent a terminal query): Ctrl-a m \
                     switches to terminal mode for its line editing and history",
                )
                .in_slot("shell"),
            ),
        ),
        (Event::ReplayFinished, Some(Note::info("Replay finished"))),
    ];
    for (event, note) in cases {
        assert_eq!(event.note(), note, "{event:?}");
    }
}

/// MAX-165 (E3): the capture events say the notes the user saw before events; a group load
/// warns only when line mode misses something too.
/// How: one event of each kind, the group's both ways.
#[test]
fn capture_events_say_their_notes() {
    let group = |warn| Event::GroupLoaded {
        message: "Loaded 3 sessions".to_string(),
        warn,
    };
    let cases = [
        (
            Event::SessionsPruned {
                message: "Session folder full: removed the oldest session (1 KB)".to_string(),
            },
            Note::info("Session folder full: removed the oldest session (1 KB)"),
        ),
        (
            Event::BackupOff {
                error: "no cache folder".to_string(),
            },
            Note::warn("Session backup off: no cache folder"),
        ),
        (
            Event::SessionRestored {
                span: "today 10:00-10:05".to_string(),
            },
            Note::info("Restored the session of today 10:00-10:05"),
        ),
        (
            Event::RestoreFailed {
                error: "not a complete session".to_string(),
            },
            Note::warn("Session not restored: not a complete session"),
        ),
        (
            Event::RestoreOverflowed,
            Note::warn("Session not restored: too much data came in while asking"),
        ),
        (
            Event::RecordingStopped,
            Note::warn("Full recording stopped: the device changed"),
        ),
        (group(false), Note::info("Loaded 3 sessions")),
        (group(true), Note::warn("Loaded 3 sessions")),
    ];
    for (event, note) in cases {
        assert_eq!(event.note(), Some(note), "{event:?}");
    }
}

/// MAX-165 (E4): the config, clipboard and screen events say the notes the user saw before
/// events; the device's failed copy in the copy's box.
/// How: one event of each kind.
#[test]
fn config_and_screen_events_say_their_notes() {
    let cases = [
        (
            Event::ConfigWarning {
                text: "Keymap: unreachable Ctrl-a x".to_string(),
            },
            Note::warn("Keymap: unreachable Ctrl-a x"),
        ),
        (
            Event::DeviceCopyFailed {
                error: "no terminal".to_string(),
            },
            Note::new(ToastLevel::Error, "Clipboard error: no terminal").in_slot("copy"),
        ),
        (
            Event::ScreenWarning {
                text: "Terminal window too small (20x5). Minimum is 40x10".to_string(),
            },
            Note::warn("Terminal window too small (20x5). Minimum is 40x10"),
        ),
    ];
    for (event, note) in cases {
        assert_eq!(event.note(), Some(note), "{event:?}");
    }
}
