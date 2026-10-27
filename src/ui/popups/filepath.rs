use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::{
    configs::{KeyResolver, ThemeConfig, keybindings::KeyBindingDialogAction},
    logger::Stream,
    overlays::{FilePathDialog, PopupTarget},
    ui::popups::{form_area, key_hints, popup_block, row_area, section},
};

pub fn render_filepath_popup(
    dialog: &FilePathDialog,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let style = Style::default()
        .fg(theme.palette.secondary)
        .add_modifier(Modifier::BOLD);

    let path_str = dialog.file_input.value();
    let stream_span = |stream: Stream| {
        let label = format!(" {} ", stream.as_str());
        if stream == dialog.stream {
            Span::styled(label, style.add_modifier(Modifier::REVERSED))
        } else {
            Span::styled(label, Style::default().fg(theme.palette.unselected))
        }
    };

    let text = vec![
        section("Recording", theme),
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
        Line::from(vec![
            Span::raw("  Stream  : "),
            stream_span(Stream::Text),
            Span::raw(" "),
            stream_span(Stream::Raw),
        ]),
    ];

    let hints = key_hints(
        resolver,
        true,
        &[
            (&[KeyBindingDialogAction::CycleNext], "Stream"),
            (&[KeyBindingDialogAction::Confirm], "Apply"),
            (&[KeyBindingDialogAction::Close], "Cancel"),
        ],
        theme,
    );
    let block = popup_block("Start Recording", &hints, theme);

    // wider than the other forms: paths are long
    let area = form_area(frame.area(), 64, text.len());
    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(text), inner);
    hints.push_block_targets(area, targets);
    // the stream row, under the file row (MAX-147)
    targets.push((row_area(inner, 3), PopupTarget::CaptureStream));

    let cursor_input = &dialog.file_input;
    let line_y_offset = 2; // relative to inner area

    let cursor_x = inner.x + 2 + 10 + cursor_input.visual_cursor() as u16;
    let cursor_y = inner.y + line_y_offset;

    frame.set_cursor_position((cursor_x, cursor_y));
    area
}
