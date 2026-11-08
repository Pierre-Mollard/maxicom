use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};
use tui_input::Input;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    capture::{ScreenshotFormat, Stream},
    configs::{KeyResolver, ThemeConfig, keybindings::KeyBindingDialogAction},
    overlays::{FilePathDialog, PopupTarget, ScreenshotDialog},
    ui::popups::{centered, form_area, key_hints, popup_block, row_area, section},
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
                dialog.stream.suffix(),
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
    let content = centered(inner, grown_width(&dialog.width, &text));
    // the name scrolled to keep the cursor in the popup, its suffix after it
    let suffix = dialog.stream.suffix();
    let room = usize::from(content.width).saturating_sub(LABEL + suffix.width());
    let (shown, cursor) = field_view(&dialog.file_input, room);
    text[2] = Line::from(vec![
        Span::styled("▶ ", style),
        Span::raw("Name    : "),
        Span::styled(shown, style),
        Span::styled(suffix, Style::default().fg(theme.palette.text_muted)),
    ]);
    frame.render_widget(Paragraph::new(text), content);
    hints.push_block_targets(area, targets);
    // the stream row, under the file row (MAX-147)
    targets.push((row_area(inner, 3), PopupTarget::CaptureStream));
    if dialog.include_session.is_some() {
        targets.push((row_area(inner, 4), PopupTarget::CaptureSession));
    }

    frame.set_cursor_position((content.x + LABEL as u16 + cursor, inner.y + 2));
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

    let mut text = vec![
        section("Screenshot", theme),
        Line::from(""),
        Line::from(vec![
            Span::styled("▶ ", style),
            Span::raw("File    : "),
            Span::styled(dialog.file_input.value(), style),
            Span::styled(FILE_NOTE, Style::default().fg(theme.palette.text_muted)),
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
    let content = centered(inner, grown_width(&dialog.width, &text));
    // the file scrolled to keep the cursor in the popup; its note only while it fits
    let room = usize::from(content.width).saturating_sub(LABEL);
    let path_width = dialog.file_input.value().width() + 1;
    let note = if path_width + FILE_NOTE.width() <= room {
        FILE_NOTE
    } else {
        ""
    };
    let (shown, cursor) = field_view(&dialog.file_input, room - note.width());
    text[2] = Line::from(vec![
        Span::styled("▶ ", style),
        Span::raw("File    : "),
        Span::styled(shown, style),
        Span::styled(note, Style::default().fg(theme.palette.text_muted)),
    ]);
    frame.render_widget(Paragraph::new(text), content);
    hints.push_block_targets(area, targets);
    targets.push((row_area(inner, 3), PopupTarget::ScreenshotFormat));

    // the file row, as in the recording dialog
    frame.set_cursor_position((content.x + LABEL as u16 + cursor, inner.y + 2));
    area
}

/// The widest line and the widest drawn before it, kept in `width`: a name typed longer
/// moves the content once past it, a shorter one doesn't
fn grown_width(width: &std::cell::Cell<u16>, text: &[Line]) -> u16 {
    let widest = text.iter().map(Line::width).max().unwrap_or(0) as u16;
    width.set(width.get().max(widest));
    width.get()
}

/// The pointer and the label before a name's field: `▶ Name    : `
const LABEL: usize = 12;

/// After the screenshot's file while it fits
const FILE_NOTE: &str = " [type filepath]";

/// The part of a field's text that fits `width` columns with the cursor in view (one column
/// kept for it at the end), and the cursor's column in that part
pub fn field_view(input: &Input, width: usize) -> (String, u16) {
    let scroll = input.visual_scroll(width.saturating_sub(1));
    let (mut skipped, mut used) = (0, 0);
    let mut shown = String::new();
    for c in input.value().chars() {
        let w = c.width().unwrap_or(0);
        if skipped < scroll {
            skipped += w;
            continue;
        }
        if used + w > width {
            break;
        }
        used += w;
        shown.push(c);
    }
    (shown, input.visual_cursor().saturating_sub(skipped) as u16)
}
