//! Answers to the queries a remote program sends the terminal (MAX-151)
//!
//! A program on the remote can't see the terminal: it writes a query (an escape sequence) to
//! its tty and reads the reply, as if the user had typed it. With no reply it waits for a
//! timeout, then guesses. `[terminal_mode] answer_queries` turns the answers off, and
//! `answer_groups` keeps the groups below. vt100 answers none of them (it only draws the screen): they all come
//! here through its `unhandled_*` callbacks. Notation: `CSI` = `ESC [`, `OSC` = `ESC ]`,
//! `DCS` = `ESC P`, `ST` = `ESC \`. Rows and columns are 1-based, counted from the top left.
//!
//! Status (group `status`)
//! * DSR status `CSI 5 n`, "are you working?": `CSI 0 n` = ready. Health checks, scripts.
//! * CPR `CSI 6 n`, "where is the cursor?": `CSI row ; col R`. The main one: `resize` moves the
//!   cursor far down right (clamped to the last cell) then asks, which gives the size; shells
//!   and readline ask it to know where the prompt ends. With origin mode a terminal counts from
//!   the scroll region, but vt100 keeps both private: the answer is from the top of the screen
//! * DECXCPR `CSI ? 6 n`: the same with the page, `CSI ? row ; col ; 1 R`
//! * Printer status `CSI ? 15 n`: `CSI ? 13 n` = no printer. Old apps
//!
//! Identity (group `identity`)
//! * DA1 `CSI c`, "what kind of terminal are you?": `CSI ? 62 ; 22 c` = VT220 class (62) with
//!   ANSI colors (22). Every terminal answers it, so apps send it last after a query that may
//!   go unanswered: when the DA1 reply comes first, the other query is unsupported
//! * DECID `ESC Z`: the DA1 of the VT100 days, same reply. Old tools
//! * DA2 `CSI > c`, "which model and version?": `CSI > 0 ; 0 ; 0 c` = no known model. vim reads
//!   an xterm version there and turns on xterm features, so maxicom doesn't claim one
//! * DA3 `CSI = c`, "your unit id?": `DCS ! | 00000000 ST`, a zero id. Rare
//! * XTVERSION `CSI > 0 q` (or `CSI > q`), "your name and version?":
//!   `DCS > | maxicom <version> ST`. neovim and tmux log it or enable per-terminal fixes
//! * DECREQTPARM `CSI x`, "your line settings?": `CSI 2 ; 1 ; 1 ; 112 ; 112 ; 1 ; 0 x` = no
//!   parity, 8 bits, 9600 baud. `CSI 1 x` gets `3` first instead of `2` (reports only when
//!   asked). Only very old VT100 apps
//! * ENQ (byte `0x05`), "answerback": the `answerback` string, none by default since `0x05`
//!   also shows up in binary data
//!
//! Size (group `size`), from the emulator (where lines wrap), not the window
//! * `CSI 18 t`, "text area in cells?": `CSI 8 ; rows ; cols t`. `resize` and apps
//! * `CSI 19 t`, "screen in cells?": `CSI 9 ; rows ; cols t`, the same here
//! * `CSI 14 t` / `CSI 16 t`, "text area / one cell in pixels?": `CSI 4 ; h ; w t` /
//!   `CSI 6 ; h ; w t`, only when the host terminal gives its pixels. Image tools (sixel,
//!   kitty graphics) size pictures with them
//! * `CSI 11 t`, "is the window iconified?": `CSI 1 t` = no
//!
//! Modes (group `modes`)
//! * DECRQM `CSI ? mode $ p` (private modes) / `CSI mode $ p` (ANSI modes), "is this mode
//!   on?": `CSI ? mode ; state $ y`, state 1 = set, 2 = reset, 0 = unknown mode. neovim and fzf
//!   ask before using synchronized output (2026), bracketed paste (2004), mouse modes. Known:
//!   the private modes vt100 tracks and shows (see `private_mode_state`); no ANSI mode
//!
//! Left unanswered: the app falls back when DA1 comes first
//! * Kitty keyboard `CSI ? u` and modifyOtherKeys `CSI ? 4 m`: richer key reports, not
//!   supported
//! * DCS queries DECRQSS `DCS $ q … ST` (the value of a setting) and XTGETTCAP `DCS + q … ST`
//!   (a terminfo capability): vt100 drops DCS sequences without a callback
//! * Colors: the theme draws the terminal with the host terminal's own colors (`Color::Reset`),
//!   so maxicom doesn't know their values. vim, neovim, bat, delta then keep their own light or
//!   dark guess. The queries: `OSC 10 ; ? ST` / `OSC 11 ; ? ST` (text / background color),
//!   `OSC 12 ; ? ST` (cursor color), `OSC 4 ; n ; ? ST` (palette color n), `CSI ? 996 n`
//!   (dark or light theme?)
//!
//! Never answered, for safety
//! * `CSI 20 t` / `CSI 21 t`, the icon and window title: the remote could set a title holding a
//!   command, then have the terminal "type" it back
//! * `OSC 52 ; c ; ? ST`, the clipboard: the remote would read the local clipboard
//! * `CSI 3 t`, `CSI 8 t`, `CSI 13 t`: move, resize or locate the host window

