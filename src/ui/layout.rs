use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    app::{App, types::FocusTarget},
    configs::{
        InputMode, SettingsConfig, ThemeConfig, WrapMode,
        settings::{PrefixWrapBehavior, RawSideOrientation, RawSideView},
    },
};

pub struct ScreenLayout {
    pub monitor_outer: Rect,
    pub content_area: Rect,
    pub scrollbar_area: Option<Rect>,
    pub raw_side_area: Option<Rect>,
    pub timestamp_gutter: Option<Rect>,
    pub prompt_area: Option<Rect>,
    pub status_bar_area: Option<Rect>,
    pub focus_target: FocusTarget,
}

impl ScreenLayout {
    pub fn compute(screen: Rect, app: &App) -> Self {
        let ui = &app.settings().ui;
        let show_borders = ui.show_borders;
        let show_status_bar = ui.show_status_bar;
        let show_scrollbar = ui.show_scrollbar;
        let input_mode = app.settings().input_mode;

        let status_height = if show_status_bar { 1 } else { 0 };
        let prompt_height = match input_mode {
            InputMode::Terminal => 0,
            InputMode::Line => {
                if show_borders {
                    3
                } else {
                    1
                }
            }
        };

        let v_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(prompt_height),
                Constraint::Length(status_height),
            ])
            .split(screen);

        let monitor_outer = v_chunks[0];
        let prompt_area = if prompt_height > 0 {
            Some(v_chunks[1])
        } else {
            None
        };
        let status_bar_area = if show_status_bar {
            Some(v_chunks[2])
        } else {
            None
        };

        let focus_target = match input_mode {
            InputMode::Terminal => FocusTarget::Terminal,
            InputMode::Line => FocusTarget::LineHistory,
        };

        let inner_monitor = if show_borders {
            Rect {
                x: monitor_outer.x.saturating_add(1),
                y: monitor_outer.y.saturating_add(1),
                width: monitor_outer.width.saturating_sub(2),
                height: monitor_outer.height.saturating_sub(2),
            }
        } else {
            monitor_outer
        };

        let (gutter_area, remaining_after_gutter) = match input_mode {
            InputMode::Terminal if app.settings().display_live_timestamp => {
                let sample_ts = chrono::Local::now();
                let ts_width = sample_ts
                    .format(&app.settings().timestamp_format)
                    .to_string()
                    .chars()
                    .count() as u16;

                let gutter_w = ts_width.saturating_add(1).min(inner_monitor.width);
                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(gutter_w), Constraint::Min(0)])
                    .split(inner_monitor);
                (Some(chunks[0]), chunks[1])
            }
            _ => (None, inner_monitor),
        };

        let (main_stream_area, raw_side_area) = if app.settings().raw_side.view != RawSideView::Off
        {
            let inspector_percent = app.settings().raw_side.split_percent.clamp(10, 85);
            let main_percent = 100 - app.settings().raw_side.split_percent;
            let orientation = match app.settings().raw_side.orientation {
                RawSideOrientation::Right => Direction::Horizontal,
                RawSideOrientation::Bottom => Direction::Vertical,
            };
            let is_sufficiently_large = match orientation {
                Direction::Horizontal => remaining_after_gutter.width >= 50,
                Direction::Vertical => remaining_after_gutter.height >= 12,
            };

            if is_sufficiently_large {
                let chunks = Layout::default()
                    .direction(orientation)
                    .constraints([
                        Constraint::Percentage(main_percent),
                        Constraint::Percentage(inspector_percent),
                    ])
                    .split(remaining_after_gutter);

                (chunks[0], Some(chunks[1]))
            } else {
                (remaining_after_gutter, None) //  TODO: rendering error : screen too small
            }
        } else {
            (remaining_after_gutter, None)
        };

        let (content_area, scrollbar_area) = if show_scrollbar {
            if show_borders {
                let sb_area = Rect {
                    x: monitor_outer.x + monitor_outer.width.saturating_sub(1),
                    y: monitor_outer.y + 1,
                    width: 1,
                    height: monitor_outer.height.saturating_sub(2),
                };
                (main_stream_area, Some(sb_area))
            } else {
                let content = Rect {
                    x: main_stream_area.x,
                    y: main_stream_area.y,
                    width: main_stream_area.width.saturating_sub(1),
                    height: main_stream_area.height,
                };
                let sb = Rect {
                    x: remaining_after_gutter.x + remaining_after_gutter.width.saturating_sub(1),
                    y: remaining_after_gutter.y,
                    width: 1,
                    height: remaining_after_gutter.height,
                };
                (content, Some(sb))
            }
        } else {
            (main_stream_area, None)
        };

        Self {
            monitor_outer,
            content_area,
            scrollbar_area,
            raw_side_area,
            timestamp_gutter: gutter_area,
            prompt_area,
            status_bar_area,
            focus_target,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VisualSubLine {
    pub spans: Vec<Span<'static>>,
    pub gutter_chars: usize,
    pub content_char_len: usize,
    pub visual_width: usize,
}

#[derive(Debug, Clone)]
pub struct LogicalLineLayout {
    pub visual_row_start: usize,
    pub sub_lines: Vec<VisualSubLine>,
}

pub struct PreparedLayout {
    pub visual_lines: Vec<Line<'static>>,
    pub line_layouts: Vec<LogicalLineLayout>,
    pub max_line_width: usize,
}

struct WordToken {
    parts: Vec<(String, Style)>,
    is_whitespace: bool,
    char_count: usize,
    visual_width: usize,
}

/// Word-wraps a Line (with gutter aware) like ratatui did before
pub fn wrap_line_by_words(
    line: Line<'static>,
    max_width: usize,
    gutter_chars: usize,
) -> Vec<VisualSubLine> {
    if max_width == 0 {
        let total_chars: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
        return vec![VisualSubLine {
            visual_width: line.width(),
            gutter_chars,
            content_char_len: total_chars.saturating_sub(gutter_chars),
            spans: line.spans,
        }];
    }

    let mut tokens: Vec<WordToken> = Vec::new();
    for span in line.spans {
        let style = span.style;
        let mut text = span.content.into_owned();

        while !text.is_empty() {
            let is_whitespace = text.starts_with(char::is_whitespace);
            let mut token_bytes = 0;
            let mut token_chars = 0;
            let mut token_width = 0;

            for (idx, ch) in text.char_indices() {
                if ch.is_whitespace() != is_whitespace {
                    break;
                }
                token_bytes = idx + ch.len_utf8();
                token_chars += 1;
                token_width += UnicodeWidthChar::width(ch).unwrap_or(0);
            }

            let chunk: String = text.drain(..token_bytes).collect();

            if let Some(last) = tokens.last_mut()
                && last.is_whitespace == is_whitespace
            {
                last.parts.push((chunk, style));
                last.char_count += token_chars;
                last.visual_width += token_width;
            } else {
                tokens.push(WordToken {
                    parts: vec![(chunk, style)],
                    is_whitespace,
                    char_count: token_chars,
                    visual_width: token_width,
                });
            }
        }
    }

    let mut sub_lines = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_row_chars = 0;
    let mut current_row_width = 0;

    for token in tokens {
        let available_width = max_width.saturating_sub(current_row_width);

        if !token.is_whitespace && token.visual_width > available_width && current_row_width > 0 {
            let is_first = sub_lines.is_empty();
            let g_chars = if is_first {
                gutter_chars.min(current_row_chars)
            } else {
                0
            };

            sub_lines.push(VisualSubLine {
                spans: std::mem::take(&mut current_spans),
                gutter_chars: g_chars,
                content_char_len: current_row_chars.saturating_sub(g_chars),
                visual_width: current_row_width,
            });
            current_row_chars = 0;
            current_row_width = 0;
        }

        if token.visual_width > max_width && current_row_width == 0 {
            for (text_part, style) in token.parts {
                let mut remaining = text_part;
                while !remaining.is_empty() {
                    let avail = max_width.saturating_sub(current_row_width);
                    let mut chunk_bytes = 0;
                    let mut chunk_chars = 0;
                    let mut chunk_w = 0;
                    for (idx, ch) in remaining.char_indices() {
                        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
                        if chunk_w + cw > avail && chunk_w > 0 {
                            break;
                        }
                        chunk_bytes = idx + ch.len_utf8();
                        chunk_chars += 1;
                        chunk_w += cw;
                    }

                    if chunk_chars == 0 {
                        let is_first = sub_lines.is_empty();
                        let g_chars = if is_first {
                            gutter_chars.min(current_row_chars)
                        } else {
                            0
                        };
                        sub_lines.push(VisualSubLine {
                            spans: std::mem::take(&mut current_spans),
                            gutter_chars: g_chars,
                            content_char_len: current_row_chars.saturating_sub(g_chars),
                            visual_width: current_row_width,
                        });
                        current_row_chars = 0;
                        current_row_width = 0;
                        continue;
                    }

                    let part: String = remaining.drain(..chunk_bytes).collect();
                    current_row_chars += chunk_chars;
                    current_row_width += chunk_w;
                    current_spans.push(Span::styled(part, style));
                }
            }
            continue;
        }

        current_row_chars += token.char_count;
        current_row_width += token.visual_width;
        for (text_part, style) in token.parts {
            current_spans.push(Span::styled(text_part, style));
        }
    }

    if !current_spans.is_empty() {
        let is_first = sub_lines.is_empty();
        let g_chars = if is_first {
            gutter_chars.min(current_row_chars)
        } else {
            0
        };

        sub_lines.push(VisualSubLine {
            spans: current_spans,
            gutter_chars: g_chars,
            content_char_len: current_row_chars.saturating_sub(g_chars),
            visual_width: current_row_width,
        });
    }

    if sub_lines.is_empty() {
        sub_lines.push(VisualSubLine {
            spans: Vec::new(),
            gutter_chars: 0,
            content_char_len: 0,
            visual_width: 0,
        });
    }

    sub_lines
}

pub fn wrap_line_by_chars(
    line: Line<'static>,
    max_width: usize,
    gutter_chars: usize,
) -> Vec<VisualSubLine> {
    if max_width == 0 {
        let total_chars: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
        return vec![VisualSubLine {
            visual_width: line.width(),
            gutter_chars,
            content_char_len: total_chars.saturating_sub(gutter_chars),
            spans: line.spans,
        }];
    }

    let mut sub_lines = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_row_chars = 0;
    let mut current_row_width = 0;

    for span in line.spans {
        let style = span.style;
        let mut text = span.content.into_owned();

        while !text.is_empty() {
            let available_width = max_width.saturating_sub(current_row_width);

            if available_width == 0 {
                let is_first = sub_lines.is_empty();
                let g_chars = if is_first {
                    gutter_chars.min(current_row_chars)
                } else {
                    0
                };

                sub_lines.push(VisualSubLine {
                    spans: std::mem::take(&mut current_spans),
                    gutter_chars: g_chars,
                    content_char_len: current_row_chars.saturating_sub(g_chars),
                    visual_width: current_row_width,
                });
                current_row_chars = 0;
                current_row_width = 0;
                continue;
            }

            let mut take_bytes = 0;
            let mut take_chars = 0;
            let mut take_width = 0;

            for (idx, ch) in text.char_indices() {
                let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
                if take_width + cw > available_width {
                    break;
                }
                take_width += cw;
                take_chars += 1;
                take_bytes = idx + ch.len_utf8();
            }

            if take_chars == 0 {
                if current_row_width > 0 {
                    let is_first = sub_lines.is_empty();
                    let g_chars = if is_first {
                        gutter_chars.min(current_row_chars)
                    } else {
                        0
                    };

                    sub_lines.push(VisualSubLine {
                        spans: std::mem::take(&mut current_spans),
                        gutter_chars: g_chars,
                        content_char_len: current_row_chars.saturating_sub(g_chars),
                        visual_width: current_row_width,
                    });
                    current_row_chars = 0;
                    current_row_width = 0;
                    continue;
                } else {
                    let ch = text.chars().next().unwrap();
                    take_bytes = ch.len_utf8();
                    take_chars = 1;
                    take_width = UnicodeWidthChar::width(ch).unwrap_or(0);
                }
            }

            let chunk: String = text.drain(..take_bytes).collect();
            current_row_chars += take_chars;
            current_row_width += take_width;
            current_spans.push(Span::styled(chunk, style));
        }
    }

    if !current_spans.is_empty() {
        let is_first = sub_lines.is_empty();
        let g_chars = if is_first {
            gutter_chars.min(current_row_chars)
        } else {
            0
        };

        sub_lines.push(VisualSubLine {
            spans: current_spans,
            gutter_chars: g_chars,
            content_char_len: current_row_chars.saturating_sub(g_chars),
            visual_width: current_row_width,
        });
    }

    if sub_lines.is_empty() {
        sub_lines.push(VisualSubLine {
            spans: Vec::new(),
            gutter_chars: 0,
            content_char_len: 0,
            visual_width: 0,
        });
    }

    sub_lines
}

pub fn prepare_view_layout(
    lines: Vec<Line<'static>>,
    settings: &SettingsConfig,
    theme: &ThemeConfig,
    content_width: usize,
) -> PreparedLayout {
    let mut visual_lines = Vec::new();
    let mut line_layouts = Vec::with_capacity(lines.len());
    let mut max_line_width = 0;

    let wrap_mode = settings.line_mode.wrap_mode;
    let gutter_chars = settings.line_gutter_width();

    let indent_prefix: &str = match settings.line_mode.wrap_indent {
        PrefixWrapBehavior::None => "",
        PrefixWrapBehavior::Prefix => &settings.line_mode.line_indent_prefix,
    };
    let indent_width = indent_prefix.width();
    let indent_style = *theme.line_indent_prefix;

    for line in lines {
        let visual_row_start = visual_lines.len();

        if content_width > 0 && matches!(wrap_mode, WrapMode::Word | WrapMode::Char) {
            let subs = match wrap_mode {
                WrapMode::Word => wrap_line_by_words(line, content_width, gutter_chars),
                WrapMode::Char => wrap_line_by_chars(line, content_width, gutter_chars),
                _ => unreachable!(),
            };
            for (sub_idx, sub) in subs.iter().enumerate() {
                let mut row_spans = Vec::new();
                let extra_w = if sub_idx > 0 && indent_width > 0 {
                    row_spans.push(Span::styled(indent_prefix.to_string(), indent_style));
                    indent_width
                } else {
                    0
                };
                row_spans.extend(sub.spans.clone());
                let total_row_w = sub.visual_width + extra_w;
                if total_row_w > max_line_width {
                    max_line_width = total_row_w;
                }
                visual_lines.push(Line::from(row_spans));
            }
            line_layouts.push(LogicalLineLayout {
                visual_row_start,
                sub_lines: subs,
            });
        } else {
            let total_chars: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
            let visual_width = line.width();
            if visual_width > max_line_width {
                max_line_width = visual_width;
            }
            visual_lines.push(line.clone());
            line_layouts.push(LogicalLineLayout {
                visual_row_start,
                sub_lines: vec![VisualSubLine {
                    spans: line.spans,
                    gutter_chars,
                    content_char_len: total_chars.saturating_sub(gutter_chars),
                    visual_width,
                }],
            });
        }
    }

    PreparedLayout {
        visual_lines,
        line_layouts,
        max_line_width,
    }
}
