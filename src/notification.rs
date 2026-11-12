use ratatui::style::Color;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    /// How much it matters to read it: a full stack drops the least first (MAX-228)
    fn rank(self) -> u8 {
        match self {
            Self::Info | Self::Success => 0,
            Self::Warn => 1,
            Self::Error => 2,
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
    /// How long it stays once shown
    pub duration: Duration,
    /// When it goes: none until its first frame, so one that doesn't fit yet waits instead of
    /// expiring unseen (MAX-228)
    pub expires_at: Option<Instant>,
    /// How many times it was said while shown: `×N` in its title (MAX-228)
    pub count: u32,
}

impl Toast {
    /// Its time starts over: now when it shows already, else at its first frame
    fn renew(&mut self, duration: Duration) {
        self.duration = duration;
        if self.expires_at.is_some() {
            self.expires_at = Some(Instant::now() + duration);
        }
    }
}

/// How many toasts are kept at once
const MAX_TOASTS: usize = 4;

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
        let message = message.into();
        // the same again while shown: its time renewed and counted, not a second box
        // (MAX-228)
        let same = |toast: &&mut Toast| {
            toast.key.is_none() && toast.level == level && toast.message == message
        };
        if let Some(toast) = self.toasts.iter_mut().find(same) {
            toast.count += 1;
            toast.renew(duration);
            return;
        }
        self.push_toast(None, message, level, duration);
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
                let message = message.into();
                // the same message again counts, a new one starts over
                toast.count = if toast.message == message {
                    toast.count + 1
                } else {
                    1
                };
                toast.message = message;
                toast.level = level;
                toast.renew(duration);
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
        // full: the oldest of the least important goes, so a burst of infos can't push out
        // a warning or an error not read yet (MAX-228)
        if self.toasts.len() >= MAX_TOASTS
            && let Some(least) = self.toasts.iter().map(|toast| toast.level.rank()).min()
            && let Some(index) = self.toasts.iter().position(|t| t.level.rank() == least)
        {
            self.toasts.remove(index);
        }
        self.next_id += 1;
        self.toasts.push(Toast {
            id: self.next_id,
            key,
            message,
            level,
            duration,
            expires_at: None,
            count: 1,
        });
    }

    /// The toasts `ids` were drawn at `now`: those drawn for the first time start their time
    pub fn shown(&mut self, ids: impl IntoIterator<Item = u64>, now: Instant) {
        for id in ids {
            if let Some(toast) = self.toasts.iter_mut().find(|toast| toast.id == id) {
                toast.expires_at.get_or_insert(now + toast.duration);
            }
        }
    }

    /// Drops expired toasts, returns true if any was removed (screen must be redrawn). One
    /// never drawn doesn't expire
    pub fn tick(&mut self) -> bool {
        let now = Instant::now();
        let before_len = self.toasts.len();
        (self.toasts).retain(|toast| toast.expires_at.is_none_or(|at| at > now));
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
