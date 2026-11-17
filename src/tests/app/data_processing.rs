//! Display strings and display-to-raw byte maps used by search, copy mode and the raw panel.

use super::*;

const ALL_MODES: [LineDisplayMode; 6] = [
    LineDisplayMode::Pretty,
    LineDisplayMode::Clean,
    LineDisplayMode::Raw,
    LineDisplayMode::Hex,
    LineDisplayMode::Octal,
    LineDisplayMode::Binary,
];

/// The compile-time digit tables agree with `format!` for every byte value.
/// How: compares all 256 bytes in each radix.
#[test]
fn byte_digit_tables_match_format() {
    for b in 0..=255u8 {
        assert_eq!(hex_byte_str(b), format!("{b:02X}"));
        assert_eq!(octal_byte_str(b), format!("{b:03o}"));
        assert_eq!(binary_byte_str(b), format!("{b:08b}"));
    }
}

/// Pretty and Clean drop ANSI styling and the trailing EOL.
/// How: compares against a colored CRLF line.
#[test]
fn displayable_pretty_and_clean_strip_ansi_and_eol() {
    let text = "\x1b[31mHi\x1b[0m there\r\n";
    assert_eq!(
        displayable_line_string(text, LineDisplayMode::Pretty),
        "Hi there"
    );
    assert_eq!(
        displayable_line_string(text, LineDisplayMode::Clean),
        "Hi there"
    );
}

/// Raw mode makes control bytes visible (`^[`, `\r`, `\xNN`, tab as spaces).
/// How: checks ESC, tab, CR, LF and DEL.
#[test]
fn displayable_raw_expands_controls() {
    assert_eq!(
        displayable_line_string("\x1b[1ma\tb\r\n", LineDisplayMode::Raw),
        "^[[1ma    b\\r\\x0A"
    );
    assert_eq!(
        displayable_line_string("\x7f", LineDisplayMode::Raw),
        "\\x7F"
    );
}

/// Hex/Octal/Binary format each byte, space separated, and empty input stays empty.
/// How: known values for "AB", plus every mode on "".
#[test]
fn displayable_numeric_modes() {
    assert_eq!(displayable_line_string("AB", LineDisplayMode::Hex), "41 42");
    assert_eq!(
        displayable_line_string("AB", LineDisplayMode::Octal),
        "101 102"
    );
    assert_eq!(
        displayable_line_string("AB", LineDisplayMode::Binary),
        "01000001 01000010"
    );
    for mode in ALL_MODES {
        assert_eq!(displayable_line_string("", mode), "", "{mode:?}");
    }
}

/// The Pretty map skips CSI sequences and the trailing CRLF.
/// How: exact map of a colored "Hi" line.
#[test]
fn map_pretty_skips_csi_and_trailing_eol() {
    // ESC [ 3 1 m H i ESC [ 0 m \r \n
    let raw = b"\x1b[31mHi\x1b[0m\r\n";
    let map = build_display_to_raw_byte_map(raw, LineDisplayMode::Pretty);
    assert_eq!(map, vec![5, 6, 11]);
}

/// The Pretty map skips OSC sequences ended by BEL.
/// How: exact map of an OSC title followed by text.
#[test]
fn map_pretty_skips_osc() {
    // ESC ] 0 ; t i t l e BEL o k
    let raw = b"\x1b]0;title\x07ok";
    let map = build_display_to_raw_byte_map(raw, LineDisplayMode::Pretty);
    assert_eq!(map, vec![10, 11, 12]);
}

/// In Raw mode every expanded display byte points back to its source byte.
/// How: exact map of "a\tb\r\n".
#[test]
fn map_raw_points_expansions_to_source_byte() {
    let raw = b"a\tb\r\n";
    let map = build_display_to_raw_byte_map(raw, LineDisplayMode::Raw);
    assert_eq!(map, vec![0, 1, 1, 1, 1, 2, 3, 3, 4, 4, 4, 4, 5]);
}

/// Invalid UTF-8 in Raw mode falls back to a 1:1 map.
/// How: map of [0xFF, 'a'].
#[test]
fn map_raw_invalid_utf8_falls_back_to_identity() {
    let map = build_display_to_raw_byte_map(&[0xff, b'a'], LineDisplayMode::Raw);
    assert_eq!(map, vec![0, 1, 2]);
}

