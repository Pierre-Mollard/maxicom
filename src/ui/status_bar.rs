use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{app::App, config::InputMode};

pub fn render_status_bar(app: &App, frame: &mut Frame, area: Rect) {
    let bar_style = Style::default().bg(Color::Rgb(30, 35, 45)).fg(Color::White);
    let highlight_style = Style::default()
        .bg(Color::Rgb(50, 70, 110))
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let alert_style = Style::default()
        .bg(Color::Red)
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let dim_style = Style::default().fg(Color::DarkGray);

    let port_str = &app.serial_config.port;
    let baud = app.serial_config.baudrate;

    let mut spans = vec![
        Span::styled(format!(" {} ", port_str), highlight_style),
        Span::styled("│", dim_style),
        Span::styled(format!(" {} baud ", baud), bar_style),
        Span::styled(format!(" {} ", app.serial_config), bar_style),
        Span::styled("│", dim_style),
    ];

    let mode_str = match app.config().input_mode {
        InputMode::Terminal => " TERMINAL ",
        InputMode::Line => " LINE ",
    };
    spans.push(Span::styled(mode_str, Style::default().fg(Color::Cyan)));
    spans.push(Span::styled("│", dim_style));

    if app.is_logging_active() {
        spans.push(Span::styled(" REC ", alert_style));
        spans.push(Span::styled("│", dim_style));
    }

    let current_len: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let right_text = "Press Ctrl-A Z for help ";
    let right_len = right_text.chars().count();

    let total_width = area.width as usize;
    let padding_count = total_width.saturating_sub(current_len + right_len);

    spans.push(Span::raw(" ".repeat(padding_count)));
    spans.push(Span::styled(right_text, Style::default().fg(Color::Gray)));

    let status_line = Line::from(spans);
    let status_widget = Paragraph::new(status_line).style(bar_style);

    frame.render_widget(status_widget, area);
}
