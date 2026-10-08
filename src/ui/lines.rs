use ansi_to_tui::IntoText;
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
        types::{FocusTarget, LogEntry},
    },
    configs::{
        KeywordHighlight, LineDisplayMode, SettingsConfig, ThemeConfig, WrapMode,
        settings::PrefixWrapBehavior,
    },
    ui::{
        PostRenderEffect,
        common::{
            apply_keyword_highlights, apply_search_highlight, apply_selection_to_spans,
            border_style_for, render_search_bar,
        },
        layout::prepare_view_layout,
    },
};

pub fn sanitize_terminal_control_sequences(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.peek().copied() {
                // Keep CSI (styles, controls)
                Some('[') => {
                    out.push(ch);
                }
                // OSC (ESC] to BELL or ESC\)
                Some(']') => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                // DCS, APC, PM (ESC] to ESC\)
                Some('P') | Some('_') | Some('^') => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                // 2-byte escape sequence (DECKPAM `=`, DECKPNM `>`, DECSC `7`, DECRC `8`, RIS `c`, etc.)
                Some(next_ch) if next_ch.is_ascii() && !next_ch.is_ascii_control() => {
                    chars.next();
                }
                _ => {}
            }
        } else {
            out.push(ch);
        }
    }

    out
}

fn trim_trailing_incomplete_ansi(text: &str) -> &str {
    if let Some(esc_idx) = text.rfind('\x1b') {
        let tail = &text[esc_idx..];
        if tail.len() == 1 {
            return &text[..esc_idx];
        }

        if tail.starts_with("\x1b[") {
            let is_finished = tail
                .chars()
                .skip(2)
                .any(|c| c.is_ascii_alphabetic() || ('@'..='~').contains(&c));
            if !is_finished {
                return &text[..esc_idx];
            }
        }
    }
    text
}

pub fn format_line_content(text: &str, mode: LineDisplayMode) -> Vec<Span<'static>> {
    match mode {
        LineDisplayMode::Pretty => {
            let sanitized = sanitize_terminal_control_sequences(text);
            let displayable = trim_trailing_incomplete_ansi(&sanitized);
            if let Ok(parsed) = displayable.as_bytes().into_text()
                && let Some(first_line) = parsed.lines.into_iter().next()
            {
                return first_line.spans;
            }
            vec![Span::raw(displayable.to_string())]
        }
        LineDisplayMode::Clean => {
            let clean_text = strip_ansi_escapes::strip_str(text);
            vec![Span::raw(clean_text)]
        }
        LineDisplayMode::Raw => {
            let mut raw = String::new();
            for ch in text.chars() {
                match ch {
                    '\x1b' => raw.push_str("^["),
                    '\r' => raw.push_str("\\r"),
                    '\t' => raw.push_str("    "),
                    c if c.is_control() => raw.push_str(&format!("\\x{:02X}", c as u32)),
                    c => raw.push(c),
                }
            }
            vec![Span::styled(raw, Style::default().fg(Color::White))]
        }
        LineDisplayMode::Hex => {
            let bytes = text.as_bytes();
            if bytes.is_empty() {
                return vec![Span::raw("")];
            }

            let mut spans = Vec::with_capacity(bytes.len() * 2);

            for (i, &b) in bytes.iter().enumerate() {
                let color = match b {
                    0x00 => Color::DarkGray,                        // \0
                    0x0A | 0x0D => Color::Yellow,                   // \r and \n
                    0xAA | 0x55 | 0x7E => Color::LightRed,          // sync / frame delimiters
                    b if (0x20..=0x7E).contains(&b) => Color::Cyan, // ASCII
                    _ => Color::White,
                };

                spans.push(Span::styled(format!("{b:02X}"), Style::default().fg(color)));

                if i + 1 < bytes.len() {
                    spans.push(Span::raw(" "));
                }
            }

            spans
        }
        LineDisplayMode::Octal => {
            let bytes = text.as_bytes();
            if bytes.is_empty() {
                return vec![Span::raw("")];
            }
            let mut spans = Vec::with_capacity(bytes.len() * 2);
            for (i, &b) in bytes.iter().enumerate() {
                let color = match b {
                    0 => Color::DarkGray,
                    0o12 | 0o15 => Color::Yellow, // \r and \n
                    b if (0x20..=0x7E).contains(&b) => Color::Green,
                    _ => Color::White,
                };
                spans.push(Span::styled(format!("{b:03o}"), Style::default().fg(color)));
                if i + 1 < bytes.len() {
                    spans.push(Span::raw(" "));
                }
            }
            spans
        }
        LineDisplayMode::Binary => {
            let bytes = text.as_bytes();
            if bytes.is_empty() {
                return vec![Span::raw("")];
            }
            let mut spans = Vec::with_capacity(bytes.len() * 2);
            for (i, &b) in bytes.iter().enumerate() {
                let color = if b == 0 {
                    Color::DarkGray
                } else if b == 0xFF {
                    Color::LightRed
                } else {
                    Color::LightBlue
                };
                spans.push(Span::styled(format!("{b:08b}"), Style::default().fg(color)));
                if i + 1 < bytes.len() {
                    spans.push(Span::raw(" "));
                }
            }
            spans
        }
    }
}

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
    scrollbar_area: Option<Rect>,
    theme: &ThemeConfig,
    highlights: &[KeywordHighlight],
) -> PostRenderEffect {
    let content_width = content_area.width as usize;
    let content_height = content_area.height as usize;
    let wrap_mode = app.settings().line_mode.wrap_mode;
    let post_effect = PostRenderEffect::None; // unused for now

    let is_search_active = app.line_mode.search().active;
    let search_query = if is_search_active {
        app.line_mode.search().query_str()
    } else {
        ""
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
                search_query,
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
            let mut remaining_content_chars = cursor_char;

            for (sub_idx, sub) in line_meta.sub_lines.iter().enumerate() {
                let is_last_sub_line = sub_idx == line_meta.sub_lines.len() - 1;

                if remaining_content_chars < sub.content_char_len || is_last_sub_line {
                    resolved_row = line_meta.visual_row_start + sub_idx;

                    let mut gutter_to_skip = sub.gutter_chars;
                    let mut content_to_take = remaining_content_chars;
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

                    accumulated_width += content_to_take;
                    resolved_col = accumulated_width;
                    break;
                }

                remaining_content_chars -= sub.content_char_len;
            }

            // Draw cursor
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
