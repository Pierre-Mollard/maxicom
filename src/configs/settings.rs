use std::{
    borrow::Cow,
    fmt::Display,
    fs,
    path::{Path, PathBuf},
};

use chrono::format::strftime::StrftimeItems;
use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};

pub const MIN_RX_BUF_SIZE: usize = 512;
pub const MAX_RX_BUF_SIZE: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineDisplayMode {
    #[default]
    Pretty,
    Clean,
    Raw,
    Hex,
    #[serde(alias = "oct")]
    Octal,
    #[serde(alias = "bin", alias = "bits", alias = "byte", alias = "bytes")]
    Binary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextEncoding {
    #[default]
    Utf8,
    Ascii,
    Cp437,
    Latin1,
}

impl TextEncoding {
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
            Self::Latin1 => bytes.iter().map(|&b| b as char).collect(),
            Self::Cp437 => decode_cp437(bytes),
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

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF8",
            Self::Ascii => "ASCII",
            Self::Cp437 => "CP437",
            Self::Latin1 => "LATIN1",
        }
    }

    pub fn toggle(&mut self) {
        *self = match *self {
            Self::Utf8 => Self::Ascii,
            Self::Ascii => Self::Cp437,
            Self::Cp437 => Self::Latin1,
            Self::Latin1 => Self::Utf8,
        };
    }
}

impl Display for TextEncoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

const CP437_HIGH_TABLE: [char; 128] = [
    'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ', 'Æ',
    'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', 'á', 'í', 'ó', 'ú', 'ñ', 'Ñ',
    'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', '░', '▒', '▓', '│', '┤', '╡', '╢', '╖', '╕',
    '╣', '║', '╗', '╝', '╜', '╛', '┐', '└', '┴', '┬', '├', '─', '┼', '╞', '╟', '╚', '╔', '╩', '╦',
    '╠', '═', '╬', '╧', '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘', '┌', '█', '▄', '▌', '▐',
    '▀', 'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ', '∞', 'φ', 'ε', '∩', '≡', '±',
    '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²', '■', '\u{00A0}',
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RxNewlineMode {
    #[default]
    Auto,
    CrLf,
    LfOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PrefixWrapBehavior {
    None,
    #[default]
    Prefix,
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
    #[default]
    Line,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WrapMode {
    #[default]
    Word,
    Char,
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
    pub wrap_indent: PrefixWrapBehavior,
    pub max_history_lines: usize,
    pub display_partial_rx: bool,
    pub line_prefix: String,
    pub line_indent_prefix: String,
    pub line_partial_prefix: String,
    pub prompt_prefix: String,
    pub prompt_vi_prefix: String,
    pub logging: LineLogConfig,
}

impl Default for LineModeConfig {
    fn default() -> Self {
        Self {
            tx_eol: TxNewlineSymbol::default(),
            rx_eol: RxNewlineMode::default(),
            display_mode: LineDisplayMode::default(),
            wrap_mode: WrapMode::default(),
            wrap_indent: PrefixWrapBehavior::default(),
            max_history_lines: 1000,
            display_partial_rx: true,
            line_prefix: String::from("│ "),
            line_indent_prefix: String::from("│ "),
            line_partial_prefix: String::from("│…"),
            prompt_prefix: String::from("> "),
            prompt_vi_prefix: String::from("< "),
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
    pub center: String,
    pub right: String,
    pub time_format: String,
}

impl Default for StatusBarConfig {
    fn default() -> Self {
        Self {
            left: String::from(
                "[󰒓 ](prefix?yellow:hide)[ {mode} ](prefix?yellow:copy?magenta:search?blue:cyan bold) {port} | [{baud}](yellow) {framing}",
            ),
            center: String::default(),
            right: String::from(
                "[[ REC]](rec?red bold:hide) {scroll_pos_up} [RX: {rx_bytes}](gray) | [TX: {tx_bytes}](gray) | [{time}](blue)",
            ),
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
    pub show_scrollbar: bool,
    pub show_status_bar: bool,
    pub status_bar: StatusBarConfig,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            show_borders: true,
            show_scrollbar: true,
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
pub struct SettingsConfig {
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
    pub hightlights_path: Option<PathBuf>,
    pub ui: UiConfig,
}

impl Default for SettingsConfig {
    fn default() -> Self {
        Self {
            local_echo: false,
            encoding: TextEncoding::default(),
            rx_buffer_size: 4096,
            input_mode: InputMode::default(),
            display_live_timestamp: false,
            timestamp_format: String::from("[%H:%M:%S%.3f]"),
            line_mode: LineModeConfig::default(),
            terminal_mode: TerminalModeConfig::default(),
            theme_path: None,
            keybindings_path: None,
            hightlights_path: None,
            ui: UiConfig::default(),
        }
    }
}

impl SettingsConfig {
    /// Loads configuration from path, creating default file if it doesn't exist
    pub fn load_or_create_default<P: AsRef<Path>>(path: P) -> Result<Self, SettingsError> {
        let path = path.as_ref();

        let app_config = if path.exists() {
            let content = fs::read_to_string(path).map_err(|e| SettingsError::Io {
                path: path.to_path_buf(),
                source: e,
            })?;
            let config: SettingsConfig =
                toml::from_str(&content).map_err(|e| SettingsError::Parse {
                    path: path.to_path_buf(),
                    source: e,
                })?;
            config.validate().map_err(|err| SettingsError::Validation {
                path: path.to_path_buf(),
                source: err,
            })?;
            config
        } else {
            let default_config = SettingsConfig::default();

            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| SettingsError::Io {
                    path: parent.to_path_buf(),
                    source: e,
                })?;
            }

            let toml_string =
                toml::to_string_pretty(&default_config).map_err(|e| SettingsError::Io {
                    path: path.to_path_buf(),
                    source: std::io::Error::other(e.to_string()),
                })?;

            fs::write(path, toml_string).map_err(|e| SettingsError::Io {
                path: path.to_path_buf(),
                source: e,
            })?;

            default_config
        };

        Ok(app_config)
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
                err: "format cannot be empty".to_owned(),
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

    pub fn line_gutter_width(&self) -> usize {
        let mut width = 0;

        if self.display_live_timestamp {
            let sample_time = chrono::Local::now();
            let ts_str = sample_time.format(&self.timestamp_format).to_string();
            width += ts_str.chars().count();
        }

        width += self.line_mode.line_prefix.chars().count();

        width
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
                write!(f, "invalid log/gutter timestamp format: {err}")
            }
            Self::InvalidStatusBarTimeFormat { err } => {
                write!(f, "invalid status bar time_format: {err}")
            }
        }
    }
}

impl std::error::Error for ConfigValidationError {}

#[derive(Debug)]
pub enum SettingsError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    Validation {
        path: PathBuf,
        source: ConfigValidationError,
    },
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    f,
                    "failed to read settings at '{}': {source}",
                    path.display()
                )
            }
            Self::Parse { path, source } => {
                write!(
                    f,
                    "invalid settings syntax in '{}':\n{source}",
                    path.display()
                )
            }
            Self::Validation { path, source } => {
                write!(
                    f,
                    "settings validation failed for '{}': {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for SettingsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::Validation { source, .. } => Some(source),
        }
    }
}
