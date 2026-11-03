use ratatui::style::Color;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub enum ToastLevel {
    Info,
    Success,
    Warn,
    Error,
}

impl ToastLevel {
    pub fn color(&self) -> Color {
        match self {
            Self::Info => Color::Cyan,
            Self::Success => Color::Green,
            Self::Warn => Color::Yellow,
            Self::Error => Color::Red,
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Info => "ℹ ",
            Self::Success => "✓ ",
            Self::Warn => "⚠ ",
            Self::Error => "✗ ",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::Info => "Info",
            Self::Success => "Success",
            Self::Warn => "Warning",
            Self::Error => "Error",
        }
    }
}

pub struct Toast {
    pub message: String,
    pub level: ToastLevel,
    pub expires_at: Instant,
}

#[derive(Default)]
pub struct NotificationManager {
    toasts: Vec<Toast>,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self { toasts: Vec::new() }
    }

    pub fn push(&mut self, message: impl Into<String>, level: ToastLevel, duration: Duration) {
        if self.toasts.len() >= 4 {
            self.toasts.remove(0);
        }
        self.toasts.push(Toast {
            message: message.into(),
            level,
            expires_at: Instant::now() + duration,
        });
    }

    /// Drops expired toasts, returns true if any was removed (screen must be redrawn)
    pub fn tick(&mut self) -> bool {
        let now = Instant::now();
        let before_len = self.toasts.len();
        self.toasts.retain(|toast| toast.expires_at > now);
        self.toasts.len() != before_len
    }

    pub fn active_toasts(&self) -> &[Toast] {
        &self.toasts
    }
}

#[cfg(test)]
#[path = "tests/notification.rs"]
mod tests;
