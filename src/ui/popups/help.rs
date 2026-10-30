use std::ops::Range;

use unicode_width::UnicodeWidthStr;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Clear, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Tabs},
};

use crate::{
    app::App,
    commands::COMMANDS,
    configs::{KeyBindingAction, ThemeConfig, keybindings::KeyBindingDialogAction},
    overlays::{HelpCategory, HelpDialog, PopupTarget},
    ui::{
        common::popup_area,
        popups::{key_hints, popup_block},
    },
};

/// The help popup's width at least, else four fifths of the screen's: room around it stays
const HELP_WIDTH: u16 = 74;

/// Columns the help popup takes around its tab bar at most: the borders and the padding,
/// which with the tabs' own spaces leave two blank columns on each side
const TABS_MARGIN: u16 = 4;

pub fn render_help_popup(
    app: &App,
    dialog: &HelpDialog,
    frame: &mut Frame,
    theme: &ThemeConfig,
    targets: &mut Vec<(Rect, PopupTarget)>,
) -> Rect {
    let width = (u32::from(frame.area().width) * 4 / 5) as u16;
    // no wider than the whole tab bar needs
    let width = width.min(tabs_span(&tab_widths()) + TABS_MARGIN);
    let area = popup_area(frame.area(), width.max(HELP_WIDTH), 20);
    frame.render_widget(Clear, area);

    let is_compact_vertical = area.height < 14;

    let hints = if is_compact_vertical {
        Default::default()
    } else {
        key_hints(
            &app.key_resolver,
            false,
            &[
                (
                    &[
                        KeyBindingDialogAction::NavLeft,
                        KeyBindingDialogAction::NavRight,
                    ],
                    "Category",
                ),
                (
                    &[
                        KeyBindingDialogAction::NavUp,
                        KeyBindingDialogAction::NavDown,
                    ],
                    "Move",
                ),
                (&[KeyBindingDialogAction::Close], "Close"),
            ],
            theme,
        )
    };
    // the tabs row starts the content, no top padding needed
    let outer_block = popup_block("Help", &hints, theme).padding(Padding::horizontal(1));
    hints.push_block_targets(area, targets);

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

    let prefix_str = format!("{}", app.key_resolver.prefix);
    let mut lines = Vec::new();

    match dialog.category {
        HelpCategory::Prefix => {
            lines.push(Line::from(Span::styled(
                format!(" Prefix Leader Key: <{prefix_str}>"),
                Style::default()
                    .fg(theme.palette.main)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));

            let mut sorted_keys: Vec<_> = app.key_resolver.prefix_actions.iter().collect();
            sorted_keys.sort_by_key(|(k, _)| format!("{k}"));
            // the longest key sets the column, so `Shift-Right` doesn't push its action (MAX-155)
            let key_width = sorted_keys
                .iter()
                .map(|(k, _)| format!("{k}").width())
                .max()
                .unwrap_or(0)
                .max(10);

            for (pattern, action) in sorted_keys {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {prefix_str} + {:<key_width$} ", format!("{pattern}")),
                        Style::default().fg(theme.palette.secondary),
                    ),
                    Span::raw(format!("-> {}", action)),
                ]));
            }
        }
        HelpCategory::Direct => {
            let mut sorted_keys: Vec<_> = app.key_resolver.direct_actions.iter().collect();
            sorted_keys.sort_by_key(|(k, _)| format!("{k}"));

            for (pattern, action) in sorted_keys {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {:<18} ", format!("{pattern}")),
                        Style::default().fg(theme.palette.secondary),
                    ),
                    Span::raw(format!("-> {}", action)),
                ]));
            }
        }
        HelpCategory::Commands => {
            let resolver = &app.key_resolver;
            let header_style = Style::default()
                .fg(theme.palette.main)
                .add_modifier(Modifier::BOLD);
            let key_style = Style::default().fg(theme.palette.secondary);
            let help_style = Style::default().fg(theme.palette.text_muted);

            // the keys bound to a command line (MAX-12), the direct ones first
            let mut direct: Vec<_> = resolver.direct_commands.iter().collect();
            direct.sort_by_key(|(k, _)| format!("{k}"));
            let mut prefixed: Vec<_> = resolver.prefix_commands.iter().collect();
            prefixed.sort_by_key(|(k, _)| format!("{k}"));
            let bound: Vec<_> = (direct.into_iter())
                .map(|(key, line)| (format!("{key}"), line))
                .chain(
                    prefixed
                        .into_iter()
                        .map(|(key, line)| (format!("{prefix_str} + {key}"), line)),
                )
                .collect();
            if !bound.is_empty() {
                lines.push(Line::from(Span::styled(
                    " Keys running a command line:",
                    header_style,
                )));
                let key_width = bound.iter().map(|(key, _)| key.width()).max().unwrap_or(0);
                for (key, line) in bound {
                    lines.push(Line::from(vec![
                        Span::styled(format!(" {key:<key_width$} "), key_style),
                        Span::raw(format!("-> {}", line.text)),
                    ]));
                }
                lines.push(Line::from(""));
            }

            // every command, those with arguments first, their help under them
            let keys = resolver.action_keys(KeyBindingAction::CommandLine);
            let header = match keys.first() {
                Some(key) => format!(" Commands, on the command line ({key}), `;` between them:"),
                None => " Commands, `;` between them:".to_string(),
            };
            lines.push(Line::from(Span::styled(header, header_style)));
            let (with_args, actions): (Vec<_>, Vec<_>) =
                COMMANDS.iter().partition(|spec| !spec.args.is_empty());
            for spec in with_args {
                lines.push(Line::from(Span::styled(
                    format!(" {}", spec.usage()),
                    key_style,
                )));
                lines.push(Line::from(Span::styled(
                    format!("     {}", spec.help),
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
                lines.push(Line::from(vec![
                    Span::styled(format!(" {:<name_width$} ", spec.name), key_style),
                    Span::styled(spec.help, help_style),
                ]));
            }
        }
        HelpCategory::CopyMode => {
            let resolver = &app.key_resolver;
            let mut moves: Vec<_> = resolver.copy_actions.iter().collect();
            moves.sort_by_key(|(k, _)| format!("{k}"));
            let (scrolls, moves): (Vec<_>, Vec<_>) = moves
                .into_iter()
                .partition(|(_, action)| action.is_scroll());
            // the direct scroll and pan keys a copy key doesn't take reach copy mode (MAX-143)
            let mut direct: Vec<_> = resolver
                .direct_actions
                .iter()
                .filter(|(k, action)| {
                    action.is_navigation() && !resolver.copy_actions.contains_key(k)
                })
                .collect();
            direct.sort_by_key(|(k, _)| format!("{k}"));

            let key_line = |pattern: String, action: String| {
                Line::from(vec![
                    Span::styled(
                        format!(" {pattern:<18} "),
                        Style::default().fg(theme.palette.secondary),
                    ),
                    Span::raw(format!("-> {action}")),
                ])
            };
            for (pattern, action) in moves {
                lines.push(key_line(format!("{pattern}"), format!("{action}")));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                " Scrolling moves the view, the cursor stays:",
                Style::default()
                    .fg(theme.palette.main)
                    .add_modifier(Modifier::BOLD),
            )));
            for (pattern, action) in scrolls {
                lines.push(key_line(format!("{pattern}"), format!("{action}")));
            }
            for (pattern, action) in direct {
                lines.push(key_line(format!("{pattern}"), format!("{action}")));
            }
        }
        HelpCategory::Search => {
            let mut sorted_keys: Vec<_> = app.key_resolver.search_actions.iter().collect();
            sorted_keys.sort_by_key(|(k, _)| format!("{k}"));

            for (pattern, action) in sorted_keys {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {:<18} ", format!("{pattern}")),
                        Style::default().fg(theme.palette.secondary),
                    ),
                    Span::raw(format!("-> {}", action)),
                ]));
            }
        }
        HelpCategory::LineMode => {
            let mut sorted_keys: Vec<_> = app.key_resolver.line_actions.iter().collect();
            sorted_keys.sort_by_key(|(k, _)| format!("{k}"));

            for (pattern, action) in sorted_keys {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {:<18} ", format!("{pattern}")),
                        Style::default().fg(theme.palette.secondary),
                    ),
                    Span::raw(format!("-> {}", action)),
                ]));
            }
        }
        HelpCategory::TerminalMode => {
            let mut sorted_keys: Vec<_> = app.key_resolver.terminal_actions.iter().collect();
            sorted_keys.sort_by_key(|(k, _)| format!("{k}"));

            for (pattern, action) in sorted_keys {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {:<18} ", format!("{pattern}")),
                        Style::default().fg(theme.palette.secondary),
                    ),
                    Span::raw(format!("-> {}", action)),
                ]));
            }
        }
        HelpCategory::Dialog => {
            let mut sorted_keys: Vec<_> = app.key_resolver.dialog_actions.iter().collect();
            sorted_keys.sort_by_key(|(k, _)| format!("{k}"));

            for (pattern, action) in sorted_keys {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {:<18} ", format!("{pattern}")),
                        Style::default().fg(theme.palette.secondary),
                    ),
                    Span::raw(format!("-> {}", action)),
                ]));
            }
        }
    }

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
        let mut scrollbar_state = ScrollbarState::new(max_scroll)
            .viewport_content_length(visible_height)
            .position(current_scroll);

        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .thumb_symbol("█")
            .track_symbol(Some("│"))
            .thumb_style(*theme.ui.scrollbar_thumb)
            .track_style(*theme.ui.scrollbar_track)
            .begin_style(*theme.ui.scrollbar_thumb)
            .end_style(*theme.ui.scrollbar_thumb);

        frame.render_stateful_widget(
            scrollbar,
            content_area.inner(Margin {
                vertical: 0,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
    area
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
mod tests {
    use super::*;

    /// Tabs 10 wide in 32 columns: three fit (10 + 1 + 10 + 1 + 10). The window moves only
    /// when the selected tab leaves it, by as little as it takes, and never leaves room after
    /// the last tab.
    #[test]
    fn tab_window_follows_the_selection() {
        let widths = [10; 6];
        assert_eq!(tab_window(&widths, 0, 0, 32), 0..3);
        assert_eq!(tab_window(&widths, 2, 0, 32), 0..3);
        assert_eq!(tab_window(&widths, 3, 0, 32), 1..4);
        assert_eq!(tab_window(&widths, 5, 1, 32), 3..6);
        assert_eq!(tab_window(&widths, 4, 3, 32), 3..6);
        assert_eq!(tab_window(&widths, 2, 3, 32), 2..5);
        // the cycle wraps to the first tab
        assert_eq!(tab_window(&widths, 0, 3, 32), 0..3);
        // a first tab kept from a narrower popup, and all fitting
        assert_eq!(tab_window(&widths, 5, 3, 54), 1..6);
        assert_eq!(tab_window(&widths, 5, 3, 65), 0..6);
        // a tab wider than the room is still the one shown
        assert_eq!(tab_window(&widths, 1, 0, 5), 1..2);
    }
}
