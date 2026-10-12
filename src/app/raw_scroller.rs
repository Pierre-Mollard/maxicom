use crate::configs::settings::RawSideView;

#[derive(Debug, Default, Clone, Copy)]
pub struct RawSideScroller {
    pub pan_x: usize,
    pub max_pan_x: usize,
}

impl RawSideScroller {
    pub fn pan_left(&mut self, amount: usize) {
        self.pan_x = self.pan_x.saturating_sub(amount);
    }

    pub fn pan_right(&mut self, amount: usize) {
        let limit = if self.max_pan_x > 0 {
            self.max_pan_x
        } else {
            2000
        };
        self.pan_x = (self.pan_x + amount).min(limit);
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
        self.max_pan_x = total_raw_width.saturating_sub(usable_panel_width);

        let raw_match_x = local_byte_offset * stride;
        let half_width = usable_panel_width / 2;
        self.pan_x = raw_match_x.saturating_sub(half_width).min(self.max_pan_x);
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
        self.max_pan_x = total_raw_width.saturating_sub(usable_panel_width);

        let raw_cursor_x = local_byte_offset * stride;
        let cursor_byte_end = raw_cursor_x + stride;

        if cursor_byte_end > self.pan_x + usable_panel_width {
            self.pan_x = cursor_byte_end.saturating_sub(usable_panel_width) + stride;
        } else if raw_cursor_x < self.pan_x {
            self.pan_x = raw_cursor_x;
        }

        self.pan_x = self.pan_x.min(self.max_pan_x);
    }
}
