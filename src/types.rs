/// Module to reduce inter modules exchange or something
use chrono::{DateTime, Local};
use tui_input::Input;

pub struct LogEntry {
    pub text: String,
    pub timestamp: DateTime<Local>,
}

impl LogEntry {
    const FORMAT: &'static str = "%H:%M:%S%.3f";

    // TODO : make format config
    pub fn formatted_time(&self) -> String {
        format!("[{}]", self.timestamp.format(Self::FORMAT))
    }

    pub fn formatted_time_builder(timestamp: DateTime<Local>) -> String {
        format!("[{}]", timestamp.format(Self::FORMAT))
    }
}

#[derive(Debug, Clone)]
pub enum Overlay {
    Help,
    Settings,
    TerminalSize(ResizeDialog),
    DeviceDisconnected(String),
    Error(ErrorDialog),
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
pub enum ResizeField {
    Rows,
    Cols,
}

#[derive(Debug, Clone)]
pub struct ResizeDialog {
    pub rows_input: Input,
    pub cols_input: Input,
    pub active_field: ResizeField,
}

impl ResizeDialog {
    pub fn new(current_rows: u16, current_cols: u16) -> Self {
        Self {
            rows_input: Input::new(current_rows.to_string()),
            cols_input: Input::new(current_cols.to_string()),
            active_field: ResizeField::Rows,
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

#[derive(Debug, Clone)]
pub struct ErrorDialog {
    pub title: &'static str,
    pub summary: String,
    pub details: Option<String>,
}

impl ErrorDialog {
    pub fn new(title: &'static str, summary: impl Into<String>) -> Self {
        Self {
            title,
            summary: summary.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: impl ToString) -> Self {
        self.details = Some(details.to_string());
        self
    }
}
