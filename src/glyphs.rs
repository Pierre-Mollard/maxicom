//! How received chars are drawn on the host terminal, the same in both modes: stand-ins for
//! invalid bytes (MAX-249), invisible chars left out (MAX-270), chars hosts may draw at another
//! width shown as a marker (MAX-271). What maxicom keeps (copy, search, raw panel) is always
//! what was received

pub mod invalid_bytes;
pub mod unsure_width;

use std::borrow::Cow;

use ratatui::{
    style::Modifier,
    text::{Line, Span},
};
use unicode_width::UnicodeWidthChar;
use unsure_width::is_unsure;

use crate::terminal::{DropKind, Dropped};

/// Drawn, dimmed, for a cell whose first char hosts may draw at another width (MAX-271): one
/// column in every width table. A wide cell's second column stays a cell of its own, written
/// as a space
pub const UNSURE_MARKER: &str = "\u{25AF}";

/// A cell's text as written to the host terminal: its first char and the marks combined with
/// it, without the invisible ones vt100 keeps in the cell (soft hyphen, zero-width space and
/// joiner, direction marks, variation selectors...) unless `zero_width` (MAX-270), and without
/// the ones of unsure width unless `unsure_width` (MAX-271). Hosts give those their own column
/// or join chars around them, each its own way, and the screen drawn drifts from vt100's cells;
/// copy and search still read them (for terminal mode)
pub fn drawn(text: &str, zero_width: bool, unsure_width: bool) -> Cow<'_, str> {
    let dropped = |c: char| (!zero_width && is_invisible(c)) || (!unsure_width && is_unsure(c));
    let mut rest = text.chars();
    let Some(first) = rest.next() else {
        return Cow::Borrowed(text);
    };
    if !rest.clone().any(dropped) {
        return Cow::Borrowed(text);
    }
    let kept = rest.filter(|&c| !dropped(c));
    Cow::Owned(std::iter::once(first).chain(kept).collect())
}

/// Unicode's default ignorable code points that have no width, the Hangul fillers left out
pub fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{AD}'
            | '\u{34F}'
            | '\u{61C}'
            | '\u{17B4}'..='\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0000}'..='\u{E0FFF}'
    )
}

/// What stands for something the emulator didn't handle, in the cell where it came (MAX-253):
/// a control byte's Unicode control picture (`␀` to `␟`, `␡` for DEL), `◊` for the rest (C1,
/// sequences, strings), all one column in every width table
pub fn marker(dropped: &Dropped) -> char {
    match (dropped.kind, dropped.bytes.first()) {
        (DropKind::Control, Some(&b)) if b < 0x20 => {
            char::from_u32(0x2400 + u32::from(b)).unwrap_or(OTHER_MARKER)
        }
        (DropKind::Control, Some(0x7F)) => '\u{2421}',
        _ => OTHER_MARKER,
    }
}

/// The marker of what has no control picture: in every common monospace font (MAX-253)
pub const OTHER_MARKER: char = '\u{25CA}';

/// A text from the device written into maxicom's own parts (a title, the status bar), as the
/// screen draws its cells: invisible chars dropped unless `zero_width`, a char of unsure width
/// as `UNSURE_MARKER` padded with spaces to its width unless `unsure_width` (MAX-275)
pub fn drawn_text(text: &str, zero_width: bool, unsure_width: bool) -> Cow<'_, str> {
    let hidden = |c: char| !zero_width && is_invisible(c);
    let marked = |c: char| !unsure_width && is_unsure(c);
    if !text.chars().any(|c| hidden(c) || marked(c)) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    for c in text.chars().filter(|&c| !hidden(c)) {
        if !marked(c) {
            out.push(c);
            continue;
        }
        let width = c.width().unwrap_or(0);
        if width > 0 {
            out.push_str(UNSURE_MARKER);
            out.push_str(&" ".repeat(width - 1));
        }
    }
    Cow::Owned(out)
}

/// The rows as written to the host terminal, as terminal mode draws its cells: invisible chars
/// dropped unless `zero_width` (MAX-270), a char of unsure width drawn as the dimmed
/// `UNSURE_MARKER` padded with spaces to its width unless `unsure_width` (MAX-271), so each row
/// keeps the columns its layout counted. Copy, search and highlights read the history (for line mode)
pub fn drawn_rows(
    rows: Vec<Line<'static>>,
    zero_width: bool,
    unsure_width: bool,
) -> Vec<Line<'static>> {
    if zero_width && unsure_width {
        return rows;
    }
    let hidden = |c: char| !zero_width && is_invisible(c);
    let marked = |c: char| !unsure_width && is_unsure(c);
    let changes = |span: &Span| span.content.chars().any(|c| hidden(c) || marked(c));
    rows.into_iter()
        .map(|mut line| {
            if line.spans.iter().any(changes) {
                let spans = std::mem::take(&mut line.spans);
                for span in spans {
                    if !changes(&span) {
                        line.spans.push(span);
                        continue;
                    }
                    let mut text = String::new();
                    for c in span.content.chars() {
                        if hidden(c) {
                            continue;
                        }
                        if !marked(c) {
                            text.push(c);
                            continue;
                        }
                        // a zero-width one is dropped, as when it's joined to a terminal cell
                        let width = c.width().unwrap_or(0);
                        if width > 0 {
                            if !text.is_empty() {
                                line.spans
                                    .push(Span::styled(std::mem::take(&mut text), span.style));
                            }
                            let dim = span.style.add_modifier(Modifier::DIM);
                            line.spans.push(Span::styled(UNSURE_MARKER, dim));
                            text.push_str(&" ".repeat(width - 1));
                        }
                    }
                    if !text.is_empty() {
                        line.spans.push(Span::styled(text, span.style));
                    }
                }
            }
            line
        })
        .collect()
}

#[cfg(test)]
#[path = "tests/glyphs.rs"]
mod tests;
