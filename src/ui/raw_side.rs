use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::configs::{LineDisplayMode, ThemeConfig, settings::RawSideOrientation};

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
        LineDisplayMode::Hex => {
            let mut s = Vec::with_capacity(bytes.len() * 2);
            let mut buf = const_hex::Buffer::<1>::new();
            for (i, &b) in bytes.iter().enumerate() {
                let color = theme.byte_color(b);
                let hex_str = buf.format_upper(&[b]);
                s.push(Span::styled(
                    hex_str.to_string(),
                    Style::default().fg(color),
                ));
                if i + 1 < bytes.len() {
                    s.push(Span::styled(
                        " ",
                        Style::default().fg(theme.palette.text_muted),
                    ));
                }
            }
            s
        }
        LineDisplayMode::Octal => {
            let mut s = Vec::with_capacity(bytes.len() * 2);
            for (i, &b) in bytes.iter().enumerate() {
                let color = theme.byte_color(b);
                let mut octal_buf = [0u8; 3];
                octal_buf[0] = b'0' + ((b >> 6) & 0x07);
                octal_buf[1] = b'0' + ((b >> 3) & 0x07);
                octal_buf[2] = b'0' + (b & 0x07);
                let octal_str = unsafe { std::str::from_utf8_unchecked(&octal_buf) };
                s.push(Span::styled(
                    octal_str.to_string(),
                    Style::default().fg(color),
                ));
                if i + 1 < bytes.len() {
                    s.push(Span::styled(
                        " ",
                        Style::default().fg(theme.palette.text_muted),
                    ));
                }
            }
            s
        }
        LineDisplayMode::Binary => {
            let mut s = Vec::with_capacity(bytes.len() * 2);
            for (i, &b) in bytes.iter().enumerate() {
                let color = theme.byte_color(b);
                let mut bin_buf = [0u8; 8];
                (0..8).for_each(|bit| {
                    bin_buf[bit] = if (b & (1 << (7 - bit))) != 0 {
                        b'1'
                    } else {
                        b'0'
                    };
                });
                let bin_str = unsafe { std::str::from_utf8_unchecked(&bin_buf) };
                s.push(Span::styled(
                    bin_str.to_string(),
                    Style::default().fg(color),
                ));
                if i + 1 < bytes.len() {
                    s.push(Span::styled(
                        " ",
                        Style::default().fg(theme.palette.text_muted),
                    ));
                }
            }
            s
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
