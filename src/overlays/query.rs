use crossterm::event::{KeyCode, KeyModifiers};

use crate::{
    commands::report::Answer,
    inputs::DialogAction,
    overlays::{OverlayResponse, PopupTarget},
};

/// How far PageUp and PageDown move the selection
const PAGE: usize = 10;

/// A query's answer (MAX-165): its lines as the value shows them, the first selected. Read
/// only: Enter, `c` or its button copies the selected line, `a` or its button the whole answer
#[derive(Debug, Clone)]
pub struct QueryView {
    pub query: &'static str,
    pub lines: Vec<String>,
    pub selected: usize,
}

impl QueryView {
    pub fn new(answer: Answer) -> Self {
        Self {
            query: answer.query,
            lines: answer.value.lines(),
            selected: 0,
        }
    }

    pub fn handle_action(&mut self, action: DialogAction) -> OverlayResponse {
        let last = self.lines.len().saturating_sub(1);
        let key = |c| {
            matches!(&action, DialogAction::HandleKey(key)
                if key.code == KeyCode::Char(c) && key.modifiers == KeyModifiers::NONE)
        };
        let (copy_line, copy_all) = (key('c'), key('a'));
        match action {
            DialogAction::Close => return OverlayResponse::Close,
            DialogAction::Up => self.selected = self.selected.saturating_sub(1),
            DialogAction::Down => self.selected = (self.selected + 1).min(last),
            DialogAction::PageUp => self.selected = self.selected.saturating_sub(PAGE),
            DialogAction::PageDown => self.selected = (self.selected + PAGE).min(last),
            DialogAction::Home => self.selected = 0,
            DialogAction::End => self.selected = last,
            DialogAction::Pick(PopupTarget::QueryRow(idx)) => self.selected = idx.min(last),
            DialogAction::Confirm | DialogAction::Pick(PopupTarget::QueryCopy) => {
                return self.copy_selected();
            }
            DialogAction::Pick(PopupTarget::QueryCopyAll) => return self.copy_all(),
            _ if copy_line => return self.copy_selected(),
            _ if copy_all => return self.copy_all(),
            _ => {}
        }
        OverlayResponse::Handled
    }

    fn copy_selected(&self) -> OverlayResponse {
        match self.lines.get(self.selected) {
            Some(line) => OverlayResponse::HandledCopy {
                text: line.clone(),
                what: format!("{} line", self.query),
            },
            None => OverlayResponse::Handled,
        }
    }

    fn copy_all(&self) -> OverlayResponse {
        OverlayResponse::HandledCopy {
            text: self.lines.join("\n"),
            what: self.query.to_string(),
        }
    }
}

#[cfg(test)]
#[path = "../tests/overlays/query.rs"]
mod tests;
