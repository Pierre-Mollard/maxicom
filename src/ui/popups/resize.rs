use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::{
    configs::{KeyResolver, ThemeConfig, keybindings::KeyBindingDialogAction},
    overlays::{PopupTarget, ResizeDialog, ResizeField},
    ui::popups::{FORM_WIDTH, centered, form_area, key_hints, popup_block, row_area, section},
};

pub fn render_resize_popup(
    dialog: &ResizeDialog,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let active = dialog.active_field;

    let row_style = if active == ResizeField::Rows {
        Style::default()
            .fg(theme.palette.secondary)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.palette.unselected)
    };

    let col_style = if active == ResizeField::Cols {
        Style::default()
            .fg(theme.palette.secondary)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.palette.unselected)
    };

    let rows_str = dialog.rows_input.value();
    let cols_str = dialog.cols_input.value();

    let text = vec![
        section("Terminal Dimensions", theme),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                if active == ResizeField::Rows {
                    "▶ "
                } else {
                    "  "
                },
                row_style,
            ),
            Span::raw("Rows    : "),
            Span::styled(format!("{rows_str:<5}"), row_style),
            Span::styled(
                " [type numbers]",
                Style::default().fg(theme.palette.text_muted),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                if active == ResizeField::Cols {
                    "▶ "
                } else {
                    "  "
                },
                col_style,
            ),
            Span::raw("Columns : "),
            Span::styled(format!("{cols_str:<5}"), col_style),
            Span::styled(
                " [type numbers]",
                Style::default().fg(theme.palette.text_muted),
            ),
        ]),
    ];

    let hints = key_hints(
        resolver,
        true,
        &[
            (&[KeyBindingDialogAction::CycleNext], "Switch"),
            (&[KeyBindingDialogAction::Confirm], "Apply"),
            (&[KeyBindingDialogAction::Close], "Cancel"),
        ],
        theme,
    );
    let block = popup_block("Resize VT100 Screen", &hints, theme);

    let area = form_area(frame.area(), FORM_WIDTH, text.len());
    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    // centered, as wide as a field's row: pointer, label, 5 digits and the note
    let content = centered(inner, 2 + 10 + 5 + 15);
    frame.render_widget(Paragraph::new(text), content);

    let (cursor_input, line_y_offset) = match active {
        ResizeField::Rows => (&dialog.rows_input, 2),
        ResizeField::Cols => (&dialog.cols_input, 3),
    };

    // after "▶ " and "Rows    : "
    let cursor_x = content.x + 2 + 10 + cursor_input.visual_cursor() as u16;
    let cursor_y = inner.y + line_y_offset;

    frame.set_cursor_position((cursor_x, cursor_y));

    hints.push_block_targets(area, targets);
    // the field rows, under the header and its blank line (MAX-113)
    targets.push((
        row_area(inner, 2),
        PopupTarget::ResizeField(ResizeField::Rows),
    ));
    targets.push((
        row_area(inner, 3),
        PopupTarget::ResizeField(ResizeField::Cols),
    ));
    area
}
