use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Clear, Padding, Paragraph, Tabs},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::App,
    configs::{KeyPattern, KeyResolver, ThemeConfig, keybindings::KeyBindingDialogAction},
    link::{LinkConfig, LinkKind, probe::ProbeState},
    overlays::{Directory, DirectoryTab, LinkForm, LinkFormField, PopupTarget},
    ui::{
        common::popup_area,
        popups::{
            Hints, form_area, hint_spans, key_hints, popup_block, render_scrollbar, row_area,
            section,
        },
    },
};

/// The tabs, in their order
const TABS: [(DirectoryTab, &str); 2] = [
    (DirectoryTab::Saved, "Saved links"),
    (DirectoryTab::Ports, "Serial ports"),
];
/// What an empty list shows
const EMPTY: &str = "No serial port found: plug one in, then r";
const NO_LINK: &str = "No saved link for this machine";
/// Under the tabs: what the test does, a port opened on Windows, which may reset a board, and a
/// TCP link connected to (MAX-186)
#[cfg(windows)]
pub const TEST_NOTE: &str = "t opens each port briefly (a board may reset), connects TCP";
#[cfg(not(windows))]
pub const TEST_NOTE: &str = "t finds who holds each port without opening it, connects TCP";

/// The directory (MAX-181): its tabs on top (MAX-185), then the tab's rows, `●` and "in use"
/// on the link in use, "opening…" or the last test's result after a row. The saved links in
/// columns, name, address and line (`raw TCP` for TCP), or the lines of why `links.toml`
/// didn't load; the serial ports. Both under the test's warning. The tab's buttons in the bottom
/// border. As wide as its longest row, as tall as the list, the screen's size at most; more
/// rows than room, a scrollbar in the right padding
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
    let buttons = |tab| {
        let mut buttons = vec![(
            dialog_key(KeyBindingDialogAction::Confirm),
            PopupTarget::DirectoryOpen,
            "Open",
        )];
        if tab == DirectoryTab::Saved {
            buttons.push((key('n'), PopupTarget::DirectoryNew, "New"));
            buttons.push((key('e'), PopupTarget::DirectoryEdit, "Edit"));
        }
        buttons.push((key('s'), PopupTarget::DirectorySave, "Save"));
        if tab == DirectoryTab::Saved {
            buttons.push((key('x'), PopupTarget::DirectoryDelete, "Delete"));
        }
        buttons.push((key('t'), PopupTarget::DirectoryTest, "Test"));
        if tab == DirectoryTab::Ports {
            buttons.push((key('r'), PopupTarget::DirectoryRefresh, "Refresh"));
        }
        buttons.push((
            dialog_key(KeyBindingDialogAction::Close),
            PopupTarget::Hint(KeyBindingDialogAction::Close),
            "Close",
        ));
        let buttons = buttons.into_iter().map(|(key, target, label)| {
            (key.map(|key| (key, target)).into_iter().collect(), label)
        });
        hint_spans(buttons, theme)
    };
    let (line, spots) = buttons(directory.tab);
    let hints = Hints {
        line,
        spots: Vec::new(),
    };

    // the rows of each tab, or why there's none, one row per line
    let muted = Style::default().fg(theme.palette.text_muted);
    let list = |tab| {
        let rows = rows(app, directory, tab, theme);
        let empty: Vec<Line> = match (&directory.links, tab) {
            (_, DirectoryTab::Ports) => vec![Line::styled(EMPTY, muted)],
            (Err(err), DirectoryTab::Saved) => (err.lines())
                .map(|line| Line::styled(line, Style::default().fg(theme.palette.text_warn)))
                .collect(),
            (Ok(_), DirectoryTab::Saved) => vec![Line::styled(NO_LINK, muted)],
        };
        (rows, empty)
    };
    // the columns and rows a tab takes: its list under its header, its note, its buttons
    let size = |tab| {
        let (rows, empty) = list(tab);
        let list_width = if rows.is_empty() {
            empty.iter().map(Line::width).max().unwrap_or(0)
        } else {
            (rows.iter())
                .map(|(text, _, note)| {
                    2 + text.width() + note.as_ref().map_or(0, |(n, _)| 2 + n.width())
                })
                .max()
                .unwrap_or(0)
        };
        let width = list_width
            .max(TEST_NOTE.width())
            .max(buttons(tab).0.width());
        let listed = if rows.is_empty() {
            empty.len()
        } else {
            rows.len()
        };
        (width as u16, listed.max(1) as u16 + HEADER)
    };
    let tab_widths: Vec<u16> = TABS.iter().map(|(_, t)| t.width() as u16 + 2).collect();
    let tabs_width = tab_widths.iter().sum::<u16>() + tab_widths.len() as u16 - 1;
    // the larger of both tabs, so switching keeps the size
    let (saved, ports) = (size(DirectoryTab::Saved), size(DirectoryTab::Ports));
    let width = saved.0.max(ports.0).max(tabs_width);
    // the borders and the padding, none on top: the tabs touch the top border
    let height = saved.1.max(ports.1) + 3;
    let area = popup_area(frame.area(), width + 4, height);
    let (rows, empty) = list(directory.tab);

    let block = popup_block("Directory", &hints, theme).padding(Padding::new(1, 1, 0, 1));
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

    render_tabs(directory.tab, &tab_widths, frame, inner, theme, targets);
    let note = Rect {
        y: inner.y + 1,
        height: 1,
        ..inner
    }
    .intersection(inner);
    frame.render_widget(Paragraph::new(Span::styled(TEST_NOTE, muted)), note);
    inner.y += HEADER;
    inner.height = inner.height.saturating_sub(HEADER);

    if rows.is_empty() {
        frame.render_widget(Paragraph::new(empty), inner);
        return area;
    }

    // from the row that keeps the selection in view; each row is a click target
    let selected = directory.selection();
    let visible = inner.height as usize;
    let first = (selected + 1).saturating_sub(visible);
    let lines: Vec<Line> = (rows.iter().enumerate().skip(first).take(visible))
        .map(|(idx, (text, in_use, note))| {
            let mut style = Style::default().fg(theme.palette.text);
            if idx == selected {
                style = style.add_modifier(Modifier::REVERSED);
            }
            let row = Rect::new(inner.x, inner.y + (idx - first) as u16, inner.width, 1);
            targets.push((row, PopupTarget::DirectoryRow(idx)));
            let mark = if *in_use { "●" } else { " " };
            let mut spans = vec![
                Span::styled(mark, style.fg(theme.palette.text_good)),
                Span::styled(" ", style),
                Span::styled(text.as_str(), style),
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

/// The text after a row and its color
type Note = (String, Color);

/// The rows above a tab's list: the tabs, the test's note, a blank row
const HEADER: u16 = 3;

/// A tab's rows: the text, whether it's the link in use, the note after it. A saved link's
/// columns are padded to the widest of each
fn rows(
    app: &App,
    directory: &Directory,
    tab: DirectoryTab,
    theme: &ThemeConfig,
) -> Vec<(String, bool, Option<Note>)> {
    let entries: Vec<(String, String)> = match tab {
        DirectoryTab::Ports => (directory.ports.iter())
            .map(|port| (port.clone(), port.clone()))
            .collect(),
        DirectoryTab::Saved => {
            let links = directory.links.as_deref().unwrap_or_default();
            let columns: Vec<[String; 3]> = (links.iter())
                .map(|(link, config)| [link.name.clone(), config.address.to_string(), line(config)])
                .collect();
            let width = |i: usize| columns.iter().map(|c| c[i].width()).max().unwrap_or(0);
            let (name_width, address_width) = (width(0), width(1));
            (links.iter().zip(&columns))
                .map(|((_, config), [name, address, line])| {
                    let text = format!("{name:name_width$}  {address:address_width$}  {line}");
                    (text, config.address.to_string())
                })
                .collect()
        }
    };
    // maxicom's link only while open: detached, lost or not picked, it's like the others
    let current = app
        .is_link_open()
        .then(|| app.link_config().address.to_string());
    (entries.into_iter())
        .map(|(text, address)| {
            let in_use = current.as_ref() == Some(&address);
            let note = if directory.opening.as_ref() == Some(&address) {
                Some(("opening…".to_string(), theme.palette.text_muted))
            } else if in_use {
                Some(("in use".to_string(), theme.palette.text_muted))
            } else {
                directory
                    .tests
                    .get(&address)
                    .map(|state| note(state.as_ref(), theme))
            };
            (text, in_use, note)
        })
        .collect()
}

/// A link's line settings as a row shows them: `115200 8N1`, `raw TCP` for TCP
fn line(config: &LinkConfig) -> String {
    match config.address.kind() {
        LinkKind::Tcp => "raw TCP".to_string(),
        _ => format!("{} {}", config.line.baud, config.line.framing),
    }
}

/// The form of a saved link (MAX-185): a row per field, the picked one marked, the cursor in
/// it when it's typed; the line's muted for an address that has none, saying why. A click on a
/// row picks its field
pub fn render_link_form_popup(
    form: &LinkForm,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let picked = Style::default()
        .fg(theme.palette.secondary)
        .add_modifier(Modifier::BOLD);
    let muted = Style::default().fg(theme.palette.text_muted);
    let line = &form.line.line;
    let [bits, _, stop_bits] = line.framing.compact_notation();
    let has_line = form.has_line();
    let value = |field| match field {
        LinkFormField::Name => form.name.value().to_string(),
        LinkFormField::Address => form.address.value().to_string(),
        LinkFormField::Hosts => form.hosts.value().to_string(),
        LinkFormField::Baud => form.baud.value().to_string(),
        LinkFormField::Bits => format!("< {bits} >"),
        LinkFormField::Parity => format!("< {:?} >", line.framing.parity),
        LinkFormField::StopBits => format!("< {stop_bits} >"),
        LinkFormField::Flow => format!("< {:?} >", line.flow_control),
    };
    let label = |field| match field {
        LinkFormField::Name => "Name",
        LinkFormField::Address => "Address",
        LinkFormField::Hosts => "For",
        LinkFormField::Baud => "Baud rate",
        LinkFormField::Bits => "Data bits",
        LinkFormField::Parity => "Parity",
        LinkFormField::StopBits => "Stop bits",
        LinkFormField::Flow => "Flow ctrl",
    };
    let title = if form.editing.is_some() {
        "Edit link"
    } else {
        "New link"
    };
    let mut text = vec![section(title, theme), Line::from("")];
    for field in LinkFormField::ALL {
        let is_picked = form.field == field;
        let style = match () {
            _ if field.is_line() && !has_line => muted,
            _ if is_picked => picked,
            _ => Style::default().fg(theme.palette.text),
        };
        let pointer = if is_picked { "▶ " } else { "  " };
        let mut spans = vec![
            Span::styled(pointer, picked),
            Span::styled(
                format!("{:<10}: ", label(field)),
                Style::default().fg(theme.palette.ternary),
            ),
            Span::styled(value(field), style),
        ];
        if field == LinkFormField::Hosts && form.hosts.value().trim().is_empty() {
            spans.push(Span::styled("every machine", muted));
        }
        text.push(Line::from(spans));
    }
    if !has_line {
        text.push(Line::from(""));
        text.push(Line::styled("  A TCP link's bridge sets its line", muted));
    }
    let hints = key_hints(
        resolver,
        true,
        &[
            (&[KeyBindingDialogAction::CycleNext], "Next"),
            (&[KeyBindingDialogAction::Confirm], "Save"),
            (&[KeyBindingDialogAction::Close], "Cancel"),
        ],
        theme,
    );
    let block = popup_block("Saved Link", &hints, theme);
    let area = form_area(frame.area(), 64, text.len());
    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(text), inner);
    hints.push_block_targets(area, targets);
    for (y, field) in LinkFormField::ALL.into_iter().enumerate() {
        targets.push((row_area(inner, 2 + y), PopupTarget::LinkFormField(field)));
    }
    if let Some(input) = form.input() {
        let y = LinkFormField::ALL
            .iter()
            .position(|f| *f == form.field)
            .unwrap_or(0);
        let x = inner.x + 2 + 12 + input.visual_cursor() as u16;
        frame.set_cursor_position((x, inner.y + 2 + y as u16));
    }
    area
}

/// The tab bar on the first row, each tab a click target
fn render_tabs(
    tab: DirectoryTab,
    widths: &[u16],
    frame: &mut Frame,
    inner: Rect,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) {
    let row = Rect { height: 1, ..inner }.intersection(inner);
    let titles: Vec<Line> = (TABS.iter())
        .map(|(this, title)| {
            let style = if *this == tab {
                Style::default()
                    .fg(theme.palette.secondary)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.palette.text)
            };
            Line::from(*title).style(style)
        })
        .collect();
    let selected = TABS.iter().position(|(this, _)| *this == tab);
    let bar = Tabs::new(titles)
        .style(*theme.popup.background)
        .select(selected)
        .divider(symbols::line::VERTICAL)
        .highlight_style(
            Style::default()
                .fg(theme.palette.secondary)
                .add_modifier(Modifier::UNDERLINED),
        );
    frame.render_widget(bar, row);
    let mut x = row.x;
    for ((this, _), width) in TABS.iter().zip(widths) {
        let target = Rect::new(x, row.y, *width, 1).intersection(row);
        targets.push((target, PopupTarget::DirectoryTab(*this)));
        x = x.saturating_add(width + 1);
    }
}

/// A tested link's result after its name: running, free, held and by whom, refused, missing,
/// a TCP link's answer, or the error (MAX-186)
fn note(state: Option<&ProbeState>, theme: &ThemeConfig) -> Note {
    let palette = &theme.palette;
    let (good, warn, muted) = (palette.text_good, palette.text_warn, palette.text_muted);
    let Some(state) = state else {
        return ("testing…".to_string(), muted);
    };
    match state {
        ProbeState::Free => ("free".to_string(), good),
        ProbeState::InUse => ("in use".to_string(), muted),
        ProbeState::Busy(None) => ("busy".to_string(), warn),
        ProbeState::Busy(Some(holder)) => (format!("busy: {holder}"), warn),
        ProbeState::NoAccess(None) => ("no access".to_string(), warn),
        ProbeState::NoAccess(Some(group)) => (format!("no access: join {group}"), warn),
        ProbeState::Missing => ("not found".to_string(), muted),
        ProbeState::NoLock => ("no lock".to_string(), good),
        ProbeState::Reachable => ("reachable".to_string(), good),
        ProbeState::Refused => ("refused".to_string(), warn),
        ProbeState::Unreachable => ("unreachable".to_string(), warn),
        ProbeState::UnknownHost => ("unknown host".to_string(), warn),
        ProbeState::NoAnswer => ("no answer".to_string(), muted),
        ProbeState::Error(err) => (err.clone(), muted),
    }
}
