//! Stand-ins for invalid bytes (MAX-249): shown like line mode, decoded back to the bytes.

use super::*;

/// The text with each stand-in drawn as `�`, as terminal mode shows it
fn shown_text(out: &[u8]) -> String {
    let text = std::str::from_utf8(out).expect("the output is valid UTF-8");
    shown(text).into_owned()
}

/// The bytes the stand-ins hold, the rest as it is, through [`received`]
fn decoded(out: &[u8]) -> Vec<u8> {
    let text = std::str::from_utf8(out).expect("the output is valid UTF-8");
    received(text).map_or_else(|| out.to_vec(), |(bytes, _)| bytes)
}

/// Garbage that mixes ASCII, valid chars, escapes and invalid bytes, the same every run
fn garbage(len: usize, seed: u32) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            match (state >> 16) % 8 {
                0 => b'a',
                1 => 0x1B,
                2 => 0xC3,
                3 => 0xA9,
                4 => 0xF0,
                5 => 0x9F,
                _ => (state >> 8) as u8,
            }
        })
        .collect()
}

/// Valid UTF-8 comes back as it is, without a copy.
/// How: ASCII, "é", an escape sequence and a rocket.
#[test]
fn valid_utf8_is_borrowed() {
    let input = "a é \x1b[31m 🚀".as_bytes();
    let out = InvalidBytes::default().utf8(input);
    assert!(matches!(out, Cow::Borrowed(_)));
    assert_eq!(&*out, input);
}

/// One `�` per broken char, as `String::from_utf8_lossy` (line mode) cuts them.
/// How: the cases of the lossy table, each shown and compared.
#[test]
fn broken_chars_are_cut_like_line_mode() {
    let cases: [(&[u8], &str); 8] = [
        (b"F\xF1\x80A", "F\u{FFFD}A"),
        (b"F\xF1\x80\x80A", "F\u{FFFD}A"),
        (b"F\xF1A", "F\u{FFFD}A"),
        (b"F\x80\x80A", "F\u{FFFD}\u{FFFD}A"),
        (b"F\xC0\x80A", "F\u{FFFD}\u{FFFD}A"),
        (b"F\xF5\x80A", "F\u{FFFD}\u{FFFD}A"),
        (b"F\xE0\x80A", "F\u{FFFD}\u{FFFD}A"),
        (b"F\x85A", "F\u{FFFD}A"),
    ];
    for (input, expected) in cases {
        let out = InvalidBytes::default().utf8(input);
        assert_eq!(shown_text(&out), expected, "{input:02X?}");
        assert_eq!(
            shown_text(&out),
            String::from_utf8_lossy(input),
            "{input:02X?}"
        );
    }
}

/// No byte is lost: the stand-ins decode back to the bytes received, and the text shown is
/// line mode's.
/// How: 200 pseudo-random garbage inputs of 64 bytes.
#[test]
fn garbage_decodes_back_to_its_bytes() {
    for seed in 0..200 {
        let mut input = garbage(64, seed);
        // a char cut at the very end waits for the next read: end on ASCII to see it all
        input.push(b'.');
        let out = InvalidBytes::default().utf8(&input);
        assert_eq!(decoded(&out), input, "seed {seed}");
        assert_eq!(
            shown_text(&out),
            String::from_utf8_lossy(&input),
            "seed {seed}"
        );
    }
}

/// A read can end anywhere: two reads give what one read gives.
/// How: garbage cut at every place, each half fed in turn.
#[test]
fn any_cut_between_reads_gives_the_same() {
    for seed in 0..20 {
        let mut input = garbage(32, seed);
        input.push(b'.');
        let whole = InvalidBytes::default().utf8(&input).into_owned();
        for cut in 0..=input.len() {
            let mut bytes = InvalidBytes::default();
            let mut out = bytes.utf8(&input[..cut]).into_owned();
            out.extend_from_slice(&bytes.utf8(&input[cut..]));
            assert_eq!(out, whole, "seed {seed} cut {cut}");
        }
    }
}

