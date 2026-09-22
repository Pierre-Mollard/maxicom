#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CursorPosition {
    pub line_idx: usize,
    pub col_idx: usize,
}

#[derive(Debug, Clone, Default)]
pub struct CopyModeState {
    pub active: bool,
    pub cursor: CursorPosition,
    pub selection_anchor: Option<CursorPosition>,
}

impl CopyModeState {
    pub fn enter(&mut self, current_bottom_line: usize) {
        self.active = true;
        self.cursor = CursorPosition {
            line_idx: current_bottom_line,
            col_idx: 0,
        };
        self.selection_anchor = None;
    }

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

    pub fn current_selection(&self) -> Option<(CursorPosition, CursorPosition)> {
        let anchor = self.selection_anchor?;
        if anchor.line_idx < self.cursor.line_idx
            || (anchor.line_idx == self.cursor.line_idx && anchor.col_idx <= self.cursor.col_idx)
        {
            Some((anchor, self.cursor))
        } else {
            Some((self.cursor, anchor))
        }
    }
}
