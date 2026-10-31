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
    capture::{Session, SessionOrder},
    configs::{KeyPattern, KeyResolver, ThemeConfig, keybindings::KeyBindingDialogAction},
    overlays::{
        PopupTarget, SessionBrowser,
        sessions::{sessions, size},
    },
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
        let mut buttons = vec![(key('p'), PopupTarget::SessionReplay, "Replay")];
        // while a replay runs, the way back to the live link it set aside (MAX-166)
        if browser.live {
            buttons.push((key('L'), PopupTarget::SessionBackToLive, "Back to live"));
        }
        buttons.extend([
            (key('d'), PopupTarget::SessionDelete, "Delete"),
            (key('D'), PopupTarget::SessionDeleteAll, "Delete all"),
            (key('s'), PopupTarget::SessionSort, sort),
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
    if !browser.confirm_delete_all {
        let (line, spots) = as_hints(vec![
            (key('c'), PopupTarget::SessionCopyLog, "Copy path"),
            (key('C'), PopupTarget::SessionCopyFolder, "Copy folder"),
            (key('r'), PopupTarget::SessionReload, "Reload"),
        ]);
        let at = Rect::new(inner.x, inner.y + 1, inner.width, 1).intersection(inner);
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
/// `●` running, `↺` what this device would restore, `?` no meta file. Its columns are
/// colored as `lsd -l` does (MAX-153): the date by the age of the last write, the size by its
/// magnitude, the port by its name
fn render_list(
    browser: &SessionBrowser,
    area: Rect,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) {
    let colors = &theme.session_browser;
    let now = Local::now();
    let height = area.height as usize;
    let first = (browser.selected + 1).saturating_sub(height);
    let rows = browser.sessions.iter().enumerate().skip(first).take(height);
    let lines: Vec<Line> = rows
        .map(|(idx, session)| {
            let (mark, mark_color) = if session.running {
                ('●', colors.mark_running)
            } else if browser.restorable.as_deref() == Some(session.stem.as_str()) {
                ('↺', colors.mark_restorable)
            } else if session.meta.is_none() {
                ('?', colors.mark_no_meta)
            } else {
                (' ', theme.palette.text)
            };
            let started = session.started.map_or_else(
                || "????-??-?? ??:??".to_string(),
                |time| time.format("%Y-%m-%d %H:%M").to_string(),
            );
            let date_color = date_color(session, now, theme);
            let mut style = Style::default();
            if idx == browser.selected {
                style = style.add_modifier(Modifier::REVERSED);
            }
            // only the text is colored: the gaps and the size's padding keep the text color,
            // so a reversed column is as wide as its text
            let cell = |text: String, color| Span::styled(text, style.fg(color));
            let gap = |width: usize| cell(" ".repeat(width), theme.palette.text);
            let size_text = size(session.size);
            let row_area = Rect::new(area.x, area.y + (idx - first) as u16, area.width, 1);
            targets.push((row_area, PopupTarget::SessionRow(idx)));
            Line::from(vec![
                cell(mark.to_string(), mark_color),
                gap(1),
                cell(started, date_color),
                gap(1 + 5usize.saturating_sub(size_text.chars().count())),
                cell(size_text, colors.size_color(session.size)),
                gap(2),
                cell(
                    session.port_tag().to_string(),
                    colors.port_color(session.port_tag()),
                ),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
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

/// The selected session's details from its meta file, then its last text lines that fit
fn render_preview(browser: &SessionBrowser, area: Rect, frame: &mut Frame, theme: &ThemeConfig) {
    let Some(session) = browser.selected_session() else {
        return;
    };
    let muted = Style::default().fg(theme.palette.text_muted);
    let field = |name: &'static str, value: String, style: Style| {
        Line::from(vec![
            Span::styled(format!("{name:<8}"), muted),
            Span::styled(value, style),
        ])
    };
    let mut lines = details(browser, session, theme, field, muted);
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
            let port = Style::default().fg(colors.port_color(session.port_tag()));
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
    } else if browser.restorable.as_deref() == Some(session.stem.as_str()) {
        "what this device would restore"
    } else {
        "past"
    };
    lines.push(field("State", state.to_string(), plain));
    lines
}
