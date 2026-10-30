use std::cell::Cell;

use crate::{
    inputs::DialogAction,
    overlays::{OverlayResponse, PopupTarget},
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

    pub fn title(self) -> &'static str {
        match self {
            Self::Prefix => "Prefixed",
            Self::Direct => "Directs",
            Self::Commands => "Commands",
            Self::CopyMode => "Copy",
            Self::Search => "Search",
            Self::LineMode => "Line",
            Self::TerminalMode => "Terminal",
            Self::Dialog => "Dialogs",
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
}

impl HelpDialog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle_action(&mut self, action: DialogAction) -> OverlayResponse {
        let max = self.max_scroll.get();

        match action {
            DialogAction::Close => OverlayResponse::Close,
            DialogAction::Pick(PopupTarget::HelpTab(category)) => {
                self.category = category;
                self.scroll = 0;
                self.max_scroll.set(0);
                OverlayResponse::Handled
            }
            DialogAction::Right | DialogAction::CycleNext => {
                self.category = self.category.next();
                self.scroll = 0;
                self.max_scroll.set(0);
                OverlayResponse::Handled
            }
            DialogAction::Left | DialogAction::CyclePrev => {
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
            _ => OverlayResponse::Handled,
        }
    }
}
