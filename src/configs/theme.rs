use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

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
    pub session_browser: SessionBrowserColors,
    pub command_line: CommandLineStyles,
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
    pub links: [Color; 8],
}

impl Default for PaletteColors {
    fn default() -> Self {
        Self {
            main: Color::Cyan,
            secondary: Color::Yellow,
            ternary: Color::Green,
            text: Color::White,
            text_muted: Color::Rgb(0x8c, 0x92, 0x9e),
            text_warn: Color::Red,
            text_good: Color::Green,
            unselected: Color::White,
            links: [75, 141, 211, 117, 183, 110, 176, 152].map(Color::Indexed),
        }
    }
}

impl PaletteColors {
    /// A link's color, one of `links` by an FNV-1a hash of its tag (`ttyUSB0`), the same
    /// from one launch to the next (std's hasher doesn't promise that): in the session
    /// browser and the status bar's `link`
    pub fn link_color(&self, tag: &str) -> Color {
        let hash = tag.bytes().fold(0x811c_9dc5_u32, |hash, b| {
            (hash ^ u32::from(b)).wrapping_mul(0x0100_0193)
        });
        self.links[hash as usize % self.links.len()]
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

/// The command line's own colors (MAX-12), apart from the line-mode input that goes to the
/// device
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommandLineStyles {
    pub border: ConfigStyle,
    pub prompt: ConfigStyle,
    pub text: ConfigStyle,
    pub selected: ConfigStyle,
}

impl Default for CommandLineStyles {
    fn default() -> Self {
        Self {
            border: ConfigStyle::from_fg(Color::LightGreen),
            prompt: ConfigStyle::new(
                Style::default()
                    .fg(Color::LightGreen)
                    .add_modifier(Modifier::BOLD),
            ),
            text: ConfigStyle::from_fg(Color::White),
            selected: ConfigStyle::new(Style::default().bg(Color::LightGreen).fg(Color::Black)),
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
    pub selection_raw: ConfigStyle,
    pub hover: ConfigStyle,
    pub status_bar: ConfigStyle,
    pub scrollbar_thumb: ConfigStyle,
    pub scrollbar_track: ConfigStyle,
    /// The screenshot's flash, and a jumped-to mark's line (MAX-164)
    pub flash: ConfigStyle,
    /// A mark's letter in the history's left column, and on the scrollbar (MAX-164)
    pub mark: ConfigStyle,
    pub scrollbar_mark: ConfigStyle,
}

impl UiStyles {
    /// The copy mode selection's style: `selection_raw` while the copy cursor is in the raw
    /// side panel, as the status bar's `COPY` badge turns red (MAX-193)
    pub fn selection_style(&self, raw: bool) -> Style {
        if raw {
            *self.selection_raw
        } else {
            *self.selection
        }
    }
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
            selection: ConfigStyle::new(Style::default().bg(Color::Magenta).fg(Color::Black)),
            selection_raw: ConfigStyle::new(Style::default().bg(Color::Red).fg(Color::Black)),
            hover: ConfigStyle::new(Style::new().add_modifier(Modifier::REVERSED)),
            status_bar: ConfigStyle::new(
                Style::default()
                    .bg(Color::Rgb(0x1e, 0x23, 0x2d))
                    .fg(Color::White),
            ),
            scrollbar_thumb: ConfigStyle::from_fg(Color::DarkGray),
            scrollbar_track: ConfigStyle::from_fg(Color::Rgb(0x30, 0x30, 0x38)),
            flash: ConfigStyle::new(Style::default().bg(Color::White).fg(Color::DarkGray)),
            mark: ConfigStyle::new(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            scrollbar_mark: ConfigStyle::from_fg(Color::Yellow),
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
            line_partial_prefix: ConfigStyle::from_fg(Color::DarkGray),
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
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalStyles {
    pub term_inside: ConfigStyle,
    pub term_outside: ConfigStyle,
    pub timestamp: ConfigStyle,
    pub timestamp_del: ConfigStyle,
    pub timestamp_del_empty: ConfigStyle,
}

impl Default for TerminalStyles {
    fn default() -> Self {
        Self {
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

/// The session browser's colors (MAX-153), as `lsd -l` draws a listing: the date fades from
/// green to a grayish green with the age of the last write (1 hour, 1 day, 1 week, 30 days),
/// the size warms from pale yellow to red (empty, 100 KiB, 1 MiB, 100 MiB, 1 GiB); each port
/// takes its link color from the palette's `links`. The defaults are 256-color indexed, lsd's
/// own where it has one
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionBrowserColors {
    pub date_hour: Color,
    pub date_day: Color,
    pub date_week: Color,
    pub date_month: Color,
    pub date_older: Color,
    pub size_empty: Color,
    pub size_small: Color,
    pub size_medium: Color,
    pub size_large: Color,
    pub size_huge: Color,
    pub size_giant: Color,
    pub mark_running: Color,
    pub mark_restored: Color,
    pub mark_no_meta: Color,
}

impl Default for SessionBrowserColors {
    fn default() -> Self {
        Self {
            date_hour: Color::Indexed(40),
            date_day: Color::Indexed(42),
            date_week: Color::Indexed(36),
            date_month: Color::Indexed(29),
            date_older: Color::Indexed(65),
            size_empty: Color::Indexed(245),
            size_small: Color::Indexed(229),
            size_medium: Color::Indexed(222),
            size_large: Color::Indexed(216),
            size_huge: Color::Indexed(172),
            size_giant: Color::Indexed(160),
            mark_running: Color::Indexed(40),
            mark_restored: Color::Cyan,
            mark_no_meta: Color::Indexed(245),
        }
    }
}

impl SessionBrowserColors {
    /// The date's color for a last write `age` ago
    pub fn date_color(&self, age: Duration) -> Color {
        const HOUR: u64 = 60 * 60;
        const DAY: u64 = 24 * HOUR;
        match age.as_secs() {
            s if s < HOUR => self.date_hour,
            s if s < DAY => self.date_day,
            s if s < 7 * DAY => self.date_week,
            s if s < 30 * DAY => self.date_month,
            _ => self.date_older,
        }
    }

    pub fn size_color(&self, bytes: u64) -> Color {
        const KIB: u64 = 1024;
        const MIB: u64 = 1024 * KIB;
        match bytes {
            0 => self.size_empty,
            b if b < 100 * KIB => self.size_small,
            b if b < MIB => self.size_medium,
            b if b < 100 * MIB => self.size_large,
            b if b < 1024 * MIB => self.size_huge,
            _ => self.size_giant,
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

#[cfg(test)]
#[path = "../tests/configs/theme.rs"]
mod tests;
