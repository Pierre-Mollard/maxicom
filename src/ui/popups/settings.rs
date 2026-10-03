use ratatui::{
    Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    app::App,
    configs::ThemeConfig,
    types::FocusTarget,
    ui::common::{border_style_for, centered_rect},
};

pub fn render_settings_popup(app: &App, frame: &mut Frame, theme: &ThemeConfig) {
    let area = centered_rect(55, 11, frame.area());
    frame.render_widget(Clear, area);

    let mode_str = match app.settings().input_mode {
        crate::configs::InputMode::Line => "Line Mode",
        crate::configs::InputMode::Terminal => "Terminal Mode",
    };
    let wrap_str = match app.settings().line_mode.wrap_mode {
        crate::configs::WrapMode::Wrap => "Soft-Wrap",
        crate::configs::WrapMode::Truncate => "Truncate",
    };
    let echo_str = if app.settings().local_echo {
        "ON"
    } else {
        "OFF"
    };
    let timestamp_str = if app.settings().display_live_timestamp {
        "ON"
    } else {
        "OFF"
    };
    let encoding_str = app.settings().encoding.as_str();

    let text = vec![
        Line::from(Span::styled(
            "--- Configuration Dashboard ---",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("[M] ", Style::default().fg(Color::Yellow)),
            Span::raw("Input Mode  : "),
            Span::styled(mode_str, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("[E] ", Style::default().fg(Color::Yellow)),
            Span::raw("Local Echo  : "),
            Span::styled(echo_str, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("[W] ", Style::default().fg(Color::Yellow)),
            Span::raw("Line Wrap   : "),
            Span::styled(wrap_str, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("[T] ", Style::default().fg(Color::Yellow)),
            Span::raw("Timestamps  : "),
            Span::styled(timestamp_str, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("[X] ", Style::default().fg(Color::Yellow)),
            Span::raw("Rx Encoding : "),
            Span::styled(encoding_str, Style::default().fg(Color::Cyan)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Press [Esc] to return to terminal",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let popup = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style_for(
                FocusTarget::Overlay,
                app.focus_target(),
                app.is_app_focused(),
                theme,
            ))
            .title(" Settings [Toggle Hotkeys] ")
            .style(Style::default().fg(Color::Green)),
    );

    frame.render_widget(popup, area);
}
