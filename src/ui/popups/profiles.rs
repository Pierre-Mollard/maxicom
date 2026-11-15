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
    app::App,
    configs::{KeyPattern, ThemeConfig, keybindings::KeyBindingDialogAction, profiles::Listed},
    overlays::{PopupTarget, ProfileBrowser},
    paths::shorten_home,
    ui::{
        common::popup_area,
        popups::{Hints, cut, hint_spans, popup_block},
    },
};

/// What an empty folder shows in place of the list
const EMPTY: &str = "No profile: add a .toml file to the folder";
/// Before the path
const FOLDER_LABEL: &str = "Folder  ";
/// A name in the list at most: a longer one is cut
const NAME_WIDTH: usize = 20;
/// Columns between the name and the description
const GAP: usize = 2;
/// The stack's place, the mark and their spaces before the name: `12 ◉ `
const MARK_WIDTH: usize = 5;
/// Under the list, what the stack's places mean
const STACK_NOTE: &str = "Laid in order: the higher number wins a clash";

/// The profiles popup (MAX-240): the folder on top, a row per profile with its place in the
/// stack when it's laid, its mark and its description, the buttons in the bottom border. An
/// off profile and one that doesn't read are muted, saying so in place of the description.
/// As big as its text, the screen's size at most
pub fn render_profiles_popup(
    app: &App,
    browser: &ProfileBrowser,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let resolver = &app.key_resolver;
    let dialog_key = |action| resolver.dialog_keys(action).into_iter().next();
    // the popup's own keys, as typed
    let key = |c| {
        Some(KeyPattern {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::NONE,
        })
    };
    let buttons = [
        (
            dialog_key(KeyBindingDialogAction::Confirm),
            PopupTarget::ProfileToggle,
            "Lay / take off",
        ),
        (key('K'), PopupTarget::ProfileMove(true), "Move up"),
        (key('J'), PopupTarget::ProfileMove(false), "Move down"),
        (key('c'), PopupTarget::ProfileCopyPath, "Copy path"),
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

    let folder = shorten_home(&browser.path);
    let name_width = (browser.rows.iter())
        .map(|row| row.name.width().min(NAME_WIDTH))
        .max()
        .unwrap_or(0);
    let body = (browser.rows.iter())
        .map(|row| MARK_WIDTH + name_width + GAP + about(row).width())
        .max()
        .unwrap_or(EMPTY.width())
        .max(STACK_NOTE.width());
    let width =
        ((FOLDER_LABEL.len() + folder.width()).max(body) as u16).max(hints.line.width() as u16);
    // the folder and a blank row on top, a blank row and the note under the rows, in the
    // borders and the bottom padding
    let rows = browser.rows.len().max(1) as u16 + 4;
    let area = popup_area(frame.area(), width + 4, rows + 3);

    let block = popup_block("Profiles", &hints, theme).padding(Padding::new(1, 1, 0, 1));
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
    let folder = Line::from(vec![
        Span::styled(FOLDER_LABEL, muted),
        Span::styled(folder, Style::default().fg(theme.palette.main)),
    ]);
    frame.render_widget(Paragraph::new(folder), Rect { height: 1, ..inner });
    inner.y += 2;
    inner.height = inner.height.saturating_sub(2);
    if browser.rows.is_empty() {
        frame.render_widget(Paragraph::new(Span::styled(EMPTY, muted)), inner);
        return area;
    }

    // the rows, from the one that keeps the selection in view, then the note
    let height = inner.height.saturating_sub(2) as usize;
    let first = (browser.selected + 1).saturating_sub(height);
    let laid = app.profiles();
    let mut lines: Vec<Line> = Vec::new();
    for (idx, row) in browser.rows.iter().enumerate().skip(first).take(height) {
        let usable = row.enabled && row.error.is_none();
        let place = laid.iter().position(|name| *name == row.name);
        let mut style = Style::default().fg(if usable {
            theme.palette.text
        } else {
            theme.palette.text_muted
        });
        if idx == browser.selected {
            style = style.add_modifier(Modifier::REVERSED);
        }
        let (number, mark, color) = match place {
            Some(place) => (format!("{:>2}", place + 1), "◉", theme.palette.text_warn),
            None => ("  ".to_string(), "○", theme.palette.text_muted),
        };
        let name = cut(&row.name, NAME_WIDTH);
        let name_pad = name_width.saturating_sub(name.width()) + GAP;
        let about = cut(
            &about(row),
            (inner.width as usize).saturating_sub(MARK_WIDTH + name_width + GAP),
        );
        let used = MARK_WIDTH + name_width + GAP + about.width();
        let row_area = Rect::new(inner.x, inner.y + (idx - first) as u16, inner.width, 1);
        targets.push((row_area, PopupTarget::ProfileRow(idx)));
        let about_style = if usable {
            style.fg(theme.palette.text_muted)
        } else {
            style
        };
        lines.push(Line::from(vec![
            Span::styled(number, style.fg(color)),
            Span::styled(" ", style),
            Span::styled(mark, style.fg(color)),
            Span::styled(" ", style),
            Span::styled(name, style),
            Span::styled(" ".repeat(name_pad), style),
            Span::styled(about, about_style),
            Span::styled(
                " ".repeat((inner.width as usize).saturating_sub(used)),
                style,
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::styled(STACK_NOTE, muted));
    frame.render_widget(Paragraph::new(lines), inner);
    area
}

/// What a row says after the name: the description, or why the profile isn't laid
fn about(row: &Listed) -> String {
    match &row.error {
        Some(err) => format!("doesn't read: {}", err.lines().next().unwrap_or_default()),
        None if !row.enabled => format!("off (enabled = false) {}", row.description),
        None => row.description.clone(),
    }
}
