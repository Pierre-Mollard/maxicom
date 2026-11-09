use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
};

use std::fmt::Write;

use unicode_width::UnicodeWidthStr;

use crate::{
    app::{
        App,
        search::{MAX_SEARCH_MATCHES, SearchDirection, SearchModeKind},
        types::{ClickAction, ClickTarget, FocusTarget, PromptMap},
    },
    configs::{
        KeyBindingAction, KeyBindingCopyAction, KeyBindingSearchAction, KeywordHighlight, Mode,
        ThemeConfig,
    },
    ui::popups::hint_spans,
};

/// The history box's title, ` maxicom LINE `; with `title_details` also the alerts (`!` for
/// missed content, `● REC`), the sizes, the scroll, the pan and the prefix. What doesn't fit in
/// `room` columns is left out whole, never cut: the details from the last one, then the
/// prefix, then the alerts; without room for the name, no title (MAX-137)
pub fn monitor_title(app: &App, area: &Rect, room: u16) -> String {
    let view = format!(
        "VIEW {}x{}",
        area.width.saturating_sub(2),
        area.height.saturating_sub(2)
    );
    let logging = app.recording().is_some();
    let (mode, mut details, missed, scroll_offset, pan_offset) = match app.settings().mode {
        Mode::Terminal => {
            let tm = &app.terminal_mode;
            let (rows, cols) = tm.vt100_screen().size();
            (
                "TERMINAL",
                vec![format!("VT {cols}x{rows}"), view],
                tm.is_new_content_missed(),
                tm.vt100_screen().scrollback(),
                tm.pan_offset(),
            )
        }
        Mode::Line => {
            let lm = &app.line_mode;
            (
                "LINE",
                vec![view],
                lm.is_new_content_missed(),
                lm.scroll_status().offset_from_bottom,
                lm.x_offset(),
            )
        }
    };
    let name = format!("maxicom {mode}");
    let compose = |alerts: &[&str], details: &[String], prefix: bool| {
        let mut title = String::from(" ");
        for alert in alerts {
            title.push_str(alert);
            title.push(' ');
        }
        title.push_str(&name);
        if !details.is_empty() {
            let _ = write!(title, " [{}]", details.join(" | "));
        }
        if prefix {
            title.push_str(" | PREFIX");
        }
        title.push(' ');
        title
    };
    let fits = |title: &String| title.width() <= room as usize;

    if !app.settings().ui.title_details {
        return Some(compose(&[], &[], false))
            .filter(fits)
            .unwrap_or_default();
    }
    let alerts: Vec<&str> = [(missed, "!"), (logging, "● REC")]
        .into_iter()
        .filter_map(|(on, alert)| on.then_some(alert))
        .collect();
    details.push(if scroll_offset > 0 {
        format!("SCROLL +{scroll_offset}")
    } else {
        "LIVE".to_string()
    });
    if pan_offset > 0 {
        details.push(format!("PAN +{pan_offset}"));
    }
    let prefix = app.is_prefix_active();

    (0..=details.len())
        .rev()
        .map(|count| compose(&alerts, &details[..count], prefix))
        .chain([compose(&alerts, &[], false), compose(&[], &[], false)])
        .find(fits)
        .unwrap_or_default()
}

pub fn monitor_bottom_title<'a>(app: &App, theme: &ThemeConfig) -> Vec<Line<'a>> {
    let mut titles = Vec::with_capacity(2);
    let arrow_style = Style::default()
        .fg(theme.palette.secondary)
        .add_modifier(Modifier::BOLD);

    if let Some(left) = pan_left_indicator(app) {
        titles
            .push(Line::from(vec![Span::raw(" "), Span::styled(left, arrow_style)]).left_aligned());
    }

    if let Some(right) = pan_right_indicator(app) {
        titles.push(
            Line::from(vec![Span::styled(right, arrow_style), Span::raw(" ")]).right_aligned(),
        );
    }

    titles
}

