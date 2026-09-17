use ratatui::style::{Color, Modifier, Style};
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LineLogConfig {
    pub enabled: bool,
    pub path: PathBuf,
    pub timestamp: bool,
}

impl Default for LineLogConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path: PathBuf::from("maxicom_lines.log"),
            timestamp: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LineModeConfig {
    pub tx_eol: TxNewlineSymbol,
    pub rx_eol: RxNewlineMode,
    pub wrap_mode: WrapMode,
    pub max_history_lines: usize,
    pub display_partial_rx: bool,
    pub logging: LineLogConfig,
}

impl Default for LineModeConfig {
    fn default() -> Self {
        Self {
            tx_eol: TxNewlineSymbol::default(),
            rx_eol: RxNewlineMode::default(),
            wrap_mode: WrapMode::default(),
            max_history_lines: 1000,
            display_partial_rx: true,
            logging: LineLogConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalLogConfig {
    pub enabled: bool,
    pub path: PathBuf,
}

impl Default for TerminalLogConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path: PathBuf::from("maxicom_terminal.raw"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CursorConfig {
    pub unfocused_glyph: String,
    pub unfocused_style: Style,
    pub focused_style: Style,
}

impl Default for CursorConfig {
    fn default() -> Self {
        Self {
            unfocused_glyph: String::from("▯"),
            unfocused_style: Style::default().fg(Color::DarkGray),
            focused_style: Style::default().add_modifier(Modifier::REVERSED),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalModeConfig {
    pub rows: u16,
    pub cols: u16,
    pub history_size: usize,
    pub empty_fill_char: String,
    pub cursor: CursorConfig,
    pub logging: TerminalLogConfig,
}

impl Default for TerminalModeConfig {
    fn default() -> Self {
        Self {
            rows: 24,
            cols: 80,
            history_size: 5000,
            cursor: CursorConfig::default(),
            empty_fill_char: String::from("░"),
            logging: TerminalLogConfig::default(),
        }
    }
}

/// MaxiCom UI & session settings
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub local_echo: bool,
    pub input_mode: InputMode,
    pub display_live_timestamp: bool,
    pub line_mode: LineModeConfig,
    pub terminal_mode: TerminalModeConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            local_echo: false,
            input_mode: InputMode::default(),
            display_live_timestamp: true,
            line_mode: LineModeConfig::default(),
            terminal_mode: TerminalModeConfig::default(),
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
