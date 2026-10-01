use chrono::format::StrftimeItems;
use ratatui::style::{Color, Modifier, Style};
/// Application configuration and files
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    fs,
    path::{Path, PathBuf},
};

use crate::{
    keybindings::{KeyBindingsConfig, KeymapFileError},
    theme::{ThemeConfig, ThemeError},
};

const APP_DIR: &str = "maxicom";
const CONFIG_FILE: &str = "config.toml";
const THEME_FILE: &str = "theme.toml";
const KEYMAP_FILE: &str = "keymap.toml";

pub const MIN_RX_BUF_SIZE: usize = 512;
pub const MAX_RX_BUF_SIZE: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineDisplayMode {
    #[default]
    Pretty,
    Clean,
    Raw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextEncoding {
    #[default]
    Utf8,
    Ascii,
    Cp437,  // IBM / DOS / Box drawing
    Latin1, // ISO-8859-1
}

impl TextEncoding {
    /// Decodes raw serial bytes into a valid UTF-8 Rust String
    /// (Invalid with U+FFFD for utf8 / ascii)
    pub fn decode(&self, bytes: &[u8]) -> String {
        match self {
            Self::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
            Self::Ascii => bytes
                .iter()
                .map(|&b| {
                    if (0x20..=0x7E).contains(&b) || b == b'\r' || b == b'\n' || b == b'\t' {
                        b as char
                    } else {
                        '\u{FFFD}'
                    }
                })
                .collect(),
            Self::Latin1 => {
                // ISO-8859-1 maps directly 1:1 to Unicode
                bytes.iter().map(|&b| b as char).collect()
            }
            Self::Cp437 => {
                // Maps 0x80..0xFF to Unicode box drawing & symbols
                decode_cp437(bytes)
            }
        }
    }

    pub fn to_utf8_bytes<'a>(self, bytes: &'a [u8]) -> Cow<'a, [u8]> {
        match self {
            Self::Utf8 | Self::Ascii => Cow::Borrowed(bytes),
            Self::Cp437 => {
                let utf8_string = decode_cp437(bytes);
                Cow::Owned(utf8_string.into_bytes())
            }
            Self::Latin1 => {
                let utf8_string: String = bytes.iter().map(|&b| b as char).collect();
                Cow::Owned(utf8_string.into_bytes())
            }
        }
    }
}

const CP437_HIGH_TABLE: [char; 128] = [
    // 0x80 - 0x8F
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å',
    // 0x90 - 0x9F
    'É', 'æ', 'Æ', 'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ',
    // 0xA0 - 0xAF
    'á', 'í', 'ó', 'ú', 'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»',
    // 0xB0 - 0xBF (Blocks & single borders)
    '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐',
    // 0xC0 - 0xCF (Corners & intersections)
    '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧',
    // 0xD0 - 0xDF (Double borders & solid blocks)
    '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐', '▀',
    // 0xE0 - 0xEF (Greek & math)
    'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩',
    // 0xF0 - 0xFF (Math & symbols)
    '≡', '±', '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{00A0}',
];

pub fn decode_cp437(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if b < 0x80 {
            out.push(b as char);
        } else {
            out.push(CP437_HIGH_TABLE[(b - 0x80) as usize]);
        }
    }
    out
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalDimensionMode {
    GrowOnly,
    FixedCols,
    MatchView,
    Fixed,
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
    pub display_mode: LineDisplayMode,
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
            display_mode: LineDisplayMode::default(),
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
    pub dimension_mode: TerminalDimensionMode,
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
            dimension_mode: TerminalDimensionMode::GrowOnly,
            history_size: 5000,
            cursor: CursorConfig::default(),
            empty_fill_char: String::from("░"),
            logging: TerminalLogConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusBarConfig {
    pub left: String,
    pub right: String,
    pub time_format: String,
}

impl Default for StatusBarConfig {
    fn default() -> Self {
        Self {
            left: String::from("{rec}[{mode}] {port} | {baud} | {framing} "),
            right: String::from(" RX: {rx_bytes} | TX: {tx_bytes} | {time} "),
            time_format: String::from("%H:%M:%S"),
        }
    }
}

impl StatusBarConfig {
    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        if self.time_format.is_empty() {
            return Err(ConfigValidationError::InvalidStatusBarTimeFormat {
                err: "format cannot be empty".to_owned(),
            });
        }

        if let Err(err) = StrftimeItems::new(&self.time_format).parse() {
            return Err(ConfigValidationError::InvalidStatusBarTimeFormat {
                err: err.to_string(),
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub show_borders: bool,
    pub show_status_bar: bool,
    pub status_bar: StatusBarConfig,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            show_borders: true,
            show_status_bar: true,
            status_bar: StatusBarConfig::default(),
        }
    }
}

impl UiConfig {
    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        self.status_bar.validate()
    }
}

/// MaxiCom UI & session settings
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub local_echo: bool,
    pub encoding: TextEncoding,
    pub input_mode: InputMode,
    pub rx_buffer_size: usize,
    pub display_live_timestamp: bool,
    pub timestamp_format: String,
    pub line_mode: LineModeConfig,
    pub terminal_mode: TerminalModeConfig,
    pub theme_path: Option<PathBuf>,
    pub keybindings_path: Option<PathBuf>,
    pub ui: UiConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            local_echo: false,
            encoding: TextEncoding::default(),
            rx_buffer_size: 4096,
            input_mode: InputMode::default(),
            display_live_timestamp: true,
            timestamp_format: String::from("%H:%M:%S%.3f"),
            line_mode: LineModeConfig::default(),
            terminal_mode: TerminalModeConfig::default(),
            theme_path: None,
            keybindings_path: None,
            ui: UiConfig::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub app: AppConfig,
    pub theme: ThemeConfig,
    pub keybindings: KeyBindingsConfig,
}

