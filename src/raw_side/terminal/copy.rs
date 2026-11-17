//! Copy mode in the dump: the cursor and the selection are absolute byte offsets, so a
//! resize changing the bytes per line, or the oldest bytes dropped, keep them on their bytes

use std::ops::Range;

use super::DumpBounds;
use crate::inputs::CopyModeAction;

/// What an action did
#[derive(Debug, PartialEq, Eq)]
pub enum DumpResult {
    Moved,
    /// The selected bytes; `as_received` for the bytes themselves, else in the view's radix
    Yank {
        bytes: Range<u64>,
        as_received: bool,
    },
    Exited,
    None,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DumpCopy {
    pub cursor: u64,
    pub anchor: Option<u64>,
    pub line_select: bool,
    /// A mouse press's selection: whether a drag followed it (the release yanks then)
    pressed: Option<bool>,
}

impl DumpCopy {
    pub fn at(offset: u64) -> Self {
        Self {
            cursor: offset,
            ..Self::default()
        }
    }

    /// The bytes selected, line-wise ones from their lines' start to their end
    pub fn selection(&self, bounds: &DumpBounds) -> Option<Range<u64>> {
        let anchor = self.anchor?;
        let (from, to) = (anchor.min(self.cursor), anchor.max(self.cursor));
        let (from, to) = (bounds.clamp(from), bounds.clamp(to));
        if self.line_select {
            Some(bounds.line_of(from).start..bounds.line_of(to).end)
        } else {
            Some(from..to + 1)
        }
    }

    pub fn press(&mut self, offset: u64, bounds: &DumpBounds) {
        *self = Self::at(bounds.clamp(offset));
        self.pressed = Some(false);
    }

    pub fn drag(&mut self, offset: u64, bounds: &DumpBounds) {
        if self.pressed == Some(false) {
            self.anchor = Some(self.cursor);
            self.pressed = Some(true);
        }
        self.cursor = bounds.clamp(offset);
    }

    /// A release after a drag yanks the bytes dragged over
    pub fn release(&mut self, bounds: &DumpBounds) -> DumpResult {
        match self.pressed.take() {
            Some(true) => self.yank(bounds, false),
            _ => DumpResult::None,
        }
    }

    pub fn apply(&mut self, action: CopyModeAction, bounds: &DumpBounds) -> DumpResult {
        let width = bounds.width();
        let page = width * bounds.rows.saturating_sub(1).max(1) as u64;
        let cursor = self.cursor;
        self.cursor = match action {
            CopyModeAction::MoveLeft => cursor.saturating_sub(1),
            CopyModeAction::MoveRight => cursor + 1,
            // a line up past the first byte kept stays: the line above is gone
            CopyModeAction::MoveUp => cursor
                .checked_sub(width)
                .filter(|&up| up >= bounds.start)
                .unwrap_or(cursor),
            CopyModeAction::MoveDown => Some(cursor + width)
                .filter(|&down| down < bounds.end)
                .unwrap_or(cursor),
            CopyModeAction::PageUp => cursor.saturating_sub(page),
            CopyModeAction::PageDown => cursor + page,
            CopyModeAction::StartOfLine => bounds.line_of(cursor).start,
            CopyModeAction::EndOfLine => bounds.line_of(cursor).end - 1,
            CopyModeAction::JumpToTop => bounds.start,
            CopyModeAction::JumpToBottom => bounds.end,
            CopyModeAction::Select | CopyModeAction::SelectLine => {
                let lines = action == CopyModeAction::SelectLine;
                if self.anchor.is_some() && self.line_select == lines {
                    self.anchor = None;
                    self.line_select = false;
                } else {
                    self.anchor.get_or_insert(cursor);
                    self.line_select = lines;
                }
                cursor
            }
            CopyModeAction::Yank => return self.yank(bounds, false),
            CopyModeAction::YankRaw => return self.yank(bounds, true),
            CopyModeAction::Exit => return DumpResult::Exited,
            // words and text objects have no bytes to go by
            _ => return DumpResult::None,
        };
        self.cursor = bounds.clamp(self.cursor);
        DumpResult::Moved
    }

    fn yank(&mut self, bounds: &DumpBounds, as_received: bool) -> DumpResult {
        match self.selection(bounds) {
            Some(bytes) if bounds.end > bounds.start => DumpResult::Yank { bytes, as_received },
            _ => DumpResult::Exited,
        }
    }
}

#[cfg(test)]
#[path = "../../tests/raw_side/terminal/copy.rs"]
mod tests;
