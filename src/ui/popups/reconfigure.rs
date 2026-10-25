use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::{
    configs::{KeyResolver, ThemeConfig, keybindings::KeyBindingDialogAction},
    overlays::{SerialConfigDialog, SerialSettingField},
    ui::popups::{FORM_WIDTH, form_area, key_hints, popup_block},
};

pub fn render_reconfigure_popup(
    dialog: &SerialConfigDialog,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
) {
    let [data_bits, _, stop_bits] = dialog.config.compact_notation();
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
            format!("< {data_bits} >"),
        ),
        (
            SerialSettingField::Parity,
            "Parity",
            format!("< {:?} >", dialog.config.parity),
        ),
        (
            SerialSettingField::StopBits,
            "Stop Bits",
            format!("< {stop_bits} >"),
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

    let block = popup_block(
        "Serial Port",
        key_hints(
            resolver,
            false,
            &[
                (&[KeyBindingDialogAction::Confirm], "Apply"),
                (&[KeyBindingDialogAction::Close], "Cancel"),
            ],
            theme,
        ),
        theme,
    );

    let area = form_area(frame.area(), FORM_WIDTH, lines.len());
    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines), inner);
}
