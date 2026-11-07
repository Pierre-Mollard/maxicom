use chrono::{DateTime, Local};
use crossterm::event::{KeyCode, KeyModifiers};

use crate::{
    inputs::DialogAction,
    overlays::{OverlayResponse, PopupTarget},
};

/// How far PageUp and PageDown move the selection
const PAGE: usize = 10;

/// A mark as the popup lists it: its letter, when its line came in, the line's text with its
/// escape sequences
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkRow {
    pub letter: char,
    pub time: Option<DateTime<Local>>,
    pub text: String,
}

/// The marks popup (MAX-164): the current mode's marks by line, as they were when it opened.
/// A letter jumps to its mark at once, like vim's `'a`; Enter or a click on the selected row
/// jumps to it, Delete removes it. A jump goes by letter, so a mark that moved with its line
/// since is still found
#[derive(Debug, Clone, Default)]
pub struct MarkBrowser {
    pub rows: Vec<MarkRow>,
    pub selected: usize,
}

impl MarkBrowser {
    pub fn new(rows: Vec<MarkRow>) -> Self {
        Self { rows, selected: 0 }
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
            DialogAction::Confirm | DialogAction::Pick(PopupTarget::MarkJump) => {
                return self.jump_selected();
            }
            DialogAction::Pick(PopupTarget::MarkDelete) => return self.delete_selected(),
            // a click selects, a click on the selected one jumps
            DialogAction::Pick(PopupTarget::MarkRow(idx)) if idx == self.selected => {
                return self.jump_selected();
            }
            DialogAction::Pick(PopupTarget::MarkRow(idx)) => self.selected = idx.min(last),
            DialogAction::HandleKey(key) => match key.code {
                KeyCode::Delete => return self.delete_selected(),
                KeyCode::Char(letter)
                    if letter.is_ascii_lowercase()
                        && !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    return OverlayResponse::JumpMark(letter);
                }
                _ => {}
            },
            _ => {}
        }
        OverlayResponse::Handled
    }

    fn jump_selected(&self) -> OverlayResponse {
        match self.rows.get(self.selected) {
            Some(row) => OverlayResponse::JumpMark(row.letter),
            None => OverlayResponse::Handled,
        }
    }

    /// The selected mark taken off the list, the popup kept open on the next one
    fn delete_selected(&mut self) -> OverlayResponse {
        if self.selected >= self.rows.len() {
            return OverlayResponse::Handled;
        }
        let row = self.rows.remove(self.selected);
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
        OverlayResponse::DeleteMark(row.letter)
    }
}

#[cfg(test)]
#[path = "../tests/overlays/marks.rs"]
mod tests;
