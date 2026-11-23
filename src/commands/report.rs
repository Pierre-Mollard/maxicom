//! What an action reports back (MAX-165): the notices it gives and why it failed, instead of
//! toasts shown from deep inside. The caller presents them: the user's toasts, a chain
//! stopped on its failure, later a client's reply
use crate::notification::ToastLevel;

/// A notice of an action, shown as a toast: in the box of `slot` when it has one, where it
/// takes the place of the last message (`notify_keyed`)
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "a note is for the user: show it or add it to the action's report"]
pub struct Note {
    pub level: ToastLevel,
    pub slot: Option<&'static str>,
    pub text: String,
}

impl Note {
    pub fn new(level: ToastLevel, text: impl Into<String>) -> Self {
        Self {
            level,
            slot: None,
            text: text.into(),
        }
    }

    pub fn info(text: impl Into<String>) -> Self {
        Self::new(ToastLevel::Info, text)
    }

    pub fn warn(text: impl Into<String>) -> Self {
        Self::new(ToastLevel::Warn, text)
    }

    /// The same note in the box of `slot`
    pub fn in_slot(self, slot: &'static str) -> Self {
        Self {
            slot: Some(slot),
            ..self
        }
    }
}

/// Why an action failed, its text ready to show: the rest of its chain doesn't run. `Said`
/// is shown as an error toast; `Shown` already has its popup (a refused reconfigure's dialog)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// error toast
    Said(String),
    /// error popup
    Shown(String),
}

/// What an action reports: its notes in the order they came, then its failure if it failed
#[derive(Debug, Default, PartialEq, Eq)]
#[must_use = "a report is for who started the action: present it or add it to its report"]
pub struct Report {
    pub notes: Vec<Note>,
    pub failure: Option<Failure>,
}

impl Report {
    /// A failure said as an error toast, no note before it
    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            notes: Vec::new(),
            failure: Some(Failure::Said(message.into())),
        }
    }

    /// A failure said as an error toast, after the notes so far
    pub fn fail(&mut self, message: impl Into<String>) {
        self.failure = Some(Failure::Said(message.into()));
    }

    /// What a later part of the same action reports, after this: its notes follow, the first
    /// failure stays
    pub fn extend(&mut self, other: Report) {
        self.notes.extend(other.notes);
        self.failure = self.failure.take().or(other.failure);
    }
}

impl From<Note> for Report {
    fn from(note: Note) -> Self {
        Self::from(vec![note])
    }
}

impl From<Vec<Note>> for Report {
    fn from(notes: Vec<Note>) -> Self {
        Self {
            notes,
            failure: None,
        }
    }
}
