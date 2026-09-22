#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VisualPosition {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Default)]
pub struct CopyModeState {
    pub active: bool,
    pub cursor: VisualPosition,
    pub selection_anchor: Option<VisualPosition>,
}

impl CopyModeState {
    pub fn enter(&mut self, current_bottom_row: usize) {
        self.active = true;
        self.cursor = VisualPosition {
            row: current_bottom_row,
            col: 0,
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

    pub fn normalized_selection(&self) -> Option<(VisualPosition, VisualPosition)> {
        let anchor = self.selection_anchor?;
        if anchor.row < self.cursor.row
            || (anchor.row == self.cursor.row && anchor.col <= self.cursor.col)
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
        if row < start.row || row > end.row {
            return false;
        }
        if start.row == end.row {
            return col >= start.col && col <= end.col;
        }
        if row == start.row {
            return col >= start.col;
        }
        if row == end.row {
            return col <= end.col;
        }
        true
    }
}
