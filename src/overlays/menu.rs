use crate::{
    configs::{KeyBindingAction, Mode},
    inputs::DialogAction,
    link::LinkKind,
    overlays::{OverlayResponse, PopupTarget},
};

/// The right-click menu (MAX-130): the main actions, opened at the pointer. Running a row runs
/// its keymap action, as its key would
#[derive(Debug, Clone)]
pub struct ActionMenu {
    /// The screen cell it was opened on, its top-left corner when it fits there
    pub at: (u16, u16),
    pub items: Vec<(&'static str, KeyBindingAction)>,
    pub selected: usize,
}

impl ActionMenu {
    /// The rows for the current mode, the link's kind and whether a recording runs. The
    /// mouse toggle is left out: a mouse user would lose the way back to the menu
    pub fn new(at: (u16, u16), mode: Mode, link: LinkKind, recording: bool) -> Self {
        let switch_mode = match mode {
            Mode::Line => "Switch to TERM",
            Mode::Terminal => "Switch to LINE",
        };
        // a replay started while running goes back to the live link (MAX-168)
        let attach = match link {
            LinkKind::Serial | LinkKind::Tcp => "Detach / attach",
            LinkKind::Replay => "Back to live",
        };
        let record = if recording {
            "Stop recording"
        } else {
            "Record…"
        };
        let mut items = vec![
            ("Help", KeyBindingAction::Help),
            ("Settings", KeyBindingAction::Settings),
            (link.name(), KeyBindingAction::LinkSettings),
            ("Directory…", KeyBindingAction::Directory),
            (switch_mode, KeyBindingAction::SKInputMode),
            (record, KeyBindingAction::Record),
            ("Search", KeyBindingAction::EnterSearch),
        ];
        if mode == Mode::Terminal {
            items.extend([
                ("Screenshot", KeyBindingAction::ScreenshotBytes),
                ("Terminal size", KeyBindingAction::ResizeDialog),
            ]);
        }
        items.extend([
            ("Sessions…", KeyBindingAction::Sessions),
            ("Triggers…", KeyBindingAction::Triggers),
            ("Clear", KeyBindingAction::Clear),
            ("Clear history", KeyBindingAction::ClearHistory),
            (attach, KeyBindingAction::ToggleAttach),
            ("Reload config", KeyBindingAction::ReloadConfig),
            ("Quit…", KeyBindingAction::Quit),
        ]);
        Self {
            at,
            items,
            selected: 0,
        }
    }

    pub fn handle_action(&mut self, action: DialogAction) -> OverlayResponse {
        let last = self.items.len().saturating_sub(1);
        match action {
            DialogAction::Close => return OverlayResponse::Close,
            DialogAction::Confirm => return OverlayResponse::Run(self.items[self.selected].1),
            DialogAction::Pick(PopupTarget::MenuItem(idx)) => {
                if let Some(&(_, action)) = self.items.get(idx) {
                    return OverlayResponse::Run(action);
                }
            }
            DialogAction::Up => self.selected = self.selected.checked_sub(1).unwrap_or(last),
            DialogAction::Down => self.selected = (self.selected + 1) % self.items.len().max(1),
            DialogAction::Home => self.selected = 0,
            DialogAction::End => self.selected = last,
            _ => {}
        }
        OverlayResponse::Handled
    }
}

#[cfg(test)]
#[path = "../tests/overlays/menu.rs"]
mod tests;
