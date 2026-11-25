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

/// One event of each kind, for the JSON tests (MAX-284): a new kind must be added here
fn one_of_each() -> Vec<Event> {
    let text = || "x".to_string();
    vec![
        Event::LinkOpened {
            label: text(),
            how: Opened::Switched,
        },
        Event::LinkShared {
            label: text(),
            holder: text(),
        },
        Event::LinkNotOpened { error: text() },
        Event::ReattachFailed { error: text() },
        Event::LinkLost { link: "Serial" },
        Event::LinkIdle { secs: 5 },
        Event::WriteFailed {
            link: "Serial",
            error: text(),
        },
        Event::ReadFailed { link: "Serial" },
        Event::BreakFailed { error: text() },
        Event::LinkWarning { text: text() },
        Event::Sent {
            name: text(),
            size: 1,
            protocol: Some("xmodem"),
        },
        Event::FileReceived {
            name: text(),
            size: 1,
        },
        Event::NothingReceived,
        Event::ReceivedFile {
            name: text(),
            size: 1,
            folder: text(),
        },
        Event::ReceivedFiles {
            count: 2,
            size: 1,
            folder: text(),
        },
        Event::TransferStopped {
            stopped: text(),
            why: text(),
        },
        Event::TriggerFired {
            name: text(),
            run: text(),
            silent: false,
        },
        Event::Unhandled {
            kinds: vec![(text(), 2)],
            more: 0,
        },
        Event::DeviceCopied { base64_len: 8 },
        Event::ShellSeen { key: text() },
        Event::ReplayFinished,
        Event::SessionsPruned { message: text() },
        Event::BackupOff { error: text() },
        Event::SessionRestored { span: text() },
        Event::RestoreFailed { error: text() },
        Event::RestoreOverflowed,
        Event::RecordingStopped,
        Event::GroupLoaded {
            message: text(),
            warn: false,
        },
        Event::ConfigWarning { text: text() },
        Event::DeviceCopyFailed { error: text() },
        Event::ScreenWarning { text: text() },
        Event::LineSettings {
            baud: 9600,
            framing: "8N1".to_string(),
            flow: FlowControl::RtsCts,
            cause: Cause::from(&Origin::Input),
        },
        Event::LineLevel {
            line: "dtr",
            on: false,
            cause: Cause::from(&Origin::Input),
        },
        Event::BreakSent {
            ms: 250,
            cause: Cause::from(&Origin::Input),
        },
        Event::LinkDetached {
            cause: Cause::from(&Origin::Input),
        },
        Event::LinkOpening {
            label: text(),
            cause: Cause::from(&Origin::Input),
        },
        Event::SettingChanged {
            setting: "local-echo",
            value: SettingValue::On(true),
            cause: Cause::from(&Origin::Input),
        },
        Event::OpenFailed {
            error: text(),
            back: Some(text()),
        },
        Event::ReplayStarted {
            path: text(),
            sessions: 1,
            cause: Cause::from(&Origin::Input),
        },
        Event::ReplayStopped {
            cause: Cause::from(&Origin::Input),
        },
        Event::ReplayControl {
            control: "delay",
            delay_ms: Some(20),
            cause: Cause::from(&Origin::Input),
        },
        Event::TransferStarted {
            direction: "send",
            name: text(),
            size: Some(3),
            protocol: Some("ymodem"),
            cause: Cause::from(&Origin::Input),
        },
        Event::MashStarted {
            until: text(),
            cause: Cause::from(&Origin::Input),
        },
        Event::MashDone { until: text() },
        Event::MashStopped { why: text() },
    ]
}

