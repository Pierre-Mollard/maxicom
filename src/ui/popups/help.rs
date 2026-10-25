use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin},
    style::{Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Clear, Padding, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Tabs},
};

use crate::{
    app::App,
    configs::{ThemeConfig, keybindings::KeyBindingDialogAction},
    overlays::{HelpCategory, HelpDialog},
    ui::{
        common::popup_area,
        popups::{key_hints, popup_block},
    },
};

pub fn render_help_popup(app: &App, dialog: &HelpDialog, frame: &mut Frame, theme: &ThemeConfig) {
    let area = popup_area(frame.area(), 74, 20);
    frame.render_widget(Clear, area);

    let is_compact_vertical = area.height < 14;

    let hints = if is_compact_vertical {
        Line::default()
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
    let outer_block = popup_block("Help", hints, theme).padding(Padding::horizontal(1));

    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let tabs_height = if is_compact_vertical { 1 } else { 2 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(tabs_height), Constraint::Min(0)])
        .split(inner_area);

    let tabs_area = chunks[0];
    let content_area = chunks[1];

    let tab_titles: Vec<Line> = HelpCategory::ALL
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

    let selected_index = HelpCategory::ALL
        .iter()
        .position(|&c| c == dialog.category)
        .unwrap_or(0);

    let tabs = Tabs::new(tab_titles)
        .style(*theme.popup.background)
        .select(selected_index)
        .divider(symbols::line::VERTICAL)
        .highlight_style(
            Style::default()
                .fg(theme.palette.secondary)
                .add_modifier(Modifier::UNDERLINED),
        );
    frame.render_widget(tabs, tabs_area);

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

            for (pattern, action) in sorted_keys {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {prefix_str} + {:<10} ", format!("{pattern}")),
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
        HelpCategory::CopyMode => {
            let mut sorted_keys: Vec<_> = app.key_resolver.copy_actions.iter().collect();
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

    let total_lines = lines.len();
    let visible_height = content_area.height as usize;
    let max_scroll = total_lines.saturating_sub(visible_height);

    dialog.max_scroll.set(max_scroll);
    let current_scroll = dialog.scroll.min(max_scroll);

    let paragraph = Paragraph::new(lines)
        .style(*theme.popup.background)
        .scroll((current_scroll as u16, 0));
    frame.render_widget(paragraph, content_area);

    if total_lines > visible_height {
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
}
