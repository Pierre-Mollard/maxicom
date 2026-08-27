/// Application configuration and files
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const APP_DIR: &str = "maxicom";
const CONFIG_FILE: &str = "config.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EolMode {
    #[default]
    CrLf,
    Cr,
    Lf,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum InputMode {
    /// Line-buffered with editable prompt and history navigation
    #[default]
    Line,
    /// Direct byte-by-byte passthrough (like classic Minicom/Picocom)
    Raw,
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
    pub input_mode: InputMode,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            local_echo: false,
            eol_mode: EolMode::default(),
            max_history_lines: 1000,
            timestamp_lines: false,
            display_partial_rx: true,
            input_mode: InputMode::default(),
        }
    }
}

impl AppConfig {
    /// Loads configuration from path, creating default file if it doesn't exist
    pub fn load_or_create_default<P: AsRef<Path>>(
        path: P,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.as_ref();

        if path.exists() {
            let content = fs::read_to_string(path)?;
            let config: AppConfig = toml::from_str(&content)?;
            return Ok(config);
        }

        let default_config = AppConfig::default();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let toml_string = toml::to_string_pretty(&default_config)?;
        fs::write(path, toml_string)?;

        Ok(default_config)
    }

    /// Load config using system configuration (like XDG or .config) on any os with crate 'dirs'
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .map(|p| p.join(APP_DIR).join(CONFIG_FILE))
            .unwrap_or_else(|| PathBuf::from(CONFIG_FILE))
    }
}
