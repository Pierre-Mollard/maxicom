use std::cell::Cell;

use crossterm::event::KeyCode;
use tui_input::Input;

use crate::{
    configs::SettingsConfig,
    inputs::{DialogAction, dispatch_input_key, paste_into_input},
    overlays::{OverlayResponse, PopupTarget, copy_config_path},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HelpCategory {
    #[default]
    Prefix,
    Direct,
    Commands,
    CopyMode,
    Search,
    LineMode,
    TerminalMode,
    Dialog,
}

impl HelpCategory {
    pub const ALL: [Self; 8] = [
        Self::Prefix,
        Self::Direct,
        Self::Commands,
        Self::CopyMode,
        Self::Search,
        Self::LineMode,
        Self::TerminalMode,
        Self::Dialog,
    ];

    /// Short enough for the eight tabs to fit an 80-column screen (MAX-274)
    pub fn title(self) -> &'static str {
        match self {
            Self::Prefix => "Prefixed",
            Self::Direct => "Direct",
            Self::Commands => "Commands",
            Self::CopyMode => "Copy",
            Self::Search => "Search",
            Self::LineMode => "Line",
            Self::TerminalMode => "Term",
            Self::Dialog => "Dialog",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Prefix => Self::Direct,
            Self::Direct => Self::Commands,
            Self::Commands => Self::CopyMode,
            Self::CopyMode => Self::Search,
            Self::Search => Self::LineMode,
            Self::LineMode => Self::TerminalMode,
            Self::TerminalMode => Self::Dialog,
            Self::Dialog => Self::Prefix,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Prefix => Self::Dialog,
            Self::Direct => Self::Prefix,
            Self::Commands => Self::Direct,
            Self::CopyMode => Self::Commands,
            Self::Search => Self::CopyMode,
            Self::LineMode => Self::Search,
            Self::TerminalMode => Self::LineMode,
            Self::Dialog => Self::TerminalMode,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HelpDialog {
    pub category: HelpCategory,
    pub scroll: usize,
    pub max_scroll: Cell<usize>,
    /// The first tab shown when they don't all fit (MAX-12): kept while the selected one is in
    /// view, moved just enough to bring it back
    pub first_tab: Cell<usize>,
    /// The filter `/` opened (MAX-274): while it holds text, the help lists the keys and
    /// commands of every tab matching it; `typing` while its field takes the keys
    pub filter: Option<Input>,
    pub typing: bool,
}

impl HelpDialog {
    pub fn new() -> Self {
        Self::default()
    }

    /// The filter's text, when it holds some
    pub fn query(&self) -> Option<&str> {
        (self.filter.as_ref())
            .map(Input::value)
            .filter(|text| !text.is_empty())
    }

    pub fn handle_action(
        &mut self,
        action: DialogAction,
        settings: &SettingsConfig,
    ) -> OverlayResponse {
        if self.typing {
            return self.handle_typing(action);
        }
        let max = self.max_scroll.get();

        match action {
            // a filter goes first, then the help (MAX-274)
            DialogAction::Close if self.filter.is_some() => {
                self.clear_filter();
                OverlayResponse::Handled
            }
            DialogAction::Close => OverlayResponse::Close,
            DialogAction::Pick(PopupTarget::HelpFilter) => {
                self.typing = true;
                self.filter.get_or_insert_default();
                OverlayResponse::Handled
            }
            DialogAction::HandleKey(key) if key.code == KeyCode::Char('/') => {
                self.typing = true;
                self.filter.get_or_insert_default();
                OverlayResponse::Handled
            }
            DialogAction::Pick(PopupTarget::HelpTab(category)) => {
                // picking a tab leaves the filter
                self.clear_filter();
                self.category = category;
                self.scroll = 0;
                self.max_scroll.set(0);
                OverlayResponse::Handled
            }
            DialogAction::Right | DialogAction::CycleNext => {
                self.clear_filter();
                self.category = self.category.next();
                self.scroll = 0;
                self.max_scroll.set(0);
                OverlayResponse::Handled
            }
            DialogAction::Left | DialogAction::CyclePrev => {
                self.clear_filter();
                self.category = self.category.prev();
                self.scroll = 0;
                self.max_scroll.set(0);
                OverlayResponse::Handled
            }
            DialogAction::Down => {
                self.scroll = self.scroll.saturating_add(1).min(max);
                OverlayResponse::Handled
            }
            DialogAction::Up => {
                self.scroll = self.scroll.saturating_sub(1);
                OverlayResponse::Handled
            }
            DialogAction::PageDown => {
                self.scroll = self.scroll.saturating_add(5).min(max);
                OverlayResponse::Handled
            }
            DialogAction::PageUp => {
                self.scroll = self.scroll.saturating_sub(5);
                OverlayResponse::Handled
            }
            DialogAction::Home => {
                self.scroll = 0;
                OverlayResponse::Handled
            }
            DialogAction::End => {
                self.scroll = max;
                OverlayResponse::Handled
            }
            // the config folder, from any tab (MAX-163)
            DialogAction::Pick(PopupTarget::ConfigPath(index)) => copy_config_path(settings, index),
            DialogAction::HandleKey(key) if key.code == KeyCode::Char('c') => {
                copy_config_path(settings, 0)
            }
            _ => OverlayResponse::Handled,
        }
    }

    /// The filter's field takes the keys: Enter keeps the filter and gives the keys back to the
    /// list, Esc clears it; each edit lists the matches from their top
    fn handle_typing(&mut self, action: DialogAction) -> OverlayResponse {
        let input = self.filter.get_or_insert_default();
        match action {
            DialogAction::Confirm => {
                self.typing = false;
                if self.query().is_none() {
                    self.filter = None;
                }
            }
            DialogAction::Close => self.clear_filter(),
            DialogAction::HandleKey(key) => {
                dispatch_input_key(input, key);
                self.scroll = 0;
            }
            DialogAction::Paste(text) => {
                paste_into_input(input, &text);
                self.scroll = 0;
            }
            _ => {}
        }
        OverlayResponse::Handled
    }

    fn clear_filter(&mut self) {
        self.filter = None;
        self.typing = false;
        self.scroll = 0;
    }
}
