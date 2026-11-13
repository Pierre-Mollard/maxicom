//! Word and char wrapping of styled lines into visual sub-lines.

use super::*;

type WrapFn = fn(Line<'static>, usize, usize, usize) -> Vec<VisualSubLine>;
const WRAPPERS: [(&str, WrapFn); 2] =
    [("words", wrap_line_by_words), ("chars", wrap_line_by_chars)];

fn line(parts: &[&str]) -> Line<'static> {
    Line::from(
        parts
            .iter()
            .map(|p| Span::raw(p.to_string()))
            .collect::<Vec<_>>(),
    )
}

fn sub_text(sub: &VisualSubLine) -> String {
    sub.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Wrapping never loses text, char counts add up, and the gutter is only on the first row.
/// How: both wrappers over several samples.
#[test]
fn wrap_preserves_text_and_char_counts() {
    let samples: [&[&str]; 4] = [
        &["│ ", "hello world foo bar"],
        &["│ ", "abcdefghijklmnopqrstuvwxyz"],
        &["│ ", "héllo ✓ wörld"],
        &["│ ", ""],
    ];
    for (name, wrap) in WRAPPERS {
        for parts in samples {
            let full: String = parts.concat();
            let subs = wrap(line(parts), 8, 2, 0);
            let joined: String = subs.iter().map(sub_text).collect();
            assert_eq!(joined, full, "{name} on {parts:?}");

            let counted: usize = subs
                .iter()
                .map(|s| s.gutter_chars + s.content_char_len)
                .sum();
            assert_eq!(counted, full.chars().count(), "{name} on {parts:?}");

            assert!(subs.iter().skip(1).all(|s| s.gutter_chars == 0), "{name}");
        }
    }
}

/// No row is wider than max_width, long words included (input without space runs).
/// How: checks visual_width on every row.
#[test]
fn wrap_respects_width_without_whitespace_runs() {
    for (name, wrap) in WRAPPERS {
        let subs = wrap(line(&["│ ", "hello world foo bar abcdefghijkl"]), 8, 2, 0);
        for sub in &subs {
            assert!(
                sub.visual_width <= 8,
                "{name}: {:?} is too wide",
                sub_text(sub)
            );
        }
    }
}

/// Word wrap breaks after the space; char wrap splits at the width.
/// How: exact rows for short inputs.
#[test]
fn wrap_breaks_at_words_and_chars() {
    let subs = wrap_line_by_words(line(&["hello world"]), 8, 0, 0);
    let texts: Vec<String> = subs.iter().map(sub_text).collect();
    assert_eq!(texts, ["hello ", "world"]);

    let subs = wrap_line_by_chars(line(&["abcdefghij"]), 4, 0, 0);
    let texts: Vec<String> = subs.iter().map(sub_text).collect();
    assert_eq!(texts, ["abcd", "efgh", "ij"]);
}

/// A word longer than the width is split over several rows (#26).
/// How: a 10-char word at width 4, then the same after a short word and a gutter.
#[test]
fn word_wrap_splits_long_word() {
    let subs = wrap_line_by_words(line(&["abcdefghij"]), 4, 0, 0);
    let texts: Vec<String> = subs.iter().map(sub_text).collect();
    assert_eq!(texts, ["abcd", "efgh", "ij"]);

    let subs = wrap_line_by_words(line(&["│ ", "hi abcdefghij"]), 6, 2, 0);
    let texts: Vec<String> = subs.iter().map(sub_text).collect();
    assert_eq!(texts, ["│ hi ", "abcdef", "ghij"]);
    assert_eq!(subs[0].gutter_chars, 2);
    assert_eq!(subs[0].content_char_len, 3);
}

/// A long word with wide chars breaks before a char that would overflow the row.
/// How: "日本語日本" (width 10) at width 5 gives rows of width 4, 4, 2.
#[test]
fn word_wrap_splits_long_word_with_wide_chars() {
    let subs = wrap_line_by_words(line(&["日本語日本"]), 5, 0, 0);
    let widths: Vec<usize> = subs.iter().map(|s| s.visual_width).collect();
    assert_eq!(widths, [4, 4, 2]);
}

/// A single char wider than max_width still lands on its own row (no infinite loop).
/// How: one double-width char at width 1.
#[test]
fn word_wrap_char_wider_than_width() {
    let subs = wrap_line_by_words(line(&["日日"]), 1, 0, 0);
    let texts: Vec<String> = subs.iter().map(sub_text).collect();
    assert_eq!(texts, ["日", "日"]);
}

/// Current behavior: a run of spaces can make a row wider than max_width. Not fixed because it
/// add a special case to check for something that is not visible (spaces) so out of screen
/// spaces are ok.
/// How: exact first row and its width.
#[test]
fn word_wrap_whitespace_run_can_exceed_width() {
    let subs = wrap_line_by_words(line(&["aaaa     b"]), 5, 0, 0);
    assert_eq!(sub_text(&subs[0]), "aaaa     ");
    assert_eq!(subs[0].visual_width, 9);
}

/// Char wrap counts double-width chars as width 2.
/// How: CJK text at width 5.
#[test]
fn char_wrap_handles_wide_chars() {
    // each '✓' is width 1, each '日' is width 2
    let subs = wrap_line_by_chars(line(&["日本語"]), 5, 0, 0);
    let texts: Vec<String> = subs.iter().map(sub_text).collect();
    assert_eq!(texts, ["日本", "語"]);
    assert_eq!(subs[0].visual_width, 4);
}

/// Rows after the first leave room for the wrap indent (#30).
/// How: a 10-char word at width 6 with a 2-wide indent: 6 chars, then 4 per row.
#[test]
fn wrap_continuation_rows_leave_room_for_indent() {
    for (name, wrap) in WRAPPERS {
        let subs = wrap(line(&["abcdefghij"]), 6, 0, 2);
        let texts: Vec<String> = subs.iter().map(sub_text).collect();
        assert_eq!(texts, ["abcdef", "ghij"], "{name}");
    }
}

/// An indent as wide as the view still makes progress, one char per row.
/// How: indent 4 at width 4.
#[test]
fn wrap_indent_as_wide_as_view_still_progresses() {
    for (name, wrap) in WRAPPERS {
        let subs = wrap(line(&["abcdef"]), 4, 0, 4);
        let texts: Vec<String> = subs.iter().map(sub_text).collect();
        assert_eq!(texts, ["abcd", "e", "f"], "{name}");
    }
}

/// End to end: no visual row (indent included) is wider than the view, and no char is lost.
/// How: the case seen in the app, a 150-char word in a 59-col view, both wrap modes.
#[test]
fn prepared_layout_rows_fit_view_with_indent() {
    let theme = ThemeConfig::default();
    let word = "0123456789".repeat(15);
    for wrap_mode in [WrapMode::Word, WrapMode::Char] {
        let mut settings = SettingsConfig::default();
        settings.line_mode.prefix_place = crate::configs::settings::PrefixPlace::Inset;
        settings.line_mode.wrap_mode = wrap_mode;
        let prefix = settings.line_prefixes().line.to_string();
        let indent = settings.line_prefixes().indent.to_string();

        let input = line(&[prefix.as_str(), word.as_str()]);
        let layout = prepare_view_layout(vec![input], &settings, &theme, 59);

        let mut content = String::new();
        for (row_idx, row) in layout.visual_lines.iter().enumerate() {
            assert!(
                row.width() <= 59,
                "{wrap_mode:?} row {row_idx} is {}",
                row.width()
            );
            let text: String = row.spans.iter().map(|s| s.content.as_ref()).collect();
            let skip = if row_idx == 0 { &prefix } else { &indent };
            content.push_str(text.strip_prefix(skip.as_str()).unwrap());
        }
        assert_eq!(content, word, "{wrap_mode:?}");
    }
}

/// Width 0 returns the whole line as a single sub-line.
/// How: checks gutter and content counts.
#[test]
fn zero_width_returns_single_sub_line() {
    for (name, wrap) in WRAPPERS {
        let subs = wrap(line(&["│ ", "hello"]), 0, 2, 0);
        assert_eq!(subs.len(), 1, "{name}");
        assert_eq!(subs[0].gutter_chars, 2, "{name}");
        assert_eq!(subs[0].content_char_len, 5, "{name}");
    }
}

/// An empty line still produces one (empty) sub-line.
/// How: both wrappers on an empty line.
#[test]
fn empty_line_gives_one_empty_sub_line() {
    for (name, wrap) in WRAPPERS {
        let subs = wrap(Line::default(), 10, 0, 0);
        assert_eq!(subs.len(), 1, "{name}");
        assert_eq!(subs[0].content_char_len, 0, "{name}");
    }
}

/// Lines laid out one at a time then joined give the layout of all of them at once (#5a).
/// How: wrapped, multi-row and empty lines, word wrap with an indent prefix and timestamps.
#[test]
fn concat_of_single_layouts_matches_one_layout() {
    let theme = ThemeConfig::default();
    let mut settings = SettingsConfig::default();
    settings.show_timestamps = true;
    settings.line_mode.wrap_indent = PrefixWrapBehavior::Prefix;
    let lines = [
        line(&["short"]),
        line(&[
            "a much longer line that wraps ",
            "over several rows ",
            "of the view",
        ]),
        line(&[""]),
        line(&["mid ", "sized line"]),
    ];
    let all = prepare_view_layout(lines.to_vec(), &settings, &theme, 16);
    let joined = PreparedLayout::concat(
        lines
            .iter()
            .map(|l| prepare_view_layout(vec![l.clone()], &settings, &theme, 16)),
    );
    assert_eq!(joined.visual_lines, all.visual_lines);
    assert_eq!(joined.max_line_width, all.max_line_width);
    assert_eq!(joined.line_layouts.len(), all.line_layouts.len());
    for (j, a) in joined.line_layouts.iter().zip(&all.line_layouts) {
        assert_eq!(j.visual_row_start, a.visual_row_start);
        let chars = |l: &LogicalLineLayout| -> Vec<usize> {
            l.sub_lines.iter().map(|s| s.content_char_len).collect()
        };
        assert_eq!(chars(j), chars(a));
    }
}

/// Every row of a laid out line as `gutter|content chars|width|text` (spaces as `·`).
fn rows_of(layout: &PreparedLayout) -> Vec<String> {
    layout
        .line_layouts
        .iter()
        .flat_map(|line| &line.sub_lines)
        .map(|sub| {
            let text: String = sub.spans.iter().map(|s| s.content.as_ref()).collect();
            format!(
                "{}|{}|{}|{}",
                sub.gutter_chars,
                sub.content_char_len,
                sub.visual_width,
                text.replace(' ', "·")
            )
        })
        .collect()
}

/// Golden rows of the word wrap, captured before it stopped copying spans (#5c): byte modes
/// (2 borrowed spans per byte), a gutter (timestamp + prefix) and an indent prefix, a byte
/// wider than the row (split char by char), and a Pretty line whose owned spans are split.
/// How: real entries through `history_entry_to_line` then `prepare_view_layout`.
#[test]
fn word_wrap_golden_rows() {
    use crate::{
        app::types::LogEntry,
        configs::{LineDisplayMode, TextEncoding},
        ui::lines::history_entry_to_line,
    };
    let theme = ThemeConfig::default();
    let laid_out = |mode, wire: &[u8], width, timestamps, indent| {
        let mut settings = SettingsConfig::default();
        // the goldens were written with 2-column "│ " prefixes: the timestamp bar then a
        // blank, or the whole prefix without timestamps
        let pad = if timestamps { " " } else { "│ " };
        settings.line_mode.prefix_place = crate::configs::settings::PrefixPlace::Inset;
        settings.line_mode.inset_prefix = crate::configs::settings::InsetPrefix {
            line: String::from(pad),
            wrap: String::from(pad),
            partial: String::from(pad),
        };
        settings.line_mode.display_mode = mode;
        settings.show_timestamps = timestamps;
        settings.timestamp_format = String::from("%M:%S");
        settings.line_mode.wrap_indent = indent;
        let time =
            chrono::TimeZone::with_ymd_and_hms(&chrono::Local, 2026, 10, 24, 12, 34, 56).unwrap();
        let entry = LogEntry::received(TextEncoding::Utf8.decode(wire), wire, Some(time));
        let prefix = settings.line_prefixes().line.to_string();
        let line = history_entry_to_line(&entry, &settings, &[], &[], None, &theme, &prefix);
        rows_of(&prepare_view_layout(vec![line], &settings, &theme, width))
    };
    use LineDisplayMode::{Binary, Hex, Octal, Pretty};
    use PrefixWrapBehavior::{None as NoIndent, Prefix};
    let cases = [
        laid_out(Hex, b"Hello World!", 20, false, NoIndent),
        laid_out(Hex, b"Hello World!", 24, true, Prefix),
        laid_out(Octal, b"abcdefgh", 18, false, Prefix),
        laid_out(Binary, b"AB\r\n", 12, false, NoIndent),
        laid_out(Binary, b"AB", 6, false, NoIndent),
        laid_out(
            Pretty,
            b"hello \x1b[31mwonderful world\x1b[0m, again",
            10,
            false,
            Prefix,
        ),
    ];
    let expected: [&[&str]; 6] = [
        &["2|18|20|│·48·65·6C·6C·6F·20·", "0|17|17|57·6F·72·6C·64·21"],
        // the trailing separator is kept even past the width (ratatui cuts it when drawing)
        &[
            "16|9|25|34:56│·48·65·6C·6C·6F·20·",
            "0|17|17|57·6F·72·6C·64·21",
        ],
        &["2|16|18|│·141·142·143·144·", "0|15|15|145·146·147·150"],
        &[
            "2|9|11|│·01000001·",
            "0|9|9|01000010·",
            "0|9|9|00001101·",
            "0|8|8|00001010",
        ],
        // a byte wider than the row is split char by char
        &[
            "2|0|2|│·",
            "0|6|6|010000",
            "0|3|3|01·",
            "0|6|6|010000",
            "0|2|2|10",
        ],
        // owned spans split: a word wider than a continuation row (10 - 2 indent) is cut
        &[
            "2|6|8|│·hello·",
            "0|8|8|wonderfu",
            "0|9|9|l·world,·",
            "0|5|5|again",
        ],
    ];
    for (rows, expected) in cases.iter().zip(expected) {
        assert_eq!(rows, expected);
    }
}

/// The arithmetic row count of byte mode lines equals the rows the word wrap makes, when it
/// gives a count (#5d).
/// How: generated lines (0-60 bytes) in Hex / Octal / Binary, with and without timestamps
/// and an indent prefix, at widths 4-90: count vs `prepare_view_layout` rows.
#[test]
fn byte_mode_rows_match_the_word_wrap() {
    use crate::{
        app::types::LogEntry,
        configs::{LineDisplayMode, TextEncoding},
        ui::lines::history_entry_to_line,
    };
    let theme = ThemeConfig::default();
    let mut counted = 0;
    for (mode, digits) in [
        (LineDisplayMode::Hex, 2),
        (LineDisplayMode::Octal, 3),
        (LineDisplayMode::Binary, 8),
    ] {
        for timestamps in [false, true] {
            for indent in [PrefixWrapBehavior::None, PrefixWrapBehavior::Prefix] {
                let mut settings = SettingsConfig::default();
                settings.line_mode.prefix_place = crate::configs::settings::PrefixPlace::Inset;
                settings.line_mode.display_mode = mode;
                settings.show_timestamps = timestamps;
                settings.line_mode.wrap_indent = indent;
                let prefix = settings.line_prefixes().line.to_string();
                let gutter = settings.line_gutter_width();
                let indent_width = continuation_indent(&settings).width();
                for len in [0, 1, 2, 5, 13, 31, 60] {
                    let wire: Vec<u8> = (0..len as u8).map(|b| b'A' + b % 26).collect();
                    let entry = LogEntry::received(
                        TextEncoding::Utf8.decode(&wire),
                        &wire,
                        Some(chrono::Local::now()),
                    );
                    let line =
                        history_entry_to_line(&entry, &settings, &[], &[], None, &theme, &prefix);
                    for width in 4..=90 {
                        let Some(rows) = byte_mode_rows(len, digits, width, gutter, indent_width)
                        else {
                            continue;
                        };
                        let layout =
                            prepare_view_layout(vec![line.clone()], &settings, &theme, width);
                        let context = format!(
                            "{mode:?} ts {timestamps} {indent:?} {len} bytes width {width}"
                        );
                        assert_eq!(rows, layout.visual_lines.len(), "{context}");
                        counted += 1;
                    }
                }
            }
        }
    }
    // most cases are counted, the fallback is only for rows too narrow
    assert!(counted > 3000, "{counted}");
}
