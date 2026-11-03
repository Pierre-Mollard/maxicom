//! # Status Bar Formatting Engine
//!
//! The status bar supports dynamic templating, Starship-style styling,
//! conditional ternaries, and element visibility toggling.
//!
//! ## Syntax
//! - **Plain Text**: Rendered using the base theme style `status_bar`, on the UI background
//!   unless the layout is `band`. Blocks with their own `bg=` keep it.
//! - **Variables**: `{variable_name}` replaced dynamically at runtime.
//! - **Styled Blocks**: `[content](style_spec)`
//! - **Conditionals**: `[content](condition?true_style:false_style)`
//! - **Grouping**: Parentheses `(style1 style2)` group styles in conditional branches.
//!
//! ## Layouts
//! `layout` in `[ui.status_bar]`:
//! - `line`: a row of its own, plain text on the UI background
//! - `band`: a row filled with the `status_bar` bg
//! - `border` (default): in the bottom border of the last box (the TX box in line mode, the
//!   history in terminal mode, where a search takes its place while open)
//! - `box`: in a box of its own, 3 rows high
//!
//! `border` and `box` need borders, else they are `line`.
//!
//! With borders on, `line` and `band` are inset one column each side.
//!
//! ## Available Variables
//! | Variable            | Description                                             | Example          |
//! |---------------------|---------------------------------------------------------|------------------|
//! | `{mode}`            | Current input mode (`TERM` or `LINE`)                   | `TERM`           |
//! | `{port}`            | Address: serial path, `tcp://host:port`, `file://name`  | `/dev/ttyUSB0`   |
//! | `{baud}`            | Baudrate (a replayed session's), `raw` (TCP), `replay`  | `115200`         |
//! | `{framing}`         | Data bits, parity, stop bits, `TCP`, the replay's state | `8N1`            |
//! | `{rx_bytes}`        | Total bytes received                                    | `1024`           |
//! | `{tx_bytes}`        | Total bytes transmitted                                 | `256`            |
//! | `{time}`            | Current local time (format in config)                   | `21:45:00`       |
//! | `{pan_left}`        | Left pan amount possible                                | `◀ (3)`          |
//! | `{pan_right}`       | Right pan amount possible with alert indicator          | `!(2) ▶`         |
//! | `{pan_offset}`      | Columns panned from the left edge                       | `3`              |
//! | `{pan_max}`         | Columns the view can pan in all                         | `117`            |
//! | `{scroll_pos}`      | Scroll position indicator (`Top`, `Bot`, `All`, or `%`) | `42%`            |
//! | `{scroll_pos_up}`   | Uppercase scroll position indicator                     | `BOT`            |
//! | `{scroll_percent}`  | Scroll position as numeric percentage (0-100)           | `42`             |
//! | `{scroll_offset}`   | Lines scrolled up from the bottom                       | `15`             |
//! | `{scroll_max}`      | Maximum total scrollback lines available                | `500`            |
//! | `{copy_side}`       | Copy cursor panel (`TXT` or `RAW`), empty out of copy  | `RAW`            |
//! | `{send_file}`       | The file `send-file` sends, empty when none             | `fw.hex`         |
//! | `{send_percent}`    | How much of it is sent, in percent                      | `42`             |
//! | `{trigger_count}`   | How many triggers (`triggers.toml`) are armed           | `2`              |
//! | `{trigger_names}`   | The armed triggers by name, empty when none             | `login, u-boot`  |
//! | `{replay_file}`     | The file the replay reads, empty when none              | `boot.raw`       |
//! | `{replay_state}`    | The replay's state (`RUN`, `PAUSE`, `END`), or empty    | `PAUSE`          |
//! | `{replay_percent}`  | How much of the file the replay read, in percent        | `42`             |
//! | `{replay_read}`     | The bytes it read                                       | `4096`           |
//! | `{replay_size}`     | The file's size                                         | `9752`           |
//!
//! ## Formats
//! `{name:spec}` with spec `[<>^][0][width][h]`, close to Rust's and C's:
//! - `{rx_bytes:8}` pads to 8 columns, numbers on the right and text on the left
//! - `<`, `>`, `^` align left, right, center: `{port:<12}`
//! - `0` pads a number with zeros: `{rx_bytes:08}`
//! - `h` writes a number as a short human-readable size with a unit letter (`999`, `1.0K`,
//!   `12K`, `1.2M`), never wider than 4 columns: `{rx_bytes:4h}`
//!
//! The numbers are `{baud}`, `{rx_bytes}`, `{tx_bytes}`, `{scroll_percent}`, `{scroll_offset}`,
//! `{scroll_max}`, `{pan_offset}`, `{pan_max}`, `{send_percent}`, `{trigger_count}`,
//! `{replay_percent}`, `{replay_read}` and `{replay_size}`. A longer value is never cut; a
//! malformed spec stays as written.
//!
//! ## The pan
//! It shows once: in the bar when it is drawn and a template has a pan variable (`{pan_left}`,
//! `{pan_right}`, `{pan_offset}`, `{pan_max}`), else as arrows in the history's bottom border.
//! The default center part is `◀ offset/max ▶`, the numbers padded so it doesn't move while
//! panning, shown while the content is wider than the view (`wide`).
//!
//! ## Conditional States (`?`)
//! | State / Keyword                 | Aliases                                    | Matches When...                                          |
//! |---------------------------------|--------------------------------------------|----------------------------------------------------------|
//! | `prefix`                        | —                                          | Leader prefix key (`Ctrl-A`) is currently active         |
//! | `rec`                           | `logging`                                  | File recording / capture session is active               |
//! | `session`                       | `backup`                                   | The session backup runs (not in the default layout)      |
//! | `line`                          | `linemode`                                 | Line Input mode is active                                |
//! | `term`                          | `terminal`                                 | Direct Terminal mode is active                           |
//! | `copy`                          | `copymode`                                 | Copy / Backlog Scrollback mode is active                 |
//! | `copy_raw`                      | `raw_cursor`                               | Copy mode cursor is in the raw side panel                |
//! | `search`                        | —                                          | Search mode is active                                    |
//! | `overlay`                       | `popup`                                    | A modal dialog or popup is open                          |
//! | `focused`                       | —                                          | Terminal window has OS desktop focus                     |
//! | `is_live`                       | `live`, `is_bottom`, `bottom`, `bot`       | Viewport is following live serial tail (offset is 0)     |
//! | `is_top`                        | `top`                                      | Viewport is scrolled to the absolute top of history      |
//! | `is_scrolled`                   | `scrolled`, `paused`                       | Viewport is scrolled up into history (not live tail)     |
//! | `scroll_up`                     | `can_scroll_up`                            | More history lines are available above the viewport      |
//! | `scroll_down`                   | `can_scroll_down`                          | More history lines are available below the viewport      |
//! | `has_scroll`                    | —                                          | Total buffer lines exceed visible screen height          |
//! | `panned`                        | `pan`, `panning`                           | Viewport is horizontally panned in either direction      |
//! | `pan_left`                      | `can_pan_left`                             | Horizontal panning to the left is possible               |
//! | `pan_right`                     | `can_pan_right`                            | Horizontal panning to the right is possible              |
//! | `pan_right_alert`               | `pan_alert`                                | New unread content arrived further to the right (Line)   |
//! | `wide`                          | `pannable`                                 | The content is wider than the view: it can pan           |
//! | `new_data`                      | `unread`, `missed`                         | Data arrived while scrolled up, not seen yet             |
//! | `mouse`                         | —                                          | Mouse capture is on (`ui.mouse`)                         |
//! | `add_cr`                        | —                                          | Add CR is on: a received LF read as CR LF (`add_cr`)     |
//! | `add_lf`                        | —                                          | Add LF is on: a received CR read as CR LF (`add_lf`)     |
//! | `line_settings`                 | `serial`                                   | Line settings to show: a serial port, a replayed session |
//! | `sending`                       | `send_file`                                | A file is being sent (`send-file`)                       |
//! | `armed`                         | `trigger`                                  | A trigger is armed, ready to fire (`triggers.toml`)      |
//! | `mashing`                       | `mash`                                     | `mash` sends its text, waiting for the other one         |
//! | `replaying`                     | `replay`                                   | A replay takes the link's place, ended or not            |
//!
//! ## Style Keywords
//! - **Modifiers**: `bold`, `dim`, `italic`, `underlined` (`underline`), `reversed` (`reverse`), `reset` (`default`), `crossed`, `slow_blink` (`sblink`), `rapid_blink` (`rblink`)
//! - **Visibility**: `hide`, `hidden`, `none` (omits the span completely)
//! - **Theme Palette Names**: `main` (or `primary`), `secondary`, `ternary`, `text`, `muted`, `warn` (`warning`), `good` (`ok`, `success`), `unselected`
//! - **Standard Named Colors**: `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `gray`, `darkgray`, `white`, or hex `#RRGGBB`
//! - **Explicit Foregrounds**: `fg=<color>` or simply `<color>`
//! - **Explicit Backgrounds**: `bg=<color>` (e.g. `bg=main`, `bg=red`, `bg=#1e1e2e`)
//! - **Clearing**: `fg=none` / `fg=reset`, `bg=none` / `bg=reset`
//! - **Mouse**: `click=<action>` runs a keymap action (`help`, `settings`, `quit`, ...) when the
//!   block is clicked. `hover=<style>` styles a clickable block under the mouse pointer
//!   (`hover=(bg=main fg=black)`, `hover=none` for none), over the `hover` option of
//!   `[ui.status_bar]` (default `reversed`). Neither is a style: they are the same in every
//!   branch of a ternary
//!
//! ## When it doesn't fit
//! A block can take `prio=N` (no style either, like `click=`): `1` is the most important.
//! When the three parts don't fit side by side, the bar tries, in order:
//! 1. the variables without a format of their own in a compact form: `{port}` as its file name
//!    (`ttyUSB0`), `{rx_bytes}`/`{tx_bytes}` as sizes (`1.2K`), the pan as `◀3` / `!2▶`,
//!    `{trigger_names}` as the first name and how many more (`login +1`);
//! 2. without the blocks of the highest `prio`, then the next one, and so on; the blocks with
//!    the same `prio` go together, so a separator goes with its block. A block without `prio=`
//!    and plain text always stay;
//! 3. without the center part, then the right part cut from its left, whole blocks only.
//!
//! The center part goes in the middle of the room the left and right parts leave, as drawn
//! once they fit, so it moves when they change width.
//!
//! ## Clicks
//! A block without `click=` runs what its first variable implies; a variable in plain text is
//! clickable on its own. The others (`{port}`, `{rx_bytes}`, `{tx_bytes}`, `{time}`,
//! `{scroll_max}`) do nothing: the port isn't changed while running.
//!
//! | Variable                                                   | Click                     |
//! |------------------------------------------------------------|---------------------------|
//! | `{mode}`                                                   | `toggle-input-mode`       |
//! | `{baud}`, `{framing}`                                      | `link-settings`           |
//! | `{copy_side}`                                              | the copy cursor's panel   |
//! | `{pan_left}`, `{pan_right}`                                | `pan-left`, `pan-right`   |
//! | `{pan_offset}`, `{pan_max}`                                | `pan-home`                |
//! | `{scroll_pos}` `{scroll_pos_up}` `{scroll_percent}` `{scroll_offset}` | `scroll-to-bottom` |
//! | `{send_file}`, `{send_percent}`                            | `cancel`                  |
//! | `{trigger_count}`, `{trigger_names}`                       | `triggers`                |
//!
//! The scroll and pan actions a `click=` can run: `scroll-up`, `scroll-down`, `page-up`,
//! `page-down`, `scroll-to-top`, `scroll-to-bottom`, `pan-left`, `pan-right`, `pan-home` (back
//! to the left edge). They move the view only, in copy mode too: the copy cursor stays. The
//! steps (all but `scroll-to-top`, `scroll-to-bottom` and `pan-home`) repeat while the button
//! is held on their block, until the pointer leaves it. The wheel over a block that pans pans
//! the history. A custom scroll block:
//! `[▲](click=page-up)[{scroll_percent:>3}%](muted)[▼](click=page-down)`
//!
//! ## Examples
//! ```toml
//! [ui.status_bar]
//! # Solid pill mode indicator that lights up with background when prefix is pressed:
//! left = "[ {mode} ](prefix?(bg=main fg=black bold):copy?(bg=magenta fg=white bold):search?(bg=good fg=black bold):(fg=main bold)){port} | [{baud}](secondary) {framing}"
//!
//! # Center scroll indicator and pan arrows with warning states:
//! center = "[{pan_left} ](pan_left?warn:hide)[{scroll_pos}](has_scroll?muted:hide)[ {pan_right}](pan_right_alert?(bg=warn fg=black bold):(pan_right?warn:hide))"
//!
//! # Right-aligned status with a red badge for recording (a click stops it) and a help button:
//! right = "[ ● REC ](rec?(bg=warn fg=white bold):hide click=record) [{scroll_pos_up}](has_scroll?muted:hide) [RX: {rx_bytes}](muted) | [TX: {tx_bytes}](muted) | [{time}](secondary) [?](muted click=help)"
//! ```

