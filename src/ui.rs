/// View, render screen from state (app)
/// Using the 'ratatui' crate
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
    },
};
use tui_term::widget::PseudoTerminal;

use crate::{
    app::{App, LogEntry, Overlay},
    config::{InputMode, WrapMode},
};

pub fn render(app: &mut App, frame: &mut Frame) {
    let screen_area = frame.area();

    let (monitor_area, prompt_area): (Rect, Option<Rect>) = match app.config().input_mode {
        InputMode::Terminal => {
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

    match app.config().input_mode {
        InputMode::Terminal => {
            let inner_height = monitor_area.height.saturating_sub(2);
            let inner_width = monitor_area.width.saturating_sub(2);
            app.resize_vt100(inner_height, inner_width);

            render_vt100_terminal(app, frame, monitor_area);
        }
        InputMode::Line => {
            render_monitor(app, frame, monitor_area);
        }
    }

    // 3. Render the Input Box (ONLY if in Line Mode)
    if let Some(prompt_rect) = prompt_area {
        render_input_prompt(app, frame, prompt_rect);
    }

    match app.overlay() {
        Some(Overlay::Help) => render_help_popup(frame),
        Some(Overlay::Settings) => render_settings_popup(app, frame),
        None => {}
    }
}

fn render_vt100_terminal(app: &App, frame: &mut Frame, area: Rect) {
    let scroll_offset = app.vt100_parser.screen().scrollback();

    let mut title = if scroll_offset > 0 {
        format!(" MaxiCom [TERMINAL | SCROLL: +{} lines] ", scroll_offset)
    } else {
        " MaxiCom [TERMINAL | LIVE] ".to_string()
    };
    if app.is_prefix_active() {
        title.push_str(" PREFIX");
    }

    let screen = app.vt100_screen();
    let term_widget =
        PseudoTerminal::new(screen).block(Block::default().borders(Borders::ALL).title(title));

    // TODO: add a way to count the line number history to display a scrollbar

    frame.render_widget(term_widget, area);
}

// NOTE: AI generated, make it better
fn render_monitor(app: &mut App, frame: &mut Frame, area: Rect) {
    let visible_height = area.height.saturating_sub(2) as usize;
    let total_lines = app.history().len();

    app.clamp_scroll(visible_height);

    let max_scroll = total_lines.saturating_sub(visible_height);
    let scroll_offset = app.scroll_offset().min(max_scroll);

    let start_idx = max_scroll.saturating_sub(scroll_offset);
    let end_idx = (start_idx + visible_height).min(total_lines);

    let mut lines: Vec<Line> = Vec::with_capacity(visible_height + 1);

    for entry in app
        .history()
        .iter()
        .skip(start_idx)
        .take(end_idx - start_idx)
    {
        // TODO: custom outisde and inside (|)

        let mut current_line = Vec::new();

        if app.config().timestamp_in_line {
            let time_str = entry.formatted_time();
            current_line.push(Span::styled(time_str, Style::default().fg(Color::Yellow)));
        }

        current_line.push(Span::styled("| ", Style::default().fg(Color::DarkGray)));
        current_line.push(Span::raw(entry.text.as_str()));
        lines.push(Line::from(current_line));
    }

    // Only show partial incoming bytes if the user is looking at the live bottom (scroll_offset == 0)
    if scroll_offset == 0 && app.display_partial_incoming() && !app.partial_rx().is_empty() {
        let partial_str = String::from_utf8_lossy(app.partial_rx());
        let mut partial_spans = Vec::new();

        if app.config().timestamp_in_line
            && let Some(partial_timestamp) = app.partial_line_timestamp()
        {
            let time = LogEntry::formatted_time_builder(partial_timestamp);
            partial_spans.push(Span::styled(time, Style::default().fg(Color::Yellow)));
        }

        // Prefix with a subtle indicator or standard pipe
        partial_spans.push(Span::styled("|p ", Style::default().fg(Color::DarkGray)));

        partial_spans.push(Span::styled(partial_str, Style::default().fg(Color::White)));

        lines.push(Line::from(partial_spans));
    }

    let mode_str = match app.config().input_mode {
        InputMode::Line => "LINE_MODE",
        InputMode::Terminal => "TERMINAL_MODE",
    };

    let mut title = if scroll_offset > 0 {
        format!(" MaxiCom Monitor [{mode_str} | SCROLL: +{scroll_offset} lines] ")
    } else {
        format!(" MaxiCom Monitor [{mode_str} | LIVE] ")
    };

    if app.is_prefix_active() {
        title.push_str(" PREFIX");
    }

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

// NOTE: AI generated, make it better
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

// NOTE: AI generated, make it better
fn render_help_popup(frame: &mut Frame) {
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
            .title(" Help [Keybindings] ")
            .style(Style::default().fg(Color::White)),
    );

    frame.render_widget(popup, area);
}

// NOTE: AI generated, make it better
fn render_settings_popup(app: &App, frame: &mut Frame) {
    let area = centered_rect(55, 12, frame.area());
    frame.render_widget(Clear, area);

    let mode_str = match app.config().input_mode {
        crate::config::InputMode::Line => "Line Mode",
        crate::config::InputMode::Terminal => "Terminal Mode",
    };
    let wrap_str = match app.config().wrap_mode {
        crate::config::WrapMode::Wrap => "Soft-Wrap",
        crate::config::WrapMode::Truncate => "Truncate",
    };
    let echo_str = if app.config().local_echo { "ON" } else { "OFF" };

    // TODO: add badge when recording is on
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
        Line::from(""),
        Line::from(Span::styled(
            "Press [Esc] to return to terminal",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let popup = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Settings [Toggle Hotkeys] ")
            .style(Style::default().fg(Color::Green)),
    );

    frame.render_widget(popup, area);
}

/// Helper function to center a popup window
fn centered_rect(percent_x: u16, height: u16, parent: Rect) -> Rect {
    let width = (parent.width * percent_x) / 100;
    let x = parent.x + parent.width.saturating_sub(width) / 2;
    let y = parent.y + parent.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(parent.width), height.min(parent.height))
}
