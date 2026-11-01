use std::cell::Cell;

use crossterm::event::KeyCode;

use crate::{
    configs::SettingsConfig,
    inputs::DialogAction,
    overlays::{OverlayResponse, PopupTarget, SettingKey, copy_config_path},
};

/// The settings popup, scrolled when the screen is too short for its rows (MAX-163); the
/// renderer sets how far it can go
#[derive(Debug, Clone, Default)]
pub struct SettingsDialog {
    pub scroll: usize,
    pub max_scroll: Cell<usize>,
}

impl SettingsDialog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle_action(
        &mut self,
        action: DialogAction,
        settings: &SettingsConfig,
    ) -> OverlayResponse {
        let max = self.max_scroll.get();
        let toggle = OverlayResponse::ToggleSetting;
        match action {
            DialogAction::Close => OverlayResponse::Close,
            DialogAction::Pick(PopupTarget::Setting(key)) => toggle(key),
            DialogAction::Pick(PopupTarget::ConfigPath(index)) => copy_config_path(settings, index),
            DialogAction::Down => self.scroll_to(self.scroll.saturating_add(1).min(max)),
            DialogAction::Up => self.scroll_to(self.scroll.saturating_sub(1)),
            DialogAction::PageDown => self.scroll_to(self.scroll.saturating_add(5).min(max)),
            DialogAction::PageUp => self.scroll_to(self.scroll.saturating_sub(5)),
            DialogAction::Home => self.scroll_to(0),
            DialogAction::End => self.scroll_to(max),
            DialogAction::HandleKey(key) => match key.code {
                KeyCode::Char('e' | 'E') => toggle(SettingKey::LocalEcho),
                KeyCode::Char('w' | 'W') => toggle(SettingKey::LineWrap),
                KeyCode::Char('m' | 'M') => toggle(SettingKey::Mode),
                KeyCode::Char('i' | 'I') => toggle(SettingKey::IdentWrap),
                KeyCode::Char('p' | 'P') => toggle(SettingKey::PrefixPlace),
                KeyCode::Char('d' | 'D') => toggle(SettingKey::DisplayMode),
                KeyCode::Char('b' | 'B') => toggle(SettingKey::DisplayRaw),
                KeyCode::Char('t' | 'T') => toggle(SettingKey::DisplayTimestamp),
                KeyCode::Char('x' | 'X') => toggle(SettingKey::TextEncoding),
                KeyCode::Char('u' | 'U') => toggle(SettingKey::Mouse),
                KeyCode::Char('c' | 'C') => copy_config_path(settings, 0),
                // the files out of the config folder, numbered after it (MAX-163)
                KeyCode::Char(digit @ '1'..='9') => {
                    copy_config_path(settings, digit as usize - '0' as usize)
                }
                KeyCode::Char('s' | 'S') => toggle(SettingKey::DimensionMode),
                KeyCode::Char('a' | 'A') => toggle(SettingKey::AnswerQueries),
                _ => OverlayResponse::Handled,
            },
            _ => OverlayResponse::Handled,
        }
    }

    fn scroll_to(&mut self, scroll: usize) -> OverlayResponse {
        self.scroll = scroll;
        OverlayResponse::Handled
    }
}
