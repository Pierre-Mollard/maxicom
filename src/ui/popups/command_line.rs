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
    commands::find_by_name,
    configs::{
        CommandLineLayout, KeyResolver, StatusBarLayout, ThemeConfig,
        keybindings::KeyBindingDialogAction,
    },
    overlays::{CommandLine, PopupTarget},
    ui::{
        common::{popup_area, prompt_view},
        layout::wrap_line_by_words,
        popups::{cut, key_hints, popup_block, render_scrollbar, row_area},
    },
};

/// Width of the command line popup, as the recording one: command lines can be long
pub const COMMAND_LINE_WIDTH: u16 = 64;

/// Rows of the completion list at most: it scrolls to keep the selection in view
pub const COMPLETION_ROWS: usize = 8;

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
/// (the status bar), one column inside it on each side. Each candidate is a row, cut with `…`
/// at the list's width; under a rule, the selected command's usage and its help wrapped
/// (MAX-190). The list shows `COMPLETION_ROWS` rows at most, from the one the overlay scrolled
/// to, a scrollbar in the right padding when there are more. Each row is a click target on its
/// candidate
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
    // inside the list's borders and padding
    let width = usize::from(anchor.width.saturating_sub(6));
    let max_scroll = candidates.len().saturating_sub(COMPLETION_ROWS);
    let first = command_line.list_scroll.min(max_scroll);

    let mut lines: Vec<Line> = Vec::new();
    let shown = candidates
        .iter()
        .enumerate()
        .skip(first)
        .take(COMPLETION_ROWS);
    for (i, candidate) in shown {
        let style = if selected == Some(i) {
            *theme.command_line.selected
        } else {
            *theme.command_line.text
        };
        let value = cut(&format!("{:<value_width$}", candidate.value), width);
        let room = width.saturating_sub(value_width + 2);
        let mut spans = vec![Span::styled(value, style)];
        if !candidate.help.is_empty() && room > 0 {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(cut(&candidate.help, room), help_style));
        }
        lines.push(Line::from(spans));
    }
    let listed = lines.len();

    // the selected command's usage and help, wrapped; none for a value. A named command
    // (MAX-239) has no arguments: its name is its usage
    let detail = selected
        .map(|i| &candidates[i])
        .filter(|candidate| !candidate.help.is_empty())
        .map(|candidate| {
            let usage = find_by_name(&candidate.value).map(|spec| spec.usage());
            (
                usage.unwrap_or_else(|| candidate.value.clone()),
                candidate.help.clone(),
            )
        });
    if let Some((usage, help)) = &detail {
        lines.push(Line::from(""));
        let style = Style::default().fg(theme.palette.main);
        lines.push(Line::styled(cut(usage, width), style));
        let help = Line::styled(help.clone(), Style::default().fg(theme.palette.text));
        let rows = wrap_line_by_words(help, width.max(1), 0, 0).into_iter();
        lines.extend(rows.map(|row| Line::from(row.spans)));
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
    // the rule between the list and the detail, joined to the borders
    let rule_y = inner.y + listed as u16;
    if detail.is_some() && rule_y < inner.bottom() {
        let rule = Rect::new(area.x, rule_y, area.width, 1);
        let middle = "─".repeat(usize::from(rule.width.saturating_sub(2)));
        let style = *theme.command_line.border;
        let text = Span::styled(format!("├{middle}┤"), style);
        frame.render_widget(Paragraph::new(text), rule);
    }
    for row in 0..listed {
        if row < inner.height as usize {
            targets.push((row_area(inner, row), PopupTarget::Completion(first + row)));
        }
    }
    if max_scroll > 0 {
        let bar = Rect {
            x: inner.right(),
            width: 1,
            height: (listed as u16).min(inner.height),
            ..inner
        }
        .intersection(area);
        render_scrollbar(frame, bar, (first, max_scroll, COMPLETION_ROWS), theme);
    }
}
