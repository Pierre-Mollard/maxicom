use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::notification::NotificationManager;

pub fn render_notifications(manager: &NotificationManager, frame: &mut Frame) {
    let active = manager.active_toasts();
    if active.is_empty() {
        return;
    }

    let screen_area = frame.area();
    let toast_width: u16 = 36;
    let toast_height: u16 = 3;

    if screen_area.width < toast_width + 2 || screen_area.height < toast_height + 2 {
        return;
    }

    let x = screen_area.width.saturating_sub(toast_width + 1);
    let mut current_y = 1;

    for toast in active {
        if current_y + toast_height > screen_area.height {
            break;
        }

        let toast_area = Rect::new(x, current_y, toast_width, toast_height);

        frame.render_widget(Clear, toast_area);

        let color = toast.level.color();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(color))
            .title(Span::styled(
                format!(" {} ", toast.level.title()),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));

        let content = Line::from(vec![
            Span::styled(toast.level.icon(), Style::default().fg(color)),
            Span::raw(&toast.message),
        ]);

        let paragraph = Paragraph::new(content).block(block);
        frame.render_widget(paragraph, toast_area);

        current_y += toast_height;
    }
}
