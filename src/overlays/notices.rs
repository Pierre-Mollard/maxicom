use crossterm::event::{KeyCode, KeyModifiers};

use crate::{
    inputs::DialogAction,
    notification::Notice,
    overlays::{OverlayResponse, PopupTarget},
};

/// How far PageUp and PageDown move the selection
const PAGE: usize = 10;

/// The notifications popup (MAX-276): what the toasts said, oldest first, as it was when it
/// opened, the newest selected. Read only: Enter, `c` or its button copies the selected text
#[derive(Debug, Clone, Default)]
pub struct NoticeBrowser {
    pub notices: Vec<Notice>,
    pub selected: usize,
}

impl NoticeBrowser {
    pub fn new(notices: Vec<Notice>) -> Self {
        let selected = notices.len().saturating_sub(1);
        Self { notices, selected }
    }

    pub fn handle_action(&mut self, action: DialogAction) -> OverlayResponse {
        let last = self.notices.len().saturating_sub(1);
        match action {
            DialogAction::Close => return OverlayResponse::Close,
            DialogAction::Up => self.selected = self.selected.saturating_sub(1),
            DialogAction::Down => self.selected = (self.selected + 1).min(last),
            DialogAction::PageUp => self.selected = self.selected.saturating_sub(PAGE),
            DialogAction::PageDown => self.selected = (self.selected + PAGE).min(last),
            DialogAction::Home => self.selected = 0,
            DialogAction::End => self.selected = last,
            DialogAction::Confirm | DialogAction::Pick(PopupTarget::NoticeCopy) => {
                return self.copy_selected();
            }
            DialogAction::Pick(PopupTarget::NoticeRow(idx)) => self.selected = idx.min(last),
            DialogAction::HandleKey(key)
                if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::NONE =>
            {
                return self.copy_selected();
            }
            _ => {}
        }
        OverlayResponse::Handled
    }

    fn copy_selected(&self) -> OverlayResponse {
        match self.notices.get(self.selected) {
            Some(notice) => OverlayResponse::HandledCopy {
                text: notice.message.clone(),
                what: "notification".to_string(),
            },
            None => OverlayResponse::Handled,
        }
    }
}

#[cfg(test)]
#[path = "../tests/overlays/notices.rs"]
mod tests;
