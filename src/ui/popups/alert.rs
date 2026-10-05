use ratatui::{
    Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Wrap},
};

use crate::{overlays::ErrorDialog, ui::common::centered_rect};

pub fn render_disconnected_popup(port: &str, frame: &mut Frame) {
    let area = centered_rect(54, 9, frame.area());
    frame.render_widget(Clear, area);

    let text = vec![
        Line::from(Span::styled(
            "⚠ SERIAL DEVICE DISCONNECTED ⚠",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("Device "),
            Span::styled(port, Style::default().fg(Color::Yellow)),
            Span::raw(" was closed or unplugged."),
        ]),
        Line::from("Waiting for port to reconnect..."),
        Line::from(""),
        Line::from(Span::styled(
            "Press [ESC] or [ENTER] or [Ctrl+C] to quit",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let popup = Paragraph::new(text)
        .alignment(ratatui::layout::Alignment::Center)
        .block(
            Block::bordered()
                .title(" Connection Lost ")
                .border_style(Style::default().fg(Color::Red)),
        );

    frame.render_widget(popup, area);
}

pub fn render_error_popup(err: &ErrorDialog, frame: &mut Frame) {
    let area = centered_rect(58, 10, frame.area());
    frame.render_widget(Clear, area);

    let mut lines = vec![
        Line::from(Span::styled(
            format!("⚠  {}  ⚠", err.title),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            &err.summary,
            Style::default().fg(Color::White),
        )),
    ];

    if let Some(ref details) = err.details {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Cause: ", Style::default().fg(Color::DarkGray)),
            Span::styled(details, Style::default().fg(Color::Yellow)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Press [Esc] or [Enter] to dismiss",
        Style::default().fg(Color::DarkGray),
    )));

    let popup = Paragraph::new(lines)
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true })
        .block(
            Block::bordered()
                .title(format!(" {} ", err.title))
                .border_style(Style::default().fg(Color::Red)),
        );

    frame.render_widget(popup, area);
}