use std::borrow::Cow;
use std::fmt::Write as _;

use unicode_width::UnicodeWidthStr;

use ratatui::{
    Frame,
    layout::{Alignment, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    app::{
        App, InputContext,
        selection::CopySide,
        types::{ClickAction, ClickTarget},
    },
    configs::{KeyBindingAction, Mode, StatusBarConfig, StatusBarLayout, ThemeConfig},
    link::LinkKind,
    ui::common::{pan_left_indicator, pan_right_indicator},
};

pub struct CompiledStatusBar {
    left: CompiledTemplate,
    center: CompiledTemplate,
    right: CompiledTemplate,
    time_format: String,
    hover: Option<Style>,
    prios: Vec<u8>,
    shows_pan: bool,
}

impl CompiledStatusBar {
    pub fn new(config: &StatusBarConfig, theme: &ThemeConfig) -> Self {
        let left = CompiledTemplate::new(&config.left, theme);
        let center = CompiledTemplate::new(&config.center, theme);
        let right = CompiledTemplate::new(&config.right, theme);
        let mut prios: Vec<u8> = [&left, &center, &right]
            .iter()
            .flat_map(|template| &template.segments)
            .filter_map(|segment| match segment {
                Segment::Styled { prio, .. } => *prio,
                Segment::Plain(_) => None,
            })
            .collect();
        prios.sort_unstable_by(|a, b| b.cmp(a));
        prios.dedup();
        let shows_pan = [&left, &center, &right]
            .iter()
            .flat_map(|template| &template.segments)
            .flat_map(|segment| match segment {
                Segment::Plain(parts) | Segment::Styled { text: parts, .. } => parts,
            })
            .any(|part| {
                matches!(
                    part,
                    TextPart::Var(
                        Var::PanLeft | Var::PanRight | Var::PanOffset | Var::PanMax,
                        _
                    )
                )
            });
        Self {
            left,
            center,
            right,
            time_format: config.time_format.clone(),
            hover: compile_hover(&config.hover, theme),
            prios,
            shows_pan,
        }
    }

    /// The bar has a pan variable: drawn, it is where the pan shows, not the history's border
    /// (MAX-139)
    pub fn shows_pan(&self) -> bool {
        self.shows_pan
    }
}

/// How hard the bar is squeezed to fit (MAX-139): the variables in their compact form, and the
/// blocks with a `prio=` of `drop_from` or more left out
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Fit {
    compact: bool,
    drop_from: Option<u8>,
}

/// A block's own `hover=`: `None` takes the bar's `hover` option, `Some(None)` is no hover
type HoverOverride = Option<Option<Style>>;

/// Where a clickable block is in its line (start column, width), what a click runs, and its
/// own hover style
type ClickSpot = (u16, u16, ClickAction, HoverOverride);

/// A hover style from style words, a group in parentheses too; `None` for `none` or `hide`
/// (MAX-131)
fn compile_hover(words: &str, theme: &ThemeConfig) -> Option<Style> {
    let words = words.trim();
    let words = words
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
        .unwrap_or(words);
    let mut style = Style::default();
    for word in compile_style_words(words, theme) {
        style = match word {
            StyleWord::Hide => return None,
            StyleWord::Modifier(modifier) => style.add_modifier(modifier),
            StyleWord::Reset => Style::default(),
            StyleWord::Fg(color) => style.fg(color),
            StyleWord::Bg(color) => style.bg(color),
        };
    }
    Some(style)
}

pub struct CompiledTemplate {
    segments: Vec<Segment>,
}

enum Segment {
    /// Text in the base style
    Plain(Vec<TextPart>),
    /// `[text](style_spec)`, omitted when its style resolves to `hide` or its text is empty.
    /// A click on it runs `click`: its `click=<action>`, else what its first variable implies.
    /// Under the pointer it takes its `hover=`, else the bar's. With a `prio=N` it is left out
    /// when the bar doesn't fit, the highest `N` first; without, never
    Styled {
        text: Vec<TextPart>,
        style: Vec<StyleExpr>,
        click: Option<ClickAction>,
        hover: HoverOverride,
        prio: Option<u8>,
    },
}

enum TextPart {
    Literal(String),
    Var(Var, VarFormat),
}

/// A variable's `{name:spec}` (MAX-139): `[<>^][0][width][h]`. The width pads the value to that
/// many columns, aligned by `<`, `>` or `^` (numbers right and text left by default), with
/// zeros for a number with `0`; `h` writes a number as a human size. A longer value is never
/// cut
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct VarFormat {
    align: Option<Align>,
    zero: bool,
    width: u8,
    human: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Align {
    Left,
    Right,
    Center,
}

impl VarFormat {
    /// `None` for a malformed spec, which leaves `{name:spec}` as written
    fn parse(spec: &str) -> Option<Self> {
        let (align, rest) = match spec.chars().next()? {
            '<' => (Some(Align::Left), &spec[1..]),
            '>' => (Some(Align::Right), &spec[1..]),
            '^' => (Some(Align::Center), &spec[1..]),
            _ => (None, spec),
        };
        let (human, rest) = match rest.strip_suffix('h') {
            Some(rest) => (true, rest),
            None => (false, rest),
        };
        let zero = rest.len() > 1 && rest.starts_with('0');
        if !rest.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let width = if rest.is_empty() {
            0
        } else {
            rest.parse().ok()?
        };
        Some(Self {
            align,
            zero,
            width,
            human,
        })
    }

    /// Pads the value written in `out` from `start`
    fn pad(self, out: &mut String, start: usize, number: bool) {
        let fill = (self.width as usize).saturating_sub(out[start..].width());
        if fill == 0 {
            return;
        }
        if self.zero && number {
            out.insert_str(start, &"0".repeat(fill));
            return;
        }
        let align = self
            .align
            .unwrap_or(if number { Align::Right } else { Align::Left });
        let left = match align {
            Align::Left => 0,
            Align::Right => fill,
            Align::Center => fill / 2,
        };
        out.insert_str(start, &" ".repeat(left));
        out.push_str(&" ".repeat(fill - left));
    }
}

/// What the replay does (MAX-166): `END` once it read its whole file or was stopped, `PAUSE`
/// while paused, else `RUN`
fn replay_state(app: &App) -> &'static str {
    if app.is_replay_ended() {
        "END"
    } else if app.replay().is_some_and(|replay| replay.is_paused()) {
        "PAUSE"
    } else {
        "RUN"
    }
}

/// A byte count like `ls -h`: up to 999 as is, then `1.0K`..`9.9K`, `10K`..`999K`, `1.0M`, ...,
/// at most 4 columns (base 1024)
pub fn write_human(out: &mut String, bytes: u64) {
    if bytes < 1000 {
        let _ = write!(out, "{bytes}");
        return;
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 999.5 && unit < 6 {
        value /= 1024.0;
        unit += 1;
    }
    let suffix = ["", "K", "M", "G", "T", "P", "E"][unit];
    let _ = if value < 9.95 {
        write!(out, "{value:.1}{suffix}")
    } else {
        write!(out, "{value:.0}{suffix}")
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Var {
    Mode,
    Port,
    Baud,
    Framing,
    RxBytes,
    TxBytes,
    Time,
    PanLeft,
    PanRight,
    PanOffset,
    PanMax,
    ScrollPos,
    ScrollPosUp,
    ScrollPercent,
    ScrollOffset,
    ScrollMax,
    CopySide,
    SendFile,
    SendPercent,
    TriggerCount,
    TriggerNames,
    ReplayFile,
    ReplayState,
    ReplayPercent,
    ReplayRead,
    ReplaySize,
}

impl Var {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "mode" => Self::Mode,
            "port" => Self::Port,
            "baud" => Self::Baud,
            "framing" => Self::Framing,
            "rx_bytes" => Self::RxBytes,
            "tx_bytes" => Self::TxBytes,
            "time" => Self::Time,
            "pan_left" => Self::PanLeft,
            "pan_right" => Self::PanRight,
            "pan_offset" => Self::PanOffset,
            "pan_max" => Self::PanMax,
            "scroll_pos" => Self::ScrollPos,
            "scroll_pos_up" => Self::ScrollPosUp,
            "scroll_percent" => Self::ScrollPercent,
            "scroll_offset" => Self::ScrollOffset,
            "scroll_max" => Self::ScrollMax,
            "copy_side" => Self::CopySide,
            "send_file" => Self::SendFile,
            "send_percent" => Self::SendPercent,
            "trigger_count" => Self::TriggerCount,
            "trigger_names" => Self::TriggerNames,
            "replay_file" => Self::ReplayFile,
            "replay_state" => Self::ReplayState,
            "replay_percent" => Self::ReplayPercent,
            "replay_read" => Self::ReplayRead,
            "replay_size" => Self::ReplaySize,
            _ => return None,
        })
    }

    /// What a click on the variable runs (MAX-125)
    fn click(self) -> Option<ClickAction> {
        let action = match self {
            Self::Mode => KeyBindingAction::SKInputMode,
            Self::Baud | Self::Framing => KeyBindingAction::LinkSettings,
            Self::PanLeft => KeyBindingAction::PanLeft,
            Self::PanRight => KeyBindingAction::PanRight,
            Self::PanOffset | Self::PanMax => KeyBindingAction::PanHome,
            Self::ScrollPos | Self::ScrollPosUp | Self::ScrollPercent | Self::ScrollOffset => {
                KeyBindingAction::ScrollToBottom
            }
            Self::CopySide => return Some(ClickAction::ToggleSide),
            Self::SendFile | Self::SendPercent => KeyBindingAction::CancelCommands,
            Self::TriggerCount | Self::TriggerNames => KeyBindingAction::Triggers,
            Self::Port
            | Self::RxBytes
            | Self::TxBytes
            | Self::Time
            | Self::ScrollMax
            | Self::ReplayFile
            | Self::ReplayState
            | Self::ReplayPercent
            | Self::ReplayRead
            | Self::ReplaySize => return None,
        };
        Some(ClickAction::Run(action))
    }

    /// The value of a number variable, which a format pads right or with zeros, or writes as a
    /// human size
    fn number(self, app: &App) -> Option<u64> {
        let scroll = app.scroll_status();
        Some(match self {
            // a TCP link or a plain file has none: written as text by `write_text`
            Self::Baud => app.line_settings()?.baud as u64,
            Self::RxBytes => app.stats.rx() as u64,
            Self::TxBytes => app.stats.tx() as u64,
            Self::ScrollPercent => scroll.percent() as u64,
            Self::ScrollOffset => scroll.offset_from_bottom as u64,
            Self::ScrollMax => scroll.max_scroll as u64,
            Self::SendPercent => app.transfer().map_or(0, |transfer| transfer.percent()),
            Self::TriggerCount => app.armed_triggers().len() as u64,
            Self::ReplayPercent => app.replay().map_or(0, |replay| replay.percent()),
            Self::ReplayRead => app.replay().map_or(0, |replay| replay.progress().0),
            Self::ReplaySize => app.replay().map_or(0, |replay| replay.progress().1),
            Self::PanOffset => app.pan_left_status().unwrap_or(0) as u64,
            Self::PanMax => app
                .pan_right_status()
                .map_or(0, |(cols, _)| cols + app.pan_left_status().unwrap_or(0))
                as u64,
            _ => return None,
        })
    }

    /// Writes the value in its format; `compact`, when the bar doesn't fit, writes a variable
    /// without a format of its own in its short form (MAX-139): the port's file name, the byte
    /// counts as sizes, the pan without its spaces and parentheses
    fn write(
        self,
        format: VarFormat,
        compact: bool,
        out: &mut String,
        app: &App,
        time_format: &str,
    ) {
        let start = out.len();
        let compact = compact && format == VarFormat::default();
        let human = format.human || (compact && matches!(self, Self::RxBytes | Self::TxBytes));
        let number = self.number(app);
        match number {
            Some(value) if human => write_human(out, value),
            Some(value) => {
                let _ = write!(out, "{value}");
            }
            // a saved link by its name, short already (MAX-185)
            None if compact && self == Self::Port => match app.link_name() {
                Some(name) => out.push_str(name),
                None => out.push_str(&app.link_config().address.tag()),
            },
            // the first name, then how many more
            None if compact && self == Self::TriggerNames => {
                let armed = app.armed_triggers();
                if let Some((first, more)) = armed.split_first() {
                    out.push_str(first);
                    if !more.is_empty() {
                        let _ = write!(out, " +{}", more.len());
                    }
                }
            }
            None if compact && matches!(self, Self::PanLeft | Self::PanRight) => {
                let mut full = String::new();
                self.write_text(&mut full, app, time_format);
                out.extend(full.chars().filter(|c| !matches!(c, ' ' | '(' | ')')));
            }
            None => self.write_text(out, app, time_format),
        }
        format.pad(out, start, number.is_some());
    }

    fn write_text(self, out: &mut String, app: &App, time_format: &str) {
        let scroll = app.scroll_status();
        let _ = match self {
            Self::Mode => write!(
                out,
                "{}",
                match app.settings().mode {
                    Mode::Terminal => "TERM",
                    Mode::Line => "LINE",
                }
            ),
            Self::Port => match app.link_name() {
                Some(name) => write!(out, "{name}"),
                None => write!(out, "{}", app.link_config().address.label()),
            },
            // a port's, or a replayed session's from its meta (MAX-166)
            Self::Framing if let Some(line) = app.line_settings() => {
                write!(out, "{}", line.framing)
            }
            // a replayed file has none
            Self::Baud if app.link_kind() == LinkKind::Replay => {
                write!(out, "replay")
            }
            Self::Framing if app.link_kind() == LinkKind::Replay => {
                write!(out, "{}", replay_state(app))
            }
            // the bridge sets them: what the stream is and the link kind, read as "raw TCP"
            Self::Baud | Self::Framing
                if let Some([baud, framing]) = app.link_kind().line_stand_in() =>
            {
                write!(out, "{}", if self == Self::Baud { baud } else { framing })
            }
            // a kind with neither, none yet
            Self::Framing => Ok(()),
            Self::Time => write!(out, "{}", chrono::Local::now().format(time_format)),
            Self::PanLeft => write!(out, "{}", pan_left_indicator(app).unwrap_or_default()),
            Self::PanRight => write!(out, "{}", pan_right_indicator(app).unwrap_or_default()),
            Self::ScrollPos => write!(out, "{}", scroll.position_display()),
            Self::ScrollPosUp => write!(out, "{}", scroll.position_display().to_uppercase()),
            Self::CopySide => write!(
                out,
                "{}",
                match app.copy_side() {
                    Some(CopySide::Text) => "TXT",
                    Some(CopySide::Raw) => "RAW",
                    None => "",
                }
            ),
            Self::SendFile => write!(
                out,
                "{}",
                app.transfer().map_or("", |transfer| transfer.name())
            ),
            Self::TriggerNames => write!(out, "{}", app.armed_triggers().join(", ")),
            Self::ReplayFile if app.link_kind() == LinkKind::Replay => {
                write!(out, "{}", app.link_config().address.tag())
            }
            Self::ReplayState if app.link_kind() == LinkKind::Replay => {
                write!(out, "{}", replay_state(app))
            }
            Self::ReplayFile | Self::ReplayState => Ok(()),
            // the numbers, written by `write`
            Self::Baud
            | Self::RxBytes
            | Self::TxBytes
            | Self::ScrollPercent
            | Self::ScrollOffset
            | Self::ScrollMax
            | Self::PanOffset
            | Self::PanMax
            | Self::SendPercent
            | Self::TriggerCount
            | Self::ReplayPercent
            | Self::ReplayRead
            | Self::ReplaySize => Ok(()),
        };
    }
}

/// One whitespace-separated token of a style spec, possibly a `cond?a:b` ternary
enum StyleExpr {
    Words(Vec<StyleWord>),
    Ternary {
        cond: Cond,
        then: Box<StyleExpr>,
        otherwise: Box<StyleExpr>,
    },
}

enum StyleWord {
    Hide,
    Modifier(Modifier),
    Reset,
    Fg(Color),
    Bg(Color),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cond {
    Prefix,
    Copy,
    CopyRaw,
    Search,
    Line,
    Term,
    Overlay,
    Rec,
    Session,
    Focused,
    IsLive,
    IsTop,
    IsScrolled,
    ScrollUp,
    ScrollDown,
    HasScroll,
    PanLeft,
    PanRight,
    PanRightAlert,
    Panned,
    Wide,
    NewData,
    Mouse,
    LineSettings,
    Sending,
    Armed,
    Mashing,
    Replaying,
    AddCr,
    AddLf,
    /// Unknown condition name, always false
    Unknown,
}

impl Cond {
    fn from_name(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "prefix" => Self::Prefix,
            "copy" | "copymode" => Self::Copy,
            "copy_raw" | "raw_cursor" => Self::CopyRaw,
            "search" => Self::Search,
            "line" | "linemode" => Self::Line,
            "term" | "terminal" => Self::Term,
            "overlay" | "popup" => Self::Overlay,
            "rec" | "logging" => Self::Rec,
            "session" | "backup" => Self::Session,
            "focused" => Self::Focused,
            "is_live" | "live" | "is_bottom" | "bottom" | "bot" => Self::IsLive,
            "is_top" | "top" => Self::IsTop,
            "is_scrolled" | "scrolled" | "paused" => Self::IsScrolled,
            "scroll_up" | "can_scroll_up" => Self::ScrollUp,
            "scroll_down" | "can_scroll_down" => Self::ScrollDown,
            "has_scroll" => Self::HasScroll,
            "pan_left" | "can_pan_left" => Self::PanLeft,
            "pan_right" | "can_pan_right" => Self::PanRight,
            "pan_right_alert" | "pan_alert" => Self::PanRightAlert,
            "panned" | "pan" | "panning" => Self::Panned,
            "wide" | "pannable" => Self::Wide,
            "new_data" | "unread" | "missed" => Self::NewData,
            "mouse" => Self::Mouse,
            "line_settings" | "serial" => Self::LineSettings,
            "sending" | "send_file" => Self::Sending,
            "armed" | "trigger" => Self::Armed,
            "mashing" | "mash" => Self::Mashing,
            "replaying" | "replay" => Self::Replaying,
            "add_cr" => Self::AddCr,
            "add_lf" => Self::AddLf,
            _ => Self::Unknown,
        }
    }

    fn eval(self, app: &App) -> bool {
        let ctx = app.input_context();
        let scroll = app.scroll_status();
        match self {
            Self::Prefix => ctx == InputContext::Prefix || app.is_prefix_active(),
            Self::Copy => ctx == InputContext::CopyMode || app.is_copy_mode_active(),
            Self::CopyRaw => app.copy_side() == Some(CopySide::Raw),
            Self::Search => ctx == InputContext::Search || app.is_search_active(),
            Self::Line => ctx == InputContext::LineInput,
            Self::Term => ctx == InputContext::Terminal,
            Self::Overlay => ctx == InputContext::Overlay || app.overlay().is_some(),
            Self::Rec => app.recording().is_some(),
            Self::Session => app.has_session(),
            Self::Focused => app.is_app_focused(),
            Self::IsLive => scroll.is_bottom(),
            Self::IsTop => scroll.is_top(),
            Self::IsScrolled => !scroll.is_bottom(),
            Self::ScrollUp => scroll.can_scroll_up(),
            Self::ScrollDown => scroll.can_scroll_down(),
            Self::HasScroll => scroll.max_scroll > 0,
            Self::PanLeft => app.pan_left_status().is_some_and(|cols| cols > 0),
            Self::PanRight => app.pan_right_status().is_some_and(|(cols, _)| cols > 0),
            // only meaningful in line mode
            Self::PanRightAlert => {
                app.settings().mode == Mode::Line
                    && app.pan_right_status().is_some_and(|(cols, explored)| {
                        cols > explored.saturating_sub(app.line_mode.x_offset())
                    })
            }
            Self::Panned => app.pan_left_status().is_some_and(|c| c > 0),
            Self::Wide => app.pan_right_status().is_some(),
            Self::NewData => app.is_new_content_missed(),
            Self::Mouse => app.settings().ui.mouse,
            Self::LineSettings => app.line_settings().is_some(),
            Self::Sending => app.transfer().is_some(),
            Self::Armed => app.triggers().iter().any(|trigger| trigger.is_armed()),
            Self::Mashing => app.mash().is_some(),
            Self::Replaying => app.link_kind() == LinkKind::Replay,
            Self::AddCr => app.settings().add_cr,
            Self::AddLf => app.settings().add_lf,
            Self::Unknown => false,
        }
    }
}

impl CompiledTemplate {
    pub fn new(template: &str, theme: &ThemeConfig) -> Self {
        let template = &*join_lines(template);
        let mut segments = Vec::new();
        let mut cursor = 0;

        while cursor < template.len() {
            let Some(open_bracket) = template[cursor..].find('[') else {
                segments.push(Segment::Plain(parse_text_parts(&template[cursor..])));
                break;
            };
            let text_start = cursor + open_bracket + 1;

            if open_bracket > 0 {
                segments.push(Segment::Plain(parse_text_parts(
                    &template[cursor..cursor + open_bracket],
                )));
            }

            if let Some(close_bracket) = template[text_start..].find("](") {
                let text_end = text_start + close_bracket;
                let style_start = text_end + 2;

                let mut depth = 1;
                let mut close_paren = None;
                for (i, ch) in template[style_start..].char_indices() {
                    match ch {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 {
                                close_paren = Some(i);
                                break;
                            }
                        }
                        _ => {}
                    }
                }

                if let Some(paren_idx) = close_paren {
                    let style_end = style_start + paren_idx;
                    let text = parse_text_parts(&template[text_start..text_end]);
                    // `click=<action>`, `hover=<style>` and `prio=N` are no style: an unknown
                    // action or a bad number is dropped like an unknown style word
                    let (mouse, style): (Vec<&str>, Vec<&str>) =
                        tokenize_spec(&template[style_start..style_end])
                            .into_iter()
                            .partition(|token| {
                                ["click=", "hover=", "prio="]
                                    .iter()
                                    .any(|keyword| token.starts_with(keyword))
                            });
                    let prio = mouse
                        .iter()
                        .find_map(|token| token.strip_prefix("prio="))
                        .and_then(|n| n.parse().ok());
                    let clicks = mouse
                        .iter()
                        .filter_map(|token| token.strip_prefix("click="));
                    let hover = mouse
                        .iter()
                        .find_map(|token| token.strip_prefix("hover="))
                        .map(|words| compile_hover(words, theme));
                    let click = clicks
                        .into_iter()
                        .find_map(|action| action.parse().ok())
                        .map(ClickAction::Run)
                        .or_else(|| {
                            text.iter().find_map(|part| match part {
                                TextPart::Var(var, _) => var.click(),
                                TextPart::Literal(_) => None,
                            })
                        });
                    segments.push(Segment::Styled {
                        text,
                        style: style
                            .into_iter()
                            .map(|token| compile_expression(token.trim(), theme))
                            .collect(),
                        click,
                        hover,
                        prio,
                    });
                    cursor = style_end + 1;
                    continue;
                }
            }

            segments.push(Segment::Plain(vec![TextPart::Literal("[".to_string())]));
            cursor = text_start;
        }

        Self { segments }
    }

    #[cfg(test)]
    pub fn spans(&self, app: &App, base_style: Style, time_format: &str) -> Vec<Span<'_>> {
        self.spans_and_clicks(app, base_style, time_format, Fit::default())
            .0
    }

    /// The spans, and where the clickable ones are in the line (start column, width) with what
    /// a click runs (MAX-125). In plain text each variable is clickable on its own, not the
    /// text around it
    fn spans_and_clicks(
        &self,
        app: &App,
        base_style: Style,
        time_format: &str,
        fit: Fit,
    ) -> (Vec<Span<'_>>, Vec<ClickSpot>) {
        let mut spans = Vec::with_capacity(self.segments.len());
        let mut clicks = Vec::new();
        let mut col: u16 = 0;
        for segment in &self.segments {
            match segment {
                Segment::Plain(parts) => {
                    let mut part_col = col;
                    for part in parts {
                        let width = match part {
                            TextPart::Literal(text) => text.width(),
                            TextPart::Var(var, format) => {
                                let mut out = String::new();
                                var.write(*format, fit.compact, &mut out, app, time_format);
                                out.width()
                            }
                        } as u16;
                        if let TextPart::Var(var, _) = part
                            && let Some(click) = var.click()
                            && width > 0
                        {
                            clicks.push((part_col, width, click, None));
                        }
                        part_col = part_col.saturating_add(width);
                    }
                    let span =
                        Span::styled(fill_text(parts, app, time_format, fit.compact), base_style);
                    col = col.saturating_add(span.width() as u16);
                    spans.push(span);
                }
                Segment::Styled {
                    text,
                    style,
                    click,
                    hover,
                    prio,
                } => {
                    if prio
                        .zip(fit.drop_from)
                        .is_some_and(|(prio, from)| prio >= from)
                    {
                        continue;
                    }
                    let Some(span_style) = resolve_style(style, base_style, app) else {
                        continue;
                    };
                    let content = fill_text(text, app, time_format, fit.compact);
                    if !content.is_empty() {
                        let span = Span::styled(content, span_style);
                        let width = span.width() as u16;
                        if let Some(click) = *click {
                            clicks.push((col, width, click, *hover));
                        }
                        col = col.saturating_add(width);
                        spans.push(span);
                    }
                }
            }
        }
        (spans, clicks)
    }
}

