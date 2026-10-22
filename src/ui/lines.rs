use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    app::{
        App,
        data_processing::{
            build_display_to_raw_byte_map, displayable_line_string, map_disp_byte_to_raw,
        },
        search::SearchMatch,
        types::{FocusTarget, LogEntry},
    },
    configs::{
        KeywordHighlight, SettingsConfig, ThemeConfig, WrapMode,
        settings::{PrefixWrapBehavior, RawSideView, raw_view_to_display_mode},
    },
    ui::{
        PostRenderEffect,
        common::{
            apply_keyword_highlights, apply_selection_to_spans, border_style_for, render_search_bar,
        },
        data_display::format_line_content,
        layout::{PreparedLayout, prepare_view_layout},
        raw_side::{
            CharHighlight, format_raw_bytes_with_highlights, raw_panel_layout,
            render_raw_panel_widget,
        },
    },
};

pub fn history_entry_to_line(
    entry: &LogEntry,
    settings: &SettingsConfig,
    line_matches: &[(usize, &SearchMatch)],
    active_match_idx: usize,
    selection_range: Option<(usize, usize)>,
    theme: &ThemeConfig,
    cached_prefix: &str,
) -> Line<'static> {
    let mut spans = Vec::new();
    let display_mode = settings.line_mode.display_mode;
    let timestamp_in_line = settings.show_timestamps;

    if timestamp_in_line {
        let formatted_timestamp = entry
            .timestamp
            .format(&settings.timestamp_format)
            .to_string();
        spans.push(Span::styled(
            formatted_timestamp,
            *theme.line_mode.timestamp,
        ));
    }

    spans.push(Span::styled(
        cached_prefix.to_string(),
        theme.line_mode.line_prefix,
    ));

    let mut content_spans = format_line_content(&entry.text, display_mode, theme);

    if !line_matches.is_empty() {
        let normal_style = *theme.search.search_match;
        let active_style = *theme.search.search_match_active;

        for &(global_idx, m) in line_matches {
            let style = if global_idx == active_match_idx {
                active_style
            } else {
                normal_style
            };
            let char_len = m.match_char_len.max(1);
            let inclusive_end = m.col_idx + char_len - 1;
            content_spans =
                apply_selection_to_spans(content_spans, m.col_idx, inclusive_end, style);
        }
    }

    if let Some((sel_start, sel_end)) = selection_range {
        content_spans =
            apply_selection_to_spans(content_spans, sel_start, sel_end, *theme.ui.selection);
    }

    spans.extend(content_spans);
    Line::from(spans)
}

