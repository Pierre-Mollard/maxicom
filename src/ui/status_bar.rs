//! # Status Bar Formatting Engine
//!
//! The status bar supports dynamic templating, Starship-style styling,
//! conditional ternaries, and element visibility toggling.
//!
//! ## Syntax
//! - **Plain Text**: Rendered using base theme style `status_bar`.
//! - **Variables**: `{variable_name}` replaced at runtime.
//! - **Styled Blocks**: `[content](style_spec)`
//! - **Conditionals**: `[content](condition?true_style:false_style)`
//! - **Grouping**: Use parentheses `(style1 style2)` to group multiple styles in conditional branches.
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
//!
//! ## Conditional States (`?`)
//! | State / Keyword                 | Aliases                                    | Matches When...                                          |
//! |---------------------------------|--------------------------------------------|----------------------------------------------------------|
//! | `prefix`                        | —                                          | Leader prefix key (`Ctrl-A`) is currently active         |
//! | `rec`                           | `logging`                                  | File recording / capture session is active               |
//! | `line`                          | `linemode`                                 | Line Input mode is active                                |
//! | `term`                          | `terminal`                                 | Direct Terminal mode is active                           |
//! | `copy`                          | `copymode`                                 | Copy / Backlog Scrollback mode is active                 |
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
//!
//! ## Style Keywords
//! - **Modifiers**: `bold`, `dim`, `italic`, `underlined` (`underline`), `reversed` (`reverse`), `reset` (`default`), `crossed`, `slow_blink` (`sblink`), `rapid_blink` (`rblink`)
//! - **Visibility**: `hide`, `hidden`, `none` (omits the span completely)
//! - **Colors**: `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`,
//!   `gray`, `darkgray`, `white`, or hex `#RRGGBB`
//! - **Prefixes**: `fg=<color>`, `bg=<color>`
//!
//! ## Examples
//! ```toml
//! [ui.status_bar]
//! # Hide REC when idle, light up red when active; show mode in yellow on prefix:
//! left = "[  REC ](rec?red bold:hide)[ {mode} ](prefix?yellow:cyan bold)[ PAUSED (+{scroll_offset}) ](scrolled?red bold:hide) {port}"
//!
//! # Center scroll position and pan arrows:
//! center = "[{pan_left}](pan_left?yellow:hide) [ {scroll_pos} ](has_scroll?gray:hide) [{pan_right}](pan_right_alert?red bold:(pan_right?yellow:hide))"
//!
//! # Right-aligned byte statistics and blue local clock:
//! right = "[RX: {rx_bytes}](gray) | [TX: {tx_bytes}](gray) | [{time}](blue)"
//! ```

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

use crate::{
    app::{App, InputContext},
    configs::{Mode, StatusBarConfig, ThemeConfig},
    ui::common::{pan_left_indicator, pan_right_indicator},
};

pub fn parse_inline_style(
    spec: &str,
    base_style: Style,
    app: &App,
    theme: &ThemeConfig,
) -> Option<Style> {
    let mut style = base_style;

    for token in tokenize_spec(spec) {
        let resolved = evaluate_expression(token.trim(), app);
        for part in resolved.split_whitespace() {
            let part_lower = part.to_ascii_lowercase();
            match part_lower.as_str() {
                "hide" | "hidden" | "none" => return None,

                "bold" => style = style.add_modifier(Modifier::BOLD),
                "dim" => style = style.add_modifier(Modifier::DIM),
                "italic" => style = style.add_modifier(Modifier::ITALIC),
                "crossed" => style = style.add_modifier(Modifier::CROSSED_OUT),
                "slow_blink" | "sblink" => style = style.add_modifier(Modifier::SLOW_BLINK),
                "rapid_blink" | "rblink" => style = style.add_modifier(Modifier::RAPID_BLINK),
                "underlined" | "underline" => style = style.add_modifier(Modifier::UNDERLINED),
                "reversed" | "reverse" => style = style.add_modifier(Modifier::REVERSED),
                "reset" | "default" => style = base_style,

                "bg=none" | "bg=reset" => style = style.bg(Color::Reset),
                "fg=none" | "fg=reset" => style = style.fg(Color::Reset),

                _ => {
                    if let Some(c) = part.strip_prefix("fg=") {
                        if let Some(color) = resolve_color(c, theme) {
                            style = style.fg(color);
                        }
                    } else if let Some(c) = part.strip_prefix("bg=") {
                        if let Some(color) = resolve_color(c, theme) {
                            style = style.bg(color);
                        }
                    } else if let Some(color) = resolve_color(part, theme) {
                        style = style.fg(color);
                    }
                }
            }
        }
    }

    Some(style)
}

