use chrono::{DateTime, Local};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::{
    capture::{NodeKind, Session, SessionNode},
    configs::{
        KeyPattern, KeyResolver, LineDisplayMode, ThemeConfig, keybindings::KeyBindingDialogAction,
    },
    overlays::{
        PopupTarget, SessionBrowser,
        sessions::{BrowserRow, BrowserView, sessions, size},
    },
    ui::{
        common::popup_area,
        data_display::format_line_content,
        popups::{Hints, hint_spans, popup_block, render_scrollbar, section},
    },
};

/// Width of the session list, the preview takes the rest
const LIST_WIDTH: u16 = 52;

/// The session browser (MAX-148): the sessions on the left, the selected one's details and
/// last lines on the right, its buttons in the bottom border. While it asks before deleting
/// all, the question is its last row and the buttons answer it
pub fn render_sessions_popup(
    browser: &SessionBrowser,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let parent = frame.area();
    let area = popup_area(
        parent,
        parent.width.saturating_sub(8).min(120),
        parent.height.saturating_sub(4).min(40),
    );

    let other_view = match browser.view {
        BrowserView::Tree => "List view",
        BrowserView::List => "Tree view",
    };
    // the browser's own keys, as typed: `D` shows as `D`, not `Shift-d`
    let key = |c| {
        Some(KeyPattern {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::NONE,
        })
    };
    let close = resolver
        .dialog_keys(KeyBindingDialogAction::Close)
        .into_iter()
        .next();
    let buttons = if browser.confirm_delete.is_some() {
        vec![
            (key('y'), PopupTarget::SessionConfirm, "Delete"),
            (key('n'), PopupTarget::SessionCancel, "Cancel"),
        ]
    } else {
        let mut buttons = vec![(key('p'), PopupTarget::SessionReplay, "Replay")];
        // while a replay runs, the way back to the live link it set aside (MAX-166)
        if browser.live {
            buttons.push((key('L'), PopupTarget::SessionBackToLive, "Back to live"));
        }
        buttons.extend([
            (key('d'), PopupTarget::SessionDelete, "Delete"),
            (key('D'), PopupTarget::SessionDeleteAll, "Delete all"),
            (key('s'), PopupTarget::SessionView, other_view),
            (
                close,
                PopupTarget::Hint(KeyBindingDialogAction::Close),
                "Close",
            ),
        ]);
        buttons
    };
    let as_hints = |buttons: Vec<(Option<KeyPattern>, PopupTarget, &'static str)>| {
        let buttons = buttons.into_iter().map(|(key, target, label)| {
            (key.map(|key| (key, target)).into_iter().collect(), label)
        });
        hint_spans(buttons, theme)
    };
    let (line, spots) = as_hints(buttons);
    let hints = Hints {
        line,
        spots: Vec::new(),
    };

    let block = popup_block("Session backups", &hints, theme);
    frame.render_widget(Clear, area);
    let mut inner = block.inner(area);
    frame.render_widget(block, area);
    // the buttons, where `popup_block` draws the hints
    let (x, y) = (area.x + 2, area.bottom().saturating_sub(1));
    for (start, width, target) in spots {
        let button = Rect::new(x.saturating_add(start), y, width, 1).intersection(area);
        if !button.is_empty() {
            targets.push((button, target));
        }
    }

    // the folder and its buttons on the line under it, to find the files out of the app
    // (MAX-154). The bottom border has no room left for them. They answer nothing while the
    // question is asked, so they are hidden then
    let muted = Style::default().fg(theme.palette.text_muted);
    let folder = Line::from(vec![
        Span::styled("Folder  ", muted),
        Span::styled(browser.dir.display().to_string(), muted),
    ]);
    frame.render_widget(Paragraph::new(folder), Rect { height: 1, ..inner });
    if browser.confirm_delete.is_none() {
        let (line, spots) = as_hints(vec![
            (key('c'), PopupTarget::SessionCopyLog, "Copy path"),
            (key('C'), PopupTarget::SessionCopyFolder, "Copy folder"),
            (key('r'), PopupTarget::SessionReload, "Reload"),
        ]);
        let at = Rect::new(inner.x, inner.y + 1, inner.width, 1).intersection(inner);
        // how full the folder is, on the right when it fits next to the buttons (MAX-202)
        let usage = browser.usage();
        let room = at.width as usize;
        if line.width() + 2 + usage.chars().count() <= room {
            let usage = Line::styled(usage, muted).right_aligned();
            frame.render_widget(Paragraph::new(usage), at);
        }
        frame.render_widget(Paragraph::new(line), at);
        for (start, width, target) in spots {
            let button = Rect::new(at.x.saturating_add(start), at.y, width, 1).intersection(at);
            if !button.is_empty() {
                targets.push((button, target));
            }
        }
    }
    inner.y += 3;
    inner.height = inner.height.saturating_sub(3);

    if let Some(confirm) = &browser.confirm_delete {
        let (count, bytes) = browser.deletable_in(&confirm.sessions);
        let group = (confirm.group.as_ref()).map_or(String::new(), |name| format!(" of {name}"));
        // only when some are: a question about one past session has nothing to add
        let kept = if count < confirm.sessions.len() {
            " The running ones are kept"
        } else {
            ""
        };
        let question = format!("Delete {}{group} ({})?{kept}", sessions(count), size(bytes));
        let at = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
        let warn = Style::default()
            .fg(theme.palette.text_warn)
            .add_modifier(Modifier::BOLD);
        frame.render_widget(Paragraph::new(Span::styled(question, warn)), at);
        inner.height = inner.height.saturating_sub(2);
    }

    if browser.sessions.is_empty() {
        let empty = format!("No session backups in {}", browser.dir.display());
        frame.render_widget(Paragraph::new(Span::styled(empty, muted)), inner);
        return area;
    }

    let list_width = LIST_WIDTH.min(inner.width / 2);
    let list = Rect::new(inner.x, inner.y, list_width, inner.height);
    let preview = Rect::new(
        inner.x + list_width + 2,
        inner.y,
        inner.width.saturating_sub(list_width + 2),
        inner.height,
    );
    render_list(browser, list, frame, theme, targets);
    render_preview(browser, preview, frame, theme);
    area
}