pub fn render_monitor(
    app: &mut App,
    frame: &mut Frame,
    content_area: Rect,
    raw_side_area: Option<Rect>,
    scrollbar_area: Option<Rect>,
    theme: &ThemeConfig,
    highlights: &[KeywordHighlight],
) -> PostRenderEffect {
    let content_width = content_area.width as usize;
    let content_height = content_area.height as usize;
    let total_entries = app.line_mode.history().len();
    let wrap_mode = app.settings().line_mode.wrap_mode;
    let raw_view = app.settings().raw_side.view;

    if total_entries == 0 || content_width == 0 || content_height == 0 {
        return PostRenderEffect::None;
    }

    let is_search_active =
        app.line_mode.search().active && !app.line_mode.search().matches.is_empty();
    let active_match_idx = app.line_mode.search().current_match;
    let copy_range = app.line_mode.copy_state().normalized_selection();

    let global_max_scroll = total_entries.saturating_sub(1);

    if let Some((target_line, target_col)) = app.line_mode.search_mut().target_jump.take() {
        let line_from_bottom = (total_entries.saturating_sub(1)).saturating_sub(target_line);
        app.line_mode
            .scroll_to(line_from_bottom.min(global_max_scroll));

        if wrap_mode == WrapMode::Truncate {
            let half_x = content_width / 2;
            let gutter_w = app.settings().line_gutter_width();
            let centered_pan_x = (gutter_w + target_col).saturating_sub(half_x);
            app.line_mode.scroller_mut().offset_x = centered_pan_x;
        }
    }

    // NOTE: check the window logic again
    let active_focus = if app.line_mode.copy_state().active {
        Some((
            app.line_mode.copy_state().cursor.line_idx,
            app.line_mode.copy_state().cursor.char_idx,
        ))
    } else if is_search_active {
        app.line_mode.search().current_match_position()
    } else {
        None
    };

    let scroll_from_bottom = app.line_mode.scroll_offset();
    let clamped_scroll = scroll_from_bottom.min(global_max_scroll);

    let focus_entry = if let Some((target_line, _)) = active_focus {
        target_line
    } else {
        (total_entries.saturating_sub(1)).saturating_sub(clamped_scroll)
    };

    // Overscan window centered around focus_entry:
    // Large enough so heavy sub-line modes (Binary/Raw) have plenty of buffer:
    let overscan = content_height.max(35);
    let start_idx = focus_entry.saturating_sub(overscan);
    let end_idx = (focus_entry + overscan + 1).min(total_entries);

    let search = app.line_mode.search();
    let line_prefix_cached = app.settings().line_mode.line_prefix.clone();

    let mut visible_lines: Vec<Line> = app
        .line_mode
        .history()
        .range(start_idx..end_idx)
        .enumerate()
        .map(|(local_idx, entry)| {
            let global_idx = start_idx + local_idx;

            let line_matches: Vec<(usize, &SearchMatch)> = if is_search_active {
                let start = search.matches.partition_point(|m| m.line_idx < global_idx);
                let end = search.matches.partition_point(|m| m.line_idx <= global_idx);
                search.matches[start..end]
                    .iter()
                    .enumerate()
                    .map(|(offset, m)| (start + offset, m))
                    .collect()
            } else {
                Vec::new()
            };

            let sel_range = copy_range.and_then(|(start, end)| {
                if global_idx < start.line_idx || global_idx > end.line_idx {
                    None
                } else {
                    let from = if global_idx == start.line_idx {
                        start.char_idx
                    } else {
                        0
                    };
                    let to = if global_idx == end.line_idx {
                        end.char_idx
                    } else {
                        usize::MAX
                    };
                    Some((from, to))
                }
            });

            history_entry_to_line(
                entry,
                app.settings(),
                &line_matches,
                active_match_idx,
                sel_range,
                theme,
                &line_prefix_cached,
            )
        })
        .collect();

    if app.settings().line_mode.show_partial_rx && !app.line_mode.partial_rx().is_empty() {
        let partial_bytes = app.line_mode.partial_rx();
        let trimmed_bytes = partial_bytes.trim_ascii_end();

        if !trimmed_bytes.is_empty() {
            let display_mode = app.settings().line_mode.display_mode;
            let partial_raw = String::from_utf8_lossy(trimmed_bytes);

            let mut partial_spans = Vec::new();

            if app.settings().show_timestamps
                && let Some(partial_timestamp) = app.line_mode.partial_line_timestamp()
            {
                let time = partial_timestamp
                    .format(&app.settings().timestamp_format)
                    .to_string();
                partial_spans.push(Span::styled(time, theme.line_mode.timestamp));
            }

            let partial_prefix = app.settings().line_mode.line_partial_prefix.to_string();
            let partial_prefix_style = *theme.line_mode.line_partial_prefix;
            partial_spans.push(Span::styled(partial_prefix, partial_prefix_style));
            partial_spans.extend(format_line_content(&partial_raw, display_mode, theme));
            visible_lines.push(Line::from(partial_spans));
        }
    }

    let layout = prepare_view_layout(visible_lines, app.settings(), theme, content_width);
    let max_pan = match wrap_mode {
        WrapMode::Word | WrapMode::Char => 0,
        WrapMode::Truncate => layout.max_line_width.saturating_sub(content_width),
    };

    app.line_mode.update_limits(global_max_scroll, max_pan);

    let total_visual_rows = layout.visual_lines.len();

    // NOTE: check this again
    let paragraph_y_offset: usize = if clamped_scroll == 0 && !is_search_active {
        total_visual_rows.saturating_sub(content_height)
    } else {
        let focus_local = focus_entry.saturating_sub(start_idx);
        let target_visual_row = if let Some(meta) = layout.line_layouts.get(focus_local) {
            let target_char = active_focus.map(|(_, c)| c).unwrap_or(0);
            let mut sub_row = 0;
            let mut char_acc = 0;
            for (s_idx, sub) in meta.sub_lines.iter().enumerate() {
                if target_char >= char_acc && target_char < char_acc + sub.content_char_len {
                    sub_row = s_idx;
                    break;
                }
                char_acc += sub.content_char_len;
            }
            meta.visual_row_start + sub_row
        } else {
            0
        };

        let half = content_height / 2;
        let ideal_offset = target_visual_row.saturating_sub(half);
        let max_offset = total_visual_rows.saturating_sub(content_height);
        ideal_offset.min(max_offset)
    };

    let line_x_offset = match wrap_mode {
        WrapMode::Word | WrapMode::Char => 0,
        WrapMode::Truncate => app.line_mode.x_offset(),
    };

    if let Some((target_line, target_char)) = active_focus
        && target_line >= start_idx
        && target_line < end_idx
        && let Some(line_meta) = layout.line_layouts.get(target_line - start_idx)
    {
        let mut local_byte_offset = 0;
        let mut total_sub_bytes = 0;

        if let Some(entry) = app.line_mode.history().get(target_line) {
            let raw_bytes = entry.text.as_bytes();
            let display_text =
                displayable_line_string(&entry.text, app.settings().line_mode.display_mode);
            let disp_map =
                build_display_to_raw_byte_map(raw_bytes, app.settings().line_mode.display_mode);

            let target_raw_byte = if is_search_active {
                app.line_mode
                    .search()
                    .matches
                    .get(active_match_idx)
                    .map(|m| m.byte_offset)
                    .unwrap_or(0)
            } else {
                let disp_b = display_text
                    .char_indices()
                    .nth(target_char)
                    .map(|(b, _)| b)
                    .unwrap_or(display_text.len());
                map_disp_byte_to_raw(disp_b, &disp_map, raw_bytes.len())
            };

            let mut disp_char_to_byte = Vec::with_capacity(display_text.chars().count() + 1);
            for (b_idx, _) in display_text.char_indices() {
                disp_char_to_byte.push(b_idx);
            }
            disp_char_to_byte.push(display_text.len());

            let mut char_offset = 0;
            let visual_sub_count = line_meta.sub_lines.len();

            for (sub_idx, sub) in line_meta.sub_lines.iter().enumerate() {
                let is_last = sub_idx == visual_sub_count.saturating_sub(1);
                let sub_char_end = (char_offset + sub.content_char_len)
                    .min(disp_char_to_byte.len().saturating_sub(1));

                let disp_b_start = disp_char_to_byte.get(char_offset).copied().unwrap_or(0);
                let disp_b_end = disp_char_to_byte
                    .get(sub_char_end)
                    .copied()
                    .unwrap_or(display_text.len());

                let raw_b_start = if char_offset == 0 {
                    0
                } else {
                    map_disp_byte_to_raw(disp_b_start, &disp_map, raw_bytes.len())
                };

                let raw_b_end = if is_last {
                    raw_bytes.len()
                } else {
                    map_disp_byte_to_raw(disp_b_end, &disp_map, raw_bytes.len())
                };

                if (target_raw_byte >= raw_b_start && target_raw_byte < raw_b_end) || is_last {
                    local_byte_offset = target_raw_byte.saturating_sub(raw_b_start);
                    total_sub_bytes = raw_b_end.saturating_sub(raw_b_start);
                    break;
                }

                char_offset = sub_char_end;
            }
        }

        if let Some(raw_area) = raw_side_area {
            let usable_width = (raw_area.width as usize).saturating_sub(1);

            if is_search_active {
                app.line_mode.center_raw_pan_on_byte(
                    raw_view,
                    usable_width,
                    local_byte_offset,
                    total_sub_bytes,
                );
            } else {
                app.line_mode.sync_raw_pan_with_local_col(
                    raw_view,
                    usable_width,
                    local_byte_offset,
                    total_sub_bytes,
                );
            }
        }
    }

    if let Some(raw_area) = raw_side_area {
        render_raw_side_panel(
            app,
            frame,
            raw_area,
            paragraph_y_offset,
            start_idx,
            &layout,
            theme,
        );
    }

    let history_paragraph = Paragraph::new(layout.visual_lines)
        .style(*theme.line_mode.history)
        .scroll((
            paragraph_y_offset.min(u16::MAX as usize) as u16,
            line_x_offset.min(u16::MAX as usize) as u16,
        ));
    frame.render_widget(history_paragraph, content_area);

    apply_keyword_highlights(frame.buffer_mut(), content_area, highlights);

    if app.line_mode.copy_state().active {
        let copy_state = app.line_mode.copy_state();
        let cursor_line = copy_state.cursor.line_idx;
        let cursor_char = copy_state.cursor.char_idx;

        let indent_width = match app.settings().line_mode.wrap_indent {
            PrefixWrapBehavior::None => 0,
            PrefixWrapBehavior::Prefix => app.settings().line_mode.line_indent_prefix.width(),
            PrefixWrapBehavior::Timestamp => {
                let prefix_w = app.settings().line_mode.line_prefix.width();
                let ts_width = if app.settings().show_timestamps {
                    app.settings().line_gutter_width().saturating_sub(prefix_w)
                } else {
                    0
                };
                ts_width + app.settings().line_mode.line_indent_prefix.width()
            }
        };

        if cursor_line >= start_idx
            && cursor_line < end_idx
            && let Some(line_meta) = layout.line_layouts.get(cursor_line - start_idx)
        {
            let mut resolved_row = line_meta.visual_row_start;
            let mut resolved_col = 0;
            let mut remaining = cursor_char;

            for (sub_idx, sub) in line_meta.sub_lines.iter().enumerate() {
                let is_last_sub_line = sub_idx == line_meta.sub_lines.len().saturating_sub(1);

                if remaining < sub.content_char_len || is_last_sub_line {
                    resolved_row = line_meta.visual_row_start + sub_idx;

                    let mut gutter_to_skip = sub.gutter_chars;
                    let mut content_to_take = remaining;
                    let mut accumulated_width = 0;

                    if sub_idx > 0 {
                        accumulated_width += indent_width;
                    }

                    for span in &sub.spans {
                        for ch in span.content.chars() {
                            let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
                            if gutter_to_skip > 0 {
                                gutter_to_skip -= 1;
                                accumulated_width += cw;
                            } else if content_to_take > 0 {
                                content_to_take -= 1;
                                accumulated_width += cw;
                            } else {
                                break;
                            }
                        }
                    }

                    resolved_col = accumulated_width;
                    break;
                }

                remaining = remaining.saturating_sub(sub.content_char_len);
            }

            if resolved_row >= paragraph_y_offset {
                let viewer_row = resolved_row - paragraph_y_offset;

                if viewer_row < content_area.height as usize && resolved_col >= line_x_offset {
                    let viewer_col = resolved_col - line_x_offset;
                    if viewer_col < content_area.width as usize {
                        let screen_x = content_area.x + viewer_col as u16;
                        let screen_y = content_area.y + viewer_row as u16;

                        let buf = frame.buffer_mut();
                        if !app.is_app_focused() {
                            buf[(screen_x, screen_y)]
                                .set_style(Style::default().add_modifier(Modifier::REVERSED));
                        }
                        frame.set_cursor_position((screen_x, screen_y));
                    }
                }
            }
        }
    }

    if let Some(sb_area) = scrollbar_area {
        let has_borders = app.settings().ui.show_borders;

        if total_entries > content_height {
            let scrollbar_pos = global_max_scroll.saturating_sub(clamped_scroll);
            let mut scrollbar_state = ScrollbarState::new(global_max_scroll)
                .viewport_content_length(content_height)
                .position(scrollbar_pos);

            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("▲"))
                .end_symbol(Some("▼"))
                .track_symbol(Some("│"))
                .thumb_symbol("█")
                .thumb_style(*theme.ui.scrollbar_thumb)
                .track_style(*theme.ui.scrollbar_track)
                .begin_style(*theme.ui.scrollbar_thumb)
                .end_style(*theme.ui.scrollbar_thumb);

            frame.render_stateful_widget(scrollbar, sb_area, &mut scrollbar_state);
        } else if !has_borders {
            let blank = Paragraph::new(" ");
            frame.render_widget(blank, sb_area);
        }
    }

    if app.line_mode.search().active {
        render_search_bar(app, frame, content_area, theme);
    }

    PostRenderEffect::None
}

