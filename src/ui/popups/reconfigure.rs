use ratatui::{
    Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    overlays::{SerialConfigDialog, SerialSettingField},
    ui::common::centered_rect,
};

pub fn render_reconfigure_popup(dialog: &SerialConfigDialog, frame: &mut Frame) {
    let popup_area = centered_rect(52, 11, frame.area());

    frame.render_widget(Clear, popup_area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Serial Port Configuration ")
        .title_bottom(Line::from(" [Enter] Apply  [Esc] Cancel ").centered());

    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let mut lines = Vec::new();

    let fields = [
        (
            SerialSettingField::BaudRate,
            "Baud Rate",
            format!("< {} >", dialog.config.baudrate),
        ),
        (
            SerialSettingField::CharSize,
            "Data Bits",
            format!("< {:?} >", dialog.config.char_size),
        ),
        (
            SerialSettingField::Parity,
            "Parity",
            format!("< {:?} >", dialog.config.parity),
        ),
        (
            SerialSettingField::StopBits,
            "Stop Bits",
            format!("< {:?} >", dialog.config.stop_bits),
        ),
        (
            SerialSettingField::FlowControl,
            "Flow Ctrl",
            format!("< {:?} >", dialog.config.flow_control),
        ),
    ];

    for (field, label, value) in fields {
        let is_selected = dialog.selected_field == field;

        let style = if is_selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let pointer = if is_selected { "▶ " } else { "  " };

        lines.push(Line::from(vec![
            Span::styled(pointer, style),
            Span::styled(
                format!("{label:<12}: "),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(value, style),
        ]));
    }

    frame.render_widget(Paragraph::new(lines), inner);
}
