use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use std::fmt::Write;

use crate::{app::App, config::InputMode, types::FocusTarget};

pub fn monitor_title(app: &App, area: &Rect) -> String {
    let mut title = String::with_capacity(80);

    let (scroll_offset, pan_offset) = match app.config().input_mode {
        InputMode::Terminal => {
            let screen = app.terminal_mode.vt100_screen();
            if app.terminal_mode.is_new_content_missed() {
                let _ = write!(title, "! ");
            }
            if app.terminal_mode.is_logging() {
                let _ = write!(title, "● REC ");
            }
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
            if app.line_mode.is_new_content_missed() {
                let _ = write!(title, "! ");
            }
            if app.line_mode.is_logging() {
                let _ = write!(title, "● REC ");
            }
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

pub fn monitor_bottom_title<'a>(app: &App) -> Vec<Line<'a>> {
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

/// Helper function to center a popup window
pub fn centered_rect(percent_x: u16, height: u16, parent: Rect) -> Rect {
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
