/// What a screenshot saves of the terminal screen: its text, or its text with the escape
/// sequences that draw its colors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotFormat {
    Text,
    Ansi,
}

impl ScreenshotFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Ansi => "ansi",
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            Self::Text => "txt",
            Self::Ansi => "ans",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Text => Self::Ansi,
            Self::Ansi => Self::Text,
        }
    }
}

/// The terminal screen taken in both formats, so the one to save is chosen after (MAX-147)
#[derive(Debug, Clone, Default)]
pub struct Screenshot {
    pub text: String,
    pub ansi: String,
}

impl Screenshot {
    pub fn into_contents(self, format: ScreenshotFormat) -> String {
        match format {
            ScreenshotFormat::Text => self.text,
            ScreenshotFormat::Ansi => self.ansi,
        }
    }
}
