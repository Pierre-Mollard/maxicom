//! Golden output of the terminal widget, locked before replacing tui-term (#29).

use super::*;
use std::fmt::Write;

/// Renders the widget into a `w`x`h` buffer and dumps each row's symbols (spaces as `.`, so
/// editors don't strip them), then the cells that have a color or a modifier.
fn dump(term: MaxicomTerminal<'_>, w: u16, h: u16) -> String {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    term.render(area, &mut buf);
    let mut out = String::new();
    for y in 0..h {
        for x in 0..w {
            out.push_str(&buf[(x, y)].symbol().replace(' ', "."));
        }
        out.push('\n');
    }
    // runs of same style cells per row, as `row cols: fg bg modifiers`
    for y in 0..h {
        let mut x = 0;
        while x < w {
            let style = buf[(x, y)].style();
            let start = x;
            while x < w && buf[(x, y)].style() == style {
                x += 1;
            }
            let plain = style.fg == Some(Color::Reset)
                && style.bg == Some(Color::Reset)
                && style.add_modifier.is_empty();
            if !plain {
                let (fg, bg, mods) = (style.fg, style.bg, style.add_modifier);
                writeln!(out, "{y} {start}..{x}: {fg:?} {bg:?} {mods:?}").unwrap();
            }
        }
    }
    out
}

fn styled_parser() -> vt100::Parser {
    let mut parser = vt100::Parser::new(vt100_dim(3), vt100_dim(8), 10);
    parser.process(b"old1\r\nold2\r\n");
    parser.process(b"\x1b[1;31mred\x1b[0m \x1b[2;3;4;7mx\x1b[0m\r\n");
    parser.process(b"\x1b[38;2;1;2;3;48;5;200mrgb\x1b[0m\r\nend");
    parser
}

/// Colors (indexed, RGB, default) and every modifier map to the ratatui cell, a base style
/// only fills default colors, and cells outside the screen get the filling char.
/// How: a 3x8 screen rendered into a 10x4 buffer with a base style.
#[test]
fn golden_styled_cells() {
    let parser = styled_parser();
    let term = MaxicomTerminal::new(parser.screen())
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .filling_char("·");
    insta_like(dump(term, 10, 4), GOLDEN_STYLED);
}

/// The unfocused fake cursor follows the scrollback offset, on an empty cell (symbol) and
/// once scrolled back so it is out of view.
/// How: same screen unfocused, at scrollback 0 then 1 and 3.
#[test]
fn golden_unfocused_cursor() {
    let mut parser = styled_parser();
    let cursor = MaxicomCursor::from_config(Style::default().fg(Color::Yellow), "▒");
    let mut out = String::new();
    for scrollback in [0, 1, 3] {
        parser.screen_mut().set_scrollback(scrollback);
        let term = MaxicomTerminal::new(parser.screen())
            .cursor(cursor.clone())
            .focused(false);
        writeln!(out, "-- scrollback {scrollback}").unwrap();
        out.push_str(&dump(term, 8, 3));
    }
    insta_like(out, GOLDEN_CURSOR);
}

/// x/y offsets pan the screen inside the widget.
/// How: offsets 1,1 on the styled screen.
#[test]
fn golden_offsets() {
    let parser = styled_parser();
    let term = MaxicomTerminal::new(parser.screen())
        .x_offset(1)
        .y_offset(1);
    insta_like(dump(term, 8, 3), GOLDEN_OFFSETS);
}

/// Compares with a golden string; `GOLDEN_PRINT=1` prints the actual output to recapture it.
fn insta_like(actual: String, expected: &str) {
    if std::env::var_os("GOLDEN_PRINT").is_some() {
        println!("{actual}");
    }
    // the constants start with a line break so their first row lines up with the others
    assert_eq!(actual, expected.strip_prefix('\n').unwrap());
}

