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
    link::{Address, LinkConfig, LinkKind, probe::ProbeState},
    overlays::{Directory, DirectoryTab, LinkForm, LinkFormField, PopupTarget},
    ui::{
        common::popup_area,
        popups::{
            Hints, cut, form_area, hint_spans, key_hints, popup_block, render_scrollbar, row_area,
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
pub const TEST_NOTE: &str = "  t opens each port briefly (a board may reset), connects TCP";
#[cfg(not(windows))]
pub const TEST_NOTE: &str = "  t finds who holds each port without opening it, connects TCP";

/// The directory (MAX-181): its tabs on top (MAX-185), then the tab's rows, `●` and "in use"
/// on the link in use, "opening…" or the last test's result in its State column. Each tab a
/// table under its headers (MAX-188): the saved links' name, address and line (`raw TCP` for
/// TCP), or the lines of why `links.toml` didn't load; the serial ports' name, device and
/// `vid:pid`. Both under the test's warning. The tab's buttons in the bottom
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

    // the table of each tab, or why there's none, one row per line; a table wider than the
    // screen cut in its long column
    let muted = Style::default().fg(theme.palette.text_muted);
    let max_width = frame.area().width.saturating_sub(4) as usize;
    let list = |tab| {
        let mut table = Table::new(app, directory, tab, theme);
        table.fit(max_width);
        let empty: Vec<Line> = match (&directory.links, tab) {
            (_, DirectoryTab::Ports) => vec![Line::styled(EMPTY, muted)],
            (Err(err), DirectoryTab::Saved) => (err.lines())
                .map(|line| Line::styled(line, Style::default().fg(theme.palette.text_warn)))
                .collect(),
            (Ok(_), DirectoryTab::Saved) => vec![Line::styled(NO_LINK, muted)],
        };
        (table, empty)
    };
    // the columns and rows a tab takes: its table (header and rows) under the tabs and its
    // note, its buttons
    let size = |tab| {
        let (table, empty) = list(tab);
        let (list_width, listed) = if table.rows.is_empty() {
            let width = empty.iter().map(Line::width).max().unwrap_or(0);
            (width, empty.len().max(1))
        } else {
            (table.width(), table.rows.len() + 1)
        };
        let width = list_width
            .max(TEST_NOTE.width())
            .max(buttons(tab).0.width());
        // a blank row and the selected row's long cell in full under a table cut
        let full = if table.is_cut() { 2 } else { 0 };
        (width as u16, (listed + full) as u16 + HEADER)
    };
    let tab_widths: Vec<u16> = TABS.iter().map(|(_, t)| t.width() as u16 + 2).collect();
    let tabs_width = tab_widths.iter().sum::<u16>() + tab_widths.len() as u16 - 1;
    // the larger of both tabs, so switching keeps the size
    let (saved, ports) = (size(DirectoryTab::Saved), size(DirectoryTab::Ports));
    // never narrower than it was: a state gets shorter as a test ends
    let width = (saved.0.max(ports.0).max(tabs_width)).max(directory.width.get());
    directory.width.set(width);
    // the borders and the padding, none on top: the tabs touch the top border
    let height = saved.1.max(ports.1) + 3;
    let area = popup_area(frame.area(), width + 4, height);
    let (table, empty) = list(directory.tab);

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
        y: inner.y + 2,
        height: 1,
        ..inner
    }
    .intersection(inner);
    // its key as the hints show one
    let key = Style::default()
        .fg(theme.palette.secondary)
        .add_modifier(Modifier::BOLD);
    let (indent, rest) = TEST_NOTE.split_at(2);
    let (t, rest) = rest.split_at(1);
    let line = Line::from(vec![
        Span::raw(indent),
        Span::styled(t, key),
        Span::styled(rest, muted),
    ]);
    frame.render_widget(Paragraph::new(line), note);
    inner.y += HEADER;
    inner.height = inner.height.saturating_sub(HEADER);

    if table.rows.is_empty() {
        frame.render_widget(Paragraph::new(empty), inner);
        return area;
    }
    // a table narrower than the popup (the other tab's wider) centered, its State column
    // as wide as its widest state now, its longest fixed one at least
    let offset = (inner.width as usize).saturating_sub(table.width()) as u16 / 2;
    let centered = |area: Rect| Rect {
        x: area.x + offset,
        width: area.width - offset,
        ..area
    };
    // the selected row's long cell in full on the last row, under a blank one, at the
    // table's first column, when the table cuts it (the rows kept when another row's is cut)
    let selected = directory.selection();
    if table.is_cut() {
        inner.height = inner.height.saturating_sub(2);
        let full = centered(Rect {
            y: inner.bottom() + 1,
            height: 1,
            ..inner
        })
        .intersection(area);
        let header = Style::default()
            .fg(theme.palette.text_muted)
            .add_modifier(Modifier::BOLD);
        let long = (table.rows.get(selected))
            .map(|row| &row.cells[1])
            .filter(|cell| cell.width() > table.widths[1]);
        if let Some(text) = long {
            let label = format!("  {}  ", table.headers[1]);
            let width = usize::from(full.width).saturating_sub(label.width());
            let line = Line::from(vec![
                Span::styled(label, header),
                Span::styled(cut(text, width), Style::default().fg(theme.palette.text)),
            ]);
            frame.render_widget(Paragraph::new(line), full);
        }
    }
    let header = Rect { height: 1, ..inner }.intersection(inner);
    frame.render_widget(Paragraph::new(table.header(theme)), centered(header));
    inner.y += 1;
    inner.height = inner.height.saturating_sub(1);

    // from the row that keeps the selection in view; each row is a click target
    let visible = inner.height as usize;
    let first = (selected + 1).saturating_sub(visible);
    let lines: Vec<Line> = (table.rows.iter().enumerate().skip(first).take(visible))
        .map(|(idx, row)| {
            let row_area = Rect::new(inner.x, inner.y + (idx - first) as u16, inner.width, 1);
            targets.push((row_area, PopupTarget::DirectoryRow(idx)));
            table.line(row, idx == selected, theme)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), centered(inner));
    let max_scroll = table.rows.len().saturating_sub(visible);
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

/// The rows above a tab's table: the tabs, a blank row, the test's note, a blank row
const HEADER: u16 = 4;

/// Between two columns
const GAP: usize = 2;
/// The State column at least: its longest state but the ones naming who holds the port
const STATE_WIDTH: usize = 12;
/// The second column (address, device) at most: a longer cell is cut, in full under the table
/// when its row is selected
const LONG_WIDTH: usize = 30;

/// A tab's row: a cell per column but the state, a saved link's color, whether it's the link in
/// use, its state
struct Row {
    cells: [String; 3],
    color: Option<Color>,
    in_use: bool,
    state: Option<Note>,
}

/// A tab's rows in columns under their headers (MAX-188): a saved link's name, address and
/// line, a port's name, device and `vid:pid`, then the state of both. Each column as wide as
/// its widest cell or header, none for a column empty on every row but the first and the
/// state's; the state's as wide as `STATE_WIDTH` at least, the second `LONG_WIDTH` at most
struct Table {
    headers: [&'static str; 4],
    rows: Vec<Row>,
    widths: [usize; 4],
}

impl Table {
    fn new(app: &App, directory: &Directory, tab: DirectoryTab, theme: &ThemeConfig) -> Self {
        let headers = match tab {
            DirectoryTab::Saved => ["Name", "Address", "Line", "State"],
            DirectoryTab::Ports => ["Port", "Device", "ID", "State"],
        };
        let rows = rows(app, directory, tab, theme);
        let mut widths = [0; 4];
        for (i, width) in widths.iter_mut().enumerate() {
            let cell = |row: &Row| match i {
                3 => row.state.as_ref().map_or(0, |(text, _)| text.width()),
                _ => row.cells[i].width(),
            };
            let widest = rows.iter().map(cell).max().unwrap_or(0);
            *width = match i {
                0 => widest.max(headers[i].width()),
                1 if widest > 0 => widest.min(LONG_WIDTH).max(headers[i].width()),
                2 if widest > 0 => widest.max(headers[i].width()),
                3 => widest.max(STATE_WIDTH),
                _ => 0,
            };
        }
        Self {
            headers,
            rows,
            widths,
        }
    }

    /// The mark, then the columns shown and their gaps
    fn width(&self) -> usize {
        let shown = self.widths.iter().filter(|w| **w > 0);
        2 + shown.clone().sum::<usize>() + GAP * shown.count().saturating_sub(1)
    }

    /// Whether a row's second cell is cut
    fn is_cut(&self) -> bool {
        (self.rows.iter()).any(|row| row.cells[1].width() > self.widths[1])
    }

    /// The second column (address, device) narrowed to fit in `max`, to its header at least
    fn fit(&mut self, max: usize) {
        let over = self.width().saturating_sub(max);
        if over > 0 && self.widths[1] > 0 {
            let least = self.headers[1].width();
            self.widths[1] = self.widths[1].saturating_sub(over).max(least);
        }
    }

    /// The cells of a row padded to their columns, the gaps between them, all in `style`
    fn spans<'a>(&self, cells: [(&str, Style); 4], style: Style) -> Vec<Span<'a>> {
        let mut spans = Vec::new();
        let shown = (cells.into_iter().zip(self.widths)).filter(|(_, width)| *width > 0);
        for (i, ((text, cell_style), width)) in shown.enumerate() {
            if i > 0 {
                spans.push(Span::styled(" ".repeat(GAP), style));
            }
            let text = cut(text, width);
            let pad = width.saturating_sub(text.width());
            spans.push(Span::styled(
                format!("{text}{}", " ".repeat(pad)),
                cell_style,
            ));
        }
        spans
    }

    fn header<'a>(&self, theme: &ThemeConfig) -> Line<'a> {
        let style = Style::default()
            .fg(theme.palette.text_muted)
            .add_modifier(Modifier::BOLD);
        let cells = self.headers.map(|header| (header, style));
        let mut spans = vec![Span::raw("  ")];
        spans.extend(self.spans(cells, style));
        Line::from(spans)
    }

    /// A row's line: `●` on the link in use, then its cells, the name in the link's color, the
    /// state in its own. The selected row is one bar, as the session browser's: a color doesn't
    /// read reversed
    fn line<'a>(&self, row: &Row, selected: bool, theme: &ThemeConfig) -> Line<'a> {
        let plain = Style::default().fg(theme.palette.text);
        let bar = plain.add_modifier(Modifier::REVERSED);
        let mark = if row.in_use { "●" } else { " " };
        let (state, color) = row
            .state
            .as_ref()
            .map_or(("", theme.palette.text), |(text, color)| {
                (text.as_str(), *color)
            });
        let colored = |color: Color| if selected { bar } else { plain.fg(color) };
        let style = if selected { bar } else { plain };
        let [a, b, c] = &row.cells;
        let cells = [
            (a.as_str(), colored(row.color.unwrap_or(theme.palette.text))),
            (b.as_str(), style),
            (c.as_str(), style),
            (state, colored(color)),
        ];
        let mut spans = vec![
            Span::styled(mark, colored(theme.palette.text_good)),
            Span::styled(" ", style),
        ];
        spans.extend(self.spans(cells, style));
        Line::from(spans)
    }
}

/// A tab's rows, a saved link in its color (MAX-201), the link in use marked, the state after:
/// "opening…", "in use" or the last test's result
fn rows(app: &App, directory: &Directory, tab: DirectoryTab, theme: &ThemeConfig) -> Vec<Row> {
    let entries: Vec<([String; 3], String, Option<Color>)> = match tab {
        DirectoryTab::Ports => (directory.ports.iter())
            .map(|port| {
                let details = directory.details.get(port).cloned().unwrap_or_default();
                let cells = [
                    port.clone(),
                    details.device.unwrap_or_default(),
                    details.id.unwrap_or_default(),
                ];
                (cells, port.clone(), None)
            })
            .collect(),
        DirectoryTab::Saved => (directory.links.as_deref().unwrap_or_default().iter())
            .map(|(link, config)| {
                let address = config.address.to_string();
                let color = (link.color)
                    .unwrap_or_else(|| theme.palette.link_color(&config.address.file_tag()));
                let cells = [link.name.clone(), address.clone(), line(config)];
                (cells, address, Some(color))
            })
            .collect(),
    };
    // maxicom's link only while open: detached, lost or not picked, it's like the others
    let current = app
        .is_link_open()
        .then(|| app.link_config().address.to_string());
    (entries.into_iter())
        .map(|(cells, address, color)| {
            let in_use = current.as_ref() == Some(&address);
            let state = if directory.opening.as_ref() == Some(&address) {
                Some(("opening…".to_string(), theme.palette.text_muted))
            } else if in_use {
                Some(("in use".to_string(), theme.palette.text_muted))
            } else {
                directory
                    .tests
                    .get(&address)
                    .map(|state| note(state.as_ref(), theme))
            };
            Row {
                cells,
                color,
                in_use,
                state,
            }
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
        LinkFormField::Color => form.color.value().to_string(),
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
        LinkFormField::Color => "Color",
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
        if field == LinkFormField::Color {
            spans.extend(color_sample(form, theme));
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

/// After the form's color: a sample of it, or of the palette's when none is typed; what is
/// wrong when it isn't a color (MAX-201)
fn color_sample<'a>(form: &LinkForm, theme: &ThemeConfig) -> Vec<Span<'a>> {
    let muted = Style::default().fg(theme.palette.text_muted);
    let sample = |color| Span::styled(" ██", Style::default().fg(color));
    match form.typed_color() {
        Ok(Some(color)) => vec![sample(color)],
        Ok(None) => {
            let address = form.address.value().trim().parse::<Address>();
            let palette = address.map(|address| theme.palette.link_color(&address.file_tag()));
            let mut spans = vec![Span::styled("from the palette", muted)];
            spans.extend(palette.ok().map(sample));
            spans
        }
        Err(_) => vec![Span::styled(
            " not a color",
            Style::default().fg(theme.palette.text_warn),
        )],
    }
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
