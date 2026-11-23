use std::{collections::HashMap, ops::Range};

use crossterm::event::{KeyCode, KeyModifiers};
use tui_input::Input;
use unicode_width::UnicodeWidthStr;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Clear, Padding, Paragraph, Tabs},
};

use crate::{
    app::App,
    commands::COMMANDS,
    configs::{
        KeyBindingAction, KeyPattern, ThemeConfig,
        keybindings::{BoundCommand, KeyBindingDialogAction},
    },
    overlays::{HelpCategory, HelpDialog, PopupTarget},
    ui::{
        common::popup_area,
        popups::{Hints, cut, filepath::field_view, hint_spans, popup_block, render_scrollbar},
    },
};

/// The help popup's width at least, else four fifths of the screen's: room around it stays
const HELP_WIDTH: u16 = 74;

/// The help popup's width at most: room for a key, its action and the action's help (MAX-239)
const HELP_MAX_WIDTH: u16 = 100;

pub fn render_help_popup(
    app: &App,
    dialog: &HelpDialog,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let width = (u32::from(frame.area().width) * 4 / 5) as u16;
    let width = width.min(HELP_MAX_WIDTH);
    let area = popup_area(frame.area(), width.max(HELP_WIDTH), 20);
    frame.render_widget(Clear, area);

    let is_compact_vertical = area.height < 14;

    let (hints, spots) = if is_compact_vertical {
        Default::default()
    } else {
        footer(app, dialog.typing, theme)
    };
    // the tabs row starts the content, no top padding needed
    let outer_block = popup_block("Help", &hints, theme).padding(Padding::horizontal(1));
    // after the corner and the space `popup_block` puts before them
    let (x, y) = (area.x + 2, area.bottom().saturating_sub(1));
    for (start, width, target) in spots {
        let spot = Rect::new(x.saturating_add(start), y, width, 1).intersection(area);
        if !spot.is_empty() {
            targets.push((spot, target));
        }
    }

    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let tabs_height = if is_compact_vertical { 1 } else { 2 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(tabs_height), Constraint::Min(0)])
        .split(inner_area);

    let tabs_area = chunks[0];
    let content_area = chunks[1];

    render_tabs(dialog, frame, tabs_area, theme, targets);

    // the text stays clear of the scrollbar's column, whether it shows or not
    let room = usize::from(content_area.width.saturating_sub(1));
    let content_area = match &dialog.filter {
        Some(filter) => render_filter(filter, dialog.typing, content_area, frame, theme),
        None => content_area,
    };
    let mut lines = match dialog.query() {
        Some(query) => filtered_lines(app, query, room, theme),
        None => tab_lines(app, dialog.category, room, theme),
    };
    lines.push(Line::from(""));

    let total_lines = lines.len();
    let visible_height = content_area.height as usize;
    let max_scroll = total_lines.saturating_sub(visible_height);

    dialog.max_scroll.set(max_scroll);
    let current_scroll = dialog.scroll.min(max_scroll);

    let scrolls = total_lines > visible_height;
    // the text stays clear of the scrollbar's column
    let text_area = Rect {
        width: content_area.width.saturating_sub(u16::from(scrolls)),
        ..content_area
    };
    let paragraph = Paragraph::new(lines)
        .style(*theme.popup.background)
        .scroll((current_scroll as u16, 0));
    frame.render_widget(paragraph, text_area);

    if scrolls {
        let scroll = (current_scroll, max_scroll, visible_height);
        render_scrollbar(frame, content_area, scroll, theme);
    }
    area
}

