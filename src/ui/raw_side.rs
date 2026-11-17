use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::{
    app::data_processing::{binary_byte_str, hex_byte_str, octal_byte_str},
    configs::{
        LineDisplayMode, ThemeConfig,
        settings::{RawSideOrientation, RawSideView},
    },
    ui::data_display::byte_digit_spans,
};

/// The dump's bytes per line come in groups of this many (MAX-252)
pub const DUMP_GROUP: usize = 8;
/// Columns between the offset and the bytes, and between the bytes and their text
const DUMP_GAP: usize = 2;

pub struct CharHighlight {
    pub start_col: usize,
    pub end_col: usize,
    pub style: Style,
}

pub fn raw_panel_layout(
    raw_area: Rect,
    orientation: RawSideOrientation,
    theme: &ThemeConfig,
) -> (Block<'static>, Rect) {
    let borders = match orientation {
        RawSideOrientation::Right => Borders::LEFT,
        RawSideOrientation::Bottom => Borders::TOP,
    };

    let block = Block::default()
        .borders(borders)
        .border_style(*theme.ui.border_panel);

    let inner_area = block.inner(raw_area);
    (block, inner_area)
}

pub fn render_raw_panel_widget(
    frame: &mut Frame,
    raw_area: Rect,
    inner_area: Rect,
    block: Block<'static>,
    lines: Vec<Line<'static>>,
    raw_pan_x: usize,
) {
    frame.render_widget(block, raw_area);

    let raw_paragraph = Paragraph::new(lines).scroll((0, raw_pan_x.min(u16::MAX as usize) as u16));

    frame.render_widget(raw_paragraph, inner_area);
}

pub fn format_raw_bytes_with_highlights(
    bytes: &[u8],
    display_mode: LineDisplayMode,
    highlights: &[CharHighlight],
    theme: &ThemeConfig,
) -> Line<'static> {
    if bytes.is_empty() {
        return Line::from("");
    }

    let mut spans = match display_mode {
        LineDisplayMode::Hex | LineDisplayMode::Octal | LineDisplayMode::Binary => {
            byte_digit_spans(
                bytes,
                display_mode,
                Style::default().fg(theme.palette.text_muted),
                theme,
            )
        }
        _ => {
            let text = String::from_utf8_lossy(bytes);
            return Line::from(text.to_string());
        }
    };

    // Apply highlights directly to the byte spans (1 byte = span 2*i):
    for h in highlights {
        let start = h.start_col.min(bytes.len());
        let end = h.end_col.min(bytes.len());
        if start < end {
            highlight_raw_byte_range(&mut spans, start, end.saturating_sub(1), h.style);
        }
    }

    Line::from(spans)
}

pub fn highlight_raw_byte_range(
    spans: &mut [Span<'static>],
    from_byte: usize,
    to_byte: usize,
    style: Style,
) {
    for byte_idx in from_byte..=to_byte {
        // one span for byte one span for spaces in between
        let span_idx = byte_idx * 2;
        if span_idx < spans.len() {
            spans[span_idx].style = style;
        }
        if byte_idx < to_byte {
            let space_idx = span_idx + 1;
            if space_idx < spans.len() {
                spans[space_idx].style = style;
            }
        }
    }
}

pub fn format_raw_bytes_to_plain_string(bytes: &[u8], display_mode: LineDisplayMode) -> String {
    let line = format_raw_bytes_with_highlights(bytes, display_mode, &[], &ThemeConfig::default());
    line.spans.iter().map(|s| s.content.as_ref()).collect()
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
#[path = "../tests/ui/raw_side.rs"]
mod tests;
