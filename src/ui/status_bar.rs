use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Paragraph},
};

use crate::{
    app::App,
    configs::{InputMode, StatusBarConfig, ThemeConfig},
};

impl StatusBarConfig {
    pub fn format_template(&self, template: &str, app: &App) -> String {
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

    // TODO: implement center? and inline style

    let sb_cfg = &app.settings().ui.status_bar;
    let left_text = sb_cfg.format_template(&sb_cfg.left, app);
    let right_text = sb_cfg.format_template(&sb_cfg.right, app);

    let bar_style = theme.status_bar_bg;
    frame.render_widget(Block::default().style(bar_style), area);

    let left_paragraph = Paragraph::new(left_text).style(bar_style);
    frame.render_widget(left_paragraph, area);

    let right_paragraph = Paragraph::new(right_text)
        .alignment(ratatui::layout::Alignment::Right)
        .style(bar_style);
    frame.render_widget(right_paragraph, area);
}
