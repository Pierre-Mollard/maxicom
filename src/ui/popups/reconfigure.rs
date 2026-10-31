use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use unicode_width::UnicodeWidthStr;

use crate::{
    configs::{KeyResolver, ThemeConfig, keybindings::KeyBindingDialogAction},
    link::LinkKind,
    overlays::{PopupTarget, SerialConfigDialog, SerialSettingField},
    ui::popups::{FORM_WIDTH, form_area, key_hints, popup_block, row_area},
};

pub fn render_reconfigure_popup(
    dialog: &SerialConfigDialog,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    if !dialog.has_line_settings() {
        return render_link_without_settings(dialog, resolver, frame, theme, targets);
    }
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

    // each field with the width of its `< value >`
    let field_rows: Vec<(SerialSettingField, u16)> = fields
        .iter()
        .map(|(field, _, value)| (*field, value.width() as u16))
        .collect();
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

    let hints = key_hints(
        resolver,
        false,
        &[
            (&[KeyBindingDialogAction::Confirm], "Apply"),
            (&[KeyBindingDialogAction::Close], "Cancel"),
        ],
        theme,
    );
    let block = popup_block("Serial Port", &hints, theme);

    let area = form_area(frame.area(), FORM_WIDTH, lines.len());
    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines), inner);
    hints.push_block_targets(area, targets);
    // one row per field, in order (MAX-113); before them, as the first target under the pointer
    // wins, the `< ` and ` >` around the value, after the pointer and the label (MAX-134)
    for (y, &(field, width)) in field_rows.iter().enumerate() {
        let row = row_area(inner, y);
        let value_x = row.x + 2 + 14;
        let arrow = |x| Rect::new(x, row.y, 2, 1).intersection(row);
        targets.push((arrow(value_x), PopupTarget::SerialPrev(field)));
        targets.push((
            arrow(value_x + width.saturating_sub(2)),
            PopupTarget::SerialNext(field),
        ));
    }
    for (y, (field, _)) in field_rows.into_iter().enumerate() {
        targets.push((row_area(inner, y), PopupTarget::SerialField(field)));
    }
    area
}

/// The popup for a link without line settings: its address and why there is nothing to set,
/// the bridge's for a TCP link (MAX-156), none for a replayed file (MAX-166)
fn render_link_without_settings(
    dialog: &SerialConfigDialog,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let label = Style::default().fg(theme.palette.ternary);
    let muted = Style::default().fg(theme.palette.unselected);
    let (title, why) = match dialog.config.address.kind() {
        LinkKind::Replay => (
            "Replay",
            [
                "  A replayed file has no baud nor framing:",
                "  nothing to set.",
            ],
        ),
        _ => (
            "Network Link",
            [
                "  The bridge sets baud and framing:",
                "  nothing to set over TCP.",
            ],
        ),
    };
    let lines = vec![
        Line::from(vec![
            Span::styled(format!("  {:<12}: ", "Address"), label),
            Span::raw(dialog.config.address.to_string()),
        ]),
        Line::from(""),
        Line::from(Span::styled(why[0], muted)),
        Line::from(Span::styled(why[1], muted)),
    ];

    let hints = key_hints(
        resolver,
        false,
        &[(&[KeyBindingDialogAction::Close], "Close")],
        theme,
    );
    let block = popup_block(title, &hints, theme);

    let area = form_area(frame.area(), FORM_WIDTH, lines.len());
    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines), inner);
    hints.push_block_targets(area, targets);
    area
}
