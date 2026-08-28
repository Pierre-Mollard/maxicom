/// View, render screen from state (app)
/// Using the 'ratatui' crate
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

use crate::{app::App, config::WrapMode};

pub fn render(app: &App, frame: &mut Frame) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(frame.area());

    let monitor_area = chunks[0];
    let visible_height = monitor_area.height.saturating_sub(2) as usize; // remove top/bottom borders
    let total_lines = app.history().len();

    let end_idx = total_lines.saturating_sub(app.scroll_offset());
    let start_idx = end_idx.saturating_sub(visible_height);

    let mut lines: Vec<Line> = Vec::with_capacity(visible_height + 1);

    for line in app
        .history()
        .iter()
        .skip(start_idx)
        .take(end_idx - start_idx)
    {
        lines.push(Line::from(vec![
            Span::styled("| ", Style::default().fg(Color::DarkGray)),
            Span::raw(line.as_str()),
        ]));
    }

    // Only show partial incoming bytes if the user is looking at the live bottom (scroll_offset == 0)
    if app.scroll_offset() == 0 && app.display_partial_incoming() && !app.partial_rx().is_empty() {
        let partial_str = String::from_utf8_lossy(app.partial_rx());
        lines.push(Line::from(vec![
            Span::styled("|p ", Style::default().fg(Color::Yellow)),
            Span::styled(partial_str, Style::default().fg(Color::Yellow)),
        ]));
    }

    // TODO: adust scrollbar behavior/sensi
    let title = if app.scroll_offset() > 0 {
        format!(" MaxiCom Monitor [SCROLL: +{} lines] ", app.scroll_offset())
    } else {
        " MaxiCom Monitor [LIVE] ".to_string()
    };

    let mut history_paragraph =
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title));

    history_paragraph = match app.config().wrap_mode {
        WrapMode::Wrap => history_paragraph.wrap(Wrap { trim: false }),
        WrapMode::Truncate => history_paragraph,
    };

    frame.render_widget(history_paragraph, monitor_area);

    if total_lines > visible_height {
        let mut scrollbar_state = ScrollbarState::new(total_lines)
            .position(start_idx)
            .viewport_content_length(visible_height);

        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .track_symbol(Some("│"))
            .thumb_symbol("█");

        frame.render_stateful_widget(
            scrollbar,
            monitor_area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }

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

    frame.render_widget(input_widget, chunks[1]);
}
