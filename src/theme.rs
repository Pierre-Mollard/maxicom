use std::{fs, path::Path};

use ratatui::style::Color;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub border: Color,
    pub timestamp: Color,
    pub selection_bg: Color,
    pub status_bar_bg: Color,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            border: Color::Cyan,
            timestamp: Color::DarkGray,
            selection_bg: Color::Rgb(0x28, 0x50, 0x96),
            status_bar_bg: Color::Rgb(0x1e, 0x23, 0x2d),
        }
    }
}

// TODO: handle theme parsing errors like keybindings or config

impl ThemeConfig {
    pub fn load_or_create_default<P: AsRef<Path>>(
        path: P,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.as_ref();

        if path.exists() {
            let content = fs::read_to_string(path)?;
            let theme: ThemeConfig = toml::from_str(&content)?;
            return Ok(theme);
        }

        let default_theme = ThemeConfig::default();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let toml_string = toml::to_string_pretty(&default_theme)?;
        fs::write(path, toml_string)?;

        Ok(default_theme)
    }
}