/// The parts of the history's title a click runs (MAX-143): `SCROLL +n` back to the live view,
/// `PAN +n` to the left edge. The title starts on the column after the border's corner
pub fn monitor_title_targets(title: &str, outer: Rect, hover: Style) -> Vec<ClickTarget> {
    [
        ("SCROLL +", KeyBindingAction::ScrollToBottom),
        ("PAN +", KeyBindingAction::PanHome),
    ]
    .into_iter()
    .filter_map(|(part, action)| {
        let start = title.find(part)?;
        let digits = title[start + part.len()..]
            .chars()
            .take_while(char::is_ascii_digit)
            .count();
        let x = outer.x + 1 + title[..start].width() as u16;
        let width = (part.len() + digits) as u16;
        Some(ClickTarget {
            area: Rect::new(x, outer.y, width, 1).intersection(outer),
            action: ClickAction::Run(action),
            hover: Some(hover),
        })
    })
    .collect()
}

/// The pan arrows of the history's bottom border (`monitor_bottom_title`) as click targets
/// (MAX-143): `◀ (n)` pans left, `(n) ▶` right. Each is a space away from its corner
pub fn monitor_bottom_targets(app: &App, outer: Rect, hover: Style) -> Vec<ClickTarget> {
    let y = outer.bottom().saturating_sub(1);
    let target = |x: u16, text: &str, action| ClickTarget {
        area: Rect::new(x, y, text.width() as u16, 1).intersection(outer),
        action: ClickAction::Run(action),
        hover: Some(hover),
    };
    let left =
        pan_left_indicator(app).map(|left| target(outer.x + 2, &left, KeyBindingAction::PanLeft));
    let right = pan_right_indicator(app).map(|right| {
        let x = outer.right().saturating_sub(2 + right.width() as u16);
        target(x, &right, KeyBindingAction::PanRight)
    });
    left.into_iter().chain(right).collect()
}

pub fn pan_left_indicator(app: &App) -> Option<String> {
    app.pan_left_status()
        .filter(|&cols| cols > 0)
        .map(|cols| format!("◀ ({})", cols))
}

pub fn pan_right_indicator(app: &App) -> Option<String> {
    app.pan_right_status().and_then(|(cols, explored)| {
        if cols == 0 {
            return None;
        }

        let alert = if app.settings().mode == crate::configs::Mode::Line
            && cols > explored.saturating_sub(app.line_mode.x_offset())
        {
            "!"
        } else {
            ""
        };

        Some(format!("{}({}) ▶", alert, cols))
    })
}

