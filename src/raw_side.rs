//! The raw side panel: the bytes received, next to the text. Each mode has its own, as each
//! mode keeps the bytes its own way: line mode shows each history line next to its bytes and
//! scrolls with the history, terminal mode a dump of every byte received with its own scroll
//! (MAX-252). What both share is here

pub mod line;
pub mod terminal;

use std::ops::Range;

use crate::configs::{RawSideView, TextEncoding};

/// The panel's pan, by whole bytes of the view
#[derive(Debug, Default, Clone, Copy)]
pub struct RawSideScroller {
    pub pan_x: usize,
    pub max_pan_x: usize,
}

impl RawSideScroller {
    pub fn pan_left(&mut self, bytes: usize, raw_view: RawSideView) {
        let stride = raw_view.stride();
        let pan_x = self.pan_x - self.pan_x % stride;
        self.pan_x = pan_x.saturating_sub(bytes * stride);
    }

    pub fn pan_right(&mut self, bytes: usize, raw_view: RawSideView) {
        let stride = raw_view.stride();
        let pan_x = self.pan_x - self.pan_x % stride;
        self.pan_x = (pan_x + bytes * stride).min(self.max_pan_x);
    }

    /// The widest row is `overflow` columns wider than the panel: the pan can go up to the
    /// byte holding that last column (rounded up to a byte start, so it shows a whole byte)
    pub fn set_limit(&mut self, overflow: usize, raw_view: RawSideView) {
        self.max_pan_x = overflow.next_multiple_of(raw_view.stride());
    }

    pub fn center_on_byte(
        &mut self,
        raw_view: RawSideView,
        usable_panel_width: usize,
        local_byte_offset: usize,
        total_sub_bytes: usize,
    ) {
        if raw_view == RawSideView::Off || usable_panel_width == 0 {
            return;
        }

        let stride = raw_view.stride();
        let total_raw_width = total_sub_bytes.saturating_mul(stride);
        self.set_limit(total_raw_width.saturating_sub(usable_panel_width), raw_view);

        let raw_match_x = local_byte_offset * stride;
        let half_width = usable_panel_width / 2;
        let pan_x = raw_match_x.saturating_sub(half_width);
        self.pan_x = (pan_x - pan_x % stride).min(self.max_pan_x);
    }

    pub fn sync_with_cursor(
        &mut self,
        raw_view: RawSideView,
        usable_panel_width: usize,
        local_byte_offset: usize,
        total_sub_bytes: usize,
    ) {
        if raw_view == RawSideView::Off || usable_panel_width == 0 {
            return;
        }

        let stride = raw_view.stride();
        let total_raw_width = total_sub_bytes.saturating_mul(stride);
        self.set_limit(total_raw_width.saturating_sub(usable_panel_width), raw_view);

        let raw_cursor_x = local_byte_offset * stride;
        let cursor_byte_end = raw_cursor_x + stride;

        if cursor_byte_end > self.pan_x + usable_panel_width {
            self.pan_x = cursor_byte_end
                .saturating_sub(usable_panel_width)
                .next_multiple_of(stride);
        } else if raw_cursor_x < self.pan_x {
            self.pan_x = raw_cursor_x;
        }

        self.pan_x = self.pan_x.min(self.max_pan_x);
    }
}

/// What a byte is in the encoding, for its color in both panels (MAX-265)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteClass {
    Null,
    Whitespace,
    Escape,
    Control,
    Ascii,
    Text,
    Invalid,
}

impl ByteClass {
    /// The class of `b` alone: from 80 up, UTF-8 needs the bytes around (`byte_classes`)
    fn alone(b: u8, encoding: TextEncoding) -> Self {
        match b {
            0x00 => Self::Null,
            0x09 | 0x0A | 0x0D => Self::Whitespace,
            0x1B => Self::Escape,
            0x20..=0x7E => Self::Ascii,
            0x01..=0x7F => Self::Control,
            _ => match encoding {
                TextEncoding::Utf8 | TextEncoding::Ascii => Self::Invalid,
                TextEncoding::Latin1 if b <= 0x9F => Self::Control,
                TextEncoding::Latin1 | TextEncoding::Cp437 => Self::Text,
            },
        }
    }
}

/// The class of each byte of `bytes[range]`, decoded with up to 3 bytes before and after it:
/// a char cut by the range's edges is seen whole. A char not complete at the end of `bytes` is
/// text, more may come
pub fn byte_classes(bytes: &[u8], range: Range<usize>, encoding: TextEncoding) -> Vec<ByteClass> {
    let from = range.start.saturating_sub(3);
    let window = &bytes[from..(range.end + 3).min(bytes.len())];
    let mut classes: Vec<ByteClass> = window
        .iter()
        .map(|&b| ByteClass::alone(b, encoding))
        .collect();
    if encoding == TextEncoding::Utf8 {
        utf8_text(window, &mut classes);
    }
    classes.drain(..range.start - from);
    classes.truncate(range.len());
    classes
}

/// The bytes from 80 up of valid chars as text, the broken ones stay invalid (one
/// `std::str::from_utf8` pass, as `InvalidBytes::repair` cuts them)
fn utf8_text(window: &[u8], classes: &mut [ByteClass]) {
    let mut at = 0;
    loop {
        let (valid_end, broken) = match std::str::from_utf8(&window[at..]) {
            Ok(_) => (window.len(), None),
            Err(err) => (at + err.valid_up_to(), Some(err.error_len())),
        };
        for idx in at..valid_end {
            if window[idx] >= 0x80 {
                classes[idx] = ByteClass::Text;
            }
        }
        match broken {
            None => return,
            Some(Some(len)) => at = valid_end + len,
            // not complete at the end
            Some(None) => {
                classes[valid_end..].fill(ByteClass::Text);
                return;
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/raw_side.rs"]
mod tests;
