use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::{Line, Span},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    app::{App, types::FocusTarget},
    configs::{
        Mode, RawSideView, SettingsConfig, ThemeConfig, WrapMode,
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

        let status_height = if show_status_bar { 1 } else { 0 };
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
        let status_bar_area = if show_status_bar {
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

struct WordToken {
    parts: Vec<(String, Style)>,
    is_whitespace: bool,
    char_count: usize,
    visual_width: usize,
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
            for (text_part, style) in token.parts {
                let mut remaining = text_part;
                while !remaining.is_empty() {
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
        let ts_width = gutter_chars.saturating_sub(prefix_w);
        " ".repeat(ts_width)
    } else {
        String::new()
    };

    let full_indent_prefix = if timestamp_pad.is_empty() {
        base_prefix.to_string()
    } else {
        format!("{}{}", timestamp_pad, base_prefix)
    };
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
}