/// A char cut at the end of a read waits, and comes whole with the next one; a broken char
/// cut the same way comes as its stand-in once the next byte shows it is broken.
/// How: "a" C3 then A9 "b"; "a" F1 then "A".
#[test]
fn a_cut_char_waits_for_the_next_read() {
    let mut bytes = InvalidBytes::default();
    assert_eq!(&*bytes.utf8(b"a\xC3"), b"a");
    assert_eq!(&*bytes.utf8(b"\xA9b"), "éb".as_bytes());

    let mut bytes = InvalidBytes::default();
    assert_eq!(&*bytes.utf8(b"a\xF1"), b"a");
    let out = bytes.utf8(b"A");
    assert_eq!(shown_text(&out), "\u{FFFD}A");
    assert_eq!(decoded(&out), b"\xF1A");
}

/// An escape sequence right after a broken char stays whole, for the emulator.
/// How: F1 then ESC[31m "x".
#[test]
fn escape_after_a_broken_char() {
    let out = InvalidBytes::default().utf8(b"\xF1\x1b[31mx");
    assert_eq!(shown_text(&out), "\u{FFFD}\x1b[31mx");
}

/// Each broken char is one char, so a char index stays a column.
/// How: F1 80 "A", E0 80 "A", 80: one char per `�`.
#[test]
fn one_char_per_broken_char() {
    for input in [&b"\xF1\x80\x80A"[..], b"\xF1\x80A", b"\xE0\x80A", b"\x80A"] {
        let out = InvalidBytes::default().utf8(input);
        let text = std::str::from_utf8(&out).unwrap();
        let lossy = String::from_utf8_lossy(input);
        assert_eq!(text.chars().count(), lossy.chars().count(), "{input:02X?}");
    }
}

/// A text without stand-ins is shown as it is, without a copy; a real plane 16 char past the
/// stand-ins stays.
/// How: "aé🚀", then U+10FFFD.
#[test]
fn shown_leaves_other_text() {
    assert!(matches!(shown("aé🚀"), Cow::Borrowed(_)));
    assert_eq!(shown("a\u{10FFFD}"), "a\u{10FFFD}");
}

/// ASCII: each byte from 80 up is a broken char of its own, the rest as it is.
/// How: "a" 80 FF "b", then plain ASCII borrowed.
#[test]
fn ascii_marks_every_high_byte() {
    let out = ascii(b"a\x80\xFFb\x7F");
    assert_eq!(shown_text(&out), "a\u{FFFD}\u{FFFD}b\x7F");
    assert_eq!(decoded(&out), b"a\x80\xFFb\x7F");
    assert!(matches!(ascii(b"plain\x1b[0m"), Cow::Borrowed(_)));
}

/// `received` gives the bytes back, and for each byte of the shown text the byte it comes
/// from: the 3 bytes of a `�` all go to its broken char's first byte.
/// How: "a" F1 80 "b" shown a�b (1 + 3 + 1 bytes), bytes 61 F1 80 62.
#[test]
fn received_maps_the_shown_text() {
    let out = InvalidBytes::default().utf8(b"a\xF1\x80b").into_owned();
    let text = std::str::from_utf8(&out).unwrap();
    assert_eq!(shown(text), "a\u{FFFD}b");
    let (bytes, offsets) = received(text).unwrap();
    assert_eq!(bytes, b"a\xF1\x80b");
    assert_eq!(offsets, [0, 1, 1, 1, 3, 4]);
    assert_eq!(received("plain é"), None);
}

/// MAX-256: a `�` received as such (`EF BF BD`, which vt100 drops too) gets a stand-in of its
/// own: shown as `�`, decoded back to its bytes, also cut between two reads; the rest of the
/// valid text is kept.
/// How: "a" EF BF BD "b" in one read, then "a" EF BF | BD "b"
#[test]
fn a_received_replacement_char_is_kept() {
    for reads in [&[&b"a\xEF\xBF\xBDb"[..]][..], &[b"a\xEF\xBF", b"\xBDb"]] {
        let mut stand_ins = InvalidBytes::default();
        let out: Vec<u8> = reads
            .iter()
            .flat_map(|read| stand_ins.utf8(read).into_owned())
            .collect();
        assert_eq!(shown_text(&out), "a\u{FFFD}b", "{reads:?}");
        assert_eq!(decoded(&out), b"a\xEF\xBF\xBDb", "{reads:?}");
    }
}
