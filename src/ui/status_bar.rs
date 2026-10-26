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
//! | `{port}`            | Serial device path                                      | `/dev/ttyUSB0`   |
//! | `{baud}`            | Serial baudrate                                         | `115200`         |
//! | `{framing}`         | Data bits, parity, stop bits                            | `8N1`            |
//! | `{rx_bytes}`        | Total bytes received                                    | `1024`           |
//! | `{tx_bytes}`        | Total bytes transmitted                                 | `256`            |
//! | `{time}`            | Current local time (format in config)                   | `21:45:00`       |
//! | `{pan_left}`        | Left pan amount possible                                | `◀ (3)`          |
//! | `{pan_right}`       | Right pan amount possible with alert indicator          | `!(2) ▶`         |
//! | `{scroll_pos}`      | Scroll position indicator (`Top`, `Bot`, `All`, or `%`) | `42%`            |
//! | `{scroll_pos_up}`   | Uppercase scroll position indicator                     | `BOT`            |
//! | `{scroll_percent}`  | Scroll position as numeric percentage (0-100)           | `42`             |
//! | `{scroll_offset}`   | Lines scrolled up from the bottom                       | `15`             |
//! | `{scroll_max}`      | Maximum total scrollback lines available                | `500`            |
//! | `{copy_side}`       | Copy cursor panel (`TXT` or `RAW`), empty out of copy  | `RAW`            |
//!
//! ## Conditional States (`?`)
//! | State / Keyword                 | Aliases                                    | Matches When...                                          |
//! |---------------------------------|--------------------------------------------|----------------------------------------------------------|
//! | `prefix`                        | —                                          | Leader prefix key (`Ctrl-A`) is currently active         |
//! | `rec`                           | `logging`                                  | File recording / capture session is active               |
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
//! | `mouse`                         | —                                          | Mouse capture is on (`ui.mouse`)                         |
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
//! ## Clicks
//! A block without `click=` runs what its first variable implies; a variable in plain text is
//! clickable on its own. The others (`{port}`, `{rx_bytes}`, `{tx_bytes}`, `{time}`,
//! `{scroll_max}`) do nothing: the port isn't changed while running.
//!
//! | Variable                                                   | Click                     |
//! |------------------------------------------------------------|---------------------------|
//! | `{mode}`                                                   | `toggle_input_mode`       |
//! | `{baud}`, `{framing}`                                      | `serial_settings`         |
//! | `{copy_side}`                                              | the copy cursor's panel   |
//! | `{pan_left}`, `{pan_right}`                                | `pan_left`, `pan_right`   |
//! | `{scroll_pos}` `{scroll_pos_up}` `{scroll_percent}` `{scroll_offset}` | `scroll_to_bottom` |
//!
//! ## Examples
//! ```toml
//! [ui.status_bar]
//! # Solid pill mode indicator that lights up with background when prefix is pressed:
//! left = "[ {mode} ](prefix?(bg=main fg=black bold):copy?(bg=magenta fg=white bold):search?(bg=good fg=black bold):(fg=main bold)) {port} | [{baud}](secondary) {framing}"
//!
//! # Center scroll indicator and pan arrows with warning states:
//! center = "[{pan_left} ](pan_left?warn:hide)[{scroll_pos}](has_scroll?muted:hide)[ {pan_right}](pan_right_alert?(bg=warn fg=black bold):(pan_right?warn:hide))"
//!
//! # Right-aligned status with a red badge for recording (a click stops it) and a help button:
//! right = "[ ● REC ](rec?(bg=warn fg=white bold):hide click=toggle_logging) [{scroll_pos_up}](has_scroll?muted:hide) [RX: {rx_bytes}](muted) | [TX: {tx_bytes}](muted) | [{time}](secondary) [?](muted click=help)"
//! ```

use std::borrow::Cow;
use std::fmt::Write as _;

use unicode_width::UnicodeWidthStr;

