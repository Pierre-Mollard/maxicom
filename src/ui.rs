/// View, render screen from state (app)
/// Using the 'ratatui' crate
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

use crate::{
    app::App,
    config::{InputMode, WrapMode},
};

pub fn render(app: &mut App, frame: &mut Frame) {
    let screen_area = frame.area();

    let (monitor_area, prompt_area): (Rect, Option<Rect>) = match app.config().input_mode {
        InputMode::Raw => {
            // Raw passthrough mode: monitor owns the entire window
            (screen_area, None)
        }
        InputMode::Line => {
            // Line mode: split into monitor (top) and input prompt (bottom)
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(3)])
                .split(screen_area);

            (chunks[0], Some(chunks[1]))
        }
    };

    render_monitor(app, frame, monitor_area);

    // 3. Render the Input Box (ONLY if in Line Mode)
    if let Some(prompt_rect) = prompt_area {
        render_input_prompt(app, frame, prompt_rect);
    }
}

fn render_monitor(app: &mut App, frame: &mut Frame, area: Rect) {
    let visible_height = area.height.saturating_sub(2) as usize;
    let total_lines = app.history().len();

    app.clamp_scroll(visible_height);

    let max_scroll = total_lines.saturating_sub(visible_height);
    let scroll_offset = app.scroll_offset().min(max_scroll);

    let start_idx = max_scroll.saturating_sub(scroll_offset);
    let end_idx = (start_idx + visible_height).min(total_lines);

    let mut lines: Vec<Line> = Vec::with_capacity(visible_height + 1);

    for line in app
        .history()
        .iter()
        .skip(start_idx)
        .take(end_idx - start_idx)
    {
        // TODO: custom outisde and inside (|)
        lines.push(Line::from(vec![
            Span::styled("| ", Style::default().fg(Color::DarkGray)),
            Span::raw(line.as_str()),
        ]));
    }

    // Only show partial incoming bytes if the user is looking at the live bottom (scroll_offset == 0)
    if scroll_offset == 0 && app.display_partial_incoming() && !app.partial_rx().is_empty() {
        let partial_str = String::from_utf8_lossy(app.partial_rx());
        lines.push(Line::from(vec![
            Span::styled("|p ", Style::default().fg(Color::Yellow)),
            Span::styled(partial_str, Style::default().fg(Color::Yellow)),
        ]));
    }

    let mode_str = match app.config().input_mode {
        InputMode::Line => "LINE",
        InputMode::Raw => "RAW",
    };

    let title = if scroll_offset > 0 {
        format!(" MaxiCom Monitor [{mode_str} | SCROLL: +{scroll_offset} lines] ")
    } else {
        format!(" MaxiCom Monitor [{mode_str} | LIVE] ")
    };

    let mut history_paragraph =
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title));

    history_paragraph = match app.config().wrap_mode {
        WrapMode::Wrap => history_paragraph.wrap(Wrap { trim: false }),
        WrapMode::Truncate => history_paragraph,
    };

    frame.render_widget(history_paragraph, area);

    if total_lines > visible_height {
        let mut scrollbar_state = ScrollbarState::new(max_scroll).position(start_idx);

        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .track_symbol(Some("│"))
            .thumb_symbol("█");

        frame.render_stateful_widget(
            scrollbar,
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
}

fn render_input_prompt(app: &App, frame: &mut Frame, area: Rect) {
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

    frame.render_widget(input_widget, area);

    // Place physical blinking cursor at the end of what the user is typing
    let cursor_x = area.x + 3 + app.input_buffer().len() as u16;
    let cursor_y = area.y + 1;
    frame.set_cursor_position((cursor_x, cursor_y));
}
