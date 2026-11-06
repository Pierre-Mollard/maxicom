use super::*;
use crate::terminal::{TerminalParser, new_parser};

fn parser() -> TerminalParser {
    new_parser(24, 80, 0)
}

/// The replies queued, joined, the answerback as `<answerback>`
fn take(parser: &mut TerminalParser) -> String {
    let answers = std::mem::take(&mut parser.callbacks_mut().answers);
    answers
        .into_iter()
        .map(|answer| match answer {
            Answer::Reply(_, reply) => String::from_utf8(reply).unwrap(),
            Answer::Answerback => "<answerback>".to_string(),
        })
        .collect()
}

/// The groups of the replies to `query`, in order
fn groups(query: &[u8]) -> Vec<QueryGroup> {
    let mut parser = parser();
    parser.process(query);
    let answers = std::mem::take(&mut parser.callbacks_mut().answers);
    answers
        .into_iter()
        .filter_map(|answer| match answer {
            Answer::Reply(group, _) => Some(group),
            Answer::Answerback => None,
        })
        .collect()
}

/// The replies to `query` on a fresh 24 x 80 screen
fn answer(query: &[u8]) -> String {
    let mut parser = parser();
    parser.process(query);
    take(&mut parser)
}

#[test]
fn dsr_status() {
    assert_eq!(answer(b"\x1b[5n"), "\x1b[0n");
    // not a status query: `CSI ? 5 n`, `CSI 5 ; 1 n`
    assert_eq!(answer(b"\x1b[?5n"), "");
    assert_eq!(answer(b"\x1b[5;1n"), "");
}

/// vte keeps its state between chunks: a query cut in two is answered once, on its end.
#[test]
fn query_split_across_chunks() {
    let mut parser = parser();
    parser.process(b"ab\x1b[");
    assert_eq!(take(&mut parser), "");
    parser.process(b"5ncd");
    assert_eq!(take(&mut parser), "\x1b[0n");
}

/// CPR gives the cursor where the stream left it at the query, even in the middle of a
/// chunk; it stays on the screen when moved past the end, which is how `resize` measures.
#[test]
fn cursor_position() {
    assert_eq!(answer(b"\x1b[6n"), "\x1b[1;1R");
    assert_eq!(answer(b"ab\x1b[6n\r\nc\x1b[6n"), "\x1b[1;3R\x1b[2;2R");
    assert_eq!(answer(b"\x1b[999;999H\x1b[6n"), "\x1b[24;80R");
    assert_eq!(answer(b"ab\x1b[?6n"), "\x1b[?1;3;1R");
    assert_eq!(answer(b"\x1b[?15n"), "\x1b[?13n");
}

/// With origin mode the position is still counted from the top of the screen: vt100 keeps
/// origin mode and the scroll region private (xterm would answer 1;1 here).
#[test]
fn cursor_position_ignores_origin_mode() {
    assert_eq!(answer(b"\x1b[5;10r\x1b[?6h\x1b[6n"), "\x1b[5;1R");
}

#[test]
fn identity() {
    assert_eq!(answer(b"\x1b[c"), "\x1b[?62;22c");
    assert_eq!(answer(b"\x1b[0c"), "\x1b[?62;22c");
    assert_eq!(answer(b"\x1bZ"), "\x1b[?62;22c");
    assert_eq!(answer(b"\x1b[>c"), "\x1b[>0;0;0c");
    assert_eq!(answer(b"\x1b[>0c"), "\x1b[>0;0;0c");
    assert_eq!(answer(b"\x1b[=c"), "\x1bP!|00000000\x1b\\");
    let version = format!("\x1bP>|maxicom {}\x1b\\", env!("CARGO_PKG_VERSION"));
    assert_eq!(answer(b"\x1b[>q"), version);
    assert_eq!(answer(b"\x1b[>0q"), version);
    assert_eq!(answer(b"\x1b[x"), "\x1b[2;1;1;112;112;1;0x");
    assert_eq!(answer(b"\x1b[1x"), "\x1b[3;1;1;112;112;1;0x");
    // DECSCUSR `CSI 2 SP q` (cursor shape) is no query
    assert_eq!(answer(b"\x1b[2 q"), "");
}

/// Sizes are the emulator's. The pixel ones need the host terminal's pixel size, which a
/// test may not have.
#[test]
fn sizes() {
    let mut parser = new_parser(30, 100, 0);
    parser.process(b"\x1b[18t\x1b[19t\x1b[11t");
    assert_eq!(take(&mut parser), "\x1b[8;30;100t\x1b[9;30;100t\x1b[1t");

    let pixels = answer(b"\x1b[14t\x1b[16t");
    assert!(
        pixels.is_empty() || (pixels.starts_with("\x1b[4;") && pixels.contains("t\x1b[6;")),
        "{pixels:?}"
    );
}

/// Title reports, window moves and the clipboard read are never answered (safety).
#[test]
fn never_answered() {
    for query in [
        &b"\x1b]2;title\x07\x1b[20t\x1b[21t"[..],
        b"\x1b[3t\x1b[8;10;10t\x1b[13t",
        b"\x1b]52;c;?\x07",
    ] {
        assert_eq!(answer(query), "", "{query:?}");
    }
}

