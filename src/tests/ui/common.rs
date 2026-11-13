use super::*;
use ratatui::style::Color;

/// The styles of `spans`' chars, the highlights applied
fn char_styles(spans: Vec<Span<'static>>, highlights: &[KeywordHighlight]) -> Vec<Style> {
    highlight_spans(spans, highlights)
        .iter()
        .flat_map(|span| span.content.chars().map(|_| span.style))
        .collect()
}

/// `text` as one plain span, the highlights applied: each char's foreground
fn lit(text: &str, highlights: &[KeywordHighlight]) -> Vec<Option<Color>> {
    char_styles(vec![Span::raw(text.to_string())], highlights)
        .iter()
        .map(|style| style.fg)
        .collect()
}

fn rule(pattern: &str, fg: Color) -> KeywordHighlight {
    KeywordHighlight {
        pattern: pattern.to_string(),
        style: Style::default().fg(fg),
        ignore_case: false,
        whole_word: false,
        line_start: false,
    }
}

/// MAX-216 step 0: a word is found as written, case included, and inside other words.
/// How: `ERROR` over "ERRORS error".
#[test]
fn highlight_matches_exactly_inside_words() {
    let cells = lit("ERRORS error", &[rule("ERROR", Color::Red)]);
    let red: Vec<usize> = (0..cells.len())
        .filter(|&i| cells[i] == Some(Color::Red))
        .collect();
    assert_eq!(red, [0, 1, 2, 3, 4]);
}

/// MAX-216 step 0: where two rules cover the same text, the later one is drawn over the
/// earlier one; a rule without a background keeps the cell's.
/// How: `FAIL` red then `FAILED` green over "FAILED", then the other order; a background
/// set before a fg-only rule.
#[test]
fn later_highlight_drawn_over_earlier() {
    let fail_then_failed = lit(
        "FAILED",
        &[rule("FAIL", Color::Red), rule("FAILED", Color::Green)],
    );
    assert!(fail_then_failed.iter().all(|&c| c == Some(Color::Green)));
    let failed_then_fail = lit(
        "FAILED",
        &[rule("FAILED", Color::Green), rule("FAIL", Color::Red)],
    );
    assert_eq!(
        failed_then_fail,
        [
            Color::Red,
            Color::Red,
            Color::Red,
            Color::Red,
            Color::Green,
            Color::Green
        ]
        .map(Some)
    );

    let blue = vec![Span::styled("OK", Style::default().bg(Color::Blue))];
    let styles = char_styles(blue, &[rule("OK", Color::Green)]);
    assert_eq!(
        (styles[0].fg, styles[0].bg),
        (Some(Color::Green), Some(Color::Blue))
    );
}

fn rule_with(pattern: &str, fg: Color, ignore_case: bool, whole_word: bool) -> KeywordHighlight {
    KeywordHighlight {
        ignore_case,
        whole_word,
        ..rule(pattern, fg)
    }
}

/// The columns lit in `fg`
fn columns(cells: &[Option<Color>], fg: Color) -> Vec<usize> {
    (0..cells.len()).filter(|&i| cells[i] == Some(fg)).collect()
}

/// MAX-216: `ignore_case` finds a word in any case, letters beyond ASCII too.
/// How: `ERROR` over "Error ERROR error"; `échec` over "ÉCHEC".
#[test]
fn highlight_ignoring_case() {
    let cells = lit(
        "Error ERROR error",
        &[rule_with("ERROR", Color::Red, true, false)],
    );
    assert_eq!(columns(&cells, Color::Red).len(), 15);
    let cells = lit("ÉCHEC", &[rule_with("échec", Color::Red, true, false)]);
    assert_eq!(columns(&cells, Color::Red), [0, 1, 2, 3, 4]);
}

/// MAX-216: `whole_word` lights a word only with no letter, digit or `_` against it, at the
/// row's ends too.
/// How: `OK`, case ignored, over "OK token ok_x 0OK ok." and "xOK".
#[test]
fn highlight_whole_words_only() {
    let ok = [rule_with("OK", Color::Green, true, true)];
    let cells = lit("OK token ok_x 0OK ok.", &ok);
    assert_eq!(columns(&cells, Color::Green), [0, 1, 18, 19]);
    assert!(columns(&lit("xOK", &ok), Color::Green).is_empty());
}

fn line_rule(prefix: &str, fg: Color, ignore_case: bool) -> KeywordHighlight {
    KeywordHighlight {
        line_start: true,
        ..rule_with(prefix, fg, ignore_case, false)
    }
}

/// `#` for the chars lit in `fg`, `.` for the others
fn marks(cells: &[Option<Color>], fg: Color) -> String {
    cells
        .iter()
        .map(|&c| if c == Some(fg) { '#' } else { '.' })
        .collect()
}

/// MAX-229: a line rule lights the whole line when it starts with the prefix, its trailing
/// blanks left out; not when the prefix is further in, nor after leading spaces.
/// How: `E (` over an ESP error line with trailing spaces, a line holding "E (" in its middle,
/// and the error line indented.
#[test]
fn line_rule_lights_lines_starting_with_the_prefix() {
    let rule = [line_rule("E (", Color::Red, false)];
    assert_eq!(
        marks(&lit("E (12) wifi  ", &rule), Color::Red),
        "###########.."
    );
    assert_eq!(marks(&lit("x E (12)", &rule), Color::Red), "........");
    assert_eq!(marks(&lit("  E (12)", &rule), Color::Red), "........");
    // a line that is just the prefix, its own space kept
    assert_eq!(marks(&lit("E (", &rule), Color::Red), "###");
}

/// MAX-229: `ignore_case` applies to the prefix; words are drawn over a lit line.
/// How: `i (` ignoring case over "I (5) app ok", with `ok` green after it.
#[test]
fn line_rule_ignoring_case_under_words() {
    let rules = [
        line_rule("i (", Color::Yellow, true),
        rule("ok", Color::Green),
    ];
    let cells = lit("I (5) app ok", &rules);
    assert_eq!(marks(&cells, Color::Yellow), "##########..");
    assert_eq!(marks(&cells, Color::Green), "..........##");
    let case_kept = [line_rule("i (", Color::Yellow, false)];
    assert_eq!(marks(&lit("I (5)", &case_kept), Color::Yellow), ".....");
}
