use chrono::{DateTime, Local};

// we no longer cache the timestamp string because there is less lines to render
// and it cost alloc and space when in the 10 of thousands of lines
pub struct LogEntry {
    pub text: String,
    pub timestamp: DateTime<Local>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewportSize {
    pub cols: usize,
    pub rows: usize,
}

impl ViewportSize {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols: cols as usize,
            rows: rows as usize,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTarget {
    Terminal,
    LineInput,
    LineHistory,
    Overlay,
}
