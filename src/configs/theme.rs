use std::{
    fs,
    path::{Path, PathBuf},
};

use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(into = "ThemeStyleDef", from = "ThemeStyleDef")]
pub struct ThemeStyle {
    pub style: Style,
}

impl ThemeStyle {
    pub const fn new(style: Style) -> Self {
        Self { style }
    }

    pub const fn from_fg(fg: Color) -> Self {
        Self {
            style: Style::new().fg(fg),
        }
    }

    pub const fn from_bg(bg: Color) -> Self {
        Self {
            style: Style::new().bg(bg),
        }
    }

    pub fn to_style(self) -> Style {
        self.style
    }
}

impl From<ThemeStyle> for Style {
    fn from(t: ThemeStyle) -> Self {
        t.style
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum ThemeStyleDef {
    Simple(Color),
    Detailed {
        fg: Option<Color>,
        bg: Option<Color>,
        #[serde(default)]
        bold: bool,
        #[serde(default)]
        dim: bool,
        #[serde(default)]
        italic: bool,
        #[serde(default)]
        underlined: bool,
        #[serde(default)]
        reversed: bool,
    },
}

impl From<ThemeStyleDef> for ThemeStyle {
    fn from(def: ThemeStyleDef) -> Self {
        match def {
            ThemeStyleDef::Simple(color) => ThemeStyle::from_fg(color),
            ThemeStyleDef::Detailed {
                fg,
                bg,
                bold,
                dim,
                italic,
                underlined,
                reversed,
            } => {
                let mut s = Style::default();
                if let Some(c) = fg {
                    s = s.fg(c);
                }
                if let Some(c) = bg {
                    s = s.bg(c);
                }
                if bold {
                    s = s.add_modifier(Modifier::BOLD);
                }
                if dim {
                    s = s.add_modifier(Modifier::DIM);
                }
                if italic {
                    s = s.add_modifier(Modifier::ITALIC);
                }
                if underlined {
                    s = s.add_modifier(Modifier::UNDERLINED);
                }
                if reversed {
                    s = s.add_modifier(Modifier::REVERSED);
                }
                ThemeStyle { style: s }
            }
        }
    }
}

impl From<ThemeStyle> for ThemeStyleDef {
    fn from(t: ThemeStyle) -> Self {
        let style = t.style;
        let mods = style.add_modifier;

        if style.bg.is_none()
            && mods.is_empty()
            && let Some(fg) = style.fg
        {
            return ThemeStyleDef::Simple(fg);
        }

        ThemeStyleDef::Detailed {
            fg: style.fg,
            bg: style.bg,
            bold: mods.contains(Modifier::BOLD),
            dim: mods.contains(Modifier::DIM),
            italic: mods.contains(Modifier::ITALIC),
            underlined: mods.contains(Modifier::UNDERLINED),
            reversed: mods.contains(Modifier::REVERSED),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub border: ThemeStyle,
    pub timestamp_line: ThemeStyle,
    pub timestamp: ThemeStyle,
    pub timestamp_bg: ThemeStyle,
    pub selection: ThemeStyle,
    pub status_bar_bg: ThemeStyle,
    pub line_prefix: ThemeStyle,
    pub line_partial_prefix: ThemeStyle,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            border: ThemeStyle::from_fg(Color::Cyan),
            timestamp_line: ThemeStyle::from_fg(Color::Yellow),
            timestamp: ThemeStyle::from_fg(Color::Gray),
            timestamp_bg: ThemeStyle::from_bg(Color::DarkGray),
            selection: ThemeStyle::new(
                Style::default()
                    .bg(Color::Rgb(0x28, 0x50, 0x96))
                    .fg(Color::White),
            ),
            status_bar_bg: ThemeStyle::from_bg(Color::Rgb(0x1e, 0x23, 0x2d)),
            line_prefix: ThemeStyle::from_fg(Color::DarkGray),
            line_partial_prefix: ThemeStyle::new(
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
