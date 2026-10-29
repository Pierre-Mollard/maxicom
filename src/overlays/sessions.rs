use std::path::PathBuf;

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
/// details and last lines. Deletes the selected one, or all but the running ones once asked,
/// and sorts them by date or by device. `restorable` is the session this device would restore
/// at the next launch. The preview is read when the selection changes, not on every frame
#[derive(Debug, Clone)]
pub struct SessionBrowser {
    pub dir: PathBuf,
    pub sessions: Vec<Session>,
    pub selected: usize,
    pub order: SessionOrder,
    pub restorable: Option<String>,
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
            confirm_delete_all: false,
            preview: Vec::new(),
        };
        browser.reload();
        browser
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
            DialogAction::Pick(PopupTarget::SessionDelete) => return self.delete_selected(),
            DialogAction::Pick(PopupTarget::SessionDeleteAll) => return self.ask_delete_all(),
            DialogAction::Pick(PopupTarget::SessionSort) => self.switch_order(),
            DialogAction::HandleKey(key) => match key.code {
                KeyCode::Char('d') | KeyCode::Delete => return self.delete_selected(),
                KeyCode::Char('D') => return self.ask_delete_all(),
                KeyCode::Char('s') => self.switch_order(),
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
        serial::SerialConfig,
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
            &SerialConfig {
                port: "live".to_string(),
                ..SerialConfig::default()
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
