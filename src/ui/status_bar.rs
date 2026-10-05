//! # Status Bar Formatting Engine
//!
//! The status bar supports dynamic templating, Starship-style styling,
//! conditional ternaries, and element visibility toggling.
//!
//! ## Syntax
//! - **Plain Text**: Rendered using default theme colors.
//! - **Variables**: `{variable_name}` replaced at runtime.
//! - **Styled Blocks**: `[content](style_spec)`
//! - **Conditionals**: `(condition?if_true:if_false)`
//!
//! ## Available Variables
//! | Variable     | Description                         | Example          |
//! |--------------|-------------------------------------|------------------|
//! | `{mode}`     | Current input mode                  | `TERM`           |
//! | `{port}`     | Serial device path                  | `/dev/ttyUSB0`   |
//! | `{baud}`     | Current baudrate                    | `115200`         |
//! | `{framing}`  | Data bits, parity, stop bits        | `8N1`            |
//! | `{rx_bytes}` | Total bytes received                | `1024`           |
//! | `{tx_bytes}` | Total bytes transmitted             | `256`            |
//! | `{time}`     | Current local time, format in conf  | `21:45:00`       |
//!
//! ## Conditional States (`?`)
//! | State      | Matches When...                                              |
//! |------------|--------------------------------------------------------------|
//! | `prefix`   | Leader prefix key (`Ctrl-A`) is currently active             |
//! | `rec`      | Session is currently logged in a file                        |
//! | `line`     | App is in Line Input mode                                    |
//! | `term`     | App is in Terminal Emulation mode                            |
//! | `copy`     | App is in Copy / Scrollback mode                             |
//! | `search`   | App is in Search mode                                        |
//! | `overlay`  | A dialog or settings popup is currently open                 |
//! | `focused`  | Terminal window has OS desktop focus                         |
//!
//! ## Style Keywords
//! - **Modifiers**: `bold`, `dim`, `italic`, `underlined`, `reversed`, `reset`, `crossed`, `slow_blink`, `rapid_blink`
//! - **Visibility**: `hide` / `hidden` / `none`
//! - **Colors**: `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`,
//!   `gray`, `darkgray`, `white`, or hex `#RRGGBB`
//! - **Prefixes**: `fg=<color>`, `bg=<color>`
//!
//! ## Examples
//! ```toml
//! [ui.status_bar]
//! # Hide REC when idle, show bright red when recording:
//! left = "[  REC ](rec?red bold:hide)[ {mode} ](prefix?yellow:cyan bold) {port} | [{baud}](yellow)"
//!
//! # Chain multiple modes:
//! left = "[ {mode} ](prefix?yellow:copy?magenta:search?blue:line?green:cyan bold)"
//!
//! # Right-aligned counters and blue timestamp:
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
    configs::{InputMode, StatusBarConfig, ThemeConfig},
};

pub fn parse_inline_style(spec: &str, base_style: Style, app: &App) -> Option<Style> {
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
                _ => {
                    if let Some(c) = part.strip_prefix("fg=") {
                        if let Ok(color) = c.parse::<Color>() {
                            style = style.fg(color);
                        }
                    } else if let Some(c) = part.strip_prefix("bg=") {
                        if let Ok(color) = c.parse::<Color>() {
                            style = style.bg(color);
                        }
                    } else if let Ok(color) = part.parse::<Color>() {
                        style = style.fg(color);
                    }
                }
            }
        }
    }

    Some(style)
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
    match cond.trim().to_ascii_lowercase().as_str() {
        "prefix" => ctx == InputContext::Prefix || app.is_prefix_active(),
        "copy" | "copymode" => ctx == InputContext::CopyMode || app.is_copy_mode_active(),
        "search" => ctx == InputContext::Search || app.is_search_active(),
        "line" | "linemode" => ctx == InputContext::LineInput,
        "term" | "terminal" => ctx == InputContext::Terminal,
        "overlay" | "popup" => ctx == InputContext::Overlay || app.overlay().is_some(),
        "rec" | "logging" => app.is_logging_active(),
        "focused" => app.is_app_focused(),

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

                        if let Some(span_style) = parse_inline_style(style_spec, base_style, app)
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
        let mode = match app.settings().input_mode {
            InputMode::Terminal => "TERM",
            InputMode::Line => "LINE",
        };

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
    }
}

pub fn render_status_bar(app: &App, frame: &mut Frame, area: Rect, theme: &ThemeConfig) {
    if !app.settings().ui.show_status_bar {
        return;
    }

    let sb_cfg = &app.settings().ui.status_bar;
    let base_style = *theme.status_bar_bg;
    frame.render_widget(Block::default().style(base_style), area);

    let left_spans = sb_cfg.parse_to_spans(&sb_cfg.left, app, base_style);
    let center_spans = sb_cfg.parse_to_spans(&sb_cfg.center, app, base_style);
    let right_spans = sb_cfg.parse_to_spans(&sb_cfg.right, app, base_style);

    if !left_spans.is_empty() {
        let left_p = Paragraph::new(Line::from(left_spans))
            .alignment(Alignment::Left)
            .style(base_style);
        frame.render_widget(left_p, area);
    }

    if !center_spans.is_empty() {
        let center_p = Paragraph::new(Line::from(center_spans))
            .alignment(Alignment::Center)
            .style(base_style);
        frame.render_widget(center_p, area);
    }

    if !right_spans.is_empty() {
        let right_p = Paragraph::new(Line::from(right_spans))
            .alignment(Alignment::Right)
            .style(base_style);
        frame.render_widget(right_p, area);
    }
}
