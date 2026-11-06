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
//! Size reports, mode 2048 (group `size`, MAX-159)
//! * The problem: over ssh or in a local terminal, the kernel tells a program its window
//!   changed size with a signal (SIGWINCH), and the program redraws. A serial line only
//!   carries bytes: the remote never hears of a resize, and a full-screen program keeps
//!   drawing at the old size until someone runs `resize` there.
//! * The fix, "in-band resize notifications" (from the contour terminal, also in kitty, foot,
//!   ghostty, wezterm): the size travels in the byte stream itself. A program that wants it
//!   turns private mode 2048 on with `CSI ? 2048 h`. The terminal answers at once with
//!   `CSI 48 ; rows ; cols ; height ; width t` (the text area in cells, then in pixels, 0
//!   when unknown), then sends that report again each time its size changes, until
//!   `CSI ? 2048 l` turns it off. The program reads the report like the answer to a query.
//! * Before turning it on, the program asks whether the terminal supports it, with DECRQM
//!   `CSI ? 2048 $ p` (see Modes below): `1` / `2` = supported (on / off), `0` = unknown, and
//!   it doesn't turn it on. neovim 0.12 asks at startup.
//! * vt100 doesn't know the mode: it hands the `h` / `l` to `unhandled_csi`, where
//!   `ParserEvents.size_reports` keeps the state, and the first report is queued. The size
//!   is the emulator's, where lines wrap, like the `t` answers above. The report and the
//!   DECRQM answer are both in group `size`: with the group off the program sees the mode as
//!   unsupported, rather than turning on reports that would never come.
//! * The next reports: `TerminalMode::set_vt100_size`, which every resize goes through, queues
//!   one in the parser's answers while the mode is on. A resize comes with no RX, so
//!   `App::take_query_answers`, called on each turn of the loop, takes them from the parser
//!   (live terminal mode only, like the other answers). A restored session turns the mode
//!   off after its bytes: the app that asked belongs to the past run.
//! * A program that dies without `CSI ? 2048 l` leaves the mode on, and the next reports land
//!   in the shell as typed text, as with any mode left on. RIS (`ESC c`, `reset`) doesn't
//!   turn it off: vt100 handles RIS with no callback; `printf '\e[?2048l'` does
//!
//! Modes (group `modes`)
//! * DECRQM `CSI ? mode $ p` (private modes) / `CSI mode $ p` (ANSI modes), "is this mode
//!   on?": `CSI ? mode ; state $ y`, state 1 = set, 2 = reset, 0 = unknown mode. neovim and fzf
//!   ask before using synchronized output (2026), bracketed paste (2004), mouse modes. Known:
//!   the private modes vt100 tracks and shows, and 2048 (size reports, answered in group
//!   `size`, see above); see `private_mode_state`. No ANSI mode
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

/// Mode 2048, in-band resize reports (MAX-159)
pub const SIZE_REPORTS: u16 = 2048;

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
/// `screen` is as the stream left it at the query, `size_reports` the state of mode 2048
pub fn answer_csi(
    screen: &Screen,
    i1: Option<u8>,
    i2: Option<u8>,
    params: &[&[u16]],
    c: char,
    size_reports: bool,
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
        // DECRQM private modes (`?`): is this mode set. Mode 2048 is in group `size`: with the
        // group off it isn't answered, so the app never turns on reports that would be dropped
        (Some(b'?'), Some(b'$'), [mode], 'p') => (
            if *mode == SIZE_REPORTS { Size } else { Modes },
            format!(
                "\x1b[?{mode};{}$y",
                private_mode_state(screen, *mode, size_reports)
            ),
        ),
        // DECRQM ANSI modes (no `?`), the standard ones: 4 insert (IRM), 20 newline (LNM, LF also
        // does CR), 12 local echo (SRM), 2 keyboard lock (KAM). vt100 exposes none of them, so 0 =
        // unknown, which apps read as "not supported". Rare: vttest, probing libraries
        (Some(b'$'), None, [mode], 'p') => (Modes, format!("\x1b[{mode};0$y")),
        _ => return None,
    };
    Some(Answer::Reply(group, reply.into_bytes()))
}

/// The in-band size report of mode 2048: `CSI 48 ; rows ; cols ; height ; width t`, the text
/// area in pixels from the host terminal's cell, 0 when it doesn't give them (MAX-159)
pub fn size_report(rows: u16, cols: u16) -> Answer {
    let (height, width) = cell_pixels().map_or((0, 0), |(height, width)| {
        (height * u32::from(rows), width * u32::from(cols))
    });
    let report = format!("\x1b[48;{rows};{cols};{height};{width}t");
    Answer::Reply(QueryGroup::Size, report.into_bytes())
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
/// keeps private, like origin mode 6). 47 and 1049 both tell the alternate screen. 2048 is
/// maxicom's own, `size_reports`, which vt100 doesn't know
fn private_mode_state(screen: &Screen, mode: u16, size_reports: bool) -> u8 {
    let set = match mode {
        SIZE_REPORTS => size_reports,
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
#[path = "../tests/app/term_answers.rs"]
mod tests;
