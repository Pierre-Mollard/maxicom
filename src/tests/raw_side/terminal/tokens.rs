//! Received bytes cut into tokens (MAX-252, kept for MAX-256).

use super::*;

/// Where the token still being read starts, none between tokens
fn pending(tokenizer: &Tokenizer) -> Option<u64> {
    match tokenizer.state {
        State::Ground => (tokenizer.utf8_left > 0).then_some(tokenizer.start),
        _ => Some(tokenizer.start),
    }
}

fn tokens_of(encoding: TextEncoding, bytes: &[u8]) -> (Vec<Token>, Tokenizer) {
    let mut tokenizer = Tokenizer::new(encoding);
    let mut out = Vec::new();
    tokenizer.feed(bytes, 0, &mut out);
    (out, tokenizer)
}

/// Each token as its kind and its bytes, for readable asserts
fn shown(bytes: &[u8], tokens: &[Token]) -> Vec<(Kind, Vec<u8>)> {
    tokens
        .iter()
        .map(|t| (t.kind, bytes[t.start as usize..t.end as usize].to_vec()))
        .collect()
}

/// Adjacent text tokens joined: a run cut between two reads is two tokens
fn merged(tokens: &[Token]) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::new();
    for &t in tokens {
        match out.last_mut() {
            Some(last)
                if last.kind == Kind::Text && t.kind == Kind::Text && last.end == t.start =>
            {
                last.end = t.end;
            }
            _ => out.push(t),
        }
    }
    out
}

const SEQ: Kind = Kind::Sequence(Effect::Other);
const JUMP: Kind = Kind::Sequence(Effect::CursorJump);
const CLEAR: Kind = Kind::Sequence(Effect::Clear);
const STYLE: Kind = Kind::Sequence(Effect::Style);

/// Text runs, controls and a color sequence, each with its bytes.
/// How: "ab" CR LF ESC[31m "cd" ESC[0m.
#[test]
fn text_controls_and_sequences() {
    let bytes = b"ab\r\n\x1b[31mcd\x1b[0m";
    let (tokens, tokenizer) = tokens_of(TextEncoding::Utf8, bytes);
    assert_eq!(
        shown(bytes, &tokens),
        [
            (Kind::Text, b"ab".to_vec()),
            (Kind::Control(b'\r'), b"\r".to_vec()),
            (Kind::Control(b'\n'), b"\n".to_vec()),
            (STYLE, b"\x1b[31m".to_vec()),
            (Kind::Text, b"cd".to_vec()),
            (STYLE, b"\x1b[0m".to_vec()),
        ]
    );
    assert_eq!(pending(&tokenizer), None);
}

/// Cursor jumps, clears and styles (which leave the cursor where it is, MAX-256) are told
/// apart from the other sequences.
/// How: each sequence alone.
#[test]
fn sequence_effects() {
    let cases: [(&[u8], Kind); 18] = [
        (b"\x1b[m", STYLE),
        (b"\x1b[1;38;5;208m", STYLE),
        (b"\x1b[38:2::255:0:0m", STYLE),
        (b"\x1b[>4;2m", SEQ),
        (b"\x1b[5;10H", JUMP),
        (b"\x1b[H", JUMP),
        (b"\x1b[5;10f", JUMP),
        (b"\x1b[3d", JUMP),
        (b"\x1b[2J", CLEAR),
        (b"\x1b[3J", CLEAR),
        (b"\x1b[J", SEQ),
        (b"\x1bc", CLEAR),
        (b"\x1b[?1049h", CLEAR),
        (b"\x1b[?47l", CLEAR),
        (b"\x1b[?25l", SEQ),
        (b"\x1b[1 q", SEQ),
        (b"\x1b[>c", SEQ),
        (b"\x1b7", SEQ),
    ];
    for (bytes, kind) in cases {
        let (tokens, _) = tokens_of(TextEncoding::Utf8, bytes);
        assert_eq!(shown(bytes, &tokens), [(kind, bytes.to_vec())], "{bytes:?}");
    }
}