/// One row per node on show from the one that keeps the selection in view. More rows than
/// fit: a scrollbar in the gap after the list (MAX-175)
fn render_list(
    browser: &SessionBrowser,
    area: Rect,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) {
    let now = Local::now();
    let height = area.height as usize;
    let rows = browser.rows();
    let first = (browser.selected + 1).saturating_sub(height);
    let lines: Vec<Line> = (rows.iter().enumerate().skip(first).take(height))
        .map(|(idx, row)| {
            let row_area = Rect::new(area.x, area.y + (idx - first) as u16, area.width, 1);
            targets.push((row_area, PopupTarget::SessionRow(idx)));
            let line = match browser.view {
                BrowserView::Tree => tree_row(browser, row, area.width, now, theme),
                BrowserView::List => list_row(browser, row.node, area.width, now, theme),
            };
            if idx == browser.selected {
                selected_bar(&line, area.width, theme)
            } else {
                line
            }
        })
        .collect();
    // a column left before the scrollbar, so a long port doesn't touch it
    let text = Rect {
        width: area.width.saturating_sub(1),
        ..area
    };
    frame.render_widget(Paragraph::new(lines), text);

    let max_scroll = rows.len().saturating_sub(height);
    if max_scroll > 0 {
        let bar = Rect {
            x: area.right(),
            width: 1,
            ..area
        };
        render_scrollbar(frame, bar, (first, max_scroll, height), theme);
    }
}

/// The selected row as one bar as wide as the list, in one color as the other popups' lists:
/// its columns' many colors don't read reversed, and the details beside it keep them. A column
/// is left before the scrollbar
fn selected_bar<'a>(line: &Line, width: u16, theme: &ThemeConfig) -> Line<'a> {
    let text: String = line.iter().map(|span| span.content.as_ref()).collect();
    let room = usize::from(width).saturating_sub(1);
    let style = Style::default()
        .fg(theme.palette.text)
        .add_modifier(Modifier::REVERSED);
    Line::styled(format!("{text:<room$}"), style)
}

/// A row's mark, the same for a group that holds such a session, which it can hide: `●`
/// running, `○` the session restored into the history, `?` no meta file
fn mark(browser: &SessionBrowser, node: &SessionNode, theme: &ThemeConfig) -> (char, Color) {
    let colors = &theme.session_browser;
    let any = |test: &dyn Fn(&Session) -> bool| {
        (node.sessions.iter()).any(|&index| test(&browser.sessions[index]))
    };
    let restored = browser.restored.as_deref();
    if any(&|session| session.running) {
        ('●', colors.mark_running)
    } else if any(&|session| restored == Some(session.stem.as_str())) {
        ('○', colors.mark_restored)
    } else if any(&|session| session.meta.is_none()) {
        ('?', colors.mark_no_meta)
    } else {
        (' ', theme.palette.text)
    }
}

