//! Terminal mode's raw side panel: the dump of every byte received (MAX-252)

use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, ScrollbarState},
};

use crate::{
    app::{
        App,
        data_processing::{binary_byte_str, hex_byte_str, octal_byte_str},
        types::{RawRows, ScrollbarMap},
    },
    configs::{RawSideView, ThemeConfig},
    ui::{common::scrollbar, raw_side::raw_panel_layout},
};

/// The dump's bytes per line come in groups of this many (MAX-252)
pub const DUMP_GROUP: usize = 8;
/// Columns between the offset and the bytes, and between the bytes and their text
const DUMP_GAP: usize = 2;

/// The raw panel's dump of the bytes received (MAX-252): the offset column pinned, the bytes
/// and their text panned, the lines following the live end unless scrolled, a scrollbar in
/// its right column. In copy mode on the raw side, its cursor and selection
pub fn render(app: &mut App, frame: &mut Frame, raw_area: Rect, theme: &ThemeConfig) {
    let raw_view = app.settings().raw_side.view;
    if raw_view == RawSideView::Off || raw_area.width == 0 || raw_area.height == 0 {
        return;
    }
    let (block, inner_area) =
        raw_panel_layout(raw_area, app.settings().raw_side.orientation, theme);
    frame.render_widget(block, raw_area);
    // the scrollbar's column, kept while there's nothing to scroll as the history's is
    let (inner_area, bar_area) = if app.show_scrollbar() && inner_area.width > 1 {
        let width = inner_area.width - 1;
        let bar = Rect {
            x: inner_area.x + width,
            width: 1,
            ..inner_area
        };
        (
            Rect {
                width,
                ..inner_area
            },
            Some(bar),
        )
    } else {
        (inner_area, None)
    };

    // the screen copy cursor's bytes (MAX-256), before the top line: the dump may follow them
    let encoding = app.settings().encoding;
    let (ghosts, follow) = app.terminal_mode.trace_ghosts(encoding);
    // the screen copy selection's bytes, in its look (MAX-256)
    let mirror = app.terminal_mode.selection_bytes(encoding);
    let mirror_style = theme.ui.selection_style(false);
    let dump = app.terminal_mode.dump_mut();
    let layout = DumpLayout::fit(inner_area.width as usize, raw_view, dump.stream().end());
    let rows = inner_area.height as usize;
    let top = dump.top_line(layout.bytes_per_line, rows);
    let (max, position) = dump.scroll_position();
    if let Some(bar) = bar_area
        && max > 0
    {
        let mut state = ScrollbarState::new(max)
            .viewport_content_length(rows)
            .position(position);
        frame.render_stateful_widget(scrollbar(theme), bar, &mut state);
        app.frame_map_mut().dump_scrollbar = Some(ScrollbarMap {
            area: bar,
            max,
            position,
            view: rows,
        });
    }
    let offset_width = (layout.offset_width() as u16).min(inner_area.width);
    let body_area = Rect {
        x: inner_area.x + offset_width,
        width: inner_area.width - offset_width,
        ..inner_area
    };
    app.terminal_mode
        .pan_to_dump_cursor(raw_view, body_area.width as usize);
    if follow && let Some(shown) = ghosts.first() {
        (app.terminal_mode.dump_mut()).pan_to(shown.start, raw_view, body_area.width as usize);
    }
    let overflow = layout.body_width.saturating_sub(body_area.width as usize);
    let dump = app.terminal_mode.dump_mut();
    dump.set_pan_limit(overflow, raw_view);
    let pan_x = dump.pan_x();
    let bounds = dump.bounds();
    let copy = app.terminal_mode.dump_copy().copied();
    let selection = copy.and_then(|copy| copy.selection(&bounds));
    let selection_style = theme.ui.selection_style(true);
    let search = app.terminal_mode.search();
    let shows_matches = app.terminal_mode.dump_search() && search.shows_matches();
    let matches = if shows_matches {
        &search.matches[..]
    } else {
        &[]
    };
    let current_match = matches.get(search.current_match).map(|m| m.byte_offset);
    let width = layout.bytes_per_line as u64;
    let mut offsets = Vec::with_capacity(rows);
    let mut bodies = Vec::with_capacity(rows);
    let mut bytes = Vec::with_capacity(layout.bytes_per_line);
    let mut cursor_cell = None;
    for (row, line) in (top..)
        .take(rows)
        .take_while(|&line| line * width < bounds.end)
        .enumerate()
    {
        let from = line * width;
        let (front, back) = app.terminal_mode.dump().stream().slices(from..from + width);
        bytes.clear();
        bytes.extend_from_slice(front);
        bytes.extend_from_slice(back);
        let lead = bounds.start.saturating_sub(from) as usize;
        // the line's bytes selected, and the cursor's, by their index in the line
        let local = |range: &std::ops::Range<u64>| {
            let to = range.end.clamp(from, from + width);
            (range.start.clamp(from, to) - from) as usize..(to - from) as usize
        };
        let mut marks = Vec::new();
        // a raw search's matches (MAX-252), sorted by their first byte
        let first = matches.partition_point(|m| (m.byte_offset + m.byte_len) as u64 <= from);
        let shown = matches[first..].iter();
        let mut active = None;
        for m in shown.take_while(|m| (m.byte_offset as u64) < from + width) {
            let bytes = local(&(m.byte_offset as u64..(m.byte_offset + m.byte_len) as u64));
            if Some(m.byte_offset) == current_match {
                active = Some(bytes);
            } else {
                marks.push((bytes, *theme.search.search_match));
            }
        }
        // over the matches it overlaps
        marks.extend(active.map(|bytes| (bytes, *theme.search.search_match_active)));
        for range in mirror
            .iter()
            .filter(|range| range.end > from && range.start < from + width)
        {
            marks.push((local(range), mirror_style));
        }
        // the screen copy cursor's bytes: the earlier writes on its cell, then the one it shows
        for (i, ghost) in ghosts.iter().enumerate().rev() {
            if ghost.end > from && ghost.start < from + width {
                let style = match i {
                    0 => *theme.ui.cursor_ghost,
                    _ => *theme.ui.cursor_ghost_old,
                };
                marks.push((local(ghost), style));
            }
        }
        if let Some(selected) = &selection {
            marks.push((local(selected), selection_style));
        }
        if let Some(copy) = copy
            && (from..from + width).contains(&copy.cursor)
        {
            let byte = (copy.cursor - from) as usize;
            marks.push((byte..byte + 1, *theme.ui.cursor_ghost));
            cursor_cell = Some((row, byte * raw_view.stride()));
        }
        offsets.push(layout.offset_line(line, theme));
        bodies.push(layout.body_line(lead, &bytes, raw_view, theme, &marks));
    }

    if app.settings().ui.mouse {
        let map = app.frame_map_mut();
        map.raw_area = body_area;
        map.raw_rows = RawRows::Dump {
            top_line: top,
            width: layout.bytes_per_line,
            end: bounds.end,
            text_col: layout.text_col(raw_view),
        };
        map.raw_pan_x = pan_x;
        map.raw_stride = raw_view.stride();
    }
    // the digits under the frame's cursor, the char in the text column as its ghost
    if let Some((row, col)) = cursor_cell
        && let Some(x) = col.checked_sub(pan_x)
        && x < body_area.width as usize
    {
        frame.set_cursor_position((body_area.x + x as u16, body_area.y + row as u16));
    }

    frame.render_widget(Paragraph::new(offsets), inner_area);
    let body = Paragraph::new(bodies).scroll((0, pan_x.min(u16::MAX as usize) as u16));
    frame.render_widget(body, body_area);
}

