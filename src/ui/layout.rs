use std::borrow::Cow;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    text::{Line, Span},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    app::{App, types::FocusTarget},
    configs::{
        Mode, RawSideView, SettingsConfig, StatusBarLayout, ThemeConfig, WrapMode,
        settings::{PrefixWrapBehavior, RawSideOrientation},
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutWarning {
    SidePanelTooSmall {
        required: u16,
        actual: u16,
        orientation: RawSideOrientation,
    },
    WindowTooSmall {
        min_cols: u16,
        min_rows: u16,
        actual_cols: u16,
        actual_rows: u16,
    },
}

pub struct ScreenLayout {
    pub monitor_outer: Rect,
    pub content_area: Rect,
    pub scrollbar_area: Option<Rect>,
    pub raw_side_area: Option<Rect>,
    pub timestamp_gutter: Option<Rect>,
    pub prompt_area: Option<Rect>,
    pub status_bar_area: Option<Rect>,
    pub focus_target: FocusTarget,
    pub warnings: Vec<LayoutWarning>,
}

impl ScreenLayout {
    pub fn compute(screen: Rect, app: &App) -> Self {
        let mut warnings = Vec::new();
        let ui = &app.settings().ui;
        let show_borders = ui.show_borders;
        let show_status_bar = ui.show_status_bar;
        let show_scrollbar = ui.show_scrollbar;
        let input_mode = app.settings().mode;

        // in the border layout, the status bar takes no row of its own (MAX-135)
        let status_in_border = show_status_bar && ui.status_bar_layout() == StatusBarLayout::Border;
        let status_height = if !show_status_bar || status_in_border {
            0
        } else if ui.status_bar_layout() == StatusBarLayout::Box {
            3
        } else {
            1
        };
        let prompt_height = match input_mode {
            Mode::Terminal => 0,
            Mode::Line => {
                if show_borders {
                    3
                } else {
                    1
                }
            }
        };

        if screen.width < 30 || screen.height < 8 {
            warnings.push(LayoutWarning::WindowTooSmall {
                min_cols: 30,
                min_rows: 8,
                actual_cols: screen.width,
                actual_rows: screen.height,
            });
        }

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
        let status_bar_area = if status_in_border {
            // the last box's bottom border, inside its `└─` and `─┘`; in terminal mode it's the
            // history's, which the search bar takes while it's open
            let last_box = prompt_area.unwrap_or(monitor_outer);
            let searching = prompt_area.is_none() && app.is_search_active();
            (!searching).then(|| {
                Rect::new(
                    last_box.x.saturating_add(2),
                    last_box.bottom().saturating_sub(1),
                    last_box.width.saturating_sub(4),
                    1,
                )
            })
        } else if show_status_bar {
            Some(v_chunks[2])
        } else {
            None
        };

        let focus_target = match input_mode {
            Mode::Terminal => FocusTarget::Terminal,
            Mode::Line => FocusTarget::LineHistory,
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
            Mode::Terminal if app.settings().show_timestamps => {
                let ts_width = app.settings().timestamp_width() as u16;

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
            let (is_sufficiently_large, required, actual) = match orientation {
                Direction::Horizontal => {
                    let req = 50;
                    (
                        remaining_after_gutter.width >= req,
                        req,
                        remaining_after_gutter.width,
                    )
                }
                Direction::Vertical => {
                    let req = 12;
                    (
                        remaining_after_gutter.height >= req,
                        req,
                        remaining_after_gutter.height,
                    )
                }
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
                warnings.push(LayoutWarning::SidePanelTooSmall {
                    required,
                    actual,
                    orientation: app.settings().raw_side.orientation,
                });
                (remaining_after_gutter, None)
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
            warnings,
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

impl PreparedLayout {
    /// One layout from layouts made separately, in order: the row starts follow on (#5a)
    pub fn concat(parts: impl IntoIterator<Item = PreparedLayout>) -> Self {
        let mut joined = PreparedLayout {
            visual_lines: Vec::new(),
            line_layouts: Vec::new(),
            max_line_width: 0,
        };
        for part in parts {
            let row_offset = joined.visual_lines.len();
            joined.visual_lines.extend(part.visual_lines);
            joined
                .line_layouts
                .extend(part.line_layouts.into_iter().map(|mut line| {
                    line.visual_row_start += row_offset;
                    line
                }));
            joined.max_line_width = joined.max_line_width.max(part.max_line_width);
        }
        joined
    }
}

struct WordToken {
    part_count: usize,
    is_whitespace: bool,
    char_count: usize,
    visual_width: usize,
}

/// The bytes `range` of `span`'s text as a span with the same style. Borrowed text (`'static`,
/// like the byte digits and separators of the byte display modes) is sliced, only owned text
/// is copied (#5c)
fn sub_span(span: &Span<'static>, range: std::ops::Range<usize>) -> Span<'static> {
    let content: Cow<'static, str> = match &span.content {
        Cow::Borrowed(text) => {
            let text: &'static str = text;
            Cow::Borrowed(&text[range])
        }
        Cow::Owned(text) => Cow::Owned(text[range].to_string()),
    };
    Span::styled(content, span.style)
}

/// Appends `part` to the token parts: to the last token when it is of the same kind (a word
/// continued in the next span), or as a new token
fn push_token_part(
    tokens: &mut Vec<WordToken>,
    parts: &mut Vec<Span<'static>>,
    part: Span<'static>,
    is_whitespace: bool,
    char_count: usize,
    visual_width: usize,
) {
    parts.push(part);
    match tokens.last_mut() {
        Some(last) if last.is_whitespace == is_whitespace => {
            last.part_count += 1;
            last.char_count += char_count;
            last.visual_width += visual_width;
        }
        _ => tokens.push(WordToken {
            part_count: 1,
            is_whitespace,
            char_count,
            visual_width,
        }),
    }
}

/// Word-wraps a Line (with gutter aware) like ratatui did before.
/// Rows after the first get `indent_width` less room, the caller prepends the wrap indent to them
pub fn wrap_line_by_words(
    line: Line<'static>,
    max_width: usize,
    gutter_chars: usize,
    indent_width: usize,
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

    // ---- Tokens: runs of whitespace and of non-whitespace, in line order ----
    //
    // Spans are moved into `parts` whole when they are one run (a byte's digits, a separator,
    // a word): no copy. A span mixing runs ("hello world") is cut into one sub-span per run,
    // sliced when its text is borrowed, copied only when owned (#5c). Consecutive runs of the
    // same kind (a word continued in the next span) make one token
    let mut tokens: Vec<WordToken> = Vec::new();
    let mut parts: Vec<Span<'static>> = Vec::with_capacity(line.spans.len());
    for span in line.spans {
        if span.content.is_empty() {
            continue;
        }

        // one pass to see if the whole span is a single run, counting it on the way
        let text: &str = &span.content;
        let is_whitespace = text.starts_with(char::is_whitespace);
        let mut single_run = true;
        let mut char_count = 0;
        let mut visual_width = 0;
        for ch in text.chars() {
            if ch.is_whitespace() != is_whitespace {
                single_run = false;
                break;
            }
            char_count += 1;
            visual_width += UnicodeWidthChar::width(ch).unwrap_or(0);
        }
        if single_run {
            push_token_part(
                &mut tokens,
                &mut parts,
                span,
                is_whitespace,
                char_count,
                visual_width,
            );
            continue;
        }

        // several runs: one sub-span each
        let mut run_start = 0;
        while run_start < text.len() {
            let rest = &text[run_start..];
            let is_whitespace = rest.starts_with(char::is_whitespace);
            let mut run_end = run_start;
            let mut char_count = 0;
            let mut visual_width = 0;
            for (idx, ch) in rest.char_indices() {
                if ch.is_whitespace() != is_whitespace {
                    break;
                }
                run_end = run_start + idx + ch.len_utf8();
                char_count += 1;
                visual_width += UnicodeWidthChar::width(ch).unwrap_or(0);
            }
            let part = sub_span(&span, run_start..run_end);
            push_token_part(
                &mut tokens,
                &mut parts,
                part,
                is_whitespace,
                char_count,
                visual_width,
            );
            run_start = run_end;
        }
    }
    // the parts are taken in order, `part_count` per token
    let mut parts = parts.into_iter();

    let mut sub_lines = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_row_chars = 0;
    let mut current_row_width = 0;

    // at least 1 so wrapping always progresses, even with an indent as wide as the view
    let continuation_width = max_width.saturating_sub(indent_width).max(1);

    for token in tokens {
        let row_width = if sub_lines.is_empty() {
            max_width
        } else {
            continuation_width
        };
        let available_width = row_width.saturating_sub(current_row_width);

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

        let row_width = if sub_lines.is_empty() {
            max_width
        } else {
            continuation_width
        };
        if token.visual_width > row_width && current_row_width == 0 {
            // a word wider than a whole row: cut it char by char over as many rows as needed
            for part in parts.by_ref().take(token.part_count) {
                let text: &str = &part.content;
                let mut pos = 0;
                while pos < text.len() {
                    let remaining = &text[pos..];
                    let row_width = if sub_lines.is_empty() {
                        max_width
                    } else {
                        continuation_width
                    };
                    let avail = row_width.saturating_sub(current_row_width);
                    let mut chunk_bytes = 0;
                    let mut chunk_chars = 0;
                    let mut chunk_w = 0;
                    for (idx, ch) in remaining.char_indices() {
                        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
                        if chunk_w + cw > avail && (chunk_w > 0 || current_row_width > 0) {
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

                    current_spans.push(sub_span(&part, pos..pos + chunk_bytes));
                    pos += chunk_bytes;
                    current_row_chars += chunk_chars;
                    current_row_width += chunk_w;
                }
            }
            continue;
        }

        current_row_chars += token.char_count;
        current_row_width += token.visual_width;
        current_spans.extend(parts.by_ref().take(token.part_count));
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

/// Char-wraps a Line (with gutter aware).
/// Rows after the first get `indent_width` less room, the caller prepends the wrap indent to them
pub fn wrap_line_by_chars(
    line: Line<'static>,
    max_width: usize,
    gutter_chars: usize,
    indent_width: usize,
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

    // at least 1 so wrapping always progresses, even with an indent as wide as the view
    let continuation_width = max_width.saturating_sub(indent_width).max(1);

    for span in line.spans {
        let style = span.style;
        let mut text = span.content.into_owned();

        while !text.is_empty() {
            let row_width = if sub_lines.is_empty() {
                max_width
            } else {
                continuation_width
            };
            let available_width = row_width.saturating_sub(current_row_width);

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

/// What starts the continuation rows of a wrapped line (`wrap_indent`): nothing, the indent
/// prefix, or the indent prefix after blanks as wide as the timestamp
pub fn continuation_indent(settings: &SettingsConfig) -> String {
    let base_prefix = match settings.line_mode.wrap_indent {
        PrefixWrapBehavior::None => "",
        PrefixWrapBehavior::Prefix | PrefixWrapBehavior::Timestamp => {
            &settings.line_mode.line_indent_prefix
        }
    };

    let timestamp_pad = if settings.line_mode.wrap_indent == PrefixWrapBehavior::Timestamp
        && settings.show_timestamps
    {
        let prefix_w = settings.line_mode.line_prefix.width();
        let ts_width = settings.line_gutter_width().saturating_sub(prefix_w);
        " ".repeat(ts_width)
    } else {
        String::new()
    };

    if timestamp_pad.is_empty() {
        base_prefix.to_string()
    } else {
        format!("{}{}", timestamp_pad, base_prefix)
    }
}

/// Rows a byte mode line (Hex, Octal, Binary) takes once word wrapped, counted without laying
/// it out (opti #5d). `None` when the count could be wrong, so the caller lays the line out instead.
///
/// It follows the placement rule of `wrap_line_by_words` for a line made of a gutter then
/// `bytes` tokens of `digits` columns separated by one space:
/// - the gutter (timestamp + prefix) opens the first row; it must fit in it and end with a
///   space, which the caller checks (a prefix ending otherwise merges with the first byte)
/// - a byte goes to a new row when it doesn't fit in what is left and the row isn't empty
/// - the separator after a byte always stays on its row, even one column past the width
/// - continuation rows are narrower by the indent; a byte wider than them is cut char by
///   char by the word wrap, a case not counted here
pub fn byte_mode_rows(
    bytes: usize,
    digits: usize,
    max_width: usize,
    gutter_width: usize,
    indent_width: usize,
) -> Option<usize> {
    if max_width == 0 {
        return Some(1);
    }
    let continuation_width = max_width.saturating_sub(indent_width).max(1);
    if gutter_width > max_width || digits > continuation_width {
        return None;
    }
    let mut rows = 1;
    let mut row_width = max_width;
    let mut used = gutter_width;
    for byte in 0..bytes {
        if digits > row_width.saturating_sub(used) && used > 0 {
            rows += 1;
            row_width = continuation_width;
            used = 0;
        }
        used += digits;
        if byte + 1 < bytes {
            used += 1;
        }
    }
    Some(rows)
}

pub fn prepare_view_layout(
    lines: Vec<Line<'static>>,
    settings: &SettingsConfig,
    theme: &ThemeConfig,
    content_width: usize,
) -> PreparedLayout {
    let mut visual_lines = Vec::with_capacity(lines.len());
    let mut line_layouts = Vec::with_capacity(lines.len());
    let mut max_line_width = 0;

    let wrap_mode = settings.line_mode.wrap_mode;
    let gutter_chars = settings.line_gutter_width();

    let full_indent_prefix = continuation_indent(settings);
    let indent_width = full_indent_prefix.width();
    let indent_style = *theme.line_mode.line_indent_prefix;

    for line in lines {
        let visual_row_start = visual_lines.len();

        if content_width > 0 && matches!(wrap_mode, WrapMode::Word | WrapMode::Char) {
            let subs = match wrap_mode {
                WrapMode::Word => {
                    wrap_line_by_words(line, content_width, gutter_chars, indent_width)
                }
                WrapMode::Char => {
                    wrap_line_by_chars(line, content_width, gutter_chars, indent_width)
                }
                _ => unreachable!(),
            };
            for (sub_idx, sub) in subs.iter().enumerate() {
                let mut row_spans = Vec::with_capacity(sub.spans.len() + 1);
                let extra_w = if sub_idx > 0 && indent_width > 0 {
                    row_spans.push(Span::styled(full_indent_prefix.clone(), indent_style));
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
            let spans = line.spans;
            visual_lines.push(Line::from(spans.clone()));
            line_layouts.push(LogicalLineLayout {
                visual_row_start,
                sub_lines: vec![VisualSubLine {
                    spans,
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

#[cfg(test)]
mod tests {
    //! Word and char wrapping of styled lines into visual sub-lines.

    use super::*;

    type WrapFn = fn(Line<'static>, usize, usize, usize) -> Vec<VisualSubLine>;
    const WRAPPERS: [(&str, WrapFn); 2] =
        [("words", wrap_line_by_words), ("chars", wrap_line_by_chars)];

    fn line(parts: &[&str]) -> Line<'static> {
        Line::from(
            parts
                .iter()
                .map(|p| Span::raw(p.to_string()))
                .collect::<Vec<_>>(),
        )
    }

    fn sub_text(sub: &VisualSubLine) -> String {
        sub.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// Wrapping never loses text, char counts add up, and the gutter is only on the first row.
    /// How: both wrappers over several samples.
    #[test]
    fn wrap_preserves_text_and_char_counts() {
        let samples: [&[&str]; 4] = [
            &["│ ", "hello world foo bar"],
            &["│ ", "abcdefghijklmnopqrstuvwxyz"],
            &["│ ", "héllo ✓ wörld"],
            &["│ ", ""],
        ];
        for (name, wrap) in WRAPPERS {
            for parts in samples {
                let full: String = parts.concat();
                let subs = wrap(line(parts), 8, 2, 0);
                let joined: String = subs.iter().map(sub_text).collect();
                assert_eq!(joined, full, "{name} on {parts:?}");

                let counted: usize = subs
                    .iter()
                    .map(|s| s.gutter_chars + s.content_char_len)
                    .sum();
                assert_eq!(counted, full.chars().count(), "{name} on {parts:?}");

                assert!(subs.iter().skip(1).all(|s| s.gutter_chars == 0), "{name}");
            }
        }
    }

    /// No row is wider than max_width, long words included (input without space runs).
    /// How: checks visual_width on every row.
    #[test]
    fn wrap_respects_width_without_whitespace_runs() {
        for (name, wrap) in WRAPPERS {
            let subs = wrap(line(&["│ ", "hello world foo bar abcdefghijkl"]), 8, 2, 0);
            for sub in &subs {
                assert!(
                    sub.visual_width <= 8,
                    "{name}: {:?} is too wide",
                    sub_text(sub)
                );
            }
        }
    }

    /// Word wrap breaks after the space; char wrap splits at the width.
    /// How: exact rows for short inputs.
    #[test]
    fn wrap_breaks_at_words_and_chars() {
        let subs = wrap_line_by_words(line(&["hello world"]), 8, 0, 0);
        let texts: Vec<String> = subs.iter().map(sub_text).collect();
        assert_eq!(texts, ["hello ", "world"]);

        let subs = wrap_line_by_chars(line(&["abcdefghij"]), 4, 0, 0);
        let texts: Vec<String> = subs.iter().map(sub_text).collect();
        assert_eq!(texts, ["abcd", "efgh", "ij"]);
    }

    /// A word longer than the width is split over several rows (#26).
    /// How: a 10-char word at width 4, then the same after a short word and a gutter.
    #[test]
    fn word_wrap_splits_long_word() {
        let subs = wrap_line_by_words(line(&["abcdefghij"]), 4, 0, 0);
        let texts: Vec<String> = subs.iter().map(sub_text).collect();
        assert_eq!(texts, ["abcd", "efgh", "ij"]);

        let subs = wrap_line_by_words(line(&["│ ", "hi abcdefghij"]), 6, 2, 0);
        let texts: Vec<String> = subs.iter().map(sub_text).collect();
        assert_eq!(texts, ["│ hi ", "abcdef", "ghij"]);
        assert_eq!(subs[0].gutter_chars, 2);
        assert_eq!(subs[0].content_char_len, 3);
    }

    /// A long word with wide chars breaks before a char that would overflow the row.
    /// How: "日本語日本" (width 10) at width 5 gives rows of width 4, 4, 2.
    #[test]
    fn word_wrap_splits_long_word_with_wide_chars() {
        let subs = wrap_line_by_words(line(&["日本語日本"]), 5, 0, 0);
        let widths: Vec<usize> = subs.iter().map(|s| s.visual_width).collect();
        assert_eq!(widths, [4, 4, 2]);
    }

    /// A single char wider than max_width still lands on its own row (no infinite loop).
    /// How: one double-width char at width 1.
    #[test]
    fn word_wrap_char_wider_than_width() {
        let subs = wrap_line_by_words(line(&["日日"]), 1, 0, 0);
        let texts: Vec<String> = subs.iter().map(sub_text).collect();
        assert_eq!(texts, ["日", "日"]);
    }

    /// Current behavior: a run of spaces can make a row wider than max_width.
    /// How: exact first row and its width.
    #[test]
    fn word_wrap_whitespace_run_can_exceed_width() {
        // FIXME: current behavior: whitespace tokens never trigger a break, so a row can be wider
        // than max_width (harmless on screen, but inflates visual_width / max_line_width)
        let subs = wrap_line_by_words(line(&["aaaa     b"]), 5, 0, 0);
        assert_eq!(sub_text(&subs[0]), "aaaa     ");
        assert_eq!(subs[0].visual_width, 9);
    }

    /// Char wrap counts double-width chars as width 2.
    /// How: CJK text at width 5.
    #[test]
    fn char_wrap_handles_wide_chars() {
        // each '✓' is width 1, each '日' is width 2
        let subs = wrap_line_by_chars(line(&["日本語"]), 5, 0, 0);
        let texts: Vec<String> = subs.iter().map(sub_text).collect();
        assert_eq!(texts, ["日本", "語"]);
        assert_eq!(subs[0].visual_width, 4);
    }

    /// Rows after the first leave room for the wrap indent (#30).
    /// How: a 10-char word at width 6 with a 2-wide indent: 6 chars, then 4 per row.
    #[test]
    fn wrap_continuation_rows_leave_room_for_indent() {
        for (name, wrap) in WRAPPERS {
            let subs = wrap(line(&["abcdefghij"]), 6, 0, 2);
            let texts: Vec<String> = subs.iter().map(sub_text).collect();
            assert_eq!(texts, ["abcdef", "ghij"], "{name}");
        }
    }

    /// An indent as wide as the view still makes progress, one char per row.
    /// How: indent 4 at width 4.
    #[test]
    fn wrap_indent_as_wide_as_view_still_progresses() {
        for (name, wrap) in WRAPPERS {
            let subs = wrap(line(&["abcdef"]), 4, 0, 4);
            let texts: Vec<String> = subs.iter().map(sub_text).collect();
            assert_eq!(texts, ["abcd", "e", "f"], "{name}");
        }
    }

    /// End to end: no visual row (indent included) is wider than the view, and no char is lost.
    /// How: the case seen in the app, a 150-char word in a 59-col view, both wrap modes.
    #[test]
    fn prepared_layout_rows_fit_view_with_indent() {
        let theme = ThemeConfig::default();
        let word = "0123456789".repeat(15);
        for wrap_mode in [WrapMode::Word, WrapMode::Char] {
            let mut settings = SettingsConfig::default();
            settings.line_mode.wrap_mode = wrap_mode;
            let prefix = settings.line_mode.line_prefix.clone();
            let indent = settings.line_mode.line_indent_prefix.clone();

            let input = line(&[prefix.as_str(), word.as_str()]);
            let layout = prepare_view_layout(vec![input], &settings, &theme, 59);

            let mut content = String::new();
            for (row_idx, row) in layout.visual_lines.iter().enumerate() {
                assert!(
                    row.width() <= 59,
                    "{wrap_mode:?} row {row_idx} is {}",
                    row.width()
                );
                let text: String = row.spans.iter().map(|s| s.content.as_ref()).collect();
                let skip = if row_idx == 0 { &prefix } else { &indent };
                content.push_str(text.strip_prefix(skip.as_str()).unwrap());
            }
            assert_eq!(content, word, "{wrap_mode:?}");
        }
    }

    /// Width 0 returns the whole line as a single sub-line.
    /// How: checks gutter and content counts.
    #[test]
    fn zero_width_returns_single_sub_line() {
        for (name, wrap) in WRAPPERS {
            let subs = wrap(line(&["│ ", "hello"]), 0, 2, 0);
            assert_eq!(subs.len(), 1, "{name}");
            assert_eq!(subs[0].gutter_chars, 2, "{name}");
            assert_eq!(subs[0].content_char_len, 5, "{name}");
        }
    }

    /// An empty line still produces one (empty) sub-line.
    /// How: both wrappers on an empty line.
    #[test]
    fn empty_line_gives_one_empty_sub_line() {
        for (name, wrap) in WRAPPERS {
            let subs = wrap(Line::default(), 10, 0, 0);
            assert_eq!(subs.len(), 1, "{name}");
            assert_eq!(subs[0].content_char_len, 0, "{name}");
        }
    }

    /// Lines laid out one at a time then joined give the layout of all of them at once (#5a).
    /// How: wrapped, multi-row and empty lines, word wrap with an indent prefix and timestamps.
    #[test]
    fn concat_of_single_layouts_matches_one_layout() {
        let theme = ThemeConfig::default();
        let mut settings = SettingsConfig::default();
        settings.show_timestamps = true;
        settings.line_mode.wrap_indent = PrefixWrapBehavior::Prefix;
        let lines = [
            line(&["short"]),
            line(&[
                "a much longer line that wraps ",
                "over several rows ",
                "of the view",
            ]),
            line(&[""]),
            line(&["mid ", "sized line"]),
        ];
        let all = prepare_view_layout(lines.to_vec(), &settings, &theme, 16);
        let joined = PreparedLayout::concat(
            lines
                .iter()
                .map(|l| prepare_view_layout(vec![l.clone()], &settings, &theme, 16)),
        );
        assert_eq!(joined.visual_lines, all.visual_lines);
        assert_eq!(joined.max_line_width, all.max_line_width);
        assert_eq!(joined.line_layouts.len(), all.line_layouts.len());
        for (j, a) in joined.line_layouts.iter().zip(&all.line_layouts) {
            assert_eq!(j.visual_row_start, a.visual_row_start);
            let chars = |l: &LogicalLineLayout| -> Vec<usize> {
                l.sub_lines.iter().map(|s| s.content_char_len).collect()
            };
            assert_eq!(chars(j), chars(a));
        }
    }

    /// Every row of a laid out line as `gutter|content chars|width|text` (spaces as `·`).
    fn rows_of(layout: &PreparedLayout) -> Vec<String> {
        layout
            .line_layouts
            .iter()
            .flat_map(|line| &line.sub_lines)
            .map(|sub| {
                let text: String = sub.spans.iter().map(|s| s.content.as_ref()).collect();
                format!(
                    "{}|{}|{}|{}",
                    sub.gutter_chars,
                    sub.content_char_len,
                    sub.visual_width,
                    text.replace(' ', "·")
                )
            })
            .collect()
    }

    /// Golden rows of the word wrap, captured before it stopped copying spans (#5c): byte modes
    /// (2 borrowed spans per byte), a gutter (timestamp + prefix) and an indent prefix, a byte
    /// wider than the row (split char by char), and a Pretty line whose owned spans are split.
    /// How: real entries through `history_entry_to_line` then `prepare_view_layout`.
    #[test]
    fn word_wrap_golden_rows() {
        use crate::{
            app::types::LogEntry,
            configs::{LineDisplayMode, TextEncoding},
            ui::lines::history_entry_to_line,
        };
        let theme = ThemeConfig::default();
        let laid_out = |mode, wire: &[u8], width, timestamps, indent| {
            let mut settings = SettingsConfig::default();
            settings.line_mode.display_mode = mode;
            settings.show_timestamps = timestamps;
            settings.timestamp_format = String::from("%M:%S");
            settings.line_mode.wrap_indent = indent;
            let time = chrono::TimeZone::with_ymd_and_hms(&chrono::Local, 2026, 10, 24, 12, 34, 56)
                .unwrap();
            let entry = LogEntry::received(TextEncoding::Utf8.decode(wire), wire, time);
            let prefix = settings.line_mode.line_prefix.clone();
            let line = history_entry_to_line(&entry, &settings, &[], 0, None, &theme, &prefix);
            rows_of(&prepare_view_layout(vec![line], &settings, &theme, width))
        };
        use LineDisplayMode::{Binary, Hex, Octal, Pretty};
        use PrefixWrapBehavior::{None as NoIndent, Prefix};
        let cases = [
            laid_out(Hex, b"Hello World!", 20, false, NoIndent),
            laid_out(Hex, b"Hello World!", 24, true, Prefix),
            laid_out(Octal, b"abcdefgh", 18, false, Prefix),
            laid_out(Binary, b"AB\r\n", 12, false, NoIndent),
            laid_out(Binary, b"AB", 6, false, NoIndent),
            laid_out(
                Pretty,
                b"hello \x1b[31mwonderful world\x1b[0m, again",
                10,
                false,
                Prefix,
            ),
        ];
        let expected: [&[&str]; 6] = [
            &["2|18|20|│·48·65·6C·6C·6F·20·", "0|17|17|57·6F·72·6C·64·21"],
            // the trailing separator is kept even past the width (ratatui cuts it when drawing)
            &[
                "16|9|25|34:56│·48·65·6C·6C·6F·20·",
                "0|17|17|57·6F·72·6C·64·21",
            ],
            &["2|16|18|│·141·142·143·144·", "0|15|15|145·146·147·150"],
            &[
                "2|9|11|│·01000001·",
                "0|9|9|01000010·",
                "0|9|9|00001101·",
                "0|8|8|00001010",
            ],
            // a byte wider than the row is split char by char
            &[
                "2|0|2|│·",
                "0|6|6|010000",
                "0|3|3|01·",
                "0|6|6|010000",
                "0|2|2|10",
            ],
            // owned spans split: a word wider than a continuation row (10 - 2 indent) is cut
            &[
                "2|6|8|│·hello·",
                "0|8|8|wonderfu",
                "0|9|9|l·world,·",
                "0|5|5|again",
            ],
        ];
        for (rows, expected) in cases.iter().zip(expected) {
            assert_eq!(rows, expected);
        }
    }

    /// The arithmetic row count of byte mode lines equals the rows the word wrap makes, when it
    /// gives a count (#5d).
    /// How: generated lines (0-60 bytes) in Hex / Octal / Binary, with and without timestamps
    /// and an indent prefix, at widths 4-90: count vs `prepare_view_layout` rows.
    #[test]
    fn byte_mode_rows_match_the_word_wrap() {
        use crate::{
            app::types::LogEntry,
            configs::{LineDisplayMode, TextEncoding},
            ui::lines::history_entry_to_line,
        };
        let theme = ThemeConfig::default();
        let mut counted = 0;
        for (mode, digits) in [
            (LineDisplayMode::Hex, 2),
            (LineDisplayMode::Octal, 3),
            (LineDisplayMode::Binary, 8),
        ] {
            for timestamps in [false, true] {
                for indent in [PrefixWrapBehavior::None, PrefixWrapBehavior::Prefix] {
                    let mut settings = SettingsConfig::default();
                    settings.line_mode.display_mode = mode;
                    settings.show_timestamps = timestamps;
                    settings.line_mode.wrap_indent = indent;
                    let prefix = settings.line_mode.line_prefix.clone();
                    let gutter = settings.line_gutter_width();
                    let indent_width = continuation_indent(&settings).width();
                    for len in [0, 1, 2, 5, 13, 31, 60] {
                        let wire: Vec<u8> = (0..len as u8).map(|b| b'A' + b % 26).collect();
                        let entry = LogEntry::received(
                            TextEncoding::Utf8.decode(&wire),
                            &wire,
                            chrono::Local::now(),
                        );
                        let line =
                            history_entry_to_line(&entry, &settings, &[], 0, None, &theme, &prefix);
                        for width in 4..=90 {
                            let Some(rows) =
                                byte_mode_rows(len, digits, width, gutter, indent_width)
                            else {
                                continue;
                            };
                            let layout =
                                prepare_view_layout(vec![line.clone()], &settings, &theme, width);
                            let context = format!(
                                "{mode:?} ts {timestamps} {indent:?} {len} bytes width {width}"
                            );
                            assert_eq!(rows, layout.visual_lines.len(), "{context}");
                            counted += 1;
                        }
                    }
                }
            }
        }
        // most cases are counted, the fallback is only for rows too narrow
        assert!(counted > 3000, "{counted}");
    }
}