/// OSC ends with BEL or ST; DCS with ST; an ESC starting another sequence ends a string.
/// How: a title with BEL, with ST, a DCS, and an OSC cut by ESC[31m.
#[test]
fn strings_end_with_bel_or_st() {
    let cases: [(&[u8], &[&[u8]]); 4] = [
        (b"\x1b]0;title\x07x", &[b"\x1b]0;title\x07", b"x"]),
        (b"\x1b]0;t\x1b\\x", &[b"\x1b]0;t\x1b\\", b"x"]),
        (b"\x1bPq#0\x07;1\x1b\\", &[b"\x1bPq#0\x07;1\x1b\\"]),
        (b"\x1b]0;t\x1b[31m", &[b"\x1b]0;t", b"\x1b[31m"]),
    ];
    for (bytes, expected) in cases {
        let (tokens, _) = tokens_of(TextEncoding::Utf8, bytes);
        let parts: Vec<Vec<u8>> = shown(bytes, &tokens).into_iter().map(|(_, b)| b).collect();
        assert_eq!(parts, expected, "{bytes:?}");
    }
}

/// CAN cancels a sequence and is a control of its own; ESC ESC ends the first one.
/// How: ESC[3 CAN "x", then ESC ESC[m.
#[test]
fn cancelled_and_restarted_sequences() {
    let bytes = b"\x1b[3\x18x";
    let (tokens, _) = tokens_of(TextEncoding::Utf8, bytes);
    assert_eq!(
        shown(bytes, &tokens),
        [
            (SEQ, b"\x1b[3".to_vec()),
            (Kind::Control(0x18), b"\x18".to_vec()),
            (Kind::Text, b"x".to_vec()),
        ]
    );
    let bytes = b"\x1b\x1b[m";
    let (tokens, _) = tokens_of(TextEncoding::Utf8, bytes);
    let parts: Vec<Vec<u8>> = shown(bytes, &tokens).into_iter().map(|(_, b)| b).collect();
    assert_eq!(parts, [b"\x1b".to_vec(), b"\x1b[m".to_vec()]);
}

/// Invalid UTF-8 is cut like `from_utf8_lossy` (MAX-249): one token per `�`.
/// How: the lossy table, invalid tokens counted against the `�`.
#[test]
fn invalid_utf8_like_line_mode() {
    let cases: [&[u8]; 8] = [
        b"F\xF1\x80A",
        b"F\xF1\x80\x80A",
        b"F\xF1A",
        b"F\x80\x80A",
        b"F\xC0\x80A",
        b"F\xF5\x80A",
        b"F\xE0\x80A",
        b"a\xC3\xA9\xF0\x9F\x9A\x80b",
    ];
    for bytes in cases {
        let (tokens, _) = tokens_of(TextEncoding::Utf8, bytes);
        let invalid = tokens.iter().filter(|t| t.kind == Kind::Invalid).count();
        let lossy = String::from_utf8_lossy(bytes).matches('\u{FFFD}').count();
        assert_eq!(invalid, lossy, "{bytes:02X?}");
    }
    let bytes = b"F\xF1\x80A";
    let (tokens, _) = tokens_of(TextEncoding::Utf8, bytes);
    assert_eq!(
        shown(bytes, &tokens),
        [
            (Kind::Text, b"F".to_vec()),
            (Kind::Invalid, b"\xF1\x80".to_vec()),
            (Kind::Text, b"A".to_vec()),
        ]
    );
}

