/// View, render screen from state (app)
/// Using the 'ratatui' crate
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::app::App;

pub fn render(app: &App, frame: &mut Frame) {
    let chunks = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(frame.area());

    let mut lines: Vec<Line> = Vec::new();

    for history_line in app.history() {
        lines.push(Line::from(vec![
            Span::styled("| ", Style::default().fg(Color::DarkGray)),
            Span::raw(history_line),
        ]));
    }

    if app.display_partial_incoming() && !app.partial_rx().is_empty() {
        let partial_str = String::from_utf8_lossy(app.partial_rx());
        lines.push(Line::from(vec![
            Span::styled("|p ", Style::default().fg(Color::Yellow)),
            Span::styled(partial_str, Style::default().fg(Color::Yellow)),
        ]));
    }

    let history_paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" MaxiCom Monitor "),
        )
        .wrap(Wrap { trim: false });

    // Render history in the top chunk
    frame.render_widget(history_paragraph, chunks[0]);

    let input_text = Line::from(vec![
        Span::styled(
            "> ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(app.input_buffer()),
    ]);

    let input_widget = Paragraph::new(input_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Transmit (TX) "),
    );

    // Render input in the bottom chunk
    frame.render_widget(input_widget, chunks[1]);
}
