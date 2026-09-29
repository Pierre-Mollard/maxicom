use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub border: String,
    pub timestamp: String,
    pub selection_bg: String,
    pub status_bar_bg: String,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            border: String::from("Cyan"),
            timestamp: String::from("DarkGray"),
            selection_bg: String::from("#285096"),
            status_bar_bg: String::from("#1e232d"),
        }
    }
}

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
