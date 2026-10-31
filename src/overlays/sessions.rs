use std::path::{Path, PathBuf};

use crossterm::event::KeyCode;

use crate::{
    capture::{
        Session, SessionOrder, delete_session, delete_sessions, list_sessions, read_tail_lines,
        sort_sessions,
    },
    inputs::DialogAction,
    overlays::{OverlayResponse, PopupTarget},
    ui::status_bar::write_human,
};

/// How many of the selected session's last text lines the preview keeps: the popup shows the
/// ones that fit
const PREVIEW_LINES: usize = 200;
/// How far PageUp and PageDown move the selection
const PAGE: usize = 10;

/// The session browser (MAX-148): the session backups of the cache folder, the selected one's
/// details and last lines. Replays the selected one (MAX-166), deletes it, or all but the
/// running ones once asked, sorts them by date or by device, copies the selected one's log path
/// or the folder's, and reads the folder again. `restorable` is the session this device would
/// restore at the next launch. `live` says a replay set the live link aside, which the browser
/// goes back to, and `live_raw` is that link's paused session (MAX-166). The preview is read
/// when the selection changes, not on every frame
#[derive(Debug, Clone)]
pub struct SessionBrowser {
    pub dir: PathBuf,
    pub sessions: Vec<Session>,
    pub selected: usize,
    pub order: SessionOrder,
    pub restorable: Option<String>,
    pub live: bool,
    pub live_raw: Option<PathBuf>,
    pub confirm_delete_all: bool,
    pub preview: Vec<String>,
}

impl SessionBrowser {
    pub fn new(dir: PathBuf, restorable: Option<String>) -> Self {
        let mut browser = Self {
            dir,
            sessions: Vec::new(),
            selected: 0,
            order: SessionOrder::default(),
            restorable,
            live: false,
            live_raw: None,
            confirm_delete_all: false,
            preview: Vec::new(),
        };
        browser.reload();
        browser
    }

    /// A replay set the live link aside: the browser can go back to it, and `raw` is its
    /// session's `.raw` (none without a journal)
    pub fn with_live(mut self, raw: Option<PathBuf>) -> Self {
        self.live = true;
        self.live_raw = raw;
        self
    }

    pub fn selected_session(&self) -> Option<&Session> {
        self.sessions.get(self.selected)
    }

    /// What deleting all would remove: the sessions not running, and their size
    pub fn deletable(&self) -> (usize, u64) {
        let deletable = self.sessions.iter().filter(|session| !session.running);
        deletable.fold((0, 0), |(count, size), session| {
            (count + 1, size + session.size)
        })
    }

    /// Reads the folder again, the same session staying selected when it is still there
    fn reload(&mut self) {
        let stem = self.selected_session().map(|session| session.stem.clone());
        self.sessions = list_sessions(&self.dir).unwrap_or_default();
        sort_sessions(&mut self.sessions, self.order);
        let kept = stem.and_then(|stem| self.sessions.iter().position(|s| s.stem == stem));
        self.select(kept.unwrap_or(self.selected));
    }

