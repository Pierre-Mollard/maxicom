use super::*;
use ratatui::style::Color;

/// One row holding `text`, the highlights applied: each cell's foreground
fn lit(text: &str, highlights: &[KeywordHighlight]) -> Vec<Option<Color>> {
    let area = Rect::new(0, 0, text.chars().count() as u16, 1);
    let mut buf = Buffer::empty(area);
    buf.set_string(0, 0, text, Style::default());
    apply_keyword_highlights(&mut buf, area, highlights);
    (0..area.width)
        .map(|x| match buf[(x, 0)].fg {
            Color::Reset => None,
            fg => Some(fg),
        })
        .collect()
}

fn rule(pattern: &str, fg: Color) -> KeywordHighlight {
    KeywordHighlight {
        pattern: pattern.to_string(),
        style: Style::default().fg(fg),
        ignore_case: false,
        whole_word: false,
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

    let area = Rect::new(0, 0, 2, 1);
    let mut buf = Buffer::empty(area);
    buf.set_string(0, 0, "OK", Style::default().bg(Color::Blue));
    apply_keyword_highlights(&mut buf, area, &[rule("OK", Color::Green)]);
    assert_eq!(
        (buf[(0, 0)].fg, buf[(0, 0)].bg),
        (Color::Green, Color::Blue)
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