/// The lines of a tab: its title, where its keys work, then its keys or commands
fn tab_lines(
    app: &App,
    category: HelpCategory,
    room: usize,
    theme: &ThemeConfig,
) -> Vec<Line<'static>> {
    let resolver = &app.key_resolver;
    let mut lines = Vec::new();
    if let Some(title) = tab_title(app, category) {
        lines.push(Line::from(Span::styled(
            cut(&format!(" {title}"), room),
            title_style(theme),
        )));
        lines.push(Line::from(""));
    }

    match category {
        HelpCategory::Commands => {
            let header_style = Style::default()
                .fg(theme.palette.main)
                .add_modifier(Modifier::BOLD);
            let key_style = Style::default().fg(theme.palette.secondary);
            let help_style = Style::default().fg(theme.palette.text_muted);
            // a line longer than the popup ends with `…`, after `used` columns of it, a column
            // kept free before the scrollbar's
            let fit = |text: &str, used: usize| cut(text, room.saturating_sub(used + 1));
            // a header, then a blank line as under the other tabs' title (MAX-274)
            let header = |lines: &mut Vec<Line<'static>>, text: &str| {
                lines.push(Line::from(Span::styled(fit(text, 0), header_style)));
                lines.push(Line::from(""));
            };

            // the named commands of `[commands]` (MAX-239): the name and the chain it runs,
            // its help under them, as the commands with arguments
            if !resolver.commands.is_empty() {
                header(&mut lines, " Named commands, [commands] in keymap.toml:");
                let name_width = (resolver.commands.iter())
                    .map(|command| command.name.width())
                    .max()
                    .unwrap_or(0);
                for command in &resolver.commands {
                    let name = fit(&format!(" {:<name_width$}  ", command.name), 0);
                    let run = fit(&command.run, name.width());
                    lines.push(Line::from(vec![
                        Span::styled(name, key_style),
                        Span::raw(run),
                    ]));
                    if !command.help.is_empty() {
                        lines.push(Line::from(Span::styled(
                            fit(&format!("     {}", command.help), 0),
                            help_style,
                        )));
                    }
                }
                lines.push(Line::from(""));
            }

            // every command, those with arguments first, their help under them
            let keys = resolver.action_keys(KeyBindingAction::CommandLine);
            let title = match keys.first() {
                Some(key) => format!(" Commands, on the command line ({key}), `;` between them:"),
                None => " Commands, `;` between them:".to_string(),
            };
            header(&mut lines, &title);
            let (with_args, actions): (Vec<_>, Vec<_>) =
                COMMANDS.iter().partition(|spec| !spec.args.is_empty());
            for spec in with_args {
                lines.push(Line::from(Span::styled(
                    fit(&format!(" {}", spec.usage()), 0),
                    key_style,
                )));
                lines.push(Line::from(Span::styled(
                    fit(&format!("     {}", spec.help), 0),
                    help_style,
                )));
            }

            lines.push(Line::from(""));
            let name_width = actions
                .iter()
                .map(|spec| spec.name.width())
                .max()
                .unwrap_or(0);
            for spec in actions {
                let name = fit(&format!(" {:<name_width$}  ", spec.name), 0);
                let help = fit(spec.help, name.width());
                lines.push(Line::from(vec![
                    Span::styled(name, key_style),
                    Span::styled(help, help_style),
                ]));
            }
        }
        HelpCategory::CopyMode => {
            let (moves, scrolls) = copy_sections(app);
            lines.extend(key_lines(&moves, room, theme));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                " Scrolling moves the view, the cursor stays:",
                Style::default()
                    .fg(theme.palette.main)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.extend(key_lines(&scrolls, room, theme));
        }
        _ => lines.extend(key_lines(&tab_rows(app, category), room, theme)),
    }

    lines
}