use vt100::{MouseProtocolEncoding, MouseProtocolMode, Screen};

use crate::{
    configs::{QueryGroup, TerminalModeConfig},
    terminal::vt100_size,
};

const DA1: &str = "\x1b[?62;22c";

/// A reply the parser queued, sent or dropped by the settings when taken: `Answerback` is the
/// user's string for ENQ, read then
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Reply(QueryGroup, Vec<u8>),
    Answerback,
}

/// The bytes to send for `answers`: none when `answer_queries` is off, else the replies of the
/// groups in `answer_groups`, and the answerback (group `identity`) when it isn't empty
pub fn answer_bytes(answers: Vec<Answer>, config: &TerminalModeConfig) -> Vec<u8> {
    if !config.answer_queries {
        return Vec::new();
    }
    let groups = &config.answer_groups;
    let mut bytes = Vec::new();
    for answer in answers {
        match answer {
            Answer::Reply(group, reply) if groups.contains(&group) => bytes.extend(reply),
            Answer::Answerback if groups.contains(&QueryGroup::Identity) => {
                bytes.extend(config.answerback.as_bytes());
            }
            _ => {}
        }
    }
    bytes
}

/// The reply to the CSI sequence that vt100 doesn't handle, if it is a query maxicom answers.
/// `screen` is as the stream left it at the query
pub fn answer_csi(
    screen: &Screen,
    i1: Option<u8>,
    i2: Option<u8>,
    params: &[&[u16]],
    c: char,
) -> Option<Answer> {
    use QueryGroup::{Identity, Modes, Size, Status};

    // a parameter's first value, 0 when left empty: no query uses sub-parameters (`:`)
    let params: Vec<u16> = params
        .iter()
        .map(|p| p.first().copied().unwrap_or(0))
        .collect();
    let (rows, cols) = vt100_size(screen);
    let (row, col) = screen.cursor_position();
    let (row, col) = (row + 1, col + 1);

    let (group, reply) = match (i1, i2, params.as_slice(), c) {
        // DSR status: "are you working?", `0 n` = no malfunction (`3 n` would be a fault). Real
        // terminals never fail it: scripts use it as a ping, or as a fence (all the output before
        // it was handled once the reply comes back)
        (None, None, [5], 'n') => (Status, "\x1b[0n".to_string()),
        // CPR: the cursor, see the module doc (`resize`, readline, fzf --height)
        (None, None, [6], 'n') => (Status, format!("\x1b[{row};{col}R")),
        // DECXCPR: the same cursor plus the page (VT420s had several pages of screen memory, here
        // always 1). The `?` in the reply is its point: a plain CPR `CSI 1 ; 5 R` reads the same as
        // Ctrl-F3 in xterm's key encoding, this one can't be mistaken for a key
        (Some(b'?'), None, [6], 'n') => (Status, format!("\x1b[?{row};{col};1R")),
        // printer status: no printer (a VT can drive a printer on its aux port)
        (Some(b'?'), None, [15], 'n') => (Status, "\x1b[?13n".to_string()),
        // DA1: VT220 class with ANSI colors
        (None, None, [] | [0], 'c') => (Identity, DA1.to_string()),
        // DA2: no known model, version 0, no ROM cartridge
        (Some(b'>'), None, [] | [0], 'c') => (Identity, "\x1b[>0;0;0c".to_string()),
        // DA3: unit id 0
        (Some(b'='), None, [] | [0], 'c') => (Identity, "\x1bP!|00000000\x1b\\".to_string()),
        // XTVERSION: name and version
        (Some(b'>'), None, [] | [0], 'q') => (
            Identity,
            format!("\x1bP>|maxicom {}\x1b\\", env!("CARGO_PKG_VERSION")),
        ),
        // DECREQTPARM: the line settings between a VT100 and its host. Fixed ones: the speed is
        // a code that stops at 38400 baud (no 115200), and no app acts on the reply, it only
        // waits for one. 2 / 3 = unasked / asked report, 1 = no parity, 1 = 8 bits, 112 = 9600
        // baud sent and received, 1 = clock multiplier, 0 = no setup switches
        (None, None, [] | [0], 'x') => (Identity, "\x1b[2;1;1;112;112;1;0x".to_string()),
        (None, None, [1], 'x') => (Identity, "\x1b[3;1;1;112;112;1;0x".to_string()),
        // window state: `1 t` = open, `2 t` = iconified, i.e. minimized to the taskbar or dock
        // (an "icon" in X11 words). Apps may stop drawing while hidden; maxicom can't see the
        // host window, so it always says open
        (None, None, [11], 't') => (Size, "\x1b[1t".to_string()),
        // text area in pixels: the host terminal's cell times the emulator size
        (None, None, [14], 't') => {
            let (height, width) = cell_pixels()?;
            let (rows, cols) = (u32::from(rows), u32::from(cols));
            (Size, format!("\x1b[4;{};{}t", height * rows, width * cols))
        }
        // one cell in pixels
        (None, None, [16], 't') => {
            let (height, width) = cell_pixels()?;
            (Size, format!("\x1b[6;{height};{width}t"))
        }
        // text area in cells, the size apps use. Each `t` reply starts with the query number
        // minus 10 (18 -> 8, 19 -> 9, 14 -> 4, 16 -> 6), which tells the app what it answers
        (None, None, [18], 't') => (Size, format!("\x1b[8;{rows};{cols}t")),
        // screen in cells: in xterm the whole monitor, the biggest the window could grow (an app
        // checks it before asking a resize). The remote can't get more than the emulator: the same
        (None, None, [19], 't') => (Size, format!("\x1b[9;{rows};{cols}t")),
        // DECRQM private modes (`?`): is this mode set
        (Some(b'?'), Some(b'$'), [mode], 'p') => (
            Modes,
            format!("\x1b[?{mode};{}$y", private_mode_state(screen, *mode)),
        ),
        // DECRQM ANSI modes (no `?`), the standard ones: 4 insert (IRM), 20 newline (LNM, LF also
        // does CR), 12 local echo (SRM), 2 keyboard lock (KAM). vt100 exposes none of them, so 0 =
        // unknown, which apps read as "not supported". Rare: vttest, probing libraries
        (Some(b'$'), None, [mode], 'p') => (Modes, format!("\x1b[{mode};0$y")),
        _ => return None,
    };
    Some(Answer::Reply(group, reply.into_bytes()))
}

