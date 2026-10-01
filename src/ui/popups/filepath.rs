use ratatui::{
    Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    app::App,
    theme::ThemeConfig,
    types::{FilePathDialog, FocusTarget},
    ui::common::{border_style_for, centered_rect},
};

pub fn render_filepath_popup(
    app: &App,
    dialog: &FilePathDialog,
    frame: &mut Frame,
    theme: &ThemeConfig,
) {
    let area = centered_rect(52, 11, frame.area());
    frame.render_widget(Clear, area);

    let style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);

    let path_str = dialog.file_input.value();

    let text = vec![
        Line::from(Span::styled(
            "--- Log File Path ---",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" ▶ ", style),
            Span::raw("File    : "),
            Span::styled(path_str, style),
            Span::styled(" [type filepath]", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(""),
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
            .title(" Logging ")
            .border_style(Style::default().fg(Color::Cyan)),
    );

    frame.render_widget(popup, area);

    let cursor_input = &dialog.file_input;
    let line_y_offset = 3;

    // area.x + 1 (border) + 3 (" ▶ ") + 10 ("File    : ") + cursor_offset
    let cursor_x = area.x + 1 + 3 + 10 + cursor_input.visual_cursor() as u16;
    let cursor_y = area.y + line_y_offset;

    frame.set_cursor_position((cursor_x, cursor_y));
}
