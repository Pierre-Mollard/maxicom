use std::{
    borrow::Cow,
    fmt::Display,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::{
    capture::{DEFAULT_GROUP_BY, GroupLevel, SessionLimits},
    clock,
    configs::comments::{CONFIG_COMMENTS, with_comments},
    link::serial::{FlowControl, Framing, LineLevel, LineSettings, Parity},
};

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

/// A switch's value as the settings popup and its notices show it (MAX-235)
pub const fn on_off(on: bool) -> &'static str {
    if on { "On" } else { "Off" }
}

/// A side panel's share of the split, in percent
pub const SPLIT_PERCENT_MIN: u16 = 10;
pub const SPLIT_PERCENT_MAX: u16 = 85;

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

/// Where the plot panel sits (MAX-230): its own place, not the raw panel's, since a time series
/// reads along its X axis and a wide panel at the bottom shows more of it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlotSideConfig {
    pub orientation: RawSideOrientation,
    pub split_percent: u16,
}

impl Default for PlotSideConfig {
    fn default() -> Self {
        Self {
            orientation: RawSideOrientation::Bottom,
            split_percent: 35,
        }
    }
}

/// Where the command line is drawn (MAX-12): a centered one-row popup, or over the status bar,
/// the popup when the status bar isn't shown
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandLineLayout {
    #[default]
    Popup,
    StatusBar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommandLineConfig {
    pub layout: CommandLineLayout,
    pub history_size: usize,
}

impl Default for CommandLineConfig {
    fn default() -> Self {
        Self {
            layout: CommandLineLayout::default(),
            history_size: 100,
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
            // the controls stay, for the escape sequences (MAX-254): only bytes from 80 up are
            // not ASCII, as terminal mode reads them (MAX-249)
            Self::Ascii => bytes
                .iter()
                .map(|&b| if b.is_ascii() { b as char } else { '\u{FFFD}' })
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
            Self::Latin1 => "Latin1",
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

impl std::str::FromStr for TextEncoding {
    type Err = String;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name.to_ascii_lowercase().as_str() {
            "utf8" | "utf-8" => Ok(Self::Utf8),
            "ascii" | "us-ascii" => Ok(Self::Ascii),
            "cp437" | "ibm437" => Ok(Self::Cp437),
            "latin1" | "iso-8859-1" | "iso8859-1" => Ok(Self::Latin1),
            _ => Err(format!(
                "invalid encoding '{name}': expected utf8, ascii, cp437 or latin1"
            )),
        }
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

    /// The next one, as the settings popup cycles them (MAX-184)
    pub fn toggle(&mut self) {
        *self = match self {
            TxNewlineSymbol::Lf => TxNewlineSymbol::Cr,
            TxNewlineSymbol::Cr => TxNewlineSymbol::CrLf,
            TxNewlineSymbol::CrLf => TxNewlineSymbol::Lf,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            TxNewlineSymbol::CrLf => "CRLF",
            TxNewlineSymbol::Cr => "CR",
            TxNewlineSymbol::Lf => "LF",
        }
    }
}

/// The byte the Backspace key sends in terminal mode (MAX-270): DEL like xterm and Linux
/// consoles, or BS like minicom, for a device whose shell erases on BS only
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BackspaceByte {
    #[default]
    Del,
    Bs,
}

impl BackspaceByte {
    pub fn byte(&self) -> u8 {
        match self {
            BackspaceByte::Del => 0x7f,
            BackspaceByte::Bs => 0x08,
        }
    }

    /// The other one, as the settings popup cycles them
    pub fn toggle(&mut self) {
        *self = match self {
            BackspaceByte::Del => BackspaceByte::Bs,
            BackspaceByte::Bs => BackspaceByte::Del,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            BackspaceByte::Del => "DEL",
            BackspaceByte::Bs => "BS",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PrefixWrapBehavior {
    None,
    Prefix,
    #[default]
    Timestamp,
}

/// Where line mode marks its rows (full line, wrapped row, partial RX line): inside the pane
/// with the `inset_prefix` strings, in the left border with border glyphs, or nowhere. With
/// timestamps on, a `│` bar follows the timestamp in every mode and is the border `border`
/// marks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PrefixPlace {
    Inset,
    #[default]
    Border,
    None,
}

/// The glyphs `prefix_place = "border"` marks the rows with; `BORDER_LINE` is also the bar
/// after the timestamp
pub const BORDER_LINE: &str = "│";
pub const BORDER_WRAP: &str = "┆";
pub const BORDER_PARTIAL: &str = "╎";

/// What starts the rows with `prefix_place = "inset"`: a line, a wrapped row, the partial RX
/// line. As wide as each other, so the text of every row kind lines up
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InsetPrefix {
    pub line: String,
    pub wrap: String,
    pub partial: String,
}

impl Default for InsetPrefix {
    fn default() -> Self {
        Self {
            line: String::from("> "),
            wrap: String::from("  "),
            partial: String::from("… "),
        }
    }
}

/// The prefixes laid out at the start of line mode rows, as `prefix_place` puts them, and the
/// separator drawn on the rows without text
pub struct LinePrefixes<'a> {
    pub line: Cow<'a, str>,
    pub indent: Cow<'a, str>,
    pub partial: Cow<'a, str>,
    pub separator: &'static str,
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
    Focus,
}

impl QueryGroup {
    pub const ALL: [Self; 5] = [
        Self::Status,
        Self::Identity,
        Self::Size,
        Self::Modes,
        Self::Focus,
    ];
}

/// The groups of what the emulator doesn't handle that `notify_unhandled` counts (MAX-264):
/// each item is in one. `Shell` holds what modern shells and programs send and a terminal may
/// ignore without harm (`Dropped::harmless`), not in `Osc`, `Csi` nor `Escape`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UnhandledGroup {
    /// C0 controls, SO and SI included, and DEL
    Controls,
    C1,
    Escape,
    Csi,
    Osc,
    /// DCS, SOS, PM, APC
    Strings,
    Shell,
}

impl UnhandledGroup {
    /// The groups only the tokenizer finds, no callback reporting them: a tokenizer of its own
    /// runs for them with the byte trace off
    pub const fn needs_tokens(self) -> bool {
        matches!(self, Self::Controls | Self::Strings)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Line,
    #[default]
    Terminal,
}

impl Mode {
    pub const fn other(self) -> Self {
        match self {
            Self::Line => Self::Terminal,
            Self::Terminal => Self::Line,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Terminal => "terminal",
        }
    }
}

/// The mode the user can't switch to, none by default (MAX-260)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DisabledMode {
    #[default]
    None,
    Line,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DualBufferMode {
    #[default]
    AlwaysBoth,
    ActiveOnly,
}

/// When the last session on the same device is restored, at startup and on a device switch
/// (MAX-148, MAX-187): `ask` asks first; `always` by default, the separator line marking where
/// the live data starts (MAX-163)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RestoreMode {
    Never,
    Ask,
    #[default]
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

/// Recordings of what is received (MAX-147): a recording is `dir` joined with the strftime
/// `name`, formatted when the recording dialog opens, without an extension: a text recording
/// adds `.log`, a full one is a folder of that name (MAX-175). An empty `dir` is the current
/// directory.
/// `timestamp`, `record_tx` and `tx_mark` set the text lines, of the recordings and of the
/// session backup, which takes them when it starts: `record_tx` adds the lines sent in line
/// mode, after `tx_mark` (`> line`, none when empty)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    pub dir: PathBuf,
    pub name: String,
    pub timestamp: bool,
    pub record_tx: bool,
    pub tx_mark: String,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            dir: PathBuf::new(),
            name: String::from("maxicom_%Y%m%d_%H%M%S"),
            timestamp: true,
            record_tx: true,
            tx_mark: String::from("> "),
        }
    }
}

impl CaptureConfig {
    pub fn path_for(&self, name: &str) -> PathBuf {
        self.dir.join(clock::format(Timestamp::now(), name))
    }
}

/// The session backup (MAX-172): with `enabled`, every session is recorded in the cache
/// folder, the last `keep` sessions kept, all together up to `max_size` (`0`: no limit,
/// MAX-163; `keep` 0 by default, the size alone limits, MAX-202). `group_by` are the time
/// levels of the session browser's tree (MAX-202). `restore` puts the last session on the same device back in the history at
/// startup and on a device switch (MAX-187), when it ended within `restore_max_age` (0: no
/// limit, MAX-148)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionsConfig {
    pub enabled: bool,
    pub keep: usize,
    pub max_size: ByteSize,
    pub restore: RestoreMode,
    #[serde(with = "super::duration")]
    pub restore_max_age: Duration,
    pub group_by: Vec<GroupLevel>,
}

impl Default for SessionsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            keep: 0,
            max_size: ByteSize(5 << 30),
            restore: RestoreMode::default(),
            restore_max_age: Duration::from_secs(24 * 3600),
            group_by: DEFAULT_GROUP_BY.to_vec(),
        }
    }
}