/// The reply to the escape sequence that vt100 doesn't handle, if it is a query maxicom answers
pub fn answer_escape(i1: Option<u8>, i2: Option<u8>, b: u8) -> Option<Answer> {
    match (i1, i2, b) {
        // DECID: the old DA1
        (None, None, b'Z') => Some(Answer::Reply(QueryGroup::Identity, DA1.as_bytes().to_vec())),
        _ => None,
    }
}

/// The reply to the control byte that vt100 doesn't handle: ENQ asks for the answerback
pub fn answer_control(b: u8) -> Option<Answer> {
    (b == 0x05).then_some(Answer::Answerback)
}

/// The DECRQM state of a private mode: 1 set, 2 reset, 0 for a mode vt100 doesn't track (or
/// keeps private, like origin mode 6). 47 and 1049 both tell the alternate screen
fn private_mode_state(screen: &Screen, mode: u16) -> u8 {
    let set = match mode {
        1 => screen.application_cursor(),
        25 => !screen.hide_cursor(),
        47 | 1049 => screen.alternate_screen(),
        9 => screen.mouse_protocol_mode() == MouseProtocolMode::Press,
        1000 => screen.mouse_protocol_mode() == MouseProtocolMode::PressRelease,
        1002 => screen.mouse_protocol_mode() == MouseProtocolMode::ButtonMotion,
        1003 => screen.mouse_protocol_mode() == MouseProtocolMode::AnyMotion,
        1005 => screen.mouse_protocol_encoding() == MouseProtocolEncoding::Utf8,
        1006 => screen.mouse_protocol_encoding() == MouseProtocolEncoding::Sgr,
        2004 => screen.bracketed_paste(),
        _ => return 0,
    };
    if set { 1 } else { 2 }
}

/// One cell of the host terminal in pixels (height, width), when it gives its pixel size
fn cell_pixels() -> Option<(u32, u32)> {
    let size = crossterm::terminal::window_size().ok()?;
    if size.width == 0 || size.height == 0 || size.columns == 0 || size.rows == 0 {
        return None;
    }
    Some((
        u32::from(size.height / size.rows),
        u32::from(size.width / size.columns),
    ))
}

#[cfg(test)]
mod tests {
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
        let reset =
            "\x1b[?1;2$y\x1b[?25;1$y\x1b[?2004;2$y\x1b[?1049;2$y\x1b[?1000;2$y\x1b[?1006;2$y";
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
}
