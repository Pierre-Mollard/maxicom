#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollStatus {
    pub max_scroll: usize,
    pub offset_from_bottom: usize,
}

impl ScrollStatus {
    pub fn is_bottom(&self) -> bool {
        self.offset_from_bottom == 0
    }

    pub fn is_top(&self) -> bool {
        self.max_scroll > 0 && self.offset_from_bottom >= self.max_scroll
    }

    pub fn can_scroll_up(&self) -> bool {
        self.max_scroll > 0 && self.offset_from_bottom < self.max_scroll
    }

    pub fn can_scroll_down(&self) -> bool {
        self.offset_from_bottom > 0
    }

    pub fn percent(&self) -> u8 {
        if self.max_scroll == 0 {
            return 100;
        }
        let pos_from_top = self.max_scroll.saturating_sub(self.offset_from_bottom);
        ((pos_from_top as f32 / self.max_scroll as f32) * 100.0).round() as u8
    }

    pub fn position_display(&self) -> String {
        if self.max_scroll == 0 {
            "All".to_string()
        } else if self.is_bottom() {
            "Bot".to_string()
        } else if self.is_top() {
            "Top".to_string()
        } else {
            format!("{}%", self.percent())
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ViewportScroller {
    /// Visual rows away from the live bottom (0 = following live tail)
    pub offset_y: usize,
    /// Horizontal pan offset in columns
    pub offset_x: usize,
    /// Maximum scrollable rows
    pub max_scroll_y: usize,
    /// Maximum horizontal pan
    pub max_pan_x: usize,
    /// Visible viewport height
    pub view_height: usize,
    /// Visible viewport width
    pub view_width: usize,
    pub new_content_missed: bool,
    pub pan_right_explored: usize,
}

impl ViewportScroller {
    pub fn on_rx_received(&mut self) {
        if self.offset_y > 0 {
            self.new_content_missed = true;
        }
    }

    pub fn update_view_size(&mut self, width: usize, height: usize) {
        self.view_width = width;
        self.view_height = height;
        self.clamp();
    }

    pub fn update_limits(&mut self, max_scroll_y: usize, max_pan_x: usize) {
        self.max_scroll_y = max_scroll_y;
        self.max_pan_x = max_pan_x;
        self.clamp();
    }

    pub fn clamp(&mut self) {
        self.offset_y = self.offset_y.min(self.max_scroll_y);
        self.offset_x = self.offset_x.min(self.max_pan_x);
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.offset_y = self.offset_y.saturating_add(lines).min(self.max_scroll_y);
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.offset_y = self.offset_y.saturating_sub(lines);
        if self.offset_y == 0 {
            self.new_content_missed = false;
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        self.offset_y = 0;
        self.new_content_missed = false;
    }

    pub fn scroll_to_top(&mut self) {
        self.offset_y = self.max_scroll_y;
    }

    pub fn scroll_to(&mut self, offset: usize) {
        self.offset_y = offset.min(self.max_scroll_y);
        if self.offset_y == 0 {
            self.new_content_missed = false;
        }
    }

    pub fn pan_left(&mut self, cols: usize) {
        self.offset_x = self.offset_x.saturating_sub(cols);
    }

    pub fn pan_right(&mut self, cols: usize) {
        self.offset_x = self.offset_x.saturating_add(cols).min(self.max_pan_x);
        self.pan_right_explored = self.pan_right_explored.max(self.offset_x);
    }

    pub fn can_pan_left(&self) -> Option<usize> {
        if self.offset_x > 0 {
            Some(self.offset_x)
        } else {
            None
        }
    }

    pub fn can_pan_right(&self) -> Option<(usize, usize)> {
        if self.max_pan_x > 0 {
            Some((
                self.max_pan_x.saturating_sub(self.offset_x),
                self.pan_right_explored,
            ))
        } else {
            None
        }
    }

    #[inline]
    pub fn status(&self) -> ScrollStatus {
        ScrollStatus {
            max_scroll: self.max_scroll_y,
            offset_from_bottom: self.offset_y,
        }
    }
}