/// The terminal raw panel's dump (MAX-252), like `xxd`: per line an offset, the bytes in the
/// view's radix, their text. The offset column is pinned, the rest pans
pub struct DumpLayout {
    /// Hex digits of the offsets
    pub offset_digits: usize,
    pub bytes_per_line: usize,
    /// Columns of the bytes and their text, past the offset column
    pub body_width: usize,
}

impl DumpLayout {
    /// The layout for a panel `width` columns wide showing bytes up to offset `end`: the most
    /// bytes per line that fit, a multiple of [`DUMP_GROUP`], at least one group
    pub fn fit(width: usize, view: RawSideView, end: u64) -> Self {
        let offset_digits = (u64::BITS - end.leading_zeros()).div_ceil(4).max(8) as usize;
        let free = width.saturating_sub(offset_digits + DUMP_GAP);
        // a byte takes its digits and a space, plus its char in the text column
        let per_byte = view.stride() + 1;
        let fits = free.saturating_sub(DUMP_GAP - 1) / per_byte;
        let bytes_per_line = (fits - fits % DUMP_GROUP).max(DUMP_GROUP);
        Self {
            offset_digits,
            bytes_per_line,
            body_width: bytes_per_line * per_byte + DUMP_GAP - 1,
        }
    }

    #[inline]
    pub fn offset_width(&self) -> usize {
        self.offset_digits + DUMP_GAP
    }