const GOLDEN_STYLED: &str = r#"
red.x...··
rgb.....··
end.....··
··········
0 0..3: Some(Indexed(1)) Some(Black) BOLD
0 3..4: Some(White) Some(Black) NONE
0 4..5: Some(White) Some(Black) DIM | ITALIC | UNDERLINED | REVERSED
0 5..8: Some(White) Some(Black) NONE
0 8..10: Some(DarkGray) Some(Black) NONE
1 0..3: Some(Rgb(1, 2, 3)) Some(Indexed(200)) NONE
1 3..8: Some(White) Some(Black) NONE
1 8..10: Some(DarkGray) Some(Black) NONE
2 0..8: Some(White) Some(Black) NONE
2 8..10: Some(DarkGray) Some(Black) NONE
3 0..10: Some(DarkGray) Some(Black) NONE
"#;
const GOLDEN_CURSOR: &str = r#"
-- scrollback 0
red.x...
rgb.....
end▒....
0 0..3: Some(Indexed(1)) Some(Reset) BOLD
0 4..5: Some(Reset) Some(Reset) DIM | ITALIC | UNDERLINED | REVERSED
1 0..3: Some(Rgb(1, 2, 3)) Some(Indexed(200)) NONE
2 3..4: Some(Yellow) Some(Reset) NONE
-- scrollback 1
old2....
red.x...
rgb.....
1 0..3: Some(Indexed(1)) Some(Reset) BOLD
1 4..5: Some(Reset) Some(Reset) DIM | ITALIC | UNDERLINED | REVERSED
2 0..3: Some(Rgb(1, 2, 3)) Some(Indexed(200)) NONE
-- scrollback 3
old1....
old2....
red.x...
2 0..3: Some(Indexed(1)) Some(Reset) BOLD
2 4..5: Some(Reset) Some(Reset) DIM | ITALIC | UNDERLINED | REVERSED
"#;
const GOLDEN_OFFSETS: &str = r#"
gb......
nd......
........
0 0..2: Some(Rgb(1, 2, 3)) Some(Indexed(200)) NONE
0 7..8: Some(DarkGray) Some(Reset) NONE
1 7..8: Some(DarkGray) Some(Reset) NONE
2 0..8: Some(DarkGray) Some(Reset) NONE
"#;

/// MAX-270: a cell's invisible chars (soft hyphen, zero-width space, joiner, variation
/// selector) aren't written to the host, combining marks are; the cells keep their place: an
/// emoji vt100 made wide still takes two cells.
/// How: each case on a 1x10 screen, the first cells' symbols
#[test]
fn invisible_chars_are_not_drawn() {
    let symbols = |text: &str, n: u16| zero_width_symbols(text, n, false);
    assert_eq!(symbols("ab\u{200B}cd", 4), ["a", "b", "c", "d"]);
    assert_eq!(symbols("x\u{AD}y", 2), ["x", "y"]);
    assert_eq!(
        symbols("a\u{2764}\u{FE0F}b", 4),
        ["a", "\u{2764}", " ", "b"]
    );
    assert_eq!(
        symbols("\u{1F468}\u{200D}\u{1F469}", 4),
        ["\u{1F468}", " ", "\u{1F469}", " "]
    );
    assert_eq!(symbols("e\u{301}\u{200E}", 1), ["e\u{301}"]);
}

/// The first `n` cells' symbols of `text` drawn on a 1x10 screen, zero-width chars written
/// or not
fn zero_width_symbols(text: &str, n: u16, zero_width: bool) -> Vec<String> {
    drawn_cells(text, 10, zero_width, false)
        .into_iter()
        .take(n.into())
        .map(|(symbol, _)| symbol)
        .collect()
}

/// Each cell's symbol and whether it's dimmed, `text` drawn on a 1x10 screen into an area
/// `width` cells wide
fn drawn_cells(
    text: &str,
    width: u16,
    zero_width: bool,
    unsure_width: bool,
) -> Vec<(String, bool)> {
    let mut parser = vt100::Parser::new(vt100_dim(1), vt100_dim(10), 0);
    parser.process(text.as_bytes());
    let area = Rect::new(0, 0, width, 1);
    let mut buf = Buffer::empty(area);
    MaxicomTerminal::new(parser.screen())
        .zero_width(zero_width)
        .unsure_width(unsure_width)
        .render(area, &mut buf);
    (0..width)
        .map(|x| {
            let cell = &buf[(x, 0)];
            let dim = cell.style().add_modifier.contains(Modifier::DIM);
            (cell.symbol().to_string(), dim)
        })
        .collect()
}

