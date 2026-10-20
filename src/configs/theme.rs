use std::{
    fs,
    path::{Path, PathBuf},
};

use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};

use crate::configs::style::ConfigStyle;

// TODO: use color background on line mode too and verify
// TODO: make color distincts from config style
// TODO: make separations for categories

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub main_color: Color,
    pub secondary_color: Color,
    pub ternary_color: Color,
    pub text_color: Color,
    pub text_muted: Color,
    pub text_warn: Color,
    pub text_good: Color,
    pub unselected_color: Color,
    pub term_flash_fg: Color,
    pub term_flash_bg: Color,

    pub border_panel: ConfigStyle,
    pub border_panel_focused: ConfigStyle,
    pub border_app_unfocused: ConfigStyle,
    pub background: ConfigStyle,
    pub cursor_unfocused: ConfigStyle,
    pub timestamp_line: ConfigStyle,
    pub timestamp: ConfigStyle,
    pub timestamp_bg: ConfigStyle,
    pub selection: ConfigStyle,
    pub status_bar_bg: ConfigStyle,
    pub line_prefix: ConfigStyle,
    pub line_indent_prefix: ConfigStyle,
    pub line_partial_prefix: ConfigStyle,
    pub search_match: ConfigStyle,
    pub search_match_active: ConfigStyle,
    pub term_outside_bg: ConfigStyle,
    pub search_indicator: ConfigStyle,

    pub byte_any: Color,        // When raw
    pub byte_null: Color,       // 0x00 (padding, zero bytes)
    pub byte_ascii: Color,      // 0x20..=0x7E (printable ASCII characters)
    pub byte_whitespace: Color, // 0x0A, 0x0D, 0x09 (\n, \r, \t)
    pub byte_sync: Color,       // 0x55, 0xAA, 0x7E (serial preambles & sync flags)
    pub byte_ones: Color,       // 0xFF (broadcast, erased flash)
    pub byte_other: Color,      // Other arbitrary binary payloads
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            main_color: Color::Cyan,
            secondary_color: Color::Yellow,
            ternary_color: Color::Green,
            text_color: Color::White,
            text_muted: Color::DarkGray,
            text_warn: Color::Red,
            text_good: Color::Green,
            unselected_color: Color::White,
            border_panel: ConfigStyle::from_fg(Color::DarkGray),
            border_panel_focused: ConfigStyle::from_fg(Color::Cyan),
            border_app_unfocused: ConfigStyle::from_fg(Color::DarkGray),
            background: ConfigStyle::from_fg(Color::Black),
            cursor_unfocused: ConfigStyle::from_fg(Color::DarkGray),
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
            line_indent_prefix: ConfigStyle::from_fg(Color::DarkGray),
            line_partial_prefix: ConfigStyle::new(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            search_match: ConfigStyle::new(Style::default().bg(Color::Yellow).fg(Color::Black)),
            search_match_active: ConfigStyle::new(
                Style::default()
                    .bg(Color::Rgb(255, 140, 0))
                    .fg(Color::Black),
            ),
            search_indicator: ConfigStyle::new(
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            ),
            term_outside_bg: ConfigStyle::from_fg(Color::DarkGray),
            term_flash_fg: Color::DarkGray,
            term_flash_bg: Color::White,
            byte_any: Color::White,
            byte_null: Color::DarkGray,
            byte_ascii: Color::Cyan,
            byte_whitespace: Color::Yellow,
            byte_sync: Color::LightRed,
            byte_ones: Color::Red,
            byte_other: Color::LightBlue,
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

    #[inline]
    pub fn byte_color(&self, b: u8) -> Color {
        match b {
            0x00 => self.byte_null,
            0x0A | 0x0D | 0x09 => self.byte_whitespace,
            0xAA | 0x55 | 0x7E => self.byte_sync,
            0xFF => self.byte_ones,
            0x20..=0x7E => self.byte_ascii,
            _ => self.byte_other,
        }
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