/// Hex/Octal/Binary maps repeat each byte index once per digit, plus its separator.
/// How: exact maps for short inputs.
#[test]
fn map_numeric_modes_follow_stride() {
    assert_eq!(
        build_display_to_raw_byte_map(b"AB", LineDisplayMode::Hex),
        vec![0, 0, 0, 1, 1, 2]
    );
    assert_eq!(
        build_display_to_raw_byte_map(b"AB", LineDisplayMode::Octal),
        vec![0, 0, 0, 0, 1, 1, 1, 2]
    );
    assert_eq!(
        build_display_to_raw_byte_map(b"A", LineDisplayMode::Binary),
        vec![0, 0, 0, 0, 0, 0, 0, 0, 1]
    );
}

/// Invariant: map length == display string length + 1 (the end sentinel).
/// How: every mode over a set of samples (ANSI, UTF-8).
#[test]
fn map_len_is_display_len_plus_one() {
    let samples = [
        "",
        "plain text",
        "\x1b[31mred\x1b[0m and \x1b[1;32mgreen\x1b[0m\r\n",
        "héllo wörld ✓",
        "name\tsize\tdate",
        "10%\r50%\r100% done\r\n",
        "a\x07b\x7fc\x1bPdcs\x1b\\d",
    ];
    for text in samples {
        for mode in ALL_MODES {
            let display = displayable_line_string(text, mode);
            let map = build_display_to_raw_byte_map(text.as_bytes(), mode);
            assert_eq!(map.len(), display.len() + 1, "{mode:?} on {text:?}");
        }
    }
}

/// A tab is shown as spaces up to the next 8 column stop, each mapped to the tab byte (#25).
/// How: display and exact map of "a\tb".
#[test]
fn map_pretty_points_tab_spaces_to_tab() {
    let display = displayable_line_string("a\tb", LineDisplayMode::Pretty);
    let map = build_display_to_raw_byte_map(b"a\tb", LineDisplayMode::Pretty);
    assert_eq!(display, "a       b");
    assert_eq!(map, vec![0, 1, 1, 1, 1, 1, 1, 1, 2, 3]);
}

/// A mid-line CR shows only the text after the last one, mapped to its own bytes (#25).
/// How: exact map of "ab\rcd".
#[test]
fn map_pretty_after_last_cr() {
    assert_eq!(
        displayable_line_string("ab\rcd", LineDisplayMode::Pretty),
        "cd"
    );
    let map = build_display_to_raw_byte_map(b"ab\rcd", LineDisplayMode::Pretty);
    assert_eq!(map, vec![3, 4, 5]);
}

/// Text shown by the walker for a line, with the SGR sequences it reported.
fn walk(text: &str) -> (String, Vec<&str>) {
    let mut shown = String::new();
    let mut sgr = Vec::new();
    walk_visible_line(text, |piece| match piece {
        Piece::Shown(piece, _) => shown.push_str(piece),
        Piece::Sgr(seq) => sgr.push(seq),
    });
    (shown, sgr)
}

/// SGR is reported for the render; OSC, DCS, 2-byte and incomplete escapes are dropped
/// (the cases the render's old sanitizer and incomplete-tail trim covered) (#25).
/// How: one input per sequence kind.
#[test]
fn walk_reports_sgr_and_drops_other_sequences() {
    assert_eq!(
        walk("\x1b[31mred\x1b[0m"),
        (String::from("red"), vec!["\x1b[31m", "\x1b[0m"])
    );
    for (text, shown) in [
        ("a\x1b]0;title\x07b", "ab"),
        ("a\x1b]0;title\x1b\\b", "ab"),
        ("a\x1bPdata\x1b\\b", "ab"),
        ("a\x1b7b", "ab"),
        ("a\x1b[2Kb", "ab"),
        ("abc\x1b", "abc"),
        ("abc\x1b[31", "abc"),
    ] {
        assert_eq!(walk(text), (String::from(shown), vec![]), "{text:?}");
    }
}