/// A tab's keys, or its commands, as rows: a command's key is its usage, its name left out;
/// what the filter looks through (MAX-274)
fn tab_rows(app: &App, category: HelpCategory) -> Vec<KeyRow> {
    let resolver = &app.key_resolver;
    match category {
        HelpCategory::Prefix => with_named(
            &resolver.prefix_actions,
            |a| a.help(),
            &resolver.prefix_commands,
        ),
        HelpCategory::Direct => with_named(
            &resolver.direct_actions,
            |a| a.help(),
            &resolver.direct_commands,
        ),
        HelpCategory::Commands => {
            let command = |key: String, help: &str| KeyRow {
                key,
                name: String::new(),
                help: help.to_string(),
                order: Default::default(),
            };
            let named = (resolver.commands.iter()).map(|named| {
                let help = if named.help.is_empty() {
                    &named.run
                } else {
                    &named.help
                };
                command(named.name.clone(), help)
            });
            let specs = COMMANDS.iter().map(|spec| command(spec.usage(), spec.help));
            named.chain(specs).collect()
        }
        HelpCategory::CopyMode => {
            let (moves, scrolls) = copy_sections(app);
            moves.into_iter().chain(scrolls).collect()
        }
        HelpCategory::Search => key_rows(&resolver.search_actions, |a| a.help().to_string()),
        HelpCategory::LineMode => with_named(
            &resolver.line_actions,
            |a| a.help(),
            &resolver.line_commands,
        ),
        HelpCategory::TerminalMode => with_named(
            &resolver.terminal_actions,
            |a| a.help(),
            &resolver.terminal_commands,
        ),
        HelpCategory::Dialog => key_rows(&resolver.dialog_actions, |a| a.help().to_string()),
    }
}

/// The rows of every tab matching `query` (MAX-274), its key, its name or its help holding it,
/// case aside: under each tab's name and where its keys work, in the tabs' order
fn filtered_lines(app: &App, query: &str, room: usize, theme: &ThemeConfig) -> Vec<Line<'static>> {
    let query = query.to_lowercase();
    let holds = |row: &KeyRow| {
        [&row.key, &row.name, &row.help]
            .iter()
            .any(|text| text.to_lowercase().contains(&query))
    };
    let muted = Style::default().fg(theme.palette.text_muted);
    let mut lines = Vec::new();
    for category in HelpCategory::ALL {
        let rows: Vec<KeyRow> = tab_rows(app, category).into_iter().filter(holds).collect();
        if rows.is_empty() {
            continue;
        }
        let mut header = vec![Span::styled(
            format!(" {}", category.title()),
            title_style(theme),
        )];
        if let Some(title) = tab_title(app, category) {
            header.push(Span::styled(format!(" · {title}"), muted));
        }
        lines.push(Line::from(header));
        lines.push(Line::from(""));
        lines.extend(key_lines(&rows, room, theme));
        lines.push(Line::from(""));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(" Nothing matches", muted)));
    }
    lines
}

/// Each tab of keys says first where they work, and the key that gets there (MAX-274); the
/// commands tab has headers of its own
fn tab_title(app: &App, category: HelpCategory) -> Option<String> {
    let resolver = &app.key_resolver;
    // a key ending with a letter first, easier to type than `[` on some layouts
    let reach = |action: KeyBindingAction| {
        let keys = resolver.action_keys(action);
        let letter = |key: &&String| key.chars().last().is_some_and(|c| c.is_ascii_alphabetic());
        match keys.iter().find(letter).or(keys.first()) {
            Some(key) => format!(" ({key})"),
            None => String::new(),
        }
    };
    Some(match category {
        // the prefix said once, each row its key alone
        HelpCategory::Prefix => format!("{}, then:", resolver.prefix),
        HelpCategory::Direct => "Without the prefix, in both modes, never sent:".to_string(),
        HelpCategory::Commands => return None,
        HelpCategory::CopyMode => {
            format!("In copy mode{}:", reach(KeyBindingAction::EnterCopyMode))
        }
        HelpCategory::Search => {
            format!("In the search bar{}:", reach(KeyBindingAction::EnterSearch))
        }
        HelpCategory::LineMode => format!(
            "In line mode{}, while typing:",
            reach(KeyBindingAction::SKInputMode)
        ),
        HelpCategory::TerminalMode => format!(
            "In terminal mode{}, before the key is sent:",
            reach(KeyBindingAction::SKInputMode)
        ),
        HelpCategory::Dialog => "In the popups:".to_string(),
    })
}

fn title_style(theme: &ThemeConfig) -> Style {
    Style::default()
        .fg(theme.palette.main)
        .add_modifier(Modifier::BOLD)
}

