use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::App,
    configs::{KeyPattern, ThemeConfig, keybindings::KeyBindingDialogAction},
    link::serial::PortState,
    overlays::{Directory, PopupTarget},
    ui::{
        common::popup_area,
        popups::{Hints, hint_spans, popup_block, render_scrollbar},
    },
};

/// The section's title, over the ports
const PORTS: &str = "Serial ports";
/// What an empty list shows
const EMPTY: &str = "No serial port found: plug one in, then r";
/// Under the title: what the test does, an open on Windows, which may reset a board
#[cfg(windows)]
pub const TEST_NOTE: &str = "t opens each port briefly: a board may reset";
#[cfg(not(windows))]
pub const TEST_NOTE: &str = "t finds who holds each port, without opening it";

/// The directory (MAX-181): the system's serial ports under their title and the test's
/// warning, `●` and "in use" on the device in use, each tested port's result after its name,
/// the buttons in the bottom border. As wide as its longest row, as tall as the list, the
/// screen's size at most; more ports than rows, a scrollbar in the right padding
pub fn render_directory_popup(
    app: &App,
    directory: &Directory,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let resolver = &app.key_resolver;
    let dialog_key = |action| resolver.dialog_keys(action).into_iter().next();
    let key = |c| {
        Some(KeyPattern {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::NONE,
        })
    };
    let buttons = [
        (
            dialog_key(KeyBindingDialogAction::Confirm),
            PopupTarget::DirectoryOpen,
            "Open",
        ),
        (key('t'), PopupTarget::DirectoryTest, "Test"),
        (key('r'), PopupTarget::DirectoryRefresh, "Refresh"),
        (
            dialog_key(KeyBindingDialogAction::Close),
            PopupTarget::Hint(KeyBindingDialogAction::Close),
            "Close",
        ),
    ];
    let buttons = buttons
        .into_iter()
        .map(|(key, target, label)| (key.map(|key| (key, target)).into_iter().collect(), label));
    let (line, spots) = hint_spans(buttons, theme);
    let hints = Hints {
        line,
        spots: Vec::new(),
    };

    // maxicom's port only while open: detached, lost or not picked, it's like the others
    let current = app
        .is_link_open()
        .then(|| app.link_config().address.to_string());
    let rows: Vec<(&str, bool, Option<Note>)> = (directory.ports.iter())
        .map(|port| {
            let in_use = current.as_ref() == Some(port);
            let note = if directory.opening.as_ref() == Some(port) {
                Some(("opening…".to_string(), theme.palette.text_muted))
            } else if in_use {
                Some(("in use".to_string(), theme.palette.text_muted))
            } else {
                directory
                    .tests
                    .get(port)
                    .map(|state| note(state.as_ref(), theme))
            };
            (port.as_str(), in_use, note)
        })
        .collect();
    let list_width = (rows.iter())
        .map(|(port, _, note)| 2 + port.width() + note.as_ref().map_or(0, |(n, _)| 2 + n.width()))
        .max()
        .unwrap_or(EMPTY.width());
    let width = (list_width as u16)
        .max(TEST_NOTE.width() as u16)
        .max(hints.line.width() as u16);
    // the borders and the padding around, the title, the note and a blank row on top
    let height = rows.len().max(1) as u16 + 3 + 4;
    let area = popup_area(frame.area(), width + 4, height);

    let block = popup_block("Directory", &hints, theme);
    frame.render_widget(Clear, area);
    let mut inner = block.inner(area);
    frame.render_widget(block, area);
    let (x, y) = (area.x + 2, area.bottom().saturating_sub(1));
    for (start, width, target) in spots {
        let button = Rect::new(x.saturating_add(start), y, width, 1).intersection(area);
        if !button.is_empty() {
            targets.push((button, target));
        }
    }

    let muted = Style::default().fg(theme.palette.text_muted);
    let title = Line::from(Span::styled(PORTS, Style::default().fg(theme.palette.main)));
    let test_note = Line::from(Span::styled(TEST_NOTE, muted));
    frame.render_widget(
        Paragraph::new(vec![title, test_note]),
        Rect { height: 2, ..inner },
    );
    inner.y += 3;
    inner.height = inner.height.saturating_sub(3);

    if rows.is_empty() {
        frame.render_widget(Paragraph::new(Span::styled(EMPTY, muted)), inner);
        return area;
    }

    // from the row that keeps the selection in view; each row is a click target
    let visible = inner.height as usize;
    let first = (directory.selected + 1).saturating_sub(visible);
    let lines: Vec<Line> = (rows.iter().enumerate().skip(first).take(visible))
        .map(|(idx, (port, in_use, note))| {
            let mut style = Style::default().fg(theme.palette.text);
            if idx == directory.selected {
                style = style.add_modifier(Modifier::REVERSED);
            }
            let row = Rect::new(inner.x, inner.y + (idx - first) as u16, inner.width, 1);
            targets.push((row, PopupTarget::DirectoryRow(idx)));
            let mark = if *in_use { "●" } else { " " };
            let mut spans = vec![
                Span::styled(mark, style.fg(theme.palette.text_good)),
                Span::styled(" ", style),
                Span::styled(*port, style),
            ];
            if let Some((text, color)) = note {
                spans.push(Span::styled("  ", style));
                spans.push(Span::styled(text.as_str(), style.fg(*color)));
            }
            Line::from(spans)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
    let max_scroll = rows.len().saturating_sub(visible);
    if max_scroll > 0 {
        let bar = Rect {
            x: inner.right(),
            width: 1,
            ..inner
        }
        .intersection(area);
        render_scrollbar(frame, bar, (first, max_scroll, visible), theme);
    }
    area
}

/// The text after a port's name and its color
type Note = (String, Color);

/// A tested port's result after its name: running, free, held, refused or the error
fn note(state: Option<&PortState>, theme: &ThemeConfig) -> Note {
    let palette = &theme.palette;
    match state {
        None => ("testing…".to_string(), palette.text_muted),
        Some(PortState::Free) => ("free".to_string(), palette.text_good),
        Some(PortState::Busy(None)) => ("busy".to_string(), palette.text_warn),
        Some(PortState::Busy(Some(holder))) => (format!("busy: {holder}"), palette.text_warn),
        Some(PortState::NoAccess) => ("no access".to_string(), palette.text_warn),
        Some(PortState::Error(err)) => (err.clone(), palette.text_muted),
    }
}