/// A row of the tree (MAX-202): its mark, an arrow on a group, its label under its parent's,
/// then its count of sessions and its size on the right. The count is a bare number in a list
/// narrower than `LIST_WIDTH`, and the label is cut when it doesn't fit
fn tree_row<'a>(
    browser: &SessionBrowser,
    row: &BrowserRow,
    width: u16,
    now: DateTime<Local>,
    theme: &ThemeConfig,
) -> Line<'a> {
    let node = row.node;
    let Some(newest) = node.sessions.last().map(|&index| &browser.sessions[index]) else {
        return Line::default();
    };
    let (mark, mark_color) = mark(browser, node, theme);
    let arrow = match (node.kind, row.open) {
        (NodeKind::Session, _) => ' ',
        (_, true) => '▾',
        (_, false) => '▸',
    };
    let label_color = match node.kind {
        NodeKind::Device => theme.palette.link_color(newest.port_tag()),
        _ => date_color(newest, now, theme),
    };
    let count = match node.kind {
        NodeKind::Session => String::new(),
        _ if width >= LIST_WIDTH => sessions(node.sessions.len()),
        _ => node.sessions.len().to_string(),
    };
    let size_text = size(node.size);
    let size_pad = 1 + 5usize.saturating_sub(size_text.chars().count());
    // a column left before the scrollbar
    let width = usize::from(width).saturating_sub(1);
    let indent = 1 + 2 * row.depth;
    let left = 1 + indent + 2;
    let right = count.chars().count() + size_pad + size_text.chars().count();
    let label = cut(&node.label, width.saturating_sub(left + right + 1));
    let pad = width.saturating_sub(left + label.chars().count() + right);
    let cell = |text: String, color| Span::styled(text, Style::default().fg(color));
    let gap = |width: usize| cell(" ".repeat(width), theme.palette.text);
    Line::from(vec![
        cell(mark.to_string(), mark_color),
        gap(indent),
        cell(arrow.to_string(), theme.palette.text_muted),
        gap(1),
        cell(label, label_color),
        gap(pad),
        cell(count, theme.palette.text_muted),
        gap(size_pad),
        cell(size_text, theme.session_browser.size_color(node.size)),
    ])
}

/// `text` in `room` columns, a `…` for the part cut
fn cut(text: &str, room: usize) -> String {
    if text.chars().count() <= room {
        return text.to_string();
    }
    let kept = text.chars().take(room.saturating_sub(1));
    kept.chain((room > 0).then_some('…')).collect()
}

/// A row of the list, its mark first. Its columns are colored as `lsd -l` does (MAX-153): the
/// date by the age of the last write, the size by its magnitude, the port by its name, cut
/// when it doesn't fit
fn list_row<'a>(
    browser: &SessionBrowser,
    node: &SessionNode,
    width: u16,
    now: DateTime<Local>,
    theme: &ThemeConfig,
) -> Line<'a> {
    let Some(session) = node.sessions.first().map(|&index| &browser.sessions[index]) else {
        return Line::default();
    };
    let (mark, mark_color) = mark(browser, node, theme);
    let started = session.started.map_or_else(
        || "????-??-?? ??:??".to_string(),
        |time| time.format("%Y-%m-%d %H:%M").to_string(),
    );
    // only the text is colored: the gaps and the size's padding keep the text color
    let cell = |text: String, color| Span::styled(text, Style::default().fg(color));
    let gap = |width: usize| cell(" ".repeat(width), theme.palette.text);
    let size_text = size(session.size);
    let pad = 1 + 5usize.saturating_sub(size_text.chars().count());
    // what the port has left, a column before the scrollbar
    let used = 2 + started.chars().count() + pad + size_text.chars().count() + 1;
    let room = usize::from(width).saturating_sub(used + 1);
    Line::from(vec![
        cell(mark.to_string(), mark_color),
        gap(1),
        cell(started, date_color(session, now, theme)),
        gap(pad),
        cell(size_text, theme.session_browser.size_color(session.size)),
        gap(1),
        cell(
            cut(session.port_tag(), room),
            theme.palette.link_color(session.port_tag()),
        ),
    ])
}

/// The date's color by the age of the session's last write, muted when its times aren't known.
/// A last write in the future (a clock set back) is as new as `now`
fn date_color(session: &Session, now: DateTime<Local>, theme: &ThemeConfig) -> Color {
    match (session.started, session.ended) {
        (Some(_), Some(ended)) => theme
            .session_browser
            .date_color((now - ended).to_std().unwrap_or_default()),
        _ => theme.palette.text_muted,
    }
}

