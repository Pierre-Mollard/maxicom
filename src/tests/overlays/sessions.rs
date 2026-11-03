//! The session browser's keys and what they do to the session folder (MAX-148).

use super::*;
use crate::{
    capture::{CaptureDevice, CaptureFormat, Recorder, SessionLimits, tests::write_test_session},
    configs::TextEncoding,
    link::{Address, LinkConfig},
};
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
    let mut browser = SessionBrowser::new(dir.clone(), None);
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
    assert_eq!(browser.preview, ["line of a"]);
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
    let mut browser = SessionBrowser::new(dir.clone(), None);
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
    let mut browser = SessionBrowser::new(dir.clone(), None);
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

/// `d` deletes the selected session with a toast, and is refused on the running one with
/// a warning, its files kept.
#[test]
fn delete_one_but_not_the_running_one() {
    let (dir, running) = folder("browser_delete");
    let mut browser = SessionBrowser::new(dir.clone(), None);
    select(&mut browser, "live");
    assert!(matches!(
        browser.handle_action(key('d')),
        OverlayResponse::HandledNotify { warn: true, .. }
    ));
    assert_eq!(browser.sessions.len(), 3);

    select(&mut browser, "a");
    let response = browser.handle_action(DialogAction::Pick(PopupTarget::SessionDelete));
    assert!(matches!(
        response,
        OverlayResponse::HandledNotify { warn: false, .. }
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
    let mut browser = SessionBrowser::new(dir.clone(), None);
    assert_eq!(browser.deletable().0, 2);
    browser.handle_action(key('D'));
    assert!(browser.confirm_delete_all);
    assert!(matches!(
        browser.handle_action(key('n')),
        OverlayResponse::Handled
    ));
    assert!(!browser.confirm_delete_all);
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
    assert!(!browser.confirm_delete_all);
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `s` switches the order, by device then by date, the same session staying selected.
#[test]
fn sort_keeps_the_selection() {
    let (dir, running) = folder("browser_sort");
    let mut browser = SessionBrowser::new(dir.clone(), None);
    select(&mut browser, "b");
    browser.handle_action(key('s'));
    assert_eq!(browser.order, SessionOrder::Device);
    assert_eq!(tags(&browser), ["a", "b", "live"]);
    assert_eq!(browser.selected_session().unwrap().port_tag(), "b");
    browser.handle_action(DialogAction::Pick(PopupTarget::SessionSort));
    assert_eq!(browser.order, SessionOrder::Date);
    assert_eq!(browser.selected_session().unwrap().port_tag(), "b");
    drop(running);
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-154: `c` copies the selected session's log path and `C` the folder's, as the
/// buttons do, the browser staying open; with no session, `c` says there is nothing.
#[test]
fn copy_keys_give_the_paths() {
    let (dir, running) = folder("browser_copy");
    let mut browser = SessionBrowser::new(dir.clone(), None);
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
    let mut browser = SessionBrowser::new(dir.clone(), None);
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
