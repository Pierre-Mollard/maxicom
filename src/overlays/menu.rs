use crate::{
    app::types::SidePanel,
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
    pub items: Vec<(&'static str, MenuRun)>,
    pub selected: usize,
}

/// What a row of the menu runs: a keymap action, a mark on the history line it was opened
/// on, none when it was opened off the history (MAX-164), or the side panel it was opened on
/// closed (MAX-230)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuRun {
    Action(KeyBindingAction),
    AddMark(Option<usize>),
    ClosePanel(SidePanel),
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
            items: (items.into_iter())
                .map(|(label, action)| (label, MenuRun::Action(action)))
                .collect(),
            selected: 0,
        }
    }

    /// The marks' rows after Search (MAX-164): "Add mark" when `add` is given, for a menu
    /// opened on a scrolled view or in copy mode, on the history line clicked; "Marks…" when
    /// there are marks to list
    pub fn with_marks(mut self, add: Option<Option<usize>>, listed: bool) -> Self {
        let search = (self.items.iter())
            .position(|&(_, run)| run == MenuRun::Action(KeyBindingAction::EnterSearch))
            .map_or(self.items.len(), |idx| idx + 1);
        let add = add.map(|line| ("Add mark", MenuRun::AddMark(line)));
        let list = listed.then_some(("Marks…", MenuRun::Action(KeyBindingAction::Marks)));
        self.items
            .splice(search..search, add.into_iter().chain(list));
        self
    }

    /// Opened on a side panel, its "Close" row first (MAX-230)
    pub fn with_close(mut self, panel: Option<SidePanel>) -> Self {
        let label = match panel {
            Some(SidePanel::Raw) => "Close raw panel",
            Some(SidePanel::Plot) => "Close plot",
            None => return self,
        };
        self.items
            .insert(0, (label, MenuRun::ClosePanel(panel.unwrap())));
        self
    }

    pub fn handle_action(&mut self, action: DialogAction) -> OverlayResponse {
        let last = self.items.len().saturating_sub(1);
        match action {
            DialogAction::Close => return OverlayResponse::Close,
            DialogAction::Confirm => return self.run(self.selected),
            DialogAction::Pick(PopupTarget::MenuItem(idx)) => return self.run(idx),
            DialogAction::Up => self.selected = self.selected.checked_sub(1).unwrap_or(last),
            DialogAction::Down => self.selected = (self.selected + 1) % self.items.len().max(1),
            DialogAction::Home => self.selected = 0,
            DialogAction::End => self.selected = last,
            _ => {}
        }
        OverlayResponse::Handled
    }

    fn run(&self, idx: usize) -> OverlayResponse {
        match self.items.get(idx) {
            Some(&(_, MenuRun::Action(action))) => OverlayResponse::Run(action),
            Some(&(_, MenuRun::AddMark(line))) => OverlayResponse::AddMark(line),
            Some(&(_, MenuRun::ClosePanel(panel))) => OverlayResponse::ClosePanel(panel),
            None => OverlayResponse::Handled,
        }
    }
}

#[cfg(test)]
#[path = "../tests/overlays/menu.rs"]
mod tests;
