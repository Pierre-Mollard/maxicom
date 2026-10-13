use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::notification::NotificationManager;

const TOAST_MIN_WIDTH: u16 = 24;
const TOAST_MAX_WIDTH: u16 = 36;
const MIN_SCREEN_WIDTH: u16 = 30;
const MIN_SCREEN_HEIGHT: u16 = 6;

const TOAST_MAX_CONTENT_LINES: usize = 5;

const TOAST_RIGHT_MARGIN: u16 = 2;
const TOAST_TOP_START_Y: u16 = 1;
const TOAST_STACK_GAP_Y: u16 = 1;

pub fn render_notifications(manager: &NotificationManager, frame: &mut Frame) {
    let active = manager.active_toasts();
    if active.is_empty() {
        return;
    }
    let screen_area = frame.area();
    if screen_area.width < MIN_SCREEN_WIDTH || screen_area.height < MIN_SCREEN_HEIGHT {
        return;
    }

    let max_allowed_width = (screen_area.width.saturating_sub(TOAST_RIGHT_MARGIN * 2))
        .clamp(TOAST_MIN_WIDTH, TOAST_MAX_WIDTH);

    let mut current_y = TOAST_TOP_START_Y;

    for toast in active {
        let color = toast.level.color();
        let icon = toast.level.icon();
        let icon_len = icon.chars().count();

        let raw_first_line_len = toast
            .message
            .lines()
            .next()
            .map_or(0, |l| l.chars().count());
        let ideal_width = (raw_first_line_len + icon_len + 4) as u16;
        let toast_width = ideal_width.clamp(TOAST_MIN_WIDTH, max_allowed_width);

        let inner_width = (toast_width as usize).saturating_sub(4);
        let text_wrap_width = inner_width.saturating_sub(icon_len);

        let wrapped_lines = wrap_text(&toast.message, text_wrap_width.max(8));
        let content_lines_count = wrapped_lines.len().min(TOAST_MAX_CONTENT_LINES);

        let toast_height = (content_lines_count as u16) + 2;
        if current_y + toast_height > screen_area.height {
            break;
        }

        let x = screen_area
            .width
            .saturating_sub(toast_width + TOAST_RIGHT_MARGIN);
        let toast_area = Rect::new(x, current_y, toast_width, toast_height);

        frame.render_widget(Clear, toast_area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(color))
            .title(Span::styled(
                format!(" {} ", toast.level.title()),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));

        let mut lines = Vec::with_capacity(content_lines_count);
        let indent_spaces = " ".repeat(icon_len);

        for (i, line_text) in wrapped_lines.iter().take(content_lines_count).enumerate() {
            if i == 0 {
                lines.push(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(
                        icon,
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(line_text),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::raw(" "),
                    Span::raw(&indent_spaces),
                    Span::raw(line_text),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines).block(block);
        frame.render_widget(paragraph, toast_area);

        current_y += toast_height + TOAST_STACK_GAP_Y;
    }
}

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }

    let mut lines = Vec::new();
    for raw_line in text.lines() {
        if raw_line.is_empty() {
            lines.push(String::new());
            continue;
        }

        let mut current_line = String::new();
        let mut current_len = 0;

        for word in raw_line.split_whitespace() {
            let word_len = word.chars().count();

            if current_len == 0 {
                current_line.push_str(word);
                current_len = word_len;
            } else if current_len + 1 + word_len <= max_width {
                current_line.push(' ');
                current_line.push_str(word);
                current_len += 1 + word_len;
            } else {
                lines.push(current_line);
                current_line = word.to_string();
                current_len = word_len;
            }
        }

        if !current_line.is_empty() {
            lines.push(current_line);
        }
    }

    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}
