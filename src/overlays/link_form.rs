use crossterm::event::KeyCode;
use tui_input::Input;

use crate::{
    configs::{KeyBindingAction, LinkDefaults, links::SavedLink},
    inputs::{DialogAction, dispatch_input_key, paste_into_input},
    link::{Address, LinkConfig, serial::LineSettings},
    overlays::{LineSettingField, LineSettingsDialog, OverlayResponse, PopupTarget},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkFormField {
    Name,
    Address,
    Hosts,
    Baud,
    Bits,
    Parity,
    StopBits,
    Flow,
}

impl LinkFormField {
    pub const ALL: [Self; 8] = [
        Self::Name,
        Self::Address,
        Self::Hosts,
        Self::Baud,
        Self::Bits,
        Self::Parity,
        Self::StopBits,
        Self::Flow,
    ];

    /// The line setting a field cycles through, for the ones that cycle
    fn cycles(self) -> Option<LineSettingField> {
        match self {
            Self::Bits => Some(LineSettingField::CharSize),
            Self::Parity => Some(LineSettingField::Parity),
            Self::StopBits => Some(LineSettingField::StopBits),
            Self::Flow => Some(LineSettingField::FlowControl),
            _ => None,
        }
    }

    pub fn is_line(self) -> bool {
        !matches!(self, Self::Name | Self::Address | Self::Hosts)
    }
}

/// The form a saved link is written in (MAX-185), new (blank from `n`, filled from the link
/// in use or a port from `s`) or edited: its name, its address, the
/// machines it's for (comma separated, none for every machine) and its line settings, which a
/// TCP address hasn't (skipped, and shown muted). Name, address, hosts and baud are typed, the
/// other line fields cycle with Left / Right or Space. Tab or Down goes to the next field,
/// Shift-Tab or Up to the one before; Enter saves, in place of `editing` for an edit; Esc
/// goes back to the directory
#[derive(Debug, Clone)]
pub struct LinkForm {
    pub field: LinkFormField,
    pub name: Input,
    pub address: Input,
    pub hosts: Input,
    pub baud: Input,
    pub line: LineSettingsDialog,
    pub editing: Option<SavedLink>,
}

impl LinkForm {
    /// A new link, filled from `link`, its missing line fields `[link]`'s
    pub fn new(link: SavedLink, defaults: &LinkDefaults) -> Self {
        Self::with(link, defaults, None)
    }

    /// `link` to edit, its missing line fields `[link]`'s
    pub fn edit(link: SavedLink, defaults: &LinkDefaults) -> Self {
        Self::with(link.clone(), defaults, Some(link))
    }

    fn with(link: SavedLink, defaults: &LinkDefaults, editing: Option<SavedLink>) -> Self {
        let line = link.over(defaults).line();
        // the dialog only cycles the values: its address isn't used
        let line_dialog = LineSettingsDialog::new(&LinkConfig {
            line,
            ..LinkConfig::default()
        });
        Self {
            field: LinkFormField::Name,
            name: Input::new(link.name),
            address: Input::new(link.address),
            hosts: Input::new(link.hosts.join(", ")),
            baud: Input::new(line.baud.to_string()),
            line: line_dialog,
            editing,
        }
    }

    /// No line settings for an address that has none (TCP, a replay)
    pub fn has_line(&self) -> bool {
        (self.address.value().trim().parse::<Address>())
            .map_or(true, |address| address.kind().has_line_settings())
    }

    /// The typed field's text, for the field that is one
    pub fn input(&self) -> Option<&Input> {
        match self.field {
            LinkFormField::Name => Some(&self.name),
            LinkFormField::Address => Some(&self.address),
            LinkFormField::Hosts => Some(&self.hosts),
            LinkFormField::Baud => Some(&self.baud),
            _ => None,
        }
    }

    fn input_mut(&mut self) -> Option<&mut Input> {
        match self.field {
            LinkFormField::Name => Some(&mut self.name),
            LinkFormField::Address => Some(&mut self.address),
            LinkFormField::Hosts => Some(&mut self.hosts),
            LinkFormField::Baud => Some(&mut self.baud),
            _ => None,
        }
    }

    /// The next field, or the one before, round; the line's skipped when there's none
    fn step(&mut self, forward: bool) {
        let fields = LinkFormField::ALL;
        let mut idx = fields.iter().position(|f| *f == self.field).unwrap_or(0);
        loop {
            idx = if forward {
                (idx + 1) % fields.len()
            } else {
                (idx + fields.len() - 1) % fields.len()
            };
            if self.has_line() || !fields[idx].is_line() {
                break;
            }
        }
        self.field = fields[idx];
    }

    fn cycle(&mut self, forward: bool) {
        if let Some(field) = self.field.cycles() {
            self.line.selected_field = field;
            if forward {
                self.line.cycle_next();
            } else {
                self.line.cycle_prev();
            }
        }
    }

    /// The link as typed, all its line settings written when it has some; the checks of the
    /// file come at the save, but the name's, said here as there's none to name the link by
    fn link(&self) -> Result<SavedLink, String> {
        if self.name.value().trim().is_empty() {
            return Err("the name is empty".to_string());
        }
        let hosts = (self.hosts.value().split(','))
            .map(str::trim)
            .filter(|host| !host.is_empty())
            .map(str::to_string)
            .collect();
        let link = SavedLink {
            name: self.name.value().trim().to_string(),
            hosts,
            address: self.address.value().trim().to_string(),
            ..SavedLink::default()
        };
        if !self.has_line() {
            return Ok(link);
        }
        let baud = self.baud.value().trim();
        let baud = (baud.parse::<u32>()).map_err(|_| format!("baud: '{baud}' is not a number"))?;
        Ok(link.with_line(&LineSettings {
            baud,
            ..self.line.line
        }))
    }

    pub fn handle_action(&mut self, action: DialogAction) -> OverlayResponse {
        match action {
            DialogAction::Close => return OverlayResponse::Run(KeyBindingAction::Directory),
            DialogAction::Confirm => {
                return match self.link() {
                    Ok(link) => OverlayResponse::SaveLink {
                        link,
                        replaces: self.editing.clone(),
                    },
                    Err(message) => OverlayResponse::HandledNotify {
                        message,
                        warn: true,
                    },
                };
            }
            DialogAction::CycleNext => self.step(true),
            DialogAction::CyclePrev => self.step(false),
            // a click picks a field, a click on the picked one cycles it
            DialogAction::Pick(PopupTarget::LinkFormField(field)) if field == self.field => {
                self.cycle(true);
            }
            DialogAction::Pick(PopupTarget::LinkFormField(field))
                if self.has_line() || !field.is_line() =>
            {
                self.field = field;
            }
            DialogAction::HandleKey(key) => match key.code {
                KeyCode::Up => self.step(false),
                KeyCode::Down => self.step(true),
                KeyCode::Left if self.field.cycles().is_some() => self.cycle(false),
                KeyCode::Right | KeyCode::Char(' ') if self.field.cycles().is_some() => {
                    self.cycle(true);
                }
                _ => {
                    if let Some(input) = self.input_mut() {
                        dispatch_input_key(input, key);
                    }
                }
            },
            DialogAction::Paste(text) => {
                if let Some(input) = self.input_mut() {
                    paste_into_input(input, &text);
                }
            }
            _ => {}
        }
        OverlayResponse::Handled
    }
}

#[cfg(test)]
#[path = "../tests/overlays/link_form.rs"]
mod tests;
