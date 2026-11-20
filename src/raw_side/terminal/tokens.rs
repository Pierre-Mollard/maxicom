//! The received bytes cut into tokens (MAX-252, kept for MAX-256)
//!
//! Text, control bytes, escape sequences and invalid bytes, each with its span in the stream,
//! so the emulator can be fed run by run and each printable run's cells recorded. The sequences follow ECMA-48 as `vte` (vt100's parser) reads them; `vte`
//! itself says what it read, not where, so the spans come from this small state machine.
//! A token cut between two reads is carried to the next one

use crate::configs::TextEncoding;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub start: u64,
    pub end: u64,
    pub kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Characters drawn on the screen
    Text,
    /// A C0 control byte, DEL, or a C1 control byte in Latin1
    Control(u8),
    /// An escape sequence, cancelled ones included
    Sequence(Effect),
    /// A broken character: shown `�` (one per maximal subpart in UTF-8, MAX-249)
    Invalid,
}

/// What a sequence does: a jump or a clear, or only a style (SGR, `m`: the cursor stays, so a
/// run after it starts where the run before ended, MAX-256)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Other,
    Style,
    /// An absolute cursor move: CUP / HVP (`H`, `f`), VPA (`d`)
    CursorJump,
    /// A clear: ED 2 or 3, RIS (`ESC c`), the alternate screen in or out
    Clear,
    /// A DCS, SOS, PM or APC string: vt100 drops it, no callback says so (MAX-264)
    String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    /// After ESC
    Escape,
    /// ESC then intermediate bytes
    EscapeIntermediate,
    Csi,
    /// OSC: ends with BEL or ST
    Osc,
    /// DCS, SOS, PM, APC: end with ST
    String,
    /// ESC inside an OSC or a string: ST if `\` comes
    StringEscape,
}

const ESC: u8 = 0x1B;
const BEL: u8 = 0x07;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1A;

#[derive(Debug)]
pub struct Tokenizer {
    encoding: TextEncoding,
    state: State,
    /// Where the token being read starts (a sequence, or a UTF-8 char)
    start: u64,
    /// Where the text run before it started
    text_start: Option<u64>,
    /// A UTF-8 char being read: the bytes still expected, and the range of the next one
    utf8_left: u8,
    utf8_next: (u8, u8),
    /// A CSI's parameter and intermediate bytes, for its effect
    csi: Vec<u8>,
    /// The string being read is a DCS, SOS, PM or APC, not an OSC
    string: bool,
}

impl Tokenizer {
    pub fn encoding(&self) -> TextEncoding {
        self.encoding
    }

    pub fn new(encoding: TextEncoding) -> Self {
        Self {
            encoding,
            state: State::Ground,
            start: 0,
            text_start: None,
            utf8_left: 0,
            utf8_next: (0x80, 0xBF),
            csi: Vec::new(),
            string: false,
        }
    }

    /// Cuts `bytes`, the first one at `offset`, into the tokens they end. Text is given up to
    /// the end of `bytes` (two pieces of one run are two tokens); a sequence or a char not
    /// complete waits for the next call
    pub fn feed(&mut self, bytes: &[u8], offset: u64, out: &mut Vec<Token>) {
        let mut i = 0;
        while i < bytes.len() {
            // most of a stream is printable ASCII: a run of it is text, skipped at once
            if self.state == State::Ground && self.utf8_left == 0 {
                let run = bytes[i..]
                    .iter()
                    .take_while(|b| (0x20..0x7F).contains(*b))
                    .count();
                if run > 0 {
                    self.text_start.get_or_insert(offset + i as u64);
                    i += run;
                    continue;
                }
            }
            self.byte(bytes[i], offset + i as u64, out);
            i += 1;
        }
        // the text so far, a char being read kept for later (it joins the next run)
        if self.state == State::Ground {
            let end = offset + bytes.len() as u64;
            let text_end = if self.utf8_left > 0 { self.start } else { end };
            self.flush_text(text_end, out);
        }
    }

    // NOTE: review from here

    fn byte(&mut self, byte: u8, at: u64, out: &mut Vec<Token>) {
        match self.state {
            State::Ground => self.ground(byte, at, out),
            State::Escape => match byte {
                b'[' => self.enter(State::Csi),
                b']' => {
                    self.state = State::Osc;
                    self.string = false;
                }
                b'P' | b'X' | b'^' | b'_' => {
                    self.state = State::String;
                    self.string = true;
                }
                0x20..=0x2F => self.state = State::EscapeIntermediate,
                0x30..=0x7E => {
                    let effect = if byte == b'c' {
                        Effect::Clear
                    } else {
                        Effect::Other
                    };
                    self.close(at + 1, effect, out);
                }
                _ => self.interrupt(byte, at, out),
            },
            State::EscapeIntermediate => match byte {
                0x20..=0x2F => {}
                0x30..=0x7E => self.close(at + 1, Effect::Other, out),
                _ => self.interrupt(byte, at, out),
            },
            State::Csi => match byte {
                0x20..=0x3F => self.csi.push(byte),
                0x40..=0x7E => {
                    let effect = csi_effect(&self.csi, byte);
                    self.close(at + 1, effect, out);
                }
                _ => self.interrupt(byte, at, out),
            },
            State::Osc | State::String => match byte {
                BEL if self.state == State::Osc => self.close(at + 1, Effect::Other, out),
                ESC => self.state = State::StringEscape,
                CAN | SUB => self.cancel(byte, at, self.string_effect(), out),
                _ => {}
            },
            State::StringEscape => match byte {
                b'\\' => self.close(at + 1, self.string_effect(), out),
                // the ESC started a new sequence: the string ended before it
                _ => {
                    self.close(at - 1, self.string_effect(), out);
                    self.start = at - 1;
                    self.state = State::Escape;
                    self.byte(byte, at, out);
                }
            },
        }
    }