impl SessionsConfig {
    /// How old a session can be to restore: none when `restore_max_age` is 0
    pub fn max_age(&self) -> Option<Duration> {
        (!self.restore_max_age.is_zero()).then_some(self.restore_max_age)
    }

    /// What the session folder keeps
    pub fn limits(&self) -> SessionLimits {
        SessionLimits {
            keep: (self.keep > 0).then_some(self.keep),
            max_size: (self.max_size.0 > 0).then_some(self.max_size.0),
        }
    }
}

/// A size in bytes, written as the status bar shows sizes: a number and a unit, `K`, `M`, `G`
/// or `T`, by 1024, with an optional `B` or `iB` (`200M`, `1.5GB`, `512KiB`), bare bytes
/// without one (MAX-163)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteSize(pub u64);

impl std::str::FromStr for ByteSize {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let text = text.trim();
        let split = text
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(text.len());
        let (number, unit) = text.split_at(split);
        let unit = unit.trim_start().to_ascii_uppercase();
        let unit = (unit.strip_suffix("IB").or_else(|| unit.strip_suffix('B'))).unwrap_or(&unit);
        let shift = match unit {
            "" => 0,
            "K" => 10,
            "M" => 20,
            "G" => 30,
            "T" => 40,
            _ => return Err(format!("unknown size unit in \"{text}\" (K, M, G or T)")),
        };
        let number: f64 = number
            .parse()
            .map_err(|_| format!("not a size: \"{text}\" (e.g. \"200M\")"))?;
        Ok(Self((number * (1u64 << shift) as f64).round() as u64))
    }
}