fn resolve_color(name: &str, theme: &ThemeConfig) -> Option<Color> {
    match name.to_ascii_lowercase().as_str() {
        "main" => Some(theme.palette.main),
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

fn evaluate_condition(cond: &str, app: &App) -> bool {
    let ctx = app.input_context();
    let scroll = app.scroll_status();

    match cond.trim().to_ascii_lowercase().as_str() {
        "prefix" => ctx == InputContext::Prefix || app.is_prefix_active(),
        "copy" | "copymode" => ctx == InputContext::CopyMode || app.is_copy_mode_active(),
        "search" => ctx == InputContext::Search || app.is_search_active(),
        "line" | "linemode" => ctx == InputContext::LineInput,
        "term" | "terminal" => ctx == InputContext::Terminal,
        "overlay" | "popup" => ctx == InputContext::Overlay || app.overlay().is_some(),
        "rec" | "logging" => app.is_logging_active(),
        "focused" => app.is_app_focused(),

        "is_live" | "live" | "is_bottom" | "bottom" | "bot" => scroll.is_bottom(),
        "is_top" | "top" => scroll.is_top(),
        "is_scrolled" | "scrolled" | "paused" => !scroll.is_bottom(),
        "scroll_up" | "can_scroll_up" => scroll.can_scroll_up(),
        "scroll_down" | "can_scroll_down" => scroll.can_scroll_down(),
        "has_scroll" => scroll.max_scroll > 0,

        "pan_left" | "can_pan_left" => app.pan_left_status().is_some_and(|cols| cols > 0),
        "pan_right" | "can_pan_right" => app.pan_right_status().is_some_and(|(cols, _)| cols > 0),
        "pan_right_alert" | "pan_alert" if app.settings().mode == crate::configs::Mode::Line => {
            app.pan_right_status().is_some_and(|(cols, explored)| {
                cols > explored.saturating_sub(app.line_mode.x_offset())
            })
        }
        "panned" | "pan" | "panning" => app.pan_left_status().is_some_and(|c| c > 0),

        _ => false,
    }
}

/// recursive, first remove top level parenthesis, then find top level '?'/':' and check condition
fn evaluate_expression<'a>(expr: &'a str, app: &App) -> &'a str {
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
            return evaluate_expression(&expr[1..expr.len() - 1], app);
        }
    }

    // Find the first top-level '?' (depth == 0)
    let mut depth: i32 = 0;
    let mut question_pos = None;

    for (i, ch) in expr.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '?' if depth == 0 && question_pos.is_none() => {
                question_pos = Some(i);
                break;
            }
            _ => {}
        }
    }

    let Some(q_idx) = question_pos else {
        return expr; // no '?', return as is
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
        return expr; // no ':', malformed!
    };

    let true_branch = &remainder[..c_idx];
    let false_branch = &remainder[c_idx + 1..];

    // recursively continue other levels based on runtime evaluation of user-chosen check
    if evaluate_condition(condition, app) {
        evaluate_expression(true_branch, app)
    } else {
        evaluate_expression(false_branch, app)
    }
}

