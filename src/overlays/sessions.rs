use std::path::PathBuf;

use crossterm::event::KeyCode;

use crate::{
    capture::{
        Session, SessionOrder, delete_session, delete_sessions, list_sessions, read_tail_lines,
        sort_sessions,
    },
    inputs::DialogAction,
    overlays::{OverlayResponse, PopupTarget, copy_path},
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
    /// session's `.raw` (none without a session backup)
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
            DialogAction::Pick(PopupTarget::SessionCopyFolder) => {
                return copy_path(&self.dir, "folder");
            }
            DialogAction::Pick(PopupTarget::SessionReload) => return self.reload_folder(),
            DialogAction::HandleKey(key) => match key.code {
                KeyCode::Char('p') => return self.replay_selected(),
                KeyCode::Char('L') => return self.back_to_live(),
                KeyCode::Char('d') | KeyCode::Delete => return self.delete_selected(),
                KeyCode::Char('D') => return self.ask_delete_all(),
                KeyCode::Char('s') => self.switch_order(),
                KeyCode::Char('c') => return self.copy_log_path(),
                KeyCode::Char('C') => return copy_path(&self.dir, "folder"),
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

    /// The selected session's `.raw` to replay, with its meta's encoding and line settings. Not a
    /// session still running, here or in another maxicom: its session backup grows, and on Windows
    /// its lock keeps it from being read. The live link's own session, which a replay paused, goes
    /// back to it instead
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
            Some(path) => copy_path(path, "session log"),
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

fn notify(message: String, warn: bool) -> OverlayResponse {
    OverlayResponse::HandledNotify { message, warn }
}

#[cfg(test)]
#[path = "../tests/overlays/sessions.rs"]
mod tests;
