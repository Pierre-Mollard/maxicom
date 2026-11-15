use std::path::PathBuf;

use crossterm::event::KeyCode;

use crate::{
    configs::profiles::Listed,
    inputs::DialogAction,
    overlays::{OverlayResponse, PopupTarget, copy_path},
};

/// How far PageUp and PageDown move the selection
const PAGE: usize = 10;

/// The profiles popup (MAX-240): the profiles of the folder, sorted by name, the laid ones
/// with their place in the stack, read from the app on each frame. Enter, Tab, Space, a
/// click on the selected row or its button lays the selected one on top of the stack or takes
/// it off, Right (l) lays it and Left (h) takes it off, as the trigger browser arms and
/// disarms; `K` and `J` move a laid one up and down the stack; `c` copies the folder's path. The
/// app refuses to lay an off profile or one that doesn't read, saying why. The rows are the
/// folder's when it opened, the laid ones with no file after them
#[derive(Debug, Clone, Default)]
pub struct ProfileBrowser {
    pub rows: Vec<Listed>,
    pub path: PathBuf,
    pub selected: usize,
}

impl ProfileBrowser {
    pub fn new(rows: Vec<Listed>, path: PathBuf) -> Self {
        Self {
            rows,
            path,
            selected: 0,
        }
    }

    pub fn handle_action(&mut self, action: DialogAction) -> OverlayResponse {
        let last = self.rows.len().saturating_sub(1);
        match action {
            DialogAction::Close => return OverlayResponse::Close,
            DialogAction::Up => self.selected = self.selected.saturating_sub(1),
            DialogAction::Down => self.selected = (self.selected + 1).min(last),
            DialogAction::PageUp => self.selected = self.selected.saturating_sub(PAGE),
            DialogAction::PageDown => self.selected = (self.selected + PAGE).min(last),
            DialogAction::Home => self.selected = 0,
            DialogAction::End => self.selected = last,
            DialogAction::Confirm
            | DialogAction::CycleNext
            | DialogAction::CyclePrev
            | DialogAction::Pick(PopupTarget::ProfileToggle) => return self.toggle_selected(),
            // a click selects, a click on the selected one toggles it
            DialogAction::Pick(PopupTarget::ProfileRow(idx)) if idx == self.selected => {
                return self.toggle_selected();
            }
            DialogAction::Pick(PopupTarget::ProfileRow(idx)) => self.selected = idx.min(last),
            DialogAction::Left => return self.set_selected(false),
            DialogAction::Right => return self.set_selected(true),
            DialogAction::Pick(PopupTarget::ProfileMove(up)) => return self.move_selected(up),
            DialogAction::Pick(PopupTarget::ProfileCopyPath) => {
                return copy_path(&self.path, "profiles folder");
            }
            DialogAction::HandleKey(key) => match key.code {
                KeyCode::Char(' ') => return self.toggle_selected(),
                KeyCode::Char('K') => return self.move_selected(true),
                KeyCode::Char('J') => return self.move_selected(false),
                KeyCode::Char('c') => return copy_path(&self.path, "profiles folder"),
                _ => {}
            },
            _ => {}
        }
        OverlayResponse::Handled
    }

    /// The selected profile laid or taken off
    fn toggle_selected(&self) -> OverlayResponse {
        match self.rows.get(self.selected) {
            Some(row) => OverlayResponse::ToggleProfile(row.name.clone()),
            None => OverlayResponse::Handled,
        }
    }

    /// The selected profile laid (`on`) or taken off, whatever it is now
    fn set_selected(&self, on: bool) -> OverlayResponse {
        match self.rows.get(self.selected) {
            Some(row) => OverlayResponse::SetProfile {
                name: row.name.clone(),
                on,
            },
            None => OverlayResponse::Handled,
        }
    }

    /// The selected profile one place up the stack (`up`), or down
    fn move_selected(&self, up: bool) -> OverlayResponse {
        match self.rows.get(self.selected) {
            Some(row) => OverlayResponse::MoveProfile {
                name: row.name.clone(),
                up,
            },
            None => OverlayResponse::Handled,
        }
    }
}

#[cfg(test)]
#[path = "../tests/overlays/profiles.rs"]
mod tests;