/// A template written on several lines (MAX-163): `\` at the end of a line joins the next one,
/// its indentation dropped, as TOML does in `"""` strings and doesn't in `'''` ones. Any other
/// `\` stays
fn join_lines(template: &str) -> Cow<'_, str> {
    if !template.contains('\\') {
        return Cow::Borrowed(template);
    }
    let mut joined = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(at) = rest.find('\\') {
        joined.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        match after
            .strip_prefix("\r\n")
            .or_else(|| after.strip_prefix('\n'))
        {
            Some(next) => rest = next.trim_start_matches([' ', '\t']),
            None => {
                joined.push('\\');
                rest = after;
            }
        }
    }
    joined.push_str(rest);
    Cow::Owned(joined)
}

/// Splits text into literals and known `{variables}`, with their `{name:spec}` format; an
/// unknown `{name}` or a malformed spec stays as written
fn parse_text_parts(text: &str) -> Vec<TextPart> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut rest = text;

    while let Some(open) = rest.find('{') {
        literal.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let var = after.find('}').and_then(|close| {
            let (name, spec) = match after[..close].split_once(':') {
                Some((name, spec)) => (name, Some(spec)),
                None => (&after[..close], None),
            };
            let format = match spec {
                Some(spec) => VarFormat::parse(spec)?,
                None => VarFormat::default(),
            };
            Some((close, Var::from_name(name)?, format))
        });
        match var {
            Some((close, var, format)) => {
                if !literal.is_empty() {
                    parts.push(TextPart::Literal(std::mem::take(&mut literal)));
                }
                parts.push(TextPart::Var(var, format));
                rest = &after[close + 1..];
            }
            None => {
                literal.push('{');
                rest = after;
            }
        }
    }
    literal.push_str(rest);
    if !literal.is_empty() {
        parts.push(TextPart::Literal(literal));
    }
    parts
}

