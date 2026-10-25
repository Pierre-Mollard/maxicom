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
mod tests {
    //! The raw panel pan stays on byte starts (MAX-71).

    use super::*;

    fn scroller(pan_x: usize, max_pan_x: usize) -> RawSideScroller {
        RawSideScroller { pan_x, max_pan_x }
    }

    /// The pan moves by whole bytes of the view's width, from a byte start.
    /// How: Binary (9 columns a byte) and Hex (3), from an aligned and an unaligned pan.
    #[test]
    fn pan_moves_by_bytes_of_the_view() {
        let mut s = scroller(0, 1000);
        s.pan_right(2, RawSideView::Binary);
        assert_eq!(s.pan_x, 18);
        s.pan_left(1, RawSideView::Binary);
        assert_eq!(s.pan_x, 9);
        s.pan_left(5, RawSideView::Binary);
        assert_eq!(s.pan_x, 0);

        // an unaligned pan (left by an older step) snaps back to its byte start first
        let mut s = scroller(13, 1000);
        s.pan_right(1, RawSideView::Hex);
        assert_eq!(s.pan_x, 15);
        let mut s = scroller(13, 1000);
        s.pan_left(1, RawSideView::Hex);
        assert_eq!(s.pan_x, 9);
    }

    /// The limit is rounded up to a byte start, and panning right stops on it.
    #[test]
    fn pan_limit_is_a_byte_start() {
        let mut s = scroller(0, 0);
        s.set_limit(10, RawSideView::Hex);
        assert_eq!(s.max_pan_x, 12);
        s.pan_right(100, RawSideView::Hex);
        assert_eq!(s.pan_x, 12);
        s.set_limit(10, RawSideView::Octal);
        assert_eq!(s.max_pan_x, 12);
        s.set_limit(10, RawSideView::Binary);
        assert_eq!(s.max_pan_x, 18);
    }

    /// Centering on a byte and following a cursor both land on byte starts, with the byte
    /// shown whole.
    /// How: Binary, a 40-column panel, a row of 30 bytes, byte 10 then byte 25.
    #[test]
    fn focus_lands_on_byte_starts() {
        let mut s = scroller(0, 0);
        s.center_on_byte(RawSideView::Binary, 40, 10, 30);
        assert!(s.pan_x.is_multiple_of(9));
        assert!(s.pan_x <= 90 && 99 <= s.pan_x + 40, "{}", s.pan_x);

        let mut s = scroller(0, 0);
        s.sync_with_cursor(RawSideView::Binary, 40, 25, 30);
        assert!(s.pan_x.is_multiple_of(9));
        assert!(s.pan_x <= 225 && 234 <= s.pan_x + 40, "{}", s.pan_x);
    }
}
