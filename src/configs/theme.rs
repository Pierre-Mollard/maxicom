use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::configs::style::ConfigStyle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub palette: PaletteColors,
    pub ui: UiStyles,
    pub line_mode: LineModeStyles,
    pub search: SearchStyles,
    pub terminal: TerminalStyles,
    pub raw_inspector: RawInspectorColors,
    pub popup: PopupStyles,
    pub notification: NotificationStyles,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PaletteColors {
    pub main: Color,
    pub secondary: Color,
    pub ternary: Color,
    pub text: Color,
    pub text_muted: Color,
    pub text_warn: Color,
    pub text_good: Color,
    pub unselected: Color,
}

impl Default for PaletteColors {
    fn default() -> Self {
        Self {
            main: Color::Cyan,
            secondary: Color::Yellow,
            ternary: Color::Green,
            text: Color::White,
            text_muted: Color::DarkGray,
            text_warn: Color::Red,
            text_good: Color::Green,
            unselected: Color::White,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PopupStyles {
    pub background: ConfigStyle,
    pub border: ConfigStyle,
    pub title: ConfigStyle,
}

impl Default for PopupStyles {
    fn default() -> Self {
        Self {
            background: ConfigStyle::new(Style::default().bg(Color::Reset).fg(Color::White)),
            border: ConfigStyle::from_fg(Color::Cyan),
            title: ConfigStyle::new(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationStyles {
    pub info: ConfigStyle,
    pub success: ConfigStyle,
    pub warn: ConfigStyle,
    pub error: ConfigStyle,
    pub border: ConfigStyle,
}

impl Default for NotificationStyles {
    fn default() -> Self {
        Self {
            info: ConfigStyle::new(Style::default().bg(Color::Reset).fg(Color::White)),
            success: ConfigStyle::new(Style::default().bg(Color::Reset).fg(Color::Green)),
            warn: ConfigStyle::new(Style::default().bg(Color::Reset).fg(Color::Yellow)),
            error: ConfigStyle::new(Style::default().bg(Color::Reset).fg(Color::Red)),
            border: ConfigStyle::from_fg(Color::DarkGray),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiStyles {
    pub background: ConfigStyle,
    pub border_panel: ConfigStyle,
    pub border_panel_focused: ConfigStyle,
    pub border_app_unfocused: ConfigStyle,
    pub cursor_unfocused: ConfigStyle,
    pub cursor_ghost: ConfigStyle,
    pub selection: ConfigStyle,
    pub status_bar: ConfigStyle,
    pub scrollbar_thumb: ConfigStyle,
    pub scrollbar_track: ConfigStyle,
}

impl Default for UiStyles {
    fn default() -> Self {
        Self {
            background: ConfigStyle::new(Style::default().bg(Color::Reset).fg(Color::Reset)),
            border_panel: ConfigStyle::from_fg(Color::DarkGray),
            border_panel_focused: ConfigStyle::from_fg(Color::Cyan),
            border_app_unfocused: ConfigStyle::from_fg(Color::DarkGray),
            cursor_unfocused: ConfigStyle::from_fg(Color::DarkGray),
            cursor_ghost: ConfigStyle::new(
                Style::default()
                    .bg(Color::Rgb(0x4a, 0x4a, 0x5a))
                    .fg(Color::White),
            ),
            selection: ConfigStyle::new(
                Style::default()
                    .bg(Color::Rgb(0x28, 0x50, 0x96))
                    .fg(Color::White),
            ),
            status_bar: ConfigStyle::new(
                Style::default()
                    .bg(Color::Rgb(0x1e, 0x23, 0x2d))
                    .fg(Color::White),
            ),
            scrollbar_thumb: ConfigStyle::from_fg(Color::DarkGray),
            scrollbar_track: ConfigStyle::from_fg(Color::Rgb(0x30, 0x30, 0x38)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LineModeStyles {
    pub prompt: ConfigStyle,
    pub history: ConfigStyle,
    pub line_prefix: ConfigStyle,
    pub line_indent_prefix: ConfigStyle,
    pub line_partial_prefix: ConfigStyle,
    pub timestamp: ConfigStyle,
}

impl Default for LineModeStyles {
    fn default() -> Self {
        Self {
            history: ConfigStyle::from_bg(Color::Reset),
            prompt: ConfigStyle::from_bg(Color::Reset),
            line_prefix: ConfigStyle::from_fg(Color::DarkGray),
            line_indent_prefix: ConfigStyle::from_fg(Color::DarkGray),
            line_partial_prefix: ConfigStyle::new(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            timestamp: ConfigStyle::from_fg(Color::Yellow),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchStyles {
    pub search_match: ConfigStyle,
    pub search_match_active: ConfigStyle,
    pub search_indicator: ConfigStyle,
}

impl Default for SearchStyles {
    fn default() -> Self {
        Self {
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
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalStyles {
    pub term_flash_fg_color: Color,
    pub term_flash_bg_color: Color,
    pub term_inside: ConfigStyle,
    pub term_outside: ConfigStyle,
    pub timestamp: ConfigStyle,
    pub timestamp_del: ConfigStyle,
    pub timestamp_del_empty: ConfigStyle,
}

impl Default for TerminalStyles {
    fn default() -> Self {
        Self {
            term_flash_fg_color: Color::DarkGray,
            term_flash_bg_color: Color::White,
            term_inside: ConfigStyle::from_fg(Color::Reset),
            term_outside: ConfigStyle::from_fg(Color::DarkGray),
            timestamp: ConfigStyle::from_fg(Color::Yellow),
            timestamp_del: ConfigStyle::from_fg(Color::Yellow),
            timestamp_del_empty: ConfigStyle::from_fg(Color::DarkGray),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RawInspectorColors {
    pub byte_any: Color,        // Default / text bytes
    pub byte_null: Color,       // 0x00 (padding, zero bytes)
    pub byte_ascii: Color,      // 0x20..=0x7E (printable ASCII)
    pub byte_whitespace: Color, // 0x0A, 0x0D, 0x09 (\n, \r, \t)
    pub byte_sync: Color,       // 0x55, 0xAA, 0x7E (sync preambles)
    pub byte_ones: Color,       // 0xFF (broadcast, erased flash)
    pub byte_other: Color,      // Other arbitrary payloads
}

impl Default for RawInspectorColors {
    fn default() -> Self {
        Self {
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
            0x00 => self.raw_inspector.byte_null,
            0x0A | 0x0D | 0x09 => self.raw_inspector.byte_whitespace,
            0xAA | 0x55 | 0x7E => self.raw_inspector.byte_sync,
            0xFF => self.raw_inspector.byte_ones,
            0x20..=0x7E => self.raw_inspector.byte_ascii,
            _ => self.raw_inspector.byte_other,
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