/// Escapes with intermediate bytes (charset `ESC ( B` sent by `tput sgr0`, `ESC # 8`) end at
/// their final byte, not at the first byte after ESC.
/// How: charset and DEC test escapes between text, and one cut at the end of the line.
#[test]
fn walk_drops_escapes_with_intermediates() {
    for (text, shown) in [
        ("a\x1b(Bb", "ab"),
        ("a\x1b(0b", "ab"),
        ("a\x1b#8b", "ab"),
        ("x\x1b(B\x1b[mdone", "xdone"),
        ("a\x1b(", "a"),
    ] {
        assert_eq!(walk(text).0, shown, "{text:?}");
    }
}

/// A new ESC aborts an unfinished CSI, OSC or DCS like in a terminal, and the new sequence
/// is parsed; a CSI with bytes outside its grammar is never passed on as SGR.
/// How: a cut CSI, OSC and DCS each followed by a full SGR, and a malformed SGR.
#[test]
fn walk_new_escape_aborts_unfinished_sequence() {
    let sgr = |text, shown: &str, seq| (text, (String::from(shown), vec![seq]));
    for (text, expected) in [
        sgr("\x1b[31\x1b[0mok", "ok", "\x1b[0m"),
        sgr("a\x1b]0;title\x1b[31mb", "ab", "\x1b[31m"),
        sgr("a\x1bPdcs\x1b[1mb", "ab", "\x1b[1m"),
    ] {
        assert_eq!(walk(text), expected, "{text:?}");
    }
    assert_eq!(walk("a\x1b[3é1mb"), (String::from("ab"), vec![]));
}

/// Only a CR outside escape sequences hides the text before it.
/// How: a CR inside an OSC title, then a real CR after the OSC.
#[test]
fn walk_ignores_cr_inside_escape() {
    assert_eq!(walk("a\x1b]0;ti\rtle\x07b").0, "ab");
    assert_eq!(walk("a\x1b]0;t\x07\rb").0, "b");
}

/// Lines with nothing to clean are borrowed as they are (minus the EOL), any control char
/// (ESC, tab, CR, BEL, DEL, C1) goes through the walker; other modes always build.
/// How: borrowed and owned samples in Pretty and Clean, plus Raw and Hex.
#[test]
fn displayable_borrows_plain_lines() {
    for mode in [LineDisplayMode::Pretty, LineDisplayMode::Clean] {
        for (text, shown) in [
            ("plain text\r\n", "plain text"),
            ("héllo ✓ 漢", "héllo ✓ 漢"),
        ] {
            let display = displayable_line_string(text, mode);
            assert!(
                matches!(display, Cow::Borrowed(d) if d == shown),
                "{text:?}"
            );
        }
        for text in [
            "a\tb",
            "\x1b[31mred",
            "50%\rdone",
            "a\x07b",
            "a\x7fb",
            "a\u{85}b",
        ] {
            let display = displayable_line_string(text, mode);
            assert!(matches!(display, Cow::Owned(_)), "{text:?} {mode:?}");
        }
    }
    for mode in [LineDisplayMode::Raw, LineDisplayMode::Hex] {
        assert!(matches!(displayable_line_string("ab", mode), Cow::Owned(_)));
    }
}

/// The shortcut map of a plain line is the one the walker builds.
/// How: walker-built map compared to `build_display_to_raw_byte_map` on plain samples.
#[test]
fn plain_line_map_matches_walker() {
    for text in ["", "plain text\r\n", "héllo ✓ 漢", "a b  c"] {
        let mut walked = Vec::new();
        walk_visible_line(text, |piece| {
            if let Piece::Shown(shown, Source::Copied(start)) = piece {
                walked.extend(start..start + shown.len());
            }
        });
        walked.push(text.trim_end_matches(['\r', '\n']).len());
        assert_eq!(
            build_display_to_raw_byte_map(text.as_bytes(), LineDisplayMode::Pretty),
            walked,
            "{text:?}"
        );
    }
}

