#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LogicalPosition {
    pub line_idx: usize,
    pub char_idx: usize,
}

#[derive(Debug, Clone, Default)]
pub struct CopyModeState {
    pub active: bool,
    pub cursor: LogicalPosition,
    pub selection_anchor: Option<LogicalPosition>,
}

impl CopyModeState {
    pub fn exit(&mut self) {
        self.active = false;
        self.selection_anchor = None;
    }

    pub fn toggle_selection(&mut self) {
        if self.selection_anchor.is_some() {
            self.selection_anchor = None;
        } else {
            self.selection_anchor = Some(self.cursor);
        }
    }

    pub fn enter_at(&mut self, line_idx: usize, char_idx: usize) {
        self.active = true;
        self.cursor = LogicalPosition { line_idx, char_idx };
        self.selection_anchor = None;
    }

    pub fn normalized_selection(&self) -> Option<(LogicalPosition, LogicalPosition)> {
        let anchor = self.selection_anchor?;
        if anchor.line_idx < self.cursor.line_idx
            || (anchor.line_idx == self.cursor.line_idx && anchor.char_idx <= self.cursor.char_idx)
        {
            Some((anchor, self.cursor))
        } else {
            Some((self.cursor, anchor))
        }
    }

    pub fn is_selected(&self, row: usize, col: usize) -> bool {
        let Some((start, end)) = self.normalized_selection() else {
            return false;
        };
        if row < start.line_idx || row > end.line_idx {
            return false;
        }
        if start.line_idx == end.line_idx {
            return col >= start.char_idx && col <= end.char_idx;
        }
        if row == start.line_idx {
            return col >= start.char_idx;
        }
        if row == end.line_idx {
            return col <= end.char_idx;
        }
        true
    }
}
