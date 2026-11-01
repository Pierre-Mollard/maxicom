use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::{
    capture::{ScreenshotFormat, Stream},
    configs::{KeyResolver, ThemeConfig, keybindings::KeyBindingDialogAction},
    overlays::{FilePathDialog, PopupTarget, ScreenshotDialog},
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

    let mut text = vec![
        section("Recording", theme),
        Line::from(""),
        Line::from(vec![
            Span::styled("▶ ", style),
            Span::raw("Name    : "),
            Span::styled(path_str, style),
            // the stream sets the extension, or the folder (MAX-175)
            Span::styled(
                dialog.suffix(),
                Style::default().fg(theme.palette.text_muted),
            ),
        ]),
        Line::from(vec![
            Span::raw("  Stream  : "),
            stream_span(Stream::Text),
            Span::raw(" "),
            stream_span(Stream::Full),
        ]),
    ];
    let mut hint_rows: Vec<(&[KeyBindingDialogAction], &str)> =
        vec![(&[KeyBindingDialogAction::CycleNext], "Stream")];
    // with a session backup running, the recording can start with it (MAX-147)
    if let Some(include) = dialog.include_session {
        let mark = if include { "[x]" } else { "[ ]" };
        text.push(Line::from(vec![
            Span::raw("  Include : "),
            Span::styled(mark, style),
            Span::raw(" since session start"),
        ]));
        hint_rows.push((&[KeyBindingDialogAction::CyclePrev], "Include"));
    }
    hint_rows.extend([
        (&[KeyBindingDialogAction::Confirm][..], "Apply"),
        (&[KeyBindingDialogAction::Close], "Cancel"),
    ]);

    let hints = key_hints(resolver, true, &hint_rows, theme);
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
    if dialog.include_session.is_some() {
        targets.push((row_area(inner, 4), PopupTarget::CaptureSession));
    }

    let cursor_input = &dialog.file_input;
    let line_y_offset = 2; // relative to inner area

    let cursor_x = inner.x + 2 + 10 + cursor_input.visual_cursor() as u16;
    let cursor_y = inner.y + line_y_offset;

    frame.set_cursor_position((cursor_x, cursor_y));
    area
}

/// The screenshot dialog: the file row and the format row, Enter saves (MAX-147)
pub fn render_screenshot_popup(
    dialog: &ScreenshotDialog,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let style = Style::default()
        .fg(theme.palette.secondary)
        .add_modifier(Modifier::BOLD);

    let format_span = |format: ScreenshotFormat| {
        let label = format!(" {} ", format.as_str());
        if format == dialog.format {
            Span::styled(label, style.add_modifier(Modifier::REVERSED))
        } else {
            Span::styled(label, Style::default().fg(theme.palette.unselected))
        }
    };

    let text = vec![
        section("Screenshot", theme),
        Line::from(""),
        Line::from(vec![
            Span::styled("▶ ", style),
            Span::raw("File    : "),
            Span::styled(dialog.file_input.value(), style),
            Span::styled(
                " [type filepath]",
                Style::default().fg(theme.palette.text_muted),
            ),
        ]),
        Line::from(vec![
            Span::raw("  Format  : "),
            format_span(ScreenshotFormat::Text),
            Span::raw(" "),
            format_span(ScreenshotFormat::Ansi),
        ]),
    ];
    let hint_rows: [(&[KeyBindingDialogAction], &str); 3] = [
        (&[KeyBindingDialogAction::CycleNext], "Format"),
        (&[KeyBindingDialogAction::Confirm], "Save"),
        (&[KeyBindingDialogAction::Close], "Cancel"),
    ];

    let hints = key_hints(resolver, true, &hint_rows, theme);
    let block = popup_block("Save Screenshot", &hints, theme);

    let area = form_area(frame.area(), 64, text.len());
    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(text), inner);
    hints.push_block_targets(area, targets);
    targets.push((row_area(inner, 3), PopupTarget::ScreenshotFormat));

    // the file row, as in the recording dialog
    let cursor_x = inner.x + 2 + 10 + dialog.file_input.visual_cursor() as u16;
    frame.set_cursor_position((cursor_x, inner.y + 2));
    area
}