    fn select(&mut self, idx: usize) {
        self.selected = idx.min(self.sessions.len().saturating_sub(1));
        self.preview = self
            .selected_session()
            .and_then(Session::text_path)
            .and_then(|path| read_tail_lines(path, PREVIEW_LINES).ok())
            .map(|tail| {
                String::from_utf8_lossy(&tail)
                    .lines()
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
    }

    fn switch_order(&mut self) {
        self.order = self.order.other();
        let stem = self.selected_session().map(|session| session.stem.clone());
        sort_sessions(&mut self.sessions, self.order);
        let kept = stem.and_then(|stem| self.sessions.iter().position(|s| s.stem == stem));
        self.selected = kept.unwrap_or(0);
    }

    pub fn handle_action(&mut self, action: DialogAction) -> OverlayResponse {
        if self.confirm_delete_all {
            return self.handle_confirm(action);
        }
        let last = self.sessions.len().saturating_sub(1);
        match action {
            DialogAction::Close => return OverlayResponse::Close,
            DialogAction::Up => self.select(self.selected.saturating_sub(1)),
            DialogAction::Down => self.select((self.selected + 1).min(last)),
            DialogAction::PageUp => self.select(self.selected.saturating_sub(PAGE)),
            DialogAction::PageDown => self.select((self.selected + PAGE).min(last)),
            DialogAction::Home => self.select(0),
            DialogAction::End => self.select(last),
            DialogAction::Pick(PopupTarget::SessionRow(idx)) => self.select(idx),
            DialogAction::Confirm | DialogAction::Pick(PopupTarget::SessionReplay) => {
                return self.replay_selected();
            }
            DialogAction::Pick(PopupTarget::SessionBackToLive) => return self.back_to_live(),
            DialogAction::Pick(PopupTarget::SessionDelete) => return self.delete_selected(),
            DialogAction::Pick(PopupTarget::SessionDeleteAll) => return self.ask_delete_all(),
            DialogAction::Pick(PopupTarget::SessionSort) => self.switch_order(),
            DialogAction::Pick(PopupTarget::SessionCopyLog) => return self.copy_log_path(),
            DialogAction::Pick(PopupTarget::SessionCopyFolder) => return copy(&self.dir, "folder"),
            DialogAction::Pick(PopupTarget::SessionReload) => return self.reload_folder(),
            DialogAction::HandleKey(key) => match key.code {
                KeyCode::Char('p') => return self.replay_selected(),
                KeyCode::Char('L') => return self.back_to_live(),
                KeyCode::Char('d') | KeyCode::Delete => return self.delete_selected(),
                KeyCode::Char('D') => return self.ask_delete_all(),
                KeyCode::Char('s') => self.switch_order(),
                KeyCode::Char('c') => return self.copy_log_path(),
                KeyCode::Char('C') => return copy(&self.dir, "folder"),
                KeyCode::Char('r') => return self.reload_folder(),
                _ => {}
            },
            _ => {}
        }
        OverlayResponse::Handled
    }

    /// The question before deleting all: yes deletes, no or Esc only closes the question
    fn handle_confirm(&mut self, action: DialogAction) -> OverlayResponse {
        let yes = match action {
            DialogAction::Confirm | DialogAction::Pick(PopupTarget::SessionConfirm) => true,
            DialogAction::Close | DialogAction::Pick(PopupTarget::SessionCancel) => false,
            DialogAction::HandleKey(key) => match key.code {
                KeyCode::Char('y' | 'Y') => true,
                KeyCode::Char('n' | 'N') => false,
                _ => return OverlayResponse::Handled,
            },
            _ => return OverlayResponse::Handled,
        };
        self.confirm_delete_all = false;
        if yes {
            self.delete_all()
        } else {
            OverlayResponse::Handled
        }
    }

    /// The selected session's `.raw` to replay, with its meta's encoding and line settings. Not
    /// a session still running, here or in another maxicom: its journal grows, and on Windows
    /// its lock keeps it from being read. The live link's own session, which a replay paused,
    /// goes back to it instead
    fn replay_selected(&self) -> OverlayResponse {
        let Some(session) = self.selected_session() else {
            return OverlayResponse::Handled;
        };
        if session.running {
            let raw = session.raw_path();
            if self
                .live_raw
                .as_deref()
                .is_some_and(|live| raw == Some(live))
            {
                return OverlayResponse::StopReplay;
            }
            return notify("A running session can't be replayed".to_string(), true);
        }
        match session.raw_path() {
            Some(path) => OverlayResponse::ReplaySession {
                path: path.to_path_buf(),
                device: session.meta.as_ref().map(|meta| meta.device.clone()),
            },
            None => notify(
                "This session has no raw capture to replay".to_string(),
                true,
            ),
        }
    }

    /// The replay stops and the live link it set aside comes back, as `replay-stop` does;
    /// nothing without one
    fn back_to_live(&self) -> OverlayResponse {
        if self.live {
            OverlayResponse::StopReplay
        } else {
            OverlayResponse::Handled
        }
    }

    fn delete_selected(&mut self) -> OverlayResponse {
        let Some(session) = self.selected_session() else {
            return OverlayResponse::Handled;
        };
        let span = session.time_span();
        let result = delete_session(session);
        self.reload();
        match result {
            Ok(()) => notify(format!("Deleted the session of {span}"), false),
            Err(err) => notify(format!("Session not deleted: {err}"), true),
        }
    }

    /// Reads the folder again, for files changed or removed out of the app (MAX-154)
    fn reload_folder(&mut self) -> OverlayResponse {
        self.reload();
        notify(
            format!("Reloaded: {}", sessions(self.sessions.len())),
            false,
        )
    }

    fn copy_log_path(&self) -> OverlayResponse {
        match self.selected_session().and_then(Session::text_path) {
            Some(path) => copy(path, "session log"),
            None => notify("No session log to copy".to_string(), false),
        }
    }

    fn ask_delete_all(&mut self) -> OverlayResponse {
        if self.deletable().0 == 0 {
            return notify("No session to delete".to_string(), false);
        }
        self.confirm_delete_all = true;
        OverlayResponse::Handled
    }

    fn delete_all(&mut self) -> OverlayResponse {
        let report = delete_sessions(&self.sessions);
        self.reload();
        let mut message = format!(
            "Deleted {} ({})",
            sessions(report.deleted),
            size(report.size)
        );
        if report.running > 0 {
            message.push_str(&format!(", {} running kept", report.running));
        }
        match report.error {
            Some(err) => notify(format!("{message}; one failed: {err}"), true),
            None => notify(message, false),
        }
    }
}

/// `n` sessions, `1 session`
pub fn sessions(n: usize) -> String {
    if n == 1 {
        "1 session".to_string()
    } else {
        format!("{n} sessions")
    }
}

/// A size as the status bar writes it, with its unit
pub fn size(bytes: u64) -> String {
    let mut out = String::new();
    write_human(&mut out, bytes);
    out.push('B');
    out
}

/// The path of `what` to the clipboard, for a look at the files out of the app (MAX-154)
fn copy(path: &Path, what: &str) -> OverlayResponse {
    OverlayResponse::HandledCopy {
        text: path.display().to_string(),
        what: format!("{what} path"),
    }
}

fn notify(message: String, warn: bool) -> OverlayResponse {
    OverlayResponse::HandledNotify { message, warn }
}

#[cfg(test)]
mod tests {
    //! The session browser's keys and what they do to the session folder (MAX-148).

    use super::*;
    use crate::{
        capture::{JournalText, Recorder, SessionDevice, tests::write_test_session},
        configs::TextEncoding,
        link::{Address, LinkConfig},
    };
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("maxicom_test_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn key(c: char) -> DialogAction {
        DialogAction::HandleKey(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    /// Sessions on ports a and b, then a running journal on "live", in a fresh folder.
    fn folder(name: &str) -> (PathBuf, Recorder) {
        let dir = temp_dir(name);
        let text = JournalText::default();
        write_test_session(&dir, "/dev/a", &text, b"line of a\r\n", b"");
        write_test_session(&dir, "/dev/b", &text, b"line of b\r\n", b"");
        let device = SessionDevice::new(
            &LinkConfig {
                address: Address::parse("live"),
                ..LinkConfig::default()
            },
            TextEncoding::Utf8,
        );
        let mut running = Recorder::default();
        running.start_journal(&dir, &device, &text, 20).unwrap();
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
                    assert_eq!(device.unwrap().address, Address::parse("/dev/a"));
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
                assert_eq!(message, "Deleted 2 sessions (42B), 1 running kept");
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
}
