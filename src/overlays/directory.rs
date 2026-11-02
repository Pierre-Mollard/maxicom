use std::collections::HashMap;

use crossterm::event::KeyCode;

use crate::{
    inputs::DialogAction,
    link::{
        Address,
        serial::{self, PortState},
    },
    overlays::{OverlayResponse, PopupTarget},
};

/// How far PageUp and PageDown move the selection
const PAGE: usize = 10;

/// The directory (MAX-181), as minicom's dialing directory: the serial ports the system lists,
/// the device in use selected when it's one of them (the saved links of MAX-185 come in a tab
/// of their own). Enter, the button or a click on the selected row opens the selected one, as
/// `open` does (its line settings kept): `opening` until it's open, which closes the
/// directory, or its error in `tests`; `r` lists the ports again, the selection kept on its
/// name; `t` tests them, each port's last result in `tests` by name (`None` while it runs)
#[derive(Debug, Clone, Default)]
pub struct Directory {
    pub ports: Vec<String>,
    pub selected: usize,
    pub tests: HashMap<String, Option<PortState>>,
    pub opening: Option<String>,
}

impl Directory {
    pub fn new(ports: Vec<String>, current: &Address) -> Self {
        let mut directory = Self::default();
        directory.set_ports(ports, Some(&current.to_string()));
        directory
    }

    /// The ports listed again, the selection on `keep` while it's there
    fn set_ports(&mut self, ports: Vec<String>, keep: Option<&str>) {
        let kept = keep.and_then(|name| ports.iter().position(|port| port == name));
        self.selected = kept.unwrap_or(self.selected.min(ports.len().saturating_sub(1)));
        self.ports = ports;
    }

    pub fn handle_action(&mut self, action: DialogAction) -> OverlayResponse {
        let last = self.ports.len().saturating_sub(1);
        match action {
            DialogAction::Close => return OverlayResponse::Close,
            DialogAction::Up => self.selected = self.selected.saturating_sub(1),
            DialogAction::Down => self.selected = (self.selected + 1).min(last),
            DialogAction::PageUp => self.selected = self.selected.saturating_sub(PAGE),
            DialogAction::PageDown => self.selected = (self.selected + PAGE).min(last),
            DialogAction::Home => self.selected = 0,
            DialogAction::End => self.selected = last,
            DialogAction::Confirm | DialogAction::Pick(PopupTarget::DirectoryOpen) => {
                return self.open_selected();
            }
            // a click selects, a click on the selected one opens it
            DialogAction::Pick(PopupTarget::DirectoryRow(idx)) if idx == self.selected => {
                return self.open_selected();
            }
            DialogAction::Pick(PopupTarget::DirectoryRow(idx)) => self.selected = idx.min(last),
            DialogAction::Pick(PopupTarget::DirectoryRefresh) => self.refresh(),
            DialogAction::Pick(PopupTarget::DirectoryTest) => return OverlayResponse::TestPorts,
            DialogAction::HandleKey(key) => match key.code {
                KeyCode::Char('r') => self.refresh(),
                KeyCode::Char('t') => return OverlayResponse::TestPorts,
                _ => {}
            },
            _ => {}
        }
        OverlayResponse::Handled
    }

    fn refresh(&mut self) {
        let keep = self.ports.get(self.selected).cloned();
        self.set_ports(serial::port_names(), keep.as_deref());
    }

    fn open_selected(&mut self) -> OverlayResponse {
        match self.ports.get(self.selected).map(|port| port.parse()) {
            Some(Ok(address)) => {
                self.opening = self.ports.get(self.selected).cloned();
                OverlayResponse::Open(address)
            }
            Some(Err(err)) => OverlayResponse::HandledNotify {
                message: err,
                warn: true,
            },
            None => OverlayResponse::Handled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opens(response: OverlayResponse) -> Option<String> {
        match response {
            OverlayResponse::Open(address) => Some(address.to_string()),
            _ => None,
        }
    }

    /// MAX-181: the device in use is selected when listed, the first port else; moves stay
    /// in the list; Enter or the button opens the selected one, a click selects then opens;
    /// an empty list opens nothing.
    /// How: three ports with the second in use, the moves, then each way to open; an empty
    /// list.
    #[test]
    fn select_and_open() {
        let ports = || vec!["COM1".to_string(), "COM3".into(), "COM4".into()];
        let mut directory = Directory::new(ports(), &"COM3".parse().unwrap());
        assert_eq!(directory.selected, 1);
        assert_eq!(
            Directory::new(ports(), &"COM9".parse().unwrap()).selected,
            0
        );

        directory.handle_action(DialogAction::End);
        assert_eq!(directory.selected, 2);
        directory.handle_action(DialogAction::Down);
        assert_eq!(directory.selected, 2);
        directory.handle_action(DialogAction::PageUp);
        assert_eq!(directory.selected, 0);

        let com1 = Some("COM1".to_string());
        assert_eq!(opens(directory.handle_action(DialogAction::Confirm)), com1);
        assert_eq!(directory.opening, com1);
        let button = DialogAction::Pick(PopupTarget::DirectoryOpen);
        assert_eq!(opens(directory.handle_action(button)), com1);
        let row = |idx| DialogAction::Pick(PopupTarget::DirectoryRow(idx));
        assert_eq!(opens(directory.handle_action(row(2))), None);
        assert_eq!(directory.selected, 2);
        let com4 = Some("COM4".to_string());
        assert_eq!(opens(directory.handle_action(row(2))), com4);

        let mut empty = Directory::new(Vec::new(), &"COM3".parse().unwrap());
        assert_eq!(opens(empty.handle_action(DialogAction::Confirm)), None);
        assert!(matches!(
            empty.handle_action(DialogAction::Close),
            OverlayResponse::Close
        ));
    }

    /// MAX-181: listed again, the selection stays on its port wherever it moved, else on its
    /// row within the new list.
    /// How: COM3 selected, then a list with a port before it, then one without it.
    #[test]
    fn refresh_keeps_the_selection() {
        let mut directory = Directory::new(
            vec!["COM1".to_string(), "COM3".into()],
            &"COM3".parse().unwrap(),
        );
        directory.set_ports(
            vec!["COM0".into(), "COM1".into(), "COM3".into()],
            Some("COM3"),
        );
        assert_eq!(directory.selected, 2);
        directory.set_ports(vec!["COM1".into()], Some("COM3"));
        assert_eq!(directory.selected, 0);
    }
}