impl StatusBarConfig {
    pub fn parse_to_spans<'a>(
        &self,
        template: &str,
        app: &App,
        base_style: Style,
        theme: &ThemeConfig,
    ) -> Vec<Span<'a>> {
        let expanded = self.expand_variables(template, app);

        let mut spans = Vec::new();
        let mut cursor = 0;

        while cursor < expanded.len() {
            if let Some(open_bracket) = expanded[cursor..].find('[') {
                let text_start = cursor + open_bracket + 1;

                if open_bracket > 0 {
                    spans.push(Span::styled(
                        expanded[cursor..cursor + open_bracket].to_string(),
                        base_style,
                    ));
                }

                if let Some(close_bracket) = expanded[text_start..].find("](") {
                    let text_end = text_start + close_bracket;
                    let style_start = text_end + 2;

                    if let Some(close_paren) = expanded[style_start..].find(')') {
                        let style_end = style_start + close_paren;

                        let text_content = &expanded[text_start..text_end];
                        let style_spec = &expanded[style_start..style_end];

                        if let Some(span_style) =
                            parse_inline_style(style_spec, base_style, app, theme)
                            && !text_content.is_empty()
                        {
                            spans.push(Span::styled(text_content.to_string(), span_style));
                        }

                        cursor = style_end + 1;
                        continue;
                    }
                }

                spans.push(Span::styled("[".to_string(), base_style));
                cursor = text_start;
            } else {
                spans.push(Span::styled(expanded[cursor..].to_string(), base_style));
                break;
            }
        }

        spans
    }

    fn expand_variables(&self, template: &str, app: &App) -> String {
        let now = chrono::Local::now().format(&self.time_format).to_string();
        let port = &app.serial_config.port;
        let baud = app.serial_config.baudrate.to_string();
        let framing = app.serial_config.to_string();
        let mode = match app.settings().mode {
            Mode::Terminal => "TERM",
            Mode::Line => "LINE",
        };

        let pan_left = pan_left_indicator(app).unwrap_or_default();
        let pan_right = pan_right_indicator(app).unwrap_or_default();

        let scroll = app.scroll_status();
        let scroll_pos = scroll.position_display();
        let scroll_pos_up = scroll.position_display().to_uppercase();
        let scroll_percent = scroll.percent().to_string();
        let scroll_offset = scroll.offset_from_bottom.to_string();
        let scroll_max = scroll.max_scroll.to_string();

        let rx_bytes = app.stats.rx().to_string();
        let tx_bytes = app.stats.tx().to_string();

        template
            .replace("{mode}", mode)
            .replace("{port}", port)
            .replace("{baud}", &baud)
            .replace("{framing}", &framing)
            .replace("{rx_bytes}", &rx_bytes)
            .replace("{tx_bytes}", &tx_bytes)
            .replace("{time}", &now)
            .replace("{pan_left}", &pan_left)
            .replace("{pan_right}", &pan_right)
            .replace("{scroll_pos}", &scroll_pos)
            .replace("{scroll_pos_up}", &scroll_pos_up)
            .replace("{scroll_percent}", &scroll_percent)
            .replace("{scroll_offset}", &scroll_offset)
            .replace("{scroll_max}", &scroll_max)
    }
}

pub fn render_status_bar(app: &App, frame: &mut Frame, area: Rect, theme: &ThemeConfig) {
    if !app.settings().ui.show_status_bar {
        return;
    }

    let sb_cfg = &app.settings().ui.status_bar;
    let base_style = *theme.ui.status_bar;
    frame.render_widget(Block::default().style(base_style), area);

    let left_spans = sb_cfg.parse_to_spans(&sb_cfg.left, app, base_style, theme);
    let center_spans = sb_cfg.parse_to_spans(&sb_cfg.center, app, base_style, theme);
    let right_spans = sb_cfg.parse_to_spans(&sb_cfg.right, app, base_style, theme);

    if !left_spans.is_empty() {
        let left_p = Paragraph::new(Line::from(left_spans)).alignment(Alignment::Left);
        frame.render_widget(left_p, area);
    }

    if !center_spans.is_empty() {
        let center_p = Paragraph::new(Line::from(center_spans)).alignment(Alignment::Center);
        frame.render_widget(center_p, area);
    }

    if !right_spans.is_empty() {
        let right_p = Paragraph::new(Line::from(right_spans)).alignment(Alignment::Right);
        frame.render_widget(right_p, area);
    }
}