/// Draws the search bar; returns its hints as click targets (MAX-131), and where its query is
/// for a click in it. Closed, in copy mode with its matches kept, it shows the last search and
/// the copy keys going through its matches, without a cursor (MAX-215)
pub fn render_search_bar(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    theme: &ThemeConfig,
) -> (Vec<ClickTarget>, Option<PromptMap>) {
    // in the history's bottom border, or on the row the layout leaves it (MAX-218)
    let in_border = !app.search_bar_row();
    let is_terminal = app.settings().mode == Mode::Terminal;

    let s = if is_terminal {
        app.terminal_mode.search()
    } else {
        app.line_mode.search()
    };

    let query_str = s.query_str();
    let visual_cursor = s.query.visual_cursor() as u16;
    let match_count = s.matches_count();
    let current_pos = s.current_match_display();

    let stats_style = if match_count > 0 {
        Style::default().fg(theme.palette.text_good)
    } else if !query_str.is_empty() {
        Style::default().fg(theme.palette.text_warn)
    } else {
        Style::default().fg(theme.palette.text_muted)
    };

    // as typed in vi: `/` forward, `?` backward, the query right after (MAX-218)
    let label = match s.direction {
        SearchDirection::Forward => " /",
        SearchDirection::Backward => " ?",
    };
    // the matches past the cap aren't counted
    let count = match match_count {
        MAX_SEARCH_MATCHES.. => format!("{MAX_SEARCH_MATCHES}+"),
        count => count.to_string(),
    };

    let muted = Style::default().fg(theme.palette.text_muted);
    let typed = Style::default()
        .fg(theme.palette.secondary)
        .add_modifier(Modifier::BOLD);
    // an empty query shows the last one dimmed, which Enter searches again (MAX-218)
    let query_span = if query_str.is_empty() && !s.last_query.is_empty() {
        Span::styled(format!("{} ", s.last_query), muted)
    } else {
        Span::styled(format!("{} ", query_str), typed)
    };

    // a text search, the default, has no tag; a byte search's is dimmed (MAX-218)
    let mut spans = Vec::with_capacity(6);
    if s.mode_kind == SearchModeKind::RawBytes {
        spans.push(Span::styled(" [bytes]", muted));
    }
    spans.push(Span::styled(label, typed));
    let query_col: usize = spans.iter().map(Span::width).sum();
    spans.push(query_span);
    let counter = Span::styled(format!("({current_pos}/{count})"), stats_style);

    // one key per action, like the popups; the hints that don't fit whole are left out, the
    // least needed ones first. A click on a key runs its action, on a label the first one
    let first_key = |action| {
        let keys = app.key_resolver.search_keys(action).into_iter().take(1);
        keys.map(move |key| (key, ClickAction::Search(action)))
    };
    let first_copy_key = |action| {
        let keys = app.key_resolver.copy_keys(action).into_iter().take(1);
        keys.map(move |key| (key, ClickAction::Copy(action)))
    };
    let hints: Vec<(Vec<_>, &str)> = if s.active {
        vec![
            (
                first_key(KeyBindingSearchAction::Next)
                    .chain(first_key(KeyBindingSearchAction::Previous))
                    .collect(),
                "Match",
            ),
            (first_key(KeyBindingSearchAction::Cancel).collect(), "Exit"),
            (first_key(KeyBindingSearchAction::Confirm).collect(), "Go"),
            (first_key(KeyBindingSearchAction::Kind).collect(), "Mode"),
        ]
    } else {
        vec![
            (
                first_copy_key(KeyBindingCopyAction::SearchNext)
                    .chain(first_copy_key(KeyBindingCopyAction::SearchPrev))
                    .collect(),
                "Match",
            ),
            (first_copy_key(KeyBindingCopyAction::Exit).collect(), "Exit"),
            (
                first_copy_key(KeyBindingCopyAction::Search).collect(),
                "Search",
            ),
            (
                first_copy_key(KeyBindingCopyAction::SearchBackward).collect(),
                "Back",
            ),
        ]
    };
    // in the border the bar starts 2 columns in and keeps a space before the corner
    let room = if in_border {
        area.width.saturating_sub(4)
    } else {
        area.width
    } as usize;
    // the counter and the hints at the bar's right end, so typing doesn't move them; after
    // the query when it's too long for that (MAX-218). The hints that fit there are shown
    let left_width: usize = spans.iter().map(Span::width).sum();
    let used = left_width + counter.width() + 4;
    let (hints, spots) = (0..=hints.len())
        .rev()
        .map(|count| hint_spans(hints[..count].iter().cloned(), theme))
        .find(|(line, _)| line.width() + used <= room)
        .unwrap_or_default();
    // a space before the counter, off the border line
    let counter_width = counter.width() + 1;
    let mut right = vec![Span::raw(" "), counter];
    if !hints.spans.is_empty() {
        right.push(Span::raw("  "));
        right.extend(hints.spans);
    }
    right.push(Span::raw(" "));
    let right_width: usize = right.iter().map(Span::width).sum();
    let right_col = room.saturating_sub(right_width).max(left_width);
    // where the hints start in the bar, after the counter and the two spaces before them
    let hints_col = (right_col + counter_width + 2) as u16;

    // under the view: on its bottom border, or the row the layout leaves it (MAX-218)
    let bar_y = area.y + area.height;
    let bar_x = if in_border { area.x + 2 } else { area.x };
    let targets = spots
        .into_iter()
        .map(|(col, width, action)| ClickTarget {
            area: Rect::new(bar_x + hints_col + col, bar_y, width, 1),
            action,
            hover: Some(*theme.ui.hover),
        })
        .collect();
    // the query, right after the kind and the label, and a column past it for its end
    let query_x = bar_x + query_col as u16;
    let query = PromptMap {
        text_area: Rect::new(query_x, bar_y, query_str.width() as u16 + 1, 1),
        scroll: 0,
    };

    // cut at the bar's end; in the border, the border line shows between the two parts
    let bar_end = bar_x as usize + room;
    let buf = frame.buffer_mut();
    for (col, part) in [(0, &spans), (right_col, &right)] {
        let mut x = bar_x as usize + col;
        for span in part.iter() {
            let width = bar_end.saturating_sub(x);
            if width == 0 {
                break;
            }
            buf.set_stringn(x as u16, bar_y, &span.content, width, span.style);
            x += span.width();
        }
    }
    let (cursor_x, cursor_y) = (query_x + visual_cursor, bar_y);

    // kept in copy mode, its query isn't typed in: no cursor, no click in it
    if !s.active {
        return (targets, None);
    }
    if !app.is_app_focused() {
        let buf = frame.buffer_mut();
        if cursor_x < buf.area.width && cursor_y < buf.area.height {
            buf[(cursor_x, cursor_y)].set_style(Style::default().add_modifier(Modifier::REVERSED));
        }
    }
    frame.set_cursor_position((cursor_x, cursor_y));
    (targets, Some(query))
}

