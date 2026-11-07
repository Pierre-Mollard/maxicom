use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Padding, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    configs::{KeyResolver, ThemeConfig},
    overlays::{ActionMenu, MenuRun, PopupTarget},
    ui::popups::row_area,
};

/// The right-click menu at the cell it was opened on, moved back inside the screen when it
/// doesn't fit there (MAX-130). Each row shows its action's first key on the right, and is a
/// click target
pub fn render_action_menu(
    menu: &ActionMenu,
    resolver: &KeyResolver,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let keys: Vec<String> = menu
        .items
        .iter()
        .map(|&(_, run)| match run {
            MenuRun::Action(action) => resolver.action_keys(action).into_iter().next(),
            // the `mark` key waits for a letter: not this row's
            MenuRun::AddMark(_) => None,
        })
        .map(|key| key.map(|key| caret(&key)).unwrap_or_default())
        .collect();
    let label_width = menu.items.iter().map(|(l, _)| l.width()).max().unwrap_or(0);
    let key_width = keys.iter().map(|k| k.width()).max().unwrap_or(0);

    // pointer, label, a gap, key; then the borders and the right padding
    let width = (2 + label_width + 2 + key_width + 3) as u16;
    let height = menu.items.len() as u16 + 2;
    let screen = frame.area();
    let (width, height) = (width.min(screen.width), height.min(screen.height));
    let x = menu.at.0.min(screen.right().saturating_sub(width));
    let y = menu.at.1.min(screen.bottom().saturating_sub(height));
    let area = Rect::new(x, y, width, height);

    let selected = Style::default()
        .fg(theme.palette.secondary)
        .add_modifier(Modifier::BOLD);
    let unselected = Style::default().fg(theme.palette.unselected);
    let key_style = Style::default().fg(theme.palette.text_muted);
    let lines: Vec<Line> = menu
        .items
        .iter()
        .zip(&keys)
        .enumerate()
        .map(|(idx, (&(label, _), key))| {
            let is_selected = idx == menu.selected;
            let style = if is_selected { selected } else { unselected };
            Line::from(vec![
                Span::styled(if is_selected { "▶ " } else { "  " }, style),
                Span::styled(format!("{label:<label_width$}  "), style),
                Span::styled(format!("{key:>key_width$}"), key_style),
            ])
        })
        .collect();

    let block = Block::bordered()
        .border_style(*theme.popup.border)
        .style(*theme.popup.background)
        .padding(Padding::right(1))
        .title(Span::styled(" Menu ", *theme.popup.title));
    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    for idx in 0..lines.len() {
        targets.push((row_area(inner, idx), PopupTarget::MenuItem(idx)));
    }
    frame.render_widget(Paragraph::new(lines), inner);
    area
}

/// A key as the menu shows it, short: a plain Ctrl and letter in caret notation, `^A z` for
/// `Ctrl-a z`. Ctrl and Shift (`Ctrl-A`) has no caret form and stays as it is
fn caret(key: &str) -> String {
    let Some(rest) = key.strip_prefix("Ctrl-") else {
        return key.to_string();
    };
    let mut chars = rest.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None | Some(' ')) if c.is_ascii_lowercase() => {
            format!("^{}{}", c.to_ascii_uppercase(), &rest[1..])
        }
        _ => key.to_string(),
    }
}