    /// Line `line`'s offset, in the offset column's style
    pub fn offset_line(&self, line: u64, theme: &ThemeConfig) -> Line<'static> {
        let offset = line * self.bytes_per_line as u64;
        let text = format!("{offset:0digits$X}", digits = self.offset_digits);
        Line::from(Span::styled(
            text,
            Style::default().fg(theme.palette.text_muted),
        ))
    }

    /// The column of the text column's first char, past the offset column
    pub fn text_col(&self, view: RawSideView) -> usize {
        self.bytes_per_line * view.stride() + DUMP_GAP - 1
    }

    /// The bytes and text of a line starting `lead` bytes before `bytes` (bytes dropped from
    /// the store, drawn blank so the columns stay aligned), each byte in its class's color;
    /// `marks` style the bytes of their ranges (indexes in the line, `lead` included) in both
    /// columns, and the spaces between two bytes of a range
    pub fn body_line(
        &self,
        lead: usize,
        bytes: &[u8],
        view: RawSideView,
        theme: &ThemeConfig,
        marks: &[(std::ops::Range<usize>, Style)],
    ) -> Line<'static> {
        let digits_of = match view {
            RawSideView::Octal => octal_byte_str,
            RawSideView::Binary => binary_byte_str,
            _ => hex_byte_str,
        };
        let stride = view.stride();
        let mut spans = Vec::with_capacity(bytes.len() * 3 + 3);
        spans.push(Span::raw(" ".repeat(lead * stride)));
        for &byte in bytes {
            let style = Style::default().fg(theme.byte_color(byte));
            spans.push(Span::styled(digits_of(byte), style));
            spans.push(Span::raw(" "));
        }
        // the text column starts at the same place on every line, the last one short
        let missing = self.bytes_per_line.saturating_sub(lead + bytes.len());
        spans.push(Span::raw(
            " ".repeat(missing * stride + DUMP_GAP - 1 + lead),
        ));
        for &byte in bytes {
            let shown = if (0x20..=0x7E).contains(&byte) {
                byte as char
            } else {
                '.'
            };
            let style = Style::default().fg(theme.byte_color(byte));
            spans.push(Span::styled(shown.to_string(), style));
        }
        // spans: the lead, a digits and a space per byte, the padding, a char per byte
        let text_from = 2 + 2 * bytes.len();
        for (range, style) in marks {
            for idx in range.start.max(lead)..range.end.min(lead + bytes.len()) {
                let byte = idx - lead;
                spans[1 + 2 * byte].style = spans[1 + 2 * byte].style.patch(*style);
                if idx + 1 < range.end.min(lead + bytes.len()) {
                    spans[2 + 2 * byte].style = spans[2 + 2 * byte].style.patch(*style);
                }
                spans[text_from + byte].style = spans[text_from + byte].style.patch(*style);
            }
        }
        Line::from(spans)
    }
}

#[cfg(test)]
#[path = "../../tests/ui/raw_side/terminal.rs"]
mod tests;