/// A centered `width` x `height` popup, cut to the screen when it is smaller
pub fn popup_area(parent: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(parent.width);
    let height = height.min(parent.height);
    let x = parent.x + (parent.width - width) / 2;
    let y = parent.y + (parent.height - height) / 2;
    Rect::new(x, y, width, height)
}

pub fn border_style_for(
    target: FocusTarget,
    current: FocusTarget,
    app_focused: bool,
    theme: &ThemeConfig,
) -> Style {
    if !app_focused {
        *theme.ui.border_app_unfocused
    } else if target == current {
        *theme.ui.border_panel_focused
    } else {
        *theme.ui.border_panel
    }
}

pub fn apply_keyword_highlights(buf: &mut Buffer, area: Rect, highlights: &[KeywordHighlight]) {
    if highlights.is_empty() {
        return;
    }

    for row in area.top()..area.bottom() {
        let mut line_str = String::with_capacity(area.width as usize);
        for col in area.left()..area.right() {
            line_str.push_str(buf[(col, row)].symbol());
        }

        for hl in highlights {
            if hl.pattern.is_empty() {
                continue;
            }

            for (byte_offset, _) in line_str.match_indices(&hl.pattern) {
                let start_char_col = line_str[..byte_offset].chars().count();
                let match_len_chars = hl.pattern.chars().count();

                for char_offset in 0..match_len_chars {
                    let col = area.left() + (start_char_col + char_offset) as u16;
                    if col >= area.right() {
                        break;
                    }

                    let cell = &mut buf[(col, row)];
                    if let Some(fg) = hl.style.fg {
                        cell.set_fg(fg);
                    }
                    if let Some(bg) = hl.style.bg {
                        cell.set_bg(bg);
                    }
                    cell.set_style(cell.style().add_modifier(hl.style.add_modifier));
                }
            }
        }
    }
}

