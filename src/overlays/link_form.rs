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
mod tests {
    use super::*;
    use crate::link::serial::{FlowControl, Parity};
    use crossterm::event::KeyEvent;

    fn key(code: KeyCode) -> DialogAction {
        DialogAction::HandleKey(KeyEvent::from(code))
    }

    fn type_in(form: &mut LinkForm, text: &str) {
        for c in text.chars() {
            form.handle_action(key(KeyCode::Char(c)));
        }
    }

    fn saved(response: OverlayResponse) -> (SavedLink, Option<SavedLink>) {
        match response {
            OverlayResponse::SaveLink { link, replaces } => (link, replaces),
            _ => panic!("not saved"),
        }
    }

    /// MAX-185: a new link starts with `[link]`'s line; Tab and Down go through the fields, Up
    /// and Shift-Tab back, round; Left, Right and Space cycle a line value; Enter gives the
    /// link with all its line settings. A baud that isn't a number or an empty name says so.
    /// How: a link for one machine, each field typed or cycled, then Enter; then a bad baud,
    /// then no name.
    #[test]
    fn new_link() {
        let defaults = LinkDefaults::default();
        let link = SavedLink {
            hosts: vec!["laptop".into()],
            ..SavedLink::default()
        };
        let mut form = LinkForm::new(link, &defaults);
        assert_eq!(form.hosts.value(), "laptop");
        assert_eq!(form.baud.value(), "115200");
        type_in(&mut form, "esp32");
        form.handle_action(DialogAction::CycleNext);
        type_in(&mut form, "/dev/ttyUSB0");
        form.handle_action(key(KeyCode::Down));
        form.handle_action(key(KeyCode::Down));
        assert_eq!(form.field, LinkFormField::Baud);
        form.baud = Input::new("74880".into());
        form.handle_action(key(KeyCode::Down));
        form.handle_action(key(KeyCode::Left));
        assert_eq!(form.field, LinkFormField::Bits);
        form.handle_action(DialogAction::CycleNext);
        form.handle_action(key(KeyCode::Char(' ')));
        form.handle_action(key(KeyCode::Down));
        form.handle_action(key(KeyCode::Down));
        form.handle_action(key(KeyCode::Right));
        assert_eq!(form.field, LinkFormField::Flow);
        form.handle_action(key(KeyCode::Down));
        assert_eq!(form.field, LinkFormField::Name);
        form.handle_action(key(KeyCode::Up));
        assert_eq!(form.field, LinkFormField::Flow);

        let (link, replaces) = saved(form.handle_action(DialogAction::Confirm));
        assert!(replaces.is_none());
        let expected = SavedLink {
            name: "esp32".into(),
            hosts: vec!["laptop".into()],
            address: "/dev/ttyUSB0".into(),
            baud: Some(74880),
            bits: Some(7),
            parity: Some(Parity::Even),
            stop_bits: Some(1),
            flow: Some(FlowControl::RtsCts),
        };
        assert_eq!(link, expected);

        form.baud = Input::new("fast".into());
        assert!(matches!(
            form.handle_action(DialogAction::Confirm),
            OverlayResponse::HandledNotify { message, .. } if message == "baud: 'fast' is not a number"
        ));
        form.name = Input::new(" ".into());
        assert!(matches!(
            form.handle_action(DialogAction::Confirm),
            OverlayResponse::HandledNotify { message, .. } if message == "the name is empty"
        ));
    }

    /// MAX-185: an edit starts on the link as saved, its missing line fields `[link]`'s; a
    /// TCP address skips the line fields and saves none; the hosts split on commas, none for
    /// every machine. Enter gives the link in place of the one edited; Esc goes back to the
    /// directory.
    /// How: a TCP link without hosts edited to two hosts, Tab through it; then Esc.
    #[test]
    fn edit_link() {
        let defaults = LinkDefaults {
            baud: 9600,
            ..LinkDefaults::default()
        };
        let old = SavedLink {
            name: "router".into(),
            address: "tcp://10.0.0.1:4000".into(),
            ..SavedLink::default()
        };
        let mut form = LinkForm::edit(old.clone(), &defaults);
        assert_eq!(form.baud.value(), "9600");
        assert!(!form.has_line());
        form.handle_action(DialogAction::CycleNext);
        form.handle_action(DialogAction::CycleNext);
        assert_eq!(form.field, LinkFormField::Hosts);
        type_in(&mut form, "a, ,b");
        form.handle_action(DialogAction::CycleNext);
        assert_eq!(form.field, LinkFormField::Name);
        form.handle_action(DialogAction::Pick(PopupTarget::LinkFormField(
            LinkFormField::Parity,
        )));
        assert_eq!(form.field, LinkFormField::Name);

        let (link, replaces) = saved(form.handle_action(DialogAction::Confirm));
        assert_eq!(replaces, Some(old));
        assert_eq!(link.hosts, ["a", "b"]);
        assert_eq!(link.baud, None);
        assert!(matches!(
            form.handle_action(DialogAction::Close),
            OverlayResponse::Run(KeyBindingAction::Directory)
        ));
    }
}
