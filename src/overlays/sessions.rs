use std::{collections::HashSet, path::PathBuf};

use crossterm::event::KeyCode;

use crate::{
    capture::{
        DEFAULT_GROUP_BY, GroupLevel, NodeKind, Session, SessionNode, delete_session,
        delete_sessions, group_separator, list_sessions, read_tail_lines, session_list,
        session_tree,
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

/// How the session browser shows the sessions: as the tree of groups, or newest first
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrowserView {
    #[default]
    Tree,
    List,
}

impl BrowserView {
    fn other(self) -> Self {
        match self {
            Self::Tree => Self::List,
            Self::List => Self::Tree,
        }
    }
}

/// A row the browser shows: a node of the view, how deep it is, whether it is open, and the
/// row of its parent
#[derive(Debug, Clone, Copy)]
pub struct BrowserRow<'a> {
    pub node: &'a SessionNode,
    pub depth: usize,
    pub open: bool,
    pub parent: Option<usize>,
}

/// A line of the preview: a line of the `.log` of the session at this index, or a separator
/// between the sessions of a group (MAX-202)
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewLine {
    pub text: String,
    pub session: Option<usize>,
}

/// The sessions a delete asks about (MAX-202): all of them, or those of the group named
#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmDelete {
    pub sessions: Vec<usize>,
    pub group: Option<String>,
}

/// The session browser (MAX-148): the session backups of the cache folder as a tree or a list
/// (MAX-202), the selected row's details and last lines. Replays the selected session or group
/// (MAX-166), deletes it (a group once asked), or all but the running ones once asked, copies the selected one's log
/// path or the folder's, and reads the folder again. `selected` is a row of `rows`, `open` the
/// keys of the open groups: they stay across a reload. `restored` is the session the history was filled from
/// restore at the next launch. `live` says a replay set the live link aside, which the browser
/// goes back to, and `live_raw` is that link's paused session (MAX-166). `max_size` is the
/// folder's size limit, for its usage in the header (MAX-202). The preview is read when the
/// selection changes, not on every frame
#[derive(Debug, Clone)]
pub struct SessionBrowser {
    pub dir: PathBuf,
    pub sessions: Vec<Session>,
    pub tree: Vec<SessionNode>,
    pub list: Vec<SessionNode>,
    pub group_by: Vec<GroupLevel>,
    pub open: HashSet<String>,
    pub view: BrowserView,
    pub selected: usize,
    pub restored: Option<String>,
    pub live: bool,
    pub live_raw: Option<PathBuf>,
    pub max_size: Option<u64>,
    pub confirm_delete: Option<ConfirmDelete>,
    pub preview: Vec<PreviewLine>,
}

impl SessionBrowser {
    pub fn new(dir: PathBuf, restored: Option<String>) -> Self {
        let mut browser = Self {
            dir,
            sessions: Vec::new(),
            tree: Vec::new(),
            list: Vec::new(),
            group_by: DEFAULT_GROUP_BY.to_vec(),
            open: HashSet::new(),
            view: BrowserView::default(),
            selected: 0,
            restored,
            live: false,
            live_raw: None,
            max_size: None,
            confirm_delete: None,
            preview: Vec::new(),
        };
        browser.reload();
        // the newest device's months at once: most folders have one device
        let first_group = browser.tree.iter().find(|node| !node.children.is_empty());
        browser
            .open
            .extend(first_group.map(|node| node.key.clone()));
        browser
    }

    /// The time levels of the tree, as `sessions.group_by` says
    pub fn with_group_by(mut self, group_by: Vec<GroupLevel>) -> Self {
        self.group_by = group_by;
        self.build();
        self
    }

    /// A replay set the live link aside: the browser can go back to it, and `raw` is its
    /// session's `.raw` (none without a session backup)
    pub fn with_live(mut self, raw: Option<PathBuf>) -> Self {
        self.live = true;
        self.live_raw = raw;
        self
    }

    /// The folder's size limit, none for no limit
    pub fn with_max_size(mut self, max_size: Option<u64>) -> Self {
        self.max_size = max_size;
        self
    }

    /// How full the folder is, and since when it holds sessions (MAX-202): `186MB / 200MB ·
    /// oldest 2026-10-13`, without the limit when there is none
    pub fn usage(&self) -> String {
        let used: u64 = self.sessions.iter().map(|session| session.size).sum();
        let mut usage = size(used);
        if let Some(max) = self.max_size {
            usage.push_str(&format!(" / {}", size(max)));
        }
        let oldest = (self.sessions.iter())
            .filter_map(|session| session.started)
            .min();
        if let Some(oldest) = oldest {
            usage.push_str(&format!(" · oldest {}", oldest.format("%Y-%m-%d")));
        }
        usage
    }