impl AppConfig {
    /// Loads configuration from path, creating default file if it doesn't exist
    pub fn load_or_create_default<P: AsRef<Path>>(path: P) -> Result<LoadedConfig, ConfigsError> {
        let path = path.as_ref();

        let app_config = if path.exists() {
            let content = fs::read_to_string(path).map_err(|e| ConfigsError::AppIo {
                path: path.to_path_buf(),
                source: e,
            })?;
            let config: AppConfig =
                toml::from_str(&content).map_err(|e| ConfigsError::ConfigParse {
                    path: path.to_path_buf(),
                    source: e,
                })?;
            config.validate().map_err(ConfigsError::Validation)?;
            config
        } else {
            let default_config = AppConfig::default();

            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| ConfigsError::AppIo {
                    path: parent.to_path_buf(),
                    source: e,
                })?;
            }

            let toml_string = toml::to_string_pretty(&default_config).unwrap();
            fs::write(path, toml_string).map_err(|e| ConfigsError::AppIo {
                path: path.to_path_buf(),
                source: e,
            })?;

            default_config
        };

        let theme_path = app_config.active_theme_path();
        let theme =
            ThemeConfig::load_or_create_default(&theme_path).map_err(ConfigsError::Theme)?;

        let keybindings_path = app_config.active_keybindings_path();
        let keybindings = KeyBindingsConfig::load_or_create_default(&keybindings_path)
            .map_err(ConfigsError::Keymap)?;

        Ok(LoadedConfig {
            app: app_config,
            theme,
            keybindings,
        })
    }

    /// Load config using system configuration (like XDG or .config) on any os with crate 'dirs'
    pub fn default_config_path() -> PathBuf {
        dirs::config_dir()
            .map(|p| p.join(APP_DIR).join(CONFIG_FILE))
            .unwrap_or_else(|| PathBuf::from(CONFIG_FILE))
    }

    pub fn default_theme_path() -> PathBuf {
        dirs::config_dir()
            .map(|p| p.join(APP_DIR).join(THEME_FILE))
            .unwrap_or_else(|| PathBuf::from(THEME_FILE))
    }

    pub fn default_keybindings_path() -> PathBuf {
        dirs::config_dir()
            .map(|p| p.join(APP_DIR).join(KEYMAP_FILE))
            .unwrap_or_else(|| PathBuf::from(KEYMAP_FILE))
    }

    pub fn active_theme_path(&self) -> PathBuf {
        self.theme_path
            .clone()
            .unwrap_or_else(Self::default_theme_path)
    }

    pub fn active_keybindings_path(&self) -> PathBuf {
        self.keybindings_path
            .clone()
            .unwrap_or_else(Self::default_keybindings_path)
    }

    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        if self.terminal_mode.rows == 0 || self.terminal_mode.cols == 0 {
            return Err(ConfigValidationError::ZeroDimensions);
        }

        if self.rx_buffer_size < MIN_RX_BUF_SIZE || self.rx_buffer_size > MAX_RX_BUF_SIZE {
            return Err(ConfigValidationError::InvalidBufferSize {
                actual: self.rx_buffer_size,
                min: MIN_RX_BUF_SIZE,
                max: MAX_RX_BUF_SIZE,
            });
        }

        if self.timestamp_format.is_empty() {
            return Err(ConfigValidationError::InvalidTimestampFormat {
                err: "cannot be empty".to_owned(),
            });
        }

        if let Err(err) = StrftimeItems::new(&self.timestamp_format).parse() {
            return Err(ConfigValidationError::InvalidTimestampFormat {
                err: err.to_string(),
            });
        };

        self.ui.validate()?;

        Ok(())
    }
}

#[derive(Debug)]
pub enum ConfigValidationError {
    ZeroDimensions,
    InvalidBufferSize {
        actual: usize,
        min: usize,
        max: usize,
    },
    InvalidTimestampFormat {
        err: String,
    },
    InvalidStatusBarTimeFormat {
        err: String,
    },
}

impl std::fmt::Display for ConfigValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroDimensions => write!(f, "terminal rows and cols must be greater than 0"),
            Self::InvalidBufferSize { actual, min, max } => {
                write!(
                    f,
                    "rx_buffer_size {actual} is out of bounds (allowed range: {min}..={max} bytes)"
                )
            }
            Self::InvalidTimestampFormat { err } => {
                write!(f, "invalid timestamp format: {err}")
            }
            Self::InvalidStatusBarTimeFormat { err } => {
                write!(f, "invalid status bar time_format: {err}")
            }
        }
    }
}

impl std::error::Error for ConfigValidationError {}

#[derive(Debug)]
pub enum ConfigsError {
    AppIo {
        path: PathBuf,
        source: std::io::Error,
    },
    ConfigParse {
        path: PathBuf,
        source: toml::de::Error,
    },
    Validation(ConfigValidationError),
    Theme(ThemeError),
    Keymap(KeymapFileError),
}

impl std::fmt::Display for ConfigsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AppIo { path, source } => {
                write!(f, "failed to read config at '{}': {source}", path.display())
            }
            Self::ConfigParse { path, source } => {
                write!(
                    f,
                    "invalid config syntax in '{}':\n{source}",
                    path.display()
                )
            }
            Self::Validation(err) => write!(f, "configuration validation error: {err}"),
            Self::Theme(err) => write!(f, "{err}"),
            Self::Keymap(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ConfigsError {}
