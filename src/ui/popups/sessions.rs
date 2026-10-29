use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::{
    configs::{KeyPattern, KeyResolver, ThemeConfig, keybindings::KeyBindingDialogAction},
    logger::{Session, SessionOrder},
    overlays::{
        PopupTarget, SessionBrowser,
        sessions::{sessions, size},
    },
    serial::SerialConfig,
    ui::{
        common::popup_area,
        popups::{Hints, hint_spans, popup_block, section},
    },
};

/// Width of the session list, the preview takes the rest
const LIST_WIDTH: u16 = 44;

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

    let sort = match browser.order {
        SessionOrder::Date => "Sort by device",
        SessionOrder::Device => "Sort by date",
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
    let buttons = if browser.confirm_delete_all {
        vec![
            (key('y'), PopupTarget::SessionConfirm, "Delete"),
            (key('n'), PopupTarget::SessionCancel, "Cancel"),
        ]
    } else {
        vec![
            (key('d'), PopupTarget::SessionDelete, "Delete"),
            (key('D'), PopupTarget::SessionDeleteAll, "Delete all"),
            (key('s'), PopupTarget::SessionSort, sort),
            (
                close,
                PopupTarget::Hint(KeyBindingDialogAction::Close),
                "Close",
            ),
        ]
    };
    let buttons = buttons
        .into_iter()
        .map(|(key, target, label)| (key.map(|key| (key, target)).into_iter().collect(), label));
    let (line, spots) = hint_spans(buttons, theme);
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

    if browser.confirm_delete_all {
        let (count, bytes) = browser.deletable();
        let question = format!(
            "Delete {} ({})? The running ones are kept",
            sessions(count),
            size(bytes)
        );
        let at = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
        let warn = Style::default()
            .fg(theme.palette.text_warn)
            .add_modifier(Modifier::BOLD);
        frame.render_widget(Paragraph::new(Span::styled(question, warn)), at);
        inner.height = inner.height.saturating_sub(2);
    }

    if browser.sessions.is_empty() {
        let empty = format!("No session backups in {}", browser.dir.display());
        let muted = Style::default().fg(theme.palette.text_muted);
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

/// One row per session from the one that keeps the selection in view, its marks first:
/// `●` running, `↺` what this device would restore, `?` no meta file
fn render_list(
    browser: &SessionBrowser,
    area: Rect,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) {
    let height = area.height as usize;
    let first = (browser.selected + 1).saturating_sub(height);
    let rows = browser.sessions.iter().enumerate().skip(first).take(height);
    let lines: Vec<Line> = rows
        .map(|(idx, session)| {
            let mark = if session.running {
                '●'
            } else if browser.restorable.as_deref() == Some(session.stem.as_str()) {
                '↺'
            } else if session.meta.is_none() {
                '?'
            } else {
                ' '
            };
            let started = session.started.map_or_else(
                || "????-??-?? ??:??".to_string(),
                |time| time.format("%Y-%m-%d %H:%M").to_string(),
            );
            let row = format!(
                "{mark} {started} {:>5}  {}",
                size(session.size),
                session.port_tag()
            );
            let mut style = Style::default().fg(if session.meta.is_some() {
                theme.palette.text
            } else {
                theme.palette.text_muted
            });
            if idx == browser.selected {
                style = style.add_modifier(Modifier::REVERSED);
            }
            let row_area = Rect::new(area.x, area.y + (idx - first) as u16, area.width, 1);
            targets.push((row_area, PopupTarget::SessionRow(idx)));
            Line::from(Span::styled(row, style))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

/// The selected session's details from its meta file, then its last text lines that fit
fn render_preview(browser: &SessionBrowser, area: Rect, frame: &mut Frame, theme: &ThemeConfig) {
    let Some(session) = browser.selected_session() else {
        return;
    };
    let muted = Style::default().fg(theme.palette.text_muted);
    let field = |name: &'static str, value: String| {
        Line::from(vec![
            Span::styled(format!("{name:<8}"), muted),
            Span::raw(value),
        ])
    };
    let mut lines = details(browser, session, field, muted);
    lines.push(Line::from(""));
    lines.push(section("Last lines", theme));
    let room = (area.height as usize).saturating_sub(lines.len());
    let tail = browser.preview.len().saturating_sub(room);
    lines.extend(
        browser.preview[tail..]
            .iter()
            .map(|line| Line::from(line.as_str())),
    );
    frame.render_widget(Paragraph::new(lines), area);
}

fn details<'a>(
    browser: &SessionBrowser,
    session: &Session,
    field: impl Fn(&'static str, String) -> Line<'a>,
    muted: Style,
) -> Vec<Line<'a>> {
    let mut lines = Vec::new();
    match &session.meta {
        Some(meta) => {
            let device = &meta.device;
            let framing: String = SerialConfig {
                port: device.port.clone(),
                baudrate: device.baud,
                char_size: device.char_size,
                stop_bits: device.stop_bits,
                parity: device.parity,
                ..SerialConfig::default()
            }
            .compact_notation()
            .iter()
            .collect();
            lines.push(field("Port", device.port.clone()));
            lines.push(field(
                "Line",
                format!("{} {framing} {}", device.baud, device.encoding.as_str()),
            ));
            lines.push(field("Host", device.host.clone()));
            let timestamp = meta.text.timestamp.clone();
            lines.push(field("Times", timestamp.unwrap_or("none".to_string())));
            let tx = match &meta.text.tx_mark {
                Some(mark) => format!("{mark:?}"),
                None => "not recorded".to_string(),
            };
            lines.push(field("TX mark", tx));
        }
        None => lines.push(Line::from(Span::styled(
            "No meta file: from before the session restore",
            muted,
        ))),
    }
    lines.push(field("Session", session.time_span()));
    lines.push(field("Size", size(session.size)));
    let state = if session.running {
        "running, can't be deleted"
    } else if browser.restorable.as_deref() == Some(session.stem.as_str()) {
        "what this device would restore"
    } else {
        "past"
    };
    lines.push(field("State", state.to_string()));
    lines
}