/// MAX-270: `draw_zero_width` writes each cell as vt100 holds it, joiners and selectors in.
/// How: zero-width space, heart with VS16, a ZWJ sequence
#[test]
fn zero_width_chars_drawn_when_asked() {
    let symbols = |text: &str, n: u16| zero_width_symbols(text, n, true);
    assert_eq!(symbols("ab\u{200B}c", 3), ["a", "b\u{200B}", "c"]);
    assert_eq!(
        symbols("\u{2764}\u{FE0F}b", 3),
        ["\u{2764}\u{FE0F}", " ", "b"]
    );
    assert_eq!(
        symbols("\u{1F468}\u{200D}\u{1F469}", 3),
        ["\u{1F468}\u{200D}", " ", "\u{1F469}"]
    );
}

/// MAX-271: the chars every width table agrees on are drawn as received: accents, combining
/// marks, Greek, box drawing, symbols, and the wide CJK and emoji over two cells.
/// How: each case on a 1x10 screen, the first cells' symbols
#[test]
fn common_chars_drawn_as_received() {
    let symbols = |text: &str, n: u16| zero_width_symbols(text, n, false);
    assert_eq!(symbols("é\u{3b1}─°", 4), ["é", "\u{3b1}", "─", "°"]);
    assert_eq!(symbols("e\u{301}\u{20AC}", 2), ["e\u{301}", "\u{20AC}"]);
    assert_eq!(
        symbols("\u{4E2D}a\u{1F600}", 5),
        ["\u{4E2D}", " ", "a", "\u{1F600}", " "]
    );
}

/// MAX-271: a cell starting with a char hosts may draw at another width is drawn as the dimmed
/// marker, one column, a wide one keeping a space in its second column; the rest of the row
/// stays where vt100 put it.
/// How: 1 → 2 (U+3248), 2 → 1 (U+2630, wide), 1 → 0 (U+FFF9), each between letters
#[test]
fn unsure_chars_drawn_as_marker() {
    let cells = |text| drawn_cells(text, 10, false, false);
    let marker = (UNSURE_MARKER.to_string(), true);
    let plain = |s: &str| (s.to_string(), false);
    assert_eq!(
        cells("a\u{3248}b")[..3],
        [plain("a"), marker.clone(), plain("b")]
    );
    assert_eq!(
        cells("a\u{2630}b")[..4],
        [plain("a"), marker.clone(), plain(" "), plain("b")]
    );
    assert_eq!(cells("a\u{FFF9}b")[..3], [plain("a"), marker, plain("b")]);
}

/// MAX-271: a zero-width char of unsure width joined to a cell is dropped from what's written,
/// the cell's own char kept, like an invisible char.
/// How: `a` with U+0605 (host gives it a column) then `b`
#[test]
fn unsure_marks_are_not_drawn() {
    let symbols: Vec<String> = drawn_cells("a\u{605}b", 10, false, false)
        .into_iter()
        .map(|(symbol, _)| symbol)
        .collect();
    assert_eq!(symbols[..2], ["a", "b"]);
}

/// MAX-271: `draw_unsure_width` writes the cells as vt100 holds them, no marker.
/// How: the 1 → 2 and the wide 2 → 1 chars, a joined unsure mark
#[test]
fn unsure_chars_drawn_when_asked() {
    let symbols = |text| -> Vec<String> {
        drawn_cells(text, 10, false, true)
            .into_iter()
            .map(|(symbol, _)| symbol)
            .collect()
    };
    assert_eq!(symbols("a\u{3248}")[..2], ["a", "\u{3248}"]);
    assert_eq!(symbols("\u{2630}b")[..3], ["\u{2630}", " ", "b"]);
    assert_eq!(symbols("a\u{605}")[..1], ["a\u{605}"]);
}

/// MAX-271: a wide char in the area's last column is drawn as a space, it would spill over the
/// right border; one that fits is kept.
/// How: `abcd中` in areas 5 and 6 cells wide
#[test]
fn wide_char_cut_at_the_right_edge() {
    let symbols = |width| -> Vec<String> {
        drawn_cells("abcd\u{4E2D}", width, false, false)
            .into_iter()
            .map(|(symbol, _)| symbol)
            .collect()
    };
    assert_eq!(symbols(5)[4], " ");
    assert_eq!(symbols(6)[4..], ["\u{4E2D}", " "]);
}

