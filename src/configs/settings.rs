use std::{
    borrow::Cow,
    fmt::Display,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::format::strftime::StrftimeItems;
use serde::{Deserialize, Serialize};

pub const DEFAULT_INACTIVITY_DETACH: Duration = Duration::from_secs(4 * 3600);
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

impl LineDisplayMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            LineDisplayMode::Pretty => "Pretty",
            LineDisplayMode::Clean => "Clean",
            LineDisplayMode::Raw => "Raw",
            LineDisplayMode::Hex => "Hex",
            LineDisplayMode::Octal => "Octal",
            LineDisplayMode::Binary => "Binary",
        }
    }

    pub fn toggle(&mut self) {
        *self = match *self {
            LineDisplayMode::Pretty => LineDisplayMode::Clean,
            LineDisplayMode::Clean => LineDisplayMode::Raw,
            LineDisplayMode::Raw => LineDisplayMode::Hex,
            LineDisplayMode::Hex => LineDisplayMode::Octal,
            LineDisplayMode::Octal => LineDisplayMode::Binary,
            LineDisplayMode::Binary => LineDisplayMode::Pretty,
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RawSideView {
    #[default]
    Off,
    Hex,
    #[serde(alias = "oct")]
    Octal,
    #[serde(alias = "bin", alias = "bits", alias = "byte", alias = "bytes")]
    Binary,
}

impl RawSideView {
    pub const fn as_str(self) -> &'static str {
        match self {
            RawSideView::Off => "Off",
            RawSideView::Hex => "Hex",
            RawSideView::Octal => "Octal",
            RawSideView::Binary => "Binary",
        }
    }

    pub fn toggle(&mut self) {
        *self = match *self {
            RawSideView::Off => RawSideView::Hex,
            RawSideView::Hex => RawSideView::Octal,
            RawSideView::Octal => RawSideView::Binary,
            RawSideView::Binary => RawSideView::Off,
        };
    }

    #[inline]
    pub fn stride(self) -> usize {
        match self {
            RawSideView::Hex => 3,
            RawSideView::Octal => 4,
            RawSideView::Binary => 9,
            RawSideView::Off => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RawSideOrientation {
    #[default]
    Right,
    Bottom,
}

pub fn raw_view_to_display_mode(view: RawSideView) -> LineDisplayMode {
    match view {
        RawSideView::Hex => LineDisplayMode::Hex,
        RawSideView::Octal => LineDisplayMode::Octal,
        RawSideView::Binary => LineDisplayMode::Binary,
        RawSideView::Off => LineDisplayMode::Clean,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RawSideConfig {
    pub view: RawSideView,
    pub orientation: RawSideOrientation,
    pub split_percent: u16,
}

impl Default for RawSideConfig {
    fn default() -> Self {
        Self {
            view: RawSideView::Off,
            orientation: RawSideOrientation::Right,
            split_percent: 40,
        }
    }
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

    /// `decode` without a copy for valid UTF-8, the common case; the other encodings decode
    pub fn decode_cow<'a>(&self, bytes: &'a [u8]) -> Cow<'a, str> {
        match self {
            Self::Utf8 => String::from_utf8_lossy(bytes),
            _ => Cow::Owned(self.decode(bytes)),
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
    CrLf,
    Cr,
    #[default]
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
#[serde(rename_all = "lowercase")]
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
    Prefix,
    #[default]
    Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalDimensionMode {
    GrowOnly,
    FixedCols,
    MatchView,
    Fixed,
}

impl TerminalDimensionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            TerminalDimensionMode::GrowOnly => "Grow Only",
            TerminalDimensionMode::MatchView => "Match View",
            TerminalDimensionMode::FixedCols => "Fixed Cols",
            TerminalDimensionMode::Fixed => "Fixed",
        }
    }

    pub fn toggle(&mut self) {
        *self = match *self {
            TerminalDimensionMode::GrowOnly => TerminalDimensionMode::MatchView,
            TerminalDimensionMode::MatchView => TerminalDimensionMode::FixedCols,
            TerminalDimensionMode::FixedCols => TerminalDimensionMode::Fixed,
            TerminalDimensionMode::Fixed => TerminalDimensionMode::GrowOnly,
        };
    }
}

/// The groups of the remote's queries that `answer_groups` turns on (MAX-151)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueryGroup {
    Status,
    Identity,
    Size,
    Modes,
}

impl QueryGroup {
    pub const ALL: [Self; 4] = [Self::Status, Self::Identity, Self::Size, Self::Modes];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Line,
    #[default]
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DualBufferMode {
    #[default]
    AlwaysBoth,
    ActiveOnly,
}

/// When the last session on the same device is restored at startup (MAX-148): `ask` asks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RestoreMode {
    Never,
    #[default]
    Ask,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WrapMode {
    #[default]
    Word,
    Char,
    Truncate,
}

/// Recordings of what is received (MAX-147): a file is `dir` joined with the stream's strftime
/// name, formatted when the recording dialog opens. An empty `dir` is the current directory.
/// `journal` backs up every session, apart from them, in the cache folder, keeping the last
/// `journal_keep` sessions. `record_tx` adds the lines sent in line mode to the text files,
/// after `tx_mark` (`> line`, none when empty). `restore` puts the last session on the same
/// device back in the history at startup, when it ended within `restore_max_age` (0: no limit,
/// MAX-148). A screenshot is `screenshot_dir` joined with `screenshot_name` and the format's
/// extension, prefilled in its dialog
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    pub dir: PathBuf,
    pub text_name: String,
    pub raw_name: String,
    pub timestamp: bool,
    pub record_tx: bool,
    pub tx_mark: String,
    pub journal: bool,
    pub journal_keep: usize,
    pub screenshot_dir: PathBuf,
    pub screenshot_name: String,
    pub restore: RestoreMode,
    #[serde(with = "humantime_serde")]
    pub restore_max_age: Duration,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            dir: PathBuf::new(),
            text_name: String::from("maxicom_%Y%m%d_%H%M%S.log"),
            raw_name: String::from("maxicom_%Y%m%d_%H%M%S.raw"),
            timestamp: true,
            record_tx: true,
            tx_mark: String::from("> "),
            journal: true,
            journal_keep: 20,
            screenshot_dir: PathBuf::new(),
            screenshot_name: String::from("screen_%Y%m%d_%H%M%S"),
            restore: RestoreMode::default(),
            restore_max_age: Duration::from_secs(24 * 3600),
        }
    }
}

impl CaptureConfig {
    pub fn path_for(&self, name: &str) -> PathBuf {
        self.dir.join(chrono::Local::now().format(name).to_string())
    }

    /// The screenshot path without its extension, which comes with the format
    pub fn screenshot_stem(&self) -> PathBuf {
        let name = chrono::Local::now().format(&self.screenshot_name);
        self.screenshot_dir.join(name.to_string())
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
    pub history_size: usize,
    pub send_history_size: usize,
    pub show_partial_rx: bool,
    pub line_prefix: String,
    pub line_indent_prefix: String,
    pub line_partial_prefix: String,
    pub unfocused_cursor_glyph: String,
    pub prompt_prefix: String,
    pub prompt_vi_prefix: String,
    pub prompt_hidden_left: String,
    pub prompt_hidden_right: String,
}

impl Default for LineModeConfig {
    fn default() -> Self {
        Self {
            tx_eol: TxNewlineSymbol::default(),
            rx_eol: RxNewlineMode::default(),
            display_mode: LineDisplayMode::default(),
            wrap_mode: WrapMode::default(),
            wrap_indent: PrefixWrapBehavior::default(),
            history_size: 1000,
            send_history_size: 1000,
            show_partial_rx: true,
            line_prefix: String::from("│ "),
            line_indent_prefix: String::from("│ "),
            line_partial_prefix: String::from("│…"),
            unfocused_cursor_glyph: String::from("█"),
            prompt_prefix: String::from("> "),
            prompt_vi_prefix: String::from("< "),
            prompt_hidden_left: String::from("◀"),
            prompt_hidden_right: String::from("▶"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalModeConfig {
    pub rows: u16,
    pub cols: u16,
    pub dimension_mode: TerminalDimensionMode,
    /// Scrollback lines kept by the emulator. Biggest memory user of the app: vt100 stores
    /// every line as a full row of 32 B cells, even blank ones, so memory is about
    /// `history_size × cols × 32 B` (2500 × 80 cols ≈ 6.4 MB, × 250 cols ≈ 20 MB).
    /// With `GrowOnly` the emulator keeps the widest width it has seen, and new lines use it
    /// even after the window shrinks (rows already in scrollback keep their own width)
    pub history_size: usize,
    pub empty_fill_char: String,
    pub unfocused_cursor_glyph: String,
    /// Answer the remote's queries (cursor position, size, device attributes, modes), in
    /// terminal mode only, for the groups of `answer_groups` (MAX-151)
    pub answer_queries: bool,
    pub answer_groups: Vec<QueryGroup>,
    /// Sent back on ENQ (`0x05`), nothing when empty: `0x05` also shows up in binary data
    pub answerback: String,
}

impl Default for TerminalModeConfig {
    fn default() -> Self {
        Self {
            rows: 24,
            cols: 80,
            dimension_mode: TerminalDimensionMode::GrowOnly,
            history_size: 2500,
            empty_fill_char: String::from("░"),
            unfocused_cursor_glyph: String::from("█"),
            answer_queries: true,
            answer_groups: QueryGroup::ALL.to_vec(),
            answerback: String::new(),
        }
    }
}

/// How the status bar is drawn: a plain row, one filled with the theme's `status_bar` bg, in
/// the last box's bottom border, or in a box of its own (MAX-135)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatusBarLayout {
    Line,
    Band,
    #[default]
    Border,
    Box,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusBarConfig {
    pub left: String,
    pub center: String,
    pub right: String,
    pub time_format: String,
    pub hover: String,
    pub layout: StatusBarLayout,
}

impl Default for StatusBarConfig {
    fn default() -> Self {
        Self {
            left: String::from(
                "[ {mode} ](prefix?(bg=secondary fg=black bold):copy_raw?(bg=red fg=black bold):copy?(bg=magenta fg=black bold):search?(bg=blue fg=white bold):(fg=primary bold))[ {copy_side}](copy_raw?(fg=red bold):copy?(fg=magenta bold):hide)[ {port}](prio=3)[ |](muted prio=3)[ ](prio=2)[{baud}](primary prio=2)[ ](prio=4)[{framing}](prio=4)",
            ),
            center: String::from(
                "[● REC ](rec?(fg=warn bold):hide click=toggle_logging)[◀ ](wide?(pan_left?secondary:muted):hide click=pan_left)[{pan_offset:>3}/{pan_max:<3}](wide?muted:hide)[ ▶](wide?(pan_alert?(fg=warn bold):(pan_right?secondary:muted)):hide click=pan_right)",
            ),
            right: String::from(
                "[{scroll_pos_up} ](has_scroll?muted:hide prio=2)[RX: {rx_bytes:4h}](muted prio=3)[ | ](muted prio=3)[TX: {tx_bytes:4h}](muted prio=3)[ | ](muted prio=4)[{time}](secondary prio=4) [?](muted click=help hover=(bg=primary fg=black)) [≡](muted click=settings hover=(bg=secondary fg=black)) ",
            ),
            time_format: String::from("%H:%M:%S"),
            hover: String::from("reversed"),
            layout: StatusBarLayout::Border,
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
    pub title_details: bool,
    pub show_scrollbar: bool,
    pub show_status_bar: bool,
    pub mouse: bool,
    pub compact_cols: u16,
    pub compact_rows: u16,
    pub status_bar: StatusBarConfig,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            show_borders: true,
            title_details: false,
            show_scrollbar: true,
            show_status_bar: true,
            mouse: true,
            compact_cols: 50,
            compact_rows: 14,
            status_bar: StatusBarConfig::default(),
        }
    }
}

impl UiConfig {
    /// The status bar layout in use: `border` and `box` need borders, else it's `line`
    pub fn status_bar_layout(&self, show_borders: bool) -> StatusBarLayout {
        match self.status_bar.layout {
            StatusBarLayout::Border | StatusBarLayout::Box if !show_borders => {
                StatusBarLayout::Line
            }
            layout => layout,
        }
    }

    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        self.status_bar.validate()
    }
}

/// maxicom UI & session settings
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsConfig {
    pub local_echo: bool,
    pub encoding: TextEncoding,
    pub mode: Mode,
    pub dual_mode_buffering: DualBufferMode,
    pub rx_buffer_size: usize,
    pub show_timestamps: bool,
    pub timestamp_format: String,
    #[serde(with = "humantime_serde")]
    pub inactivity_detach_timeout: Duration,
    pub line_mode: LineModeConfig,
    pub terminal_mode: TerminalModeConfig,
    pub capture: CaptureConfig,
    pub theme_path: Option<PathBuf>,
    pub keybindings_path: Option<PathBuf>,
    pub highlights_path: Option<PathBuf>,
    pub ui: UiConfig,
    pub raw_side: RawSideConfig,

    // cache
    #[serde(skip)]
    timestamp_width: usize,
}

impl Default for SettingsConfig {
    fn default() -> Self {
        let mut settings = Self {
            local_echo: false,
            encoding: TextEncoding::default(),
            rx_buffer_size: 4096,
            mode: Mode::default(),
            dual_mode_buffering: DualBufferMode::default(),
            show_timestamps: false,
            timestamp_format: String::from("[%H:%M:%S%.3f]"),
            line_mode: LineModeConfig::default(),
            terminal_mode: TerminalModeConfig::default(),
            capture: CaptureConfig::default(),
            inactivity_detach_timeout: DEFAULT_INACTIVITY_DETACH,
            theme_path: None,
            keybindings_path: None,
            highlights_path: None,
            ui: UiConfig::default(),
            raw_side: RawSideConfig::default(),
            timestamp_width: 0,
        };
        settings.refresh_timestamp_width();
        settings
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
            let mut config: SettingsConfig =
                toml::from_str(&content).map_err(|e| SettingsError::Parse {
                    path: path.to_path_buf(),
                    source: e,
                })?;
            config.validate().map_err(|err| SettingsError::Validation {
                path: path.to_path_buf(),
                source: err,
            })?;
            config.refresh_timestamp_width();
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

        if self.raw_side.split_percent < 10 || self.raw_side.split_percent > 85 {
            return Err(ConfigValidationError::InvalidGenericField {
                name: "raw_side.split_percent".to_owned(),
                min: 10,
                max: 85,
            });
        }

        if let Err(err) = StrftimeItems::new(&self.timestamp_format).parse() {
            return Err(ConfigValidationError::InvalidTimestampFormat {
                err: err.to_string(),
            });
        };

        for (field, name) in [
            ("capture.text_name", &self.capture.text_name),
            ("capture.raw_name", &self.capture.raw_name),
            ("capture.screenshot_name", &self.capture.screenshot_name),
        ] {
            if let Err(err) = StrftimeItems::new(name).parse() {
                return Err(ConfigValidationError::InvalidCaptureName {
                    field,
                    err: err.to_string(),
                });
            }
        }

        self.ui.validate()?;

        Ok(())
    }

    pub fn refresh_timestamp_width(&mut self) {
        let sample = chrono::Local::now()
            .format(&self.timestamp_format)
            .to_string();
        self.timestamp_width = sample.chars().count();
    }

    #[inline]
    pub fn timestamp_width(&self) -> usize {
        self.timestamp_width
    }

    pub fn line_gutter_width(&self) -> usize {
        let mut width = 0;

        if self.show_timestamps {
            width += self.timestamp_width;
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
    InvalidCaptureName {
        field: &'static str,
        err: String,
    },
    InvalidGenericField {
        name: String,
        min: usize,
        max: usize,
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
            Self::InvalidCaptureName { field, err } => write!(f, "invalid {field}: {err}"),
            Self::InvalidGenericField { name, min, max } => {
                write!(f, "{name} is not a value between {min} and {max}")
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

#[cfg(test)]
mod tests {
    //! Settings derived at load time.

    use super::*;

    /// MAX-148: `capture.restore` defaults to `ask` and `restore_max_age` to 24 hours; both
    /// read from the config, the age as a human duration, `0` included.
    /// MAX-151: the answer settings are on by default, groups by name; `colors` is no group
    #[test]
    fn answer_settings_parse() {
        let parse = |toml: &str| toml::from_str::<TerminalModeConfig>(toml);
        let config = parse("").unwrap();
        assert!(config.answer_queries);
        assert_eq!(config.answer_groups, QueryGroup::ALL);
        assert_eq!(config.answerback, "");

        let config = parse("answer_groups = [\"size\", \"modes\"]\nanswerback = \"x\"").unwrap();
        assert_eq!(config.answer_groups, [QueryGroup::Size, QueryGroup::Modes]);
        assert_eq!(config.answerback, "x");
        assert!(parse("answer_groups = [\"colors\"]").is_err());
    }

    #[test]
    fn restore_settings_parse() {
        let capture = CaptureConfig::default();
        assert_eq!(capture.restore, RestoreMode::Ask);
        assert_eq!(capture.restore_max_age, Duration::from_secs(24 * 3600));
        let parse = |toml: &str| toml::from_str::<CaptureConfig>(toml).unwrap();
        let capture = parse("restore = \"always\"\nrestore_max_age = \"2h 30m\"");
        assert_eq!(capture.restore, RestoreMode::Always);
        assert_eq!(capture.restore_max_age, Duration::from_secs(9000));
        assert!(parse("restore_max_age = \"0s\"").restore_max_age.is_zero());
        assert_eq!(parse("restore = \"never\"").restore, RestoreMode::Never);
    }

    /// The cached timestamp width matches a formatted timestamp, and feeds the line gutter.
    /// How: default format "[%H:%M:%S%.3f]" is 14 chars, plus the 2-char line prefix.
    #[test]
    fn timestamp_width_is_cached_from_format() {
        let mut settings = SettingsConfig::default();
        assert_eq!(settings.timestamp_width(), 14);
        settings.show_timestamps = true;
        assert_eq!(
            settings.line_gutter_width(),
            14 + settings.line_mode.line_prefix.chars().count()
        );
    }
}
