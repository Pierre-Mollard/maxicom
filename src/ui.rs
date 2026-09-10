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

use std::fmt::Write;

use crate::{
    app::App,
    config::{InputMode, WrapMode},
    terminal::MaxicomTerminal,
    types::{Overlay, ViewportSize},
};

//TODO: move this module around when adding terminal stream saving feature
use crate::app::line_mode::LogEntry; // this is ugly deps i find

pub fn render(app: &mut App, frame: &mut Frame) {
    let screen_area = frame.area();

    let (monitor_area, prompt_area): (Rect, Option<Rect>) = match app.config().input_mode {
        InputMode::Terminal => (screen_area, None),
        InputMode::Line => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(3)])
                .split(screen_area);

            (chunks[0], Some(chunks[1]))
        }
    };

    let mut block = Block::default().borders(Borders::ALL);
    let inner = block.inner(monitor_area);
    app.update_viewport(ViewportSize::new(inner.width, inner.height));

    let title = monitor_title(app, &monitor_area);
    block = block.title(title);

    let bottom_titles = monitor_bottom_title(app);
    block = bottom_titles
        .into_iter()
        .fold(block, |b, t| b.title_bottom(t));

    match app.config().input_mode {
        InputMode::Terminal => {
            render_vt100_terminal(app, frame, monitor_area, block);
        }
        InputMode::Line => {
            render_monitor(app, frame, monitor_area, block);
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

fn monitor_title(app: &App, area: &Rect) -> String {
    let mut title = String::with_capacity(80);

    let (scroll_offset, pan_offset) = match app.config().input_mode {
        InputMode::Terminal => {
            let screen = app.terminal_mode.vt100_screen();
            let _ = write!(
                title,
                " MaxiCom TERMINAL [VT {}x{} | VIEW {}x{}",
                screen.size().1,
                screen.size().0,
                area.width.saturating_sub(2),
                area.height.saturating_sub(2),
            );
            (
                app.terminal_mode.vt100_screen().scrollback(),
                app.terminal_mode.pan_offset(),
            )
        }
        InputMode::Line => {
            let _ = write!(
                title,
                " MaxiCom LINE [VIEW {}x{}",
                area.width.saturating_sub(2),
                area.height.saturating_sub(2),
            );
            (app.line_mode.scroll_offset(), app.line_mode.x_offset())
        }
    };

    // Append scroll state
    if scroll_offset > 0 {
        let _ = write!(title, " | SCROLL +{scroll_offset}");
    } else {
        title.push_str(" | LIVE");
    }

    // Append pan state
    if pan_offset > 0 {
        let _ = write!(title, " | PAN +{pan_offset}");
    }

    title.push(']');

    if app.is_prefix_active() {
        title.push_str(" | PREFIX");
    }

    title.push(' ');
    title
}

fn monitor_bottom_title<'a>(app: &App) -> Vec<Line<'a>> {
    let mut titles = Vec::with_capacity(2);
    let arrow_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);

    match app.config().input_mode {
        InputMode::Terminal => {
            if let Some(pan_left_possible) = app.terminal_mode.can_pan_left() {
                titles.push(
                    Line::from(vec![
                        Span::styled(" ◀ (", arrow_style),
                        Span::styled(pan_left_possible.to_string(), arrow_style),
                        Span::styled(")", arrow_style),
                    ])
                    .left_aligned(),
                );
            }
            if let Some(pan_right_possible) = app.terminal_mode.can_pan_right() {
                titles.push(
                    Line::from(vec![
                        Span::styled("(", arrow_style),
                        Span::styled(pan_right_possible.to_string(), arrow_style),
                        Span::styled(") ▶ ", arrow_style),
                    ])
                    .right_aligned(),
                );
            }
        }
        InputMode::Line => {
            if let Some(pan_left_possible) = app.line_mode.can_pan_left() {
                titles.push(
                    Line::from(vec![
                        Span::styled(" ◀ (", arrow_style),
                        Span::styled(pan_left_possible.to_string(), arrow_style),
                        Span::styled(")", arrow_style),
                    ])
                    .left_aligned(),
                );
            }
            if let Some((pan_right_possible, pan_explored)) = app.line_mode.can_pan_right()
                && pan_right_possible > 0
            {
                let to_discover_alert =
                    if pan_right_possible > pan_explored.saturating_sub(app.line_mode.x_offset()) {
                        "!"
                    } else {
                        ""
                    };
                titles.push(
                    Line::from(vec![
                        Span::styled(to_discover_alert, arrow_style),
                        Span::styled("(", arrow_style),
                        Span::styled(pan_right_possible.to_string(), arrow_style),
                        Span::styled(") ▶ ", arrow_style),
                    ])
                    .right_aligned(),
                );
            }
        }
    }
    titles
}

// TODO: see if adding horizontal scrollbar for panning (and in line mode too)
fn render_vt100_terminal(app: &mut App, frame: &mut Frame, area: Rect, block: Block) {
    let inner = block.inner(area);
    let view_height = inner.height as usize;

    let term_widget = MaxicomTerminal::new(app.terminal_mode.vt100_screen())
        .x_offset(app.terminal_mode.pan_offset_x() as u16)
        .y_offset(app.terminal_mode.pan_offset_y() as u16)
        .block(block)
        .filling_char(&app.config().empty_fill_char);

    frame.render_widget(term_widget, area);

    let max_vertical = app.terminal_mode.max_vertical_scroll();
    if max_vertical > 0 && view_height > 0 {
        let position_from_top = max_vertical.saturating_sub(app.terminal_mode.scrollback_offset());

        let mut scrollbar_state = ScrollbarState::new(max_vertical)
            .viewport_content_length(view_height)
            .position(position_from_top);

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

    // Cusor
    if !app.terminal_mode.vt100_screen().hide_cursor() {
        let (c_row, c_col) = app.terminal_mode.vt100_screen().cursor_position();

        let local_col = c_col.checked_sub(app.terminal_mode.pan_offset_x() as u16);
        let local_row = c_row.checked_sub(app.terminal_mode.pan_offset_y() as u16);

        if let (Some(col), Some(row)) = (local_col, local_row)
            && col < inner.width
            && row < inner.height
        {
            frame.set_cursor_position((inner.x + col, inner.y + row));
        }
    }
}

fn history_entry_to_line(entry: &LogEntry, timestamp_in_line: bool) -> Line<'static> {
    let mut spans = Vec::new();

    if timestamp_in_line {
        let time_str = entry.formatted_time();
        spans.push(Span::styled(time_str, Style::default().fg(Color::Yellow)));
    }

    // TODO: custom outside and inside (|)
    spans.push(Span::styled("| ", Style::default().fg(Color::DarkGray)));
    spans.push(Span::raw(entry.text.clone()));
    Line::from(spans)
}

// PERF: do not rebuild everything every frame
fn render_monitor(app: &mut App, frame: &mut Frame, area: Rect, block: Block) {
    let timestamp_in_line = app.config().timestamp_in_line;
    let inner = block.inner(area);
    let content_width = inner.width as usize;
    let content_height = inner.height as usize;
    let wrap_mode = app.config().wrap_mode;

    let mut history_lines: Vec<Line> = app
        .line_mode
        .history()
        .iter()
        .map(|entry| history_entry_to_line(entry, timestamp_in_line))
        .collect();

    if app.config().display_partial_rx && !app.line_mode.partial_rx().is_empty() {
        let partial_str = String::from_utf8_lossy(app.line_mode.partial_rx()).into_owned();
        let mut partial_spans = Vec::new();

        if app.config().timestamp_in_line
            && let Some(partial_timestamp) = app.line_mode.partial_line_timestamp()
        {
            let time = LogEntry::formatted_time_builder(partial_timestamp);
            partial_spans.push(Span::styled(time, Style::default().fg(Color::Yellow)));
        }

        // Prefix with a subtle indicator or standard pipe
        partial_spans.push(Span::styled("|p ", Style::default().fg(Color::DarkGray)));

        partial_spans.push(Span::styled(partial_str, Style::default().fg(Color::White)));

        history_lines.push(Line::from(partial_spans));
    }

    let line_x_offset = match wrap_mode {
        WrapMode::Wrap => 0,
        WrapMode::Truncate => app.line_mode.x_offset(),
    };
    let history_line_count = history_lines.len();
    let mut history_paragraph = Paragraph::new(history_lines).block(block);

    history_paragraph = match wrap_mode {
        WrapMode::Wrap => history_paragraph.wrap(Wrap { trim: false }),
        WrapMode::Truncate => history_paragraph,
    };

    // WARN: Paragraph.line_width and .line_count are experiemental deps
    let max_pan = match wrap_mode {
        WrapMode::Wrap => 0, //unused so no need to process
        WrapMode::Truncate => history_paragraph.line_width().saturating_sub(content_width),
    };
    let block_vertical_space = area.height.saturating_sub(inner.height) as usize;
    let total_visual_rows = match wrap_mode {
        WrapMode::Wrap => history_paragraph
            .line_count(content_width as u16)
            .saturating_sub(block_vertical_space),
        WrapMode::Truncate => history_line_count,
    };

    let max_scroll = total_visual_rows.saturating_sub(content_height);

    app.line_mode.update_limits(max_scroll, max_pan);

    let scroll_from_bottom = app.line_mode.scroll_offset();
    let paragraph_y_offset = max_scroll.saturating_sub(scroll_from_bottom);

    history_paragraph = history_paragraph.scroll((
        paragraph_y_offset.min(u16::MAX as usize) as u16,
        line_x_offset.min(u16::MAX as usize) as u16,
    ));
    frame.render_widget(history_paragraph, area);

    if total_visual_rows > content_height {
        let mut scrollbar_state = ScrollbarState::new(max_scroll)
            .viewport_content_length(content_height)
            .position(paragraph_y_offset);

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
        Span::raw(app.line_mode.input_buffer()),
    ]);

    let input_widget = Paragraph::new(input_text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Transmit (TX) "),
    );

    frame.render_widget(input_widget, area);

    // Place physical blinking cursor at the end of what the user is typing
    let cursor_x = area.x + 3 + app.line_mode.input_buffer().len() as u16;
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
