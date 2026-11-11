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
        settings::{PrefixWrapBehavior, RawSideOrientation, SPLIT_PERCENT_MAX, SPLIT_PERCENT_MIN},
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
    /// The side area when a plot is shown: it takes the raw panel's place (MAX-222)
    pub plot_area: Option<Rect>,
    pub raw_split_area: Option<Rect>,
    pub timestamp_gutter: Option<Rect>,
    pub prompt_area: Option<Rect>,
    pub status_bar_area: Option<Rect>,
    pub focus_target: FocusTarget,
    pub warnings: Vec<LayoutWarning>,
}

impl ScreenLayout {
    pub fn compute(screen: Rect, app: &App) -> Self {
        let mut warnings = Vec::new();
        let show_borders = app.show_borders();
        let show_status_bar = app.show_status_bar();
        let show_scrollbar = app.show_scrollbar();
        let input_mode = app.settings().mode;

        // in the border layout, the status bar takes no row of its own (MAX-135)
        let status_in_border =
            show_status_bar && app.status_bar_layout() == StatusBarLayout::Border;
        let status_height = if !show_status_bar || status_in_border {
            0
        } else if app.status_bar_layout() == StatusBarLayout::Box {
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
            // history's, the search bar on a row above it (MAX-218)
            let last_box = prompt_area.unwrap_or(monitor_outer);
            Some(Rect::new(
                last_box.x.saturating_add(2),
                last_box.bottom().saturating_sub(1),
                last_box.width.saturating_sub(4),
                1,
            ))
        } else if show_status_bar {
            Some(v_chunks[2])
        } else if app.status_bar_over_view() && monitor_outer.height > 0 {
            Some(Rect::new(
                monitor_outer.x,
                monitor_outer.bottom() - 1,
                monitor_outer.width,
                1,
            ))
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

        let side_shown = app.settings().raw_side.view != RawSideView::Off || app.plot().is_some();
        let (main_stream_area, side_area) = if side_shown {
            let inspector_percent = app
                .settings()
                .raw_side
                .split_percent
                .clamp(SPLIT_PERCENT_MIN, SPLIT_PERCENT_MAX);
            let main_percent = 100 - inspector_percent;
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

        // the search bar's row, under the history and over a bottom side panel (MAX-218)
        let main_stream_area = if app.search_bar_row() {
            Rect {
                height: main_stream_area.height.saturating_sub(1),
                ..main_stream_area
            }
        } else {
            main_stream_area
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
            raw_side_area: side_area.filter(|_| app.plot().is_none()),
            plot_area: side_area.filter(|_| app.plot().is_some()),
            raw_split_area: side_area.map(|_| remaining_after_gutter),
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

/// One line laid out: from `visual_row_start`, its `gap` blank rows (`entry_spacing`), then
/// one row per sub-line
#[derive(Debug, Clone)]
pub struct LogicalLineLayout {
    pub visual_row_start: usize,
    pub gap: usize,
    pub sub_lines: Vec<VisualSubLine>,
}

impl LogicalLineLayout {
    /// The row of the first sub-line, after the gap
    pub fn first_text_row(&self) -> usize {
        self.visual_row_start + self.gap
    }

    /// The rows it takes, gap included (an empty line still takes a row)
    pub fn rows(&self) -> usize {
        self.gap + self.sub_lines.len().max(1)
    }
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
    let prefixes = settings.line_prefixes();
    let base_prefix = match settings.line_mode.wrap_indent {
        PrefixWrapBehavior::None => "",
        PrefixWrapBehavior::Prefix | PrefixWrapBehavior::Timestamp => &prefixes.indent,
    };

    let timestamp_pad = if settings.line_mode.wrap_indent == PrefixWrapBehavior::Timestamp
        && settings.show_timestamps
    {
        let prefix_w = prefixes.line.width();
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
    let gap = settings.line_mode.entry_spacing;

    for line in lines {
        let visual_row_start = visual_lines.len();
        visual_lines.extend(std::iter::repeat_n(Line::default(), gap));

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
                gap,
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
                gap,
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
#[path = "../tests/ui/layout.rs"]
mod tests;
