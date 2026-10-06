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