/// MAX-284 (step 2c): what reaches the device or the link is silent, whoever did it: its
/// result told them. In JSON an action's cause is the origin's kind as `status` says it, with
/// the trigger's or the hook's name, or `link` for what an open sets itself; the line
/// settings' framing as written (`8N1`), the flow as a config word; a setting's value as
/// `get` gives it, on/off or a word; a replay's delay only for `delay`.
/// How: each such kind from `one_of_each`, then a few with a cause or a value of each sort.
#[test]
fn device_actions_are_silent_with_their_cause() {
    let events = one_of_each();
    let actions = &events[KINDS
        .iter()
        .position(|kind| *kind == "line-settings")
        .unwrap()..];
    assert_eq!(actions.len(), 14);
    for event in actions {
        assert_eq!(event.note(), None, "{event:?}");
    }
    let json = |event: &Event| serde_json::to_value(event).unwrap();
    assert_eq!(
        json(&actions[0]),
        serde_json::json!({
            "kind": "line-settings", "baud": 9600, "framing": "8N1", "flow": "rtscts",
            "cause": {"kind": "input"}
        })
    );
    let level = Event::LineLevel {
        line: "rts",
        on: true,
        cause: Cause::link(),
    };
    assert_eq!(json(&level)["cause"], serde_json::json!({"kind": "link"}));
    let mode = Event::SettingChanged {
        setting: "mode",
        value: SettingValue::Word("terminal".to_string()),
        cause: Cause::from(&Origin::Trigger("boot".to_string())),
    };
    assert_eq!(
        json(&mode),
        serde_json::json!({
            "kind": "setting-changed", "setting": "mode", "value": "terminal",
            "cause": {"kind": "trigger", "name": "boot"}
        })
    );
    let echo = json(&actions[5]);
    assert_eq!(
        (&echo["value"], &echo["cause"]["kind"]),
        (&serde_json::json!(true), &serde_json::json!("input"))
    );
    let hook = Event::ReplayControl {
        control: "pause",
        delay_ms: None,
        cause: Cause::from(&Origin::Profile("on_load")),
    };
    assert_eq!(
        json(&hook),
        serde_json::json!({
            "kind": "replay-control", "control": "pause",
            "cause": {"kind": "profile", "name": "on_load"}
        })
    );
    let failed = Event::OpenFailed {
        error: "no such port".to_string(),
        back: None,
    };
    assert_eq!(
        json(&failed),
        serde_json::json!({"kind": "open-failed", "error": "no such port", "back": null})
    );
}

/// MAX-284 (step 2b): every event's kind is named once in `KINDS`, the same as its JSON's
/// `kind`, its fields beside it by their names; `Opened` in lowercase.
/// How: one event of each kind, serialized.
#[test]
fn every_kind_named_as_in_its_json() {
    let events = one_of_each();
    assert_eq!(events.len(), KINDS.len());
    let kinds: Vec<&str> = events.iter().map(Event::kind).collect();
    assert_eq!(kinds, KINDS);
    for event in &events {
        let json = serde_json::to_value(event).unwrap();
        assert_eq!(json["kind"], event.kind(), "{json}");
    }
    let opened = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(
        opened,
        serde_json::json!({"kind": "link-opened", "label": "x", "how": "switched"})
    );
    let unit = serde_json::to_value(Event::ReplayFinished).unwrap();
    assert_eq!(unit, serde_json::json!({"kind": "replay-finished"}));
}

/// MAX-284 (step 2b): the log numbers its events from 1 and keeps the last 1000: `after`
/// gives those after a number, all kept ones from before the oldest, none past the last;
/// `oldest` is past `last` while it is empty.
/// How: an empty log, then 1003 events.
#[test]
fn log_numbers_and_keeps_the_last() {
    let mut log = EventLog::default();
    assert_eq!((log.last(), log.oldest()), (0, 1));
    assert_eq!(log.after(0).count(), 0);
    for secs in 1..=1003 {
        log.push(Event::LinkIdle { secs });
    }
    assert_eq!((log.last(), log.oldest()), (1003, 4));
    let seqs = |after: u64| -> Vec<u64> { log.after(after).map(|logged| logged.seq).collect() };
    assert_eq!(seqs(1000), [1001, 1002, 1003]);
    assert_eq!(seqs(0).len(), 1000);
    assert_eq!(seqs(0)[0], 4);
    assert!(seqs(1003).is_empty());
    assert!(seqs(5000).is_empty());
    let last = log.after(1002).next().unwrap();
    assert_eq!(last.event, Event::LinkIdle { secs: 1003 });
}
