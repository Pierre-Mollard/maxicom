use std::path::PathBuf;

use crossterm::event::KeyCode;

use crate::{
    inputs::DialogAction,
    overlays::{OverlayResponse, PopupTarget, copy_path},
};

/// How far PageUp and PageDown move the selection
const PAGE: usize = 10;

/// The trigger browser (MAX-167): the rules of `triggers.toml` and whether each is armed, the
/// selected one's config on the side. Enter, Tab, Space, a click on the selected row or its
/// button toggles it; Left (h) disarms it and Right (l) arms it (MAX-163); `a` toggles them
/// all, `c` copies the file's path. The names are the app's when it opened (a reload closes
/// nothing, a row past the rules is ignored); the states are read from the app on each frame
#[derive(Debug, Clone, Default)]
pub struct TriggerBrowser {
    pub names: Vec<String>,
    pub path: PathBuf,
    pub selected: usize,
}

impl TriggerBrowser {
    pub fn new(names: Vec<String>, path: PathBuf) -> Self {
        Self {
            names,
            path,
            selected: 0,
        }
    }

    pub fn handle_action(&mut self, action: DialogAction) -> OverlayResponse {
        let last = self.names.len().saturating_sub(1);
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
            | DialogAction::Pick(PopupTarget::TriggerToggle) => return self.toggle_selected(),
            DialogAction::Left => return self.arm_selected(false),
            DialogAction::Right => return self.arm_selected(true),
            // a click selects, a click on the selected one toggles it
            DialogAction::Pick(PopupTarget::TriggerRow(idx)) if idx == self.selected => {
                return self.toggle_selected();
            }
            DialogAction::Pick(PopupTarget::TriggerRow(idx)) => self.selected = idx.min(last),
            DialogAction::Pick(PopupTarget::TriggerToggleAll) => {
                return OverlayResponse::ToggleTrigger(None);
            }
            DialogAction::Pick(PopupTarget::TriggerCopyPath) => {
                return copy_path(&self.path, "triggers file");
            }
            DialogAction::HandleKey(key) => match key.code {
                KeyCode::Char(' ') => return self.toggle_selected(),
                KeyCode::Char('a') => return OverlayResponse::ToggleTrigger(None),
                KeyCode::Char('c') => return copy_path(&self.path, "triggers file"),
                _ => {}
            },
            _ => {}
        }
        OverlayResponse::Handled
    }

    fn toggle_selected(&self) -> OverlayResponse {
        match self.names.get(self.selected) {
            Some(name) => OverlayResponse::ToggleTrigger(Some(name.clone())),
            None => OverlayResponse::Handled,
        }
    }

    /// The selected one armed (`on`) or disarmed, whatever it is now
    fn arm_selected(&self, on: bool) -> OverlayResponse {
        match self.names.get(self.selected) {
            Some(name) => OverlayResponse::ArmTrigger {
                name: name.clone(),
                on,
            },
            None => OverlayResponse::Handled,
        }
    }
}

#[cfg(test)]
#[path = "../tests/overlays/triggers.rs"]
mod tests;
