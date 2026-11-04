//! Wire bytes kept next to the decoded text, and how text offsets map back to them (#20).

use super::*;
use crate::configs::TextEncoding;

fn received(encoding: TextEncoding, wire: &[u8]) -> LogEntry {
    LogEntry::received(encoding.decode(wire), wire, Some(Local::now()))
}

/// Wire bytes are only stored when they differ from the text's UTF-8, and `wire_bytes`
/// always gives what was received.
/// How: valid UTF-8, Latin1, invalid UTF-8 and a locally echoed line.
#[test]
fn wire_kept_only_when_it_differs() {
    let utf8 = received(TextEncoding::Utf8, "é".as_bytes());
    assert!(utf8.wire.is_none());
    assert_eq!(utf8.wire_bytes(), "é".as_bytes());

    let latin1 = received(TextEncoding::Latin1, &[0xE9]);
    assert_eq!(latin1.text, "é");
    assert_eq!(latin1.wire_bytes(), [0xE9]);

    let invalid = received(TextEncoding::Utf8, &[0xFF]);
    assert_eq!(invalid.text, "\u{FFFD}");
    assert_eq!(invalid.wire_bytes(), [0xFF]);

    let echoed = LogEntry::new(String::from("ls"), Local::now());
    assert_eq!(echoed.wire_bytes(), b"ls");
}

/// Each text byte points to the received byte it was decoded from.
/// How: exact offsets for each encoding, including multi-byte invalid UTF-8 sequences.
#[test]
fn text_to_wire_offsets_per_encoding() {
    let offsets = |encoding, wire: &[u8]| received(encoding, wire).text_to_wire_offsets();
    // x é y: é is 1 received byte, 2 text bytes
    assert_eq!(offsets(TextEncoding::Latin1, b"x\xE9y"), [0, 1, 1, 2, 3]);
    assert_eq!(offsets(TextEncoding::Cp437, b"x\x82y"), [0, 1, 1, 2, 3]);
    // ASCII and UTF-8: an invalid byte becomes U+FFFD (3 text bytes)
    assert_eq!(offsets(TextEncoding::Ascii, b"a\xE9b"), [0, 1, 1, 1, 2, 3]);
    assert_eq!(offsets(TextEncoding::Utf8, b"a\xFFb"), [0, 1, 1, 1, 2, 3]);
    // valid é then an invalid byte: fewer chars than bytes, split like from_utf8_lossy
    assert_eq!(
        offsets(TextEncoding::Utf8, b"\xC3\xA9\xFF"),
        [0, 1, 2, 2, 2, 3]
    );
    // a cut 3-byte sequence is one U+FFFD for 2 received bytes
    assert_eq!(offsets(TextEncoding::Utf8, b"\xE2\x82a"), [0, 0, 0, 2, 3]);
    // nothing to map for valid UTF-8
    assert_eq!(offsets(TextEncoding::Utf8, "hé".as_bytes()), [0, 1, 2, 3]);
}

/// For any received bytes and encoding: one offset per text byte plus the end, never
/// decreasing, every char starting on a received byte, the end at the wire length.
/// How: 2000 lines from a fixed-seed xorshift, decoded with every encoding.
#[test]
fn text_to_wire_offsets_hold_on_generated_bytes() {
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..2000 {
        let len = (next() % 24) as usize;
        // biased towards UTF-8 lead and continuation bytes
        let wire: Vec<u8> = (0..len)
            .map(|_| match next() % 4 {
                0 => (next() % 0x80) as u8,
                1 => 0x80 | (next() % 0x40) as u8,
                _ => 0xC0 | (next() % 0x40) as u8,
            })
            .collect();
        for encoding in [
            TextEncoding::Utf8,
            TextEncoding::Ascii,
            TextEncoding::Latin1,
            TextEncoding::Cp437,
        ] {
            let entry = received(encoding, &wire);
            let offsets = entry.text_to_wire_offsets();
            let context = format!("{encoding:?} {wire:02X?}");
            assert_eq!(offsets.len(), entry.text.len() + 1, "{context}");
            assert_eq!(offsets.last(), Some(&wire.len()), "{context}");
            assert!(offsets.windows(2).all(|w| w[0] <= w[1]), "{context}");
            for (byte, _) in entry.text.char_indices() {
                assert!(offsets[byte] < wire.len(), "{context}");
            }
        }
    }
}