/// Entry display text and map go to the bytes received: byte modes format them, the other
/// modes map their decoded text back through `text_to_wire_offsets` (#20).
/// How: "xéy" received as 78 E9 79 0A in Latin1, in Hex and Pretty; a UTF-8 line unchanged.
#[test]
fn entry_display_and_map_use_received_bytes() {
    use crate::configs::TextEncoding;
    let wire = b"x\xE9y\n";
    let entry = LogEntry::received(
        TextEncoding::Latin1.decode(wire),
        wire,
        Some(chrono::Local::now()),
    );
    assert_eq!(
        entry_display_text(&entry, LineDisplayMode::Hex),
        "78 E9 79 0A"
    );
    assert_eq!(
        entry_display_to_wire_map(&entry, LineDisplayMode::Hex),
        build_display_to_raw_byte_map(wire, LineDisplayMode::Hex)
    );
    // "xéy": é is 2 display bytes, both on the received byte 1; the end is the wire's
    assert_eq!(entry_display_text(&entry, LineDisplayMode::Pretty), "xéy");
    assert_eq!(
        entry_display_to_wire_map(&entry, LineDisplayMode::Pretty),
        [0, 1, 1, 2, 3]
    );

    let utf8 = LogEntry::new(String::from("xéy\n"), chrono::Local::now());
    assert_eq!(
        entry_display_to_wire_map(&utf8, LineDisplayMode::Pretty),
        build_display_to_raw_byte_map("xéy\n".as_bytes(), LineDisplayMode::Pretty)
    );
}

/// Tabs expand to the next 8 column stop counted in display width, wide chars count 2.
/// How: tabs after 0, 2 and 9 columns, two tabs in a row, and after a wide char.
#[test]
fn walk_tab_stops() {
    assert_eq!(walk("\tx").0, "        x");
    assert_eq!(walk("ab\tc").0, "ab      c");
    assert_eq!(walk("123456789\tx").0, "123456789       x");
    assert_eq!(walk("\t\tx").0, format!("{}x", " ".repeat(16)));
    assert_eq!(walk("漢\tx").0, "漢      x");
}

/// Only the text after the last mid-line CR is shown, SGR before it is still reported and
/// tab stops restart after it.
/// How: a progress line overwritten twice, colored before the CR.
#[test]
fn walk_keeps_text_after_last_cr() {
    assert_eq!(walk("10%\r50%\r100% done\r\n").0, "100% done");
    assert_eq!(
        walk("\x1b[31m50%\rok\tx"),
        (String::from("ok      x"), vec!["\x1b[31m"])
    );
}

/// Out-of-range display bytes map to the raw length.
/// How: one in-range and one past-the-end lookup.
#[test]
fn map_disp_byte_to_raw_clamps() {
    let map = [5, 6, 11];
    assert_eq!(map_disp_byte_to_raw(1, &map, 13), 6);
    assert_eq!(map_disp_byte_to_raw(3, &map, 13), 13);
}

/// A raw byte maps back to the right display column when there is ANSI before it.
/// How: 'i' of a colored "Hi" gives col 1.
#[test]
fn raw_byte_to_display_col_pretty() {
    let raw = b"\x1b[31mHi\x1b[0m";
    let map = build_display_to_raw_byte_map(raw, LineDisplayMode::Pretty);
    assert_eq!(raw_byte_to_display_col(6, &map, "Hi"), 1);
}

/// A raw byte maps to the first column of its digit group in Hex/Octal/Binary (#24).
/// How: every byte of "ABC" in each mode, expected col = byte index * stride.
#[test]
fn raw_byte_to_display_col_lands_on_group_start() {
    let modes = [
        (LineDisplayMode::Hex, 3),
        (LineDisplayMode::Octal, 4),
        (LineDisplayMode::Binary, 9),
    ];
    for (mode, stride) in modes {
        let display = displayable_line_string("ABC", mode);
        let map = build_display_to_raw_byte_map(b"ABC", mode);
        for raw_byte in 0..3 {
            let col = raw_byte_to_display_col(raw_byte, &map, &display);
            assert_eq!(col, raw_byte * stride, "{mode:?} byte {raw_byte}");
        }
    }
}

/// In Raw mode an expanded control maps to the start of its expansion (#24).
/// How: the tab of "a\tb" (raw byte 1) is display col 1, 'b' is col 5.
#[test]
fn raw_byte_to_display_col_raw_expansion_start() {
    let display = displayable_line_string("a\tb", LineDisplayMode::Raw);
    let map = build_display_to_raw_byte_map(b"a\tb", LineDisplayMode::Raw);
    assert_eq!(raw_byte_to_display_col(1, &map, &display), 1);
    assert_eq!(raw_byte_to_display_col(2, &map, &display), 5);
}

