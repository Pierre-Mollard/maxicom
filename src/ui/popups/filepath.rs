use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{configs::ThemeConfig, overlays::FilePathDialog, ui::common::popup_area};

pub fn render_filepath_popup(dialog: &FilePathDialog, frame: &mut Frame, theme: &ThemeConfig) {
    let area = popup_area(frame.area(), 60, 6, 40, 8);
    frame.render_widget(Clear, area);

    let style = Style::default()
        .fg(theme.palette.secondary)
        .add_modifier(Modifier::BOLD);

    let path_str = dialog.file_input.value();

    let text = vec![
        Line::from(Span::styled(
            "--- Log File Path ---",
            Style::default()
                .fg(theme.palette.main)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" ▶ ", style),
            Span::raw("File    : "),
            Span::styled(path_str, style),
            Span::styled(
                " [type filepath]",
                Style::default().fg(theme.palette.text_muted),
            ),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(*theme.popup.border)
        .style(*theme.popup.background)
        .title(Span::styled(" Logging ", *theme.popup.title))
        .title_bottom(Line::from(vec![
            Span::styled("[Enter]: ", Style::default().fg(theme.palette.text_muted)),
            Span::raw("Apply"),
            Span::styled("  [Esc]: ", Style::default().fg(theme.palette.text_muted)),
            Span::raw("Cancel"),
        ]));

    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(text).style(*theme.popup.background), inner);

    let cursor_input = &dialog.file_input;
    let line_y_offset = 2; // relative to inner area

    let cursor_x = inner.x + 3 + 10 + cursor_input.visual_cursor() as u16;
    let cursor_y = inner.y + line_y_offset;

    frame.set_cursor_position((cursor_x, cursor_y));
}