/// Modes vt100 tracks answer 1 (set) or 2 (reset), the others 0, ANSI modes included.
#[test]
fn mode_states() {
    let mut parser = parser();
    parser.process(b"\x1b[?1$p\x1b[?25$p\x1b[?2004$p\x1b[?1049$p\x1b[?1000$p\x1b[?1006$p");
    let reset = "\x1b[?1;2$y\x1b[?25;1$y\x1b[?2004;2$y\x1b[?1049;2$y\x1b[?1000;2$y\x1b[?1006;2$y";
    assert_eq!(take(&mut parser), reset);

    parser.process(b"\x1b[?1h\x1b[?25l\x1b[?2004h\x1b[?1049h\x1b[?1000h\x1b[?1006h");
    parser.process(b"\x1b[?1$p\x1b[?25$p\x1b[?2004$p\x1b[?1049$p\x1b[?1000$p\x1b[?1006$p");
    let set = "\x1b[?1;1$y\x1b[?25;2$y\x1b[?2004;1$y\x1b[?1049;1$y\x1b[?1000;1$y\x1b[?1006;1$y";
    assert_eq!(take(&mut parser), set);

    assert_eq!(answer(b"\x1b[?2026$p"), "\x1b[?2026;0$y");
    assert_eq!(answer(b"\x1b[?6$p"), "\x1b[?6;0$y");
    assert_eq!(answer(b"\x1b[4$p"), "\x1b[4;0$y");
    // kitty keyboard and modifyOtherKeys stay unanswered: DA1 tells the app
    assert_eq!(answer(b"\x1b[?u\x1b[?4m\x1b[c"), "\x1b[?62;22c");
}

/// Mode 2048 (MAX-159): `?2048h` turns the size reports on and sends the size at once, a
/// single report even when the sequence holds other modes vt100 doesn't know (it hands the
/// whole list over once per such mode) or turns it on again; `?2048l` turns them off. DECRQM
/// 2048 follows the state, and a new parser starts with the mode off.
/// How: a 30x100 parser, DECRQM, then `?1049;2048;9999h` (alternate screen, the mode, an
/// unknown mode), `h` again, `l`.
#[test]
fn size_reports_mode() {
    let mut parser = new_parser(30, 100, 0);
    parser.process(b"\x1b[?2048$p");
    assert_eq!(take(&mut parser), "\x1b[?2048;2$y");

    parser.process(b"\x1b[?1049;2048;9999h");
    assert!(parser.screen().alternate_screen());
    assert!(parser.callbacks().size_reports);
    let report = take(&mut parser);
    // the pixels are the host terminal's, 0 when it doesn't give them
    let pixels = report
        .strip_prefix("\x1b[48;30;100;")
        .and_then(|rest| rest.strip_suffix('t'))
        .map(|pixels| pixels.split(';').map(str::parse::<u32>).collect::<Vec<_>>());
    assert!(
        matches!(pixels.as_deref(), Some([Ok(_), Ok(_)])),
        "{report:?}"
    );

    parser.process(b"\x1b[?2048h\x1b[?2048$p");
    assert_eq!(take(&mut parser), "\x1b[?2048;1$y");

    parser.process(b"\x1b[?2048l\x1b[?2048$p");
    assert_eq!(take(&mut parser), "\x1b[?2048;2$y");
    assert!(!parser.callbacks().size_reports);
}

/// The report and the DECRQM answer of mode 2048 are in group `size`: with the group off the
/// app reads the mode as unsupported (MAX-159).
#[test]
fn size_reports_group() {
    use QueryGroup::Size;
    assert_eq!(groups(b"\x1b[?2048$p\x1b[?2048h"), [Size, Size]);
}

/// The colors are the host terminal's, unknown to maxicom: their queries stay unanswered,
/// DA1 after them tells the app.
#[test]
fn colors_unanswered() {
    let query = b"\x1b]10;?\x1b\\\x1b]11;?\x07\x1b]12;?\x07\x1b]4;1;?\x07\x1b[?996n\x1b[c";
    assert_eq!(answer(query), "\x1b[?62;22c");
}

/// ENQ queues the answerback, sent as the setting says when taken.
#[test]
fn enq_answerback() {
    assert_eq!(answer(b"a\x05b"), "<answerback>");
}

#[test]
fn query_groups() {
    use QueryGroup::*;
    let query = b"\x1b[5n\x1b[6n\x1b[c\x1bZ\x1b[>q\x1b[18t\x1b[11t\x1b[?1$p\x1b[4$p";
    assert_eq!(
        groups(query),
        [
            Status, Status, Identity, Identity, Identity, Size, Size, Modes, Modes
        ]
    );
}

/// `answer_queries` off sends nothing; a group left out of `answer_groups` is dropped; the
/// answerback is sent when set, with the `identity` group.
#[test]
fn settings_filter_the_answers() {
    let answers = || {
        vec![
            Answer::Reply(QueryGroup::Status, b"S".to_vec()),
            Answer::Answerback,
            Answer::Reply(QueryGroup::Size, b"Z".to_vec()),
        ]
    };
    let mut config = TerminalModeConfig::default();
    assert_eq!(answer_bytes(answers(), &config), b"SZ");
    config.answerback = "hi".to_string();
    assert_eq!(answer_bytes(answers(), &config), b"ShiZ");
    config.answer_groups = vec![QueryGroup::Size, QueryGroup::Modes];
    assert_eq!(answer_bytes(answers(), &config), b"Z");
    config.answer_groups = QueryGroup::ALL.to_vec();
    config.answer_queries = false;
    assert!(answer_bytes(answers(), &config).is_empty());
}
