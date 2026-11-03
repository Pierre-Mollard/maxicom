use std::{cell::Cell, collections::HashMap};

use crossterm::event::KeyCode;

use crate::{
    configs::links::SavedLink,
    inputs::DialogAction,
    link::{
        Address, LinkConfig, LinkKind,
        probe::ProbeState,
        serial::{self, PortDetails},
    },
    overlays::{OverlayResponse, PopupTarget},
};

/// How far PageUp and PageDown move the selection
const PAGE: usize = 10;

/// The directory (MAX-181), as minicom's dialing directory, in two tabs (MAX-185): the saved
/// links of this machine, by name with their line settings, or why `links.toml` didn't load;
/// the serial ports the system lists. Each tab has its selection, on the link or the device
/// in use when listed. Enter, the button or a click on the selected row opens the selected
/// one: a port as `open` does (its line settings kept), a link with its own (a TCP link has
/// none). `opening` holds the address until it's open, which closes the directory, or its
/// error in `tests`. `s` saves the link in use from the saved links, the selected port from
/// the ports, in the link form filled from it; on the saved links `n` adds one and `e` edits
/// the selected one in the same form, `x` or Del deletes it, asked first. Tab, Shift-Tab, Left
/// and Right switch tabs. On the ports, `r` lists them again, the selection kept on its name.
/// `t` tests the links of both tabs, each one's last result in `tests` by address (`None`
/// while it runs, MAX-186). The ports' `details` are read with the list (MAX-188). `width` is
/// the widest the popup was drawn: it doesn't shrink while open, as a state gets shorter
#[derive(Debug, Clone)]
pub struct Directory {
    pub tab: DirectoryTab,
    pub links: Result<Vec<(SavedLink, LinkConfig)>, String>,
    pub link_selected: usize,
    pub ports: Vec<String>,
    pub details: HashMap<String, PortDetails>,
    pub selected: usize,
    pub tests: HashMap<String, Option<ProbeState>>,
    pub opening: Option<String>,
    pub width: Cell<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryTab {
    Saved,
    Ports,
}

impl Directory {
    pub fn new(ports: Vec<String>, current: &Address) -> Self {
        let mut directory = Self {
            tab: DirectoryTab::Ports,
            links: Ok(Vec::new()),
            link_selected: 0,
            ports: Vec::new(),
            details: HashMap::new(),
            selected: 0,
            tests: HashMap::new(),
            opening: None,
            width: Cell::new(0),
        };
        directory.set_ports(ports, Some(&current.to_string()));
        directory
    }

    /// The saved links, the tab on them when there's one or an error to show
    pub fn with_links(
        mut self,
        links: Result<Vec<(SavedLink, LinkConfig)>, String>,
        current: &Address,
    ) -> Self {
        if links.as_ref().map_or(true, |links| !links.is_empty()) {
            self.tab = DirectoryTab::Saved;
        }
        let in_use = links
            .as_ref()
            .ok()
            .and_then(|links| (links.iter()).position(|(_, config)| config.address == *current));
        self.link_selected = in_use.unwrap_or(0);
        self.links = links;
        self
    }

    /// The rows of the tab shown
    fn len(&self) -> usize {
        match self.tab {
            DirectoryTab::Saved => self.links.as_ref().map_or(0, Vec::len),
            DirectoryTab::Ports => self.ports.len(),
        }
    }

    /// The selection of the tab shown
    pub fn selection(&self) -> usize {
        match self.tab {
            DirectoryTab::Saved => self.link_selected,
            DirectoryTab::Ports => self.selected,
        }
    }

    fn select(&mut self, idx: usize) {
        let idx = idx.min(self.len().saturating_sub(1));
        match self.tab {
            DirectoryTab::Saved => self.link_selected = idx,
            DirectoryTab::Ports => self.selected = idx,
        }
    }

    fn other_tab(&mut self) {
        self.tab = match self.tab {
            DirectoryTab::Saved => DirectoryTab::Ports,
            DirectoryTab::Ports => DirectoryTab::Saved,
        };
    }

    /// The ports listed again, the selection on `keep` while it's there
    fn set_ports(&mut self, ports: Vec<String>, keep: Option<&str>) {
        let kept = keep.and_then(|name| ports.iter().position(|port| port == name));
        self.selected = kept.unwrap_or(self.selected.min(ports.len().saturating_sub(1)));
        self.details = serial::port_details(&ports);
        self.ports = ports;
    }

