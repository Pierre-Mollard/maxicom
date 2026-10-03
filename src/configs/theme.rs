use std::{
    fs,
    path::{Path, PathBuf},
};

use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};

use crate::configs::style::ConfigStyle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub border: ConfigStyle,
    pub timestamp_line: ConfigStyle,
    pub timestamp: ConfigStyle,
    pub timestamp_bg: ConfigStyle,
    pub selection: ConfigStyle,
    pub status_bar_bg: ConfigStyle,
    pub line_prefix: ConfigStyle,
    pub line_partial_prefix: ConfigStyle,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            border: ConfigStyle::from_fg(Color::Cyan),
            timestamp_line: ConfigStyle::from_fg(Color::Yellow),
            timestamp: ConfigStyle::from_fg(Color::Gray),
            timestamp_bg: ConfigStyle::from_fg(Color::DarkGray),
            selection: ConfigStyle::new(
                Style::default()
                    .bg(Color::Rgb(0x28, 0x50, 0x96))
                    .fg(Color::White),
            ),
            status_bar_bg: ConfigStyle::from_bg(Color::Rgb(0x1e, 0x23, 0x2d)),
            line_prefix: ConfigStyle::from_fg(Color::DarkGray),
            line_partial_prefix: ConfigStyle::new(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        }
    }
}

impl ThemeConfig {
    pub fn load_or_create_default<P: AsRef<Path>>(path: P) -> Result<Self, ThemeError> {
        let path = path.as_ref();

        if path.exists() {
            let content = fs::read_to_string(path).map_err(|err| ThemeError::Io {
                path: path.to_path_buf(),
                source: err,
            })?;

            let theme: ThemeConfig = toml::from_str(&content).map_err(|err| ThemeError::Parse {
                path: path.to_path_buf(),
                source: err,
            })?;

            return Ok(theme);
        }

        let default_theme = ThemeConfig::default();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| ThemeError::Io {
                path: parent.to_path_buf(),
                source: err,
            })?;
        }

        let toml_string = toml::to_string_pretty(&default_theme).map_err(|err| ThemeError::Io {
            path: path.to_path_buf(),
            source: std::io::Error::other(err.to_string()),
        })?;

        fs::write(path, toml_string).map_err(|err| ThemeError::Io {
            path: path.to_path_buf(),
            source: err,
        })?;

        Ok(default_theme)
    }
}

#[derive(Debug)]
pub enum ThemeError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    f,
                    "failed to read theme file '{}': {source}",
                    path.display()
                )
            }
            Self::Parse { path, source } => {
                write!(f, "invalid theme in '{}':\n{source}", path.display())
            }
        }
    }
}

impl std::error::Error for ThemeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
        }
    }
}