impl Display for ByteSize {
    /// In the biggest unit that writes it whole
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let units = [(40, "T"), (30, "G"), (20, "M"), (10, "K")];
        match units
            .iter()
            .find(|&&(shift, _)| self.0 > 0 && self.0.is_multiple_of(1u64 << shift))
        {
            Some(&(shift, unit)) => write!(f, "{}{unit}", self.0 >> shift),
            None => write!(f, "{}", self.0),
        }
    }
}

impl Serialize for ByteSize {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ByteSize {
    /// A string with its unit, or a bare number of bytes
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Bytes(u64),
            Text(String),
        }
        match Raw::deserialize(deserializer)? {
            Raw::Bytes(bytes) => Ok(Self(bytes)),
            Raw::Text(text) => text.parse().map_err(serde::de::Error::custom),
        }
    }
}

/// A screenshot is `dir` joined with `name` (strftime) and the format's extension, prefilled
/// in its dialog (MAX-147)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScreenshotConfig {
    pub dir: PathBuf,
    pub name: String,
}

impl Default for ScreenshotConfig {
    fn default() -> Self {
        Self {
            dir: PathBuf::new(),
            name: String::from("screen_%Y%m%d_%H%M%S"),
        }
    }
}

/// Where `download` puts what it receives (MAX-162): `dir` when the command names no
/// folder, the current directory when empty, and `xmodem_name` (strftime) for an XMODEM file,
/// which comes with no name
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TransfersConfig {
    pub dir: PathBuf,
    pub xmodem_name: String,
}

impl Default for TransfersConfig {
    fn default() -> Self {
        Self {
            dir: PathBuf::new(),
            xmodem_name: String::from("xmodem_%Y%m%d_%H%M%S.bin"),
        }
    }
}