use ratatui::{
    Frame,
    layout::{Alignment, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::{
    app::{
        App, InputContext,
        selection::CopySide,
        types::{ClickAction, ClickTarget},
    },
    configs::{KeyBindingAction, Mode, StatusBarConfig, StatusBarLayout, ThemeConfig},
    ui::common::{pan_left_indicator, pan_right_indicator},
};

pub struct CompiledStatusBar {
    left: CompiledTemplate,
    center: CompiledTemplate,
    right: CompiledTemplate,
    time_format: String,
    hover: Option<Style>,
}

impl CompiledStatusBar {
    pub fn new(config: &StatusBarConfig, theme: &ThemeConfig) -> Self {
        Self {
            left: CompiledTemplate::new(&config.left, theme),
            center: CompiledTemplate::new(&config.center, theme),
            right: CompiledTemplate::new(&config.right, theme),
            time_format: config.time_format.clone(),
            hover: compile_hover(&config.hover, theme),
        }
    }
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
    /// Under the pointer it takes its `hover=`, else the bar's
    Styled {
        text: Vec<TextPart>,
        style: Vec<StyleExpr>,
        click: Option<ClickAction>,
        hover: HoverOverride,
    },
}

enum TextPart {
    Literal(String),
    Var(Var),
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
    ScrollPos,
    ScrollPosUp,
    ScrollPercent,
    ScrollOffset,
    ScrollMax,
    CopySide,
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
            "scroll_pos" => Self::ScrollPos,
            "scroll_pos_up" => Self::ScrollPosUp,
            "scroll_percent" => Self::ScrollPercent,
            "scroll_offset" => Self::ScrollOffset,
            "scroll_max" => Self::ScrollMax,
            "copy_side" => Self::CopySide,
            _ => return None,
        })
    }

    /// What a click on the variable runs (MAX-125)
    fn click(self) -> Option<ClickAction> {
        let action = match self {
            Self::Mode => KeyBindingAction::SKInputMode,
            Self::Baud | Self::Framing => KeyBindingAction::SerialSettings,
            Self::PanLeft => KeyBindingAction::PanLeft,
            Self::PanRight => KeyBindingAction::PanRight,
            Self::ScrollPos | Self::ScrollPosUp | Self::ScrollPercent | Self::ScrollOffset => {
                KeyBindingAction::ScrollToBottom
            }
            Self::CopySide => return Some(ClickAction::ToggleSide),
            Self::Port | Self::RxBytes | Self::TxBytes | Self::Time | Self::ScrollMax => {
                return None;
            }
        };
        Some(ClickAction::Run(action))
    }

    fn write(self, out: &mut String, app: &App, time_format: &str) {
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
            Self::Port => write!(out, "{}", app.serial_config.port),
            Self::Baud => write!(out, "{}", app.serial_config.baudrate),
            Self::Framing => write!(out, "{}", app.serial_config),
            Self::RxBytes => write!(out, "{}", app.stats.rx()),
            Self::TxBytes => write!(out, "{}", app.stats.tx()),
            Self::Time => write!(out, "{}", chrono::Local::now().format(time_format)),
            Self::PanLeft => write!(out, "{}", pan_left_indicator(app).unwrap_or_default()),
            Self::PanRight => write!(out, "{}", pan_right_indicator(app).unwrap_or_default()),
            Self::ScrollPos => write!(out, "{}", scroll.position_display()),
            Self::ScrollPosUp => write!(out, "{}", scroll.position_display().to_uppercase()),
            Self::ScrollPercent => write!(out, "{}", scroll.percent()),
            Self::ScrollOffset => write!(out, "{}", scroll.offset_from_bottom),
            Self::ScrollMax => write!(out, "{}", scroll.max_scroll),
            Self::CopySide => write!(
                out,
                "{}",
                match app.copy_side() {
                    Some(CopySide::Text) => "TXT",
                    Some(CopySide::Raw) => "RAW",
                    None => "",
                }
            ),
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
    Mouse,
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
            "mouse" => Self::Mouse,
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
            Self::Rec => app.is_logging_active(),
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
            Self::Mouse => app.settings().ui.mouse,
            Self::Unknown => false,
        }
    }
}

