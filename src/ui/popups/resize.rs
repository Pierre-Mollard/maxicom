use ratatui::{
    Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    app::{App, types::FocusTarget},
    configs::ThemeConfig,
    overlays::{ResizeDialog, ResizeField},
    ui::common::{border_style_for, popup_area},
};

pub fn render_resize_popup(
    app: &App,
    dialog: &ResizeDialog,
    frame: &mut Frame,
    theme: &ThemeConfig,
) {
    let area = popup_area(frame.area(), 40, 11, 42, 10);
    frame.render_widget(Clear, area);

    let active = dialog.active_field;

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
            "[Tab] Switch Field",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "[Enter] Apply | [Esc] Cancel",
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
                theme,
            ))
            .title(" Resize VT100 Screen ")
            .border_style(Style::default().fg(Color::Cyan)),
    );

    frame.render_widget(popup, area);

    let (cursor_input, line_y_offset) = match active {
        ResizeField::Rows => (&dialog.rows_input, 3),
        ResizeField::Cols => (&dialog.cols_input, 4),
    };

    // area.x + 1 (border) + 3 (" ▶ ") + 10 ("Rows    : ") + cursor_offset
    let cursor_x = area.x + 1 + 3 + 10 + cursor_input.visual_cursor() as u16;
    let cursor_y = area.y + line_y_offset;

    frame.set_cursor_position((cursor_x, cursor_y));
}
