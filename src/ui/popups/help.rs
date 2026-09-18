use ratatui::{
    Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    app::App,
    types::FocusTarget,
    ui::common::{border_style_for, centered_rect},
};

// NOTE: AI generated, make it better
pub fn render_help_popup(app: &App, frame: &mut Frame) {
    let area = centered_rect(60, 14, frame.area());
    frame.render_widget(Clear, area); // Blank background

    let help_text = vec![
        Line::from(Span::styled(
            "--- MaxiCom Command Keys (Prefix: Ctrl-A) ---",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Ctrl-A H / Z  ", Style::default().fg(Color::Yellow)),
            Span::raw(": Show this help screen"),
        ]),
        Line::from(vec![
            Span::styled("Ctrl-A O / S  ", Style::default().fg(Color::Yellow)),
            Span::raw(": Open settings menu"),
        ]),
        Line::from(vec![
            Span::styled("Ctrl-A M      ", Style::default().fg(Color::Yellow)),
            Span::raw(": Toggle Line / Raw mode"),
        ]),
        Line::from(vec![
            Span::styled("Ctrl-A V", Style::default().fg(Color::Yellow)),
            Span::raw(": Auto set local emulator terminal size"),
        ]),
        Line::from(vec![Span::styled(
            "May loose data history current of screen",
            Style::default().fg(Color::Red),
        )])
        .right_aligned(),
        Line::from(vec![
            Span::styled("Ctrl-A T", Style::default().fg(Color::Yellow)),
            Span::raw(": Auto set remote bash terminal size"),
        ]),
        Line::from(vec![Span::styled(
            "Remote must be BASH SHELL",
            Style::default().fg(Color::Red),
        )])
        .right_aligned(),
        Line::from(vec![
            Span::styled("Ctrl-A Q      ", Style::default().fg(Color::Yellow)),
            Span::raw(": Quit MaxiCom"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "--- Navigation ---",
            Style::default().fg(Color::Cyan),
        )),
        Line::from("PageUp / PageDown : Scroll serial history"),
        Line::from("Shift + Home/End  : Jump to start / live bottom"),
        Line::from(""),
        Line::from(Span::styled(
            "Press any key to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let popup = Paragraph::new(help_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style_for(
                FocusTarget::Overlay,
                app.focus_target(),
                app.is_app_focused(),
            ))
            .title(" Help [Keybindings] ")
            .style(Style::default().fg(Color::White)),
    );

    frame.render_widget(popup, area);
}