    fn ground(&mut self, byte: u8, at: u64, out: &mut Vec<Token>) {
        if self.utf8_left > 0 {
            let (low, high) = self.utf8_next;
            if (low..=high).contains(&byte) {
                self.utf8_left -= 1;
                self.utf8_next = (0x80, 0xBF);
                if self.utf8_left == 0 {
                    self.text_start.get_or_insert(self.start);
                }
                return;
            }
            // the char is broken before this byte, which is read again on its own
            self.utf8_left = 0;
            self.flush_text(self.start, out);
            out.push(token(self.start, at, Kind::Invalid));
        }
        match byte {
            ESC => {
                self.flush_text(at, out);
                self.start = at;
                self.state = State::Escape;
            }
            0x00..=0x1F | 0x7F => {
                self.flush_text(at, out);
                out.push(token(at, at + 1, Kind::Control(byte)));
            }
            0x20..=0x7E => {
                self.text_start.get_or_insert(at);
            }
            _ => match self.encoding {
                // Latin1's 80..9F are the C1 controls, CP437 draws them
                TextEncoding::Latin1 if byte <= 0x9F => {
                    self.flush_text(at, out);
                    out.push(token(at, at + 1, Kind::Control(byte)));
                }
                TextEncoding::Latin1 | TextEncoding::Cp437 => {
                    self.text_start.get_or_insert(at);
                }
                TextEncoding::Ascii => self.invalid(at, at + 1, out),
                TextEncoding::Utf8 => match utf8_lead(byte) {
                    Some((left, next)) => {
                        self.start = at;
                        self.utf8_left = left;
                        self.utf8_next = next;
                    }
                    None => self.invalid(at, at + 1, out),
                },
            },
        }
    }

    fn invalid(&mut self, start: u64, end: u64, out: &mut Vec<Token>) {
        self.flush_text(start, out);
        out.push(token(start, end, Kind::Invalid));
    }

    /// The text run before `end`, given as a token
    fn flush_text(&mut self, end: u64, out: &mut Vec<Token>) {
        if let Some(start) = self.text_start.take().filter(|&start| start < end) {
            out.push(token(start, end, Kind::Text));
        }
    }

    fn enter(&mut self, state: State) {
        self.csi.clear();
        self.state = state;
    }

    /// The sequence read so far ends at `end`
    fn close(&mut self, end: u64, effect: Effect, out: &mut Vec<Token>) {
        out.push(token(self.start, end, Kind::Sequence(effect)));
        self.state = State::Ground;
    }

    /// The effect of the OSC or string being read
    fn string_effect(&self) -> Effect {
        if self.string {
            Effect::String
        } else {
            Effect::Other
        }
    }

    /// CAN and SUB cancel the sequence and are controls of their own
    fn cancel(&mut self, byte: u8, at: u64, effect: Effect, out: &mut Vec<Token>) {
        self.close(at, effect, out);
        out.push(token(at, at + 1, Kind::Control(byte)));
    }

    /// A byte that can't go on in a sequence: ESC starts another one, CAN and SUB cancel it,
    /// another C0 control goes in it (the parser runs it there), anything else ends it and
    /// is read again
    fn interrupt(&mut self, byte: u8, at: u64, out: &mut Vec<Token>) {
        match byte {
            ESC => {
                self.close(at, Effect::Other, out);
                self.start = at;
                self.state = State::Escape;
            }
            CAN | SUB => self.cancel(byte, at, Effect::Other, out),
            0x00..=0x1F | 0x7F => {}
            _ => {
                self.close(at, Effect::Other, out);
                self.ground(byte, at, out);
            }
        }
    }
}

fn token(start: u64, end: u64, kind: Kind) -> Token {
    Token { start, end, kind }
}

/// The bytes a UTF-8 lead byte still expects, and the range of the next one (the ranges of
/// RFC 3629, no overlong or surrogate); none for a byte that can't start a char
fn utf8_lead(byte: u8) -> Option<(u8, (u8, u8))> {
    let any = (0x80, 0xBF);
    Some(match byte {
        0xC2..=0xDF => (1, any),
        0xE0 => (2, (0xA0, 0xBF)),
        0xED => (2, (0x80, 0x9F)),
        0xE1..=0xEF => (2, any),
        0xF0 => (3, (0x90, 0xBF)),
        0xF4 => (3, (0x80, 0x8F)),
        0xF1..=0xF3 => (3, any),
        _ => return None,
    })
}

/// What a CSI does, from its parameter / intermediate bytes and its final byte
fn csi_effect(params: &[u8], last: u8) -> Effect {
    let private = params.first() == Some(&b'?');
    let plain = !params
        .iter()
        .any(|&b| matches!(b, 0x20..=0x2F | b'<'..=b'?'));
    let numbers = || {
        let digits = if private { &params[1..] } else { params };
        digits
            .split(|&b| b == b';')
            .map(|n| {
                std::str::from_utf8(n)
                    .ok()
                    .and_then(|n| n.parse::<u16>().ok())
                    .unwrap_or(0)
            })
            .collect::<Vec<_>>()
    };
    match last {
        b'm' if plain => Effect::Style,
        b'H' | b'f' | b'd' if plain => Effect::CursorJump,
        b'J' if plain && numbers().iter().any(|&n| n == 2 || n == 3) => Effect::Clear,
        b'h' | b'l' if private && numbers().iter().any(|n| [47, 1047, 1049].contains(n)) => {
            Effect::Clear
        }
        _ => Effect::Other,
    }
}

#[cfg(test)]
#[path = "../../tests/raw_side/terminal/tokens.rs"]
mod tests;
