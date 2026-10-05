use std::cell::Cell;

use crossterm::event::{KeyCode, KeyEvent};

use crate::overlays::OverlayResponse;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HelpCategory {
    #[default]
    Prefix,
    Direct,
    CopyMode,
    Search,
    LineMode,
    TerminalMode,
}

impl HelpCategory {
    pub const ALL: [Self; 6] = [
        Self::Prefix,
        Self::Direct,
        Self::CopyMode,
        Self::Search,
        Self::LineMode,
        Self::TerminalMode,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Prefix => "Prefixed",
            Self::Direct => "Directs",
            Self::CopyMode => "Copy",
            Self::Search => "Search",
            Self::LineMode => "Line",
            Self::TerminalMode => "Terminal",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Prefix => Self::Direct,
            Self::Direct => Self::CopyMode,
            Self::CopyMode => Self::Search,
            Self::Search => Self::LineMode,
            Self::LineMode => Self::TerminalMode,
            Self::TerminalMode => Self::Prefix,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Prefix => Self::TerminalMode,
            Self::Direct => Self::Prefix,
            Self::CopyMode => Self::Direct,
            Self::Search => Self::CopyMode,
            Self::LineMode => Self::Search,
            Self::TerminalMode => Self::LineMode,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HelpDialog {
    pub category: HelpCategory,
    pub scroll: usize,
    pub max_scroll: Cell<usize>,
}

impl HelpDialog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> OverlayResponse {
        let max = self.max_scroll.get();

        match key.code {
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                self.category = self.category.next();
                self.scroll = 0;
                self.max_scroll.set(0);
                OverlayResponse::Handled
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                self.category = self.category.prev();
                self.scroll = 0;
                self.max_scroll.set(0);
                OverlayResponse::Handled
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll = self.scroll.saturating_add(1).min(max);
                OverlayResponse::Handled
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll = self.scroll.saturating_sub(1);
                OverlayResponse::Handled
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(5).min(max);
                OverlayResponse::Handled
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(5);
                OverlayResponse::Handled
            }
            KeyCode::Home => {
                self.scroll = 0;
                OverlayResponse::Handled
            }
            KeyCode::End => {
                self.scroll = max;
                OverlayResponse::Handled
            }
            _ => OverlayResponse::Close,
        }
    }
}
