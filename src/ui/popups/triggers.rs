use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Padding, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::{App, triggers::Trigger},
    configs::{KeyPattern, ThemeConfig, keybindings::KeyBindingDialogAction},
    overlays::{PopupTarget, TriggerBrowser},
    paths::shorten_home,
    ui::{
        common::popup_area,
        popups::{Hints, cut, hint_spans, popup_block, section},
    },
};

/// Columns between the list and the config
const GAP: u16 = 2;
/// What an empty file shows in place of the list
const EMPTY: &str = "No trigger: add a table per rule to the file, then reload the config";
/// Over the list
const LIST_TITLE: &str = "Rules";
/// Before the path, as wide as the config's field names
const FILE_LABEL: &str = "File   ";

/// The trigger browser (MAX-167): the file on top, the triggers on the left with their state,
/// the selected one's config on the right under its name, the buttons in the bottom border.
/// As big as its text with every trigger's config in it, so it doesn't change size with the
/// selection; the screen's size at most. The list and the config centered under the file
/// when it's wider (MAX-183)
pub fn render_triggers_popup(
    app: &App,
    browser: &TriggerBrowser,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let resolver = &app.key_resolver;
    let dialog_key = |action| resolver.dialog_keys(action).into_iter().next();
    // the browser's own keys, as typed
    let key = |c| {
        Some(KeyPattern {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::NONE,
        })
    };
    let buttons = [
        (
            dialog_key(KeyBindingDialogAction::Confirm),
            PopupTarget::TriggerToggle,
            "Toggle",
        ),
        (key('a'), PopupTarget::TriggerToggleAll, "Toggle all"),
        (key('c'), PopupTarget::TriggerCopyPath, "Copy path"),
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

    let triggers = app.triggers();
    let file = shorten_home(&browser.path);
    let list_width = (triggers.iter())
        .map(list_width)
        .max()
        .unwrap_or(0)
        .max(LIST_TITLE.width() as u16);
    let config_width = (triggers.iter())
        .flat_map(|trigger| config(trigger, theme))
        .map(|(name, value, _)| name.len() + value.width())
        .chain(triggers.iter().map(|trigger| trigger.name.width()))
        .max()
        .unwrap_or(0) as u16;
    let body = if triggers.is_empty() {
        EMPTY.width() as u16
    } else {
        list_width + GAP + config_width
    };
    // the buttons in the bottom border take as much as the text: a corner and a space on
    // each side, as the border and the padding
    let width = ((FILE_LABEL.len() + file.width()) as u16)
        .max(body)
        .max(hints.line.width() as u16);
    // under their titles
    let rows = triggers.len().max(FIELDS) as u16 + 1;
    // the borders and the padding around, none on top (MAX-183), the file and a blank row on
    // top
    let parent = frame.area();
    let area = popup_area(parent, width + 4, rows + 2 + 3);

    let block = popup_block("Triggers", &hints, theme).padding(Padding::new(1, 1, 0, 1));
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
    let file = Line::from(vec![
        Span::styled(FILE_LABEL, muted),
        Span::styled(file, Style::default().fg(theme.palette.main)),
    ]);
    frame.render_widget(Paragraph::new(file), Rect { height: 1, ..inner });
    inner.y += 2;
    inner.height = inner.height.saturating_sub(2);
    // a body narrower than the popup centered in it
    let offset = inner.width.saturating_sub(body) / 2;
    inner.x += offset;
    inner.width -= offset;

    if triggers.is_empty() {
        frame.render_widget(Paragraph::new(Span::styled(EMPTY, muted)), inner);
        return area;
    }

    // squeezed by the screen, the list keeps half at most
    let list_width = list_width.min(inner.width / 2);
    let list = Rect::new(inner.x, inner.y, list_width, inner.height);
    let config = Rect::new(
        inner.x + list_width + GAP,
        inner.y,
        inner.width.saturating_sub(list_width + GAP),
        inner.height,
    );
    render_list(app, browser, list, frame, theme, targets);
    render_config(app, browser, config, frame, theme);
    area
}

/// A row of the list: the mark, a space and the name, `NAME_WIDTH` at most
fn list_width(trigger: &Trigger) -> u16 {
    (2 + trigger.name.width().min(NAME_WIDTH)) as u16
}

/// A name in the list at most: a longer one is cut, in full over the selected one's config
const NAME_WIDTH: usize = 20;

/// The rows of the config
const FIELDS: usize = 4;

/// A trigger's config: each field's name padded, its value and style. Armed in the status
/// bar's color, the text matched and the line run in the settings' colors
fn config(trigger: &Trigger, theme: &ThemeConfig) -> [(&'static str, String, Style); FIELDS] {
    let muted = Style::default().fg(theme.palette.text_muted);
    let plain = Style::default().fg(theme.palette.text);
    let (state, state_style) = if trigger.is_armed() {
        let warn = Style::default().fg(theme.palette.text_warn);
        ("armed", warn.add_modifier(Modifier::BOLD))
    } else {
        ("disarmed", muted)
    };
    let fires = if trigger.rearm {
        "each time, it stays armed"
    } else {
        "once, then disarms"
    };
    [
        ("State  ", state.to_string(), state_style),
        (
            "Match  ",
            format!("{:?}", trigger.text),
            Style::default().fg(theme.palette.secondary),
        ),
        (
            "Run    ",
            trigger.run.clone(),
            Style::default().fg(theme.palette.main),
        ),
        ("Fires  ", fires.to_string(), plain),
    ]
}

/// The list's title, then one row per trigger from the one that keeps the selection in view:
/// `◎` armed, `○` not, a long name cut, the selected one's bar as wide as the list. Each row
/// is a click target
fn render_list(
    app: &App,
    browser: &TriggerBrowser,
    area: Rect,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) {
    frame.render_widget(Paragraph::new(section(LIST_TITLE, theme)), area);
    let area = Rect {
        y: area.y + 1,
        height: area.height.saturating_sub(1),
        ..area
    };
    let height = area.height as usize;
    let first = (browser.selected + 1).saturating_sub(height);
    let rows = app.triggers().iter().enumerate().skip(first).take(height);
    let lines: Vec<Line> = rows
        .map(|(idx, trigger)| {
            let mut style = Style::default().fg(theme.palette.text);
            if idx == browser.selected {
                style = style.add_modifier(Modifier::REVERSED);
            }
            let (mark, color) = if trigger.is_armed() {
                ("◎", theme.palette.text_warn)
            } else {
                ("○", theme.palette.text_muted)
            };
            let row_area = Rect::new(area.x, area.y + (idx - first) as u16, area.width, 1);
            targets.push((row_area, PopupTarget::TriggerRow(idx)));
            // cut to the list's width, narrower than `NAME_WIDTH` on a narrow screen
            let name = cut(&trigger.name, (area.width as usize).saturating_sub(2));
            let pad = (area.width as usize).saturating_sub(2 + name.width());
            Line::from(vec![
                Span::styled(mark, style.fg(color)),
                Span::styled(" ", style),
                Span::styled(name, style),
                Span::styled(" ".repeat(pad), style),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

/// The selected trigger's name, its rule as written and its state
fn render_config(
    app: &App,
    browser: &TriggerBrowser,
    area: Rect,
    frame: &mut Frame,
    theme: &ThemeConfig,
) {
    let Some(trigger) = app.triggers().get(browser.selected) else {
        return;
    };
    let muted = Style::default().fg(theme.palette.text_muted);
    let mut lines = vec![section(trigger.name.as_str(), theme)];
    lines.extend(
        config(trigger, theme)
            .into_iter()
            .map(|(name, value, style)| {
                Line::from(vec![Span::styled(name, muted), Span::styled(value, style)])
            }),
    );
    frame.render_widget(Paragraph::new(lines), area);
}