/// Borrows the text when it is a single literal, builds it only when variables are involved
fn fill_text<'a>(
    parts: &'a [TextPart],
    app: &App,
    time_format: &str,
    compact: bool,
) -> Cow<'a, str> {
    match parts {
        [] => Cow::Borrowed(""),
        [TextPart::Literal(text)] => Cow::Borrowed(text),
        _ => {
            let mut out = String::new();
            for part in parts {
                match part {
                    TextPart::Literal(text) => out.push_str(text),
                    TextPart::Var(var, format) => {
                        var.write(*format, compact, &mut out, app, time_format)
                    }
                }
            }
            Cow::Owned(out)
        }
    }
}

/// None when the style resolves to `hide`
fn resolve_style(exprs: &[StyleExpr], base_style: Style, app: &App) -> Option<Style> {
    let mut style = base_style;
    for expr in exprs {
        for word in expr.resolve(app) {
            style = match *word {
                StyleWord::Hide => return None,
                StyleWord::Modifier(modifier) => style.add_modifier(modifier),
                StyleWord::Reset => base_style,
                StyleWord::Fg(color) => style.fg(color),
                StyleWord::Bg(color) => style.bg(color),
            };
        }
    }
    Some(style)
}

impl StyleExpr {
    fn resolve(&self, app: &App) -> &[StyleWord] {
        match self {
            Self::Words(words) => words,
            Self::Ternary {
                cond,
                then,
                otherwise,
            } => {
                if cond.eval(app) {
                    then.resolve(app)
                } else {
                    otherwise.resolve(app)
                }
            }
        }
    }
}

