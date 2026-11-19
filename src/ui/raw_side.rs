//! The raw side panel's parts both modes share: its frame and the bytes in a view's digits.
//! Each mode draws its own panel, in `line` and `terminal`

pub mod line;
pub mod terminal;

use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders},
};

use crate::{
    configs::{LineDisplayMode, TextEncoding, ThemeConfig, settings::RawSideOrientation},
    raw_side::{ByteClass, byte_classes},
    ui::data_display::byte_digit_spans,
};

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

/// `bytes` in the colors of their `classes`
pub fn format_raw_bytes_with_highlights(
    bytes: &[u8],
    classes: &[ByteClass],
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
                classes,
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
    let classes = byte_classes(bytes, 0..bytes.len(), TextEncoding::Utf8);
    let line = format_raw_bytes_with_highlights(
        bytes,
        &classes,
        display_mode,
        &[],
        &ThemeConfig::default(),
    );
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

#[cfg(test)]
#[path = "../tests/ui/raw_side.rs"]
mod tests;
