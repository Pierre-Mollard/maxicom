use crate::{
    capture::Stream,
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
    /// The rows for the current mode, the link's kind and the state of each recording. The
    /// mouse toggle is left out: a mouse user would lose the way back to the menu
    pub fn new(
        at: (u16, u16),
        mode: Mode,
        link: LinkKind,
        is_recording: impl Fn(Stream) -> bool,
    ) -> Self {
        let switch_mode = match mode {
            Mode::Line => "Switch to TERM",
            Mode::Terminal => "Switch to LINE",
        };
        // a replay started while running goes back to the live link (MAX-168)
        let attach = match link {
            LinkKind::Serial => "Detach / attach port",
            LinkKind::Tcp => "Detach / attach connection",
            LinkKind::Replay => "Back to live link",
        };
        let record_text = if is_recording(Stream::Text) {
            "Stop text recording"
        } else {
            "Record text"
        };
        let record_raw = if is_recording(Stream::Raw) {
            "Stop raw recording"
        } else {
            "Record raw"
        };
        let mut items = vec![
            ("Help", KeyBindingAction::Help),
            ("Settings", KeyBindingAction::Settings),
            (link.name(), KeyBindingAction::LinkSettings),
            (switch_mode, KeyBindingAction::SKInputMode),
            (record_text, KeyBindingAction::RecordText),
            (record_raw, KeyBindingAction::RecordRaw),
            ("Copy mode", KeyBindingAction::EnterCopyMode),
            ("Search", KeyBindingAction::EnterSearch),
        ];
        if mode == Mode::Terminal {
            items.extend([
                ("Screenshot", KeyBindingAction::ScreenshotTxt),
                ("Terminal size", KeyBindingAction::ResizeDialog),
            ]);
        }
        items.extend([
            ("Session backups…", KeyBindingAction::Sessions),
            ("Triggers…", KeyBindingAction::Triggers),
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
mod tests {
    //! The right-click menu's rows and keys (MAX-130).

    use super::*;

    /// The rows follow the mode and each recording's state, both recordings in both modes
    /// (MAX-147); the mouse toggle is never one. The screenshot and the terminal size are
    /// terminal mode's only (MAX-132). The link's rows say its kind (MAX-168).
    /// How: line mode on a serial port with nothing recording, terminal mode on a TCP link
    /// recording text only, then a replay.
    #[test]
    fn rows_follow_the_state() {
        let labels = |menu: &ActionMenu| menu.items.iter().map(|(l, _)| *l).collect::<Vec<_>>();
        let line = ActionMenu::new((0, 0), Mode::Line, LinkKind::Serial, |_| false);
        assert!(labels(&line).contains(&"Switch to TERM"));
        assert!(labels(&line).contains(&"Record text"));
        assert!(labels(&line).contains(&"Record raw"));
        assert!(!labels(&line).contains(&"Screenshot"));
        assert!(!labels(&line).contains(&"Terminal size"));
        assert!(labels(&line).contains(&"Serial port"));
        assert!(labels(&line).contains(&"Detach / attach port"));

        let term = ActionMenu::new((0, 0), Mode::Terminal, LinkKind::Tcp, |stream| {
            stream == Stream::Text
        });
        assert!(labels(&term).contains(&"Switch to LINE"));
        assert!(labels(&term).contains(&"Stop text recording"));
        assert!(labels(&term).contains(&"Record raw"));
        assert!(labels(&term).contains(&"Screenshot"));
        assert!(labels(&term).contains(&"Terminal size"));
        assert!(labels(&term).contains(&"TCP connection"));
        assert!(labels(&term).contains(&"Detach / attach connection"));
        let replay = ActionMenu::new((0, 0), Mode::Terminal, LinkKind::Replay, |_| false);
        assert!(labels(&replay).contains(&"Replay"));
        assert!(labels(&replay).contains(&"Back to live link"));
        assert!(
            !term
                .items
                .iter()
                .any(|&(_, action)| action == KeyBindingAction::ToggleMouse)
        );
    }

    /// Up and Down wrap around, Enter and a click run the row's action, Esc closes.
    /// How: Up from the first row, Down back, Enter; a click on row 1; Esc.
    #[test]
    fn keys_and_clicks() {
        let mut menu = ActionMenu::new((0, 0), Mode::Line, LinkKind::Serial, |_| false);
        let last = menu.items.len() - 1;
        menu.handle_action(DialogAction::Up);
        assert_eq!(menu.selected, last);
        menu.handle_action(DialogAction::Down);
        assert_eq!(menu.selected, 0);
        assert!(matches!(
            menu.handle_action(DialogAction::Confirm),
            OverlayResponse::Run(KeyBindingAction::Help)
        ));
        assert!(matches!(
            menu.handle_action(DialogAction::Pick(PopupTarget::MenuItem(1))),
            OverlayResponse::Run(KeyBindingAction::Settings)
        ));
        assert!(matches!(
            menu.handle_action(DialogAction::Close),
            OverlayResponse::Close
        ));
    }
}