/// First removes top level parentheses, then splits on the top level '?' / ':' into a ternary.
/// Without a matching ':' the whole expression is read as style words
fn compile_expression(expr: &str, theme: &ThemeConfig) -> StyleExpr {
    let expr = expr.trim();

    if expr.starts_with('(') && expr.ends_with(')') {
        let mut depth = 0;
        let mut fully_enclosed = true;
        for (i, ch) in expr.char_indices() {
            if ch == '(' {
                depth += 1;
            } else if ch == ')' {
                depth -= 1;
                if depth == 0 && i < expr.len() - 1 {
                    fully_enclosed = false;
                    break;
                }
            }
        }
        if fully_enclosed {
            return compile_expression(&expr[1..expr.len() - 1], theme);
        }
    }

    // first top-level '?' (depth == 0)
    let mut depth: i32 = 0;
    let mut question_pos = None;
    for (i, ch) in expr.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '?' if depth == 0 => {
                question_pos = Some(i);
                break;
            }
            _ => {}
        }
    }

    let Some(q_idx) = question_pos else {
        return StyleExpr::Words(compile_style_words(expr, theme));
    };

    let condition = &expr[..q_idx];
    let remainder = &expr[q_idx + 1..];

    depth = 0;
    let mut colon_pos = None;
    for (i, ch) in remainder.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '?' => depth += 1,
            ':' => {
                if depth == 0 {
                    colon_pos = Some(i);
                    break;
                } else {
                    depth -= 1;
                }
            }
            _ => {}
        }
    }

    let Some(c_idx) = colon_pos else {
        // no ':', malformed: read as words like before
        return StyleExpr::Words(compile_style_words(expr, theme));
    };

    StyleExpr::Ternary {
        cond: Cond::from_name(condition),
        then: Box::new(compile_expression(&remainder[..c_idx], theme)),
        otherwise: Box::new(compile_expression(&remainder[c_idx + 1..], theme)),
    }
}

/// Unknown words are dropped (they never changed the style)
fn compile_style_words(resolved: &str, theme: &ThemeConfig) -> Vec<StyleWord> {
    let mut words = Vec::new();
    for part in resolved.split_whitespace() {
        let word = match part.to_ascii_lowercase().as_str() {
            "hide" | "hidden" | "none" => Some(StyleWord::Hide),
            "bold" => Some(StyleWord::Modifier(Modifier::BOLD)),
            "dim" => Some(StyleWord::Modifier(Modifier::DIM)),
            "italic" => Some(StyleWord::Modifier(Modifier::ITALIC)),
            "crossed" => Some(StyleWord::Modifier(Modifier::CROSSED_OUT)),
            "slow_blink" | "sblink" => Some(StyleWord::Modifier(Modifier::SLOW_BLINK)),
            "rapid_blink" | "rblink" => Some(StyleWord::Modifier(Modifier::RAPID_BLINK)),
            "underlined" | "underline" => Some(StyleWord::Modifier(Modifier::UNDERLINED)),
            "reversed" | "reverse" => Some(StyleWord::Modifier(Modifier::REVERSED)),
            "reset" | "default" => Some(StyleWord::Reset),
            "bg=none" | "bg=reset" => Some(StyleWord::Bg(Color::Reset)),
            "fg=none" | "fg=reset" => Some(StyleWord::Fg(Color::Reset)),
            _ => {
                if let Some(c) = part.strip_prefix("fg=") {
                    resolve_color(c, theme).map(StyleWord::Fg)
                } else if let Some(c) = part.strip_prefix("bg=") {
                    resolve_color(c, theme).map(StyleWord::Bg)
                } else {
                    resolve_color(part, theme).map(StyleWord::Fg)
                }
            }
        };
        words.extend(word);
    }
    words
}

fn resolve_color(name: &str, theme: &ThemeConfig) -> Option<Color> {
    match name.to_ascii_lowercase().as_str() {
        "main" | "primary" => Some(theme.palette.main),
        "secondary" => Some(theme.palette.secondary),
        "ternary" => Some(theme.palette.ternary),
        "text" => Some(theme.palette.text),
        "muted" => Some(theme.palette.text_muted),
        "warn" | "warning" => Some(theme.palette.text_warn),
        "good" | "ok" | "success" => Some(theme.palette.text_good),
        "unselected" => Some(theme.palette.unselected),
        _ => name.parse::<Color>().ok(),
    }
}

fn tokenize_spec(spec: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut paren_depth: i32 = 0;
    let mut ternary_depth: i32 = 0;
    let mut start = 0;

    for (i, ch) in spec.char_indices() {
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '?' if paren_depth == 0 => ternary_depth += 1,
            ':' if paren_depth == 0 => ternary_depth = ternary_depth.saturating_sub(1),
            ' ' | '\t' if paren_depth == 0 && ternary_depth == 0 => {
                let token = spec[start..i].trim();
                if !token.is_empty() {
                    tokens.push(token);
                }
                start = i + 1;
            }
            _ => {}
        }
    }

    let tail = spec[start..].trim();
    if !tail.is_empty() {
        tokens.push(tail);
    }

    tokens
}

/// Draws the status bar; returns its clickable blocks with their hover style (MAX-131), the
/// left part's first, then the center's, then the right's, as they are drawn over each other
/// (MAX-125)
/// A part of the bar as drawn: its spans, and its clickable blocks
type DrawnPart<'a> = (Vec<Span<'a>>, Vec<ClickSpot>);

/// The widths of the left, center and right parts
fn widths(parts: &[Option<DrawnPart>; 3]) -> [u16; 3] {
    parts.each_ref().map(|part| {
        part.as_ref().map_or(0, |(spans, _)| {
            spans.iter().map(Span::width).sum::<usize>() as u16
        })
    })
}

/// Where the center part starts in `room` columns: in the middle of the room the left and
/// right parts leave, a column apart from each (MAX-163). None when it doesn't fit there
fn center_start(room: u16, [left, center, right]: [u16; 3]) -> Option<u16> {
    let [room, left, center, right] = [room, left, center, right].map(u32::from);
    let gap = |a: u32, b: u32| u32::from(a > 0 && b > 0);
    let from = left + gap(left, center);
    let to = room.checked_sub(right + gap(center, right))?;
    let free = to.checked_sub(from)?.checked_sub(center)?;
    u16::try_from(from + free / 2).ok()
}

/// Whether the parts fit in `room` columns as they are drawn, the left one at the start, the
/// right one at the end and the center one between them, a column apart (MAX-139)
fn fits(room: u16, widths @ [left, center, right]: [u16; 3]) -> bool {
    if center == 0 {
        let gap = u32::from(left > 0 && right > 0);
        return u32::from(left) + gap + u32::from(right) <= u32::from(room);
    }
    center_start(room, widths).is_some()
}

pub fn render_status_bar<'a>(
    app: &'a App,
    frame: &mut Frame,
    area: Rect,
    theme: &ThemeConfig,
    status_bar: &'a CompiledStatusBar,
) -> Vec<ClickTarget> {
    let mut targets = Vec::new();
    let over_view = app.status_bar_over_view();
    if !app.show_status_bar() && !over_view {
        return targets;
    }
    // the view's row it covers, cleared so no text shows through (MAX-181)
    if over_view {
        frame.render_widget(Clear, area);
    }

    // only the band layout fills the row with its colour; the others put the text on the UI
    // background (MAX-135)
    let layout = app.status_bar_layout();
    let mut base_style = *theme.ui.status_bar;
    if layout != StatusBarLayout::Band {
        base_style.bg = theme.ui.background.bg;
    }
    let in_border = layout == StatusBarLayout::Border;
    let mut parts_area = area;
    if layout == StatusBarLayout::Box {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(*theme.ui.border_panel)
            .style(base_style);
        parts_area = block.inner(area);
        frame.render_widget(block, area);
    } else if !in_border {
        // with borders, inset one column each side to line up with the boxes' edges
        if app.show_borders() {
            parts_area = area.inner(Margin::new(1, 0));
        }
        frame.render_widget(Block::default().style(base_style), area);
    }

    let time_format = &status_bar.time_format;
    // a part as drawn: none when empty or blank (it would take room, and cut the `─` of the
    // border), and in the border a space keeps the text off the dashes unless the part has one
    // there
    let part = |template: &'a CompiledTemplate, fit: Fit| -> Option<DrawnPart<'a>> {
        let (mut spans, mut clicks) = template.spans_and_clicks(app, base_style, time_format, fit);
        let blank = spans.iter().all(|span| span.content.trim().is_empty());
        if blank {
            return None;
        }
        if in_border {
            let space = || Span::styled(" ", base_style);
            if !spans[0].content.starts_with(char::is_whitespace) {
                spans.insert(0, space());
                for click in &mut clicks {
                    click.0 += 1;
                }
            }
            if !spans[spans.len() - 1]
                .content
                .ends_with(char::is_whitespace)
            {
                spans.push(space());
            }
        }
        Some((spans, clicks))
    };
    let templates = [&status_bar.left, &status_bar.center, &status_bar.right];
    let room = parts_area.width;

    // the full text, then the compact forms, then without the `prio=` levels from the highest:
    // the first that fits (MAX-139)
    let compact = |drop_from| Fit {
        compact: true,
        drop_from,
    };
    let tries = std::iter::once(Fit::default())
        .chain(std::iter::once(compact(None)))
        .chain(status_bar.prios.iter().map(|&prio| compact(Some(prio))));
    let mut parts = [None, None, None];
    for fit in tries {
        parts = templates.map(|template| part(template, fit));
        if fits(room, widths(&parts)) {
            break;
        }
    }
    // still too wide: the center goes, then the right part's blocks from its left side
    if !fits(room, widths(&parts)) {
        parts[1] = None;
        while !fits(room, widths(&parts)) {
            let Some((spans, clicks)) = &mut parts[2] else {
                break;
            };
            // the cut block's clicks go, the others move left with the part
            let cut = spans.remove(0).width() as u16;
            clicks.retain(|click| click.0 >= cut);
            for click in clicks.iter_mut() {
                click.0 -= cut;
            }
            if spans.is_empty() {
                parts[2] = None;
            }
        }
    }

    let center = center_start(room, widths(&parts)).unwrap_or(0);
    let alignments = [Alignment::Left, Alignment::Center, Alignment::Right];
    for (drawn, alignment) in parts.into_iter().zip(alignments) {
        let Some((spans, clicks)) = drawn else {
            continue;
        };
        let line = Line::from(spans);
        let width = line.width() as u16;
        let offset = match alignment {
            Alignment::Left => 0,
            Alignment::Center => center,
            Alignment::Right => room.saturating_sub(width),
        };
        let start = parts_area.x + offset;
        for (col, width, action, hover) in clicks {
            let cell = Rect::new(start.saturating_add(col), parts_area.y, width, 1)
                .intersection(parts_area);
            if !cell.is_empty() {
                targets.push(ClickTarget {
                    area: cell,
                    action,
                    hover: hover.unwrap_or(status_bar.hover),
                });
            }
        }
        let area = Rect {
            x: start,
            width: width.min(room.saturating_sub(offset)),
            ..parts_area
        };
        frame.render_widget(Paragraph::new(line), area);
    }
    targets
}

