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

#[cfg(test)]
#[path = "../tests/app/raw_scroller.rs"]
mod tests;
