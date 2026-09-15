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
    types::{ErrorDialog, FocusTarget, Overlay, ResizeDialog, ResizeField, ViewportSize},
};

//TODO: move this module around when adding terminal stream saving feature
use crate::app::line_mode::LogEntry; // this is ugly deps i find

pub fn render(app: &mut App, frame: &mut Frame) {
    let screen_area = frame.area();

    let (monitor_area, prompt_area, focus_target): (Rect, Option<Rect>, FocusTarget) =
        match app.config().input_mode {
            InputMode::Terminal => (screen_area, None, FocusTarget::Terminal),
            InputMode::Line => {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Min(3), Constraint::Length(3)])
                    .split(screen_area);

                (chunks[0], Some(chunks[1]), FocusTarget::LineInput)
            }
        };

    let focus = app.focus_target();
    let border_style = border_style_for(focus_target, focus, app.is_app_focused());

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner_area = block.inner(monitor_area);

    let (gutter_area, terminal_area) = match app.config().input_mode {
        InputMode::Terminal if app.config().timestamp_in_line => {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(13), Constraint::Min(0)]) // 12 chars + 1 space/separator
                .split(inner_area);
            (Some(chunks[0]), chunks[1])
        }
        _ => (None, inner_area),
    };

    app.update_viewport(ViewportSize::new(terminal_area.width, terminal_area.height));

    let title = monitor_title(app, &monitor_area);
    block = block.title(title);

    let bottom_titles = monitor_bottom_title(app);
    block = bottom_titles
        .into_iter()
        .fold(block, |b, t| b.title_bottom(t));

    frame.render_widget(block, monitor_area);

    match app.config().input_mode {
        InputMode::Terminal => {
            if let Some(g_area) = gutter_area {
                render_timestamp_gutter(app, frame, g_area);
            }
            render_vt100_terminal(app, frame, terminal_area, monitor_area);
        }
        InputMode::Line => {
            render_monitor(app, frame, inner_area, monitor_area);
        }
    }

    // 3. Render the Input Box (ONLY if in Line Mode)
    if let Some(prompt_rect) = prompt_area {
        render_input_prompt(app, frame, prompt_rect);
    }

    match app.overlay() {
        Some(Overlay::Help) => render_help_popup(app, frame),
        Some(Overlay::Settings) => render_settings_popup(app, frame),
        Some(Overlay::TerminalSize(dialog)) => render_resize_popup(app, dialog, frame),
        Some(Overlay::DeviceDisconnected(port)) => render_disconnected_popup(port, frame),
        Some(Overlay::Error(err)) => render_error_popup(err, frame),
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

fn render_vt100_terminal(app: &mut App, frame: &mut Frame, area: Rect, outer_area: Rect) {
    let view_height = area.height as usize;
    let focus = app.focus_target();
    let is_terminal_focused = focus == FocusTarget::Terminal;

    // TODO: make the cursor config with its style
    let term_widget = MaxicomTerminal::new(app.terminal_mode.vt100_screen())
        .x_offset(app.terminal_mode.pan_offset_x() as u16)
        .y_offset(app.terminal_mode.pan_offset_y() as u16)
        .focused(is_terminal_focused && app.is_app_focused())
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

        let scrollbar_area = Rect {
            x: outer_area.x + outer_area.width.saturating_sub(1),
            y: outer_area.y + 1,
            width: 1,
            height: outer_area.height.saturating_sub(2),
        };

        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }

    // Cusor
    let screen = app.terminal_mode.vt100_screen();
    let is_scrolled_into_history = screen.scrollback() > 0;

    if is_terminal_focused && !screen.hide_cursor() && !is_scrolled_into_history {
        let (c_row, c_col) = screen.cursor_position();

        let local_col = c_col.checked_sub(app.terminal_mode.pan_offset_x() as u16);
        let local_row = c_row.checked_sub(app.terminal_mode.pan_offset_y() as u16);

        if let (Some(col), Some(row)) = (local_col, local_row)
            && col < area.width
            && row < area.height
        {
            frame.set_cursor_position((area.x + col, area.y + row));
        }
    }
}