#[cfg(test)]
mod tests {
    //! Golden output of the status bar templates, locked before precompiling them (#10).

    use super::*;
    use crate::{
        configs::{KeyBindingsConfig, KeyResolver, SettingsConfig},
        inputs::{Action, AppAction, CopyModeAction},
        link::{Address, LinkConfig},
    };

    fn app(mode: Mode) -> App {
        let mut settings = SettingsConfig::default();
        settings.mode = mode;
        settings.ui.status_bar.time_format = "TIME".to_string();
        let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
        App::new(settings, resolver, LinkConfig::default())
    }

    /// One "text|fg|bg|modifiers" string per span, so expectations stay readable
    fn render(template: &str, app: &App) -> Vec<String> {
        let theme = ThemeConfig::default();
        let time_format = &app.settings().ui.status_bar.time_format;
        CompiledTemplate::new(template, &theme)
            .spans(app, *theme.ui.status_bar, time_format)
            .iter()
            .map(|s| {
                format!(
                    "{}|{:?}|{:?}|{:?}",
                    s.content, s.style.fg, s.style.bg, s.style.add_modifier
                )
            })
            .collect()
    }

    const BG: &str = "Some(Rgb(30, 35, 45))";
    const MUTED: &str = "Some(Rgb(140, 146, 158))";

    fn span(text: &str, fg: &str, bg: &str, mods: &str) -> String {
        format!("{text}|{fg}|{bg}|{mods}")
    }

    fn plain(text: &str) -> String {
        span(text, "Some(White)", BG, "NONE")
    }

    /// Default left template in terminal mode: mode pill, port, baud and framing styles.
    /// How: golden spans captured from the code before #10.
    #[test]
    fn default_left_terminal() {
        let a = app(Mode::Terminal);
        let d = StatusBarConfig::default();
        assert_eq!(
            render(&d.left, &a),
            [
                span(" TERM ", "Some(Cyan)", BG, "BOLD"),
                plain("/dev/ttyUSB0"),
                span(" |", MUTED, BG, "NONE"),
                plain(" "),
                span("115200", "Some(Cyan)", BG, "NONE"),
                plain(" "),
                plain("8N1"),
            ]
        );
    }

    /// MAX-156: a TCP link has no line settings: `{baud}` is `raw` and `{framing}` the link
    /// kind, in the same places and styles; `line_settings` can hide them.
    /// How: the default left template with a `tcp://` port, a padded baud, the condition.
    #[test]
    fn tcp_link_has_no_baud_nor_framing() {
        let mut a = app(Mode::Terminal);
        a.link_mut()
            .set_address("tcp://192.168.1.50:4000".parse::<Address>().unwrap());
        let d = StatusBarConfig::default();
        assert_eq!(
            render(&d.left, &a),
            [
                span(" TERM ", "Some(Cyan)", BG, "BOLD"),
                plain("tcp://192.168.1.50:4000"),
                span(" |", MUTED, BG, "NONE"),
                plain(" "),
                span("raw", "Some(Cyan)", BG, "NONE"),
                plain(" "),
                plain("TCP"),
            ]
        );
        assert_eq!(render("<{baud:5}>", &a), [plain("<raw  >")]);
        assert_eq!(render("[x](serial?bold:hide)", &a), Vec::<String>::new());
    }

    /// MAX-166: a replayed file has no line settings: `{port}` is `file://` and its name,
    /// `{baud}` is `replay` and `{framing}` its state, `RUN` then `END`, in the same places and
    /// styles; `line_settings` hides them.
    /// How: the default left template on a `file://` port, then after the end.
    #[test]
    fn replay_shows_its_file_and_state() {
        let mut a = app(Mode::Terminal);
        // as a replay starts: the replay state comes with the address (MAX-175)
        a.start_replay(std::path::PathBuf::from("/tmp/logs/boot.raw"), None);
        let d = StatusBarConfig::default();
        assert_eq!(
            render(&d.left, &a),
            [
                span(" TERM ", "Some(Cyan)", BG, "BOLD"),
                plain("file://boot.raw"),
                span(" |", MUTED, BG, "NONE"),
                plain(" "),
                span("replay", "Some(Cyan)", BG, "NONE"),
                plain(" "),
                plain("RUN"),
            ]
        );
        a.end_replay();
        assert_eq!(render("{baud} {framing}", &a), [plain("replay END")]);
        assert_eq!(render("[x](serial?bold:hide)", &a), Vec::<String>::new());
    }

    /// MAX-166: the replay's variables: its file, state and progress; `{framing}` says
    /// `PAUSE` while paused, `replaying` holds; without a replay they are empty or zero.
    /// How: a 10-byte file replayed, 4 bytes read, paused; then an app on a port.
    #[test]
    fn replay_variables() {
        let path = std::env::temp_dir().join(format!(
            "maxicom_test_{}_status_replay.raw",
            std::process::id()
        ));
        std::fs::write(&path, b"0123456789").unwrap();
        let mut a = app(Mode::Terminal);
        let template = "{replay_file}|{replay_state}|{replay_percent}|{replay_read}|{replay_size}";
        assert_eq!(render(template, &a), [plain("||0|0|0")]);
        assert_eq!(render("[x](replaying?bold:hide)", &a), Vec::<String>::new());

        let replay = crate::link::replay::Replay::open(&path, std::time::Duration::ZERO).unwrap();
        let mut link = crate::link::LinkState::new(a.link_config().clone());
        link.set_replay(replay);
        *a.link_mut() = link;
        a.start_replay(path.clone(), None);
        a.link_mut().read(&mut [0; 4]).unwrap().unwrap();
        a.link_mut().replay_mut().unwrap().pause();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(
            render(template, &a),
            [plain(&format!("{name}|PAUSE|40|4|10"))]
        );
        assert_eq!(render("{baud} {framing}", &a), [plain("replay PAUSE")]);
        assert_eq!(
            render("[x](replaying?bold:hide)", &a),
            [span("x", "Some(White)", BG, "BOLD")]
        );
        let _ = std::fs::remove_file(&path);
    }

    /// MAX-166: a replayed serial session shows the line settings it was recorded with, in
    /// the baud and framing places and styles; `line_settings` shows them.
    /// How: the default left template while replaying a session recorded at 9600 7E1.
    #[test]
    fn session_replay_shows_its_recorded_line() {
        let mut a = app(Mode::Terminal);
        let mut session = crate::capture::tests::device("/dev/ttyACM1");
        session.line = Some(crate::capture::CaptureLine {
            baud: 9600,
            framing: "7E1".parse().unwrap(),
        });
        a.start_replay(
            std::path::PathBuf::from("/tmp/logs/boot.raw"),
            Some(&session),
        );
        let d = StatusBarConfig::default();
        assert_eq!(
            render(&d.left, &a),
            [
                span(" TERM ", "Some(Cyan)", BG, "BOLD"),
                plain("file://boot.raw"),
                span(" |", MUTED, BG, "NONE"),
                plain(" "),
                span("9600", "Some(Cyan)", BG, "NONE"),
                plain(" "),
                plain("7E1"),
            ]
        );
        assert_eq!(render("<{baud:5}>", &a), [plain("< 9600>")]);
        assert_eq!(
            render("[x](serial?bold:hide)", &a),
            [span("x", "Some(White)", BG, "BOLD")]
        );
    }

    /// The mode pill follows the prefix and copy-mode conditions.
    /// How: same template with the prefix active, then in line copy mode.
    #[test]
    fn default_left_conditions() {
        let d = StatusBarConfig::default();
        let mut a = app(Mode::Terminal);
        a.update(Action::App(AppAction::SetPrefix(true)));
        assert_eq!(
            render(&d.left, &a)[0],
            span(" TERM ", "Some(Black)", "Some(Yellow)", "BOLD")
        );

        let mut l = app(Mode::Line);
        l.update(Action::CopyMode(CopyModeAction::Enter));
        assert_eq!(
            render(&d.left, &l)[0],
            span(" LINE ", "Some(Black)", "Some(Magenta)", "BOLD")
        );
    }

