use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::{
    configs::{KeyResolver, ThemeConfig, keybindings::KeyBindingDialogAction},
    overlays::FilePathDialog,
    ui::popups::{form_area, key_hints, popup_block, section},
};

pub fn render_filepath_popup(
    dialog: &FilePathDialog,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
) {
    let style = Style::default()
        .fg(theme.palette.secondary)
        .add_modifier(Modifier::BOLD);

    let path_str = dialog.file_input.value();

    let text = vec![
        section("Log File Path", theme),
        Line::from(""),
        Line::from(vec![
            Span::styled("▶ ", style),
            Span::raw("File    : "),
            Span::styled(path_str, style),
            Span::styled(
                " [type filepath]",
                Style::default().fg(theme.palette.text_muted),
            ),
        ]),
    ];

    let block = popup_block(
        "Logging",
        key_hints(
            resolver,
            true,
            &[
                (&[KeyBindingDialogAction::Confirm], "Apply"),
                (&[KeyBindingDialogAction::Close], "Cancel"),
            ],
            theme,
        ),
        theme,
    );

    // wider than the other forms: paths are long
    let area = form_area(frame.area(), 64, text.len());
    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(text), inner);

    let cursor_input = &dialog.file_input;
    let line_y_offset = 2; // relative to inner area

    let cursor_x = inner.x + 2 + 10 + cursor_input.visual_cursor() as u16;
    let cursor_y = inner.y + line_y_offset;

    frame.set_cursor_position((cursor_x, cursor_y));
}