pub fn render_input_prompt(app: &App, frame: &mut Frame, area: Rect, theme: &ThemeConfig) {
    let input = app.line_mode.input();

    let prompt_prefix = if app.line_mode.is_prompt_vi() {
        &app.settings().line_mode.prompt_vi_prefix
    } else {
        &app.settings().line_mode.prompt_prefix
    };

    let prefix_width = prompt_prefix.len() as u16;

    let border_style = border_style_for(
        app.focus_target(),
        FocusTarget::LineInput,
        app.is_app_focused(),
        theme,
    );

    let has_borders = app.settings().ui.show_borders;

    let (block, inner_area) = if has_borders {
        let b = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .style(*theme.ui.background)
            .title(Span::styled(" Transmit (TX) ", border_style));
        let inner = b.inner(area);
        (Some(b), inner)
    } else {
        (None, area)
    };

    let inner_width = (inner_area.width.saturating_sub(prefix_width)) as usize;
    let scroll = input.visual_scroll(inner_width);

    let input_text = Line::from(vec![
        Span::styled(
            prompt_prefix,
            Style::default()
                .fg(theme.palette.main)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(input.value()),
    ]);

    if let Some(b) = block {
        frame.render_widget(b, area);
    }

    let input_widget = Paragraph::new(input_text)
        .style(*theme.line_mode.prompt)
        .scroll((0, scroll as u16));

    frame.render_widget(input_widget, inner_area);

    if !app.line_mode.search().active && !app.line_mode.copy_state().active {
        let cursor_x =
            inner_area.x + prefix_width + (input.visual_cursor().saturating_sub(scroll)) as u16;
        let cursor_y = inner_area.y;

        if !app.is_app_focused() {
            let buf = frame.buffer_mut();
            buf[(cursor_x, cursor_y)].set_style(*theme.ui.cursor_unfocused);
            buf[(cursor_x, cursor_y)].set_symbol(&app.settings().line_mode.unfocused_cursor_glyph);
        }
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

// NOTE: verify evolution with mapping
pub fn render_raw_side_panel(
    app: &mut App,
    frame: &mut Frame,
    raw_area: Rect,
    paragraph_y_offset: usize,
    start_idx: usize,
    layout: &PreparedLayout,
    theme: &ThemeConfig,
) {
    let raw_view = app.settings().raw_side.view;
    if raw_view == RawSideView::Off || raw_area.width == 0 || raw_area.height == 0 {
        return;
    }

    let (block, inner_area) =
        raw_panel_layout(raw_area, app.settings().raw_side.orientation, theme);
    let view_height = inner_area.height as usize;
    let visible_visual_start = paragraph_y_offset;
    let visible_visual_end = visible_visual_start + view_height;

    let raw_display_mode = raw_view_to_display_mode(raw_view);
    let copy_range = app.line_mode.copy_state().normalized_selection();
    let history = app.line_mode.history();
    let is_search_active =
        app.line_mode.search().active && !app.line_mode.search().matches.is_empty();
    let active_match_idx = app.line_mode.search().current_match;

    let normal_style = *theme.search.search_match;
    let active_style = *theme.search.search_match_active;

    let mut visible_raw_lines = Vec::with_capacity(view_height);
    let mut current_visual_row = 0;

    for (local_layout_idx, entry_layout) in layout.line_layouts.iter().enumerate() {
        let global_idx = start_idx + local_layout_idx;

        let visual_sub_count = entry_layout.sub_lines.len();
        let entry_visual_end = current_visual_row + visual_sub_count;

        if entry_visual_end <= visible_visual_start {
            current_visual_row = entry_visual_end;
            continue;
        }
        if current_visual_row >= visible_visual_end {
            break;
        }

        if let Some(entry) = history.get(global_idx) {
            let raw_bytes = entry.text.as_bytes();
            let display_text =
                displayable_line_string(&entry.text, app.settings().line_mode.display_mode);

            // Display text character -> byte mapping:
            let mut disp_char_to_byte = Vec::with_capacity(display_text.chars().count() + 1);
            for (b_idx, _) in display_text.char_indices() {
                disp_char_to_byte.push(b_idx);
            }
            disp_char_to_byte.push(display_text.len());

            // Map from display byte -> raw wire byte:
            let disp_map =
                build_display_to_raw_byte_map(raw_bytes, app.settings().line_mode.display_mode);

            let mut char_offset = 0;

            let line_matches: Vec<(usize, &SearchMatch)> = if is_search_active {
                let search = app.line_mode.search();
                let start = search.matches.partition_point(|m| m.line_idx < global_idx);
                let end = search.matches.partition_point(|m| m.line_idx <= global_idx);
                search.matches[start..end]
                    .iter()
                    .enumerate()
                    .map(|(offset, m)| (start + offset, m))
                    .collect()
            } else {
                Vec::new()
            };

            for (sub_idx, sub) in entry_layout.sub_lines.iter().enumerate() {
                let line_y = current_visual_row + sub_idx;
                let is_last_sub = sub_idx == visual_sub_count.saturating_sub(1);

                let sub_char_end = (char_offset + sub.content_char_len)
                    .min(disp_char_to_byte.len().saturating_sub(1));

                if line_y >= visible_visual_start && line_y < visible_visual_end {
                    let disp_byte_start = disp_char_to_byte.get(char_offset).copied().unwrap_or(0);
                    let disp_byte_end = disp_char_to_byte
                        .get(sub_char_end)
                        .copied()
                        .unwrap_or(display_text.len());

                    let raw_byte_start = if char_offset == 0 {
                        0
                    } else {
                        map_disp_byte_to_raw(disp_byte_start, &disp_map, raw_bytes.len())
                    };

                    let raw_byte_end = if is_last_sub {
                        raw_bytes.len()
                    } else {
                        map_disp_byte_to_raw(disp_byte_end, &disp_map, raw_bytes.len())
                    };

                    let sub_raw_bytes = &raw_bytes
                        [raw_byte_start.min(raw_bytes.len())..raw_byte_end.min(raw_bytes.len())];

                    let mut highlights = Vec::new();

                    // 1. Search Highlights (m.byte_offset is already in raw byte space!)
                    for &(global_m_idx, m) in &line_matches {
                        let m_start = m.byte_offset;
                        let m_end = m.byte_offset + m.byte_len;
                        if m_end > raw_byte_start && m_start < raw_byte_end {
                            highlights.push(CharHighlight {
                                start_col: m_start.saturating_sub(raw_byte_start),
                                end_col: m_end.saturating_sub(raw_byte_start),
                                style: if global_m_idx == active_match_idx {
                                    active_style
                                } else {
                                    normal_style
                                },
                            });
                        }
                    }

                    // 2. Selection Highlights:
                    if let Some((sel_start, sel_end)) = copy_range
                        && global_idx >= sel_start.line_idx
                        && global_idx <= sel_end.line_idx
                    {
                        let is_first_line = global_idx == sel_start.line_idx;
                        let is_last_line = global_idx == sel_end.line_idx;

                        let raw_from_byte = if !is_first_line || sel_start.char_idx == 0 {
                            0 // Include ALL leading escape codes!
                        } else {
                            let disp_from_b = disp_char_to_byte
                                .get(sel_start.char_idx)
                                .copied()
                                .unwrap_or(0);
                            map_disp_byte_to_raw(disp_from_b, &disp_map, raw_bytes.len())
                        };

                        let raw_to_byte = if !is_last_line {
                            raw_bytes.len() // Include ALL trailing bytes, \r, \n!
                        } else {
                            let to_char = sel_end.char_idx + 1;
                            if to_char >= display_text.chars().count() {
                                raw_bytes.len()
                            } else {
                                let disp_to_b = disp_char_to_byte
                                    .get(to_char)
                                    .copied()
                                    .unwrap_or(display_text.len());
                                map_disp_byte_to_raw(disp_to_b, &disp_map, raw_bytes.len())
                            }
                        };

                        if raw_to_byte > raw_byte_start && raw_from_byte < raw_byte_end {
                            highlights.push(CharHighlight {
                                start_col: raw_from_byte.saturating_sub(raw_byte_start),
                                end_col: raw_to_byte.saturating_sub(raw_byte_start),
                                style: *theme.ui.selection,
                            });
                        }
                    }

                    visible_raw_lines.push(format_raw_bytes_with_highlights(
                        sub_raw_bytes,
                        raw_display_mode,
                        &highlights,
                        theme,
                    ));
                }

                char_offset = sub_char_end;
            }
        }

        current_visual_row = entry_visual_end;
    }

    if app.settings().line_mode.show_partial_rx
        && !app.line_mode.partial_rx().is_empty()
        && visible_raw_lines.len() < view_height
    {
        visible_raw_lines.push(format_raw_bytes_with_highlights(
            app.line_mode.partial_rx(),
            raw_display_mode,
            &[],
            theme,
        ));
    }

    let max_visible_raw_width = visible_raw_lines
        .iter()
        .map(|l| l.width())
        .max()
        .unwrap_or(0);

    let usable_width = inner_area.width as usize;
    app.line_mode
        .update_raw_limits(max_visible_raw_width.saturating_sub(usable_width));

    let raw_pan_x = app.line_mode.raw_pan_x().min(app.line_mode.raw_pan_x_max());

    render_raw_panel_widget(
        frame,
        raw_area,
        inner_area,
        block,
        visible_raw_lines,
        raw_pan_x,
    );
}