    /// MAX-71: `{copy_side}` and `copy_raw` follow the copy cursor's panel; the default left
    /// template shows the side next to the mode pill in copy mode only.
    /// How: line mode with a Hex raw panel and one line, copy mode, then the toggle.
    #[test]
    fn copy_side_variable_and_condition() {
        let d = StatusBarConfig::default();
        let template = "[{copy_side}](copy_raw?red:blue)";
        let mut l = app(Mode::Line);
        l.settings_mut().raw_side.view = crate::configs::RawSideView::Hex;
        l.feed_rx(b"abc\r\n");
        assert_eq!(render(&d.left, &l).len(), 7);

        l.update(Action::CopyMode(CopyModeAction::Enter));
        assert_eq!(
            render(template, &l),
            [span("TXT", "Some(Blue)", BG, "NONE")]
        );
        assert_eq!(
            render(&d.left, &l)[1],
            span("TXT ", "Some(Magenta)", BG, "BOLD")
        );

        l.update(Action::CopyMode(CopyModeAction::ToggleSide));
        assert_eq!(render(template, &l), [span("RAW", "Some(Red)", BG, "NONE")]);
        assert_eq!(
            render(&d.left, &l)[..2],
            [
                span(" LINE ", "Some(Black)", "Some(Red)", "BOLD"),
                span("RAW ", "Some(Red)", BG, "BOLD"),
            ]
        );
    }

    /// `mouse` is true while the mouse capture setting is on (MAX-113).
    #[test]
    fn mouse_condition() {
        let mut l = app(Mode::Line);
        assert_eq!(
            render("[m](mouse?red:blue)", &l),
            [span("m", "Some(Red)", BG, "NONE")]
        );
        l.settings_mut().ui.mouse = false;
        assert_eq!(
            render("[m](mouse?red:blue)", &l),
            [span("m", "Some(Blue)", BG, "NONE")]
        );
    }

    /// MAX-163: `\` at the end of a line joins the next one, its indentation (spaces, tabs)
    /// dropped, with LF or CRLF; a space before the `\` stays; another `\` stays as written.
    /// The default templates, written one block per line, join into one line.
    /// How: the same template on one line and on three, rendered; the joins alone.
    #[test]
    fn templates_join_lines_ending_in_backslash() {
        let a = app(Mode::Line);
        let one = "[a](red) [b](blue)[c](green)";
        let three = "[a](red) \\\n    [b](blue)\\\r\n\t[c](green)";
        assert_eq!(render(three, &a), render(one, &a));
        assert_eq!(join_lines("a\\b\\"), "a\\b\\");
        assert_eq!(join_lines("a\\\n\n  b"), "a\n  b");

        let defaults = StatusBarConfig::default();
        for template in [&defaults.left, &defaults.center, &defaults.right] {
            assert!(template.contains("\\\n"));
            let joined = join_lines(template);
            assert!(!joined.contains(['\\', '\n']), "{joined}");
        }
    }

