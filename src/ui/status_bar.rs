use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};

use crate::{
    app::App,
    configs::{InputMode, StatusBarConfig, ThemeConfig},
};

pub fn parse_inline_style(spec: &str, base_style: Style) -> Style {
    let mut style = base_style;

    for part in spec.split_whitespace() {
        let part_lower = part.to_ascii_lowercase();
        match part_lower.as_str() {
            "bold" => style = style.add_modifier(Modifier::BOLD),
            "dim" => style = style.add_modifier(Modifier::DIM),
            "italic" => style = style.add_modifier(Modifier::ITALIC),
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

    style
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

                        let span_style = parse_inline_style(style_spec, base_style);
                        spans.push(Span::styled(text_content.to_string(), span_style));

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
        let rec = if app.is_logging_active() {
            " REC "
        } else {
            ""
        };
        let prefix = if app.is_prefix_active() { "󰒓 " } else { "" };

        let rx_bytes = app.stats.rx().to_string();
        let tx_bytes = app.stats.tx().to_string();

        template
            .replace("{rec}", rec)
            .replace("{prefix}", prefix)
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
