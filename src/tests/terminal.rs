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