impl ScreenshotConfig {
    /// The screenshot path without its extension, which comes with the format
    pub fn stem(&self) -> PathBuf {
        self.dir.join(clock::format(Timestamp::now(), &self.name))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LineModeConfig {
    pub tx_eol: TxNewlineSymbol,
    pub display_mode: LineDisplayMode,
    pub wrap_mode: WrapMode,
    pub wrap_indent: PrefixWrapBehavior,
    /// Lines kept, memory used only as they come: about 1.3 KB a line with the default
    /// `AlwaysBoth`, and a resize measures them all (MAX-214)
    pub history_size: usize,
    pub send_history_size: usize,
    pub show_partial_rx: bool,
    pub entry_spacing: usize,
    pub prefix_place: PrefixPlace,
    pub inset_prefix: InsetPrefix,
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
            display_mode: LineDisplayMode::default(),
            wrap_mode: WrapMode::default(),
            wrap_indent: PrefixWrapBehavior::default(),
            history_size: 250_000,
            send_history_size: 1000,
            show_partial_rx: true,
            entry_spacing: 0,
            prefix_place: PrefixPlace::default(),
            inset_prefix: InsetPrefix::default(),
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
    /// `history_size × cols × 32 B` (100000 × 80 cols ≈ 256 MB, × 250 cols ≈ 800 MB), used only
    /// as the rows come, and a search keeps a copy of their text while it's open or its matches
    /// are kept (MAX-214).
    /// With `GrowOnly` the emulator keeps the widest width it has seen, and new lines use it
    /// even after the window shrinks (rows already in scrollback keep their own width)
    pub history_size: usize,
    pub empty_fill_char: String,
    pub unfocused_cursor_glyph: String,
    pub backspace: BackspaceByte,
    /// Record which received bytes drew which cells, to connect the screen and the raw side
    /// panel (MAX-256): about 256 B per `history_size` row at most, and some time on a fast link
    /// (TCP, a replay). Off: nothing recorded, the memory given back
    pub byte_trace: bool,
    /// Copy what the device asks to (OSC 52) to the clipboard, as maxicom's own copies go:
    /// off, as a device or a binary dump could replace what the user copied (MAX-275)
    pub device_clipboard: bool,
    /// Sent back on ENQ (`0x05`), the "answerback", with the `identity` group; nothing when
    /// empty: `0x05` also shows up in binary data
    pub enq_answer: String,
    /// The groups of the remote's queries answered (cursor position, size, device attributes,
    /// modes), in terminal mode only (MAX-151); none: no answers
    pub answer_groups: Vec<QueryGroup>,
    /// A notification counting what the emulator received and didn't handle, for these groups
    /// (MAX-264); none by default, an advanced notification
    pub notify_unhandled: Vec<UnhandledGroup>,
    /// A marker in the cell where something of these groups wasn't handled, in both modes
    /// (MAX-253); none by default, like minicom. It takes the cell: what comes after on the
    /// line moves one column right
    pub mark_unhandled: Vec<UnhandledGroup>,
}

impl Default for TerminalModeConfig {
    fn default() -> Self {
        Self {
            rows: 24,
            cols: 80,
            dimension_mode: TerminalDimensionMode::GrowOnly,
            history_size: 100_000,
            empty_fill_char: String::from("░"),
            unfocused_cursor_glyph: String::from("█"),
            backspace: BackspaceByte::default(),
            byte_trace: true,
            device_clipboard: false,
            enq_answer: String::new(),
            answer_groups: QueryGroup::ALL.to_vec(),
            notify_unhandled: Vec::new(),
            mark_unhandled: Vec::new(),
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
            left: block_lines(&[
                "[ {mode} ](prefix?(bg=secondary fg=black bold):mark?(bg=good fg=black bold):search?(bg=search fg=black bold):copy_raw?(bg=selection_raw fg=black bold):copy?(bg=selection fg=black bold):(fg=primary bold))",
                "[|](muted prio=3)",
                "[ {port}](link prio=3)",
                "[ |](muted prio=3)",
                "[ ](prio=2)",
                "[{baud}](primary prio=2)",
                "[ ](prio=4)",
                "[{framing}](prio=4)",
                "[ |](term?muted:hide prio=4)",
                "[ {cursor_keys}](term?muted:hide prio=4)",
            ]),
            center: block_lines(&[
                "[◎ {trigger_count} ](armed?primary:hide)",
                "[⟳ mash ](mashing?(fg=secondary bold):hide click=cancel)",
                "[▷ {replay_state} {replay_percent:>3}% ](replaying?(fg=secondary bold):hide prio=5)",
                "[∿ {plot} ](plot?(frozen?(fg=warn bold):(fg=secondary)):hide prio=5)",
                "[« live ](replaced?(fg=main bold):hide click=toggle-attach)",
                "[+CR ](add_cr?(fg=warn bold):hide click=toggle-add-cr)",
                "[+LF ](add_lf?(fg=warn bold):hide click=toggle-add-lf)",
                "[● REC ](rec?(fg=warn bold):hide click=record)",
                "[◀ ](wide?(pan_left?secondary:muted):hide click=pan-left)",
                "[{pan_offset:>3}/{pan_max:<3}](wide?muted:hide)",
                "[ ▶](wide?(pan_alert?(fg=warn bold):(pan_right?secondary:muted)):hide click=pan-right)",
            ]),
            right: block_lines(&[
                "[{scroll_pos_up} ](has_scroll?muted:hide prio=2)",
                "[RX: {rx_bytes:4h}](muted prio=3)",
                "[ | ](muted prio=3)",
                "[TX: {tx_bytes:4h}](muted prio=3)",
                "[ | ](muted prio=4)",
                "[{time}](secondary prio=4) ",
                "[?](muted click=help hover=(bg=primary fg=black)) ",
                "[≡](muted click=settings hover=(bg=secondary fg=black)) ",
            ]),
            time_format: String::from("%H:%M:%S"),
            hover: String::from("reversed"),
            layout: StatusBarLayout::Border,
        }
    }
}

/// A default status bar template written one block per line (MAX-163): each line but the last
/// ends with `\`, which the template parser joins with the next one, its indentation dropped
fn block_lines(blocks: &[&str]) -> String {
    blocks.join("\\\n    ")
}

impl StatusBarConfig {
    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        if self.time_format.is_empty() {
            return Err(ConfigValidationError::InvalidStatusBarTimeFormat {
                err: "format cannot be empty".to_owned(),
            });
        }

        if let Err(err) = clock::check(&self.time_format) {
            return Err(ConfigValidationError::InvalidStatusBarTimeFormat { err });
        }

        Ok(())
    }
}

/// The link opened when the command line names none, as minicom's `minirc.dfl` (MAX-181).
/// No device by default, on any system: a port's name depends on the adapter, and a guessed
/// one that isn't there would be retried forever. Read at startup, the command line over it
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LinkDefaults {
    pub device: String,
    pub baud: u32,
    pub bits: u8,
    pub parity: Parity,
    pub stop_bits: u8,
    pub flow: FlowControl,
    pub dtr: LineLevel,
    pub rts: LineLevel,
}

/// No device, 115200 8N1, no flow control, DTR and RTS as the OS puts them
impl Default for LinkDefaults {
    fn default() -> Self {
        Self {
            device: String::new(),
            baud: 115_200,
            bits: 8,
            parity: Parity::None,
            stop_bits: 1,
            flow: FlowControl::None,
            dtr: LineLevel::Keep,
            rts: LineLevel::Keep,
        }
    }
}

impl LinkDefaults {
    /// `device`, none when empty
    pub fn device(&self) -> Option<&str> {
        Some(self.device.as_str()).filter(|device| !device.is_empty())
    }