pub fn apply_selection_to_spans(
    spans: Vec<Span<'static>>,
    sel_start: usize,
    sel_end: usize,
    selection_style: Style,
) -> Vec<Span<'static>> {
    if sel_start > sel_end {
        return spans;
    }

    let mut out = Vec::with_capacity(spans.len() + 2);
    let mut char_cursor: usize = 0;

    for span in spans {
        let span_chars: Vec<char> = span.content.chars().collect();
        let span_len = span_chars.len();
        if span_len == 0 {
            out.push(span);
            continue;
        }

        let span_start = char_cursor;
        let span_end = char_cursor + span_len - 1;
        char_cursor += span_len;

        if span_end < sel_start || span_start > sel_end {
            out.push(span);
            continue;
        }

        if span_start >= sel_start && span_end <= sel_end {
            out.push(Span::styled(span.content, selection_style));
            continue;
        }

        let overlap_start = span_start.max(sel_start);
        let overlap_end = span_end.min(sel_end);

        let local_start = overlap_start - span_start;
        let local_end = overlap_end - span_start + 1;

        if local_start > 0 {
            let before: String = span_chars[..local_start].iter().collect();
            out.push(Span::styled(before, span.style));
        }

        if local_start < local_end {
            let selected: String = span_chars[local_start..local_end].iter().collect();
            out.push(Span::styled(selected, selection_style));
        }

        if local_end < span_len {
            let after: String = span_chars[local_end..].iter().collect();
            out.push(Span::styled(after, span.style));
        }
    }

    out
}

/// A one-row input's scroll (the TX input, the command line): the text's columns hidden on the
/// left, and whether text is hidden on the left and on the right, each shown by a mark on that
/// edge's cell (MAX-145). The view keeps the last frame's scroll while the cursor is on one of
/// its text cells, not under a mark, and moves just enough to bring it back otherwise: a cursor
/// pushing past an edge scrolls by one. At the end of the text the cursor takes one more cell
pub fn prompt_view(
    text_width: usize,
    cursor: usize,
    width: usize,
    last_scroll: usize,
) -> (usize, bool, bool) {
    let content = text_width.max(cursor + 1);
    if content <= width {
        return (0, false, false);
    }
    // too narrow for marks
    if width < 3 {
        let scroll = last_scroll.clamp((cursor + 1).saturating_sub(width), cursor);
        return (scroll, false, false);
    }
    // at most the end of the text shown, after the left mark, with the cell after it kept
    // whatever the cursor's place, so moving off the end doesn't scroll
    let max_scroll = text_width + 1 - width;
    let mut scroll = last_scroll.min(max_scroll);
    let first = if scroll > 0 { scroll + 1 } else { 0 };
    let last = if scroll < max_scroll {
        scroll + width - 2
    } else {
        scroll + width - 1
    };
    if cursor < first {
        // the cursor right after the left mark, or the text's start
        scroll = cursor.saturating_sub(1);
    } else if cursor > last {
        // the cursor right before the right mark, or the text's end
        scroll = (cursor + 2 - width).min(max_scroll);
    }
    (scroll, scroll > 0, scroll < max_scroll)
}

/// The history marks' letters on the scrollbar's track, each where its row is in the whole
/// history (MAX-164): `marks` gives each letter's row, of `total` rows. On the thumb a letter
/// takes the thumb's color as its background, so the thumb shows no hole
pub fn draw_scrollbar_marks(
    buf: &mut Buffer,
    area: Rect,
    marks: &[(char, usize)],
    total: usize,
    theme: &ThemeConfig,
) {
    let track = area.height.saturating_sub(2) as usize;
    if track == 0 || total == 0 {
        return;
    }
    let thumb = theme.ui.scrollbar_thumb.fg;
    for &(letter, row) in marks {
        let at = (row * track / total).min(track - 1);
        let cell = &mut buf[(area.x, area.y + 1 + at as u16)];
        let mut style = *theme.ui.scrollbar_mark;
        if cell.symbol() == "█"
            && let Some(thumb) = thumb
        {
            style = style.bg(thumb);
        }
        cell.set_symbol(letter.encode_utf8(&mut [0; 4]))
            .set_style(style);
    }
}
