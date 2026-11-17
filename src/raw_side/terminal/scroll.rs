/// The terminal raw panel's dump scroll (MAX-252): its top line kept as that line's first byte,
/// so a resize changing the bytes per line keeps the place; none while it follows the live end.
/// `width` (bytes per line) and `rows` are the dump's as last drawn
#[derive(Debug, Default, Clone, Copy)]
pub struct DumpScroller {
    top: Option<u64>,
    pub width: usize,
    pub rows: usize,
}

impl DumpScroller {
    /// The first line of the bytes `start..end` and the top line of the live view
    pub fn bounds(&self, start: u64, end: u64) -> (u64, u64) {
        let width = self.width.max(1) as u64;
        let first = start / width;
        let last = if end > start {
            (end - 1) / width
        } else {
            first
        };
        let live = (last + 1).saturating_sub(self.rows as u64).max(first);
        (first, live)
    }

    /// The top line shown of the bytes `start..end` (line `n` holds bytes `n*width..`)
    pub fn top_line(&self, start: u64, end: u64) -> u64 {
        let (first, live) = self.bounds(start, end);
        let width = self.width.max(1) as u64;
        self.top
            .map_or(live, |top| (top / width).clamp(first, live))
    }

    pub fn scroll_up(&mut self, lines: usize, start: u64, end: u64) {
        let top = self.top_line(start, end).saturating_sub(lines as u64);
        self.scroll_to(top, start, end);
    }

    pub fn scroll_down(&mut self, lines: usize, start: u64, end: u64) {
        let top = self.top_line(start, end).saturating_add(lines as u64);
        self.scroll_to(top, start, end);
    }

    /// Reaching the live view follows it again
    pub fn scroll_to(&mut self, line: u64, start: u64, end: u64) {
        let (first, live) = self.bounds(start, end);
        let line = line.max(first);
        self.top = (line < live).then(|| line * self.width.max(1) as u64);
    }

    /// Scrolls the fewest lines that show `line`
    pub fn reveal(&mut self, line: u64, start: u64, end: u64) {
        let top = self.top_line(start, end);
        let rows = self.rows.max(1) as u64;
        if line < top {
            self.scroll_to(line, start, end);
        } else if line >= top + rows {
            self.scroll_to(line + 1 - rows, start, end);
        }
    }

    pub fn follow(&mut self) {
        self.top = None;
    }
}

#[cfg(test)]
#[path = "../../tests/raw_side/terminal/scroll.rs"]
mod tests;