/// Copy mode's keys in two sections: the moves and the keys running a named command, then the
/// scrolls, which move the view and not the cursor
fn copy_sections(app: &App) -> (Vec<KeyRow>, Vec<KeyRow>) {
    let resolver = &app.key_resolver;
    let (scrolls, moves): (Vec<_>, Vec<_>) = resolver
        .copy_actions
        .iter()
        .partition(|(_, action)| action.is_scroll());
    let moves = key_rows(moves, |action| action.help().to_string());
    // the copy keys running a named command, and the direct ones no copy key takes
    // (MAX-239)
    let taken = |key: &KeyPattern| {
        resolver.copy_actions.contains_key(key) || resolver.copy_commands.contains_key(key)
    };
    let direct_named = (resolver.direct_commands.iter()).filter(|(key, _)| !taken(key));
    let mut bound = key_rows(&resolver.copy_commands, |command| command.help.clone());
    bound.extend(key_rows(direct_named, |command| command.help.clone()));
    let scrolls = key_rows(scrolls, |action| action.help().to_string());
    // the direct scroll and pan keys a copy key doesn't take reach copy mode (MAX-143)
    let direct = key_rows(
        resolver
            .direct_actions
            .iter()
            .filter(|(k, action)| action.is_navigation() && !taken(k)),
        |action| action.help().to_string(),
    );

    let moves = moves.into_iter().chain(bound).collect();
    let scrolls = scrolls.into_iter().chain(direct).collect();
    (moves, scrolls)
}

/// One key of a tab: the key, the name of what it runs, and that one's help (MAX-239)
#[derive(Clone)]
struct KeyRow {
    key: String,
    name: String,
    help: String,
    order: (u8, u8, u32, bool),
}

impl KeyRow {
    /// ` key  help    name`, the key padded to `key_width`, the help in `room` columns and the
    /// name dimmed at their right end (MAX-274): what the key does is read first, the name is
    /// for keymap.toml. When the name would leave the help less than `MIN_HELP` columns, it's
    /// left out and the help is cut with `…`
    fn line(&self, key_width: usize, room: usize, theme: &ThemeConfig) -> Line<'static> {
        let key = Span::styled(
            format!(" {:<key_width$}  ", self.key),
            Style::default().fg(theme.palette.secondary),
        );
        let name_style = Style::default().fg(theme.palette.text_muted);
        // a column kept free before the scrollbar's
        let left = room.saturating_sub(key.width() + 1);
        if self.help.is_empty() {
            return Line::from(vec![key, Span::styled(cut(&self.name, left), name_style)]);
        }
        let help_style = Style::default().fg(theme.palette.text);
        let help_room = left.saturating_sub(self.name.width() + 2);
        if help_room < MIN_HELP.min(self.help.width()) {
            return Line::from(vec![key, Span::styled(cut(&self.help, left), help_style)]);
        }
        let help = cut(&self.help, help_room);
        let pad = left.saturating_sub(help.width() + self.name.width());
        Line::from(vec![
            key,
            Span::styled(help, help_style),
            Span::raw(" ".repeat(pad)),
            Span::styled(self.name.clone(), name_style),
        ])
    }
}

/// The help's columns at least, beside the name: below, the name is left out (MAX-274)
const MIN_HELP: usize = 20;

/// The rows of `bindings` in the help's key order, the help of each from `help`
fn key_rows<'a, A: std::fmt::Display + 'a>(
    bindings: impl IntoIterator<Item = (&'a KeyPattern, &'a A)>,
    help: impl Fn(&A) -> String,
) -> Vec<KeyRow> {
    let mut rows: Vec<_> = (bindings.into_iter())
        .map(|(key, action)| KeyRow {
            key: key.to_string(),
            name: action.to_string(),
            help: help(action),
            order: key.help_order(),
        })
        .collect();
    rows.sort_by_key(|row| row.order);
    rows
}

