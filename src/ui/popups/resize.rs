use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    configs::ThemeConfig,
    overlays::{ResizeDialog, ResizeField},
    ui::common::popup_area,
};

pub fn render_resize_popup(dialog: &ResizeDialog, frame: &mut Frame, theme: &ThemeConfig) {
    let area = popup_area(frame.area(), 50, 8, 52, 7);
    frame.render_widget(Clear, area);

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
        Line::from(Span::styled(
            "--- Terminal Dimensions ---",
            Style::default()
                .fg(theme.palette.main)
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
            Span::styled(
                " [type numbers]",
                Style::default().fg(theme.palette.text_muted),
            ),
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
            Span::styled(
                " [type numbers]",
                Style::default().fg(theme.palette.text_muted),
            ),
        ]),
        Line::from(""),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(*theme.popup.border)
        .style(*theme.popup.background)
        .title(Span::styled(" Resize VT100 Screen ", *theme.popup.title))
        .title_bottom(Line::from(vec![
            Span::styled("[Tab]: ", Style::default().fg(theme.palette.text_muted)),
            Span::raw("Switch"),
            Span::styled(" [Enter]: ", Style::default().fg(theme.palette.text_muted)),
            Span::raw("Apply"),
            Span::styled(" [Esc]: ", Style::default().fg(theme.palette.text_muted)),
            Span::raw("Cancel"),
        ]));

    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(text).style(*theme.popup.background), inner);

    let (cursor_input, line_y_offset) = match active {
        ResizeField::Rows => (&dialog.rows_input, 2),
        ResizeField::Cols => (&dialog.cols_input, 3),
    };

    // inner.x + 3 (" ▶ ") + 10 ("Rows    : ") + cursor_offset
    let cursor_x = inner.x + 3 + 10 + cursor_input.visual_cursor() as u16;
    let cursor_y = inner.y + line_y_offset;

    frame.set_cursor_position((cursor_x, cursor_y));
}