    /// MAX-147: `session` is true while the session backup runs, which never lights `rec`.
    /// How: a backup started in a temp folder.
    #[test]
    fn session_condition() {
        let mut a = app(Mode::Line);
        let template = "[s](session?red:blue)[r](rec?red:blue)";
        assert_eq!(
            render(template, &a),
            [
                span("s", "Some(Blue)", BG, "NONE"),
                span("r", "Some(Blue)", BG, "NONE")
            ]
        );
        let dir =
            std::env::temp_dir().join(format!("maxicom_test_{}_session_cond", std::process::id()));
        a.start_session(Some(dir.clone()));
        assert_eq!(
            render(template, &a),
            [
                span("s", "Some(Red)", BG, "NONE"),
                span("r", "Some(Blue)", BG, "NONE")
            ]
        );
        drop(a);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Default center and right templates: hidden blocks are omitted, counters and time filled.
    /// How: golden spans in terminal mode, time format set to a literal.
    #[test]
    fn default_center_and_right() {
        let a = app(Mode::Terminal);
        let d = StatusBarConfig::default();
        assert!(render(&d.center, &a).is_empty());
        assert_eq!(
            render(&d.right, &a),
            [
                span("RX:    0", MUTED, BG, "NONE"),
                span(" | ", MUTED, BG, "NONE"),
                span("TX:    0", MUTED, BG, "NONE"),
                span(" | ", MUTED, BG, "NONE"),
                span("TIME", "Some(Yellow)", BG, "NONE"),
                plain(" "),
                span("?", MUTED, BG, "NONE"),
                plain(" "),
                span("≡", MUTED, BG, "NONE"),
                plain(" "),
            ]
        );
    }

    /// MAX-6: the default center shows how many triggers are armed, hidden while none is;
    /// `{trigger_names}` names them, the first and how many more when squeezed; a click on
    /// either opens the trigger browser (MAX-167).
    /// How: the default triggers, disarmed then all armed, then one.
    #[test]
    fn armed_triggers() {
        let mut a = app(Mode::Terminal);
        let triggers = crate::configs::TriggersConfig::default().compile().unwrap();
        a.set_triggers(triggers);
        let d = StatusBarConfig::default();
        let texts = |a: &App| -> Vec<String> {
            let spans = render(&d.center, a);
            spans
                .iter()
                .map(|s| s.split('|').next().unwrap().to_string())
                .collect()
        };
        let compact = Fit {
            compact: true,
            drop_from: None,
        };
        assert!(texts(&a).is_empty());
        assert_eq!(text_with("{trigger_names}", &a, compact), "");

        a.update(Action::App(AppAction::ToggleTriggers));
        assert_eq!(texts(&a), ["◎ 2 "]);
        assert_eq!(render("{trigger_names}", &a), [plain("login, u-boot")]);
        assert_eq!(text_with("{trigger_names}", &a, compact), "login +1");
        assert_eq!(render("{trigger_count:02}", &a), [plain("02")]);

        a.update(Action::App(AppAction::ArmTriggers {
            on: false,
            name: Some("login".to_string()),
        }));
        assert_eq!(text_with("{trigger_names}", &a, compact), "u-boot");
        for var in [Var::TriggerCount, Var::TriggerNames] {
            assert_eq!(
                var.click(),
                Some(ClickAction::Run(KeyBindingAction::Triggers))
            );
        }
    }

    /// Every variable is filled; an unknown variable stays as written.
    /// How: one template with all variables, one with `{unknown}`.
    #[test]
    fn variables() {
        let a = app(Mode::Terminal);
        assert_eq!(
            render(
                "[ {mode} {port} {baud} {framing} {rx_bytes} {tx_bytes} {time}](reset)",
                &a
            ),
            [plain(" TERM /dev/ttyUSB0 115200 8N1 0 0 TIME")]
        );
        assert_eq!(
            render(
                "{scroll_pos}/{scroll_pos_up}/{scroll_percent}/{scroll_offset}/{scroll_max}",
                &a
            ),
            [plain("All/ALL/100/0/0")]
        );
        assert_eq!(
            render("plain {unknown} text", &a),
            [plain("plain {unknown} text")]
        );
    }

    /// Style words: modifiers, explicit colors, unknown words ignored, `hide` drops the span.
    /// How: four styled blocks in one template.
    #[test]
    fn style_words() {
        let a = app(Mode::Terminal);
        assert_eq!(
            render("[a](bold) [b](nope) [c](fg=#112233 bg=red) [d](hide)", &a),
            [
                span("a", "Some(White)", BG, "BOLD"),
                plain(" "),
                plain("b"),
                plain(" "),
                span("c", "Some(Rgb(17, 34, 51))", "Some(Red)", "NONE"),
                plain(" "),
            ]
        );
    }

    /// Nested ternaries resolve, a ternary without `:` is ignored (base style).
    /// How: `term` is false while the prefix is active, so the outer false branch wins.
    #[test]
    fn ternaries() {
        let mut a = app(Mode::Terminal);
        a.update(Action::App(AppAction::SetPrefix(true)));
        assert_eq!(
            render("[x](term?(copy?red:green):blue) [y](line?red)", &a),
            [span("x", "Some(Blue)", BG, "NONE"), plain(" "), plain("y")]
        );
        let a = app(Mode::Terminal);
        assert_eq!(
            render("[x](term?(copy?red:green):blue)", &a),
            [span("x", "Some(Green)", BG, "NONE")]
        );
    }

    /// A variable's value is never read as markup, since templates are parsed before filling.
    /// How: a port name that looks like a styled block is shown as written.
    #[test]
    fn variable_value_is_not_markup() {
        let mut a = app(Mode::Terminal);
        a.link_mut()
            .set_address("[x](bold)".parse::<Address>().unwrap());
        assert_eq!(render("port {port}", &a), [plain("port [x](bold)")]);
    }

    /// Braces around a known variable: only the inner `{mode}` is filled.
    /// How: `{{mode}}` and an unclosed `{mode`.
    #[test]
    fn braces_edge_cases() {
        let a = app(Mode::Terminal);
        assert_eq!(render("{{mode}} {mode", &a), [plain("{TERM} {mode")]);
    }

    /// Unfinished markup is kept as text, and an empty block renders nothing.
    /// How: an unclosed `[`, a `](` without closing paren, and `[](bold)`.
    #[test]
    fn malformed_markup() {
        let a = app(Mode::Terminal);
        assert_eq!(
            render("open [ bracket and ](x", &a),
            [plain("open "), plain("["), plain(" bracket and ](x")]
        );
        assert_eq!(
            render("[](bold)[empty text above]", &a),
            [plain("["), plain("empty text above]")]
        );
    }

    /// Where a template's clickable blocks are (start column, width) and what they run
    fn clicks(template: &str, app: &App) -> Vec<(u16, u16, ClickAction)> {
        spots(template, app)
            .into_iter()
            .map(|(col, width, click, _)| (col, width, click))
            .collect()
    }

    fn spots(template: &str, app: &App) -> Vec<ClickSpot> {
        let theme = ThemeConfig::default();
        CompiledTemplate::new(template, &theme)
            .spans_and_clicks(app, *theme.ui.status_bar, "TIME", Fit::default())
            .1
    }

    /// MAX-125: `click=<action>` makes a block clickable and is no style word; an unknown
    /// action is dropped and a hidden block isn't clickable.
    /// How: the same block with and without `click=help`, `click=nope`, a `hide` one.
    #[test]
    fn click_keyword() {
        use KeyBindingAction::{Help, Settings};
        let a = app(Mode::Terminal);
        assert_eq!(
            render("[ ? ](muted bold click=help)", &a),
            render("[ ? ](muted bold)", &a)
        );
        assert_eq!(
            clicks("ab [ ? ](muted click=help)[x](click=settings)", &a),
            [
                (3, 3, ClickAction::Run(Help)),
                (6, 1, ClickAction::Run(Settings))
            ]
        );
        assert_eq!(clicks("[ ? ](click=nope)", &a), []);
        assert_eq!(clicks("[ ? ](hide click=help)", &a), []);
    }

    /// MAX-125: a block takes the click its first variable implies, unless it has its own
    /// `click=`; in plain text only the variable is clickable, not the text around it.
    /// How: `{mode}`, `{port}` (no click) and `{baud}` in plain text, `{rx_bytes}` (no click),
    /// `{copy_side}`.
    #[test]
    fn variables_imply_clicks() {
        use KeyBindingAction::{Help, LinkSettings, SKInputMode};
        let mut a = app(Mode::Terminal);
        a.link_mut()
            .set_address("/dev/x".parse::<Address>().unwrap());
        let run = ClickAction::Run;
        assert_eq!(clicks("[ {mode} ](bold)", &a), [(0, 6, run(SKInputMode))]);
        assert_eq!(clicks("[{mode}](click=help)", &a), [(0, 4, run(Help))]);
        assert_eq!(
            clicks("[{mode}](click=nope)", &a),
            [(0, 4, run(SKInputMode))]
        );
        let baud = a.link_config().line.baud.to_string().len() as u16;
        assert_eq!(
            clicks("port {port} | {baud} {rx_bytes}", &a),
            [(14, baud, run(LinkSettings))]
        );
        assert_eq!(clicks("[RX {rx_bytes}](muted)", &a), []);

        a.update(Action::CopyMode(CopyModeAction::Enter));
        assert_eq!(
            clicks("[{copy_side}](bold)", &a),
            [(0, 3, ClickAction::ToggleSide)]
        );
    }

    /// MAX-131: a block's `hover=` is no style word: its own style, `none` to have none, or
    /// nothing to take the bar's `hover` option; the option is read the same way.
    /// How: `hover=(bg=red bold)`, `hover=none`, no `hover=`, and `compile_hover` alone.
    #[test]
    fn hover_keyword_and_option() {
        let a = app(Mode::Terminal);
        let theme = ThemeConfig::default();
        assert_eq!(
            render("[ ? ](muted hover=(bg=red bold) click=help)", &a),
            render("[ ? ](muted)", &a)
        );
        let red_bold = Style::default().bg(Color::Red).add_modifier(Modifier::BOLD);
        let hover = |template| spots(template, &a)[0].3;
        assert_eq!(
            hover("[?](click=help hover=(bg=red bold))"),
            Some(Some(red_bold))
        );
        assert_eq!(hover("[?](click=help hover=none)"), Some(None));
        assert_eq!(hover("[?](click=help)"), None);

        let reversed = Style::default().add_modifier(Modifier::REVERSED);
        assert_eq!(compile_hover("reversed", &theme), Some(reversed));
        assert_eq!(compile_hover(" (bg=red bold) ", &theme), Some(red_bold));
        assert_eq!(compile_hover("none", &theme), None);
    }

    /// MAX-139: `{name:spec}` pads a variable: numbers right and text left by default, `<` `>`
    /// `^` to align, `0` for zeros (numbers only), `h` for a human size; a longer value isn't
    /// cut, and a malformed spec stays as written.
    /// How: the baud rate (115200) and the port with each spec, RX at 1536 bytes.
    #[test]
    fn variable_formats() {
        let mut a = app(Mode::Terminal);
        for (template, out) in [
            ("{baud:8}", "  115200"),
            ("{baud:08}", "00115200"),
            ("{baud:<8}", "115200  "),
            ("{baud:^9}", " 115200  "),
            ("{baud:3}", "115200"),
            ("{port:14}", "/dev/ttyUSB0  "),
            ("{port:>14}", "  /dev/ttyUSB0"),
            ("{port:014}", "/dev/ttyUSB0  "),
            ("{baud:x}", "{baud:x}"),
            ("{baud:}", "{baud:}"),
            ("{baud:8x}", "{baud:8x}"),
            ("{baud:+8}", "{baud:+8}"),
        ] {
            assert_eq!(render(template, &a), [plain(out)], "{template}");
        }
        a.stats.update_rx(1536);
        assert_eq!(render("{rx_bytes:h}", &a), [plain("1.5K")]);
        assert_eq!(render("{rx_bytes:>6h}", &a), [plain("  1.5K")]);
        assert_eq!(render("{rx_bytes:06}", &a), [plain("001536")]);
    }

    /// MAX-139: a human size is the number up to 999, then one decimal below 10 and none above,
    /// in units of 1024, never more than 4 columns.
    #[test]
    fn human_sizes() {
        let human = |bytes| {
            let mut out = String::new();
            write_human(&mut out, bytes);
            out
        };
        for (bytes, out) in [
            (0, "0"),
            (999, "999"),
            (1000, "1.0K"),
            (1023, "1.0K"),
            (1024, "1.0K"),
            (1536, "1.5K"),
            (10 * 1024, "10K"),
            (999 * 1024, "999K"),
            (1024 * 1024 - 1, "1.0M"),
            (5_000_000, "4.8M"),
            (u64::MAX, "16E"),
        ] {
            assert_eq!(human(bytes), out, "{bytes}");
        }
    }

    /// MAX-139: a padded variable is clickable over its whole width.
    #[test]
    fn padded_variable_click() {
        use KeyBindingAction::LinkSettings;
        let a = app(Mode::Terminal);
        assert_eq!(
            clicks("{baud:10}", &a),
            [(0, 10, ClickAction::Run(LinkSettings))]
        );
    }

    /// Text of a template's spans in one string, squeezed by `fit`
    fn text_with(template: &str, app: &App, fit: Fit) -> String {
        let theme = ThemeConfig::default();
        CompiledTemplate::new(template, &theme)
            .spans_and_clicks(app, *theme.ui.status_bar, "TIME", fit)
            .0
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    /// MAX-139: `prio=N` is no style word; squeezed, the blocks of a level and above are left
    /// out, the ones without `prio=` never; the bar's levels are listed from the highest.
    /// How: a template with prio 1, 2, 3 and none, then `drop_from` 3, 2 and 1.
    #[test]
    fn prio_keyword() {
        let a = app(Mode::Terminal);
        assert_eq!(render("[a](bold prio=2)", &a), render("[a](bold)", &a));
        let template = "[a](prio=1)[b](prio=2)[c]()[d](prio=3)[e](prio=x)";
        let drop = |from| Fit {
            compact: false,
            drop_from: from,
        };
        assert_eq!(text_with(template, &a, drop(None)), "abcde");
        assert_eq!(text_with(template, &a, drop(Some(3))), "abce");
        assert_eq!(text_with(template, &a, drop(Some(2))), "ace");
        assert_eq!(text_with(template, &a, drop(Some(1))), "ce");

        let config = StatusBarConfig {
            left: "[a](prio=2)[b](prio=4)".into(),
            center: String::new(),
            right: "[c](prio=2)[d](prio=1)".into(),
            ..StatusBarConfig::default()
        };
        let bar = CompiledStatusBar::new(&config, &ThemeConfig::default());
        assert_eq!(bar.prios, [4, 2, 1]);
    }

    /// MAX-139: squeezed, a variable without a format of its own takes its compact form: the
    /// port's file name, a byte count as a size, the pan without spaces and parentheses; one
    /// with a format keeps it.
    /// How: the port, RX at 1536 with and without `:6`, a 200-column terminal panned by 3.
    #[test]
    fn compact_forms() {
        use crate::{configs::TerminalDimensionMode, inputs::HistoryAction};
        let compact = Fit {
            compact: true,
            drop_from: None,
        };
        let mut a = app(Mode::Terminal);
        a.stats.update_rx(1536);
        assert_eq!(text_with("{port}", &a, Fit::default()), "/dev/ttyUSB0");
        assert_eq!(text_with("{port}", &a, compact), "ttyUSB0");
        assert_eq!(text_with("{rx_bytes}", &a, compact), "1.5K");
        assert_eq!(text_with("{rx_bytes:6}", &a, compact), "  1536");

        a.settings_mut().terminal_mode.dimension_mode = TerminalDimensionMode::Fixed;
        a.settings_mut().terminal_mode.cols = 200;
        a.update_viewport(crate::app::types::ViewportSize::new(80, 20));
        a.update(Action::History(HistoryAction::PanRight(3)));
        assert_eq!(text_with("{pan_left}", &a, Fit::default()), "◀ (3)");
        assert_eq!(text_with("{pan_left}", &a, compact), "◀3");
        assert_eq!(text_with("{pan_right}", &a, compact), "117▶");
    }

    /// MAX-185: a link saved for this machine shows by its name, full or squeezed; one saved
    /// for another machine by its address.
    /// How: the app's `/dev/ttyUSB0` saved as `esp32`, then saved for another machine only.
    #[test]
    fn saved_link_by_name() {
        let compact = Fit {
            compact: true,
            drop_from: None,
        };
        let mut a = app(Mode::Terminal);
        let saved = |hosts: &str| {
            let text = format!("[[link]]\nname = 'esp32'\naddress = '/dev/ttyUSB0'\n{hosts}");
            Ok(toml::from_str(&text).unwrap())
        };
        a.set_links(saved(""));
        assert_eq!(text_with("{port}", &a, Fit::default()), "esp32");
        assert_eq!(text_with("{port}", &a, compact), "esp32");
        a.set_links(saved("hosts = ['not-this-machine-0']"));
        assert_eq!(text_with("{port}", &a, Fit::default()), "/dev/ttyUSB0");
    }

    /// MAX-181: `add_cr` / `add_lf` hold while their setting is on, in both modes.
    #[test]
    fn add_cr_add_lf_conditions() {
        for mode in [Mode::Terminal, Mode::Line] {
            let mut app = app(mode);
            assert!(!Cond::AddCr.eval(&app) && !Cond::AddLf.eval(&app));
            app.settings_mut().add_cr = true;
            assert!(
                Cond::AddCr.eval(&app) && !Cond::AddLf.eval(&app),
                "{mode:?}"
            );
            app.settings_mut().add_lf = true;
            assert!(Cond::AddLf.eval(&app), "{mode:?}");
        }
        assert_eq!(Cond::from_name("add_cr"), Cond::AddCr);
        assert_eq!(Cond::from_name("ADD_LF"), Cond::AddLf);
    }
}