/// The rows of a section's actions and of its keys running a named command (MAX-239), in
/// the help's key order
fn with_named<A: std::fmt::Display>(
    actions: &HashMap<KeyPattern, A>,
    help: impl Fn(&A) -> &'static str,
    commands: &HashMap<KeyPattern, BoundCommand>,
) -> Vec<KeyRow> {
    let mut rows = key_rows(actions, |action| help(action).to_string());
    rows.extend(key_rows(commands, |command| command.help.clone()));
    rows.sort_by_key(|row| row.order);
    rows
}

/// The lines of `rows`: the one-character keys first, then a blank line, a dimmed title and
/// the longer ones (`Shift-PageDown`), each block its key column as wide as its longest key,
/// so a long key scrolled out of view doesn't push the letters' help away (MAX-274). Each
/// block keeps the rows' order
fn key_lines(rows: &[KeyRow], room: usize, theme: &ThemeConfig) -> Vec<Line<'static>> {
    let (short, long): (Vec<&KeyRow>, Vec<&KeyRow>) =
        rows.iter().partition(|row| row.key.width() == 1);
    let block = |rows: &[&KeyRow]| -> Vec<Line<'static>> {
        let width = rows.iter().map(|row| row.key.width()).max().unwrap_or(0);
        rows.iter()
            .map(|row| row.line(width, room, theme))
            .collect()
    };
    let mut lines = block(&short);
    if !short.is_empty() && !long.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Named keys and combinations:",
            Style::default().fg(theme.palette.text_muted),
        )));
    }
    lines.extend(block(&long));
    lines
}

/// The filter's row at the bottom of `area`, ` / text`, the cursor in it while it takes the keys
/// (MAX-274); the area left above it, a blank row between
fn render_filter(
    filter: &Input,
    typing: bool,
    area: Rect,
    frame: &mut Frame,
    theme: &ThemeConfig,
) -> Rect {
    let at = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1);
    let label = " / ";
    let room = usize::from(at.width).saturating_sub(label.len() + 1);
    let (shown, cursor) = field_view(filter, room);
    let text = if typing {
        theme.palette.text
    } else {
        theme.palette.text_muted
    };
    let line = Line::from(vec![
        Span::styled(label, Style::default().fg(theme.palette.secondary)),
        Span::styled(shown, Style::default().fg(text)),
    ]);
    frame.render_widget(Paragraph::new(line).style(*theme.popup.background), at);
    if typing {
        frame.set_cursor_position((at.x + label.len() as u16 + cursor, at.y));
    }
    Rect {
        height: area.height.saturating_sub(2),
        ..area
    }
}

/// The keys in the bottom border, and where each is for the mouse: the dialog's, `/` typing a
/// filter (MAX-274) and `c` copying the config folder (MAX-163); while the filter takes the
/// keys, the two that leave it
fn footer<'a>(
    app: &App,
    typing: bool,
    theme: &ThemeConfig,
) -> (Hints<'a>, Vec<(u16, u16, PopupTarget)>) {
    let keys = |actions: &[KeyBindingDialogAction]| -> Vec<(KeyPattern, PopupTarget)> {
        (actions.iter())
            .filter_map(|&action| {
                let key = app.key_resolver.dialog_keys(action).into_iter().next()?;
                Some((key, PopupTarget::Hint(action)))
            })
            .collect()
    };
    let key = |c| KeyPattern {
        code: KeyCode::Char(c),
        modifiers: KeyModifiers::NONE,
    };
    let (copy, slash) = (key('c'), key('/'));
    let buttons = if typing {
        vec![
            (keys(&[KeyBindingDialogAction::Confirm]), "Keep"),
            (keys(&[KeyBindingDialogAction::Close]), "Clear"),
        ]
    } else {
        vec![
            (
                keys(&[
                    KeyBindingDialogAction::NavLeft,
                    KeyBindingDialogAction::NavRight,
                ]),
                "Tab",
            ),
            (
                keys(&[
                    KeyBindingDialogAction::NavUp,
                    KeyBindingDialogAction::NavDown,
                ]),
                "Move",
            ),
            (vec![(slash, PopupTarget::HelpFilter)], "Filter"),
            (vec![(copy, PopupTarget::ConfigPath(0))], "Config folder"),
            (keys(&[KeyBindingDialogAction::Close]), "Close"),
        ]
    };
    let (line, spots) = hint_spans(buttons, theme);
    let hints = Hints {
        line,
        spots: Vec::new(),
    };
    (hints, spots)
}

