use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::{
    configs::{LineDisplayMode, ThemeConfig, settings::RawSideOrientation},
    ui::data_display::format_line_content,
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
        .border_style(*theme.border_panel);

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

pub fn format_raw_line_with_highlights(
    text: &str,
    display_mode: LineDisplayMode,
    highlights: &[CharHighlight],
) -> Line<'static> {
    if text.is_empty() {
        return Line::from("");
    }

    let mut spans = format_line_content(text, display_mode);
    let char_count = text.chars().count();

    // because some char are multiple bytes! utf8 1 to 4 bytes
    let mut char_to_byte_idx = Vec::with_capacity(char_count + 1);
    for (b_idx, _) in text.char_indices() {
        char_to_byte_idx.push(b_idx);
    }
    char_to_byte_idx.push(text.len());

    for h in highlights {
        let start = h.start_col.min(char_count);
        let end = h.end_col.min(char_count);
        if start < end {
            let byte_start = char_to_byte_idx[start];
            let byte_end = char_to_byte_idx[end];
            if byte_start < byte_end {
                highlight_raw_byte_range(
                    &mut spans,
                    byte_start,
                    byte_end.saturating_sub(1),
                    h.style,
                );
            }
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
