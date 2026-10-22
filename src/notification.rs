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
mod tests {
    //! Toast lifetime and the redraw signal returned by `tick`.

    use super::*;

    /// `tick` reports a change only when a toast actually expires.
    /// How: one expired and one long-lived toast, ticked twice.
    #[test]
    fn tick_reports_expired_toasts_once() {
        let mut manager = NotificationManager::new();
        manager.push("gone", ToastLevel::Info, Duration::ZERO);
        manager.push("stays", ToastLevel::Info, Duration::from_secs(60));
        assert!(manager.tick());
        assert_eq!(manager.active_toasts().len(), 1);
        assert!(!manager.tick());
    }

    /// No more than 4 toasts are kept, the oldest is dropped first.
    /// How: pushes 5 and checks the remaining messages.
    #[test]
    fn push_keeps_the_four_newest() {
        let mut manager = NotificationManager::new();
        for i in 0..5 {
            manager.push(format!("t{i}"), ToastLevel::Info, Duration::from_secs(60));
        }
        let messages: Vec<&str> = manager
            .active_toasts()
            .iter()
            .map(|t| t.message.as_str())
            .collect();
        assert_eq!(messages, ["t1", "t2", "t3", "t4"]);
    }
}
