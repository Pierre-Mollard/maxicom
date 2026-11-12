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
    /// Its own number, so a click dismisses the toast drawn there even if others expired
    /// since (MAX-197)
    pub id: u64,
    /// The activity whose toasts follow each other in this one (MAX-228)
    pub key: Option<&'static str>,
    pub message: String,
    pub level: ToastLevel,
    pub expires_at: Instant,
}

#[derive(Default)]
pub struct NotificationManager {
    toasts: Vec<Toast>,
    next_id: u64,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, message: impl Into<String>, level: ToastLevel, duration: Duration) {
        self.push_toast(None, message.into(), level, duration);
    }

    /// A toast of one activity (`key`, as `transfer`): it takes the place of the activity's
    /// toast still shown, its text, level and time renewed, so the activity's messages follow
    /// each other in one box instead of piling up (MAX-228)
    pub fn push_keyed(
        &mut self,
        key: &'static str,
        message: impl Into<String>,
        level: ToastLevel,
        duration: Duration,
    ) {
        match self.toasts.iter_mut().find(|toast| toast.key == Some(key)) {
            // same place and id: a click on it still finds it
            Some(toast) => {
                toast.message = message.into();
                toast.level = level;
                toast.expires_at = Instant::now() + duration;
            }
            None => self.push_toast(Some(key), message.into(), level, duration),
        }
    }

    fn push_toast(
        &mut self,
        key: Option<&'static str>,
        message: String,
        level: ToastLevel,
        duration: Duration,
    ) {
        if self.toasts.len() >= 4 {
            self.toasts.remove(0);
        }
        self.next_id += 1;
        self.toasts.push(Toast {
            id: self.next_id,
            key,
            message,
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

    /// Drops the toast `id`, or the top one (the oldest) for none (MAX-197). True if one was
    /// dropped
    pub fn dismiss(&mut self, id: Option<u64>) -> bool {
        let index = match id {
            Some(id) => self.toasts.iter().position(|toast| toast.id == id),
            None => (!self.toasts.is_empty()).then_some(0),
        };
        index.map(|index| self.toasts.remove(index)).is_some()
    }

    /// Drops every toast (MAX-197). True if there was one
    pub fn dismiss_all(&mut self) -> bool {
        let any = !self.toasts.is_empty();
        self.toasts.clear();
        any
    }
}

#[cfg(test)]
#[path = "tests/notification.rs"]
mod tests;