    /// The line settings, `validate` having checked `bits` and `stop_bits`
    pub fn line(&self) -> LineSettings {
        let framing = Framing {
            char_size: self.bits.to_string().parse().unwrap_or_default(),
            parity: self.parity,
            stop_bits: self.stop_bits.to_string().parse().unwrap_or_default(),
        };
        LineSettings {
            baud: self.baud,
            framing,
            flow_control: self.flow,
            dtr: self.dtr,
            rts: self.rts,
        }
    }

    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        for (name, value, min, max) in [
            ("link.baud", self.baud as usize, 1, u32::MAX as usize),
            ("link.bits", self.bits.into(), 5, 8),
            ("link.stop_bits", self.stop_bits.into(), 1, 2),
        ] {
            if !(min..=max).contains(&value) {
                return Err(ConfigValidationError::InvalidGenericField {
                    name: name.to_owned(),
                    min,
                    max,
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub show_borders: bool,
    pub title_details: bool,
    /// The window title the device set (OSC 0 / 2) in the history's title, in the room left
    /// (MAX-275)
    pub show_device_title: bool,
    /// The icon name the device set (OSC 0 / 1), a second title: mostly the same text, so off
    pub show_device_icon_name: bool,
    pub show_scrollbar: bool,
    pub show_status_bar: bool,
    pub mouse: bool,
    /// Off, the window always looks focused: no gray border or cursor when it loses the
    /// focus (MAX-204)
    pub focus_effects: bool,
    /// A middle click pastes the last copy; off, it does nothing (MAX-212)
    pub middle_click_paste: bool,
    /// A row with the file being sent and its progress over the status bar, while one is sent
    /// (MAX-162); off, the status bar's `{send_*}` variables can show it
    pub transfer_row: bool,
    /// A notification after each copy, saying what was copied (MAX-269); off, only a clipboard
    /// error shows
    pub notify_copy: bool,
    /// Send the zero-width chars a cell holds (emoji joiners and selectors, soft hyphens) to
    /// the host terminal as received: only right when the host counts them as vt100 does,
    /// else the screen drawn drifts (MAX-270). Copy and search always have them
    pub draw_zero_width: bool,
    /// Send the chars hosts may draw at another width (`unsure_width`) as received, instead
    /// of a marker: only right when the host counts them as ratatui does, else the rest of the
    /// row drifts and pieces of earlier lines stay on screen (MAX-271). Copy and search always
    /// have them
    pub draw_unsure_width: bool,
    /// Each part hides on a screen narrower or shorter than its `[cols, rows]`, a `0` side
    /// never (MAX-181): the borders first, the status bar last, as minicom always shows its own
    pub hide_borders_below: [u16; 2],
    pub hide_scrollbar_below: [u16; 2],
    pub hide_status_bar_below: [u16; 2],
    pub status_bar: StatusBarConfig,
    pub marks: MarksConfig,
}

/// How the history's marks show (MAX-164), each part on its own: the letter in the left
/// column, on the scrollbar, the flash of the line jumped to
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MarksConfig {
    pub letters: bool,
    pub scrollbar: bool,
    pub flash: bool,
}

impl Default for MarksConfig {
    fn default() -> Self {
        Self {
            letters: true,
            scrollbar: true,
            flash: true,
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            show_borders: true,
            title_details: false,
            show_device_title: true,
            show_device_icon_name: false,
            show_scrollbar: true,
            show_status_bar: true,
            mouse: true,
            focus_effects: true,
            middle_click_paste: true,
            transfer_row: true,
            notify_copy: true,
            draw_zero_width: false,
            draw_unsure_width: false,
            hide_borders_below: [60, 16],
            hide_scrollbar_below: [40, 10],
            hide_status_bar_below: [35, 6],
            status_bar: StatusBarConfig::default(),
            marks: MarksConfig::default(),
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
    pub add_cr: bool,
    pub add_lf: bool,
    pub encoding: TextEncoding,
    pub mode: Mode,
    pub disabled_mode: DisabledMode,
    pub dual_mode_buffering: DualBufferMode,
    /// The plot of `plots.toml` shown at start, read at start only (MAX-222)
    pub plot: Option<String>,
    pub rx_buffer_size: usize,
    pub show_timestamps: bool,
    pub timestamp_format: String,
    #[serde(with = "super::duration")]
    pub inactivity_detach_timeout: Duration,
    pub line_mode: LineModeConfig,
    pub terminal_mode: TerminalModeConfig,
    pub capture: CaptureConfig,
    pub sessions: SessionsConfig,
    pub screenshot: ScreenshotConfig,
    pub transfers: TransfersConfig,
    pub theme_path: Option<PathBuf>,
    pub keybindings_path: Option<PathBuf>,
    pub highlights_path: Option<PathBuf>,
    pub triggers_path: Option<PathBuf>,
    pub links_path: Option<PathBuf>,
    pub plots_path: Option<PathBuf>,
    pub link: LinkDefaults,
    pub ui: UiConfig,
    pub raw_side: RawSideConfig,
    pub plot_side: PlotSideConfig,
    pub command_line: CommandLineConfig,

    // cache
    #[serde(skip)]
    timestamp_width: usize,
}

impl Default for SettingsConfig {
    fn default() -> Self {
        let mut settings = Self {
            local_echo: false,
            add_cr: false,
            add_lf: false,
            encoding: TextEncoding::default(),
            rx_buffer_size: 4096,
            mode: Mode::default(),
            disabled_mode: DisabledMode::default(),
            dual_mode_buffering: DualBufferMode::default(),
            plot: None,
            show_timestamps: false,
            timestamp_format: String::from("[%H:%M:%S%.3f]"),
            line_mode: LineModeConfig::default(),
            terminal_mode: TerminalModeConfig::default(),
            capture: CaptureConfig::default(),
            sessions: SessionsConfig::default(),
            screenshot: ScreenshotConfig::default(),
            transfers: TransfersConfig::default(),
            inactivity_detach_timeout: DEFAULT_INACTIVITY_DETACH,
            theme_path: None,
            keybindings_path: None,
            highlights_path: None,
            triggers_path: None,
            links_path: None,
            plots_path: None,
            link: LinkDefaults::default(),
            ui: UiConfig::default(),
            raw_side: RawSideConfig::default(),
            plot_side: PlotSideConfig::default(),
            command_line: CommandLineConfig::default(),
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

            fs::write(path, with_comments(&toml_string, CONFIG_COMMENTS)).map_err(|e| {
                SettingsError::Io {
                    path: path.to_path_buf(),
                    source: e,
                }
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

        if !self.mode_enabled(self.mode) {
            return Err(ConfigValidationError::DisabledMode(self.mode));
        }

        if self.timestamp_format.is_empty() {
            return Err(ConfigValidationError::InvalidTimestampFormat {
                err: "format cannot be empty".to_owned(),
            });
        }

        for (percent, name) in [
            (self.raw_side.split_percent, "raw_side.split_percent"),
            (self.plot_side.split_percent, "plot_side.split_percent"),
        ] {
            if !(SPLIT_PERCENT_MIN..=SPLIT_PERCENT_MAX).contains(&percent) {
                return Err(ConfigValidationError::InvalidGenericField {
                    name: name.to_owned(),
                    min: SPLIT_PERCENT_MIN as _,
                    max: SPLIT_PERCENT_MAX as _,
                });
            }
        }

        if let Err(err) = clock::check(&self.timestamp_format) {
            return Err(ConfigValidationError::InvalidTimestampFormat { err });
        };

        for (field, name) in [
            ("capture.name", &self.capture.name),
            ("screenshot.name", &self.screenshot.name),
            ("transfers.xmodem_name", &self.transfers.xmodem_name),
        ] {
            if let Err(err) = clock::check(name) {
                return Err(ConfigValidationError::InvalidCaptureName { field, err });
            }
        }

        self.ui.validate()?;
        self.link.validate()?;

        Ok(())
    }

    /// `mode` isn't the one `disabled_mode` turns off (MAX-260)
    pub fn mode_enabled(&self, mode: Mode) -> bool {
        !matches!(
            (self.disabled_mode, mode),
            (DisabledMode::Line, Mode::Line) | (DisabledMode::Terminal, Mode::Terminal)
        )
    }

    pub fn refresh_timestamp_width(&mut self) {
        let sample = clock::format(Timestamp::now(), &self.timestamp_format);
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

        width += self.line_prefixes().line.chars().count();

        width
    }

    /// The prefixes line mode rows start with: the `│` bar when timestamps are on (the
    /// timestamp's border), then the `inset_prefix` strings in `inset`. In `border` the bar
    /// takes the border glyphs; without timestamps there is no bar and the render marks the
    /// left border cell instead (`markers_in_border`), nothing when there is no left border
    pub fn line_prefixes<'a>(&'a self) -> LinePrefixes<'a> {
        let bar = if self.show_timestamps {
            BORDER_LINE
        } else {
            ""
        };
        let barred = |prefix: &'a str| -> Cow<'a, str> {
            if bar.is_empty() {
                Cow::Borrowed(prefix)
            } else {
                Cow::Owned(format!("{bar}{prefix}"))
            }
        };
        match self.line_mode.prefix_place {
            PrefixPlace::Inset => {
                let inset = &self.line_mode.inset_prefix;
                LinePrefixes {
                    line: barred(&inset.line),
                    indent: barred(&inset.wrap),
                    partial: barred(&inset.partial),
                    separator: bar,
                }
            }
            PrefixPlace::Border if self.show_timestamps => LinePrefixes {
                line: Cow::Borrowed(BORDER_LINE),
                indent: Cow::Borrowed(BORDER_WRAP),
                partial: Cow::Borrowed(BORDER_PARTIAL),
                separator: BORDER_LINE,
            },
            PrefixPlace::Border | PrefixPlace::None => LinePrefixes {
                line: Cow::Borrowed(bar),
                indent: Cow::Borrowed(bar),
                partial: Cow::Borrowed(bar),
                separator: bar,
            },
        }
    }

    /// Whether the row markers go in the left border cell, when there is one
    pub fn markers_in_border(&self) -> bool {
        self.line_mode.prefix_place == PrefixPlace::Border && !self.show_timestamps
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
    DisabledMode(Mode),
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
            Self::DisabledMode(mode) => {
                let mode = mode.as_str();
                write!(f, "mode is \"{mode}\", the mode disabled_mode turns off")
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
#[path = "../tests/configs/settings.rs"]
mod tests;
