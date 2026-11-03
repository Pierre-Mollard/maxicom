use std::{
    borrow::Cow,
    fmt::Display,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::format::strftime::StrftimeItems;
use serde::{Deserialize, Serialize};

use crate::{
    capture::SessionLimits,
    link::serial::{FlowControl, Framing, LineSettings, Parity},
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
/// first; `always` by default, the separator line marking where the live data starts (MAX-163)
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
        self.dir.join(chrono::Local::now().format(name).to_string())
    }
}

/// The session backup (MAX-172): with `enabled`, every session is recorded in the cache
/// folder, the last `keep` sessions kept, all together up to `max_size` (`0`: no limit,
/// MAX-163). `restore` puts the last session on the same device back in the history at
/// startup, when it ended within `restore_max_age` (0: no limit, MAX-148)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionsConfig {
    pub enabled: bool,
    pub keep: usize,
    pub max_size: ByteSize,
    pub restore: RestoreMode,
    #[serde(with = "humantime_serde")]
    pub restore_max_age: Duration,
}

impl Default for SessionsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            keep: 200,
            max_size: ByteSize(200 << 20),
            restore: RestoreMode::default(),
            restore_max_age: Duration::from_secs(24 * 3600),
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
            keep: self.keep,
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

impl ScreenshotConfig {
    /// The screenshot path without its extension, which comes with the format
    pub fn stem(&self) -> PathBuf {
        let name = chrono::Local::now().format(&self.name);
        self.dir.join(name.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LineModeConfig {
    pub tx_eol: TxNewlineSymbol,
    pub display_mode: LineDisplayMode,
    pub wrap_mode: WrapMode,
    pub wrap_indent: PrefixWrapBehavior,
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
            history_size: 1000,
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
            left: block_lines(&[
                "[ {mode} ](prefix?(bg=secondary fg=black bold):copy_raw?(bg=red fg=black bold):copy?(bg=magenta fg=black bold):search?(bg=blue fg=white bold):(fg=primary bold))",
                "[ {copy_side}](copy_raw?(fg=red bold):copy?(fg=magenta bold):hide)",
                "[ {port}](prio=3)",
                "[ |](muted prio=3)",
                "[ ](prio=2)",
                "[{baud}](primary prio=2)",
                "[ ](prio=4)",
                "[{framing}](prio=4)",
            ]),
            center: block_lines(&[
                "[◎ {trigger_count} ](armed?(fg=warn bold):hide)",
                "[⟳ mash ](mashing?(fg=secondary bold):hide click=cancel)",
                "[⇡ {send_file} {send_percent:>3}% ](sending?(fg=secondary bold):hide)",
                "[▷ {replay_state} {replay_percent:>3}% ](replaying?(fg=secondary bold):hide prio=5)",
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

        if let Err(err) = StrftimeItems::new(&self.time_format).parse() {
            return Err(ConfigValidationError::InvalidStatusBarTimeFormat {
                err: err.to_string(),
            });
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
}

/// No device, 115200 8N1, no flow control
impl Default for LinkDefaults {
    fn default() -> Self {
        Self {
            device: String::new(),
            baud: 115_200,
            bits: 8,
            parity: Parity::None,
            stop_bits: 1,
            flow: FlowControl::None,
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
    pub show_scrollbar: bool,
    pub show_status_bar: bool,
    pub mouse: bool,
    /// Each part hides on a screen narrower or shorter than its `[cols, rows]`, a `0` side
    /// never (MAX-181): the borders first, the status bar last, as minicom always shows its own
    pub hide_borders_below: [u16; 2],
    pub hide_scrollbar_below: [u16; 2],
    pub hide_status_bar_below: [u16; 2],
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
            hide_borders_below: [60, 16],
            hide_scrollbar_below: [40, 10],
            hide_status_bar_below: [35, 6],
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
    pub add_cr: bool,
    pub add_lf: bool,
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
    pub sessions: SessionsConfig,
    pub screenshot: ScreenshotConfig,
    pub theme_path: Option<PathBuf>,
    pub keybindings_path: Option<PathBuf>,
    pub highlights_path: Option<PathBuf>,
    pub triggers_path: Option<PathBuf>,
    pub links_path: Option<PathBuf>,
    pub link: LinkDefaults,
    pub ui: UiConfig,
    pub raw_side: RawSideConfig,
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
            dual_mode_buffering: DualBufferMode::default(),
            show_timestamps: false,
            timestamp_format: String::from("[%H:%M:%S%.3f]"),
            line_mode: LineModeConfig::default(),
            terminal_mode: TerminalModeConfig::default(),
            capture: CaptureConfig::default(),
            sessions: SessionsConfig::default(),
            screenshot: ScreenshotConfig::default(),
            inactivity_detach_timeout: DEFAULT_INACTIVITY_DETACH,
            theme_path: None,
            keybindings_path: None,
            highlights_path: None,
            triggers_path: None,
            links_path: None,
            link: LinkDefaults::default(),
            ui: UiConfig::default(),
            raw_side: RawSideConfig::default(),
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
            ("capture.name", &self.capture.name),
            ("screenshot.name", &self.screenshot.name),
        ] {
            if let Err(err) = StrftimeItems::new(name).parse() {
                return Err(ConfigValidationError::InvalidCaptureName {
                    field,
                    err: err.to_string(),
                });
            }
        }

        self.ui.validate()?;
        self.link.validate()?;

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

    /// MAX-181: `[link]` reads minicom-like numbers and names, an empty or missing `device`
    /// is none, and out-of-range bits, stop bits or baud fail the check.
    /// How: a full section, an empty one, then three bad values through `SettingsConfig`.
    #[test]
    fn link_defaults_from_toml() {
        let parse = |toml: &str| toml::from_str::<LinkDefaults>(toml).unwrap();
        let link = parse(
            "device = \"COM7\"\nbaud = 9600\nbits = 7\nparity = \"even\"\n\
             stop_bits = 2\nflow = \"rtscts\"",
        );
        assert_eq!(link.device(), Some("COM7"));
        let line = link.line();
        assert_eq!(
            (line.baud, line.framing.to_string(), line.flow_control),
            (9600, "7E2".to_string(), FlowControl::RtsCts)
        );
        let empty = parse("");
        assert_eq!(empty, LinkDefaults::default());
        assert_eq!(empty.device(), None);
        assert_eq!(empty.line(), LineSettings::default());

        for (toml, name) in [
            ("bits = 9", "link.bits"),
            ("stop_bits = 0", "link.stop_bits"),
            ("baud = 0", "link.baud"),
        ] {
            let settings = SettingsConfig {
                link: parse(toml),
                ..SettingsConfig::default()
            };
            let err = settings.validate().unwrap_err().to_string();
            assert!(err.starts_with(name), "{toml}: {err}");
        }
    }

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

    /// MAX-148: `restore_max_age` defaults to 24 hours; both read from the config, the age as
    /// a human duration, `0` included. MAX-163: `sessions.restore` defaults to `always`.
    #[test]
    fn restore_settings_parse() {
        let sessions = SessionsConfig::default();
        assert_eq!(sessions.restore, RestoreMode::Always);
        assert_eq!(sessions.restore_max_age, Duration::from_secs(24 * 3600));
        let parse = |toml: &str| toml::from_str::<SessionsConfig>(toml).unwrap();
        let sessions = parse("restore = \"ask\"\nrestore_max_age = \"2h 30m\"");
        assert_eq!(sessions.restore, RestoreMode::Ask);
        assert_eq!(sessions.restore_max_age, Duration::from_secs(9000));
        assert!(parse("restore_max_age = \"0s\"").restore_max_age.is_zero());
        assert_eq!(parse("restore = \"never\"").restore, RestoreMode::Never);
    }

    /// MAX-163: `max_size` reads a number with a unit by 1024 (`B` and `iB` optional, any
    /// case), a decimal, or bare bytes; `0` is no limit; it writes back in its biggest whole
    /// unit; a bad unit or number fails the config.
    #[test]
    fn session_max_size_parses() {
        let parse = |toml: &str| toml::from_str::<SessionsConfig>(toml).map(|s| s.max_size.0);
        assert_eq!(parse("max_size = \"200M\""), Ok(200 << 20));
        assert_eq!(parse("max_size = \"200MB\""), Ok(200 << 20));
        assert_eq!(parse("max_size = \"512KiB\""), Ok(512 << 10));
        assert_eq!(parse("max_size = \"1.5gb\""), Ok(3 << 29));
        assert_eq!(parse("max_size = \"100\""), Ok(100));
        assert_eq!(parse("max_size = 100"), Ok(100));
        assert!(parse("max_size = \"200X\"").is_err());
        assert!(parse("max_size = \"big\"").is_err());

        let defaults = SessionsConfig::default();
        assert_eq!(defaults.limits().max_size, Some(200 << 20));
        assert_eq!(defaults.keep, 200);
        let unlimited = toml::from_str::<SessionsConfig>("max_size = \"0\"").unwrap();
        assert_eq!(unlimited.limits().max_size, None);

        for size in [200 << 20, 3 << 29, 1000, 0] {
            let text = ByteSize(size).to_string();
            assert_eq!(text.parse(), Ok(ByteSize(size)), "{text}");
        }
        assert_eq!(ByteSize(200 << 20).to_string(), "200M");
        assert_eq!(ByteSize(3 << 29).to_string(), "1536M");
    }

    /// MAX-163: the default status bar templates are written to the config file one block per
    /// line, in `'''` literal strings where TOML keeps each `\`, and read back the same.
    /// How: the default `[ui.status_bar]` written as the default config file is, then parsed.
    #[test]
    fn status_bar_templates_written_one_block_per_line() {
        let config = StatusBarConfig::default();
        let written = toml::to_string_pretty(&config).unwrap();
        assert!(
            written.contains("left = '''\n[ {mode} ](prefix?"),
            "{written}"
        );
        assert!(
            written.contains("\\\n    [ {copy_side}](copy_raw?"),
            "{written}"
        );
        assert!(written.contains("[{framing}](prio=4)'''"), "{written}");
        assert_eq!(toml::from_str::<StatusBarConfig>(&written).unwrap(), config);
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
            14 + settings.line_prefixes().line.chars().count()
        );
    }
}
