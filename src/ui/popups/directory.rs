use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Clear, Paragraph, Tabs},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::App,
    configs::{
        KeyPattern, KeyResolver, LinkDefaults, ThemeConfig, keybindings::KeyBindingDialogAction,
    },
    link::{Address, LinkConfig, LinkKind, serial::PortState},
    overlays::{Directory, DirectoryTab, LinkNameDialog, PopupTarget},
    ui::{
        common::popup_area,
        popups::{Hints, form_area, hint_spans, key_hints, popup_block, render_scrollbar, section},
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
/// Under the tabs on the ports: what the test does, an open on Windows, which may reset a board
#[cfg(windows)]
pub const TEST_NOTE: &str = "t opens each port briefly: a board may reset";
#[cfg(not(windows))]
pub const TEST_NOTE: &str = "t finds who holds each port, without opening it";

/// The directory (MAX-181): its tabs on top (MAX-185), then the tab's rows, `●` and "in use"
/// on the link in use, "opening…" or the last test's result after a row. The saved links in
/// columns, name, address and line (`raw TCP` for TCP), or the lines of why `links.toml`
/// didn't load; the serial ports under the test's warning. The tab's buttons in the bottom
/// border. As wide as its longest row, as tall as the list, the screen's size at most; more
/// rows than room, a scrollbar in the right padding
pub fn render_directory_popup(
    app: &App,
    directory: &Directory,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let on_ports = directory.tab == DirectoryTab::Ports;
    let resolver = &app.key_resolver;
    let dialog_key = |action| resolver.dialog_keys(action).into_iter().next();
    let key = |c| {
        Some(KeyPattern {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::NONE,
        })
    };
    let mut buttons = vec![
        (
            dialog_key(KeyBindingDialogAction::Confirm),
            PopupTarget::DirectoryOpen,
            "Open",
        ),
        (key('s'), PopupTarget::DirectorySave, "Save"),
    ];
    if on_ports {
        buttons.push((key('t'), PopupTarget::DirectoryTest, "Test"));
        buttons.push((key('r'), PopupTarget::DirectoryRefresh, "Refresh"));
    }
    buttons.push((
        dialog_key(KeyBindingDialogAction::Close),
        PopupTarget::Hint(KeyBindingDialogAction::Close),
        "Close",
    ));
    let buttons = buttons
        .into_iter()
        .map(|(key, target, label)| (key.map(|key| (key, target)).into_iter().collect(), label));
    let (line, spots) = hint_spans(buttons, theme);
    let hints = Hints {
        line,
        spots: Vec::new(),
    };

    let rows = rows(app, directory, theme);
    // nothing to list: why, one row per line
    let muted = Style::default().fg(theme.palette.text_muted);
    let empty: Vec<Line> = match (&directory.links, on_ports) {
        (_, true) => vec![Line::styled(EMPTY, muted)],
        (Err(err), false) => (err.lines())
            .map(|line| Line::styled(line, Style::default().fg(theme.palette.text_warn)))
            .collect(),
        (Ok(_), false) => vec![Line::styled(NO_LINK, muted)],
    };
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
    let tab_widths: Vec<u16> = TABS.iter().map(|(_, t)| t.width() as u16 + 2).collect();
    let tabs_width = tab_widths.iter().sum::<u16>() + tab_widths.len() as u16 - 1;
    let note_width = if on_ports { TEST_NOTE.width() } else { 0 };
    let width = (list_width.max(note_width) as u16)
        .max(tabs_width)
        .max(hints.line.width() as u16);
    // the tabs and a blank row on top, the ports' note between; the borders and the padding
    let header = if on_ports { 3 } else { 2 };
    let listed = if rows.is_empty() {
        empty.len()
    } else {
        rows.len()
    };
    let height = listed.max(1) as u16 + header + 4;
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

    render_tabs(directory.tab, &tab_widths, frame, inner, theme, targets);
    if on_ports {
        let note = Rect {
            y: inner.y + 1,
            height: 1,
            ..inner
        }
        .intersection(inner);
        frame.render_widget(Paragraph::new(Span::styled(TEST_NOTE, muted)), note);
    }
    inner.y += header;
    inner.height = inner.height.saturating_sub(header);

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

/// The tab's rows: the text, whether it's the link in use, the note after it. A saved link's
/// columns are padded to the widest of each
fn rows(
    app: &App,
    directory: &Directory,
    theme: &ThemeConfig,
) -> Vec<(String, bool, Option<Note>)> {
    let entries: Vec<(String, String)> = match directory.tab {
        DirectoryTab::Ports => (directory.ports.iter())
            .map(|port| (port.clone(), port.clone()))
            .collect(),
        DirectoryTab::Saved => {
            let links = directory.links.as_deref().unwrap_or_default();
            let columns: Vec<[String; 3]> = (links.iter())
                .map(|(name, config)| [name.clone(), config.address.to_string(), line(config)])
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

/// The name a link is saved under (MAX-185): the name typed, what's saved under it, for which
/// machines
pub fn render_link_name_popup(
    dialog: &LinkNameDialog,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let style = Style::default()
        .fg(theme.palette.secondary)
        .add_modifier(Modifier::BOLD);
    let link = &dialog.link;
    // a serial link's fields are all set, from the link it saves
    let line_text = (link.address.parse::<Address>()).map_or_else(
        |_| String::new(),
        |address| {
            let line = link.over(&LinkDefaults::default()).line();
            self::line(&LinkConfig { address, line })
        },
    );
    let hosts = if link.hosts.is_empty() {
        "every machine".to_string()
    } else {
        link.hosts.join(", ")
    };
    let text = vec![
        section("Save link", theme),
        Line::from(""),
        Line::from(vec![
            Span::styled("▶ ", style),
            Span::raw("Name    : "),
            Span::styled(dialog.input.value(), style),
        ]),
        Line::from(format!("  Address : {}", link.address)),
        Line::from(format!("  Line    : {line_text}")),
        Line::from(format!("  For     : {hosts}")),
    ];
    let hints = key_hints(
        resolver,
        true,
        &[
            (&[KeyBindingDialogAction::Confirm], "Save"),
            (&[KeyBindingDialogAction::Close], "Cancel"),
        ],
        theme,
    );
    let block = popup_block("Save Link", &hints, theme);
    let area = form_area(frame.area(), 64, text.len());
    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(text), inner);
    hints.push_block_targets(area, targets);
    let cursor_x = inner.x + 2 + 10 + dialog.input.visual_cursor() as u16;
    frame.set_cursor_position((cursor_x, inner.y + 2));
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
