use chrono::{DateTime, Local};

// PERF: use CompactStr for text and formatted_timestamp
pub struct LogEntry {
    pub text: String,
    pub timestamp: DateTime<Local>,
    pub formatted_timestamp: String,
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
