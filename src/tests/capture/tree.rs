//! The session tree: groups by device, then months, ISO weeks and days.

use super::*;
use crate::capture::{CaptureFormat, RecordingMeta, tests::device};
use chrono::{Local, TimeZone};
use std::path::PathBuf;

/// A session on `address` started at `at` (`2026-11-07 09:00`), its meta with `device`
/// and `text`, 10 bytes.
fn session(at: &str, device: &CaptureDevice, text: &CaptureFormat) -> Session {
    let naive = chrono::NaiveDateTime::parse_from_str(at, "%Y-%m-%d %H:%M").unwrap();
    let started = Local.from_local_datetime(&naive).earliest().unwrap();
    let stem = format!(
        "{}_{}",
        started.format("%Y%m%d_%H%M%S%3f"),
        device.address.tag()
    );
    Session {
        folder: PathBuf::from(&stem),
        stem,
        files: Vec::new(),
        meta: Some(RecordingMeta::new(started, device.clone(), text.clone())),
        started: Some(started),
        ended: None,
        raw_len: 10,
        size: 10,
        running: false,
    }
}

fn sessions_on(device: &CaptureDevice, times: &[&str]) -> Vec<Session> {
    let text = CaptureFormat::default();
    times.iter().map(|at| session(at, device, &text)).collect()
}