/// Byte offsets convert to char columns across multibyte chars, clamped at the end.
/// How: offsets around 'é' and past the end.
#[test]
fn safe_byte_to_char_col_handles_multibyte() {
    assert_eq!(safe_byte_to_char_col("héllo", 0), 0);
    assert_eq!(safe_byte_to_char_col("héllo", 2), 1);
    assert_eq!(safe_byte_to_char_col("héllo", 3), 2);
    assert_eq!(safe_byte_to_char_col("héllo", 99), 5);
}

// RxStream, the received bytes kept for the terminal raw panel (MAX-252)

/// The bytes of `range`, both slices joined
fn bytes_of(stream: &RxStream, range: Range<u64>) -> Vec<u8> {
    let (front, back) = stream.slices(range);
    [front, back].concat()
}

fn time(second: u32) -> Option<DateTime<Local>> {
    use chrono::TimeZone;
    Local.with_ymd_and_hms(2026, 11, 16, 12, 0, second).single()
}

/// Reads go in order, each one's offset and time kept; an empty read adds nothing.
/// How: "abc" at 1 s, "" at 2 s, "de" at 3 s.
#[test]
fn reads_get_their_offsets_and_times() {
    let mut stream = RxStream::new(100);
    assert_eq!(stream.push(b"abc", time(1)), 0);
    assert_eq!(stream.push(b"", time(2)), 3);
    assert_eq!(stream.push(b"de", time(3)), 3);
    assert_eq!((stream.start(), stream.end()), (0, 5));
    assert_eq!(bytes_of(&stream, 1..4), b"bcd");
    let reads: Vec<Read> = stream.reads().iter().copied().collect();
    assert_eq!(
        reads,
        [
            Read {
                offset: 0,
                time: time(1)
            },
            Read {
                offset: 3,
                time: time(3)
            },
        ]
    );
}

/// Past the ceiling the oldest bytes go, the offsets of the others don't change, and the read
/// cut in two starts at the first byte kept.
/// How: a 64 KiB stream (the least), 40 KiB of "a" then 40 KiB of "b".
#[test]
fn ceiling_drops_the_oldest_bytes() {
    let mut stream = RxStream::new(1);
    stream.push(&[b'a'; 40 * 1024], time(1));
    let second = stream.push(&[b'b'; 40 * 1024], time(2));
    assert_eq!(second, 40 * 1024);
    assert_eq!(stream.end(), 80 * 1024);
    assert_eq!(stream.start(), 16 * 1024);
    assert_eq!(bytes_of(&stream, second..second + 2), b"bb");
    assert_eq!(bytes_of(&stream, 0..stream.start() + 1), b"a");
    let reads: Vec<u64> = stream.reads().iter().map(|read| read.offset).collect();
    assert_eq!(reads, [16 * 1024, 40 * 1024]);
}

/// The bytes come back right when the ring wraps, a range across the wrap included.
/// How: a 64 KiB stream (the least) fed 7 byte reads past its ceiling, so the ring wraps; read
/// every range of 3 bytes kept.
#[test]
fn slices_across_the_wrap() {
    let mut stream = RxStream::new(1);
    let mut all = Vec::new();
    let mut wrapped = false;
    for round in 0..40_000u32 {
        let read: Vec<u8> = (0..7).map(|i| (round * 7 + i) as u8).collect();
        stream.push(&read, None);
        all.extend_from_slice(&read);
        wrapped |= !stream.bytes.as_slices().1.is_empty();
    }
    assert!(wrapped);
    assert!(stream.start() > 0);
    for from in stream.start()..stream.end() - 3 {
        let expected = &all[from as usize..from as usize + 3];
        assert_eq!(bytes_of(&stream, from..from + 3), expected, "from {from}");
    }
}

/// Clear keeps counting from where the stream was: an offset is never given twice.
/// How: "abc", clear, "d" is at 3.
#[test]
fn clear_keeps_the_offsets() {
    let mut stream = RxStream::new(100);
    stream.push(b"abc", None);
    stream.clear();
    assert_eq!((stream.start(), stream.end()), (3, 3));
    assert_eq!(stream.push(b"d", None), 3);
}
