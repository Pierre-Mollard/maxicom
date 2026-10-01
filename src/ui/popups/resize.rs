use ratatui::{
    Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    app::App,
    theme::ThemeConfig,
    types::{FocusTarget, ResizeDialog, ResizeField},
    ui::common::{border_style_for, centered_rect},
};

// NOTE: AI generated, make it better
pub fn render_resize_popup(
    app: &App,
    dialog: &ResizeDialog,
    frame: &mut Frame,
    theme: &ThemeConfig,
) {
    let area = centered_rect(52, 11, frame.area());
    frame.render_widget(Clear, area);

    let active = dialog.active_field;

    // Highlight styles for the active field
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

    // Read the string values straight from tui-input
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
            "[Tab] Switch Field | [Enter] Apply | [Esc] Cancel",
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

    // Position WezTerm's real blinking cursor over the active input field
    let (cursor_input, line_y_offset) = match active {
        ResizeField::Rows => (&dialog.rows_input, 3), // line index inside popup
        ResizeField::Cols => (&dialog.cols_input, 4), // line index inside popup
    };

    // area.x + 1 (border) + 3 (" ▶ ") + 10 ("Rows    : ") + cursor_offset
    let cursor_x = area.x + 1 + 3 + 10 + cursor_input.visual_cursor() as u16;
    let cursor_y = area.y + line_y_offset;

    frame.set_cursor_position((cursor_x, cursor_y));
}