/// The tree as indented lines: a group's kind and label, a session's stem.
fn shape(sessions: &[Session], nodes: &[SessionNode]) -> Vec<String> {
    fn walk(sessions: &[Session], nodes: &[SessionNode], depth: usize, out: &mut Vec<String>) {
        for node in nodes {
            let text = match node.kind {
                NodeKind::Session => sessions[node.sessions[0]].stem.clone(),
                kind => format!("{kind:?} {}", node.label),
            };
            out.push(format!("{}{text}", "  ".repeat(depth)));
            walk(sessions, &node.children, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    walk(sessions, nodes, 0, &mut out);
    out
}

/// MAX-202: months, ISO weeks and days, the newest first; a group of one child is that
/// child (a week of one session is the session); a week belongs to the month of its
/// Thursday, so Sunday Nov 1 sits in October's W44.
/// How: sessions on one port from Oct 20 to Nov 10, the default levels.
#[test]
fn months_weeks_and_days() {
    let usb = device("/dev/ttyUSB0");
    let sessions = sessions_on(
        &usb,
        &[
            "2026-10-20 10:00",
            "2026-11-01 10:00",
            "2026-11-02 10:00",
            "2026-11-06 10:00",
            "2026-11-07 09:00",
            "2026-11-07 14:00",
            "2026-11-10 10:00",
        ],
    );
    let tree = session_tree(&sessions, &DEFAULT_GROUP_BY);
    assert_eq!(
        shape(&sessions, &tree),
        [
            "Device ttyUSB0 115200 8N1 UTF8",
            "  Month 2026-11",
            "    20261110_100000000_ttyUSB0",
            "    Week 2026-W45 (Nov 02–08)",
            "      Day Sat 2026-11-07",
            "        20261107_140000000_ttyUSB0",
            "        20261107_090000000_ttyUSB0",
            "      20261106_100000000_ttyUSB0",
            "      20261102_100000000_ttyUSB0",
            "  Month 2026-10",
            "    20261101_100000000_ttyUSB0",
            "    20261020_100000000_ttyUSB0",
        ]
    );
    // a group holds all the sessions under it, the oldest first, and their size
    assert_eq!(tree[0].sessions, (0..7).collect::<Vec<_>>());
    assert_eq!(tree[0].size, 70);
    let week = &tree[0].children[0].children[1];
    assert_eq!(week.sessions, [2, 3, 4, 5]);
    assert_eq!(week.key, format!("{}/2026-11/2026-W45", tree[0].key));
}

/// MAX-202: a group is replayed as one stream, so another port or other line settings make
/// another group, but not another text format, which only the `.log` has; the group with the newest
/// session comes first; a session without a meta stays alone at the top.
/// How: sessions on ttyUSB0, on ttyUSB0 at 9600, on ttyACM0, on ttyUSB0 with timestamps,
/// and one without a meta.
#[test]
fn devices_and_settings_split() {
    let usb = device("/dev/ttyUSB0");
    let mut slow = usb.clone();
    slow.line.as_mut().unwrap().baud = 9600;
    let acm = device("/dev/ttyACM0");
    let plain = CaptureFormat::default();
    let stamped = CaptureFormat {
        timestamp: Some("%H:%M".to_string()),
        tx_mark: None,
    };
    let mut no_meta = session("2026-11-05 10:00", &usb, &plain);
    no_meta.meta = None;
    let sessions = vec![
        session("2026-11-01 10:00", &usb, &plain),
        session("2026-11-02 10:00", &usb, &plain),
        session("2026-11-03 10:00", &slow, &plain),
        session("2026-11-04 10:00", &acm, &plain),
        no_meta,
        session("2026-11-06 10:00", &usb, &stamped),
    ];
    let tree = session_tree(&sessions, &[GroupLevel::Day]);
    assert_eq!(
        shape(&sessions, &tree),
        [
            "Device ttyUSB0 115200 8N1 UTF8",
            "  20261106_100000000_ttyUSB0",
            "  20261102_100000000_ttyUSB0",
            "  20261101_100000000_ttyUSB0",
            "20261105_100000000_ttyUSB0",
            "Device ttyACM0 115200 8N1 UTF8",
            "  20261104_100000000_ttyACM0",
            "Device ttyUSB0 9600 8N1 UTF8",
            "  20261103_100000000_ttyUSB0",
        ]
    );
}

/// MAX-202: `group_by` picks the levels, in any order; without weeks a month is the
/// calendar month (Nov 1 in November, a month of one day showing as the day); without
/// levels a device holds its sessions.
/// How: the same sessions with `[day, month]`, `[week]`, then `[]`.
#[test]
fn group_by_picks_the_levels() {
    let usb = device("/dev/ttyUSB0");
    let sessions = sessions_on(
        &usb,
        &["2026-10-31 10:00", "2026-11-01 10:00", "2026-11-01 12:00"],
    );
    let lines = |levels: &[GroupLevel]| shape(&sessions, &session_tree(&sessions, levels));
    assert_eq!(
        lines(&[GroupLevel::Day, GroupLevel::Month]),
        [
            "Device ttyUSB0 115200 8N1 UTF8",
            "  Day Sun 2026-11-01",
            "    20261101_120000000_ttyUSB0",
            "    20261101_100000000_ttyUSB0",
            "  20261031_100000000_ttyUSB0",
        ]
    );
    assert_eq!(
        lines(&[GroupLevel::Week]),
        [
            "Device ttyUSB0 115200 8N1 UTF8",
            "  Week 2026-W44 (Oct 26–Nov 01)",
            "    20261101_120000000_ttyUSB0",
            "    20261101_100000000_ttyUSB0",
            "    20261031_100000000_ttyUSB0",
        ]
    );
    assert_eq!(
        lines(&[]),
        [
            "Device ttyUSB0 115200 8N1 UTF8",
            "  20261101_120000000_ttyUSB0",
            "  20261101_100000000_ttyUSB0",
            "  20261031_100000000_ttyUSB0",
        ]
    );
}

/// MAX-202: a week reads on its own, its end's month written when it changes.
/// How: the week of Oct 26 and the week of Nov 2, 2026.
#[test]
fn week_labels() {
    let monday = |d| NaiveDate::from_ymd_opt(2026, 10, 26).unwrap() + Duration::days(d);
    assert_eq!(
        period_label(GroupLevel::Week, monday(0)),
        "2026-W44 (Oct 26–Nov 01)"
    );
    assert_eq!(
        period_label(GroupLevel::Week, monday(7)),
        "2026-W45 (Nov 02–08)"
    );
    assert_eq!(period_label(GroupLevel::Day, monday(12)), "Sat 2026-11-07");
}

/// MAX-202: `group_by` reads as a list of names, months, weeks and days by default.
/// How: the default, then `["week", "day"]`, then an unknown level.
#[test]
fn group_by_setting() {
    use crate::configs::settings::SessionsConfig;
    assert_eq!(SessionsConfig::default().group_by, DEFAULT_GROUP_BY);
    let parse = |toml: &str| toml::from_str::<SessionsConfig>(toml).map(|s| s.group_by);
    assert_eq!(
        parse("group_by = [\"week\", \"day\"]"),
        Ok(vec![GroupLevel::Week, GroupLevel::Day])
    );
    assert!(parse("group_by = [\"year\"]").is_err());
}

/// MAX-213: a session's row starts with its name, and its name doesn't change its key or split
/// its device's group: a group holds one setup whatever the names.
/// How: two sessions of a device, one named.
#[test]
fn a_name_labels_the_row_only() {
    let device = device("/dev/ttyUSB0");
    let text = CaptureFormat::default();
    let plain = session("2026-11-07 09:00", &device, &text);
    let mut named = session("2026-11-07 10:00", &device, &text);
    let key = session_list(std::slice::from_ref(&named))[0].key.clone();
    named.meta.as_mut().unwrap().name = Some("boot test".to_string());
    assert_eq!(session_list(std::slice::from_ref(&named))[0].key, key);

    let tree = session_tree(&[named, plain], &[]);
    assert_eq!(tree.len(), 1);
    let labels: Vec<_> = tree[0].children.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels[0], "boot test · 2026-11-07 10:00 → ?");
    assert_eq!(labels[1], "2026-11-07 09:00 → ?");
}
