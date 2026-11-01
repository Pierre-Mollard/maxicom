use ratatui::{
    Frame,
    layout::{Margin, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Clear, Padding, Paragraph},
};

use unicode_width::UnicodeWidthStr;

use crate::{
    app::{App, types::PromptMap},
    configs::{
        CommandLineLayout, KeyResolver, StatusBarLayout, ThemeConfig,
        keybindings::KeyBindingDialogAction,
    },
    overlays::{CommandLine, PopupTarget},
    ui::{
        common::{popup_area, prompt_view},
        layout::wrap_line_by_words,
        popups::{key_hints, popup_block, row_area},
    },
};

/// Width of the command line popup, as the recording one: command lines can be long
pub const COMMAND_LINE_WIDTH: u16 = 64;

/// Rows of the completion list at most: it scrolls to keep the selection in view
pub const COMPLETION_ROWS: usize = 8;

/// The narrowest help column the selected candidate's help wraps in; narrower, it is cut
const MIN_HELP_WRAP: usize = 12;

/// The command line and its completion list (MAX-12): over the status bar with `layout =
/// "status_bar"` when the bar is drawn (`status_bar` its area), else in its popup. Returns the
/// area the command line takes and where its text is, for the mouse
pub fn render_command_line(
    app: &App,
    command_line: &CommandLine,
    status_bar: Option<Rect>,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> (Rect, PromptMap) {
    let line_mode = &app.settings().line_mode;
    let marks = [
        line_mode.prompt_hidden_left.as_str(),
        line_mode.prompt_hidden_right.as_str(),
    ];
    // the list goes under the popup, above the status bar
    let (area, prompt, anchor, below) = match status_bar {
        Some(bar) if app.settings().command_line.layout == CommandLineLayout::StatusBar => {
            let layout = app.status_bar_layout();
            let show_borders = app.show_borders();
            let (row, prompt) = render_command_line_bar(
                command_line,
                marks,
                frame,
                bar,
                layout,
                show_borders,
                theme,
            );
            (row, prompt, bar, false)
        }
        _ => {
            let resolver = &app.key_resolver;
            let (area, prompt) =
                render_command_line_popup(command_line, resolver, marks, frame, theme, targets);
            (area, prompt, area, true)
        }
    };
    render_completion_list(command_line, anchor, below, frame, theme, targets);
    (area, prompt)
}

/// The command line in a centered popup of one row, always bordered, its key hints in the
/// bottom border. `marks` are the line mode's marks of hidden text, left and right
fn render_command_line_popup(
    command_line: &CommandLine,
    resolver: &KeyResolver,
    marks: [&str; 2],
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> (Rect, PromptMap) {
    let hint_rows: [(&[KeyBindingDialogAction], &str); 3] = [
        (&[KeyBindingDialogAction::CycleNext], "Complete"),
        (&[KeyBindingDialogAction::Confirm], "Run"),
        (&[KeyBindingDialogAction::Close], "Cancel"),
    ];
    let hints = key_hints(resolver, true, &hint_rows, theme);
    let block = popup_block("Command", &hints, theme)
        .border_style(*theme.command_line.border)
        .padding(Padding::horizontal(1));

    let area = popup_area(frame.area(), COMMAND_LINE_WIDTH, 3);
    frame.render_widget(Clear, area);
    let row = block.inner(area);
    frame.render_widget(block, area);
    let prompt = render_input_row(
        command_line,
        marks,
        frame,
        row,
        *theme.popup.background,
        theme,
    );
    hints.push_block_targets(area, targets);
    (area, prompt)
}

/// The command line over the status bar's row: inside its box, or between the borders' edges
/// like the bar's text. The row is the popup's area
fn render_command_line_bar(
    command_line: &CommandLine,
    marks: [&str; 2],
    frame: &mut Frame,
    bar: Rect,
    layout: StatusBarLayout,
    show_borders: bool,
    theme: &ThemeConfig,
) -> (Rect, PromptMap) {
    let row = match layout {
        StatusBarLayout::Box => bar.inner(Margin::new(1, 1)),
        _ if show_borders => bar.inner(Margin::new(1, 0)),
        _ => bar,
    };
    frame.render_widget(Clear, row);
    let prompt = render_input_row(command_line, marks, frame, row, *theme.ui.background, theme);
    (row, prompt)
}

/// The `:` prompt and the text after it, scrolled as the TX input (a mark on an edge hiding
/// text), and the cursor
fn render_input_row(
    command_line: &CommandLine,
    [hidden_left_mark, hidden_right_mark]: [&str; 2],
    frame: &mut Frame,
    row: Rect,
    base: Style,
    theme: &ThemeConfig,
) -> PromptMap {
    let prompt = ":";
    let prompt_width = prompt.len() as u16;
    let text_area = Rect {
        x: row.x + prompt_width,
        width: row.width.saturating_sub(prompt_width),
        ..row
    };
    let prompt_style = base.patch(*theme.command_line.prompt);
    frame.render_widget(Paragraph::new(prompt).style(prompt_style), row);
    if text_area.is_empty() {
        return PromptMap {
            text_area,
            scroll: 0,
        };
    }

    let input = &command_line.input;
    let cursor = input.visual_cursor();
    let (scroll, hidden_left, hidden_right) = prompt_view(
        input.value().width(),
        cursor,
        text_area.width as usize,
        command_line.scroll.get(),
    );
    command_line.scroll.set(scroll);
    frame.render_widget(
        Paragraph::new(input.value())
            .style(base.patch(*theme.command_line.text))
            .scroll((0, scroll as u16)),
        text_area,
    );
    let buf = frame.buffer_mut();
    if hidden_left {
        buf[(text_area.x, text_area.y)]
            .set_symbol(hidden_left_mark)
            .set_style(prompt_style);
    }
    if hidden_right {
        buf[(text_area.right() - 1, text_area.y)]
            .set_symbol(hidden_right_mark)
            .set_style(prompt_style);
    }
    frame.set_cursor_position((text_area.x + (cursor - scroll) as u16, text_area.y));
    PromptMap { text_area, scroll }
}

/// The completion of the word before the cursor: a list under `anchor` (the popup), or above it
/// (the status bar), one column inside it on each side. Each candidate is a row, cut at the
/// list's width, but the selected one, whose help wraps under the help column (MAX-163). The
/// list shows `COMPLETION_ROWS` rows at most, scrolled to keep the selected one whole in view.
/// Each row is a click target on its candidate
fn render_completion_list(
    command_line: &CommandLine,
    anchor: Rect,
    below: bool,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) {
    let candidates = &command_line.completion.candidates;
    if candidates.is_empty() {
        return;
    }
    let value_width = candidates
        .iter()
        .map(|c| c.value.width())
        .max()
        .unwrap_or(0);
    let help_style = Style::default().fg(theme.palette.text_muted);
    let selected = command_line.selected;

    // the selected help's rows, in the room left of the list's borders and padding
    let help_width = usize::from(anchor.width.saturating_sub(6)).saturating_sub(value_width + 2);
    let selected_help: Vec<Line> = match selected.map(|i| candidates[i].help) {
        Some(help) if !help.is_empty() && help_width >= MIN_HELP_WRAP => {
            let help = Line::from(Span::styled(help, help_style));
            (wrap_line_by_words(help, help_width, 0, 0).into_iter())
                .map(|row| Line::from(row.spans))
                .collect()
        }
        _ => Vec::new(),
    };
    let height_of = |i: usize| match selected {
        Some(s) if s == i => selected_help.len().clamp(1, COMPLETION_ROWS),
        _ => 1,
    };
    // the first shown: the selected one at the bottom once the list scrolls
    let first = selected.map_or(0, |s| {
        let (mut first, mut used) = (s, height_of(s));
        while first > 0 && used < COMPLETION_ROWS {
            first -= 1;
            used += 1;
        }
        first
    });

    let mut lines: Vec<Line> = Vec::new();
    // each candidate shown: its index, first row and row count
    let mut shown: Vec<(usize, usize, usize)> = Vec::new();
    for (i, candidate) in candidates.iter().enumerate().skip(first) {
        let room = COMPLETION_ROWS - lines.len();
        if room == 0 {
            break;
        }
        let style = if selected == Some(i) {
            *theme.command_line.selected
        } else {
            *theme.command_line.text
        };
        let value = Span::styled(format!("{:<value_width$}", candidate.value), style);
        let start = lines.len();
        if selected == Some(i) && !selected_help.is_empty() {
            // the next rows under the help column
            let indent = " ".repeat(value_width + 2);
            for (row, help) in selected_help.iter().take(room).enumerate() {
                let lead = if row == 0 {
                    vec![value.clone(), Span::raw("  ")]
                } else {
                    vec![Span::raw(indent.clone())]
                };
                lines.push(Line::from([lead, help.spans.clone()].concat()));
            }
        } else {
            let mut spans = vec![value];
            if !candidate.help.is_empty() {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(candidate.help, help_style));
            }
            lines.push(Line::from(spans));
        }
        shown.push((i, start, lines.len() - start));
    }

    let height = lines.len() as u16 + 2;
    let y = if below {
        anchor.bottom()
    } else {
        anchor.y.saturating_sub(height)
    };
    let area = Rect::new(anchor.x + 1, y, anchor.width.saturating_sub(2), height)
        .intersection(frame.area());
    if area.is_empty() {
        return;
    }

    let block = Block::bordered()
        .border_style(*theme.command_line.border)
        .style(*theme.popup.background)
        .padding(Padding::horizontal(1));
    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines), inner);
    for (i, start, rows) in shown {
        for row in start..start + rows {
            if row < inner.height as usize {
                targets.push((row_area(inner, row), PopupTarget::Completion(i)));
            }
        }
    }
}