/// The other encodings: ASCII marks each byte from 80 up, Latin1's 80..9F are controls,
/// CP437 draws every byte.
/// How: "a" E9 85 "b" in each.
#[test]
fn single_byte_encodings() {
    let bytes = b"a\xE9\x85b";
    let kinds = |encoding| {
        let (tokens, _) = tokens_of(encoding, bytes);
        shown(bytes, &tokens)
    };
    assert_eq!(
        kinds(TextEncoding::Ascii),
        [
            (Kind::Text, b"a".to_vec()),
            (Kind::Invalid, b"\xE9".to_vec()),
            (Kind::Invalid, b"\x85".to_vec()),
            (Kind::Text, b"b".to_vec()),
        ]
    );
    assert_eq!(
        kinds(TextEncoding::Latin1),
        [
            (Kind::Text, b"a\xE9".to_vec()),
            (Kind::Control(0x85), b"\x85".to_vec()),
            (Kind::Text, b"b".to_vec()),
        ]
    );
    assert_eq!(kinds(TextEncoding::Cp437), [(Kind::Text, bytes.to_vec())]);
}

/// A sequence or a char cut between two reads waits, and comes whole with the next one.
/// How: "ab" ESC[3 then "1m"; "a" C3 then A9 "b".
#[test]
fn tokens_cut_between_reads() {
    let mut tokenizer = Tokenizer::new(TextEncoding::Utf8);
    let mut out = Vec::new();
    tokenizer.feed(b"ab\x1b[3", 0, &mut out);
    assert_eq!(out, [token(0, 2, Kind::Text)]);
    assert_eq!(pending(&tokenizer), Some(2));
    tokenizer.feed(b"1m", 5, &mut out);
    assert_eq!(out[1..], [token(2, 7, STYLE)]);

    let mut tokenizer = Tokenizer::new(TextEncoding::Utf8);
    let mut out = Vec::new();
    tokenizer.feed(b"a\xC3", 0, &mut out);
    assert_eq!(out, [token(0, 1, Kind::Text)]);
    assert_eq!(pending(&tokenizer), Some(1));
    tokenizer.feed(b"\xA9b", 2, &mut out);
    assert_eq!(out[1..], [token(1, 4, Kind::Text)]);
}

/// Garbage that mixes text, controls, escapes and invalid bytes, the same every run
fn garbage(len: usize, seed: u32) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            match (state >> 16) % 12 {
                0 => 0x1B,
                1 => b'[',
                2 => b']',
                3 => b'm',
                4 => b';',
                5 => 0x07,
                6 => b'\\',
                7 => 0xC3,
                8 => 0xA9,
                9 => b'\n',
                _ => (state >> 8) as u8,
            }
        })
        .collect()
}

/// The tokens cover every byte in order, with no gap, up to what is still pending; and
/// reads cut anywhere give the same tokens as one read (text runs joined).
/// How: 300 garbage inputs, then 30 of them cut at every place.
#[test]
fn tokens_cover_the_bytes_and_any_cut() {
    for seed in 0..300 {
        let bytes = garbage(48, seed);
        for encoding in [
            TextEncoding::Utf8,
            TextEncoding::Ascii,
            TextEncoding::Latin1,
        ] {
            let (tokens, tokenizer) = tokens_of(encoding, &bytes);
            let mut at = 0;
            for t in &tokens {
                assert_eq!(t.start, at, "seed {seed} {encoding:?} gap before {t:?}");
                assert!(t.end > t.start, "seed {seed} empty {t:?}");
                at = t.end;
            }
            assert_eq!(
                at,
                pending(&tokenizer).unwrap_or(bytes.len() as u64),
                "seed {seed}"
            );
        }
    }
    for seed in 0..30 {
        let bytes = garbage(32, seed);
        let (whole, _) = tokens_of(TextEncoding::Utf8, &bytes);
        for cut in 0..=bytes.len() {
            let mut tokenizer = Tokenizer::new(TextEncoding::Utf8);
            let mut out = Vec::new();
            tokenizer.feed(&bytes[..cut], 0, &mut out);
            tokenizer.feed(&bytes[cut..], cut as u64, &mut out);
            assert_eq!(merged(&out), merged(&whole), "seed {seed} cut {cut}");
        }
    }
}