/// The selected session's details from its meta file, then its last text lines that fit; a
/// group's count, time span and size
fn render_preview(browser: &SessionBrowser, area: Rect, frame: &mut Frame, theme: &ThemeConfig) {
    let muted = Style::default().fg(theme.palette.text_muted);
    let field = |name: &'static str, value: String, style: Style| {
        Line::from(vec![
            Span::styled(format!("{name:<8}"), muted),
            Span::styled(value, style),
        ])
    };
    let mut lines = match (browser.selected_session(), browser.selected_row()) {
        (Some(session), _) => details(browser, session, theme, field, muted),
        (None, Some(row)) => group_details(browser, row.node, theme, field),
        (None, None) => return,
    };
    lines.push(Line::from(""));
    lines.push(section("Last lines", theme));
    let room = (area.height as usize).saturating_sub(lines.len());
    let tail = browser.preview.len().saturating_sub(room);
    let dim = Style::default().add_modifier(Modifier::DIM);
    lines.extend(browser.preview[tail..].iter().map(|line| {
        let Some(index) = line.session else {
            return Line::styled(line.text.clone(), muted);
        };
        // the timestamp its session wrote before each line, dimmed: as wide as its format
        // gives, a space after it. The sessions of a group can differ
        let stamp_width = (browser.sessions[index].meta.as_ref())
            .and_then(|meta| meta.text.timestamp.as_deref())
            .map(|format| Local::now().format(format).to_string().len() + 1);
        let (stamp, text) = match stamp_width {
            Some(width) if line.text.is_char_boundary(width) => line.text.split_at(width),
            _ => ("", line.text.as_str()),
        };
        let mut spans = vec![Span::styled(stamp.to_string(), dim)];
        // the text as Pretty shows it: in its colors, the other escape sequences left out
        spans.extend(format_line_content(text, LineDisplayMode::Pretty, theme));
        Line::from(spans)
    }));
    frame.render_widget(Paragraph::new(lines), area);
}

fn group_details<'a>(
    browser: &SessionBrowser,
    node: &SessionNode,
    theme: &ThemeConfig,
    field: impl Fn(&'static str, String, Style) -> Line<'a>,
) -> Vec<Line<'a>> {
    let plain = Style::default();
    let size_color = theme.session_browser.size_color(node.size);
    let newest = node.sessions.last().map(|&index| &browser.sessions[index]);
    let date = newest.map_or(theme.palette.text_muted, |newest| {
        date_color(newest, Local::now(), theme)
    });
    vec![
        field("Group", node.label.clone(), plain),
        field("Holds", sessions(node.sessions.len()), plain),
        field(
            "Span",
            node.time_span(&browser.sessions),
            Style::default().fg(date),
        ),
        field("Size", size(node.size), Style::default().fg(size_color)),
    ]
}

fn details<'a>(
    browser: &SessionBrowser,
    session: &Session,
    theme: &ThemeConfig,
    field: impl Fn(&'static str, String, Style) -> Line<'a>,
    muted: Style,
) -> Vec<Line<'a>> {
    // the port, the times and the size are colored as in the list (MAX-153)
    let colors = &theme.session_browser;
    let plain = Style::default();
    let mut lines = Vec::new();
    match &session.meta {
        Some(meta) => {
            let device = &meta.device;
            // a link whose bridge sets the line shows what it is, as in the status bar (MAX-156)
            let line = match device.line {
                Some(line) => {
                    let framing = line.framing;
                    format!("{} {framing} {}", line.baud, device.encoding.as_str())
                }
                None => match device.address.kind().line_stand_in() {
                    Some([baud, framing]) => {
                        format!("{baud} {framing} {}", device.encoding.as_str())
                    }
                    None => device.encoding.as_str().to_string(),
                },
            };
            let port = Style::default().fg(theme.palette.link_color(session.port_tag()));
            lines.push(field("Port", device.address.to_string(), port));
            lines.push(field("Line", line, plain));
            lines.push(field("Host", device.host.clone(), plain));
            let timestamp = meta.text.timestamp.clone();
            lines.push(field(
                "Times",
                timestamp.unwrap_or("none".to_string()),
                plain,
            ));
            let tx = match &meta.text.tx_mark {
                Some(mark) => format!("{mark:?}"),
                None => "not recorded".to_string(),
            };
            lines.push(field("TX mark", tx, plain));
        }
        None => lines.push(Line::from(Span::styled(
            "No meta file: from before the session restore",
            muted,
        ))),
    }
    let date = Style::default().fg(date_color(session, Local::now(), theme));
    lines.push(field("Session", session.time_span(), date));
    let size_style = Style::default().fg(colors.size_color(session.size));
    lines.push(field("Size", size(session.size), size_style));
    let state = if session.running {
        "running, can't be deleted"
    } else if browser.restored.as_deref() == Some(session.stem.as_str()) {
        "restored into the history when the device opened"
    } else {
        "past"
    };
    lines.push(field("State", state.to_string(), plain));
    lines
}