    pub fn handle_action(&mut self, action: DialogAction) -> OverlayResponse {
        let (selected, ports) = (self.selection(), self.tab == DirectoryTab::Ports);
        match action {
            DialogAction::Close => return OverlayResponse::Close,
            DialogAction::Up => self.select(selected.saturating_sub(1)),
            DialogAction::Down => self.select(selected + 1),
            DialogAction::PageUp => self.select(selected.saturating_sub(PAGE)),
            DialogAction::PageDown => self.select(selected + PAGE),
            DialogAction::Home => self.select(0),
            DialogAction::End => self.select(usize::MAX),
            DialogAction::CycleNext
            | DialogAction::CyclePrev
            | DialogAction::Left
            | DialogAction::Right => self.other_tab(),
            DialogAction::Pick(PopupTarget::DirectoryTab(tab)) => self.tab = tab,
            DialogAction::Confirm | DialogAction::Pick(PopupTarget::DirectoryOpen) => {
                return self.open_selected();
            }
            // a click selects, a click on the selected one opens it
            DialogAction::Pick(PopupTarget::DirectoryRow(idx)) if idx == selected => {
                return self.open_selected();
            }
            DialogAction::Pick(PopupTarget::DirectoryRow(idx)) => self.select(idx),
            DialogAction::Pick(PopupTarget::DirectorySave) => return self.save_selected(),
            DialogAction::Pick(PopupTarget::DirectoryNew) if !ports => {
                return OverlayResponse::NewLink;
            }
            DialogAction::Pick(PopupTarget::DirectoryEdit) if !ports => {
                return self.edit_selected();
            }
            DialogAction::Pick(PopupTarget::DirectoryDelete) if !ports => {
                return self.delete_selected();
            }
            DialogAction::Pick(PopupTarget::DirectoryRefresh) if ports => self.refresh(),
            DialogAction::Pick(PopupTarget::DirectoryTest) => return OverlayResponse::TestLinks,
            DialogAction::HandleKey(key) => match key.code {
                KeyCode::Char('s') => return self.save_selected(),
                KeyCode::Char('n') if !ports => return OverlayResponse::NewLink,
                KeyCode::Char('e') if !ports => return self.edit_selected(),
                KeyCode::Char('x') | KeyCode::Delete if !ports => return self.delete_selected(),
                KeyCode::Char('r') if ports => self.refresh(),
                KeyCode::Char('t') => return OverlayResponse::TestLinks,
                _ => {}
            },
            _ => {}
        }
        OverlayResponse::Handled
    }

    /// The link in use from the saved links, the selected port from the ports
    fn save_selected(&self) -> OverlayResponse {
        if self.tab == DirectoryTab::Saved {
            return OverlayResponse::NameLink(None);
        }
        match self.ports.get(self.selected).map(|port| port.parse()) {
            Some(Ok(address)) => OverlayResponse::NameLink(Some(address)),
            Some(Err(message)) => OverlayResponse::HandledNotify {
                message,
                warn: true,
            },
            None => OverlayResponse::Handled,
        }
    }

    fn edit_selected(&self) -> OverlayResponse {
        match self
            .links
            .as_ref()
            .ok()
            .and_then(|l| l.get(self.link_selected))
        {
            Some((link, _)) => OverlayResponse::EditLink(link.clone()),
            None => OverlayResponse::Handled,
        }
    }

    fn delete_selected(&self) -> OverlayResponse {
        match self
            .links
            .as_ref()
            .ok()
            .and_then(|l| l.get(self.link_selected))
        {
            Some((link, _)) => OverlayResponse::AskDeleteLink(link.clone()),
            None => OverlayResponse::Handled,
        }
    }

    fn refresh(&mut self) {
        let keep = self.ports.get(self.selected).cloned();
        self.set_ports(serial::port_names(), keep.as_deref());
    }

    fn open_selected(&mut self) -> OverlayResponse {
        if self.tab == DirectoryTab::Saved {
            let link = self
                .links
                .as_ref()
                .ok()
                .and_then(|l| l.get(self.link_selected));
            let Some((_, config)) = link.cloned() else {
                return OverlayResponse::Handled;
            };
            self.opening = Some(config.address.to_string());
            return match config.address.kind() {
                LinkKind::Tcp => OverlayResponse::Open(config.address),
                _ => OverlayResponse::OpenLink(config),
            };
        }
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
#[path = "../tests/overlays/directory.rs"]
mod tests;
