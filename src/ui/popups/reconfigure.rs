use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    configs::ThemeConfig,
    overlays::{SerialConfigDialog, SerialSettingField},
    ui::common::popup_area,
};

pub fn render_reconfigure_popup(
    dialog: &SerialConfigDialog,
    frame: &mut Frame,
    theme: &ThemeConfig,
) {
    let area = popup_area(frame.area(), 40, 11, 42, 10);
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(*theme.popup.border)
        .style(*theme.popup.background)
        .title(Span::styled(
            " Serial Port Configuration ",
            *theme.popup.title,
        ))
        .title_bottom(Line::from(vec![
            Span::styled("[Enter]: ", Style::default().fg(theme.palette.text_muted)),
            Span::raw("Apply  "),
            Span::styled("[Esc]: ", Style::default().fg(theme.palette.text_muted)),
            Span::raw("Cancel"),
        ]));

    let inner = block.inner(area);
    frame.render_widget(block, area);

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
                .fg(theme.palette.secondary)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.palette.unselected)
        };

        let pointer = if is_selected { "▶ " } else { "  " };

        lines.push(Line::from(vec![
            Span::styled(pointer, style),
            Span::styled(
                format!("{label:<12}: "),
                Style::default().fg(theme.palette.ternary),
            ),
            Span::styled(value, style),
        ]));
    }

    frame.render_widget(Paragraph::new(lines).style(*theme.popup.background), inner);
}