/// The tab bar. When the tabs don't all fit, a column on each edge takes a `◀` or `▶` where
/// tabs are hidden, a click on it picks the hidden tab next to it (MAX-12)
fn render_tabs(
    dialog: &HelpDialog,
    frame: &mut Frame,
    tabs_area: Rect,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) {
    let tabs = HelpCategory::ALL;
    let widths = tab_widths();
    let selected = tabs.iter().position(|&c| c == dialog.category).unwrap_or(0);
    let overflows = tabs_span(&widths) > tabs_area.width;
    let (shown, row) = if overflows {
        let row = tabs_area.inner(Margin::new(1, 0));
        let shown = tab_window(&widths, selected, dialog.first_tab.get(), row.width);
        (shown, row)
    } else {
        (0..tabs.len(), tabs_area)
    };
    dialog.first_tab.set(shown.start);

    let titles: Vec<Line> = tabs[shown.clone()]
        .iter()
        .map(|cat| {
            let style = if *cat == dialog.category {
                Style::default()
                    .fg(theme.palette.secondary)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.palette.text)
            };
            Line::from(cat.title()).style(style)
        })
        .collect();
    let bar = Tabs::new(titles)
        .style(*theme.popup.background)
        .select(selected - shown.start)
        .divider(symbols::line::VERTICAL)
        .highlight_style(
            Style::default()
                .fg(theme.palette.secondary)
                .add_modifier(Modifier::UNDERLINED),
        );
    frame.render_widget(bar, row);

    let mut x = row.x;
    for i in shown.clone() {
        let tab = Rect::new(x, row.y, widths[i], 1).intersection(row);
        targets.push((tab, PopupTarget::HelpTab(tabs[i])));
        x = x.saturating_add(widths[i] + 1);
    }

    let arrow_style = Style::default().fg(theme.palette.secondary);
    let left = Rect::new(tabs_area.x, row.y, 1, 1);
    let right = Rect::new(tabs_area.right().saturating_sub(1), row.y, 1, 1);
    if shown.start > 0 {
        frame.render_widget(Paragraph::new("◀").style(arrow_style), left);
        targets.push((left, PopupTarget::HelpTab(tabs[shown.start - 1])));
    }
    if shown.end < tabs.len() {
        frame.render_widget(Paragraph::new("▶").style(arrow_style), right);
        targets.push((right, PopupTarget::HelpTab(tabs[shown.end])));
    }
}

/// Each tab's columns as `Tabs` draws it: a space, the title, a space; the divider after it
/// is apart (MAX-113)
fn tab_widths() -> Vec<u16> {
    HelpCategory::ALL
        .iter()
        .map(|category| category.title().width() as u16 + 2)
        .collect()
}

/// The columns tabs of these widths take, a divider between each two
fn tabs_span(widths: &[u16]) -> u16 {
    widths.iter().sum::<u16>() + widths.len().saturating_sub(1) as u16
}

/// The tabs shown in `room` columns: from `first` (shown before), moved just enough to show the
/// selected one, and back as far as the room allows, so none is left after the last tab
fn tab_window(widths: &[u16], selected: usize, first: usize, room: u16) -> Range<usize> {
    let fits = |range: Range<usize>| tabs_span(&widths[range]) <= room;
    let mut first = first.min(selected);
    while first < selected && !fits(first..selected + 1) {
        first += 1;
    }
    while first > 0 && fits(first - 1..widths.len()) {
        first -= 1;
    }
    let mut end = selected + 1;
    while end < widths.len() && fits(first..end + 1) {
        end += 1;
    }
    first..end
}

#[cfg(test)]
#[path = "../../tests/ui/popups/help.rs"]
mod tests;