impl CompiledTemplate {
    pub fn new(template: &str, theme: &ThemeConfig) -> Self {
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
                    // `click=<action>` and `hover=<style>` are no style: an unknown action is
                    // dropped like an unknown style word
                    let (mouse, style): (Vec<&str>, Vec<&str>) =
                        tokenize_spec(&template[style_start..style_end])
                            .into_iter()
                            .partition(|token| {
                                token.starts_with("click=") || token.starts_with("hover=")
                            });
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
                                TextPart::Var(var) => var.click(),
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
        self.spans_and_clicks(app, base_style, time_format).0
    }

    /// The spans, and where the clickable ones are in the line (start column, width) with what
    /// a click runs (MAX-125). In plain text each variable is clickable on its own, not the
    /// text around it
    fn spans_and_clicks(
        &self,
        app: &App,
        base_style: Style,
        time_format: &str,
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
                            TextPart::Var(var) => {
                                let mut out = String::new();
                                var.write(&mut out, app, time_format);
                                out.width()
                            }
                        } as u16;
                        if let TextPart::Var(var) = part
                            && let Some(click) = var.click()
                            && width > 0
                        {
                            clicks.push((part_col, width, click, None));
                        }
                        part_col = part_col.saturating_add(width);
                    }
                    let span = Span::styled(fill_text(parts, app, time_format), base_style);
                    col = col.saturating_add(span.width() as u16);
                    spans.push(span);
                }
                Segment::Styled {
                    text,
                    style,
                    click,
                    hover,
                } => {
                    let Some(span_style) = resolve_style(style, base_style, app) else {
                        continue;
                    };
                    let content = fill_text(text, app, time_format);
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

/// Splits text into literals and known `{variables}`; an unknown `{name}` stays as written
fn parse_text_parts(text: &str) -> Vec<TextPart> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut rest = text;

    while let Some(open) = rest.find('{') {
        literal.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after
            .find('}')
            .and_then(|close| Some((close, Var::from_name(&after[..close])?)))
        {
            Some((close, var)) => {
                if !literal.is_empty() {
                    parts.push(TextPart::Literal(std::mem::take(&mut literal)));
                }
                parts.push(TextPart::Var(var));
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
fn fill_text<'a>(parts: &'a [TextPart], app: &App, time_format: &str) -> Cow<'a, str> {
    match parts {
        [] => Cow::Borrowed(""),
        [TextPart::Literal(text)] => Cow::Borrowed(text),
        _ => {
            let mut out = String::new();
            for part in parts {
                match part {
                    TextPart::Literal(text) => out.push_str(text),
                    TextPart::Var(var) => var.write(&mut out, app, time_format),
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
pub fn render_status_bar(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    theme: &ThemeConfig,
    status_bar: &CompiledStatusBar,
) -> Vec<ClickTarget> {
    let mut targets = Vec::new();
    if !app.settings().ui.show_status_bar {
        return targets;
    }

    // only the band layout fills the row with its colour; the others put the text on the UI
    // background (MAX-135)
    let layout = app.settings().ui.status_bar_layout();
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
        if app.settings().ui.show_borders {
            parts_area = area.inner(Margin::new(1, 0));
        }
        frame.render_widget(Block::default().style(base_style), area);
    }

    let time_format = &status_bar.time_format;
    let parts = [
        (&status_bar.left, Alignment::Left),
        (&status_bar.center, Alignment::Center),
        (&status_bar.right, Alignment::Right),
    ];
    for (template, alignment) in parts {
        let (mut spans, clicks) = template.spans_and_clicks(app, base_style, time_format);
        // in the border, the cells left unwritten keep the `─`: a part of blanks would cut it
        let blank = || spans.iter().all(|span| span.content.trim().is_empty());
        if spans.is_empty() || (in_border && blank()) {
            continue;
        }
        // and a space keeps the text off the dashes, unless the part has one there
        let mut lead = 0;
        if in_border {
            let space = || Span::styled(" ", base_style);
            if !spans[0].content.starts_with(char::is_whitespace) {
                spans.insert(0, space());
                lead = 1;
            }
            if !spans[spans.len() - 1]
                .content
                .ends_with(char::is_whitespace)
            {
                spans.push(space());
            }
        }
        let line = Line::from(spans);
        // where the paragraph starts the line, as ratatui works it out
        let width = line.width() as u16;
        let start = parts_area.x
            + match alignment {
                Alignment::Left => 0,
                Alignment::Center => (parts_area.width / 2).saturating_sub(width / 2),
                Alignment::Right => parts_area.width.saturating_sub(width),
            };
        for (col, width, action, hover) in clicks {
            let cell = Rect::new(start.saturating_add(lead + col), parts_area.y, width, 1)
                .intersection(parts_area);
            if !cell.is_empty() {
                targets.push(ClickTarget {
                    area: cell,
                    action,
                    hover: hover.unwrap_or(status_bar.hover),
                });
            }
        }
        frame.render_widget(Paragraph::new(line).alignment(alignment), parts_area);
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
        serial::SerialConfig,
    };

    fn app(mode: Mode) -> App {
        let mut settings = SettingsConfig::default();
        settings.mode = mode;
        settings.ui.status_bar.time_format = "TIME".to_string();
        let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
        App::new(settings, resolver, SerialConfig::default())
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
                plain(" /dev/ttyUSB0 "),
                span("|", MUTED, BG, "NONE"),
                plain(" "),
                span("115200", "Some(Cyan)", BG, "NONE"),
                plain(" 8N1"),
            ]
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
        assert_eq!(render(&d.left, &l).len(), 6);

        l.update(Action::CopyMode(CopyModeAction::Enter));
        assert_eq!(
            render(template, &l),
            [span("TXT", "Some(Blue)", BG, "NONE")]
        );
        assert_eq!(
            render(&d.left, &l)[1],
            span(" TXT", "Some(Magenta)", BG, "BOLD")
        );

        l.update(Action::CopyMode(CopyModeAction::ToggleSide));
        assert_eq!(render(template, &l), [span("RAW", "Some(Red)", BG, "NONE")]);
        assert_eq!(
            render(&d.left, &l)[..2],
            [
                span(" LINE ", "Some(Black)", "Some(Red)", "BOLD"),
                span(" RAW", "Some(Red)", BG, "BOLD"),
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

    /// Default center and right templates: hidden blocks are omitted, counters and time filled.
    /// How: golden spans in terminal mode, time format set to a literal.
    #[test]
    fn default_center_and_right() {
        let a = app(Mode::Terminal);
        let d = StatusBarConfig::default();
        assert_eq!(render(&d.center, &a), [plain(" ")]);
        assert_eq!(
            render(&d.right, &a),
            [
                span("RX: 0", MUTED, BG, "NONE"),
                plain(" "),
                span("|", MUTED, BG, "NONE"),
                plain(" "),
                span("TX: 0", MUTED, BG, "NONE"),
                plain(" "),
                span("|", MUTED, BG, "NONE"),
                plain(" "),
                span("TIME", "Some(Yellow)", BG, "NONE"),
                plain(" "),
                span("?", MUTED, BG, "NONE"),
                plain(" "),
                span("≡", MUTED, BG, "NONE"),
                plain(" "),
            ]
        );
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
        a.serial_config.port = "[x](bold)".to_string();
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
            .spans_and_clicks(app, *theme.ui.status_bar, "TIME")
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
        use KeyBindingAction::{Help, SKInputMode, SerialSettings};
        let mut a = app(Mode::Terminal);
        a.serial_config.port = "/dev/x".to_string();
        let run = ClickAction::Run;
        assert_eq!(clicks("[ {mode} ](bold)", &a), [(0, 6, run(SKInputMode))]);
        assert_eq!(clicks("[{mode}](click=help)", &a), [(0, 4, run(Help))]);
        assert_eq!(
            clicks("[{mode}](click=nope)", &a),
            [(0, 4, run(SKInputMode))]
        );
        let baud = a.serial_config.baudrate.to_string().len() as u16;
        assert_eq!(
            clicks("port {port} | {baud} {rx_bytes}", &a),
            [(14, baud, run(SerialSettings))]
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
}
