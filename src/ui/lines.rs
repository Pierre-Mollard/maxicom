use std::{
    collections::VecDeque,
    hash::{DefaultHasher, Hash, Hasher},
};

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    app::{
        App,
        data_processing::{entry_display_text, entry_display_to_wire_map, map_disp_byte_to_raw},
        line_mode::ViewFloor,
        scroller::ScrollStatus,
        search::SearchMatch,
        selection::{CopySide, Reveal},
        types::{FocusTarget, LogEntry, PromptMap, RawRow, RowChars, ScrollbarMap, TextRow},
    },
    configs::{
        KeywordHighlight, LineDisplayMode, SettingsConfig, ThemeConfig, WrapMode,
        settings::{
            BORDER_PARTIAL, BORDER_WRAP, PrefixWrapBehavior, RawSideView, raw_view_to_display_mode,
        },
    },
    ui::{
        PostRenderEffect,
        common::{
            apply_keyword_highlights, apply_selection_to_spans, border_style_for, prompt_view,
            render_search_bar,
        },
        data_display::{format_entry_content, format_line_content},
        layout::{
            LogicalLineLayout, PreparedLayout, byte_mode_rows, continuation_indent,
            prepare_view_layout,
        },
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
        // a line without a time keeps the column blank, so the text stays aligned (MAX-148)
        let formatted_timestamp = match entry.timestamp {
            Some(time) => time.format(&settings.timestamp_format).to_string(),
            None => " ".repeat(settings.timestamp_width()),
        };
        spans.push(Span::styled(
            formatted_timestamp,
            *theme.line_mode.timestamp,
        ));
    }

    spans.push(Span::styled(
        cached_prefix.to_string(),
        theme.line_mode.line_prefix,
    ));

    let mut content_spans = format_entry_content(entry, display_mode, theme);

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

    if content_width == 0 || content_height == 0 {
        return PostRenderEffect::None;
    }
    if total_entries == 0 {
        // no text yet, the separator still runs down the pane
        let marks = vec![RowMark::Empty; content_height];
        draw_row_marks(app, frame.buffer_mut(), content_area, &marks, 0, theme);
        return PostRenderEffect::None;
    }

    let is_search_active =
        app.line_mode.search().active && !app.line_mode.search().matches.is_empty();
    let active_match_idx = app.line_mode.search().current_match;
    let copy_range = app.line_mode.copy_state().normalized_selection();

    let global_max_scroll = total_entries.saturating_sub(1);

    let search_jump = app.line_mode.search_mut().target_jump.take();
    if let Some((target_line, target_col)) = search_jump {
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

    let is_copy_active = app.line_mode.copy_state().active;
    let copy_cursor = (
        app.line_mode.copy_state().cursor.line_idx,
        app.line_mode.copy_state().cursor.char_idx,
    );
    // in copy mode the view moves only for a cursor move, which asked to reveal the cursor
    // (MAX-143); a search match is a jump
    let reveal = match app.line_mode.take_copy_reveal() {
        _ if !is_copy_active => None,
        _ if search_jump.is_some() => Some(Reveal::Center),
        reveal => reveal,
    };
    let active_focus = if reveal.is_some() {
        Some(copy_cursor)
    } else if is_copy_active {
        None
    } else if is_search_active && search_jump.is_some() {
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

    // ---- What this frame shows ----
    //
    // Two ways of choosing the visible rows, decided below once the window is laid out:
    // - plain scrolling (no search jump or copy cursor move this frame, copy mode included):
    //   the view's bottom row is a position kept by `LineMode` (#36): an entry counted from the
    //   bottom, and how many of its rows are hidden below the view. Scroll keys only add rows
    //   to a pending count, applied here. Offset 0 is the live view, aligned to the bottom of
    //   the history
    // - a search jump or a copy cursor move: the view is brought to the focused row (match or
    //   cursor), clamped to the rows laid out, so near the end it still ends on the last row.
    //   Centered for a jump; for a cursor step, the kept view moved by the fewest rows that
    //   show the cursor's row (MAX-143), not its entry, which can be taller than the view
    //   (MAX-123). Either way the view is stored as the scroll position, so the next frames
    //   scroll from there (MAX-72)
    let row_scrolling = active_focus.is_none();
    // The entry the window is built around: the scrolled-to one (the last one when live), or
    // the focused one
    let anchor_entry = focus_entry;

    // Rows scrolled since the last frame (#36): positive is up. Taken now, before layout, so the
    // window below lays out exactly what this frame needs. Capped at a view height either way
    // (a page is a view height minus one; more keys between two frames are dropped). A copy
    // cursor move in the same frame wins over them
    let limit = content_height as isize;
    let pending_rows = app
        .line_mode
        .take_pending_scroll_rows()
        .clamp(-limit, limit);

    // ---- Entry heights for the scroll position in rows (#5d) ----
    //
    // `LineMode` keeps one height (rows on screen) per history entry, 0 when not counted yet.
    // They depend on how entries are laid out, so they are counted for a key made of what
    // decides it: when the view width, wrap mode, display mode, timestamps, indent or gutter
    // change, the key changes and every height goes back to "not counted". Taken here and given
    // back once the position is known
    let display_mode = app.settings().line_mode.display_mode;
    let heights_key = {
        let mut hasher = DefaultHasher::new();
        content_width.hash(&mut hasher);
        std::mem::discriminant(&wrap_mode).hash(&mut hasher);
        std::mem::discriminant(&display_mode).hash(&mut hasher);
        std::mem::discriminant(&app.settings().line_mode.wrap_indent).hash(&mut hasher);
        app.settings().show_timestamps.hash(&mut hasher);
        app.settings().line_gutter_width().hash(&mut hasher);
        std::mem::discriminant(&app.settings().line_mode.prefix_place).hash(&mut hasher);
        app.settings().line_mode.entry_spacing.hash(&mut hasher);
        hasher.finish()
    };
    let mut row_heights = app.line_mode.take_row_heights(heights_key);
    row_heights.resize(total_entries, 0);

    // ---- Laying out one entry ----
    //
    // Builds the styled line of one history entry (timestamp, prefix, content, with its search
    // matches and selection range) and wraps it to the view width. The result has one visual
    // row per wrapped sub-line, so its `visual_lines.len()` is the entry's height on screen.
    // Laying out entries one at a time is what lets the window stop as soon as it has enough
    // rows; `PreparedLayout::concat` then joins them as if they had been laid out together
    let search = app.line_mode.search();
    let line_prefix_cached = app.settings().line_prefixes().line.to_string();
    let lay_out_entry = |global_idx: usize| {
        let entry = &app.line_mode.history()[global_idx];
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

        let line = history_entry_to_line(
            entry,
            app.settings(),
            &line_matches,
            active_match_idx,
            sel_range,
            theme,
            &line_prefix_cached,
        );
        prepare_view_layout(vec![line], app.settings(), theme, content_width)
    };

    // ---- The window: only the entries this frame can show (#5a) ----
    // Entries are laid out one at a time, outward from the anchor, and the walk stops as soon
    // as the rows this frame needs are covered, counted in rows
    // (an entry can be one row or dozens):
    //
    // - plain scrolling: the view's bottom row must be able to move up by the pending rows and
    //   still have a full view above it, so above it we need `view height + rows scrolled up`;
    //   below it, only the rows scrolled down
    // - bringing the view to a search / copy focus: up to a view height on each side (the
    //   focused row can end up anywhere from the top to the bottom of the view once clamped).
    //   The kept view counts only when its bottom row is in those rows: farther, it can't show
    //   the focused row
    //
    // Example, plain scrolling, view of 10 rows, 3 rows scrolled up, anchor of 6 rows with 2
    // hidden below the view: 4 anchor rows are already above the bottom row, 13 are needed, so
    // entries above are added until at least 9 more rows are counted; below, the 2 hidden rows
    // already cover the 0 rows needed
    let (rows_above_needed, rows_below_needed) = if row_scrolling {
        (
            content_height + pending_rows.max(0) as usize,
            (-pending_rows).max(0) as usize,
        )
    } else {
        (content_height, content_height)
    };
    // The window is the history range `start_idx..end_idx`, laid out in order in `window`
    let mut start_idx = anchor_entry;
    let mut end_idx = anchor_entry + 1;
    let mut window: VecDeque<PreparedLayout> = VecDeque::new();

    // every entry laid out for the window gives its height for free (#5d)
    let mut lay_out_and_count = |global_idx: usize| {
        let entry = lay_out_entry(global_idx);
        row_heights[global_idx] = entry.visual_lines.len() as u32;
        entry
    };

    let anchor = lay_out_and_count(anchor_entry);
    let anchor_rows = anchor.visual_lines.len();
    // Rows already covered by the anchor itself. In plain scrolling the view's bottom row can be
    // inside a tall anchor: its rows above that bottom row count as "above", the ones hidden
    // below the view as "below" (`rows_below` is capped so at least one row stays above). When
    // centering a focus, where the focused row falls in the anchor is only known later, so
    // nothing is counted and a full view height is laid out on each side
    let (mut rows_above, mut rows_below) = if row_scrolling {
        let hidden = app
            .line_mode
            .rows_below()
            .min(anchor_rows.saturating_sub(1));
        (anchor_rows - hidden, hidden)
    } else {
        (0, 0)
    };
    window.push_back(anchor);

    // Walk up: lay out the entry before the window and put it in front, until enough rows are
    // above the view's bottom row or the history starts
    while rows_above < rows_above_needed && start_idx > 0 {
        start_idx -= 1;
        let entry = lay_out_and_count(start_idx);
        rows_above += entry.visual_lines.len();
        window.push_front(entry);
    }

    // Walking up stopped short at the start of the history: the view can't go that high, so
    // the top clamp (below) moves it down instead, onto rows after the anchor. Those rows are
    // needed too (e.g. scroll to top with a first entry shorter than the view)
    let rows_below_needed = rows_below_needed + rows_above_needed.saturating_sub(rows_above);

    // Walk down: lay out the entry after the window and append it, until enough rows are below
    // or the history ends
    while rows_below < rows_below_needed && end_idx < total_entries {
        let entry = lay_out_and_count(end_idx);
        rows_below += entry.visual_lines.len();
        window.push_back(entry);
        end_idx += 1;
    }

    // ---- Heights of the entries outside the window (#5d) ----
    //
    // Counted once (they stay counted until the key changes), the cheapest correct way:
    // - truncate: one row each
    // - byte modes (Hex, Octal, Binary) with word wrap: by arithmetic from the byte count
    //   (`byte_mode_rows`), when the gutter ends with a space so it can't merge with the first
    //   byte (a prefix like "|" would); `None` from it falls back to laying out
    // - anything else: laid out like the window entries (after a width change in Pretty
    //   that is ~2 µs per entry, once)
    let byte_digits = match display_mode {
        LineDisplayMode::Hex => Some(2),
        LineDisplayMode::Octal => Some(3),
        LineDisplayMode::Binary => Some(8),
        _ => None,
    };
    let gutter_width = app.settings().line_gutter_width();
    let gutter_ends_with_space = gutter_width == 0
        || app
            .settings()
            .line_prefixes()
            .line
            .ends_with(char::is_whitespace);
    let indent_width = continuation_indent(app.settings()).width();
    for (idx, height) in row_heights.iter_mut().enumerate() {
        if *height != 0 {
            continue;
        }
        let counted = match (wrap_mode, byte_digits) {
            (WrapMode::Truncate, _) => Some(1),
            (WrapMode::Word, Some(digits)) if gutter_ends_with_space => byte_mode_rows(
                app.line_mode.history()[idx].wire_bytes().len(),
                digits,
                content_width,
                gutter_width,
                indent_width,
            ),
            _ => None,
        };
        // the counts leave out the gap rows the layout puts before the text
        let counted = counted.map(|rows| rows + app.settings().line_mode.entry_spacing);
        *height = counted.unwrap_or_else(|| lay_out_entry(idx).visual_lines.len()) as u32;
    }

    // The partial RX line (received but not ended yet) is drawn after the last entry, so only
    // when the window reaches the last entry: otherwise it would land in the middle of history
    if end_idx == total_entries
        && app.settings().line_mode.show_partial_rx
        && !app.line_mode.partial_rx().is_empty()
    {
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

            let partial_prefix = app.settings().line_prefixes().partial.to_string();
            let partial_prefix_style = *theme.line_mode.line_partial_prefix;
            partial_spans.push(Span::styled(partial_prefix, partial_prefix_style));
            partial_spans.extend(format_line_content(&partial_raw, display_mode, theme));
            let partial_line = Line::from(partial_spans);
            window.push_back(prepare_view_layout(
                vec![partial_line],
                app.settings(),
                theme,
                content_width,
            ));
        }
    }

    // One layout for the window: its row indexes start at 0 on `start_idx`'s first row, and
    // `layout.line_layouts[i]` is the entry `start_idx + i` (then the partial RX line, if any)
    let layout = PreparedLayout::concat(window);
    let max_pan = match wrap_mode {
        WrapMode::Word | WrapMode::Char => 0,
        WrapMode::Truncate => layout.max_line_width.saturating_sub(content_width),
    };

    app.line_mode.update_limits(global_max_scroll, max_pan);

    let total_visual_rows = layout.visual_lines.len();

    // ---- Which rows of the window are on screen ----
    //
    // `paragraph_y_offset` is the window row drawn on the top line of the view. All the rows
    // below are window rows (`layout.visual_lines`), so positions stay valid for the copy
    // cursor, the selection, the search highlights and the raw panel
    //
    // Only the window's entries count for the scroll position, not the partial RX line drawn
    // after them
    let entries = &layout.line_layouts[..end_idx - start_idx];
    let rows_of = LogicalLineLayout::rows;
    let last_row = entries
        .last()
        .map_or(0, |meta| meta.visual_row_start + rows_of(meta) - 1);
    // Stores the view ending on window row `bottom` as the scroll position: back to the live
    // view when it is the last entry's last row, else (entry, rows hidden below): the entry
    // holding the bottom row (the last one starting at or before it) and its rows after it
    let position_of = |bottom: usize| {
        if end_idx == total_entries && bottom >= last_row {
            return None;
        }
        let local = entries.partition_point(|meta| meta.visual_row_start <= bottom) - 1;
        let meta = &entries[local];
        let rows_below = meta.visual_row_start + rows_of(meta) - 1 - bottom;
        let entries_from_bottom = total_entries - 1 - (start_idx + local);
        Some((entries_from_bottom, rows_below))
    };
    let store_position = |app: &mut App, bottom: usize| match position_of(bottom) {
        Some((entries_from_bottom, rows_below)) => app
            .line_mode
            .set_scroll_position(entries_from_bottom, rows_below),
        None => app.line_mode.scroll_to_bottom(),
    };
    // The window row of the sub-line holding char `char_idx` of the window's entry `local` (the
    // last sub-line past its end), as the copy cursor is drawn
    let row_of_char = |local: usize, char_idx: usize| {
        let meta = layout.line_layouts.get(local)?;
        let mut remaining = char_idx;
        let mut sub_row = 0;
        for (s_idx, sub) in meta.sub_lines.iter().enumerate() {
            sub_row = s_idx;
            if remaining < sub.content_char_len {
                break;
            }
            remaining -= sub.content_char_len;
        }
        Some(meta.first_text_row() + sub_row)
    };

    // ---- The live view's floor (MAX-176) ----
    //
    // After a `clear` (a restore) the live view starts on the floor entry, blank rows under the
    // last line, while the lines from there don't fill it. Its top row is the floor entry's
    // first text row less the rows scrolled above it; only a view still live has it, and the
    // floor goes once the normal live view starts lower (new lines filled the blank rows). The
    // floor entry may be the partial RX line, or not received yet: the window's end then
    let live_top = total_visual_rows.saturating_sub(content_height);
    let is_live = clamped_scroll == 0 && app.line_mode.rows_below() == 0;
    let floor = app
        .line_mode
        .view_floor()
        .filter(|_| is_live)
        .and_then(|floor| {
            let local = floor.line_idx.checked_sub(start_idx)?;
            let row = match layout.line_layouts.get(local) {
                Some(meta) => meta.first_text_row(),
                None if local == layout.line_layouts.len() => total_visual_rows,
                None => return None,
            };
            Some((row, floor.rows_above))
        });
    let floor_top = |(row, above): (usize, usize)| row.saturating_sub(above);
    if floor.is_none() {
        app.line_mode.set_view_floor(None);
    }

    let paragraph_y_offset: usize = if row_scrolling {
        // Plain scrolling (#36): where the view's bottom row was (the anchor's last row, minus
        // its rows hidden below), then moved up by the rows scrolled since the last frame (down
        // when negative)
        let anchor = &entries[focus_entry - start_idx];
        let rows_below = app.line_mode.rows_below().min(rows_of(anchor) - 1);
        let anchor_bottom = anchor.visual_row_start + rows_of(anchor) - 1 - rows_below;
        let mut bottom = anchor_bottom as isize - pending_rows;

        // On the floor the scrolled rows move it (never under its entry) while blank rows still
        // show; past that, the view it reached is a normal one, its bottom row a view lower
        let floor = floor.map(|(row, above)| {
            let above = (above as isize + pending_rows).max(0) as usize;
            (row, above)
        });
        let on_floor = floor.filter(|&floor| floor_top(floor) > live_top);
        if let Some((row, above)) = floor
            && on_floor.is_none()
        {
            app.line_mode.set_view_floor(None);
            bottom = (floor_top((row, above)) + content_height) as isize - 1;
        }

        if let Some((row, above)) = on_floor {
            let line_idx = app.line_mode.view_floor().map_or(0, |floor| floor.line_idx);
            app.line_mode.set_view_floor(Some(ViewFloor {
                line_idx,
                rows_above: above,
            }));
            floor_top((row, above))
        } else if end_idx == total_entries && bottom >= last_row as isize {
            // Reached the last entry's last row: back to the live view, aligned to the bottom
            // of everything laid out (partial RX line included)
            app.line_mode.scroll_to_bottom();
            total_visual_rows.saturating_sub(content_height)
        } else {
            // Top clamp: when the window starts at the first entry, the bottom row can't be
            // higher than a view height down, so the first row stays on the top line
            let min_bottom = if start_idx == 0 {
                content_height.saturating_sub(1).min(last_row)
            } else {
                0
            };
            let bottom = bottom.clamp(min_bottom as isize, last_row as isize) as usize;
            store_position(app, bottom);
            // the view ends on `bottom`: its top line is a view height above
            (bottom + 1).saturating_sub(content_height)
        }
    } else {
        // A search jump or a copy cursor move: the focused row (the sub-line holding the
        // focused char of the focused entry) is brought into view, clamped so the view stays
        // filled
        let target_char = active_focus.map(|(_, c)| c).unwrap_or(0);
        let target_visual_row = row_of_char(focus_entry - start_idx, target_char).unwrap_or(0);

        // The kept view's top row, as plain scrolling would draw it, when its bottom row is in
        // the window (else it is more than a view height away from the focus: not shown)
        let kept_top = || {
            let rows_below = app.line_mode.rows_below();
            if is_live {
                let top = floor.map_or(live_top, floor_top).max(live_top);
                return (end_idx == total_entries).then_some(top);
            }
            let local = (total_entries - 1 - clamped_scroll).checked_sub(start_idx)?;
            let meta = entries.get(local)?;
            let bottom =
                meta.visual_row_start + rows_of(meta) - 1 - rows_below.min(rows_of(meta) - 1);
            let min_bottom = if start_idx == 0 {
                content_height.saturating_sub(1).min(last_row)
            } else {
                0
            };
            Some((bottom.max(min_bottom) + 1).saturating_sub(content_height))
        };

        // a copy cursor already shown keeps the view, whatever the move (MAX-143)
        let centered = target_visual_row.saturating_sub(content_height / 2);
        let offset = match (reveal, kept_top()) {
            (Some(_), Some(top)) if (top..top + content_height).contains(&target_visual_row) => top,
            (Some(Reveal::Edge), Some(top)) if target_visual_row < top => target_visual_row,
            (Some(Reveal::Edge), Some(_)) => target_visual_row + 1 - content_height,
            _ => centered,
        };
        // On the floor, a view still higher than the live top moves the floor, as scrolling
        // does (never under its entry); a lower one is a normal view and the floor goes
        let offset = match (floor, app.line_mode.view_floor()) {
            (Some((row, _)), Some(view_floor)) if offset > live_top => {
                let offset = offset.min(row);
                app.line_mode.set_view_floor(Some(ViewFloor {
                    rows_above: row - offset,
                    ..view_floor
                }));
                offset
            }
            _ => {
                app.line_mode.set_view_floor(None);
                offset.min(live_top)
            }
        };
        // the next frames scroll from this view
        let bottom = (offset + content_height).saturating_sub(1).min(last_row);
        store_position(app, bottom);
        offset
    };

    // for the next cursor move: a cursor that isn't shown is revealed centered (MAX-143)
    let cursor_shown = is_copy_active
        && (start_idx..end_idx).contains(&copy_cursor.0)
        && row_of_char(copy_cursor.0 - start_idx, copy_cursor.1).is_some_and(|row| {
            (paragraph_y_offset..paragraph_y_offset + content_height).contains(&row)
        });
    app.line_mode.set_copy_cursor_shown(cursor_shown);

    // where copy mode entered from a scrolled view puts its cursor: the start of the view's
    // bottom row, or of the last entry's last row below it (the partial RX line isn't one)
    let bottom = (paragraph_y_offset + content_height)
        .saturating_sub(1)
        .min(last_row);
    let mut local = entries.partition_point(|meta| meta.visual_row_start <= bottom) - 1;
    // a bottom row in a gap: the text of its entry is below the view, the entry above ends in it
    if bottom < entries[local].first_text_row() && local > 0 {
        local -= 1;
    }
    let meta = &entries[local];
    let sub_row = bottom
        .saturating_sub(meta.first_text_row())
        .min(meta.sub_lines.len().saturating_sub(1));
    let char_idx = meta.sub_lines[..sub_row]
        .iter()
        .map(|sub| sub.content_char_len)
        .sum();
    app.line_mode
        .set_bottom_row_start((start_idx + local, char_idx));

    // ---- Scroll position in rows (#5d) ----
    //
    // With every entry's height known, the view's place in the whole history is exact: its top
    // row is the rows of the entries before the window plus `paragraph_y_offset`. From the
    // bottom of the history it is what is left below the view (0 when live; the partial RX line
    // is not counted, so it can only make that 0 sooner). The scrollbar, `Bot` / `%` and the
    // title's `SCROLL +n` all read this
    let rows_before_window: usize = row_heights
        .range(..start_idx)
        .map(|&height| height as usize)
        .sum();
    let total_rows: usize = row_heights.iter().map(|&height| height as usize).sum();
    let max_scroll = total_rows.saturating_sub(content_height);
    let view_top = rows_before_window + paragraph_y_offset;
    let offset_from_bottom = total_rows
        .saturating_sub(view_top + content_height)
        .min(max_scroll);
    let scroll_status = ScrollStatus {
        max_scroll,
        offset_from_bottom,
    };
    app.line_mode.set_scroll_status(scroll_status);
    app.line_mode.put_row_heights(row_heights);

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
            let raw_bytes = entry.wire_bytes();
            let display_text = entry_display_text(entry, app.settings().line_mode.display_mode);
            let disp_map = entry_display_to_wire_map(entry, app.settings().line_mode.display_mode);

            let target_raw_byte = if is_search_active {
                app.line_mode
                    .search()
                    .matches
                    .get(active_match_idx)
                    .map(|m| m.byte_offset)
                    .unwrap_or(0)
            } else if app.line_mode.copy_state().side == CopySide::Raw {
                app.line_mode.copy_state().cursor.byte_idx
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

    if app.settings().ui.mouse {
        map_text_rows(
            app,
            &layout,
            end_idx - start_idx,
            start_idx,
            paragraph_y_offset,
            line_x_offset,
            content_area,
        );
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

    let partial_idx =
        (layout.line_layouts.len() > end_idx - start_idx).then_some(end_idx - start_idx);
    let marks = row_marks(&layout, partial_idx, paragraph_y_offset, content_height);

    let mut visible_rows = layout.visual_lines;
    let first_row = paragraph_y_offset.min(visible_rows.len());
    let last_row = (first_row + content_height).min(visible_rows.len());
    visible_rows.truncate(last_row);
    visible_rows.drain(..first_row);
    let history_paragraph = Paragraph::new(visible_rows)
        .style(*theme.line_mode.history)
        .scroll((0, line_x_offset.min(u16::MAX as usize) as u16));
    frame.render_widget(history_paragraph, content_area);
    draw_row_marks(
        app,
        frame.buffer_mut(),
        content_area,
        &marks,
        line_x_offset,
        theme,
    );

    apply_keyword_highlights(frame.buffer_mut(), content_area, highlights);

    if app.line_mode.copy_state().active {
        let copy_state = app.line_mode.copy_state();
        let cursor_line = copy_state.cursor.line_idx;
        let cursor_char = copy_state.cursor.char_idx;

        let indent_width = continuation_indent_width(app.settings());

        if cursor_line >= start_idx
            && cursor_line < end_idx
            && let Some(line_meta) = layout.line_layouts.get(cursor_line - start_idx)
        {
            let mut resolved_row = line_meta.first_text_row();
            let mut resolved_col = 0;
            let mut remaining = cursor_char;

            for (sub_idx, sub) in line_meta.sub_lines.iter().enumerate() {
                let is_last_sub_line = sub_idx == line_meta.sub_lines.len().saturating_sub(1);

                if remaining < sub.content_char_len || is_last_sub_line {
                    resolved_row = line_meta.first_text_row() + sub_idx;

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
                        if copy_state.side == CopySide::Raw {
                            // the real cursor is in the raw panel, the text shows its ghost
                            // (MAX-71)
                            buf[(screen_x, screen_y)].set_style(*theme.ui.cursor_ghost);
                        } else {
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
    }

    if let Some(sb_area) = scrollbar_area {
        let has_borders = app.show_borders();

        // in rows (#5d): the thumb moves one step per row scrolled
        if total_rows > content_height {
            let scrollbar_pos = max_scroll - offset_from_bottom;
            let mut scrollbar_state = ScrollbarState::new(max_scroll)
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
            // for the mouse (MAX-142)
            app.frame_map_mut().scrollbar = Some(ScrollbarMap {
                area: sb_area,
                max: max_scroll,
                position: scrollbar_pos,
                view: content_height,
            });
        } else if !has_borders {
            let blank = Paragraph::new(" ");
            frame.render_widget(blank, sb_area);
        }
    }

    if app.line_mode.search().active {
        let (targets, query) = render_search_bar(app, frame, content_area, theme);
        let map = app.frame_map_mut();
        map.click_targets.extend(targets);
        map.search_query = Some(query);
    }

    PostRenderEffect::None
}

pub fn render_input_prompt(app: &mut App, frame: &mut Frame, area: Rect, theme: &ThemeConfig) {
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

    let has_borders = app.show_borders();

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

    let text_area = Rect {
        x: inner_area.x + prefix_width,
        width: inner_area.width.saturating_sub(prefix_width),
        ..inner_area
    };
    let (scroll, hidden_left, hidden_right) = prompt_view(
        input.value().width(),
        input.visual_cursor(),
        text_area.width as usize,
        app.line_mode.prompt_scroll(),
    );
    let prefix_style = Style::default()
        .fg(theme.palette.main)
        .add_modifier(Modifier::BOLD);

    if let Some(b) = block {
        frame.render_widget(b, area);
    }

    // the prefix stays, only the text scrolls
    let prefix_widget =
        Paragraph::new(Span::styled(prompt_prefix, prefix_style)).style(*theme.line_mode.prompt);
    frame.render_widget(prefix_widget, inner_area);
    let input_widget = Paragraph::new(input.value())
        .style(*theme.line_mode.prompt)
        .scroll((0, scroll as u16));
    frame.render_widget(input_widget, text_area);

    // marks on the edges hiding text (MAX-145)
    let settings = &app.settings().line_mode;
    let buf = frame.buffer_mut();
    if hidden_left {
        buf[(text_area.x, text_area.y)]
            .set_symbol(&settings.prompt_hidden_left)
            .set_style(prefix_style);
    }
    if hidden_right {
        buf[(text_area.right() - 1, text_area.y)]
            .set_symbol(&settings.prompt_hidden_right)
            .set_style(prefix_style);
    }

    app.line_mode.set_prompt_scroll(scroll);
    // where the text is, for mouse clicks (MAX-113)
    app.frame_map_mut().prompt = Some(PromptMap { text_area, scroll });

    let input = app.line_mode.input();
    if !app.line_mode.search().active && !app.line_mode.copy_state().active {
        let cursor_x = text_area.x + (input.visual_cursor().saturating_sub(scroll)) as u16;
        let cursor_y = inner_area.y;

        if !app.is_app_focused() {
            let buf = frame.buffer_mut();
            buf[(cursor_x, cursor_y)].set_style(*theme.ui.cursor_unfocused);
            buf[(cursor_x, cursor_y)].set_symbol(&app.settings().line_mode.unfocused_cursor_glyph);
        }
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

/// What starts a screen row of the line view: an entry's first row, a wrapped row, the
/// partial RX line's first row, or no text
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowMark {
    Line,
    Wrap,
    Partial,
    Empty,
}

/// The mark of each screen row, from the window row on the view's top line. `partial_idx` is
/// the partial RX line's place in `layout.line_layouts`, when it is drawn
fn row_marks(
    layout: &PreparedLayout,
    partial_idx: Option<usize>,
    first_row: usize,
    height: usize,
) -> Vec<RowMark> {
    (first_row..first_row + height)
        .map(|window_row| {
            if window_row >= layout.visual_lines.len() {
                return RowMark::Empty;
            }
            let local = layout
                .line_layouts
                .partition_point(|meta| meta.visual_row_start <= window_row)
                - 1;
            let first_text_row = layout.line_layouts[local].first_text_row();
            if window_row < first_text_row {
                RowMark::Empty
            } else if window_row > first_text_row {
                RowMark::Wrap
            } else if partial_idx == Some(local) {
                RowMark::Partial
            } else {
                RowMark::Line
            }
        })
        .collect()
}

/// Line mode's marks over the drawn text (MAX-163): with `prefix_place = "border"` and no
/// timestamps, the wrapped and partial rows' glyphs in the left border cell (the plain border
/// marks a line); and the separator on the rows with no text, so it runs down to the bottom of
/// the pane. It sits where the prefixes start, after the timestamp, moved by the pan
fn draw_row_marks(
    app: &App,
    buf: &mut Buffer,
    content_area: Rect,
    marks: &[RowMark],
    x_offset: usize,
    theme: &ThemeConfig,
) {
    let settings = app.settings();
    let rows = || marks.iter().zip(content_area.rows());

    if settings.markers_in_border() && app.show_borders() && content_area.x > 0 {
        let x = content_area.x - 1;
        for (mark, row) in rows() {
            match mark {
                RowMark::Wrap => {
                    buf[(x, row.y)].set_symbol(BORDER_WRAP);
                }
                RowMark::Partial => {
                    buf[(x, row.y)]
                        .set_symbol(BORDER_PARTIAL)
                        .set_style(*theme.line_mode.line_partial_prefix);
                }
                RowMark::Line | RowMark::Empty => {}
            }
        }
    }

    let prefixes = settings.line_prefixes();
    if prefixes.separator.is_empty() {
        return;
    }
    let timestamp_width = settings
        .line_gutter_width()
        .saturating_sub(prefixes.line.width());
    let Some(col) = timestamp_width.checked_sub(x_offset) else {
        return;
    };
    let style = *theme.line_mode.line_prefix;
    let width = (content_area.width as usize).saturating_sub(col);
    for (_, row) in rows().filter(|(mark, _)| **mark == RowMark::Empty) {
        buf.set_stringn(row.x + col as u16, row.y, prefixes.separator, width, style);
    }
}

/// Columns a continuation row (a wrapped entry's second row and after) starts in, before its
/// spans: where the copy cursor and a mouse click count from
fn continuation_indent_width(settings: &SettingsConfig) -> usize {
    let prefixes = settings.line_prefixes();
    match settings.line_mode.wrap_indent {
        PrefixWrapBehavior::None => 0,
        PrefixWrapBehavior::Prefix => prefixes.indent.width(),
        PrefixWrapBehavior::Timestamp => {
            let prefix_w = prefixes.line.width();
            let ts_width = if settings.show_timestamps {
                settings.line_gutter_width().saturating_sub(prefix_w)
            } else {
                0
            };
            ts_width + prefixes.indent.width()
        }
    }
}

/// Where each screen row's chars are, for mouse clicks (MAX-113): the same walk as the copy
/// cursor's, from the window row on the view's top line and the pan
fn map_text_rows(
    app: &mut App,
    layout: &PreparedLayout,
    entries_shown: usize,
    start_idx: usize,
    paragraph_y_offset: usize,
    line_x_offset: usize,
    content_area: Rect,
) {
    let indent_width = continuation_indent_width(app.settings());
    let entries = &layout.line_layouts[..entries_shown];
    let content_width = content_area.width as usize;
    let map = app.frame_map_mut();
    map.text_area = content_area;

    for screen_row in 0..content_area.height as usize {
        let window_row = paragraph_y_offset + screen_row;
        let local = entries.partition_point(|meta| meta.visual_row_start <= window_row);
        let row = local.checked_sub(1).and_then(|local| {
            let meta = &entries[local];
            let line_idx = start_idx + local;
            // no text in the gap rows before the entry
            let sub_idx = window_row.checked_sub(meta.first_text_row())?;
            let Some(sub) = meta.sub_lines.get(sub_idx) else {
                // an empty entry still takes a row
                return (sub_idx == 0).then_some(TextRow {
                    line_idx,
                    chars: RowChars::Cols {
                        first: 0,
                        cols: Vec::new(),
                    },
                });
            };
            let first: usize = meta.sub_lines[..sub_idx]
                .iter()
                .map(|sub| sub.content_char_len)
                .sum();
            let mut col = if sub_idx > 0 { indent_width } else { 0 };
            let mut gutter_left = sub.gutter_chars;
            let mut char_idx = first;
            let mut cols = Vec::new();
            for ch in sub.spans.iter().flat_map(|span| span.content.chars()) {
                if gutter_left > 0 {
                    gutter_left -= 1;
                } else if char_idx < first + sub.content_char_len {
                    if col >= line_x_offset && col - line_x_offset < content_width {
                        cols.push(((col - line_x_offset) as u16, char_idx));
                    }
                    char_idx += 1;
                } else {
                    break;
                }
                col += UnicodeWidthChar::width(ch).unwrap_or(0);
            }
            Some(TextRow {
                line_idx,
                chars: RowChars::Cols { first, cols },
            })
        });
        map.text_rows.push(row);
    }
}

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
    // copy cursor on the raw side: its byte's panel row and column, before the pan (MAX-71)
    let copy_state = app.line_mode.copy_state();
    let raw_side = copy_state.side == CopySide::Raw;
    let raw_cursor = (copy_state.active && raw_side)
        .then_some((copy_state.cursor.line_idx, copy_state.cursor.byte_idx));
    let mut raw_cursor_cell = None;
    // copy cursor on the text side: its ghost in the panel
    let raw_ghost = (copy_state.active && !raw_side)
        .then_some((copy_state.cursor.line_idx, copy_state.cursor.char_idx));
    let mut current_visual_row = 0;
    // the bytes of each panel row, for mouse clicks (MAX-113)
    let map_rows = app.settings().ui.mouse;
    let mut raw_rows = Vec::new();

    for (local_layout_idx, entry_layout) in layout.line_layouts.iter().enumerate() {
        let global_idx = start_idx + local_layout_idx;

        let visual_sub_count = entry_layout.sub_lines.len();
        let entry_visual_end = current_visual_row + entry_layout.gap + visual_sub_count;

        if entry_visual_end <= visible_visual_start {
            current_visual_row = entry_visual_end;
            continue;
        }
        if current_visual_row >= visible_visual_end {
            break;
        }

        // the gap rows before the entry are blank in the panel too
        for gap_row in current_visual_row..current_visual_row + entry_layout.gap {
            if (visible_visual_start..visible_visual_end).contains(&gap_row) {
                visible_raw_lines.push(Line::default());
                if map_rows {
                    raw_rows.push(None);
                }
            }
        }
        let first_text_row = current_visual_row + entry_layout.gap;

        if let Some(entry) = history.get(global_idx) {
            let raw_bytes = entry.wire_bytes();
            let display_text = entry_display_text(entry, app.settings().line_mode.display_mode);

            // Display text character -> byte mapping:
            let mut disp_char_to_byte = Vec::with_capacity(display_text.chars().count() + 1);
            for (b_idx, _) in display_text.char_indices() {
                disp_char_to_byte.push(b_idx);
            }
            disp_char_to_byte.push(display_text.len());

            // Map from display byte -> raw wire byte:
            let disp_map = entry_display_to_wire_map(entry, app.settings().line_mode.display_mode);

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
                let line_y = first_text_row + sub_idx;
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

                        let raw_from_byte = if raw_side {
                            // a raw side selection is a byte range (MAX-71)
                            if is_first_line { sel_start.byte_idx } else { 0 }
                        } else if !is_first_line || sel_start.char_idx == 0 {
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
                        } else if raw_side {
                            sel_end.byte_idx + 1
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

                    // 3. Ghost of the text cursor: its char's first byte (MAX-71)
                    if let Some((ghost_line, ghost_char)) = raw_ghost
                        && ghost_line == global_idx
                    {
                        let disp_b = disp_char_to_byte
                            .get(ghost_char)
                            .copied()
                            .unwrap_or(display_text.len());
                        let ghost_byte = map_disp_byte_to_raw(disp_b, &disp_map, raw_bytes.len())
                            .min(raw_bytes.len().saturating_sub(1));
                        if ghost_byte >= raw_byte_start
                            && (ghost_byte < raw_byte_end || is_last_sub)
                        {
                            highlights.push(CharHighlight {
                                start_col: ghost_byte - raw_byte_start,
                                end_col: ghost_byte - raw_byte_start + 1,
                                style: *theme.ui.cursor_ghost,
                            });
                        }
                    }

                    if let Some((cursor_line, cursor_byte)) = raw_cursor
                        && cursor_line == global_idx
                        && (cursor_byte < raw_byte_end || is_last_sub)
                        && cursor_byte >= raw_byte_start
                    {
                        let col = (cursor_byte - raw_byte_start) * raw_view.stride();
                        raw_cursor_cell = Some((visible_raw_lines.len(), col));
                    }
                    if map_rows {
                        raw_rows.push(Some(RawRow {
                            line_idx: global_idx,
                            byte_start: raw_byte_start,
                            byte_count: sub_raw_bytes.len(),
                        }));
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
        .update_raw_limits(max_visible_raw_width.saturating_sub(usable_width), raw_view);

    let raw_pan_x = app.line_mode.raw_pan_x().min(app.line_mode.raw_pan_x_max());

    if map_rows {
        let map = app.frame_map_mut();
        map.raw_area = inner_area;
        map.raw_rows = raw_rows;
        map.raw_pan_x = raw_pan_x;
        map.raw_stride = raw_view.stride();
    }

    if let Some((row, col)) = raw_cursor_cell
        && col >= raw_pan_x
        && col - raw_pan_x < usable_width
    {
        let x = inner_area.x + (col - raw_pan_x) as u16;
        frame.set_cursor_position((x, inner_area.y + row as u16));
    }

    render_raw_panel_widget(
        frame,
        raw_area,
        inner_area,
        block,
        visible_raw_lines,
        raw_pan_x,
    );
}

#[cfg(test)]
#[path = "../tests/ui/lines.rs"]
mod tests;
