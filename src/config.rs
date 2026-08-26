/// Application configuration and files
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EolMode {
    CrLf,
    Cr,
    Lf,
}

impl Default for EolMode {
    fn default() -> Self {
        Self::CrLf
    }
}

impl EolMode {
    pub fn as_bytes(&self) -> &'static [u8] {
        match self {
            EolMode::CrLf => b"\r\n",
            EolMode::Cr => b"\r",
            EolMode::Lf => b"\n",
        }
    }
}

/// MaxiCom UI & session settings
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub local_echo: bool,
    pub eol_mode: EolMode,
    pub max_history_lines: usize,
    pub timestamp_lines: bool,
    pub display_partial_rx: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            local_echo: false,
            eol_mode: EolMode::CrLf,
            max_history_lines: 1000,
            timestamp_lines: false,
            display_partial_rx: true,
        }
    }
}

impl AppConfig {
    //pub fn load_from_file() {}

    //pub fn create_default_config() {}

    /// Loads configuration from path, creating default file if it doesn't exist
    pub fn load_or_create_default<P: AsRef<Path>>(
        path: P,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.as_ref();

        // 1. If file exists, read and deserialize
        if path.exists() {
            let content = fs::read_to_string(path)?;
            let config: AppConfig = toml::from_str(&content)?;
            return Ok(config);
        }

        // 2. Otherwise, generate default config
        let default_config = AppConfig::default();

        // Ensure parent directory exists (e.g. ~/.config/maxicom/)
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // 3. Serialize and write to disk with clean TOML formatting
        let toml_string = toml::to_string_pretty(&default_config)?;
        fs::write(path, toml_string)?;

        Ok(default_config)
    }
}
