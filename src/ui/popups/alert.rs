use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Wrap},
};

use crate::{configs::ThemeConfig, overlays::ErrorDialog, ui::common::popup_area};

pub fn render_disconnected_popup(port: &str, frame: &mut Frame, theme: &ThemeConfig) {
    let area = popup_area(frame.area(), 48, 8, 40, 6);
    frame.render_widget(Clear, area);

    let text = vec![
        Line::from(Span::styled(
            "⚠ SERIAL DEVICE DISCONNECTED ⚠",
            Style::default()
                .fg(theme.palette.text_warn)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("Device "),
            Span::styled(port, Style::default().fg(theme.palette.secondary)),
            Span::raw(" was closed or unplugged."),
        ]),
        Line::from("Waiting for port to reconnect..."),
        Line::from(""),
        Line::from(Span::styled(
            "Press [ESC] or [ENTER] or [Ctrl+C] to quit",
            Style::default().fg(theme.palette.text_muted),
        )),
    ];

    let popup = Paragraph::new(text)
        .alignment(ratatui::layout::Alignment::Center)
        .block(
            Block::bordered()
                .title(" Connection Lost ")
                .border_style(Style::default().fg(theme.palette.text_warn)),
        );

    frame.render_widget(popup, area);
}

pub fn render_detached_popup(port: &str, frame: &mut Frame, theme: &ThemeConfig) {
    let area = popup_area(frame.area(), 48, 8, 40, 6);
    frame.render_widget(Clear, area);

    let text = vec![
        Line::from(Span::styled(
            "⚠ SERIAL DEVICE DETACHED ⚠",
            Style::default()
                .fg(theme.palette.text_warn)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::raw("Device "),
            Span::styled(port, Style::default().fg(theme.palette.secondary)),
            Span::raw(" was released."),
        ]),
        Line::from("The serial port is free..."),
        Line::from(""),
        Line::from(Span::styled(
            "Press [ENTER] to reattach",
            Style::default().fg(theme.palette.text_muted),
        )),
        Line::from(Span::styled(
            "Press [ESC] or [Ctrl+C] to quit",
            Style::default().fg(theme.palette.text_muted),
        )),
    ];

    let popup = Paragraph::new(text)
        .alignment(ratatui::layout::Alignment::Center)
        .block(
            Block::bordered()
                .title(" Connection Lost ")
                .border_style(Style::default().fg(theme.palette.text_warn)),
        );

    frame.render_widget(popup, area);
}

pub fn render_error_popup(err: &ErrorDialog, frame: &mut Frame, theme: &ThemeConfig) {
    let area = popup_area(frame.area(), 48, 8, 40, 6);
    frame.render_widget(Clear, area);

    let mut lines = vec![
        Line::from(Span::styled(
            format!("⚠  {}  ⚠", err.title),
            Style::default()
                .fg(theme.palette.text_warn)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            &err.summary,
            Style::default().fg(theme.palette.text),
        )),
    ];

    if let Some(ref details) = err.details {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Cause: ", Style::default().fg(theme.palette.text_muted)),
            Span::styled(details, Style::default().fg(theme.palette.secondary)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Press [Esc] or [Enter] to dismiss",
        Style::default().fg(theme.palette.text_muted),
    )));

    let popup = Paragraph::new(lines)
        .alignment(ratatui::layout::Alignment::Center)
        .wrap(Wrap { trim: true })
        .block(
            Block::bordered()
                .title(format!(" {} ", err.title))
                .border_style(Style::default().fg(theme.palette.text_warn)),
        );

    frame.render_widget(popup, area);
}
