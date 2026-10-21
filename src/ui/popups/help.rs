use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin},
    style::{Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Tabs,
    },
};

use crate::{
    app::{App, types::FocusTarget},
    configs::ThemeConfig,
    overlays::{HelpCategory, HelpDialog},
    ui::common::{border_style_for, popup_area},
};

pub fn render_help_popup(app: &App, dialog: &HelpDialog, frame: &mut Frame, theme: &ThemeConfig) {
    let area = popup_area(frame.area(), 74, 20, 32, 14);
    frame.render_widget(Clear, area);

    let border_style = border_style_for(
        FocusTarget::Overlay,
        app.focus_target(),
        app.is_app_focused(),
        theme,
    );

    let is_compact_vertical = area.height < 14;

    let mut outer_block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(" Help [Keybindings] ");

    if !is_compact_vertical {
        outer_block = outer_block.title_bottom(Line::from(vec![
            Span::styled(
                "Tab/Arrows: ",
                Style::default().fg(theme.palette.text_muted),
            ),
            Span::raw("Category  "),
            Span::styled(
                "j/k/Scroll: ",
                Style::default().fg(theme.palette.text_muted),
            ),
            Span::raw("Move  "),
            Span::styled("Esc/q: ", Style::default().fg(theme.palette.text_muted)),
            Span::raw("Close"),
        ]));
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
                format!("Prefix Leader Key: <{prefix_str}>"),
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
    }

    let total_lines = lines.len();
    let visible_height = content_area.height as usize;
    let max_scroll = total_lines.saturating_sub(visible_height);

    dialog.max_scroll.set(max_scroll);
    let current_scroll = dialog.scroll.min(max_scroll);

    let paragraph = Paragraph::new(lines).scroll((current_scroll as u16, 0));
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
