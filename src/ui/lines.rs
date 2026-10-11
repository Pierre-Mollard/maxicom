// TODO: see what in common with the render in terminal mode
// TODO: check if factorize panning logic in terminal and line mode
// TODO: center the occurance of search when next/previous and when typing too
// TODO: add highlight logic to terminal too and fix to match what's here

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    app::{
        App,
        data_processing::displayable_line_string,
        types::{FocusTarget, LogEntry},
    },
    configs::{
        KeywordHighlight, SettingsConfig, ThemeConfig, WrapMode,
        settings::{PrefixWrapBehavior, RawSideOrientation, RawSideView, raw_view_to_display_mode},
    },
    ui::{
        PostRenderEffect,
        common::{
            apply_keyword_highlights, apply_search_highlight, apply_selection_to_spans,
            border_style_for, highlight_raw_byte_range, render_search_bar,
        },
        data_display::format_line_content,
        layout::{PreparedLayout, prepare_view_layout},
    },
};

pub fn history_entry_to_line(
    entry: &LogEntry,
    settings: &SettingsConfig,
    search_query: &str,
    active_occurrence: Option<usize>,
    selection_range: Option<(usize, usize)>,
    theme: &ThemeConfig,
) -> Line<'static> {
    let mut spans = Vec::new();
    let display_mode = settings.line_mode.display_mode;
    let timestamp_in_line = settings.display_live_timestamp;
    let timestamp_format = &settings.timestamp_format;
    let line_prefix = &settings.line_mode.line_prefix;

    if timestamp_in_line {
        let time = entry.timestamp.format(timestamp_format).to_string();
        spans.push(Span::styled(time, *theme.timestamp_line));
    }

    spans.push(Span::styled(line_prefix.to_string(), theme.line_prefix));

    let mut content_spans = format_line_content(&entry.text, display_mode);

    if !search_query.is_empty() {
        content_spans = apply_search_highlight(content_spans, search_query, active_occurrence);
    }

    if let Some((sel_start, sel_end)) = selection_range {
        content_spans =
            apply_selection_to_spans(content_spans, sel_start, sel_end, *theme.selection);
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
    let wrap_mode = app.settings().line_mode.wrap_mode;
    let raw_view = app.settings().raw_side.view;
    let post_effect = PostRenderEffect::None;

    let is_search_active = app.line_mode.search().active;
    let search_query = if is_search_active {
        app.line_mode.search().query_str().to_string()
    } else {
        String::new()
    };

    let copy_range = app.line_mode.copy_state().normalized_selection();

    let mut history_lines: Vec<Line> = app
        .line_mode
        .history()
        .iter()
        .enumerate()
        .map(|(idx, entry)| {
            let active_occ = if is_search_active {
                app.line_mode.search().active_occurrence_for_line(idx)
            } else {
                None
            };

            let sel_range = copy_range.and_then(|(start, end)| {
                if idx < start.line_idx || idx > end.line_idx {
                    None
                } else {
                    let from = if idx == start.line_idx {
                        start.char_idx
                    } else {
                        0
                    };
                    let to = if idx == end.line_idx {
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
                &search_query,
                active_occ,
                sel_range,
                theme,
            )
        })
        .collect();

    if app.settings().line_mode.display_partial_rx && !app.line_mode.partial_rx().is_empty() {
        let display_mode = app.settings().line_mode.display_mode;
        let partial_raw = String::from_utf8_lossy(app.line_mode.partial_rx());

        let mut partial_spans = Vec::new();

        if app.settings().display_live_timestamp
            && let Some(partial_timestamp) = app.line_mode.partial_line_timestamp()
        {
            let time = partial_timestamp
                .format(&app.settings().timestamp_format)
                .to_string();
            partial_spans.push(Span::styled(time, theme.timestamp_line));
        }

        let partial_prefix = app.settings().line_mode.line_partial_prefix.to_string();
        let partial_prefix_style = *theme.line_partial_prefix;
        partial_spans.push(Span::styled(partial_prefix, partial_prefix_style));
        partial_spans.extend(format_line_content(&partial_raw, display_mode));
        history_lines.push(Line::from(partial_spans));
    }

    let layout = prepare_view_layout(history_lines, app.settings(), theme, content_width);
    let total_visual_rows = layout.visual_lines.len();
    let max_pan = match wrap_mode {
        WrapMode::Word | WrapMode::Char => 0,
        WrapMode::Truncate => layout.max_line_width.saturating_sub(content_width),
    };

    let max_scroll = total_visual_rows.saturating_sub(content_height);
    app.line_mode.update_limits(max_scroll, max_pan);

    if let Some(target_idx) = app.line_mode.search_mut().target_jump_index.take()
        && target_idx < layout.line_layouts.len()
    {
        let target_visual_row = layout.line_layouts[target_idx].visual_row_start;

        let rows_from_bottom = total_visual_rows
            .saturating_sub(1)
            .saturating_sub(target_visual_row);
        let half_screen = content_height / 2;
        let centered_scroll = rows_from_bottom.saturating_sub(half_screen);

        app.line_mode.scroll_to(centered_scroll.min(max_scroll));
    }

    let scroll_from_bottom = app.line_mode.scroll_offset();
    let paragraph_y_offset = max_scroll.saturating_sub(scroll_from_bottom);
    let line_x_offset = match wrap_mode {
        WrapMode::Word | WrapMode::Char => 0,
        WrapMode::Truncate => app.line_mode.x_offset(),
    };

    // NOTE: review this
    // side view raw panel
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

    if let Some((target_line, target_char)) = active_focus
        && let Some(line_meta) = layout.line_layouts.get(target_line)
    {
        let mut remaining = target_char;
        let mut local_byte_offset = 0;
        let mut total_sub_bytes = 0;

        if let Some(entry) = app.line_mode.history().get(target_line) {
            let display_text =
                displayable_line_string(&entry.text, app.settings().line_mode.display_mode);
            let entry_chars: Vec<char> = display_text.chars().collect();
            let mut char_offset = 0;
            let visual_sub_count = line_meta.sub_lines.len();

            for (sub_idx, sub) in line_meta.sub_lines.iter().enumerate() {
                let is_last = sub_idx == visual_sub_count.saturating_sub(1);
                let sub_char_end = if is_last {
                    entry_chars.len()
                } else {
                    (char_offset + sub.content_char_len).min(entry_chars.len())
                };

                let sub_slice: String = if char_offset < entry_chars.len() {
                    entry_chars[char_offset..sub_char_end].iter().collect()
                } else {
                    String::new()
                };

                let current_sub_chars = sub_slice.chars().count();

                if remaining < current_sub_chars || is_last {
                    let local_char = remaining.min(current_sub_chars.saturating_sub(1));

                    let byte_offset = sub_slice
                        .char_indices()
                        .nth(local_char)
                        .map(|(b, _)| b)
                        .unwrap_or(sub_slice.len());

                    local_byte_offset = byte_offset;
                    total_sub_bytes = sub_slice.len();
                    break;
                }

                remaining = remaining.saturating_sub(current_sub_chars);
                char_offset = sub_char_end;
            }
        }

        if let Some(raw_area) = raw_side_area {
            let usable_width = (raw_area.width as usize).saturating_sub(1);
            app.line_mode.sync_raw_pan_with_local_col(
                raw_view,
                usable_width,
                local_byte_offset,
                total_sub_bytes,
            );
        }
    }

    if let Some(raw_area) = raw_side_area {
        render_raw_side_panel(
            app,
            frame,
            raw_area,
            paragraph_y_offset,
            &search_query,
            &layout,
            theme,
        );
    }

    // main history
    let history_paragraph = Paragraph::new(layout.visual_lines).scroll((
        paragraph_y_offset.min(u16::MAX as usize) as u16,
        line_x_offset.min(u16::MAX as usize) as u16,
    ));
    frame.render_widget(history_paragraph, content_area);

    apply_keyword_highlights(frame.buffer_mut(), content_area, highlights);

    // selection cursor
    if app.line_mode.copy_state().active {
        let copy_state = app.line_mode.copy_state();
        let cursor_line = copy_state.cursor.line_idx;
        let cursor_char = copy_state.cursor.char_idx;

        let indent_width = match app.settings().line_mode.wrap_indent {
            PrefixWrapBehavior::None => 0,
            PrefixWrapBehavior::Prefix => app.settings().line_mode.line_indent_prefix.width(),
        };

        if let Some(line_meta) = layout.line_layouts.get(cursor_line) {
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

    // scrollbar
    if let Some(sb_area) = scrollbar_area {
        let has_borders = app.settings().ui.show_borders;

        if total_visual_rows > content_height {
            let mut scrollbar_state = ScrollbarState::new(max_scroll)
                .viewport_content_length(content_height)
                .position(paragraph_y_offset);

            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("▲"))
                .end_symbol(Some("▼"))
                .track_symbol(if has_borders { Some("│") } else { Some(" ") })
                .thumb_symbol("█");

            frame.render_stateful_widget(scrollbar, sb_area, &mut scrollbar_state);
        } else if !has_borders {
            let blank = Paragraph::new(" ");
            frame.render_widget(blank, sb_area);
        }
    }

    // search
    if app.line_mode.search().active {
        render_search_bar(app, frame, content_area, theme);
    }

    post_effect
}

pub fn render_input_prompt(app: &App, frame: &mut Frame, area: Rect, theme: &ThemeConfig) {
    let input = app.line_mode.input();

    let prompt_prefix = if app.line_mode.is_prompt_vi() {
        &app.settings().line_mode.prompt_vi_prefix
    } else {
        &app.settings().line_mode.prompt_prefix
    };

    let prefix_width = prompt_prefix.len() as u16;
    let inner_width = (area.width.saturating_sub(2 + prefix_width)) as usize;

    let scroll = input.visual_scroll(inner_width);

    let input_text = Line::from(vec![
        Span::styled(
            prompt_prefix,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(input.value()),
    ]);

    let border_style = border_style_for(
        app.focus_target(),
        FocusTarget::LineInput,
        app.is_app_focused(),
        theme,
    );

    let mut block = Block::default();
    if app.settings().ui.show_borders {
        block = block
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(" Transmit (TX) ");
    } else {
        block = block.borders(Borders::NONE);
    }

    let input_widget = Paragraph::new(input_text)
        .scroll((0, scroll as u16))
        .block(block);

    frame.render_widget(input_widget, area);

    if !app.line_mode.search().active && !app.line_mode.copy_state().active {
        let cursor_x = if app.settings().ui.show_borders {
            area.x + 1 + prefix_width + (input.visual_cursor().saturating_sub(scroll)) as u16
        } else {
            area.x + prefix_width + (input.visual_cursor().saturating_sub(scroll)) as u16
        };
        let cursor_y = if app.settings().ui.show_borders {
            area.y + 1
        } else {
            area.y
        };

        if !app.is_app_focused() {
            let buf = frame.buffer_mut();
            buf[(cursor_x, cursor_y)].set_style(Style::default().add_modifier(Modifier::REVERSED));
        }
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

// NOTE: review this
pub fn render_raw_side_panel(
    app: &App,
    frame: &mut Frame,
    raw_area: Rect,
    paragraph_y_offset: usize,
    search_query: &str,
    layout: &PreparedLayout,
    theme: &ThemeConfig,
) {
    let raw_view = app.settings().raw_side.view;
    if raw_view == RawSideView::Off || raw_area.width == 0 || raw_area.height == 0 {
        return;
    }

    let raw_display_mode = raw_view_to_display_mode(raw_view);
    let text_display_mode = app.settings().line_mode.display_mode;

    let borders = match app.settings().raw_side.orientation {
        RawSideOrientation::Right => Borders::LEFT,
        RawSideOrientation::Bottom => Borders::TOP,
    };

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));

    let inner_area = block.inner(raw_area);
    frame.render_widget(block, raw_area);

    let view_height = inner_area.height as usize;
    let visible_visual_start = paragraph_y_offset;
    let visible_visual_end = visible_visual_start + view_height;

    let copy_range = app.line_mode.copy_state().normalized_selection();
    let history = app.line_mode.history();
    let is_search_active = !search_query.is_empty();
    let q_lower = search_query.to_lowercase();
    let query_len = search_query.chars().count();

    let normal_match_style = Style::default().bg(Color::Yellow).fg(Color::Black);
    let active_match_style = Style::default()
        .bg(Color::Rgb(255, 140, 0))
        .fg(Color::Black);

    let mut visible_raw_lines: Vec<Line<'static>> = Vec::with_capacity(view_height);
    let mut current_visual_row = 0;

    for (logical_idx, entry_layout) in layout.line_layouts.iter().enumerate() {
        let visual_sub_count = entry_layout.sub_lines.len();
        let entry_visual_end = current_visual_row + visual_sub_count;

        if entry_visual_end <= visible_visual_start {
            current_visual_row = entry_visual_end;
            continue;
        }
        if current_visual_row >= visible_visual_end {
            break;
        }

        if let Some(entry) = history.get(logical_idx) {
            let display_text = displayable_line_string(&entry.text, text_display_mode);
            let entry_chars: Vec<char> = display_text.chars().collect();
            let mut char_offset = 0;

            let sel_range = copy_range.and_then(|(start, end)| {
                if logical_idx < start.line_idx || logical_idx > end.line_idx {
                    None
                } else {
                    let from = if logical_idx == start.line_idx {
                        start.char_idx
                    } else {
                        0
                    };
                    let to = if logical_idx == end.line_idx {
                        end.char_idx
                    } else {
                        usize::MAX
                    };
                    Some((from, to))
                }
            });

            let active_occ = if is_search_active {
                app.line_mode
                    .search()
                    .active_occurrence_for_line(logical_idx)
            } else {
                None
            };

            // search matches
            let mut line_matches = Vec::new();
            if is_search_active && query_len > 0 {
                let text_lower = display_text.to_lowercase();
                for (occ_idx, (byte_idx, _)) in text_lower.match_indices(&q_lower).enumerate() {
                    let m_start = display_text[..byte_idx].chars().count();
                    let m_end = m_start + query_len;
                    let is_active = active_occ == Some(occ_idx);
                    line_matches.push((m_start, m_end, is_active));
                }
            }

            for (sub_idx, sub) in entry_layout.sub_lines.iter().enumerate() {
                let line_y = current_visual_row + sub_idx;
                let is_last_sub = sub_idx == visual_sub_count.saturating_sub(1);

                // Take all remaining chars on the last sub-line
                let sub_char_end = if is_last_sub {
                    entry_chars.len()
                } else {
                    (char_offset + sub.content_char_len).min(entry_chars.len())
                };

                let sub_slice: String = if char_offset < entry_chars.len() {
                    entry_chars[char_offset..sub_char_end].iter().collect()
                } else {
                    String::new()
                };

                let current_sub_chars = sub_slice.chars().count();

                if line_y >= visible_visual_start && line_y < visible_visual_end {
                    if sub_slice.is_empty() {
                        visible_raw_lines.push(Line::from(""));
                    } else {
                        let mut spans = format_line_content(&sub_slice, raw_display_mode);

                        let mut char_to_byte_idx = Vec::with_capacity(current_sub_chars + 1);
                        for (b_idx, _) in sub_slice.char_indices() {
                            char_to_byte_idx.push(b_idx);
                        }
                        char_to_byte_idx.push(sub_slice.len());

                        // 1. Search Highlights
                        for &(m_start, m_end, is_active) in &line_matches {
                            if m_end > char_offset && m_start < sub_char_end {
                                let local_char_start = m_start.saturating_sub(char_offset);
                                let local_char_end =
                                    (m_end.saturating_sub(char_offset)).min(current_sub_chars);

                                if local_char_start < local_char_end {
                                    let byte_start = char_to_byte_idx[local_char_start];
                                    let byte_end = char_to_byte_idx[local_char_end];

                                    if byte_start < byte_end {
                                        let style = if is_active {
                                            active_match_style
                                        } else {
                                            normal_match_style
                                        };
                                        highlight_raw_byte_range(
                                            &mut spans,
                                            byte_start,
                                            byte_end - 1,
                                            style,
                                        );
                                    }
                                }
                            }
                        }

                        // 2. Selection Highlights
                        if let Some((sel_start, sel_end)) = sel_range
                            && sel_end >= char_offset
                            && sel_start < sub_char_end
                        {
                            let local_char_start = sel_start.saturating_sub(char_offset);
                            let local_char_end = (sel_end.saturating_sub(char_offset))
                                .min(current_sub_chars.saturating_sub(1));

                            if local_char_start <= local_char_end
                                && local_char_end < current_sub_chars
                            {
                                let byte_start = char_to_byte_idx[local_char_start];
                                let byte_end = char_to_byte_idx[local_char_end + 1];

                                if byte_start < byte_end && !spans.is_empty() {
                                    highlight_raw_byte_range(
                                        &mut spans,
                                        byte_start,
                                        byte_end - 1,
                                        *theme.selection,
                                    );
                                }
                            }
                        }

                        visible_raw_lines.push(Line::from(spans));
                    }
                }

                char_offset = sub_char_end;
            }
        }

        current_visual_row = entry_visual_end;
    }

    if app.settings().line_mode.display_partial_rx
        && !app.line_mode.partial_rx().is_empty()
        && visible_raw_lines.len() < view_height
    {
        let partial_raw = String::from_utf8_lossy(app.line_mode.partial_rx());
        let spans = format_line_content(&partial_raw, raw_display_mode);
        visible_raw_lines.push(Line::from(spans));
    }

    let raw_pan_x = app.line_mode.raw_pan_x();
    let raw_paragraph =
        Paragraph::new(visible_raw_lines).scroll((0, raw_pan_x.min(u16::MAX as usize) as u16));

    frame.render_widget(raw_paragraph, inner_area);
}
