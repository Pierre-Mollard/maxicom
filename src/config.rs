/// Application configuration and files
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const APP_DIR: &str = "maxicom";
const CONFIG_FILE: &str = "config.toml";

/// Char use at the end of the line when sending in LineMode (not raw)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TxNewlineSymbol {
    #[default]
    CrLf,
    Cr,
    Lf,
}

impl TxNewlineSymbol {
    pub fn as_bytes(&self) -> &'static [u8] {
        match self {
            TxNewlineSymbol::CrLf => b"\r\n",
            TxNewlineSymbol::Cr => b"\r",
            TxNewlineSymbol::Lf => b"\n",
        }
    }
}

/// In LineMode (not raw or interactive) used to define how to treat a new line
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RxNewlineMode {
    #[default]
    Auto, // Splits line on \n or \r\n, handles \r as line-reset (best for embedded)
    CrLf,   // Expects strictly \r\n
    LfOnly, // Treats \n as newline, ignores \r
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum InputMode {
    /// Line-buffered with editable prompt and history navigation (Split Rx and Tx)
    #[default]
    Line,
    /// Raw on 2D screen
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WrapMode {
    /// Soft-wraps at window edge without trimming whitespace (Default)
    #[default]
    Wrap,
    /// Truncates lines exceeding screen width (preserves table vertical alignment)
    Truncate,
}

/// MaxiCom UI & session settings
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub local_echo: bool,
    pub tx_eol_mode: TxNewlineSymbol,
    pub rx_eol_mode: RxNewlineMode,
    pub max_history_lines: usize,
    pub timestamp_in_file: bool,
    pub timestamp_in_line: bool,
    pub display_partial_rx: bool,
    pub input_mode: InputMode,
    pub wrap_mode: WrapMode,
    pub empty_fill_char: String,
    pub unfocused_cusor: String,
    pub terminal_history_size: usize,
    pub terminal_rows: u16,
    pub terminal_cols: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            local_echo: false,
            tx_eol_mode: TxNewlineSymbol::default(),
            rx_eol_mode: RxNewlineMode::default(),
            max_history_lines: 1000,
            timestamp_in_file: false,
            timestamp_in_line: true,
            display_partial_rx: true,
            input_mode: InputMode::default(),
            wrap_mode: WrapMode::default(),
            empty_fill_char: String::from("░"),
            unfocused_cusor: String::from("▯"),
            terminal_history_size: 5000,
            terminal_rows: 24,
            terminal_cols: 80,
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

/// All settings types
#[derive(Debug, PartialEq, Eq)]
pub enum SettingKey {
    LocalEcho,
    InputMode,
    LineWrap,
    DisplayTimestamp,
} // TODO: add all settings
