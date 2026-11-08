//! The session browser's keys and what they do to the session folder (MAX-148).

use super::*;
use crate::{
    capture::{
        CaptureDevice, CaptureFormat, Recorder, RecordingMeta, SessionLimits,
        tests::{device, write_test_session},
    },
    configs::TextEncoding,
    link::{Address, LinkConfig},
};
use chrono::{Local, NaiveDateTime, TimeZone};
use crossterm::event::{KeyEvent, KeyModifiers};
use std::path::Path;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("maxicom_test_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn key(c: char) -> DialogAction {
    DialogAction::HandleKey(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
}

/// Sessions on ports a and b, then a running session backup on "live", in a fresh folder.
fn folder(name: &str) -> (PathBuf, Recorder) {
    let dir = temp_dir(name);
    let text = CaptureFormat::default();
    write_test_session(&dir, "/dev/a", &text, b"line of a\r\n", b"");
    write_test_session(&dir, "/dev/b", &text, b"line of b\r\n", b"");
    let device = CaptureDevice::new(
        &LinkConfig {
            address: "live".parse::<Address>().unwrap(),
            ..LinkConfig::default()
        },
        TextEncoding::Utf8,
    );
    let mut running = Recorder::default();
    running
        .start_session(&dir, &device, &text, SessionLimits::keep(20))
        .unwrap();
    (dir, running)
}

/// The browser on `dir` in its list view, a row per session in `sessions`' order.
fn list_view(dir: &Path) -> SessionBrowser {
    let mut browser = SessionBrowser::new(dir.to_path_buf(), None);
    browser.handle_action(key('s'));
    browser
}

fn tags(browser: &SessionBrowser) -> Vec<&str> {
    browser.sessions.iter().map(Session::port_tag).collect()
}

fn select(browser: &mut SessionBrowser, tag: &str) {
    let idx = tags(browser).iter().position(|t| *t == tag).unwrap();
    browser.handle_action(DialogAction::Pick(PopupTarget::SessionRow(idx)));
}

/// The selection moves with the keys and stays in the list; the preview is the selected
/// session's last lines.
#[test]
fn selection_moves_and_previews() {
    let (dir, running) = folder("browser_select");
    let mut browser = list_view(&dir);
    assert_eq!(browser.sessions.len(), 3);
    browser.handle_action(DialogAction::Up);
    assert_eq!(browser.selected, 0);
    browser.handle_action(DialogAction::End);
    assert_eq!(browser.selected, 2);
    browser.handle_action(DialogAction::Down);
    assert_eq!(browser.selected, 2);
    browser.handle_action(DialogAction::PageUp);
    assert_eq!(browser.selected, 0);
    select(&mut browser, "a");
    let preview: Vec<_> = browser
        .preview
        .iter()
        .map(|line| line.text.as_str())
        .collect();
    assert_eq!(preview, ["line of a"]);
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-166: `p`, Enter or the button replay the selected session's `.raw` with what its
/// meta says it was recorded on; the running one is refused with a warning, and so is a
/// session without a `.raw`.
/// How: sessions a and live; then a's `.raw` removed.
#[test]
fn replay_the_selected_session_not_the_running_one() {
    let (dir, running) = folder("browser_replay");
    let mut browser = list_view(&dir);
    select(&mut browser, "a");
    let raw = browser
        .selected_session()
        .unwrap()
        .raw_path()
        .unwrap()
        .to_path_buf();
    for action in [
        key('p'),
        DialogAction::Confirm,
        DialogAction::Pick(PopupTarget::SessionReplay),
    ] {
        match browser.handle_action(action) {
            OverlayResponse::ReplaySession { path, device } => {
                assert_eq!(path, raw);
                assert_eq!(
                    device.unwrap().address,
                    "/dev/a".parse::<Address>().unwrap()
                );
            }
            _ => panic!("no replay"),
        }
    }

    select(&mut browser, "live");
    assert!(matches!(
        browser.handle_action(key('p')),
        OverlayResponse::HandledNotify { warn: true, message }
            if message == "A running session can't be replayed"
    ));

    std::fs::remove_file(&raw).unwrap();
    browser.handle_action(key('r'));
    select(&mut browser, "a");
    assert!(matches!(
        browser.handle_action(key('p')),
        OverlayResponse::HandledNotify { warn: true, message }
            if message == "This session has no raw capture to replay"
    ));
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-166: `L` or the button go back to the live link a replay set aside, as `replay-stop`
/// does; without one they do nothing. Replaying the live link's own running session goes
/// back to it too; another running session is still refused.
/// How: a browser without a live link, then one whose live session is "live", and "a"
/// marked running.
#[test]
fn back_to_live_only_with_a_live_link() {
    let (dir, running) = folder("browser_live");
    let mut browser = list_view(&dir);
    let back = || [key('L'), DialogAction::Pick(PopupTarget::SessionBackToLive)];
    for action in back() {
        assert!(matches!(
            browser.handle_action(action),
            OverlayResponse::Handled
        ));
    }
    select(&mut browser, "live");
    assert!(matches!(
        browser.handle_action(key('p')),
        OverlayResponse::HandledNotify { warn: true, .. }
    ));

    let raw = browser
        .selected_session()
        .unwrap()
        .raw_path()
        .map(Path::to_path_buf);
    let mut browser = browser.with_live(raw);
    for action in back() {
        assert!(matches!(
            browser.handle_action(action),
            OverlayResponse::StopReplay
        ));
    }
    assert!(matches!(
        browser.handle_action(key('p')),
        OverlayResponse::StopReplay
    ));
    select(&mut browser, "a");
    browser.sessions[browser.selected].running = true;
    assert!(matches!(
        browser.handle_action(key('p')),
        OverlayResponse::HandledNotify { warn: true, .. }
    ));
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `d` asks before deleting the selected session: `n` keeps it, `y` deletes it with a toast. On
/// the running one it's refused at once with a warning, its files kept.
#[test]
fn delete_one_but_not_the_running_one() {
    let (dir, running) = folder("browser_delete");
    let mut browser = list_view(&dir);
    select(&mut browser, "live");
    assert!(matches!(
        browser.handle_action(key('d')),
        OverlayResponse::HandledNotify { warn: true, message }
            if message == "A running session can't be deleted"
    ));
    assert!(browser.confirm_delete.is_none());
    assert_eq!(browser.sessions.len(), 3);

    select(&mut browser, "a");
    // `n` keeps it
    browser.handle_action(key('d'));
    assert!(browser.confirm_delete.is_some());
    browser.handle_action(key('n'));
    assert_eq!(browser.sessions.len(), 3);

    browser.handle_action(DialogAction::Pick(PopupTarget::SessionDelete));
    assert_eq!(browser.sessions.len(), 3, "deleted before the answer");
    let response = browser.handle_action(key('y'));
    assert!(matches!(
        response,
        OverlayResponse::HandledNotify { warn: false, message }
            if message.starts_with("Deleted 1 session (")
    ));
    let mut left = tags(&browser);
    left.sort_unstable();
    assert_eq!(left, ["b", "live"]);
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `D` asks first: `n` or Esc only closes the question, `y` deletes all but the running
/// one and says how many and how much.
#[test]
fn delete_all_asks_then_keeps_the_running_one() {
    let (dir, running) = folder("browser_delete_all");
    let mut browser = list_view(&dir);
    let all: Vec<usize> = (0..browser.sessions.len()).collect();
    assert_eq!(browser.deletable_in(&all).0, 2);
    browser.handle_action(key('D'));
    assert!(browser.confirm_delete.is_some());
    assert!(matches!(
        browser.handle_action(key('n')),
        OverlayResponse::Handled
    ));
    assert!(browser.confirm_delete.is_none());
    assert_eq!(browser.sessions.len(), 3);

    browser.handle_action(DialogAction::Pick(PopupTarget::SessionDeleteAll));
    assert!(matches!(
        browser.handle_action(DialogAction::Close),
        OverlayResponse::Handled
    ));
    browser.handle_action(key('D'));
    match browser.handle_action(key('y')) {
        OverlayResponse::HandledNotify { message, warn } => {
            assert!(!warn);
            // the size is all their files' (MAX-175)
            assert!(message.starts_with("Deleted 2 sessions ("), "{message}");
            assert!(message.ends_with("), 1 running kept"), "{message}");
        }
        _ => panic!("no toast"),
    }
    assert_eq!(tags(&browser), ["live"]);
    // nothing left to delete: no question
    assert!(matches!(
        browser.handle_action(key('D')),
        OverlayResponse::HandledNotify { warn: false, .. }
    ));
    assert!(browser.confirm_delete.is_none());
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-154: `c` copies the selected session's log path and `C` the folder's, as the
/// buttons do, the browser staying open; with no session, `c` says there is nothing.
#[test]
fn copy_keys_give_the_paths() {
    let (dir, running) = folder("browser_copy");
    let mut browser = list_view(&dir);
    select(&mut browser, "a");
    let log = browser.selected_session().unwrap().text_path().unwrap();
    let log = log.display().to_string();
    for action in [key('c'), DialogAction::Pick(PopupTarget::SessionCopyLog)] {
        match browser.handle_action(action) {
            OverlayResponse::HandledCopy { text, what } => {
                assert_eq!(
                    (text.as_str(), what.as_str()),
                    (log.as_str(), "session log path")
                );
            }
            _ => panic!("no copy"),
        }
    }
    assert!(log.ends_with("_a.log"), "{log}");
    let folder = dir.display().to_string();
    for action in [key('C'), DialogAction::Pick(PopupTarget::SessionCopyFolder)] {
        match browser.handle_action(action) {
            OverlayResponse::HandledCopy { text, what } => {
                assert_eq!(
                    (text.as_str(), what.as_str()),
                    (folder.as_str(), "folder path")
                );
            }
            _ => panic!("no copy"),
        }
    }
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);

    let mut empty = SessionBrowser::new(temp_dir("browser_copy_none"), None);
    assert!(matches!(
        empty.handle_action(key('c')),
        OverlayResponse::HandledNotify { warn: false, .. }
    ));
}

/// MAX-154: `r` reads the folder again, as the button does: a session removed out of the
/// app leaves the list, the same session stays selected, and a toast gives the count.
#[test]
fn reload_sees_changes_out_of_the_app() {
    let (dir, running) = folder("browser_reload");
    let mut browser = list_view(&dir);
    select(&mut browser, "b");
    let gone = browser
        .sessions
        .iter()
        .find(|s| s.port_tag() == "a")
        .unwrap();
    for path in &gone.files {
        std::fs::remove_file(path).unwrap();
    }
    assert_eq!(browser.sessions.len(), 3);
    match browser.handle_action(key('r')) {
        OverlayResponse::HandledNotify { message, warn } => {
            assert!(!warn);
            assert_eq!(message, "Reloaded: 2 sessions");
        }
        _ => panic!("no toast"),
    }
    let mut left = tags(&browser);
    left.sort_unstable();
    assert_eq!(left, ["b", "live"]);
    assert_eq!(browser.selected_session().unwrap().port_tag(), "b");
    assert!(matches!(
        browser.handle_action(DialogAction::Pick(PopupTarget::SessionReload)),
        OverlayResponse::HandledNotify { warn: false, .. }
    ));
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A folder that doesn't exist is an empty list, and the keys do nothing.
#[test]
fn empty_folder() {
    let mut browser = SessionBrowser::new(temp_dir("browser_none"), None);
    assert!(browser.sessions.is_empty());
    for action in [DialogAction::Down, DialogAction::End, key('d'), key('s')] {
        assert!(matches!(
            browser.handle_action(action),
            OverlayResponse::Handled
        ));
    }
    assert!(matches!(
        browser.handle_action(key('D')),
        OverlayResponse::HandledNotify { .. }
    ));
}

/// MAX-202: the header says how full the folder is, against its limit when it has one, and
/// the day of the oldest session.
/// How: the 3 sessions of `folder`, without then with a 200 MiB limit.
#[test]
fn usage_against_the_limit() {
    let (dir, running) = folder("browser_usage");
    let browser = SessionBrowser::new(dir.clone(), None);
    let used = size(browser.sessions.iter().map(|session| session.size).sum());
    let oldest = (browser.sessions.iter())
        .filter_map(|session| session.started)
        .min()
        .unwrap()
        .format("%Y-%m-%d");
    assert_eq!(browser.usage(), format!("{used} · oldest {oldest}"));
    let browser = browser.with_max_size(Some(200 << 20));
    assert_eq!(
        browser.usage(),
        format!("{used} / {} · oldest {oldest}", size(200 << 20))
    );
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A browser on no folder, showing sessions on one port at these times, newest first: Device
/// > [Week W45 > [Day Sat > 2 sessions, 1 session], 1 session], under the default levels.
fn dated_browser() -> SessionBrowser {
    let usb = device("/dev/ttyUSB0");
    let session = |at: &str| {
        let naive = NaiveDateTime::parse_from_str(at, "%Y-%m-%d %H:%M").unwrap();
        let started = Local.from_local_datetime(&naive).earliest().unwrap();
        let stem = format!("{}_ttyUSB0", started.format("%Y%m%d_%H%M%S%3f"));
        Session {
            folder: PathBuf::from(&stem),
            stem,
            files: Vec::new(),
            meta: Some(RecordingMeta::new(
                started,
                usb.clone(),
                CaptureFormat::default(),
            )),
            started: Some(started),
            ended: None,
            raw_len: 10,
            size: 10,
            running: false,
        }
    };
    let mut browser = SessionBrowser::new(temp_dir("browser_dated"), None);
    browser.sessions = [
        "2026-11-07 14:00",
        "2026-11-07 09:00",
        "2026-11-06 10:00",
        "2026-10-20 10:00",
    ]
    .map(session)
    .to_vec();
    browser.build();
    browser
}

/// The rows on show: indented, an arrow on a group, a group's label, a session's day and time.
fn shown(browser: &SessionBrowser) -> Vec<String> {
    let rows = browser.rows();
    let text = |row: &BrowserRow| {
        let indent = "  ".repeat(row.depth);
        match (row.node.kind, row.open) {
            (NodeKind::Session, _) => {
                let session = &browser.sessions[row.node.sessions[0]];
                format!("{indent}{}", &session.stem[4..13])
            }
            (_, true) => format!("{indent}v {}", row.node.label),
            (_, false) => format!("{indent}> {}", row.node.label),
        }
    };
    rows.iter().map(text).collect()
}

const DEVICE: &str = "ttyUSB0 115200 8N1 UTF8";

/// MAX-202: Right opens a group, Left closes it or goes to the parent, Enter does both ways.
/// Right does nothing on a session, Left nothing on a closed root.
/// How: the dated tree, walked from its closed device.
#[test]
fn groups_open_and_close() {
    let mut browser = dated_browser();
    assert_eq!(shown(&browser), [format!("> {DEVICE}")]);
    browser.handle_action(DialogAction::Left);
    assert_eq!(browser.selected, 0);

    browser.handle_action(DialogAction::Right);
    assert_eq!(
        shown(&browser),
        [
            format!("v {DEVICE}"),
            "  > 2026-W45 (Nov 02–08)".to_string(),
            "  1020_1000".to_string(),
        ]
    );
    browser.handle_action(DialogAction::Down);
    browser.handle_action(DialogAction::Confirm);
    browser.handle_action(DialogAction::Down);
    browser.handle_action(DialogAction::Right);
    assert_eq!(
        shown(&browser),
        [
            format!("v {DEVICE}"),
            "  v 2026-W45 (Nov 02–08)".to_string(),
            "    v Sat 2026-11-07".to_string(),
            "      1107_1400".to_string(),
            "      1107_0900".to_string(),
            "    1106_1000".to_string(),
            "  1020_1000".to_string(),
        ]
    );
    // on a session, Right does nothing and Left goes to the group
    browser.handle_action(DialogAction::Down);
    assert_eq!(browser.selected, 3);
    browser.handle_action(DialogAction::Right);
    assert_eq!(browser.selected, 3);
    browser.handle_action(DialogAction::Left);
    assert_eq!(browser.selected, 2);
    // Left closes, then goes up; Enter on the open group closes it
    browser.handle_action(DialogAction::Left);
    assert_eq!(shown(&browser)[2], "    > Sat 2026-11-07");
    browser.handle_action(DialogAction::Left);
    assert_eq!(browser.selected, 1);
    browser.handle_action(DialogAction::Confirm);
    assert_eq!(shown(&browser)[1], "  > 2026-W45 (Nov 02–08)");
}

/// MAX-202: a click selects a row, a click on the selected group opens or closes it, and a
/// click on the selected session does nothing more.
/// How: clicks on the device, then on the session under it.
#[test]
fn clicks_select_then_toggle() {
    let mut browser = dated_browser();
    let click = |browser: &mut SessionBrowser, row| {
        browser.handle_action(DialogAction::Pick(PopupTarget::SessionRow(row)))
    };
    click(&mut browser, 0);
    assert!(browser.rows()[0].open);
    click(&mut browser, 2);
    assert_eq!(browser.selected, 2);
    assert!(matches!(click(&mut browser, 2), OverlayResponse::Handled));
    assert_eq!(shown(&browser).len(), 3);
    click(&mut browser, 0);
    assert_eq!(browser.selected, 0);
    click(&mut browser, 0);
    assert!(!browser.rows()[0].open);
}

/// MAX-202: a group holds no one session: `c` has no log to copy, and `p` on one without any
/// `.raw` says so.
/// How: the dated tree's device selected, its sessions without files.
#[test]
fn group_keys_without_files() {
    let mut browser = dated_browser();
    assert!(browser.selected_session().is_none());
    assert!(matches!(
        browser.handle_action(key('c')),
        OverlayResponse::HandledNotify { warn: false, .. }
    ));
    assert!(matches!(
        browser.handle_action(key('p')),
        OverlayResponse::HandledNotify { warn: true, message }
            if message == "This group has no finished session to replay"
    ));
}

/// Sessions on port a with these lines, a session on port b, and a running one on a, in a tree
/// without time levels. Port a's device is selected.
fn group_folder(name: &str, rx: &[&[u8]]) -> (PathBuf, Recorder, SessionBrowser, CaptureDevice) {
    let dir = temp_dir(name);
    let text = CaptureFormat::default();
    write_test_session(&dir, "/dev/b", &text, b"on b\r\n", b"");
    let sessions: Vec<Session> = (rx.iter())
        .map(|rx| write_test_session(&dir, "/dev/a", &text, rx, b""))
        .collect();
    let device = sessions[0].meta.as_ref().unwrap().device.clone();
    let mut running = Recorder::default();
    running
        .start_session(&dir, &device, &text, SessionLimits::keep(20))
        .unwrap();
    let mut browser = SessionBrowser::new(dir.clone(), None).with_group_by(Vec::new());
    let device_row = browser
        .rows()
        .iter()
        .position(|row| row.node.kind == NodeKind::Device);
    browser.handle_action(DialogAction::Pick(PopupTarget::SessionRow(
        device_row.unwrap(),
    )));
    (dir, running, browser, device)
}

/// MAX-202: a group's preview is the last lines of its sessions, oldest first, a separator after
/// each session, its own session noted on each line.
/// How: two sessions of a line each and the running one, the group selected.
#[test]
fn group_preview_goes_through_its_sessions() {
    let (dir, running, browser, _) =
        group_folder("browser_group_preview", &[b"first\r\n", b"second\r\n"]);
    assert_eq!(browser.selected_row().unwrap().node.kind, NodeKind::Device);
    let texts: Vec<_> = browser
        .preview
        .iter()
        .map(|line| line.text.as_str())
        .collect();
    assert_eq!(texts[0], "first", "{texts:?}");
    assert!(texts[1].starts_with("── end of session: a, "), "{texts:?}");
    assert_eq!(texts[2], "second", "{texts:?}");
    assert!(texts[3].starts_with("── end of session: a, "), "{texts:?}");
    assert_eq!(browser.preview[1].session, None);
    let first = browser.preview[0].session.unwrap();
    assert!(browser.sessions[first].stem.ends_with("_a"));
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-202: a group's preview keeps at most `PREVIEW_LINES`, from the end: the newest session's
/// lines and separators all there, the older sessions not read past it.
/// How: two sessions of more lines than the preview keeps.
#[test]
fn group_preview_is_capped_from_the_end() {
    let many = |name: &str| -> Vec<u8> {
        (0..PREVIEW_LINES + 50)
            .flat_map(|i| format!("{name} {i}\r\n").into_bytes())
            .collect()
    };
    let (older, newer) = (many("older"), many("newer"));
    let (dir, running, browser, _) = group_folder("browser_group_preview_cap", &[&older, &newer]);
    assert_eq!(browser.preview.len(), PREVIEW_LINES);
    assert!(
        browser
            .preview
            .iter()
            .all(|line| !line.text.starts_with("older"))
    );
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-202: `d` or the button on a group asks first, naming it: `n` keeps everything, `y` deletes
/// its sessions but the running one, the others in the folder untouched.
/// How: two sessions and the running one on port a, one on port b.
#[test]
fn group_delete_asks_then_keeps_the_running_one() {
    let (dir, running, mut browser, _) =
        group_folder("browser_group_delete", &[b"first\r\n", b"second\r\n"]);
    let label = browser.selected_row().unwrap().node.label.clone();
    assert!(matches!(
        browser.handle_action(key('d')),
        OverlayResponse::Handled
    ));
    let confirm = browser.confirm_delete.clone().unwrap();
    assert_eq!(confirm.group.as_deref(), Some(label.as_str()));
    assert_eq!(browser.deletable_in(&confirm.sessions).0, 2);
    browser.handle_action(key('n'));
    assert!(browser.confirm_delete.is_none());
    assert_eq!(browser.sessions.len(), 4);

    browser.handle_action(DialogAction::Pick(PopupTarget::SessionDelete));
    match browser.handle_action(key('y')) {
        OverlayResponse::HandledNotify { message, warn } => {
            assert!(!warn);
            assert!(message.starts_with("Deleted 2 sessions ("), "{message}");
            assert!(message.ends_with("), 1 running kept"), "{message}");
        }
        _ => panic!("no toast"),
    }
    let mut left: Vec<_> = browser.sessions.iter().map(Session::port_tag).collect();
    left.sort_unstable();
    assert_eq!(left, ["a", "b"]);
    assert!(browser.sessions.iter().any(|s| s.running));

    // only the running one left in the group: nothing to delete, no question
    let selected = browser.selected_row().unwrap();
    assert_eq!(selected.node.sessions.len(), 1);
    assert!(browser.sessions[selected.node.sessions[0]].running);
    assert!(matches!(
        browser.handle_action(key('d')),
        OverlayResponse::HandledNotify { warn: false, message }
            if message == "No session to delete"
    ));
    assert!(browser.confirm_delete.is_none());
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-202: `p` or the button on a group gives its finished sessions, oldest first: a group
/// with the running session stops just before it, and one with only that session says so.
/// How: two past sessions on a port plus a running one on the same device; then the group of
/// the running session on another port.
#[test]
fn group_replay_gives_the_finished_sessions_oldest_first() {
    let dir = temp_dir("browser_group_replay");
    let text = CaptureFormat::default();
    let first = write_test_session(&dir, "/dev/a", &text, b"one\r\n", b"");
    let second = write_test_session(&dir, "/dev/a", &text, b"two\r\n", b"");
    let device = first.meta.as_ref().unwrap().device.clone();
    let mut running = Recorder::default();
    running
        .start_session(&dir, &device, &text, SessionLimits::keep(20))
        .unwrap();
    let mut browser = SessionBrowser::new(dir.clone(), None).with_group_by(Vec::new());
    for action in [key('p'), DialogAction::Pick(PopupTarget::SessionReplay)] {
        match browser.handle_action(action) {
            OverlayResponse::ReplayGroup(sessions) => {
                let stems: Vec<_> = sessions.iter().map(|s| s.stem.clone()).collect();
                assert_eq!(stems, [first.stem.clone(), second.stem.clone()]);
            }
            _ => panic!("no group replay"),
        }
    }
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);

    let (dir, running) = folder("browser_group_running");
    let mut browser = SessionBrowser::new(dir.clone(), None).with_group_by(Vec::new());
    assert!(browser.rows()[0].node.label.starts_with("live"));
    assert!(matches!(
        browser.handle_action(key('p')),
        OverlayResponse::HandledNotify { warn: true, .. }
    ));
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-202: `s` switches to the newest-first list and back. The list selects the newest session
/// of the selected group; the tree selects the session, or the closed group that hides it.
/// How: the dated tree, with the week selected, then a session of the week.
#[test]
fn view_switch_keeps_the_selection() {
    let mut browser = dated_browser();
    let stem = |browser: &SessionBrowser| browser.selected_session().map(|s| s.stem.clone());
    browser.handle_action(DialogAction::Right);
    browser.handle_action(DialogAction::Down);
    browser.handle_action(key('s'));
    assert_eq!(browser.view, BrowserView::List);
    assert_eq!(browser.rows().len(), 4);
    assert_eq!(stem(&browser), Some(browser.sessions[0].stem.clone()));

    // the session is under the closed week
    browser.handle_action(DialogAction::Pick(PopupTarget::SessionView));
    assert_eq!(browser.view, BrowserView::Tree);
    assert_eq!(browser.selected, 1);
    assert_eq!(browser.selected_session(), None);

    // the week open, the session is on show down to its day
    browser.handle_action(DialogAction::Right);
    browser.handle_action(DialogAction::Down);
    browser.handle_action(DialogAction::Right);
    browser.handle_action(DialogAction::Down);
    browser.handle_action(key('s'));
    browser.handle_action(key('s'));
    assert_eq!(stem(&browser), Some(browser.sessions[0].stem.clone()));
    assert_eq!(browser.selected, 3);
}

/// MAX-202: the open groups stay open and the selection stays on its key across a reload; when
/// a session's group is gone, it stays on the row holding it.
/// How: sessions on ports a and b without time levels, so the date doesn't matter; a's device
/// open with its session selected; then a's other session removed, then the selected one's
/// group closed.
#[test]
fn reload_keeps_the_open_groups_and_the_selection() {
    let dir = temp_dir("browser_tree_reload");
    let text = CaptureFormat::default();
    write_test_session(&dir, "/dev/a", &text, b"a1\r\n", b"");
    write_test_session(&dir, "/dev/a", &text, b"a2\r\n", b"");
    let gone = write_test_session(&dir, "/dev/b", &text, b"b\r\n", b"");
    let mut browser = SessionBrowser::new(dir.clone(), None).with_group_by(Vec::new());
    // b is the newest device, open; a is closed under it
    assert_eq!(browser.rows().len(), 3);
    assert!(browser.rows()[0].open);
    browser.handle_action(DialogAction::End);
    browser.handle_action(DialogAction::Right);
    browser.handle_action(DialogAction::Down);
    let stem = browser.selected_session().unwrap().stem.clone();

    std::fs::remove_dir_all(&gone.folder).unwrap();
    browser.handle_action(key('r'));
    assert_eq!(browser.selected_session().unwrap().stem, stem);
    assert!(browser.rows()[0].open);

    // closed behind the selection: the group holding the session is selected
    let device = browser.rows()[0].node.key.clone();
    browser.open.remove(&device);
    browser.handle_action(key('r'));
    assert_eq!(browser.selected, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-202: `with_group_by` sets the levels of the tree; the first device starts open.
/// How: the dated tree by days alone.
#[test]
fn group_by_sets_the_tree_levels() {
    let mut browser = dated_browser();
    browser.handle_action(DialogAction::Right);
    let browser = browser.with_group_by(vec![GroupLevel::Day]);
    assert_eq!(
        shown(&browser),
        [
            format!("v {DEVICE}"),
            "  > Sat 2026-11-07".to_string(),
            "  1106_1000".to_string(),
            "  1020_1000".to_string(),
        ]
    );
}

/// MAX-202: focusing a session opens only the groups down to it, however deep, and selects it as
/// a leaf with its details and preview; a session that isn't there changes nothing.
/// How: the dated tree with the raw paths of its sessions faked, a session under a day under a
/// week under the device; then an unknown path.
#[test]
fn focus_opens_down_to_the_session() {
    let raw = |at: usize| PathBuf::from(format!("/nowhere/{at}.raw"));
    let with_raws = || {
        let mut browser = dated_browser();
        for (at, session) in browser.sessions.iter_mut().enumerate() {
            session.files = vec![raw(at)];
        }
        browser
    };
    let browser = with_raws().with_focus(Some(&raw(1)));
    assert_eq!(
        shown(&browser),
        [
            format!("v {DEVICE}"),
            "  v 2026-W45 (Nov 02–08)".to_string(),
            "    v Sat 2026-11-07".to_string(),
            "      1107_1400".to_string(),
            "      1107_0900".to_string(),
            "    1106_1000".to_string(),
            "  1020_1000".to_string(),
        ]
    );
    assert_eq!(browser.selected, 4);
    assert_eq!(browser.selected_session(), browser.sessions.get(1));

    // a session right under the device: only the device opens
    let browser = with_raws().with_focus(Some(&raw(3)));
    assert_eq!(
        shown(&browser),
        [
            format!("v {DEVICE}"),
            "  > 2026-W45 (Nov 02–08)".to_string(),
            "  1020_1000".to_string(),
        ]
    );
    assert_eq!(browser.selected, 2);

    // unknown, or none: as before
    for focus in [Some(raw(9)), None] {
        let browser = with_raws().with_focus(focus.as_deref());
        assert_eq!(browser.selected, 0);
        assert_eq!(shown(&browser), [format!("> {DEVICE}")]);
    }
}

fn type_name(browser: &mut SessionBrowser, name: &str) {
    for c in name.chars() {
        assert!(matches!(
            browser.handle_action(key(c)),
            OverlayResponse::Handled
        ));
    }
}

/// MAX-213: `n` asks a finished session's name in a field holding its name, whose letters are
/// typed and not the browser's keys. Enter writes it and says so, Esc leaves it as it was, and
/// an emptied field takes it away.
/// How: a session on port a, named, renamed then cancelled, then emptied.
#[test]
fn name_a_finished_session() {
    let (dir, running) = folder("browser_name");
    let mut browser = list_view(&dir);
    select(&mut browser, "a");
    browser.handle_action(key('n'));
    assert_eq!(browser.naming.as_ref().unwrap().value(), "");
    // `d` and `s` here are letters of the name
    type_name(&mut browser, "dps");
    assert!(browser.confirm_delete.is_none());
    assert!(matches!(
        browser.handle_action(DialogAction::Confirm),
        OverlayResponse::HandledNotify { message, warn: false } if message == "Session named 'dps'"
    ));
    assert!(browser.naming.is_none());
    let session = browser.selected_session().unwrap();
    assert_eq!((session.port_tag(), session.name()), ("a", Some("dps")));

    browser.handle_action(key('n'));
    assert_eq!(browser.naming.as_ref().unwrap().value(), "dps");
    type_name(&mut browser, "x");
    browser.handle_action(DialogAction::Close);
    assert!(browser.naming.is_none());
    assert_eq!(browser.selected_session().unwrap().name(), Some("dps"));

    browser.handle_action(DialogAction::Pick(PopupTarget::SessionName));
    browser.naming = Some(Input::default());
    assert!(matches!(
        browser.handle_action(DialogAction::Pick(PopupTarget::SessionConfirm)),
        OverlayResponse::HandledNotify { message, warn: false } if message == "Session name removed"
    ));
    assert_eq!(browser.selected_session().unwrap().name(), None);
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-213: this maxicom's running session is named through the app, as its recording holds the
/// meta; one running in another maxicom, a group and a session without a meta aren't, and say so.
/// How: the running session of `folder` first as another maxicom's, then as this one's; the tree's
/// group; a session's meta removed.
#[test]
fn what_can_not_be_named() {
    let (dir, running) = folder("browser_name_refused");
    let refused = |response: OverlayResponse, expected: &str| match response {
        OverlayResponse::HandledNotify { message, .. } => assert_eq!(message, expected),
        _ => panic!("not refused: {expected}"),
    };
    let mut browser = list_view(&dir);
    select(&mut browser, "live");
    refused(
        browser.handle_action(key('n')),
        "Name this session in the maxicom running it",
    );
    assert!(browser.naming.is_none());

    let mut browser = list_view(&dir).with_own_session(running.session_raw());
    select(&mut browser, "live");
    browser.handle_action(key('n'));
    type_name(&mut browser, "mine");
    assert!(matches!(
        browser.handle_action(DialogAction::Confirm),
        OverlayResponse::NameRunningSession(name) if name == "mine"
    ));

    let mut tree = SessionBrowser::new(dir.clone(), None);
    assert!(tree.selected_session().is_none());
    refused(tree.handle_action(key('n')), "Only a session has a name");

    let mut browser = list_view(&dir);
    select(&mut browser, "a");
    let meta = browser.selected_session().unwrap().raw_path().unwrap();
    std::fs::remove_file(meta.with_extension("meta")).unwrap();
    browser.handle_action(key('r'));
    select(&mut browser, "a");
    refused(
        browser.handle_action(key('n')),
        "This session has no meta file for a name",
    );
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-201: a session takes the color its address is saved with when it was recorded on this
/// machine, the palette's otherwise: another address, or the same path on another machine.
/// How: sessions on two ports, one saved; then the host changed.
#[test]
fn a_session_takes_its_link_color() {
    let (dir, running) = folder("browser_link_color");
    let theme = ThemeConfig::default();
    let address = "/dev/a".parse::<Address>().unwrap();
    let mut browser =
        SessionBrowser::new(dir.clone(), None).with_link_colors(vec![(address, Color::Indexed(9))]);
    let session = |browser: &SessionBrowser, tag: &str| {
        let found = browser.sessions.iter().find(|s| s.port_tag() == tag);
        found.unwrap().clone()
    };
    let (a, b) = (session(&browser, "a"), session(&browser, "b"));
    assert_eq!(browser.link_color(&a, &theme), Color::Indexed(9));
    assert_eq!(
        browser.link_color(&b, &theme),
        theme.palette.link_color("b")
    );
    browser.host = "another-machine".to_string();
    assert_eq!(
        browser.link_color(&a, &theme),
        theme.palette.link_color("a")
    );
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}