fn render_timestamp_gutter(app: &App, frame: &mut Frame, area: Rect) {
    let screen = app.terminal_mode.vt100_screen();
    let vt_rows = screen.size().0 as usize;
    let pan_y = app.terminal_mode.pan_offset_y();

    let remaining_vt_rows = vt_rows.saturating_sub(pan_y);

    let rows_to_draw = (area.height as usize).min(remaining_vt_rows);
    if rows_to_draw == 0 {
        return;
    }

    let vt_scrollback = screen.scrollback();
    let total_timeline = app.terminal_mode.total_timeline_len();

    let end_idx = total_timeline.saturating_sub(vt_scrollback);
    let start_idx = end_idx.saturating_sub(vt_rows);

    let mut lines = Vec::with_capacity(rows_to_draw);

    for row in 0..rows_to_draw {
        let source_row = row + pan_y;
        let global_ts_idx = start_idx + source_row;

        let maybe_ts = if global_ts_idx < end_idx {
            app.terminal_mode.get_timeline_timestamp(global_ts_idx)
        } else {
            None
        };

        let row_has_text = app.terminal_mode.row_has_content(source_row as u16);

        if row_has_text && let Some(ts) = maybe_ts {
            let formatted = ts.format("%H:%M:%S%.3f").to_string();
            lines.push(Line::from(vec![
                Span::styled(formatted, Style::default().fg(Color::DarkGray)),
                Span::styled("│", Style::default().fg(Color::DarkGray)),
            ]));
        } else {
            lines.push(Line::raw("            │"));
        }
    }

    let gutter = Paragraph::new(lines);
    frame.render_widget(gutter, area);
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

fn render_monitor(app: &mut App, frame: &mut Frame, area: Rect, outer_area: Rect) {
    let timestamp_in_line = app.config().timestamp_in_line;
    let content_width = area.width as usize;
    let content_height = area.height as usize;
    let wrap_mode = app.config().wrap_mode;

    app.update_viewport(ViewportSize::new(area.width, area.height));

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
    let mut history_paragraph = Paragraph::new(history_lines);

    history_paragraph = match wrap_mode {
        WrapMode::Wrap => history_paragraph.wrap(Wrap { trim: false }),
        WrapMode::Truncate => history_paragraph,
    };

    // WARN: Paragraph.line_width and .line_count are experiemental deps
    let max_pan = match wrap_mode {
        WrapMode::Wrap => 0, //unused so no need to process
        WrapMode::Truncate => history_paragraph.line_width().saturating_sub(content_width),
    };
    let total_visual_rows = match wrap_mode {
        WrapMode::Wrap => history_paragraph.line_count(content_width as u16),
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

        let scrollbar_area = Rect {
            x: outer_area.x + outer_area.width.saturating_sub(1),
            y: outer_area.y + 1,
            width: 1,
            height: outer_area.height.saturating_sub(2),
        };

        frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
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
fn render_help_popup(app: &App, frame: &mut Frame) {
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
    let timestamp_str = if app.config().timestamp_in_line {
        "ON"
    } else {
        "OFF"
    };

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
        Line::from(vec![
            Span::styled("[T] ", Style::default().fg(Color::Yellow)),
            Span::raw("Timestamps  : "),
            Span::styled(timestamp_str, Style::default().fg(Color::Cyan)),
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
            ))
            .title(" Settings [Toggle Hotkeys] ")
            .style(Style::default().fg(Color::Green)),
    );

    frame.render_widget(popup, area);
}

// NOTE: AI generated, make it better
pub fn render_resize_popup(app: &App, dialog: &ResizeDialog, frame: &mut Frame) {
    let area = centered_rect(52, 11, frame.area());
    frame.render_widget(Clear, area);

    let active = dialog.active_field;

    // Highlight styles for the active field
    let row_style = if active == ResizeField::Rows {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    let col_style = if active == ResizeField::Cols {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };

    // Read the string values straight from tui-input
    let rows_str = dialog.rows_input.value();
    let cols_str = dialog.cols_input.value();

    let text = vec![
        Line::from(Span::styled(
            "--- Terminal Dimensions ---",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                if active == ResizeField::Rows {
                    " ▶ "
                } else {
                    "   "
                },
                row_style,
            ),
            Span::raw("Rows    : "),
            Span::styled(format!("{rows_str:<5}"), row_style),
            Span::styled(" [type numbers]", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(vec![
            Span::styled(
                if active == ResizeField::Cols {
                    " ▶ "
                } else {
                    "   "
                },
                col_style,
            ),
            Span::raw("Columns : "),
            Span::styled(format!("{cols_str:<5}"), col_style),
            Span::styled(" [type numbers]", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "[Tab] Switch Field | [Enter] Apply | [Esc] Cancel",
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
            ))
            .title(" Resize VT100 Screen ")
            .border_style(Style::default().fg(Color::Cyan)),
    );

    frame.render_widget(popup, area);

    // Position WezTerm's real blinking cursor over the active input field
    let (cursor_input, line_y_offset) = match active {
        ResizeField::Rows => (&dialog.rows_input, 3), // line index inside popup
        ResizeField::Cols => (&dialog.cols_input, 4), // line index inside popup
    };

    // area.x + 1 (border) + 3 (" ▶ ") + 10 ("Rows    : ") + cursor_offset
    let cursor_x = area.x + 1 + 3 + 10 + cursor_input.visual_cursor() as u16;
    let cursor_y = area.y + line_y_offset;

    frame.set_cursor_position((cursor_x, cursor_y));
}

fn render_disconnected_popup(port: &str, frame: &mut Frame) {
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
        .block(
            Block::bordered()
                .title(format!(" {} ", err.title))
                .border_style(Style::default().fg(Color::Red)),
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

/// Helper function to get standard borders
pub fn border_style_for(target: FocusTarget, current: FocusTarget, app_focused: bool) -> Style {
    if !app_focused {
        Style::default().fg(Color::DarkGray)
    } else if target == current {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}