    /// The rows on show: the view's nodes, with the children of the open ones
    pub fn rows(&self) -> Vec<BrowserRow<'_>> {
        let nodes = match self.view {
            BrowserView::Tree => &self.tree,
            BrowserView::List => &self.list,
        };
        let mut rows = Vec::new();
        self.push_rows(nodes, 0, None, &mut rows);
        rows
    }

    fn push_rows<'a>(
        &'a self,
        nodes: &'a [SessionNode],
        depth: usize,
        parent: Option<usize>,
        rows: &mut Vec<BrowserRow<'a>>,
    ) {
        for node in nodes {
            let row = rows.len();
            let open = self.open.contains(&node.key);
            rows.push(BrowserRow {
                node,
                depth,
                open,
                parent,
            });
            if open {
                self.push_rows(&node.children, depth + 1, Some(row), rows);
            }
        }
    }

    pub fn selected_row(&self) -> Option<BrowserRow<'_>> {
        self.rows().get(self.selected).copied()
    }

    /// The session of the selected row, none on a group
    pub fn selected_session(&self) -> Option<&Session> {
        let row = self.selected_row()?;
        match row.node.kind {
            NodeKind::Session => self.sessions.get(*row.node.sessions.first()?),
            _ => None,
        }
    }

    /// The key of the selected row when it is a group
    fn selected_group(&self) -> Option<String> {
        let row = self.selected_row()?;
        (row.node.kind != NodeKind::Session).then(|| row.node.key.clone())
    }

    /// The deepest row on show holding the session at `index`: itself, or the closed group
    /// that hides it
    fn row_holding(&self, index: usize) -> Option<usize> {
        (self.rows().iter()).rposition(|row| row.node.sessions.contains(&index))
    }

    /// What deleting the sessions at `indexes` would remove: those not running, and their size
    pub fn deletable_in(&self, indexes: &[usize]) -> (usize, u64) {
        let sessions = indexes.iter().map(|&index| &self.sessions[index]);
        let deletable = sessions.filter(|session| !session.running);
        deletable.fold((0, 0), |(count, size), session| {
            (count + 1, size + session.size)
        })
    }

    /// Builds the views from the sessions
    fn build(&mut self) {
        self.tree = session_tree(&self.sessions, &self.group_by);
        self.list = session_list(&self.sessions);
    }

    /// Reads the folder again, the groups staying open and the same row staying selected when
    /// it is still there, else the row holding its newest session
    fn reload(&mut self) {
        let kept = self.selected_row().map(|row| {
            let newest = row.node.sessions.last();
            let stem = newest.map(|&index| self.sessions[index].stem.clone());
            (row.node.key.clone(), stem)
        });
        self.sessions = list_sessions(&self.dir).unwrap_or_default();
        self.build();
        let kept = kept.and_then(|(key, stem)| {
            let same_key = self.rows().iter().position(|row| row.node.key == key);
            same_key.or_else(|| {
                let stem = stem?;
                let index = self.sessions.iter().position(|s| s.stem == stem)?;
                self.row_holding(index)
            })
        });
        self.select(kept.unwrap_or(self.selected));
    }

    fn select(&mut self, idx: usize) {
        self.selected = idx.min(self.rows().len().saturating_sub(1));
        self.preview = self.read_preview();
    }

    /// The selected row's last text lines, at most `PREVIEW_LINES`: a session's, or a group's,
    /// oldest first, a separator after each session. Read from the newest back, so a big group
    /// costs what the preview shows (MAX-202)
    fn read_preview(&self) -> Vec<PreviewLine> {
        let Some(row) = self.selected_row() else {
            return Vec::new();
        };
        let separators = usize::from(row.node.kind != NodeKind::Session);
        let mut left = PREVIEW_LINES;
        let mut parts = Vec::new();
        for &index in row.node.sessions.iter().rev() {
            if left == 0 {
                break;
            }
            let session = &self.sessions[index];
            let tail = session
                .text_path()
                .and_then(|path| read_tail_lines(path, left - separators).ok())
                .unwrap_or_default();
            let mut part: Vec<PreviewLine> = String::from_utf8_lossy(&tail)
                .lines()
                .map(|line| PreviewLine {
                    text: line.to_string(),
                    session: Some(index),
                })
                .collect();
            if separators > 0 {
                part.push(PreviewLine {
                    text: group_separator(session),
                    session: None,
                });
            }
            left = left.saturating_sub(part.len());
            parts.push(part);
        }
        parts.into_iter().rev().flatten().collect()
    }

    /// Tree to list and back, on the row holding the selected row's newest session
    fn switch_view(&mut self) {
        let newest = self.selected_row();
        let newest = newest.and_then(|row| row.node.sessions.last().copied());
        self.view = self.view.other();
        let kept = newest.and_then(|index| self.row_holding(index));
        self.select(kept.unwrap_or(0));
    }

    fn open_selected(&mut self) {
        self.open.extend(self.selected_group());
    }

    fn toggle_selected(&mut self) {
        if let Some(key) = self.selected_group()
            && !self.open.remove(&key)
        {
            self.open.insert(key);
        }
    }

    /// Closes the selected group when it is open, else goes to its parent
    fn close_selected(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        let (key, open, parent) = (row.node.key.clone(), row.open, row.parent);
        if open {
            self.open.remove(&key);
        } else if let Some(parent) = parent {
            self.select(parent);
        }
    }

    pub fn handle_action(&mut self, action: DialogAction) -> OverlayResponse {
        if self.confirm_delete.is_some() {
            return self.handle_confirm(action);
        }
        let last = self.rows().len().saturating_sub(1);
        match action {
            DialogAction::Close => return OverlayResponse::Close,
            DialogAction::Up => self.select(self.selected.saturating_sub(1)),
            DialogAction::Down => self.select((self.selected + 1).min(last)),
            DialogAction::PageUp => self.select(self.selected.saturating_sub(PAGE)),
            DialogAction::PageDown => self.select((self.selected + PAGE).min(last)),
            DialogAction::Home => self.select(0),
            DialogAction::End => self.select(last),
            DialogAction::Right => self.open_selected(),
            DialogAction::Left => self.close_selected(),
            // a click selects, a click on the selected group opens or closes it
            DialogAction::Pick(PopupTarget::SessionRow(idx))
                if idx == self.selected && self.selected_group().is_some() =>
            {
                self.toggle_selected();
            }
            DialogAction::Pick(PopupTarget::SessionRow(idx)) => self.select(idx),
            DialogAction::Confirm if self.selected_group().is_some() => self.toggle_selected(),
            DialogAction::Confirm | DialogAction::Pick(PopupTarget::SessionReplay) => {
                return self.replay_selected();
            }
            DialogAction::Pick(PopupTarget::SessionBackToLive) => return self.back_to_live(),
            DialogAction::Pick(PopupTarget::SessionDelete) => return self.delete_selected(),
            DialogAction::Pick(PopupTarget::SessionDeleteAll) => return self.ask_delete_all(),
            DialogAction::Pick(PopupTarget::SessionView) => self.switch_view(),
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
                KeyCode::Char('s') => self.switch_view(),
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
        match self.confirm_delete.take() {
            Some(confirm) if yes => self.delete_confirmed(&confirm.sessions),
            _ => OverlayResponse::Handled,
        }
    }

    /// The selected session's `.raw` to replay, with its meta's encoding and line settings. Not a
    /// session still running, here or in another maxicom: its session backup grows, and on Windows
    /// its lock keeps it from being read. The live link's own session, which a replay paused, goes
    /// back to it instead
    fn replay_selected(&self) -> OverlayResponse {
        let Some(session) = self.selected_session() else {
            return self.replay_selected_group();
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

    /// The finished sessions of the selected group with a `.raw`, oldest first, to load at once:
    /// a group with the running session stops just before it (MAX-202)
    fn replay_selected_group(&self) -> OverlayResponse {
        let Some(row) = self.selected_row() else {
            return OverlayResponse::Handled;
        };
        let sessions: Vec<Session> = (row.node.sessions.iter())
            .map(|&index| &self.sessions[index])
            .filter(|session| !session.running && session.raw_path().is_some())
            .cloned()
            .collect();
        if sessions.is_empty() {
            return notify(
                "This group has no finished session to replay".to_string(),
                true,
            );
        }
        OverlayResponse::ReplayGroup(sessions)
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
            return self.ask_delete_group();
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
        self.ask_delete(ConfirmDelete {
            sessions: (0..self.sessions.len()).collect(),
            group: None,
        })
    }

    /// The question before deleting the selected group's sessions (MAX-202)
    fn ask_delete_group(&mut self) -> OverlayResponse {
        let Some(row) = self.selected_row() else {
            return OverlayResponse::Handled;
        };
        let confirm = ConfirmDelete {
            sessions: row.node.sessions.clone(),
            group: Some(row.node.label.clone()),
        };
        self.ask_delete(confirm)
    }

    fn ask_delete(&mut self, confirm: ConfirmDelete) -> OverlayResponse {
        if self.deletable_in(&confirm.sessions).0 == 0 {
            return notify("No session to delete".to_string(), false);
        }
        self.confirm_delete = Some(confirm);
        OverlayResponse::Handled
    }

    fn delete_confirmed(&mut self, indexes: &[usize]) -> OverlayResponse {
        let entries: Vec<Session> = (indexes.iter())
            .map(|&index| self.sessions[index].clone())
            .collect();
        let report = delete_sessions(&entries);
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
