use crate::configs::RawSideView;

#[derive(Debug, Default, Clone, Copy)]
pub struct RawSideScroller {
    pub pan_x: usize,
    pub max_pan_x: usize,
}

impl RawSideScroller {
    pub fn pan_left(&mut self, bytes: usize, raw_view: RawSideView) {
        let stride = raw_view.stride();
        let pan_x = self.pan_x - self.pan_x % stride;
        self.pan_x = pan_x.saturating_sub(bytes * stride);
    }

    pub fn pan_right(&mut self, bytes: usize, raw_view: RawSideView) {
        let stride = raw_view.stride();
        let pan_x = self.pan_x - self.pan_x % stride;
        self.pan_x = (pan_x + bytes * stride).min(self.max_pan_x);
    }

    /// The widest row is `overflow` columns wider than the panel: the pan can go up to the
    /// byte holding that last column (rounded up to a byte start, so it shows a whole byte)
    pub fn set_limit(&mut self, overflow: usize, raw_view: RawSideView) {
        self.max_pan_x = overflow.next_multiple_of(raw_view.stride());
    }

    pub fn center_on_byte(
        &mut self,
        raw_view: RawSideView,
        usable_panel_width: usize,
        local_byte_offset: usize,
        total_sub_bytes: usize,
    ) {
        if raw_view == RawSideView::Off || usable_panel_width == 0 {
            return;
        }

        let stride = raw_view.stride();
        let total_raw_width = total_sub_bytes.saturating_mul(stride);
        self.set_limit(total_raw_width.saturating_sub(usable_panel_width), raw_view);

        let raw_match_x = local_byte_offset * stride;
        let half_width = usable_panel_width / 2;
        let pan_x = raw_match_x.saturating_sub(half_width);
        self.pan_x = (pan_x - pan_x % stride).min(self.max_pan_x);
    }

    pub fn sync_with_cursor(
        &mut self,
        raw_view: RawSideView,
        usable_panel_width: usize,
        local_byte_offset: usize,
        total_sub_bytes: usize,
    ) {
        if raw_view == RawSideView::Off || usable_panel_width == 0 {
            return;
        }

        let stride = raw_view.stride();
        let total_raw_width = total_sub_bytes.saturating_mul(stride);
        self.set_limit(total_raw_width.saturating_sub(usable_panel_width), raw_view);

        let raw_cursor_x = local_byte_offset * stride;
        let cursor_byte_end = raw_cursor_x + stride;

        if cursor_byte_end > self.pan_x + usable_panel_width {
            self.pan_x = cursor_byte_end
                .saturating_sub(usable_panel_width)
                .next_multiple_of(stride);
        } else if raw_cursor_x < self.pan_x {
            self.pan_x = raw_cursor_x;
        }

        self.pan_x = self.pan_x.min(self.max_pan_x);
    }
}

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
#[path = "../tests/app/raw_scroller.rs"]
mod tests;