/// MAX-264: each dropped kind's label in the notification, a long sequence cut; the
/// harmless sequences of shells known as such.
/// How: one `Dropped` per kind, a 40 parameter CSI, then the allow list and its neighbours
#[test]
fn dropped_labels_and_harmless() {
    let label = |kind, bytes: &[u8]| {
        Dropped {
            kind,
            bytes: bytes.to_vec(),
        }
        .label()
    };
    assert_eq!(label(DropKind::Control, b"\x00"), "NUL");
    assert_eq!(label(DropKind::Control, b"\x0e"), "SO");
    assert_eq!(label(DropKind::Control, b"\x7f"), "DEL");
    assert_eq!(label(DropKind::C1, "\u{85}".as_bytes()), "C1 85");
    assert_eq!(label(DropKind::Escape, b"\x1b#3"), "ESC #3");
    assert_eq!(label(DropKind::Csi, b"\x1b[?2026h"), "CSI ?2026h");
    assert_eq!(label(DropKind::Osc, b"\x1b]52;c;?\x07"), "OSC 52");
    assert_eq!(label(DropKind::String, b"\x1bPq\x1b\\"), "DCS");
    assert_eq!(label(DropKind::String, b"\x1b_a\x1b\\"), "APC");
    let long = [&b"\x1b["[..], "1;".repeat(20).as_bytes(), b"W"].concat();
    assert_eq!(
        label(DropKind::Csi, &long),
        format!("CSI {}...", "1;".repeat(12))
    );

    let harmless = |kind, bytes: &[u8]| {
        Dropped {
            kind,
            bytes: bytes.to_vec(),
        }
        .harmless()
    };
    assert!(harmless(DropKind::Osc, b"\x1b]7;file://h/x\x07"));
    assert!(harmless(DropKind::Osc, b"\x1b]133;A\x07"));
    assert!(harmless(DropKind::Osc, b"\x1b]8;;http://x\x07"));
    assert!(harmless(DropKind::Csi, b"\x1b[?2026l"));
    assert!(harmless(DropKind::Csi, b"\x1b[?1004h"));
    assert!(!harmless(DropKind::Osc, b"\x1b]52;c;?\x07"));
    assert!(!harmless(DropKind::Osc, b"\x1b]77;x\x07"));
    assert!(!harmless(DropKind::Csi, b"\x1b[?2027h"));
    assert!(harmless(DropKind::Csi, b"\x1b[22;0t"));
    assert!(harmless(DropKind::Csi, b"\x1b[23;2t"));
    assert!(harmless(DropKind::Csi, b"\x1b[?12l"));
    assert!(harmless(DropKind::Csi, b"\x1b[?7h"));
    assert!(!harmless(DropKind::Csi, b"\x1b[?7l"));
    assert!(!harmless(DropKind::Csi, b"\x1b[24t"));
    assert!(harmless(DropKind::Escape, b"\x1b(B"));
    assert!(!harmless(DropKind::Escape, b"\x1b(0"));
    assert!(!harmless(DropKind::Control, b"\x00"));

    let group = |kind, bytes: &[u8]| {
        Dropped {
            kind,
            bytes: bytes.to_vec(),
        }
        .group()
    };
    assert_eq!(group(DropKind::Osc, b"\x1b]7;x\x07"), UnhandledGroup::Shell);
    assert_eq!(
        group(DropKind::Osc, b"\x1b]52;c;?\x07"),
        UnhandledGroup::Osc
    );
    assert_eq!(group(DropKind::Csi, b"\x1b[?2026h"), UnhandledGroup::Shell);
    assert_eq!(group(DropKind::Csi, b"\x1b[5W"), UnhandledGroup::Csi);
    assert_eq!(group(DropKind::Control, b"\x7f"), UnhandledGroup::Controls);
    assert_eq!(
        group(DropKind::String, b"\x1bPq\x1b\\"),
        UnhandledGroup::Strings
    );
}
