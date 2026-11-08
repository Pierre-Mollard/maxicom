//! Whole-frame rendering through `render`, on a test backend.

use super::*;
use crate::{
    app::{
        UpdateEffect,
        selection::{CopySide, LogicalPosition},
    },
    capture::{Screenshot, ScreenshotFormat, Stream},
    configs::{
        KeyBindingsConfig, KeyResolver, LineDisplayMode, RawSideView, SettingsConfig, TextEncoding,
    },
    inputs::{Action, AppAction, CopyModeAction, DialogAction, HistoryAction, SearchModeAction},
    link::{Address, LinkConfig},
    overlays::{
        CommandLine, ErrorDialog, FilePathDialog, HelpDialog, LineSettingsDialog, ResizeDialog,
        ScreenshotDialog, SettingsDialog,
    },
};
use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};

struct Harness {
    app: App,
    theme: ThemeConfig,
    status_bar: CompiledStatusBar,
    terminal: Terminal<TestBackend>,
}

impl Harness {
    /// Line mode, no timestamps, `display_mode`, on a `w`x`h` screen
    fn line_mode(display_mode: LineDisplayMode, w: u16, h: u16) -> Self {
        Self::line_mode_with(display_mode, w, h, |_| {})
    }

    /// Same, with settings that must be set before the app is built (history size)
    fn line_mode_with(
        display_mode: LineDisplayMode,
        w: u16,
        h: u16,
        tweak: impl FnOnce(&mut SettingsConfig),
    ) -> Self {
        let mut settings = SettingsConfig::default();
        settings.mode = Mode::Line;
        settings.show_timestamps = false;
        // the rows the tests expect have no border and a 2-column inset "│ " prefix,
        // whatever the defaults, and their small screens aren't compact (MAX-136)
        settings.ui.show_borders = false;
        settings.line_mode.prefix_place = crate::configs::settings::PrefixPlace::Inset;
        settings.line_mode.inset_prefix = crate::configs::settings::InsetPrefix {
            line: String::from("│ "),
            wrap: String::from("│ "),
            partial: String::from("│…"),
        };
        settings.ui.hide_borders_below = [0, 0];
        settings.ui.hide_scrollbar_below = [0, 0];
        settings.ui.hide_status_bar_below = [0, 0];
        // a fixed clock: two frames compared whole can straddle a second (MAX-210)
        settings.ui.status_bar.time_format = String::from("--:--:--");
        settings.line_mode.display_mode = display_mode;
        tweak(&mut settings);
        let theme = ThemeConfig::default();
        let status_bar = CompiledStatusBar::new(&settings.ui.status_bar, &theme);
        let app = App::new(
            settings,
            KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap(),
            LinkConfig::default(),
        );
        let terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        Self {
            app,
            theme,
            status_bar,
            terminal,
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        self.feed_as(TextEncoding::Utf8, bytes);
    }

    fn feed_as(&mut self, encoding: TextEncoding, bytes: &[u8]) {
        self.app.line_mode.feed_rx(false, encoding, bytes);
    }

    fn scroll(&mut self, action: HistoryAction) {
        let effect = self.app.update(Action::History(action));
        assert!(matches!(effect, UpdateEffect::None));
    }

    /// Draws a frame and returns its rows as text
    fn draw(&mut self) -> Vec<String> {
        let Self {
            app,
            theme,
            status_bar,
            terminal,
        } = self;
        terminal
            .draw(|frame| {
                render(app, theme, frame, &[], status_bar);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect()
    }
}

/// Screen rows showing Binary data (at least one 8 digit byte), as drawn, without the last
/// column: the scrollbar, which moves with every step since it counts rows (#5d).
fn binary_rows(rows: &[String]) -> Vec<String> {
    let is_byte = |word: &str| word.len() == 8 && word.bytes().all(|b| b == b'0' || b == b'1');
    rows.iter()
        .filter(|row| row.split(|c: char| !c.is_ascii_digit()).any(is_byte))
        .map(|row| {
            let mut row = row.clone();
            row.pop();
            row
        })
        .collect()
}

/// Four lines of 120 bytes in Binary (about 14 rows each) on a screen showing about 9.
/// Bytes run through printable ASCII 32..=126 (valid UTF-8, never CR or LF), and 95 is not
/// a multiple of the 8 bytes per row, so no two views look the same.
fn tall_binary_lines() -> Harness {
    let mut h = Harness::line_mode(LineDisplayMode::Binary, 80, 12);
    for line in 0..4 {
        let mut bytes: Vec<u8> = (0..120)
            .map(|i| ((line * 120 + i) % 95 + 32) as u8)
            .collect();
        bytes.extend_from_slice(b"\r\n");
        h.feed(&bytes);
    }
    h
}

/// Scrolling moves one row at a time through entries taller than the view, every row is
/// reachable, and the top and bottom are clamped (#36).
/// How: from the bottom, scroll up one row at a time until the view stops changing; each
/// view must be the previous one moved down by one row; then page down and back to bottom.
#[test]
fn line_scroll_moves_one_row_through_tall_entries() {
    let mut h = tall_binary_lines();
    let mut views = vec![binary_rows(&h.draw())];
    loop {
        h.scroll(HistoryAction::ScrollUp(1));
        let view = binary_rows(&h.draw());
        if &view == views.last().unwrap() {
            break;
        }
        views.push(view);
        assert!(views.len() < 200, "scrolling never reached the top");
    }

    // each step shows the previous view moved down by one row: no row skipped
    for pair in views.windows(2) {
        let (below, above) = (&pair[0], &pair[1]);
        assert_eq!(above[1..], below[..below.len() - 1]);
    }
    // the top shows the first byte received (32 = 00100000)
    let top = views.last().unwrap();
    assert!(top[0].contains("00100000 00100001"), "{}", top[0]);
    // 4 entries of about 14 rows each all went by, not 3 steps
    assert!(views.len() > 40, "{} views", views.len());

    // scroll to top from the bottom lands on the same view, even with entries taller than it
    h.scroll(HistoryAction::ScrollToBottom);
    h.draw();
    h.scroll(HistoryAction::ScrollToTop);
    assert_eq!(&binary_rows(&h.draw()), top);

    // a page down from the top moves by a view height minus one row
    h.scroll(HistoryAction::ScrollDownPage);
    let paged = binary_rows(&h.draw());
    assert_eq!(paged[0], top[top.len() - 1]);
    // and scroll to bottom is back to the live view
    h.scroll(HistoryAction::ScrollToBottom);
    assert_eq!(binary_rows(&h.draw()), views[0]);
}

/// The raw panel and the byte display modes show the bytes received for a Latin1 line (#20).
/// How: "xéy" received as 78 E9 79 in Latin1, raw panel in Hex, then the main view in Hex.
#[test]
fn raw_panel_and_hex_view_show_received_bytes() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 12);
    h.app.settings_mut().raw_side.view = RawSideView::Hex;
    h.feed_as(TextEncoding::Latin1, b"x\xE9y\n");
    let rows = h.draw();
    let line = rows.iter().find(|row| row.contains("xéy")).unwrap();
    assert!(line.contains("78 E9 79"), "{line}");
    assert!(!line.contains("C3 A9"), "{line}");

    h.app.settings_mut().raw_side.view = RawSideView::Off;
    h.app.settings_mut().line_mode.display_mode = LineDisplayMode::Hex;
    let rows = h.draw();
    assert!(
        rows.iter().any(|row| row.contains("78 E9 79 0A")),
        "{rows:#?}"
    );
}

/// The rows `prefix_place` marks: a line, a wrapped row and the partial RX line (MAX-163).
/// Fed: a short line, a line wrapping over two rows, then a partial line; drawn 40 wide.
fn prefix_rows(tweak: impl FnOnce(&mut SettingsConfig)) -> Vec<String> {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 40, 12, |s| {
        s.line_mode.prefix_place = crate::configs::settings::PrefixPlace::Border;
        tweak(s);
    });
    h.feed(b"short\r\n");
    h.feed(format!("{}\r\n", "word ".repeat(10)).as_bytes());
    h.feed(b"part");
    h.draw()
}

/// `border`: the text starts at the pane's first column, the left border cell marks the
/// wrapped row `┆` and the partial line `╎`, a line keeps the plain `│`.
#[test]
fn border_prefix_marks_the_left_border() {
    let rows = prefix_rows(|s| s.ui.show_borders = true);
    let short = rows.iter().position(|row| row.contains("short")).unwrap();
    assert!(rows[short].starts_with("│short"), "{rows:#?}");
    assert!(rows[short + 1].starts_with("│word word"), "{rows:#?}");
    assert!(rows[short + 2].starts_with("┆word"), "{rows:#?}");
    assert!(rows[short + 3].starts_with("╎part"), "{rows:#?}");
    assert!(rows[short + 4].starts_with("│ "), "{rows:#?}");
}

/// `border` with no left border: nothing marks the rows, the text starts at column 0.
#[test]
fn border_prefix_without_borders_marks_nothing() {
    let rows = prefix_rows(|_| {});
    let short = rows.iter().position(|row| row.contains("short")).unwrap();
    assert!(rows[short].starts_with("short"), "{rows:#?}");
    assert!(rows[short + 1].starts_with("word word"), "{rows:#?}");
    assert!(!rows[short + 2].starts_with(['┆', '│']), "{rows:#?}");
    assert!(rows[short + 3].starts_with("part"), "{rows:#?}");
    assert!(rows[short + 4].trim().is_empty(), "{rows:#?}");
}

/// `border` with timestamps: the marks go in the column after the timestamp, merged with
/// it, and the separator runs down the rows with no text.
/// How: `%M:%S` timestamps (5 columns), the default wrap indent under the timestamp.
#[test]
fn border_prefix_follows_the_timestamp() {
    let rows = prefix_rows(|s| {
        s.show_timestamps = true;
        s.timestamp_format = String::from("%M:%S");
        s.refresh_timestamp_width();
    });
    let after_time = |row: usize| rows[row].chars().skip(5).collect::<String>();
    let short = rows.iter().position(|row| row.contains("short")).unwrap();
    assert!(after_time(short).starts_with("│short"), "{rows:#?}");
    assert!(after_time(short + 2).starts_with("┆word"), "{rows:#?}");
    assert!(rows[short + 2].starts_with("     ┆"), "{rows:#?}");
    assert!(after_time(short + 3).starts_with("╎part"), "{rows:#?}");
    assert_eq!(after_time(short + 4).trim_end(), "│", "{rows:#?}");
}

/// `inset` with timestamps: the `│` bar follows the timestamp, the default prefixes come
/// after it, on every row kind; the bar runs down the rows with no text.
/// How: `%M:%S` timestamps (5 columns), the default wrap indent under the timestamp.
#[test]
fn inset_prefix_after_the_timestamp_bar() {
    let rows = prefix_rows(|s| {
        s.line_mode.prefix_place = crate::configs::settings::PrefixPlace::Inset;
        s.line_mode.inset_prefix = crate::configs::settings::InsetPrefix::default();
        s.show_timestamps = true;
        s.timestamp_format = String::from("%M:%S");
        s.refresh_timestamp_width();
    });
    let after_time = |row: usize| rows[row].chars().skip(5).collect::<String>();
    let short = rows.iter().position(|row| row.contains("short")).unwrap();
    assert!(after_time(short).starts_with("│> short"), "{rows:#?}");
    assert!(rows[short + 2].starts_with("     │  word"), "{rows:#?}");
    assert!(after_time(short + 3).starts_with("│… part"), "{rows:#?}");
    assert_eq!(after_time(short + 4).trim_end(), "│", "{rows:#?}");
}

/// `inset` without timestamps: no bar of its own, the prefixes follow the box border (no
/// `││`), and the rows with no text stay blank.
#[test]
fn inset_prefix_after_the_border() {
    let rows = prefix_rows(|s| {
        s.line_mode.prefix_place = crate::configs::settings::PrefixPlace::Inset;
        s.line_mode.inset_prefix = crate::configs::settings::InsetPrefix::default();
        s.ui.show_borders = true;
    });
    let short = rows.iter().position(|row| row.contains("short")).unwrap();
    assert!(rows[short].starts_with("│> short"), "{rows:#?}");
    assert!(rows[short + 2].starts_with("│  word"), "{rows:#?}");
    assert!(rows[short + 3].starts_with("│… part"), "{rows:#?}");
    assert!(rows[short + 4].starts_with("│ "), "{rows:#?}");
}

/// `none` with timestamps: still the `│` bar after the timestamp, no marks, down to the
/// bottom of the pane, also with no entry yet.
/// How: `%M:%S` timestamps (5 columns), 12 rows with a prompt and a status bar row below
/// the pane (10 rows).
#[test]
fn none_keeps_the_timestamp_bar() {
    let tweak = |s: &mut SettingsConfig| {
        s.line_mode.prefix_place = crate::configs::settings::PrefixPlace::None;
        s.show_timestamps = true;
        s.timestamp_format = String::from("%M:%S");
        s.refresh_timestamp_width();
    };
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 40, 12, tweak);
    let rows = h.draw();
    let bar_column = |row: &String| row.chars().nth(5) == Some('│');
    assert!(rows[..10].iter().all(bar_column), "{rows:#?}");

    let rows = prefix_rows(tweak);
    let after_time = |row: usize| rows[row].chars().skip(5).collect::<String>();
    let short = rows.iter().position(|row| row.contains("short")).unwrap();
    assert!(after_time(short).starts_with("│short"), "{rows:#?}");
    assert!(rows[short + 2].starts_with("     │word"), "{rows:#?}");
    assert!(after_time(short + 3).starts_with("│part"), "{rows:#?}");
    assert_eq!(after_time(short + 4).trim_end(), "│", "{rows:#?}");
}

/// `entry_spacing`: blank rows before each entry, in the text and in the raw panel next to
/// it; the mouse finds no text on them; in `border` they keep the plain border.
/// How: 2 short lines with 1 row of spacing, the raw panel in Hex (80 columns for it).
#[test]
fn entry_spacing_puts_blank_rows_before_entries() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 12, |s| {
        s.line_mode.entry_spacing = 1;
        s.raw_side.view = RawSideView::Hex;
    });
    h.feed(b"a\r\nb\r\n");
    let rows = h.draw();
    // the text, left of the raw panel, and no bytes in the panel
    let blank = |row: &String| row.chars().take(40).all(|c| c == ' ') && !row.contains("0A");
    assert!(blank(&rows[0]), "{rows:#?}");
    assert!(
        rows[1].starts_with("│ a") && rows[1].contains("61"),
        "{rows:#?}"
    );
    assert!(blank(&rows[2]), "{rows:#?}");
    assert!(
        rows[3].starts_with("│ b") && rows[3].contains("62"),
        "{rows:#?}"
    );
    let text_rows = &h.app.frame_map().text_rows;
    assert!(text_rows[0].is_none() && text_rows[2].is_none());
    assert_eq!(text_rows[1].as_ref().map(|row| row.line_idx), Some(0));
    assert_eq!(text_rows[3].as_ref().map(|row| row.line_idx), Some(1));
    let raw_rows = &h.app.frame_map().raw_rows;
    assert!(raw_rows[0].is_none() && raw_rows[2].is_none());
    assert_eq!(raw_rows[3].as_ref().map(|row| row.line_idx), Some(1));

    let rows = prefix_rows(|s| {
        s.ui.show_borders = true;
        s.line_mode.entry_spacing = 1;
    });
    let short = rows.iter().position(|row| row.contains("short")).unwrap();
    assert!(rows[short - 1].starts_with("│ "), "{rows:#?}");
    assert!(rows[short + 1].starts_with("│ "), "{rows:#?}");
    assert!(rows[short + 2].starts_with("│word"), "{rows:#?}");
    assert!(rows[short + 3].starts_with("┆word"), "{rows:#?}");
    assert!(rows[short + 4].starts_with("│ "), "{rows:#?}");
    assert!(rows[short + 5].starts_with("╎part"), "{rows:#?}");
}

/// `entry_spacing` counts in the scroll: every entry is its gap and its rows, also when the
/// height is counted without laying the entry out (Truncate, byte modes).
/// How: 100 numbered lines with 1 row of spacing, in Pretty, Hex and Truncate: the rows
/// to scroll are 200 less a view; live, the last line is on the bottom row.
#[test]
fn entry_spacing_counts_in_the_scroll() {
    for (display_mode, wrap_mode) in [
        (LineDisplayMode::Pretty, crate::configs::WrapMode::Word),
        (LineDisplayMode::Hex, crate::configs::WrapMode::Word),
        (LineDisplayMode::Pretty, crate::configs::WrapMode::Truncate),
    ] {
        let mut h = Harness::line_mode_with(display_mode, 40, 12, |s| {
            s.line_mode.entry_spacing = 1;
            s.line_mode.wrap_mode = wrap_mode;
        });
        for i in 0..100 {
            h.feed(format!("line {i:02}\r\n").as_bytes());
        }
        let rows = h.draw();
        let height = h.app.frame_areas().content.height as usize;
        let status = h.app.line_mode.scroll_status();
        assert_eq!(
            status.max_scroll,
            200 - height,
            "{display_mode:?} {wrap_mode:?}"
        );
        // the text, left of the scrollbar
        let blank = |row: &String| row.chars().take(39).all(|c| c == ' ');
        assert!(!blank(&rows[height - 1]), "{rows:#?}");
        assert!(blank(&rows[height - 2]), "{rows:#?}");
    }
}

/// `entry_spacing` in copy mode: the cursor is drawn on the text rows, a step up skips the
/// gap; entered from a view whose bottom row is a gap, it goes to the entry above, shown.
/// How: 100 numbered lines with 1 row of spacing; from the live view, then scrolled 1 row.
#[test]
fn entry_spacing_in_copy_mode() {
    let numbered = || {
        let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 40, 12, |s| {
            s.line_mode.entry_spacing = 1;
        });
        for i in 0..100 {
            h.feed(format!("line {i:02}\r\n").as_bytes());
        }
        h.draw();
        let height = h.app.frame_areas().content.height;
        (h, height)
    };
    let (mut h, height) = numbered();
    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    let rows = h.draw();
    assert_eq!(cursor_cell(&mut h).1, height - 1, "{rows:#?}");
    h.app.update(Action::CopyMode(CopyModeAction::MoveUp));
    let rows = h.draw();
    let y = cursor_cell(&mut h).1;
    assert_eq!(y, height - 3, "{rows:#?}");
    assert!(rows[y as usize].contains("line 98"), "{rows:#?}");

    let (mut h, height) = numbered();
    h.scroll(HistoryAction::ScrollUp(1));
    let rows = h.draw();
    let gap = &rows[height as usize - 1];
    assert!(gap.chars().take(39).all(|c| c == ' '), "{rows:#?}");
    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    let rows = h.draw();
    assert_eq!(h.app.line_mode.copy_state().cursor.line_idx, 98);
    assert_eq!(cursor_cell(&mut h).1, height - 2, "{rows:#?}");
}

/// 30 numbered lines, the live view cleared, then a separator, as a restore does (MAX-176):
/// the history's rows, the text columns only (the scrollbar left out)
fn cleared_view() -> (Harness, usize) {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 40, 12);
    for i in 0..30 {
        h.feed(format!("line {i:02}\r\n").as_bytes());
    }
    h.app.line_mode.clear_view();
    h.feed(b"-- restored --\r\n");
    h.draw();
    let height = h.app.frame_areas().content.height as usize;
    (h, height)
}

fn text_rows(h: &mut Harness, height: usize) -> Vec<String> {
    h.draw()[..height]
        .iter()
        .map(|row| {
            row.chars()
                .take(39)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

/// MAX-176: after a `clear` the separator is on the top row, blank rows under it, filled by
/// the new lines; once they reach the bottom the normal live view takes over.
/// How: 30 lines and the separator, cleared; 1 new line, then 12 more (the view is 10).
#[test]
fn cleared_view_starts_on_the_separator() {
    let (mut h, height) = cleared_view();
    let rows = text_rows(&mut h, height);
    assert_eq!(rows[0], "│ -- restored --", "{rows:#?}");
    assert!(rows[1..].iter().all(String::is_empty), "{rows:#?}");

    h.feed(b"new\r\n");
    let rows = text_rows(&mut h, height);
    assert_eq!(rows[..3], ["│ -- restored --", "│ new", ""], "{rows:#?}");

    for i in 0..12 {
        h.feed(format!("more {i:02}\r\n").as_bytes());
    }
    let rows = text_rows(&mut h, height);
    assert_eq!(rows[height - 1], "│ more 11", "{rows:#?}");
    assert_eq!(h.app.line_mode.view_floor(), None);
}

/// MAX-176: from the cleared view, scrolling moves row by row: up shows the restored lines
/// one row at a time, also past the point where the blank rows are gone (the view becomes a
/// normal one); down comes back, never lower than the separator on the top row.
/// How: cleared on the separator (row 30) with 1 new line, a view of 10 rows: up 3, 3, 3
/// (the top at line 21 is under the live top, 22), then down 9 from the floor.
#[test]
fn cleared_view_scrolls_by_rows() {
    let (mut h, height) = cleared_view();
    h.feed(b"new\r\n");
    let top = |h: &mut Harness| text_rows(h, height)[0].clone();
    h.scroll(HistoryAction::ScrollUp(3));
    assert_eq!(top(&mut h), "│ line 27");
    h.scroll(HistoryAction::ScrollDown(9));
    assert_eq!(top(&mut h), "│ -- restored --");
    assert!(h.app.line_mode.view_floor().is_some());

    h.scroll(HistoryAction::ScrollUp(3));
    assert_eq!(top(&mut h), "│ line 27");
    h.scroll(HistoryAction::ScrollUp(3));
    assert_eq!(top(&mut h), "│ line 24");
    h.scroll(HistoryAction::ScrollUp(3));
    assert_eq!(top(&mut h), "│ line 21");
    assert_eq!(h.app.line_mode.view_floor(), None);
    h.scroll(HistoryAction::ScrollUp(1));
    assert_eq!(top(&mut h), "│ line 20");
    // back down: the normal live view, the floor is gone
    h.scroll(HistoryAction::ScrollToBottom);
    assert_eq!(top(&mut h), "│ line 22");
}

/// MAX-176: copy mode entered on the cleared view keeps it (the cursor is shown); a step
/// above it moves the view one row, as scrolling does. A search jump into the restored
/// lines drops the floor.
/// How: cleared with 1 new line; Enter, Up (the separator), Up (line 29, above the view).
#[test]
fn cleared_view_in_copy_mode_and_search() {
    let (mut h, height) = cleared_view();
    h.feed(b"new\r\n");
    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    let rows = text_rows(&mut h, height);
    assert_eq!(rows[0], "│ -- restored --", "{rows:#?}");
    assert_eq!(cursor_cell(&mut h).1, 1);
    h.app.update(Action::CopyMode(CopyModeAction::MoveUp));
    let rows = text_rows(&mut h, height);
    assert_eq!(rows[0], "│ -- restored --", "{rows:#?}");
    assert!(h.app.line_mode.view_floor().is_some());
    h.app.update(Action::CopyMode(CopyModeAction::MoveUp));
    let rows = text_rows(&mut h, height);
    assert_eq!(
        rows[..3],
        ["│ line 29", "│ -- restored --", "│ new"],
        "{rows:#?}"
    );
    assert_eq!(cursor_cell(&mut h).1, 0);

    let (mut h, height) = cleared_view();
    search(&mut h, "line 05");
    let rows = text_rows(&mut h, height);
    assert!(rows.iter().any(|row| row == "│ line 05"), "{rows:#?}");
    assert_eq!(h.app.line_mode.view_floor(), None);
}

/// MAX-176: the `clear` command starts the live view on the next line: blank rows, then the
/// new lines from the top; the lines before stay a scroll up. A prompt not ended yet (the
/// partial RX line) stays on the top row and its line keeps it there once ended.
/// How: 30 lines, cleared; 1 new line, scroll up 1. Then 30 lines and a `$ ` prompt,
/// cleared; the prompt ended by the echoed `ls`.
#[test]
fn clear_starts_the_view_on_the_next_line() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 40, 12);
    h.draw();
    let height = h.app.frame_areas().content.height as usize;
    for i in 0..30 {
        h.feed(format!("line {i:02}\r\n").as_bytes());
    }
    h.app.update(Action::History(HistoryAction::ClearView));
    let rows = text_rows(&mut h, height);
    assert!(rows.iter().all(String::is_empty), "{rows:#?}");
    h.feed(b"new\r\n");
    let rows = text_rows(&mut h, height);
    assert_eq!(rows[..2], ["│ new", ""], "{rows:#?}");
    h.scroll(HistoryAction::ScrollUp(1));
    let rows = text_rows(&mut h, height);
    assert_eq!(rows[..3], ["│ line 29", "│ new", ""], "{rows:#?}");

    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 40, 12);
    for i in 0..30 {
        h.feed(format!("line {i:02}\r\n").as_bytes());
    }
    h.feed(b"$ ");
    h.app.update(Action::History(HistoryAction::ClearView));
    let rows = text_rows(&mut h, height);
    assert!(rows[0].ends_with('$'), "{rows:#?}");
    assert!(rows[1..].iter().all(String::is_empty), "{rows:#?}");
    h.feed(b"ls\r\n");
    let rows = text_rows(&mut h, height);
    assert_eq!(rows[..2], ["│ $ ls", ""], "{rows:#?}");
}

/// Truncate mode scrolled up and panned right shows the same rows and columns once the
/// paragraph only gets the visible rows (#5b).
/// How: 40 numbered lines wider than the view, scroll up 5 rows, pan right 3 columns (the
/// line numbers stay in view).
#[test]
fn truncate_scrolled_and_panned_view() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 60, 12);
    h.app.settings_mut().line_mode.wrap_mode = crate::configs::WrapMode::Truncate;
    for i in 0..40 {
        h.feed(format!("line {i:02} {}\r\n", "abcdefghij".repeat(8)).as_bytes());
    }
    h.draw();
    for _ in 0..5 {
        h.scroll(HistoryAction::ScrollUp(1));
    }
    h.scroll(HistoryAction::PanRight(3));
    let rows = h.draw();
    // the 59 text columns (the last one is the scrollbar): "│ line NN abc…" panned by 3
    let shown: Vec<String> = rows[..9]
        .iter()
        .map(|row| row.chars().take(59).collect())
        .collect();
    let expected: Vec<String> = (25..34)
        .map(|i| format!("ine {i:02} {}ab", "abcdefghij".repeat(5)))
        .collect();
    assert_eq!(shown, expected);
}

/// The scroll position counts rows (#5d): each scroll step moves it by one, from 0 (live,
/// `Bot`) to `max_scroll` = total rows - view (`Top`), and scroll to top / bottom land on
/// both ends exactly.
/// How: the tall Binary lines, scrolled up one row at a time until the view stops moving.
#[test]
fn scroll_status_counts_rows() {
    let mut h = tall_binary_lines();
    h.draw();
    let status = h.app.scroll_status();
    assert_eq!(status.offset_from_bottom, 0);
    assert!(status.is_bottom());
    let max_scroll = status.max_scroll;
    // 4 entries of ~15 rows each, minus a view: more than the 3 entries the old count gave
    assert!(max_scroll > 40, "{max_scroll}");

    for step in 1..=max_scroll {
        h.scroll(HistoryAction::ScrollUp(1));
        h.draw();
        assert_eq!(h.app.scroll_status().offset_from_bottom, step);
    }
    h.scroll(HistoryAction::ScrollUp(1));
    h.draw();
    assert_eq!(h.app.scroll_status().offset_from_bottom, max_scroll);
    assert!(h.app.scroll_status().is_top());

    h.scroll(HistoryAction::ScrollToBottom);
    h.draw();
    assert_eq!(h.app.scroll_status().offset_from_bottom, 0);
    h.scroll(HistoryAction::ScrollToTop);
    h.draw();
    assert_eq!(h.app.scroll_status().offset_from_bottom, max_scroll);
}

/// Entry heights stay aligned with the history when old entries are evicted, and are
/// counted again after a width change (#5d).
/// How: the status after 6 tall lines with a history of 3, drawn after each line, equals a
/// history that only got the last 3; a view drawn at 80 then 60 columns equals one drawn
/// at 60 from the start.
#[test]
fn row_heights_follow_evictions_and_width() {
    // drawn after each line, like live RX, so heights are counted before the evictions
    // (lines of different lengths, so a misaligned height shows)
    let feed = |h: &mut Harness, lines: std::ops::Range<u8>| {
        for line in lines {
            let mut bytes = vec![b'a' + line; 40 + 20 * line as usize];
            bytes.extend_from_slice(b"\r\n");
            h.feed(&bytes);
            h.draw();
        }
    };
    let small_history = |w, hgt| {
        Harness::line_mode_with(LineDisplayMode::Binary, w, hgt, |s| {
            s.line_mode.history_size = 3;
        })
    };

    let mut evicted = small_history(80, 12);
    feed(&mut evicted, 0..6);
    evicted.draw();
    let mut fresh = small_history(80, 12);
    feed(&mut fresh, 3..6);
    fresh.draw();
    assert_eq!(evicted.app.scroll_status(), fresh.app.scroll_status());

    evicted.terminal.backend_mut().resize(60, 12);
    evicted.draw();
    let mut narrow = small_history(60, 12);
    feed(&mut narrow, 3..6);
    narrow.draw();
    assert_eq!(evicted.app.scroll_status(), narrow.app.scroll_status());
    assert!(narrow.app.scroll_status().max_scroll > fresh.app.scroll_status().max_scroll);
}

/// Six tall Hex entries (~7 rows each) on a 60x12 view, Binary side panel: entry `k` is its
/// letter repeated, `E` (second from the bottom) holds "in" (69 6E), `F` starts with "ZZ".
fn tall_hex_lines() -> Harness {
    tall_lines(LineDisplayMode::Hex, RawSideView::Binary, |_| {})
}

/// The same entries in another main view / side panel setup
fn tall_lines(
    mode: LineDisplayMode,
    raw: RawSideView,
    tweak: impl FnOnce(&mut SettingsConfig),
) -> Harness {
    let mut h = Harness::line_mode_with(mode, 60, 12, tweak);
    h.app.settings_mut().raw_side.view = raw;
    for letter in b"ABCDEF" {
        let mut bytes = vec![*letter; 120];
        if *letter == b'E' {
            bytes[50..52].copy_from_slice(b"in");
        }
        if *letter == b'F' {
            bytes[..2].copy_from_slice(b"ZZ");
        }
        bytes.extend_from_slice(b"\r\n");
        h.feed(&bytes);
    }
    h.draw();
    h
}

fn shows(rows: &[String], text: &str) -> bool {
    rows.iter().any(|row| row.contains(text))
}

/// MAX-123: copy mode entered from a search match keeps the cursor's row on screen when the
/// cursor moves, with tall entries near the bottom of the history.
/// How: search "69 6e" (only in `E`), confirm with Enter, move right: the match must still be
/// shown, before and after each move. Two setups: Hex main view with a Binary side panel and
/// a text search; Binary main view with timestamps and an indent prefix, a Hex side panel
/// and a raw bytes search (the ticket's)
#[test]
fn copy_mode_from_search_keeps_cursor_row_in_view() {
    let ticket_setup = |s: &mut SettingsConfig| {
        s.show_timestamps = true;
        s.line_mode.wrap_indent = crate::configs::settings::PrefixWrapBehavior::Prefix;
    };
    let setups = [
        (tall_hex_lines(), false, "69 6E"),
        (
            tall_lines(LineDisplayMode::Binary, RawSideView::Hex, ticket_setup),
            true,
            "01101001 01101110",
        ),
    ];
    for (mut h, raw_search, shown) in setups {
        h.app.update(Action::Search(SearchModeAction::Start));
        if raw_search {
            h.app.update(Action::Search(SearchModeAction::Kind));
        }
        let query = String::from("69 6e");
        h.app.update(Action::Search(SearchModeAction::Paste(query)));
        h.draw();
        h.app.update(Action::Search(SearchModeAction::Confirm));
        let rows = h.draw();
        assert!(shows(&rows, shown), "after Enter: {rows:#?}");
        for _ in 0..3 {
            h.app.update(Action::CopyMode(CopyModeAction::MoveRight));
            let rows = h.draw();
            assert!(shows(&rows, shown), "after a move: {rows:#?}");
        }
    }
}

/// MAX-123: entering copy mode at the bottom puts the cursor on the first row of the last
/// entry, which must be on screen even when that entry is taller than the view.
/// How: copy mode entered on the tall `F` entry, which starts with "ZZ" (5A 5A).
#[test]
fn copy_mode_at_bottom_shows_the_cursor_row() {
    let mut h = tall_hex_lines();
    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    let rows = h.draw();
    assert!(shows(&rows, "5A 5A"), "{rows:#?}");
    h.app.update(Action::CopyMode(CopyModeAction::MoveRight));
    let rows = h.draw();
    assert!(shows(&rows, "5A 5A"), "{rows:#?}");
}

/// MAX-124: a view scrolled up into the history keeps showing the same rows when new lines
/// arrive, also when old ones are evicted; the live view still follows the new lines.
/// How: 20 numbered lines with a history of 15, scrolled up 3 rows, then 2 more lines
#[test]
fn scrolled_view_stays_put_when_lines_arrive() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 60, 12, |s| {
        s.line_mode.history_size = 15;
    });
    let line_rows = |rows: &[String]| -> Vec<String> {
        rows.iter()
            .filter(|row| row.contains("line "))
            .map(|row| row.chars().take(59).collect())
            .collect()
    };
    for i in 0..20 {
        h.feed(format!("line {i:02}\r\n").as_bytes());
    }
    h.draw();
    for _ in 0..3 {
        h.scroll(HistoryAction::ScrollUp(1));
    }
    let before = line_rows(&h.draw());
    h.feed(b"line 20\r\nline 21\r\n");
    assert_eq!(line_rows(&h.draw()), before);

    h.scroll(HistoryAction::ScrollToBottom);
    h.draw();
    h.feed(b"line 22\r\n");
    assert!(shows(&h.draw(), "line 22"));

    // a scroll not drawn yet when a line arrives: 2 rows up from "line 22", not from the
    // new line
    h.scroll(HistoryAction::ScrollUp(2));
    h.feed(b"line 23\r\n");
    let rows = line_rows(&h.draw());
    assert!(rows.last().unwrap().contains("line 20"), "{rows:#?}");
}

/// MAX-193: copy mode holds the live view when lines arrive, in both modes: the rows drawn
/// and the selection stay where they were. Leaving copy mode goes back to the live view; a
/// view scrolled up by hand before copy mode stays scrolled after it, and without copy mode
/// the live view follows.
/// How: 20 numbered lines in a 12-row screen, copy mode with a line selected, 2 more lines,
/// then Esc; again scrolled up 2 rows first. Line mode, then terminal mode.
#[test]
fn copy_mode_holds_the_live_view() {
    for mode in [Mode::Line, Mode::Terminal] {
        let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 60, 12, |s| s.mode = mode);
        let line_rows = |rows: &[String]| -> Vec<String> {
            rows.iter()
                .filter(|row| row.contains("line "))
                .map(|row| row.chars().take(59).collect())
                .collect()
        };
        let selection = h_theme().ui.selection.bg;
        for i in 0..20 {
            h.app.feed_rx(format!("line {i:02}\r\n").as_bytes());
        }
        h.draw();
        h.app.update(Action::CopyMode(CopyModeAction::Enter));
        h.app.update(Action::CopyMode(CopyModeAction::MoveUp));
        h.app.update(Action::CopyMode(CopyModeAction::SelectLine));
        // the selected row: its text's first cell on the selection's background
        let selected = |h: &Harness, rows: &[String]| -> Vec<String> {
            let lines = rows
                .iter()
                .enumerate()
                .filter(|(_, row)| row.contains("line "));
            lines
                .filter(|&(y, row)| {
                    let x = cell_of(std::slice::from_ref(row), "line ").0;
                    bg_at(h, (x, y as u16)) == selection
                })
                .map(|(_, row)| row.chars().take(59).collect())
                .collect()
        };
        let rows = h.draw();
        let before = line_rows(&rows);
        let before_selected = selected(&h, &rows);
        assert_eq!(before_selected.len(), 1, "{mode:?} {rows:#?}");

        h.app.feed_rx(b"line 20\r\nline 21\r\n");
        let rows = h.draw();
        // the same rows, the blank one under them (the remote's cursor) may fill
        assert_eq!(line_rows(&rows)[..before.len()], before[..], "{mode:?}");
        assert!(!shows(&rows, "line 21"), "{mode:?} {rows:#?}");
        assert_eq!(selected(&h, &rows), before_selected, "{mode:?} {rows:#?}");

        h.app.update(Action::CopyMode(CopyModeAction::Exit));
        let rows = h.draw();
        assert!(shows(&rows, "line 21"), "{mode:?} {rows:#?}");
        h.app.feed_rx(b"line 22\r\n");
        assert!(shows(&h.draw(), "line 22"), "{mode:?}");

        // scrolled up by hand first: still scrolled after copy mode
        h.scroll(HistoryAction::ScrollUp(2));
        h.draw();
        h.app.update(Action::CopyMode(CopyModeAction::Enter));
        let before = line_rows(&h.draw());
        h.app.feed_rx(b"line 23\r\n");
        h.app.update(Action::CopyMode(CopyModeAction::Exit));
        let rows = h.draw();
        assert_eq!(line_rows(&rows), before, "{mode:?}");
        assert!(!shows(&rows, "line 23"), "{mode:?} {rows:#?}");
    }
}

/// MAX-124, with rows: scrolled up one row inside the last tall entry (the entry offset is
/// still 0, only rows are hidden below), a new tall line leaves the view where it was.
#[test]
fn view_scrolled_inside_the_last_entry_stays_put() {
    let mut h = tall_binary_lines();
    h.draw();
    h.scroll(HistoryAction::ScrollUp(1));
    let before = binary_rows(&h.draw());
    let mut bytes = vec![b'~'; 120];
    bytes.extend_from_slice(b"\r\n");
    h.feed(&bytes);
    assert_eq!(binary_rows(&h.draw()), before);
}

fn search(h: &mut Harness, query: &str) {
    h.app.update(Action::Search(SearchModeAction::Start));
    h.app
        .update(Action::Search(SearchModeAction::Paste(query.to_string())));
}

/// MAX-72: a search jump centers the view on the match once; after that the view scrolls
/// freely and stays where it was moved, until the next jump.
/// How: 40 numbered lines, search "line 20", scroll down 3 rows, draw twice, then Next
#[test]
fn search_view_scrolls_freely_after_the_jump() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 60, 12);
    for i in 0..40 {
        h.feed(format!("line {i:02}\r\n").as_bytes());
    }
    h.draw();
    let line_rows = |rows: &[String]| -> Vec<String> {
        rows.iter()
            .filter(|row| row.starts_with("│ line"))
            .map(|row| row.chars().take(9).collect())
            .collect()
    };
    search(&mut h, "line 20");
    let centered = line_rows(&h.draw());
    assert!(centered.contains(&"│ line 20".to_string()), "{centered:#?}");

    h.scroll(HistoryAction::ScrollDown(3));
    let moved = line_rows(&h.draw());
    assert_eq!(
        moved[..moved.len() - 3],
        centered[3..],
        "{centered:#?} {moved:#?}"
    );
    assert_eq!(line_rows(&h.draw()), moved);

    h.app.update(Action::Search(SearchModeAction::Next));
    assert_eq!(line_rows(&h.draw()), centered);
}

/// MAX-72: the raw panel pan is centered on the match by the jump, then follows the side
/// pan keys, in line mode and in terminal mode.
/// How: a line of 100 bytes with "QQ" at 90 (far right in the Hex panel), search "QQ",
/// pan the side panel left by 6, draw twice
#[test]
fn search_raw_pan_moves_after_the_jump() {
    let mut line = vec![b'a'; 100];
    line[90..92].copy_from_slice(b"QQ");
    line.extend_from_slice(b"\r\n");

    let mut lm = Harness::line_mode(LineDisplayMode::Pretty, 80, 12);
    lm.app.settings_mut().raw_side.view = RawSideView::Hex;
    lm.app.settings_mut().line_mode.wrap_mode = crate::configs::WrapMode::Truncate;
    lm.feed(&line);
    lm.draw();
    let mut tm = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 12, |s| {
        s.mode = Mode::Terminal;
        s.terminal_mode.cols = 120;
    });
    tm.app.settings_mut().raw_side.view = RawSideView::Hex;
    tm.app.terminal_mode.process_rx(TextEncoding::Utf8, &line);
    tm.draw();

    let raw_pan = |h: &Harness| match h.app.settings().mode {
        Mode::Line => h.app.line_mode.raw_pan_x(),
        Mode::Terminal => h.app.terminal_mode.raw_pan_x(),
    };
    for mut h in [lm, tm] {
        search(&mut h, "QQ");
        h.draw();
        let centered = raw_pan(&h);
        assert!(centered >= 6, "{centered}");
        h.app.update(Action::App(crate::inputs::AppAction::SideView(
            crate::inputs::SideViewAction::PanLeft(2),
        )));
        h.draw();
        h.draw();
        assert_eq!(raw_pan(&h), centered - 6, "{:?}", h.app.settings().mode);
    }
}

/// MAX-72: terminal mode scrolls while the search bar is open (the scroller's offset counts
/// the rows panned inside the grid, then the scrollback).
#[test]
fn terminal_scrolls_during_a_search() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 12, |s| {
        s.mode = Mode::Terminal;
    });
    for i in 0..60 {
        let line = format!("line {i:02}\r\n");
        h.app
            .terminal_mode
            .process_rx(TextEncoding::Utf8, line.as_bytes());
    }
    h.draw();
    search(&mut h, "line 50");
    h.draw();
    let before = h.app.terminal_mode.scroller().offset_y;
    h.scroll(HistoryAction::ScrollUp(3));
    h.draw();
    assert_eq!(h.app.terminal_mode.scroller().offset_y, before + 3);
}

/// MAX-72: the view stored by a search jump is the centered one, also inside tall entries:
/// the next frame (plain scrolling from that position) shows the same rows.
/// How: the tall Hex lines, search "69 6e" (inside `E`), draw twice; then the same at the
/// bottom (`F` holds "ZZ", the view ends on the last row: back to the live view)
#[test]
fn search_jump_view_is_kept_with_tall_entries() {
    for query in ["69 6e", "ZZ"] {
        let mut h = tall_hex_lines();
        search(&mut h, query);
        let jumped = h.draw();
        assert_eq!(h.draw(), jumped, "{query}");
    }
}

/// MAX-71: terminal mode's raw panel pan stops at its widest row (it had no limit), on a
/// byte start.
/// How: a 40 byte row in a Binary panel, pan right by 100 bytes
#[test]
fn terminal_raw_pan_stops_at_the_widest_row() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 12, |s| {
        s.mode = Mode::Terminal;
    });
    h.app.settings_mut().raw_side.view = RawSideView::Binary;
    // on the grid's last row (24 rows), which the 12 row view shows
    let mut bytes = b"\r\n".repeat(23);
    bytes.extend_from_slice(&[b'a'; 40]);
    h.app.terminal_mode.process_rx(TextEncoding::Utf8, &bytes);
    h.draw();
    h.app.update(Action::App(crate::inputs::AppAction::SideView(
        crate::inputs::SideViewAction::PanRight(100),
    )));
    let max = h.app.terminal_mode.raw_pan_x_max();
    assert!(max > 0 && max.is_multiple_of(9), "{max}");
    assert_eq!(h.app.terminal_mode.raw_pan_x(), max);
    // the last byte is still on screen
    assert!(shows(&h.draw(), "01100001"));
}

/// The two hex digits under the frame's cursor (MAX-71)
fn under_cursor(h: &mut Harness, rows: &[String]) -> String {
    let pos = h.terminal.get_cursor_position().unwrap();
    rows[pos.y as usize]
        .chars()
        .skip(pos.x as usize)
        .take(2)
        .collect()
}

/// MAX-71: on the raw side the copy cursor is drawn on its byte in the raw panel, also on
/// bytes the text does not show.
/// How: a green "ok" line and "abc", Hex panel; copy mode (on "abc"), Tab: on 61 ("a"); up:
/// on 1B, the ESC starting the line; Tab back: the cursor is on the "o" of the text
#[test]
fn raw_copy_cursor_is_drawn_in_the_panel() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 12);
    h.app.settings_mut().raw_side.view = RawSideView::Hex;
    h.feed(b"\x1b[32mok\x1b[0m\r\nabc\r\n");
    h.draw();
    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    h.app.update(Action::CopyMode(CopyModeAction::ToggleSide));
    let rows = h.draw();
    assert_eq!(under_cursor(&mut h, &rows), "61");
    h.app.update(Action::CopyMode(CopyModeAction::MoveUp));
    let rows = h.draw();
    assert_eq!(under_cursor(&mut h, &rows), "1B");
    h.app.update(Action::CopyMode(CopyModeAction::ToggleSide));
    let rows = h.draw();
    assert_eq!(under_cursor(&mut h, &rows), "ok");
}

/// Screen cell (x, y) where `text` starts, searched in chars (single-width cells)
fn cell_of(rows: &[String], text: &str) -> (u16, u16) {
    let needle: Vec<char> = text.chars().collect();
    for (y, row) in rows.iter().enumerate() {
        let chars: Vec<char> = row.chars().collect();
        if let Some(x) = chars.windows(needle.len()).position(|w| w == needle) {
            return (x as u16, y as u16);
        }
    }
    panic!("{text} not on screen: {rows:#?}");
}

fn bg_at(h: &Harness, (x, y): (u16, u16)) -> Option<ratatui::style::Color> {
    Some(h.terminal.backend().buffer()[(x, y)].bg)
}

/// MAX-71: the panel the cursor is not in shows a ghost of it, and a selection made on the
/// raw side covers its bytes, the ones the text does not show included. MAX-193: in the
/// `selection_raw` style while the cursor is on the raw side, `selection` back on the text.
/// How: green "ok" and "abc", Hex panel. Copy mode on "abc": ghost on 61. Raw side, up to
/// 1B: ghost on the "o" of "ok". Select 1B to 6D (the 5 bytes of ESC[32m): selected, 6F not;
/// then the text side again
#[test]
fn copy_ghosts_and_raw_selection() {
    let ghost = h_theme().ui.cursor_ghost.bg;
    let selection = h_theme().ui.selection_raw.bg;
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 12);
    h.app.settings_mut().raw_side.view = RawSideView::Hex;
    h.feed(b"\x1b[32mok\x1b[0m\r\nabc\r\n");
    h.draw();
    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    let rows = h.draw();
    assert_eq!(bg_at(&h, cell_of(&rows, "61 62")), ghost);

    h.app.update(Action::CopyMode(CopyModeAction::ToggleSide));
    h.app.update(Action::CopyMode(CopyModeAction::MoveUp));
    let rows = h.draw();
    assert_eq!(bg_at(&h, cell_of(&rows, "ok")), ghost);
    assert_ne!(bg_at(&h, cell_of(&rows, "61 62")), ghost);

    h.app.update(Action::CopyMode(CopyModeAction::Select));
    for _ in 0..4 {
        h.app.update(Action::CopyMode(CopyModeAction::MoveRight));
    }
    let rows = h.draw();
    let (x, y) = cell_of(&rows, "1B 5B 33 32 6D 6F");
    assert_eq!(bg_at(&h, (x, y)), selection);
    assert_eq!(bg_at(&h, (x + 12, y)), selection);
    assert_ne!(bg_at(&h, (x + 15, y)), selection);
    assert_ne!(selection, h_theme().ui.selection.bg);

    h.app.update(Action::CopyMode(CopyModeAction::ToggleSide));
    let rows = h.draw();
    let (x, y) = cell_of(&rows, "1B 5B 33 32 6D 6F");
    assert_eq!(bg_at(&h, (x, y)), h_theme().ui.selection.bg);
}

fn h_theme() -> ThemeConfig {
    ThemeConfig::default()
}

/// MAX-71: terminal mode's raw copy cursor walks the bytes its panel shows (the formatted
/// row, escape sequences included), with the ghost in the screen, and yanks them.
/// How: a green "ok" on the grid's last row, Hex panel. Copy mode on that row, start of
/// line, toggle: on 6F ("o", after the color sequence), ghost on the "o"; start of the raw
/// row: on 1B; select to 6D and yank: the color sequence
#[test]
fn terminal_raw_copy_cursor() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 12, |s| {
        s.mode = Mode::Terminal;
    });
    h.app.settings_mut().raw_side.view = RawSideView::Hex;
    let mut bytes = b"\r\n".repeat(23);
    bytes.extend_from_slice(b"\x1b[32mok\x1b[0m");
    h.app.terminal_mode.process_rx(TextEncoding::Utf8, &bytes);
    h.draw();
    let copy = |h: &mut Harness, action| h.app.update(Action::CopyMode(action));

    copy(&mut h, CopyModeAction::Enter);
    copy(&mut h, CopyModeAction::StartOfLine);
    copy(&mut h, CopyModeAction::ToggleSide);
    let rows = h.draw();
    assert_eq!(under_cursor(&mut h, &rows), "6F");
    assert_eq!(
        bg_at(&h, cell_of(&rows, "ok")),
        h_theme().ui.cursor_ghost.bg
    );

    copy(&mut h, CopyModeAction::StartOfLine);
    let rows = h.draw();
    assert_eq!(under_cursor(&mut h, &rows), "1B");

    copy(&mut h, CopyModeAction::Select);
    for _ in 0..4 {
        copy(&mut h, CopyModeAction::MoveRight);
    }
    match copy(&mut h, CopyModeAction::Yank) {
        UpdateEffect::CopyToClipboard(text) => assert_eq!(text, "1B 5B 33 32 6D"),
        _ => panic!("expected a yank"),
    }
}

/// MAX-127: `V` through the default keymap selects whole lines in line mode: every char of
/// them is highlighted, in the text and in the raw panel, whatever the cursor's column.
/// How: copy mode on the last line, `l` twice, `V`, `k`: both lines are highlighted from
/// their first char to their last, and their bytes in the panel.
#[test]
fn select_line_key_highlights_whole_lines() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    h.app.settings_mut().raw_side.view = RawSideView::Hex;
    h.feed(b"first line\r\nsecond line\r\n");
    h.draw();
    let key = |h: &mut Harness, code, modifiers| {
        let event = Event::Key(crossterm::event::KeyEvent::new(code, modifiers));
        let action = crate::inputs::handle_event(event, &h.app);
        h.app.update(action);
    };
    use crossterm::event::KeyCode::Char;
    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    key(&mut h, Char('l'), KeyModifiers::NONE);
    key(&mut h, Char('l'), KeyModifiers::NONE);
    key(&mut h, Char('V'), KeyModifiers::SHIFT);
    key(&mut h, Char('k'), KeyModifiers::NONE);
    assert!(h.app.line_mode.copy_state().line_select);
    let rows = h.draw();

    let selection = h_theme().ui.selection.bg;
    for (text, len) in [("first line", 10), ("second line", 11)] {
        let (x, y) = cell_of(&rows, text);
        assert_eq!(bg_at(&h, (x, y)), selection, "{text}");
        assert_eq!(bg_at(&h, (x + len - 1, y)), selection, "{text}");
    }
    // "fi" and "ne" of "first line", in the panel
    let (x, y) = cell_of(&rows, "66 69");
    assert_eq!(bg_at(&h, (x, y)), selection);
    let (x, y) = cell_of(&rows, "6E 65");
    assert_eq!(bg_at(&h, (x + 3, y)), selection);
}

/// Rows of a frame drawn with `overlay` open and the `[dialog_actions]` of `keymap`
/// (the default ones when empty)
fn popup_rows(overlay: Overlay, keymap: &[(&str, &str)]) -> Vec<String> {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    if !keymap.is_empty() {
        let config = KeyBindingsConfig {
            dialog_actions: keymap
                .iter()
                .map(|(k, a)| (k.to_string(), a.to_string()))
                .collect(),
            ..Default::default()
        };
        h.app.key_resolver = KeyResolver::try_new(&config).unwrap();
    }
    h.app.update(Action::App(AppAction::OpenOverlay(overlay)));
    h.draw()
}

fn has(rows: &[String], text: &str) -> bool {
    rows.iter().any(|row| row.contains(text))
}

/// Popup hints show the keys of the user's keymap (MAX-73).
#[test]
fn popup_hints_follow_the_keymap() {
    let serial = || Overlay::LinkSettings(LineSettingsDialog::new(&LinkConfig::default()));

    let rows = popup_rows(serial(), &[]);
    assert!(has(&rows, "Enter Apply · Esc Cancel"), "{rows:#?}");

    let rows = popup_rows(serial(), &[("x", "close"), ("ctrl-s", "confirm")]);
    assert!(has(&rows, "Ctrl-s Apply · x Cancel"), "{rows:#?}");

    let rows = popup_rows(Overlay::Help(HelpDialog::new()), &[]);
    assert!(
        has(
            &rows,
            "Left/Right Category · Up/Down Move · c Config folder · Esc Close"
        ),
        "{rows:#?}"
    );

    let rows = popup_rows(Overlay::Settings(SettingsDialog::new()), &[("x", "close")]);
    assert!(has(&rows, "x Return"), "{rows:#?}");
}

/// MAX-163: the settings show the config folder and the files a setting puts out of it;
/// `c` or a click on the folder copies its path, the file's number or a click on it the
/// file's, a number with no file nothing. The
/// help copies the folder with `c`, from any tab, or a click on its hint.
/// How: a theme set out of the folder; the settings popup drawn, `c`, clicks on both rows,
/// `1` and `2`;
/// then the help, `c` on its second tab and a click on the hint.
#[test]
fn settings_and_help_copy_the_config_folder() {
    use crossterm::event::KeyCode::{Char, Right};
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 100, 40);
    h.app.settings_mut().theme_path = Some("/themes/dark.toml".into());
    let folder = crate::configs::AppConfig::config_dir();
    let folder_text = crate::paths::shorten_home(&folder);
    let copies = |effect: UpdateEffect| match effect {
        UpdateEffect::CopyToClipboardAs { text, what } => Some((text, what)),
        _ => None,
    };
    let folder_copy = Some((
        folder.display().to_string(),
        "config folder path".to_string(),
    ));
    let press =
        |h: &mut Harness, (x, y)| mouse_do(h, MouseEventKind::Down(MouseButton::Left), (x, y));

    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::Settings(
            SettingsDialog::new(),
        ))));
    let rows = h.draw();
    assert!(
        has(&rows, &format!("[C] Config        : {folder_text}")),
        "{rows:#?}"
    );
    assert!(
        has(&rows, "[1] Theme         : /themes/dark.toml"),
        "{rows:#?}"
    );
    assert_eq!(
        copies(key_do(&mut h, Char('c'), KeyModifiers::NONE)),
        folder_copy
    );
    assert_eq!(
        copies(press(&mut h, cell_of(&rows, "[C] Config"))),
        folder_copy
    );
    let theme = Some((
        "/themes/dark.toml".to_string(),
        "theme file path".to_string(),
    ));
    assert_eq!(
        copies(press(&mut h, cell_of(&rows, "/themes/dark.toml"))),
        theme
    );
    assert_eq!(copies(key_do(&mut h, Char('1'), KeyModifiers::NONE)), theme);
    assert_eq!(copies(key_do(&mut h, Char('2'), KeyModifiers::NONE)), None);
    assert!(matches!(h.app.overlay(), Some(Overlay::Settings(_))));

    h.app.update(Action::Overlay(DialogAction::Close));
    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::Help(
            HelpDialog::new(),
        ))));
    key_do(&mut h, Right, KeyModifiers::NONE);
    assert_eq!(
        copies(key_do(&mut h, Char('c'), KeyModifiers::NONE)),
        folder_copy
    );
    let rows = h.draw();
    assert_eq!(
        copies(press(&mut h, cell_of(&rows, "Config folder"))),
        folder_copy
    );
    assert!(matches!(h.app.overlay(), Some(Overlay::Help(_))));
}

/// MAX-163: on a screen too short for the settings, the popup scrolls with a scrollbar; the
/// wheel and Down/End bring the last rows in view, the click targets follow the rows.
/// How: a 12-row screen; the popup drawn, End, drawn again, a click on the config row.
#[test]
fn settings_scroll_on_a_short_screen() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 100, 12);
    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::Settings(
            SettingsDialog::new(),
        ))));
    let rows = h.draw();
    assert!(has(&rows, "General"), "{rows:#?}");
    assert!(!has(&rows, "[C] Config"), "{rows:#?}");
    assert!(has(&rows, "▼"), "{rows:#?}");

    h.app.update(Action::Overlay(DialogAction::End));
    let rows = h.draw();
    assert!(!has(&rows, "General"), "{rows:#?}");
    assert!(has(&rows, "[C] Config"), "{rows:#?}");
    let folder = crate::configs::AppConfig::config_dir()
        .display()
        .to_string();
    let press = mouse_do(
        &mut h,
        MouseEventKind::Down(MouseButton::Left),
        cell_of(&rows, "[C] Config"),
    );
    assert!(matches!(
        press,
        UpdateEffect::CopyToClipboardAs { ref text, .. } if *text == folder
    ));

    h.app.update(Action::Overlay(DialogAction::Home));
    mouse_do(&mut h, MouseEventKind::ScrollDown, (50, 6));
    let rows = h.draw();
    assert!(!has(&rows, "General"), "{rows:#?}");
}

/// MAX-184: on 80x24 the settings show in two columns with no scrollbar, General and
/// Newlines on the left, Line Mode and Terminal Mode on the right, Files under both; a
/// click on a right-column row toggles its setting, `N` cycles what Enter sends. A
/// narrower screen has one column, scrolled. Each value is padded to the longest one, so
/// the popup keeps its width as they change (the mode, `Line` or `Terminal`).
/// How: the popup at 80x24, a click on Query Answers, `N` three times, `M`; then at 60x24.
#[test]
fn settings_in_two_columns() {
    use crate::configs::settings::TxNewlineSymbol;
    use crossterm::event::KeyCode::Char;
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let open = |h: &mut Harness| {
        let overlay = Overlay::Settings(SettingsDialog::new());
        h.app.update(Action::App(AppAction::OpenOverlay(overlay)));
        h.draw()
    };
    let rows = open(&mut h);
    for row in [
        "General                           Line Mode",
        "[M] Input Mode    : Line          [W] Line Wrap     : Word-Wrap",
        "[U] Mouse         : ON            Terminal Mode",
        "Newlines                          [A] Query Answers : ON",
        "[N] Line Enter    : LF",
        "Files",
        "[C] Config        :",
    ] {
        assert!(has(&rows, row), "{row}: {rows:#?}");
    }
    assert!(!has(&rows, "▼"), "{rows:#?}");

    let answers = h.app.settings().terminal_mode.answer_queries;
    click(&mut h, cell_of(&rows, "Query Answers"));
    assert_eq!(h.app.settings().terminal_mode.answer_queries, !answers);
    let mut eols = Vec::new();
    for _ in 0..3 {
        key_do(&mut h, Char('n'), KeyModifiers::NONE);
        eols.push(h.app.settings().line_mode.tx_eol);
    }
    use TxNewlineSymbol::{Cr, CrLf, Lf};
    assert_eq!(eols, [Cr, CrLf, Lf]);

    let before = h.app.frame_map().popup;
    key_do(&mut h, Char('m'), KeyModifiers::NONE);
    let rows = h.draw();
    assert!(
        has(&rows, "[M] Input Mode    : Terminal      [W]"),
        "{rows:#?}"
    );
    assert_eq!(h.app.frame_map().popup, before);

    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 60, 24);
    let rows = open(&mut h);
    assert!(has(&rows, "[N] Line Enter    : LF"), "{rows:#?}");
    let side_by_side = |row: &String| row.contains("[M]") && row.contains("[W]");
    assert!(!rows.iter().any(side_by_side), "{rows:#?}");
    assert!(has(&rows, "▼"), "{rows:#?}");
}

/// The resize and recording popups center their content. The recording's stays put as its
/// name gets shorter, and moves once the name outgrows the widest it has been.
/// How: the resize popup, then the recording popup with a 30-char name, 10 chars deleted,
/// then 20 typed past the start; the column of each popup's labels.
#[test]
fn form_popups_center_their_content() {
    use crossterm::event::KeyCode::{Backspace, Char};
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let column = |rows: &[String], text: &str| cell_of(rows, text).0;
    let border = |rows: &[String]| column(rows, "┌");

    let resize = Overlay::TerminalSize(ResizeDialog::new(24, 80));
    h.app.update(Action::App(AppAction::OpenOverlay(resize)));
    let rows = h.draw();
    // the popup's 2 columns of border and padding, then at least 5 more
    assert!(
        column(&rows, "Rows    :") >= border(&rows) + 2 + 5,
        "{rows:#?}"
    );
    h.app.update(Action::Overlay(DialogAction::Close));

    let name = "a".repeat(30);
    let record = Overlay::FilePath(FilePathDialog::new(Stream::Text, name));
    h.app.update(Action::App(AppAction::OpenOverlay(record)));
    let rows = h.draw();
    let stream = column(&rows, "Stream  :");
    assert!(stream > border(&rows) + 2 + 2, "{rows:#?}");
    for _ in 0..10 {
        key_do(&mut h, Backspace, KeyModifiers::NONE);
    }
    assert_eq!(column(&h.draw(), "Stream  :"), stream);
    for _ in 0..20 {
        key_do(&mut h, Char('b'), KeyModifiers::NONE);
    }
    assert!(column(&h.draw(), "Stream  :") < stream);
}

/// A name longer than the recording or screenshot popup scrolls in its field: the cursor
/// at its end stays in the popup with the name's end before it, the recording's suffix
/// after it; the screenshot's note goes while the name doesn't leave room for it.
/// How: a 100-char name ending in "zzzz" in each popup, the cursor at its end.
#[test]
fn long_names_scroll_in_their_field() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let name = format!("{}zzzz", "a".repeat(96));
    let check = |h: &mut Harness, overlay, tail: &str| {
        h.app.update(Action::App(AppAction::OpenOverlay(overlay)));
        let rows = h.draw();
        let popup = h.app.frame_map().popup.unwrap();
        let (x, y) = cursor_cell(h);
        assert!(x < popup.right() - 2, "{x} {popup:?} {rows:#?}");
        let row = &rows[y as usize];
        assert!(row.contains(tail), "{row:?}");
        assert!(!row.contains("[type filepath]"), "{row:?}");
        h.app.update(Action::Overlay(DialogAction::Close));
    };
    let record = Overlay::FilePath(FilePathDialog::new(Stream::Full, name.clone()));
    check(&mut h, record, "zzzz/");
    let shot = ScreenshotDialog::new(ScreenshotFormat::Text, &name, Screenshot::default());
    check(&mut h, Overlay::Screenshot(shot), "zzzz.txt");
}

/// MAX-12: the command line is a centered bordered popup of one row, or with `layout =
/// "status_bar"` takes the status bar's row, inside its box; the popup again when the
/// status bar is hidden. The cursor follows the text.
/// How: the command line open with `help` typed, drawn in each case.
#[test]
fn command_line_layouts() {
    use crate::configs::{CommandLineLayout, StatusBarLayout};
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 24, |s| {
        s.ui.show_borders = true;
        s.ui.status_bar.layout = StatusBarLayout::Box;
    });
    let mut command_line = CommandLine::new(Vec::new(), Vec::new(), Vec::new());
    command_line.handle_action(DialogAction::Paste("help".into()));
    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::CommandLine(
            Box::new(command_line),
        ))));

    // 64 wide, 3 high, centered: the row inside the border and the padding column
    let rows = h.draw();
    assert_eq!(cell_of(&rows, ":help"), (10, 11));
    assert_eq!(cursor_cell(&mut h), (15, 11));
    assert!(rows[10].contains(" Command "), "{:?}", rows[10]);
    assert!(has(&rows, "Enter Run · Esc Cancel"), "{rows:#?}");
    assert!(rows[22].starts_with("│ LINE "), "{:?}", rows[22]);

    h.app.settings_mut().command_line.layout = CommandLineLayout::StatusBar;
    let rows = h.draw();
    assert!(rows[22].starts_with("│:help "), "{:?}", rows[22]);
    assert!(rows[22].ends_with(" │"), "{:?}", rows[22]);
    assert_eq!(cursor_cell(&mut h), (6, 22));
    assert!(!has(&rows, " Command "), "{rows:#?}");

    h.app.settings_mut().ui.show_status_bar = false;
    let rows = h.draw();
    assert_eq!(cell_of(&rows, ":help"), (10, 11));
}

/// MAX-12: the command line scrolls as the TX input, a mark on the edge hiding text, and a
/// click on its text puts the cursor there; a click outside the popup still closes it.
/// How: 80 digits typed in the 59 columns of the popup's text, the cursor at the end.
#[test]
fn command_line_scroll_and_click() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let mut command_line = CommandLine::new(Vec::new(), Vec::new(), Vec::new());
    command_line.handle_action(DialogAction::Paste("0123456789".repeat(8)));
    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::CommandLine(
            Box::new(command_line),
        ))));
    let input_cursor = |h: &Harness| match h.app.overlay() {
        Some(Overlay::CommandLine(command_line)) => command_line.input.visual_cursor(),
        _ => panic!("the command line is closed"),
    };

    // the text starts after the `:` at column 11: 22 columns hidden on the left
    let rows = h.draw();
    let row: Vec<char> = rows[11].chars().collect();
    assert_eq!(row[10..13], [':', '◀', '3'], "{:?}", rows[11]);
    assert_eq!(row[68], '9', "{:?}", rows[11]);
    assert_eq!(cursor_cell(&mut h), (69, 11));

    click(&mut h, (16, 11));
    assert_eq!(input_cursor(&h), 27);
    let rows = h.draw();
    assert_eq!(cursor_cell(&mut h), (16, 11), "{:?}", rows[11]);

    // the `:` puts it at the text's first column shown
    click(&mut h, (10, 11));
    assert_eq!(input_cursor(&h), 22);

    click(&mut h, (2, 11));
    assert!(h.app.overlay().is_none());
}

/// MAX-12: the completion is listed under the popup, one column inside it on each side,
/// with each command's help; the selected candidate is in the `selected` color and a click
/// takes one. With the status bar layout the list is above the bar, inside it too.
/// How: `help; toggle-d` typed (3 candidates), Tab, a click.
#[test]
fn command_line_completion_list() {
    use crate::configs::{CommandLineLayout, StatusBarLayout};
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 24, |s| {
        s.ui.show_borders = true;
        s.ui.status_bar.layout = StatusBarLayout::Box;
    });
    let open = |h: &mut Harness, text: &str| {
        let mut command_line = CommandLine::new(Vec::new(), Vec::new(), Vec::new());
        command_line.handle_action(DialogAction::Paste(text.into()));
        h.app
            .update(Action::App(AppAction::OpenOverlay(Overlay::CommandLine(
                Box::new(command_line),
            ))));
        h.draw()
    };

    // the popup on columns 8 to 71 and rows 10 to 12, the list on columns 9 to 70 from row 13
    let rows = open(&mut h, "help; toggle-d");
    let row: Vec<char> = rows[13].chars().collect();
    assert_eq!((row[9], row[70]), ('┌', '┐'), "{:?}", rows[13]);
    assert_eq!(cell_of(&rows, "toggle-display-mode"), (11, 14));
    assert!(
        rows[14].contains("Cycle the display mode"),
        "{:?}",
        rows[14]
    );
    assert_eq!(cell_of(&rows, "toggle-display-timestamp"), (11, 16));
    assert!(has(&rows, "Tab Complete"), "{rows:#?}");

    h.app.update(Action::Overlay(DialogAction::CycleNext));
    let rows = h.draw();
    assert!(
        rows[11].contains(":help; toggle-display-mode"),
        "{:?}",
        rows[11]
    );
    let selected = h.theme.command_line.selected.bg;
    assert_eq!(bg_at(&h, (17, 14)), selected);
    assert_ne!(bg_at(&h, (17, 15)), selected);

    click(&mut h, (17, 15));
    let rows = h.draw();
    assert!(
        rows[11].contains(":help; toggle-display-raw "),
        "{:?}",
        rows[11]
    );
    assert!(!has(&rows, "toggle-display-mode"), "{rows:#?}");

    // the bar's box on rows 21 to 23, the list on columns 1 to 78 above it
    h.app.close_overlay();
    h.app.settings_mut().command_line.layout = CommandLineLayout::StatusBar;
    let rows = open(&mut h, "toggle-d");
    assert_eq!(cell_of(&rows, "toggle-display-mode"), (3, 17));
    let row: Vec<char> = rows[16].chars().collect();
    assert_eq!((row[1], row[78]), ('┌', '┐'), "{:?}", rows[16]);
    assert!(rows[20].starts_with('└'), "{:?}", rows[20]);
    assert!(rows[22].starts_with("│:toggle-d"), "{:?}", rows[22]);
}

/// MAX-190: every candidate is one row, its help cut with `…`; under a rule, the selected
/// command's usage and its whole help wrapped. A click on a row takes its candidate. A
/// list longer than 8 rows keeps the selected one in view, and the wheel scrolls it.
/// How: `replay-` typed on a 60-column screen, `replay-restart` (the longest help) selected,
/// a click on `replay-stop`; then every command listed, cycled to `replay-restart`, the
/// wheel down and up.
#[test]
fn command_line_selected_help_below_the_list() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 60, 24);
    let open = |h: &mut Harness, typed: &str, until: &str| {
        let mut command_line = CommandLine::new(Vec::new(), Vec::new(), Vec::new());
        if typed.is_empty() {
            command_line.handle_action(DialogAction::CycleNext);
        } else {
            command_line.handle_action(DialogAction::Paste(typed.into()));
        }
        while command_line.input.value() != until {
            command_line.handle_action(DialogAction::CycleNext);
        }
        h.app
            .update(Action::App(AppAction::OpenOverlay(Overlay::CommandLine(
                Box::new(command_line),
            ))));
        h.draw()
    };
    let rows = open(&mut h, "replay-", "replay-restart");
    let (_, y) = cell_of(&rows, "replay-restart  Play the replay again");
    assert!(rows[usize::from(y)].contains("…"), "{rows:#?}");
    let (_, rule) = cell_of(&rows, "├");
    assert!(rows[usize::from(rule)].contains('┤'), "{rows:#?}");
    let usage = crate::commands::find_by_name("replay-restart")
        .unwrap()
        .usage();
    let under = rows[usize::from(rule) + 1].trim_matches([' ', '│']);
    assert_eq!(under, usage, "{rows:#?}");
    let help: Vec<&str> = rows[usize::from(rule) + 2..]
        .iter()
        .take_while(|row| !row.contains('└'))
        // the TX prompt in the column left of the list
        .map(|row| row.trim_matches([' ', '│', '>']))
        .collect();
    assert_eq!(
        help.join(" "),
        "Play the replay again from the start, on a clean screen"
    );

    click(&mut h, cell_of(&rows, "replay-stop "));
    assert!(has(&h.draw(), ":replay-stop "));

    // every command listed: 8 rows, the selected one the last shown
    h.app.close_overlay();
    let rows = open(&mut h, "", "replay-restart");
    let corner =
        |rows: &[String], c: char| (rows.iter()).position(|row| row.chars().nth(1) == Some(c));
    let top = corner(&rows, '┌').unwrap();
    let (_, rule) = cell_of(&rows, "├");
    assert_eq!(usize::from(rule) - top - 1, 8, "{rows:#?}");
    assert!(rows[usize::from(rule) - 1].contains("replay-restart "));
    // the wheel: a row down, the selection kept; back up
    let first = |rows: &[String]| rows[top + 1].clone();
    let before = first(&rows);
    h.app.update(Action::Overlay(DialogAction::Down));
    let rows = h.draw();
    assert_ne!(first(&rows), before, "{rows:#?}");
    assert!(has(&rows, ":replay-restart"), "{rows:#?}");
    h.app.update(Action::Overlay(DialogAction::Up));
    assert_eq!(first(&h.draw()), before);
}

/// MAX-12: the default status bar shows the file `send-file` sends and its progress, only
/// while it is sent; a click on it stops it.
#[test]
fn status_bar_send_progress() {
    use crate::app::transfer::FileTransfer;
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 100, 10);
    assert!(!has(&h.draw(), "⇡"));
    let transfer = FileTransfer::new(
        Box::new(std::io::Cursor::new(vec![b'x'; 10])),
        "fw.bin".to_string(),
        10,
        960.0,
        std::time::Duration::ZERO,
        std::time::Instant::now(),
    );
    h.app
        .update(Action::App(AppAction::StartTransfer(Box::new(transfer))));
    let rows = h.draw();
    assert!(has(&rows, "⇡ fw.bin   0%"), "{rows:#?}");
    // not the "Sending fw.bin" notice
    click(&mut h, cell_of(&rows, "⇡ fw.bin"));
    assert!(h.app.transfer().is_none());
    assert!(!has(&h.draw(), "⇡"));
}

/// A text input popup types the keys that are not Esc, Enter, Tab or modified, so they
/// are not hints there.
/// How: Cancel bound to `q` only, then to `q` and Ctrl-q (`q` comes first in the order).
#[test]
fn text_input_popups_hint_only_keys_they_route() {
    let file = || Overlay::FilePath(FilePathDialog::new(Stream::Text, "log.txt".into()));
    let resize = || Overlay::TerminalSize(ResizeDialog::new(24, 80));

    let rows = popup_rows(file(), &[("q", "close"), ("enter", "confirm")]);
    assert!(has(&rows, "Enter Apply"), "{rows:#?}");
    assert!(!has(&rows, "Cancel"), "{rows:#?}");

    let rows = popup_rows(file(), &[("q", "close"), ("ctrl-q", "close")]);
    assert!(has(&rows, "Ctrl-q Cancel"), "{rows:#?}");

    let rows = popup_rows(resize(), &[]);
    assert!(
        has(&rows, "Tab Switch · Enter Apply · Esc Cancel"),
        "{rows:#?}"
    );
}

/// The alerts name the keys that answer them, and no longer Ctrl+C, which nothing binds.
/// Their text wraps, and they grow to show all of it.
#[test]
fn alert_hints_follow_the_keymap() {
    let rows = popup_rows(Overlay::DeviceDisconnected, &[]);
    assert!(has(&rows, "d Directory · Esc/Enter Quit"), "{rows:#?}");
    assert!(!has(&rows, "Ctrl"), "{rows:#?}");

    // a long port path wraps instead of being cut; the popup shows the link's address
    let port = "/dev/serial/by-id/usb-FTDI_FT232R_USB_UART_A50285BI-if00-port0";
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    h.app
        .link_mut()
        .set_address(port.parse::<Address>().unwrap());
    h.app.update(Action::App(AppAction::OpenOverlay(
        Overlay::DeviceDisconnected,
    )));
    let rows = h.draw();
    assert!(has(&rows, "if00-port0"), "{rows:#?}");
    assert!(has(&rows, "Esc/Enter Quit"), "{rows:#?}");

    let rows = popup_rows(Overlay::DeviceDetached, &[]);
    assert!(
        has(&rows, "Enter Reattach · d Directory · Esc Quit"),
        "{rows:#?}"
    );

    let error = ErrorDialog::new("Oops", "it broke");
    let rows = popup_rows(Overlay::Error(error), &[("x", "close")]);
    assert!(has(&rows, "x Dismiss"), "{rows:#?}");
}

/// MAX-163: an alert's border and title take its tone: the link and error alerts warn,
/// the quit question is neutral, the restore question is good news.
/// How: each popup opened, the color of the first letter of its title.
#[test]
fn alert_tones() {
    let title_fg = |h: &mut Harness, title: &str| {
        let rows = h.draw();
        let (x, y) = cell_of(&rows, &format!(" {title} "));
        h.terminal.backend().buffer()[(x + 1, y)].fg
    };
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let palette = h.theme.palette;
    let error = ErrorDialog::new("Oops", "it broke");
    for (overlay, title, color) in [
        (
            Overlay::DeviceDisconnected,
            "Connection Lost",
            palette.text_warn,
        ),
        (Overlay::DeviceDetached, "Detached", palette.text_warn),
        (Overlay::Error(error), "Oops", palette.text_warn),
        (Overlay::ConfirmQuit, "Quit", palette.main),
    ] {
        h.app.update(Action::App(AppAction::OpenOverlay(overlay)));
        assert_eq!(title_fg(&mut h, title), color, "{title}");
        h.app.update(Action::Overlay(DialogAction::Close));
    }

    let dir = std::env::temp_dir().join(format!("maxicom_test_{}_alert_tones", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    crate::capture::tests::write_test_session(
        &dir,
        "/dev/ttyUSB0",
        &crate::capture::CaptureFormat::default(),
        b"old line\r\n",
        b"",
    );
    h.app.settings_mut().sessions.restore = crate::configs::settings::RestoreMode::Ask;
    h.app.restore_last_session(Some(&dir), None);
    assert_eq!(title_fg(&mut h, "Restore"), palette.text_good);
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-168: the disconnected and detached popups say the link's kind: a serial port's
/// words for a port, a connection's for a TCP link.
/// How: both popups on the default port, then on a TCP address.
#[test]
fn link_popups_follow_the_kind() {
    let rows = popup_rows(Overlay::DeviceDisconnected, &[]);
    assert!(has(&rows, "SERIAL PORT DISCONNECTED"), "{rows:#?}");
    assert!(has(&rows, "Waiting for it to come back"), "{rows:#?}");
    let rows = popup_rows(Overlay::DeviceDetached, &[]);
    assert!(has(&rows, "SERIAL PORT DETACHED"), "{rows:#?}");
    assert!(has(&rows, "Other programs can open it now"), "{rows:#?}");

    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    h.app
        .link_mut()
        .set_address("tcp://192.168.1.50:4000".parse::<Address>().unwrap());
    for (overlay, words) in [
        (Overlay::DeviceDisconnected, "TCP CONNECTION LOST"),
        (Overlay::DeviceDetached, "TCP CONNECTION DETACHED"),
    ] {
        h.app.update(Action::App(AppAction::OpenOverlay(overlay)));
        let rows = h.draw();
        assert!(has(&rows, words), "{rows:#?}");
        assert!(has(&rows, "192.168.1.50:4000"), "{rows:#?}");
        assert!(!has(&rows, "SERIAL") && !has(&rows, "Port "), "{rows:#?}");
        h.app.close_overlay();
    }
}

/// MAX-199: the lost popup says the read error that closed the link, under its next line;
/// a plain close shows none.
/// How: the popup drawn alone with a reason, then without.
#[test]
fn lost_popup_says_its_reason() {
    use crate::ui::popups::alert::render_disconnected_popup;
    let theme = ThemeConfig::default();
    let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
    let draw = |reason: Option<&str>| {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| {
                let label = "COM3".to_string();
                let mut targets = Vec::new();
                let kind = crate::link::LinkKind::Serial;
                render_disconnected_popup(
                    kind,
                    label,
                    reason,
                    &resolver,
                    frame,
                    &theme,
                    &mut targets,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect::<Vec<String>>()
    };
    let reason = "failed to read from the link: Bad command (os error 22)";
    let rows = draw(Some(reason));
    let next = rows
        .iter()
        .position(|row| row.contains("Waiting for it to come back"));
    let said = rows.iter().position(|row| row.contains("(os error 22)"));
    assert!(
        matches!((next, said), (Some(n), Some(s)) if s > n),
        "{rows:#?}"
    );
    let rows = draw(None);
    assert!(!has(&rows, "failed to read"), "{rows:#?}");
}

/// MAX-12: the help's third tab lists the keys bound to a command line, the direct ones
/// first, then every command: those with arguments with their help under them, then the
/// others, one a row.
/// How: the default keymap (break and the DTR pulse after the prefix, the marks' keys after
/// the prefix and in copy mode) and `send-hex 03` on F5, the tab reached two forward from
/// the first.
#[test]
fn help_lists_the_commands() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 60);
    let mut config = KeyBindingsConfig::default();
    config
        .direct_actions
        .insert("f5".to_string(), "send-hex 03".to_string());
    h.app.key_resolver = KeyResolver::try_new(&config).unwrap();
    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::Help(
            HelpDialog::new(),
        ))));
    h.app.update(Action::Overlay(DialogAction::CycleNext));
    h.app.update(Action::Overlay(DialogAction::CycleNext));
    let rows = h.draw();
    let row_of = |rows: &[String], text: &str| rows.iter().position(|row| row.contains(text));
    let f5 = row_of(&rows, "F5 ").expect("F5");
    assert!(rows[f5].contains("-> send-hex 03"), "{rows:#?}");
    // the letters in order, a shifted one after its plain one: `L` back to live, then `U`,
    // the u-boot trigger; then the symbols (the marks', MAX-164); the copy mode keys last
    assert_eq!(row_of(&rows, "Ctrl-a + f "), Some(f5 + 1), "{rows:#?}");
    let pulse = row_of(&rows, "Ctrl-a + h ");
    assert_eq!(pulse, Some(f5 + 2), "{rows:#?}");
    assert!(
        rows[f5 + 2].contains("-> dtr off; sleep 100; dtr on"),
        "{rows:#?}"
    );
    assert_eq!(row_of(&rows, "Ctrl-a + L "), Some(f5 + 3), "{rows:#?}");
    assert_eq!(row_of(&rows, "Ctrl-a + U "), Some(f5 + 4), "{rows:#?}");
    assert_eq!(row_of(&rows, "Ctrl-a + , "), Some(f5 + 5), "{rows:#?}");
    assert!(rows[f5 + 5].contains("-> mark"), "{rows:#?}");
    assert_eq!(row_of(&rows, "copy mode , "), Some(f5 + 6), "{rows:#?}");
    assert!(has(&rows, "on the command line (Ctrl-a :)"), "{rows:#?}");

    // scrolled down until `text` shows with the row under it
    let scroll_to = |h: &mut Harness, text: &str| loop {
        let rows = h.draw();
        if let Some(row) = row_of(&rows, text).filter(|row| !rows[row + 1].contains('└')) {
            return (rows[row].clone(), rows[row + 1].clone());
        }
        assert!(
            !has(&rows, " cancel "),
            "{text} not before the last command"
        );
        h.app.update(Action::Overlay(DialogAction::Down));
    };
    let (_, help) = scroll_to(&mut h, " dtr <state>");
    assert!(help.contains("     Set the DTR line: on or off"), "{help}");
    let (quit, _) = scroll_to(&mut h, " quit ");
    assert!(quit.contains("Quit maxicom"), "{quit}");
    // the last command, whole: the scrollbar keeps its own column
    h.app.update(Action::Overlay(DialogAction::End));
    let rows = h.draw();
    let cancel = &rows[row_of(&rows, " cancel ").expect("cancel")];
    assert!(cancel.contains("and queued commands "), "{cancel}");
}

/// MAX-12: the help popup takes four fifths of the screen, 74 columns at least, and no
/// more than its whole tab bar needs (75 columns) inside the borders and the padding:
/// every tab fits from 99 columns, no arrow; at 80 the last ones hide behind `▶`.
#[test]
fn help_popup_widens() {
    for (width, popup, arrow) in [(200, 79, false), (99, 79, false), (80, 74, true)] {
        let mut h = Harness::line_mode(LineDisplayMode::Pretty, width, 24);
        h.app
            .update(Action::App(AppAction::OpenOverlay(Overlay::Help(
                HelpDialog::new(),
            ))));
        let rows = h.draw();
        let top = rows.iter().find(|row| row.contains("┌ Help")).unwrap();
        assert_eq!(top.trim().chars().count(), popup, "{rows:#?}");
        assert_eq!(has(&rows, "Dialogs"), !arrow, "{rows:#?}");
        assert_eq!(has(&rows, "▶"), arrow, "{rows:#?}");
    }
}

/// The help popup has a tab for the popup bindings, the last one (MAX-73), brought into
/// view when the tabs overflow (MAX-12).
/// How: back one tab from the first, with Close also on Ctrl-x.
#[test]
fn help_lists_the_popup_bindings() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let mut config = KeyBindingsConfig::default();
    config
        .dialog_actions
        .insert("ctrl-x".to_string(), "close".to_string());
    h.app.key_resolver = KeyResolver::try_new(&config).unwrap();
    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::Help(
            HelpDialog::new(),
        ))));
    h.app.update(Action::Overlay(DialogAction::CyclePrev));
    let rows = h.draw();

    assert!(has(&rows, "Dialogs"), "{rows:#?}");
    let bound_to_close = |rows: &[String], key: &str| {
        let line = rows.iter().find(|row| row.contains(&format!(" {key} ")));
        line.is_some_and(|row| row.contains("-> close"))
    };
    // the letters come first, the named keys and the Ctrl ones below the view
    assert!(bound_to_close(&rows, "q"), "{rows:#?}");
    h.app.update(Action::Overlay(DialogAction::End));
    let rows = h.draw();
    assert!(bound_to_close(&rows, "Esc"), "{rows:#?}");
    assert!(bound_to_close(&rows, "Ctrl-x"), "{rows:#?}");
    assert!(has(&rows, "-> cycle-next"), "{rows:#?}");
}

/// MAX-155: the prefix tab's actions start on one column, past the longest key
/// (`Shift-Right`, one char wider than the old fixed column).
/// How: the default keymap, every page of the tab, `Shift-Right` sorting in the middle.
#[test]
fn help_prefix_actions_align() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 60);
    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::Help(
            HelpDialog::new(),
        ))));
    let mut rows = h.draw();
    for _ in 0..8 {
        h.app.update(Action::Overlay(DialogAction::PageDown));
        rows.extend(h.draw());
    }
    assert!(has(&rows, "Shift-Right"), "{rows:#?}");
    let mut columns: Vec<usize> = rows.iter().filter_map(|row| row.find("-> ")).collect();
    columns.sort_unstable();
    columns.dedup();
    assert_eq!(columns.len(), 1, "{rows:#?}");
}

/// Rows inside the popup frame (between its top and bottom borders), without the borders
/// MAX-143: the help's copy tab lists the scroll keys apart from the move keys, last: the
/// copy mode's own, then the direct scroll and pan keys no copy key takes.
/// Every tab ends with a blank row.
/// How: the default keymap, the copy tab (the fourth) scrolled to its end.
#[test]
fn help_lists_the_copy_scroll_keys_apart() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::Help(
            HelpDialog::new(),
        ))));
    h.app.update(Action::Overlay(DialogAction::CycleNext));
    h.app.update(Action::Overlay(DialogAction::CycleNext));
    h.app.update(Action::Overlay(DialogAction::CycleNext));
    // the popup knows how far it scrolls once drawn
    h.draw();
    h.app.update(Action::Overlay(DialogAction::End));
    let rows = h.draw();
    let row_of = |text: &str| rows.iter().position(|row| row.contains(text));
    // the last row is blank, off the bottom border
    let bottom = rows.iter().position(|row| row.contains("└")).unwrap();
    let last = rows[bottom - 1].trim_matches(|c| " │▼".contains(c));
    assert!(last.is_empty(), "{rows:#?}");
    let header = row_of("Scrolling moves the view").expect("the scroll header");
    assert!(
        row_of("-> down").is_none_or(|row| row < header),
        "{rows:#?}"
    );
    for scroll in ["scroll-up", "scroll-down", "scroll-to-top", "page-up"] {
        let below = rows[header..]
            .iter()
            .any(|row| row.contains(&format!("-> {scroll}")));
        assert!(below, "{scroll}: {rows:#?}");
    }
    // `PageUp` moves the copy cursor, the direct `Shift-PageUp` scrolls (MAX-155)
    assert!(
        row_of("Shift-PageUp").is_some_and(|row| row > header),
        "{rows:#?}"
    );
    assert!(row_of("Ctrl-y").is_some(), "{rows:#?}");
}

fn popup_inside(rows: &[String]) -> Vec<String> {
    let top = rows.iter().position(|row| row.contains('┌')).unwrap();
    let bottom = rows.iter().position(|row| row.contains('└')).unwrap();
    rows[top + 1..bottom]
        .iter()
        .map(|row| {
            let chars: Vec<char> = row.chars().collect();
            let left = chars.iter().position(|&c| c == '│').unwrap();
            let right = chars.iter().rposition(|&c| c == '│').unwrap();
            chars[left + 1..right].iter().collect()
        })
        .collect()
}

/// The form popups are as tall as their content: one padding row above and below it, no
/// empty rows left over, and one padding column on each side.
#[test]
fn popups_fit_their_content() {
    let popups = [
        Overlay::Settings(SettingsDialog::new()),
        Overlay::LinkSettings(LineSettingsDialog::new(&LinkConfig::default())),
        Overlay::FilePath(FilePathDialog::new(Stream::Text, "log.txt".into())),
        Overlay::TerminalSize(ResizeDialog::new(24, 80)),
    ];
    for overlay in popups {
        let rows = popup_rows(overlay, &[]);
        let inside = popup_inside(&rows);
        let blank = |row: &String| row.trim().is_empty();
        assert!(blank(&inside[0]), "{rows:#?}");
        assert!(blank(&inside[inside.len() - 1]), "{rows:#?}");
        assert!(!blank(&inside[1]), "{rows:#?}");
        assert!(!blank(&inside[inside.len() - 2]), "{rows:#?}");
        assert!(
            inside
                .iter()
                .all(|row| row.starts_with(' ') && row.ends_with(' ')),
            "{rows:#?}"
        );
    }
}

/// The serial popup shows the data and stop bits as numbers.
#[test]
fn serial_popup_values_are_readable() {
    let dialog = LineSettingsDialog::new(&LinkConfig::default());
    let rows = popup_rows(Overlay::LinkSettings(dialog), &[]);
    assert!(has(&rows, "< 8 >"), "{rows:#?}");
    assert!(has(&rows, "< 1 >"), "{rows:#?}");
}

/// MAX-156: a TCP link's popup shows its address and nothing to edit, and any key but
/// navigation closes it without reconfiguring.
/// How: the popup rows for a `tcp://` port, then its answers to Confirm and Right.
#[test]
fn serial_popup_for_a_tcp_link() {
    let config = LinkConfig {
        address: "tcp://192.168.1.50:4000".parse::<Address>().unwrap(),
        ..LinkConfig::default()
    };
    let rows = popup_rows(Overlay::LinkSettings(LineSettingsDialog::new(&config)), &[]);
    assert!(has(&rows, "Network Link"), "{rows:#?}");
    assert!(has(&rows, "tcp://192.168.1.50:4000"), "{rows:#?}");
    assert!(
        !has(&rows, "Baud Rate") && !has(&rows, "< 8 >"),
        "{rows:#?}"
    );

    let mut dialog = LineSettingsDialog::new(&config);
    assert!(matches!(
        dialog.handle_action(DialogAction::Right),
        crate::overlays::OverlayResponse::Handled
    ));
    assert_eq!(
        (&dialog.address, dialog.line),
        (&config.address, config.line)
    );
    assert!(matches!(
        dialog.handle_action(DialogAction::Confirm),
        crate::overlays::OverlayResponse::Close
    ));
}

/// MAX-166: a replayed session's popup shows what it was recorded with, and that a replay
/// has nothing to set.
/// How: the popup rows for a `file://` port with a recorded 9600 7E1.
#[test]
fn serial_popup_for_a_session_replay() {
    let config = LinkConfig {
        address: "file:///tmp/boot.raw".parse::<Address>().unwrap(),
        ..LinkConfig::default()
    };
    let line = crate::capture::CaptureLine {
        baud: 9600,
        framing: "7E1".parse().unwrap(),
    };
    let dialog = LineSettingsDialog::new(&config).with_recorded_line(Some(line));
    let rows = popup_rows(Overlay::LinkSettings(dialog), &[]);
    assert!(has(&rows, "Recorded    : 9600 7E1"), "{rows:#?}");
    assert!(has(&rows, "A replay plays what was recorded"), "{rows:#?}");
    assert!(!has(&rows, "Baud Rate"), "{rows:#?}");
}

/// MAX-166: a replay's popup shows its address and that a file has nothing to set.
/// How: the popup rows for a `file://` port.
#[test]
fn serial_popup_for_a_replay() {
    let config = LinkConfig {
        address: "file:///tmp/boot.raw".parse::<Address>().unwrap(),
        ..LinkConfig::default()
    };
    let rows = popup_rows(Overlay::LinkSettings(LineSettingsDialog::new(&config)), &[]);
    assert!(
        has(&rows, "Replay") && !has(&rows, "Network Link"),
        "{rows:#?}"
    );
    assert!(has(&rows, "file:///tmp/boot.raw"), "{rows:#?}");
    assert!(
        has(&rows, "A replayed file has no baud nor framing"),
        "{rows:#?}"
    );
    assert!(!has(&rows, "Baud Rate"), "{rows:#?}");
}

/// The text cursor of the text input popups sits right after the typed text.
#[test]
fn text_input_cursor_follows_the_text() {
    let popups = [
        (
            Overlay::FilePath(FilePathDialog::new(Stream::Text, "log.txt".into())),
            "log.txt",
        ),
        (Overlay::TerminalSize(ResizeDialog::new(24, 80)), "24"),
    ];
    for (overlay, text) in popups {
        let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
        h.app.update(Action::App(AppAction::OpenOverlay(overlay)));
        let rows = h.draw();
        let (x, y) = cell_of(&rows, text);
        let cursor = h.terminal.get_cursor_position().unwrap();
        assert_eq!(
            (cursor.x, cursor.y),
            (x + text.len() as u16, y),
            "{rows:#?}"
        );
    }
}

/// The search bar hint names the keys of the user's `[search_mode]` (MAX-73), with or
/// without borders, and only the hints that fit whole.
/// How: the default keymap, then Next on F3, Previous on Shift-F3 and Cancel on Ctrl-g only
/// (Kind and Confirm unbound: no hint).
#[test]
fn search_bar_hint_follows_the_keymap() {
    for show_borders in [true, false] {
        let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 90, 10, |settings| {
            settings.ui.show_borders = show_borders;
        });
        h.feed(b"hi there\r\n");
        search(&mut h, "hi");
        let rows = h.draw();
        assert!(
            has(
                &rows,
                "Ctrl-n/Ctrl-p Match · Esc Exit · Enter Copy · Tab Mode"
            ),
            "{rows:#?}"
        );

        let config = KeyBindingsConfig {
            search_mode: [
                ("f3", "next"),
                ("shift-f3", "previous"),
                ("ctrl-g", "cancel"),
            ]
            .iter()
            .map(|(k, a)| (k.to_string(), a.to_string()))
            .collect(),
            ..Default::default()
        };
        h.app.key_resolver = KeyResolver::try_new(&config).unwrap();
        let rows = h.draw();
        assert!(has(&rows, "F3/Shift-F3 Match · Ctrl-g Exit"), "{rows:#?}");
        assert!(!has(&rows, "Tab"), "{rows:#?}");
    }

    // a narrow bar shows the hints that fit whole, Match first: Esc Exit would end on
    // column 58
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 56, 10);
    h.feed(b"hi there\r\n");
    search(&mut h, "hi");
    let rows = h.draw();
    assert!(has(&rows, "(1/1)  Ctrl-n/Ctrl-p Match "), "{rows:#?}");
    assert!(!has(&rows, "Match ·"), "{rows:#?}");
}

/// A mouse event at a screen cell
fn mouse(kind: MouseEventKind, (x, y): (u16, u16), modifiers: KeyModifiers) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers,
    })
}

/// The wheel scrolls the history, and pans it with Shift or sideways; over the raw panel
/// Shift+wheel pans the panel. In copy mode too, the copy cursor stays (MAX-143), and in a
/// popup it moves up and down (MAX-113).
/// How: line mode with the Hex panel, events on the history and on the panel, as they
/// were last drawn.
#[test]
fn mouse_wheel_scrolls_and_pans() {
    use crate::inputs::{DialogAction, SideViewAction, handle_event};
    use MouseEventKind::{ScrollDown, ScrollLeft, ScrollRight, ScrollUp};

    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 100, 12);
    h.app.settings_mut().raw_side.view = RawSideView::Hex;
    for i in 0..30 {
        h.feed(format!("line {i:02} with some text\r\n").as_bytes());
    }
    h.draw();
    let areas = h.app.frame_areas();
    let text = (areas.content.x + 1, areas.content.y + 1);
    let raw = areas.raw_side.unwrap();
    let raw = (raw.x + 1, raw.y + 1);
    let none = KeyModifiers::NONE;
    let shift = KeyModifiers::SHIFT;
    let wheel = |h: &Harness, kind, at, modifiers| handle_event(mouse(kind, at, modifiers), &h.app);

    assert!(matches!(
        wheel(&h, ScrollUp, text, none),
        Action::History(HistoryAction::ScrollUp(3))
    ));
    assert!(matches!(
        wheel(&h, ScrollDown, raw, none),
        Action::History(HistoryAction::ScrollDown(3))
    ));
    assert!(matches!(
        wheel(&h, ScrollDown, text, shift),
        Action::History(HistoryAction::PanRight(4))
    ));
    assert!(matches!(
        wheel(&h, ScrollLeft, text, none),
        Action::History(HistoryAction::PanLeft(4))
    ));
    assert!(matches!(
        wheel(&h, ScrollUp, raw, shift),
        Action::App(AppAction::SideView(SideViewAction::PanLeft(2)))
    ));
    assert!(matches!(
        wheel(&h, ScrollRight, raw, none),
        Action::App(AppAction::SideView(SideViewAction::PanRight(2)))
    ));

    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    assert!(matches!(
        wheel(&h, ScrollUp, text, none),
        Action::History(HistoryAction::ScrollUp(3))
    ));
    assert!(matches!(
        wheel(&h, ScrollUp, text, shift),
        Action::History(HistoryAction::PanLeft(4))
    ));
    h.app.update(Action::CopyMode(CopyModeAction::Exit));

    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::Help(
            HelpDialog::new(),
        ))));
    assert!(matches!(
        wheel(&h, ScrollDown, text, none),
        Action::Overlay(DialogAction::Down)
    ));
}

/// MAX-197: a press anywhere on a toast dismisses that one, the others kept, and goes no
/// further: the popup under it stays open. A press beside the toasts doesn't touch them.
/// How: two toasts over the help popup, a press on the first one's text, then on the
/// second one's border, then one far from both.
#[test]
fn click_dismisses_a_toast() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    h.app.notify_info("first toast");
    h.app.notify_warn("second toast");
    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::Help(
            HelpDialog::new(),
        ))));
    let rows = h.draw();
    let first = cell_of(&rows, "first toast");
    mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), first);
    let rows = h.draw();
    assert!(!has(&rows, "first toast"), "{rows:#?}");
    assert!(has(&rows, "second toast"), "{rows:#?}");
    assert!(matches!(h.app.overlay(), Some(Overlay::Help(_))));

    let (x, y) = cell_of(&rows, "Warning");
    mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), (x, y));
    let rows = h.draw();
    assert!(!has(&rows, "second toast"), "{rows:#?}");
    assert!(matches!(h.app.overlay(), Some(Overlay::Help(_))));

    h.app.notify_info("third toast");
    h.draw();
    click(&mut h, (0, 23));
    let rows = h.draw();
    assert!(has(&rows, "third toast"), "{rows:#?}");
}

/// Terminal mode: a plain click only focuses, it doesn't enter copy mode, so the next keys
/// still reach the device. A drag selects from the press and copies on the release, a double
/// click its word, as before; in copy mode a click moves the cursor. Line mode
/// keeps a click on the history entering copy mode.
/// How: two lines on the grid; a click on "hello", a drag from it to "world", a double
/// click on "second", a click while in copy mode; then a click in line mode.
#[test]
fn terminal_click_only_focuses() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 12, |s| {
        s.mode = Mode::Terminal;
    });
    h.app.feed_rx(b"hello world\r\nsecond line");
    let rows = h.draw();
    let (hello, world) = (cell_of(&rows, "hello"), cell_of(&rows, "world"));
    let down = MouseEventKind::Down(MouseButton::Left);
    let up = MouseEventKind::Up(MouseButton::Left);

    click(&mut h, hello);
    assert_eq!(h.app.copy_side(), None);
    assert_eq!(h.app.input_context(), crate::app::InputContext::Terminal);

    mouse_do(&mut h, down, hello);
    mouse_do(
        &mut h,
        MouseEventKind::Drag(MouseButton::Left),
        (world.0 + 4, world.1),
    );
    assert_eq!(h.app.copy_side(), Some(CopySide::Text));
    let effect = mouse_do(&mut h, up, (world.0 + 4, world.1));
    let copied = match effect {
        UpdateEffect::CopyToClipboard(text) => text,
        _ => panic!("no copy on the release"),
    };
    assert_eq!(copied, "hello world");
    assert_eq!(h.app.copy_side(), None);

    let rows = h.draw();
    let second = cell_of(&rows, "second");
    click(&mut h, second);
    assert_eq!(h.app.copy_side(), None);
    mouse_do(&mut h, down, second);
    assert_eq!(h.app.copy_side(), Some(CopySide::Text));
    let copied = match mouse_do(&mut h, up, second) {
        UpdateEffect::CopyToClipboard(text) => text,
        _ => panic!("no copy on the double click's release"),
    };
    assert_eq!(copied, "second");

    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    click(&mut h, hello);
    assert_eq!(h.app.copy_side(), Some(CopySide::Text));
    h.app.update(Action::CopyMode(CopyModeAction::Exit));

    let mut l = Harness::line_mode(LineDisplayMode::Pretty, 80, 12);
    l.feed(b"hello\r\n");
    let rows = l.draw();
    click(&mut l, cell_of(&rows, "hello"));
    assert_eq!(l.app.copy_side(), Some(CopySide::Text));
}

/// The click that focuses the window doesn't select, in either mode: a press while it's
/// unfocused (a terminal that sends the click first), or right after it gained focus (one
/// that sends the focus first). A later click selects again; a quick second click after a
/// focus gained late (after the press) too.
/// How: line mode history, the focus lost then each order; a click 250 ms after the focus.
#[test]
fn focusing_click_doesnt_select() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 12);
    h.feed(b"hello\r\n");
    let rows = h.draw();
    let at = cell_of(&rows, "hello");
    let focus = |h: &mut Harness, on: bool| h.app.update(Action::App(AppAction::FocusGained(on)));
    // whether a click's press enters copy mode (quick clicks here make a double click,
    // which copies on the release: read on the press)
    let selects = |h: &mut Harness| {
        mouse_do(h, MouseEventKind::Down(MouseButton::Left), at);
        let on = h.app.copy_side().is_some();
        mouse_do(h, MouseEventKind::Up(MouseButton::Left), at);
        h.app.update(Action::CopyMode(CopyModeAction::Exit));
        on
    };

    // the click before the focus
    focus(&mut h, false);
    assert!(!selects(&mut h));
    focus(&mut h, true);
    assert!(selects(&mut h));

    // the focus before the click
    focus(&mut h, false);
    focus(&mut h, true);
    assert!(!selects(&mut h));
    assert!(selects(&mut h));

    // back by the keyboard, then a click later on
    focus(&mut h, false);
    focus(&mut h, true);
    std::thread::sleep(std::time::Duration::from_millis(250));
    assert!(selects(&mut h));
}

/// MAX-204: with `focus_effects` off the window keeps its focused look when it loses the
/// focus (the focused panel's border here), while the focusing click is still only a focus.
/// How: line mode with borders, the input box's corner color focused then unfocused, with
/// the setting on then off; then a click while unfocused.
#[test]
fn focus_effects_off_keeps_the_focused_look() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 12, |s| {
        s.ui.show_borders = true;
    });
    h.feed(b"hello\r\n");
    let focus = |h: &mut Harness, on: bool| h.app.update(Action::App(AppAction::FocusGained(on)));
    // the input box's corner: the focused panel's border, gray when the window isn't focused
    let corner = |h: &mut Harness| {
        let y = cell_of(&h.draw(), "Transmit").1;
        h.terminal.backend().buffer()[(0, y)].fg
    };
    let focused = corner(&mut h);
    focus(&mut h, false);
    assert_ne!(corner(&mut h), focused);

    h.app.settings_mut().ui.focus_effects = false;
    assert_eq!(corner(&mut h), focused);
    let rows = h.draw();
    mouse_do(
        &mut h,
        MouseEventKind::Down(MouseButton::Left),
        cell_of(&rows, "hello"),
    );
    assert_eq!(h.app.copy_side(), None);
}

/// Terminal mode scrolled into the history: the real cursor isn't placed, a fake one is drawn
/// where the remote's cursor still shows, in the unfocused cursor's style, so it doesn't
/// vanish. Live, the real cursor is back.
/// How: the grid sized to the view, 30 rows then the cursor 4 rows up from the bottom, the
/// view scrolled up 2 rows.
#[test]
fn scrolled_terminal_keeps_a_fake_cursor() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 12, |s| {
        s.mode = Mode::Terminal;
    });
    // the grid as tall as the view: scrolling up goes into the history, not a pan of it
    h.draw();
    h.app.update(Action::Terminal(
        crate::inputs::TerminalAction::AutoResizeEmulatorToViewer,
    ));
    let bytes: String = (0..30).map(|i| format!("row {i:02}\r\n")).collect();
    h.app.feed_rx(bytes.as_bytes());
    h.app.feed_rx(b"\x1b[4A\x1b[2Kat");
    let rows = h.draw();
    let live = cursor_cell(&mut h);
    assert_eq!(live, (cell_of(&rows, "at").0 + 2, cell_of(&rows, "at").1));

    h.scroll(HistoryAction::ScrollUp(2));
    let rows = h.draw();
    let (x, y) = cell_of(&rows, "at");
    // on the empty cell after "at": the unfocused glyph in the unfocused cursor's color
    let fake = &h.terminal.backend().buffer()[(x + 2, y)];
    assert_eq!(fake.symbol(), "█", "{rows:#?}");
    assert_eq!(Some(fake.fg), h_theme().ui.cursor_unfocused.fg);

    h.scroll(HistoryAction::ScrollToBottom);
    let rows = h.draw();
    assert_eq!(cursor_cell(&mut h), live, "{rows:#?}");
}

/// A mouse event through the input path, applied
fn mouse_do(h: &mut Harness, kind: MouseEventKind, at: (u16, u16)) -> UpdateEffect {
    let action = crate::inputs::handle_event(mouse(kind, at, KeyModifiers::NONE), &h.app);
    h.app.update(action)
}

fn click(h: &mut Harness, at: (u16, u16)) {
    mouse_do(h, MouseEventKind::Down(MouseButton::Left), at);
    mouse_do(h, MouseEventKind::Up(MouseButton::Left), at);
}

fn cursor_cell(h: &mut Harness) -> (u16, u16) {
    let pos = h.terminal.get_cursor_position().unwrap();
    (pos.x, pos.y)
}

/// Wrapped lines with a timestamp gutter, in line mode
fn wrapped_lines() -> Harness {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 40, 12, |settings| {
        settings.show_timestamps = true;
    });
    for i in 0..30 {
        h.feed(format!("line {i:02} alpha beta gamma delta epsilon zeta eta theta\r\n").as_bytes());
    }
    h
}

/// A click in the history puts the copy cursor on the clicked char, on a first row or a
/// wrapped one, and the view stays where it was (MAX-113).
/// How: wrapped lines with timestamps; click a char of a first row, then one of a
/// continuation row while in copy mode; the cursor is drawn on the clicked cell and the
/// history rows don't move.
#[test]
fn mouse_click_puts_the_copy_cursor_there() {
    let mut h = wrapped_lines();
    let before = h.draw();
    let height = h.app.frame_areas().content.height as usize;

    let (x, y) = cell_of(&before, "alpha");
    click(&mut h, (x + 2, y));
    let after = h.draw();
    assert!(h.app.line_mode.copy_state().active);
    assert_eq!(cursor_cell(&mut h), (x + 2, y), "{after:#?}");
    assert_eq!(before[..height], after[..height]);

    let (x, y) = cell_of(&after, "theta");
    click(&mut h, (x + 4, y));
    let again = h.draw();
    assert_eq!(cursor_cell(&mut h), (x + 4, y), "{again:#?}");
    assert_eq!(before[..height], again[..height]);

    // a key step that stays in view doesn't move it either (MAX-143)
    h.app.update(Action::CopyMode(CopyModeAction::MoveDown));
    let moved = h.draw();
    assert!(cursor_cell(&mut h).1 > y, "{moved:#?}");
    assert_eq!(before[..height], moved[..height]);
}

/// The cell of `text` on the row holding `on`, for labels drawn more than once.
fn cell_on_row(rows: &[String], on: &str, text: &str) -> (u16, u16) {
    let y = cell_of(rows, on).1;
    let row = &rows[y as usize];
    let x = row[..row.find(text).unwrap()].chars().count();
    (x as u16, y)
}

/// MAX-167: the trigger browser lists the triggers with their state (`◎` armed, `○` not) and
/// the selected one's rule on the side; a click selects a row, a click on the selected one or
/// the Toggle button toggles it, Toggle all toggles them all, and it stays open. The status
/// bar's count opens it. The list and the rule under their titles, the rule's the selected
/// name, the file's path from `~` (MAX-183).
/// How: the default triggers, the popup opened by its action, then by the status bar.
#[test]
fn trigger_browser_lists_and_toggles() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 120, 30);
    h.app
        .set_triggers(crate::configs::TriggersConfig::default().compile().unwrap());
    h.app.update(KeyBindingAction::Triggers.to_action(&h.app));
    let rows = h.draw();
    let shows = |rows: &[String], texts: &[&str]| {
        for text in texts {
            assert!(has(rows, text), "{text}: {rows:#?}");
        }
    };
    shows(
        &rows,
        &[
            "Rules     login",
            "○ login",
            "○ u-boot",
            "State  disarmed",
            r#"Match  "login: ""#,
            "Run    send root\\r",
            "Fires  each time, it stays armed",
        ],
    );

    click(&mut h, cell_of(&rows, "u-boot"));
    let rows = h.draw();
    shows(
        &rows,
        &[
            "Rules     u-boot",
            "Run    send ' '",
            "Fires  once, then disarms",
        ],
    );
    assert!(h.app.armed_triggers().is_empty());
    // the row, not the rule's title
    click(&mut h, cell_of(&rows, "○ u-boot"));
    assert_eq!(h.app.armed_triggers(), ["u-boot"]);
    let rows = h.draw();
    shows(&rows, &["○ login", "◎ u-boot", "State  armed"]);
    click(&mut h, cell_of(&rows, "Toggle all"));
    assert!(h.app.armed_triggers().is_empty());
    click(&mut h, cell_of(&rows, "Toggle all"));
    assert_eq!(h.app.armed_triggers(), ["login", "u-boot"]);
    let (x, y) = cell_of(&rows, " Toggle ");
    click(&mut h, (x + 1, y));
    assert_eq!(h.app.armed_triggers(), ["login"]);
    assert!(matches!(h.app.overlay(), Some(Overlay::Triggers(_))));

    h.app.update(Action::Overlay(DialogAction::Close));
    let rows = h.draw();
    click(&mut h, cell_of(&rows, "◎ 1"));
    assert!(matches!(h.app.overlay(), Some(Overlay::Triggers(_))));
}

/// MAX-181: the directory lists the serial ports under their title, `●` and "in use" on
/// the device in use, which starts selected; Enter on another switches to it, as `open`
/// does, the popup staying with "opening…" on it until it's open. With no port it says
/// how to list them again.
/// How: the default device and `/dev/ttyACM0` listed, Down and Enter; then an empty list.
#[test]
fn directory_lists_and_opens_the_ports() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let current = h.app.link_config().address.clone();
    let ports = vec![current.to_string(), "/dev/ttyACM0".to_string()];
    let directory = crate::overlays::Directory::new(ports, &current);
    let open = Overlay::Directory(directory);
    h.app.update(Action::App(AppAction::OpenOverlay(open)));
    // not open: like the other ports
    let rows = h.draw();
    assert!(has(&rows, &format!("  {current}")), "{rows:#?}");
    assert!(!has(&rows, "in use"), "{rows:#?}");
    // open (a file stands for the device): in use
    let path = std::env::temp_dir().join(format!("maxicom_dir_{}.raw", std::process::id()));
    std::fs::write(&path, b"").unwrap();
    let replay = crate::link::replay::Replay::open(&path, std::time::Duration::ZERO).unwrap();
    h.app.link_mut().set_replay(replay);
    let rows = h.draw();
    let _ = std::fs::remove_file(&path);
    assert!(has(&rows, "Serial ports"), "{rows:#?}");
    assert!(has(&rows, &format!("● {current}  in use")), "{rows:#?}");
    assert!(has(&rows, "  /dev/ttyACM0"), "{rows:#?}");

    h.app.update(Action::Overlay(DialogAction::Down));
    let effect = h.app.update(Action::Overlay(DialogAction::Confirm));
    assert!(matches!(
        effect,
        UpdateEffect::Open(address) if address.to_string() == "/dev/ttyACM0"
    ));
    assert!(has(&h.draw(), "/dev/ttyACM0  opening…"));

    let empty = crate::overlays::Directory::new(Vec::new(), &current);
    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::Directory(
            empty,
        ))));
    let rows = h.draw();
    assert!(
        has(&rows, "No serial port found: plug one in, then r"),
        "{rows:#?}"
    );
}

/// MAX-185 step 0: the directory's title and buttons; a click on a row selects it, a click
/// on the selected one or on Open opens it, Close closes the directory.
/// How: three ports, the bottom border read, clicks on the third row (twice), on the
/// first row then Open, then on Close.
#[test]
fn directory_buttons_and_clicks() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let current = h.app.link_config().address.clone();
    let ports = vec![
        current.to_string(),
        "/dev/ttyACM0".to_string(),
        "/dev/ttyACM1".to_string(),
    ];
    let directory = crate::overlays::Directory::new(ports, &current);
    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::Directory(
            directory,
        ))));
    let rows = h.draw();
    assert!(has(&rows, "Directory"), "{rows:#?}");
    assert!(
        has(
            &rows,
            "Enter Open · s Save · t Test · r Refresh · Esc Close"
        ),
        "{rows:#?}"
    );
    let opens = |effect: UpdateEffect| match effect {
        UpdateEffect::Open(address) => Some(address.to_string()),
        _ => None,
    };
    let selected = |h: &Harness| match h.app.overlay() {
        Some(Overlay::Directory(directory)) => directory.selected,
        _ => panic!("no directory"),
    };

    let at = cell_of(&rows, "/dev/ttyACM1");
    click(&mut h, at);
    assert_eq!(selected(&h), 2);
    h.draw();
    // a popup click acts on the press
    let effect = mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), at);
    mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), at);
    assert_eq!(opens(effect).as_deref(), Some("/dev/ttyACM1"));

    let rows = h.draw();
    click(&mut h, cell_of(&rows, &current.to_string()));
    assert_eq!(selected(&h), 0);
    let rows = h.draw();
    let (x, y) = cell_of(&rows, "Enter Open");
    let effect = mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), (x + 6, y));
    mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), (x + 6, y));
    assert_eq!(opens(effect), Some(current.to_string()));

    let rows = h.draw();
    let (x, y) = cell_of(&rows, "Esc Close");
    click(&mut h, (x + 4, y));
    assert!(h.app.overlay().is_none());
}

/// MAX-185: the saved links of this machine in columns, name, address and line (`raw TCP`
/// for TCP), centered (MAX-188), the ones for another machine left out; the Saved tab's buttons; a click on
/// the ports' tab shows them with theirs. The tabs touch the top border, and both tabs
/// have the same size. A `links.toml` that didn't load shows why; no
/// link says so.
/// How: three links, one for another machine; the tabs clicked; then an error, then none.
#[test]
fn directory_saved_tab() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let links = "[[link]]\nname = 'esp32'\naddress = '/dev/ttyUSB9'\nbaud = 9600\n\
                     [[link]]\nname = 'lab'\nhosts = ['not-this-machine-0']\naddress = 'COM3'\n\
                     [[link]]\nname = 'router'\naddress = 'tcp://10.0.0.1:4000'\n";
    h.app.set_links(Ok(toml::from_str(links).unwrap()));
    let open = |h: &mut Harness| {
        let directory = h.app.directory(vec!["/dev/ttyACM0".to_string()]);
        h.app
            .update(Action::App(AppAction::OpenOverlay(Overlay::Directory(
                directory,
            ))));
        h.draw()
    };
    let rows = open(&mut h);
    assert!(has(&rows, "Saved links │ Serial ports"), "{rows:#?}");
    assert!(
        has(&rows, "  esp32   /dev/ttyUSB9         9600 8N1"),
        "{rows:#?}"
    );
    assert!(
        has(&rows, "  router  tcp://10.0.0.1:4000  raw TCP"),
        "{rows:#?}"
    );
    assert!(!has(&rows, "COM3"), "{rows:#?}");
    // MAX-188: narrower than the popup, the table is centered in it
    let row = rows.iter().find(|row| row.contains("esp32")).unwrap();
    let inside = row.trim().trim_matches('│');
    let left = inside.len() - inside.trim_start().len();
    // the State column, empty before a test, as blank
    let state = "unknown host".len() + 2;
    let right = inside.len() - inside.trim_end().len() - state;
    assert!(
        left > 2 && left.abs_diff(right) <= 1,
        "{left} {right}: {rows:#?}"
    );
    // MAX-186: the test from this tab too
    let note = crate::ui::popups::directory::TEST_NOTE;
    assert!(has(&rows, note), "{rows:#?}");
    assert!(
        has(
            &rows,
            "Enter Open · n New · e Edit · s Save · x Delete · t Test · Esc Close"
        ),
        "{rows:#?}"
    );

    click(&mut h, cell_of(&rows, "Serial ports"));
    let rows = h.draw();
    assert!(has(&rows, "  /dev/ttyACM0"), "{rows:#?}");
    assert!(
        has(
            &rows,
            "Enter Open · s Save · t Test · r Refresh · Esc Close"
        ),
        "{rows:#?}"
    );
    click(&mut h, cell_of(&rows, "Saved links"));
    assert!(has(&h.draw(), "esp32"));

    // the tabs on the row under the top border; both tabs the same size
    let frame = |h: &mut Harness| {
        let rows = h.draw();
        let top = rows.iter().position(|r| r.contains("┌ Directory")).unwrap();
        let bottom = rows.iter().position(|r| r.contains("└ Enter")).unwrap();
        let width = rows[top].trim().chars().count();
        assert!(
            rows[top + 1].contains("Saved links │ Serial ports"),
            "{rows:#?}"
        );
        (top, bottom, width)
    };
    let saved = frame(&mut h);
    h.app.update(Action::Overlay(DialogAction::CycleNext));
    assert_eq!(frame(&mut h), saved);

    h.app
        .set_links(Err("invalid syntax in 'links.toml':\nline 2".into()));
    let rows = open(&mut h);
    assert!(has(&rows, "invalid syntax in 'links.toml':"), "{rows:#?}");
    assert!(has(&rows, "line 2"), "{rows:#?}");

    h.app.set_links(Ok(crate::configs::LinksConfig::default()));
    open(&mut h);
    h.app.update(Action::Overlay(DialogAction::CycleNext));
    assert!(has(&h.draw(), "No saved link for this machine"));
}

/// MAX-185: the form shows a row per field, the picked one marked with the cursor in it,
/// the line values between arrows; no hosts says every machine; a TCP address mutes the
/// line and says why; a click picks a field.
/// How: a new serial link, then a TCP address, then a click on the address row.
#[test]
fn link_form_popup() {
    use crate::overlays::LinkForm;
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 30);
    let link = crate::configs::links::SavedLink::default();
    let mut form = LinkForm::new(link, &crate::configs::LinkDefaults::default());
    form.name = "esp32".to_string().into();
    let open = |h: &mut Harness, form: &LinkForm| {
        let overlay = Overlay::LinkForm(Box::new(form.clone()));
        h.app.update(Action::App(AppAction::OpenOverlay(overlay)));
        h.draw()
    };
    let rows = open(&mut h, &form);
    for row in [
        "New link",
        "▶ Name      : esp32",
        "  For       : every machine",
        "  Baud rate : 115200",
        "  Data bits : < 8 >",
        "  Parity    : < None >",
        "  Flow ctrl : < None >",
        "Tab Next · Enter Save · Esc Cancel",
    ] {
        assert!(has(&rows, row), "{row}: {rows:#?}");
    }
    let (x, y) = cell_of(&rows, "▶ Name      : esp32");
    assert_eq!(cursor_cell(&mut h), (x + 19, y));
    assert!(!has(&rows, "bridge"), "{rows:#?}");

    form.address = "tcp://10.0.0.1:4000".to_string().into();
    let rows = open(&mut h, &form);
    assert!(
        has(&rows, "  A TCP link's bridge sets its line"),
        "{rows:#?}"
    );
    click(&mut h, cell_of(&rows, "  Address   :"));
    let Some(Overlay::LinkForm(form)) = h.app.overlay() else {
        panic!("no form");
    };
    assert_eq!(form.field, crate::overlays::LinkFormField::Address);
}

/// MAX-185: the link's popups name a saved link by its name, its address after it.
/// How: the app's `/dev/ttyUSB0` saved as `esp32`, the detached then the lost popup.
#[test]
fn link_popups_name_a_saved_link() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let text = "[[link]]\nname = 'esp32'\naddress = '/dev/ttyUSB0'\n";
    h.app.set_links(Ok(toml::from_str(text).unwrap()));
    for overlay in [Overlay::DeviceDetached, Overlay::DeviceDisconnected] {
        h.app.update(Action::App(AppAction::OpenOverlay(overlay)));
        let rows = h.draw();
        assert!(has(&rows, "Port esp32 (/dev/ttyUSB0)"), "{rows:#?}");
        assert!(has(&rows, " LINE | esp32 |"), "{rows:#?}");
    }
}

/// MAX-185: the delete question names the link and its address, with its keys.
#[test]
fn delete_link_popup() {
    let link = crate::configs::links::SavedLink {
        name: "esp32".into(),
        address: "/dev/ttyUSB0".into(),
        ..Default::default()
    };
    let rows = popup_rows(Overlay::DeleteLink(link), &[]);
    for row in [
        "Delete link",
        "Delete the link 'esp32'?",
        "/dev/ttyUSB0",
        "Enter Delete · Esc Cancel",
    ] {
        assert!(has(&rows, row), "{row}: {rows:#?}");
    }
}

/// MAX-181: the directory warns what its test does, and shows each tested port's result
/// after its name; more ports than rows, a scrollbar in its right padding.
/// How: six ports with a test result each but the device in use; then 40 ports.
#[test]
fn directory_test_marks_and_scrollbar() {
    use crate::link::probe::ProbeState;
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let current = h.app.link_config().address.clone();
    let ports = [
        "/dev/ttyACM0",
        "/dev/ttyACM1",
        "/dev/ttyACM2",
        "/dev/ttyACM3",
        "/dev/ttyACM4",
        "/dev/ttyACM5",
    ];
    let mut all = vec![current.to_string()];
    all.extend(ports.iter().map(|port| port.to_string()));
    let mut directory = crate::overlays::Directory::new(all, &current);
    let states = [
        None,
        Some(ProbeState::Free),
        Some(ProbeState::Busy(Some("minicom (1234)".into()))),
        Some(ProbeState::NoAccess(Some("dialout".into()))),
        Some(ProbeState::Busy(None)),
        Some(ProbeState::NoAnswer),
    ];
    for (port, state) in ports.iter().zip(states) {
        directory.tests.insert(port.to_string(), state);
    }
    let open = Overlay::Directory(directory);
    h.app.update(Action::App(AppAction::OpenOverlay(open)));
    let rows = h.draw();
    let note = crate::ui::popups::directory::TEST_NOTE;
    assert!(has(&rows, note), "{rows:#?}");
    for (port, mark) in ports.iter().zip([
        "testing…",
        "free",
        "busy: minicom (1234)",
        "no access: join dialout",
        "busy",
        "no answer",
    ]) {
        assert!(has(&rows, &format!("{port}  {mark}")), "{port}: {rows:#?}");
    }
    assert!(has(&rows, "t Test"), "{rows:#?}");
    assert!(!rows.iter().any(|row| row.contains('▲')), "{rows:#?}");

    let many = (0..40).map(|i| format!("/dev/ttyS{i}")).collect();
    let directory = crate::overlays::Directory::new(many, &current);
    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::Directory(
            directory,
        ))));
    let rows = h.draw();
    assert!(rows.iter().any(|row| row.contains('▲')), "{rows:#?}");
    assert!(rows.iter().any(|row| row.contains('▼')), "{rows:#?}");
}

/// MAX-188: each tab is a table under its headers, the state in its own column lined up;
/// a column empty on every row isn't shown (no device or ID), the state's always is, as
/// wide as its longest fixed state, so a test ending doesn't narrow the popup. A device
/// longer than 30 columns is cut, `…` at its end, and in full under the table while its
/// row is selected; a narrow screen cuts it more.
/// How: two ports, one with its USB details, one tested; the second selected; then no
/// details nor test; then the first at 50 columns.
#[test]
fn directory_columns() {
    use crate::link::{probe::ProbeState, serial::PortDetails};
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let current = h.app.link_config().address.clone();
    let ports = vec!["/dev/ttyACM0".to_string(), "/dev/ttyUSB1".to_string()];
    let mut directory = crate::overlays::Directory::new(ports.clone(), &current);
    let details = PortDetails {
        device: Some("Silicon Labs CP2102 USB to UART Bridge Controller".into()),
        id: Some("10c4:ea60".into()),
    };
    directory.details.insert("/dev/ttyUSB1".into(), details);
    (directory.tests).insert("/dev/ttyACM0".into(), Some(ProbeState::Free));
    let open = |h: &mut Harness, directory: crate::overlays::Directory| {
        let overlay = Overlay::Directory(directory);
        h.app.update(Action::App(AppAction::OpenOverlay(overlay)));
        h.draw()
    };
    let rows = open(&mut h, directory.clone());
    for row in [
        "  Port          Device                          ID         State",
        "  /dev/ttyACM0                                             free",
        "  /dev/ttyUSB1  Silicon Labs CP2102 USB to UA…  10c4:ea60",
    ] {
        assert!(has(&rows, row), "{row}: {rows:#?}");
    }
    let full = "Device  Silicon Labs CP2102 USB to UART Bridge Controller";
    assert!(!has(&rows, full), "{rows:#?}");
    h.app.update(Action::Overlay(DialogAction::Down));
    let rows = h.draw();
    assert!(has(&rows, full), "{rows:#?}");

    let plain = crate::overlays::Directory::new(ports, &current);
    let rows = open(&mut h, plain);
    assert!(has(&rows, "  Port          State"), "{rows:#?}");
    for gone in ["Device", "ID"] {
        assert!(!has(&rows, gone), "{gone}: {rows:#?}");
    }

    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 50, 24);
    let rows = open(&mut h, directory);
    assert!(has(&rows, "/dev/ttyUSB1  Silic…  10c4:ea60"), "{rows:#?}");
}

/// MAX-163: in the trigger browser, j/k and Up/Down move; l/Right arm the selected one and
/// h/Left disarm it, a second press changing nothing and saying nothing; Tab, Space and
/// Shift-Tab toggle it.
/// How: the default triggers, each key typed through the keymap, the armed ones and the
/// toasts read after each.
#[test]
fn trigger_browser_keys() {
    use crossterm::event::KeyCode::{BackTab, Char, Down, Left, Right, Tab, Up};
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 120, 30);
    h.app
        .set_triggers(crate::configs::TriggersConfig::default().compile().unwrap());
    h.app.update(KeyBindingAction::Triggers.to_action(&h.app));
    let none = KeyModifiers::NONE;
    let toasts = |h: &Harness| h.app.notifications.active_toasts().len();
    let mut press = |code, mods| {
        key_do(&mut h, code, mods);
        assert!(matches!(h.app.overlay(), Some(Overlay::Triggers(_))));
        let armed: Vec<String> = h
            .app
            .armed_triggers()
            .iter()
            .map(|s| s.to_string())
            .collect();
        (armed, toasts(&h))
    };
    let (_, before) = press(Char('j'), none);
    assert_eq!(
        press(Char('l'), none),
        (vec!["u-boot".to_string()], before + 1)
    );
    assert_eq!(press(Right, none), (vec!["u-boot".to_string()], before + 1));
    assert_eq!(press(Char('h'), none).0, Vec::<String>::new());
    let (_, count) = press(Left, none);
    assert_eq!(count, before + 2);
    press(Char('k'), none);
    assert_eq!(press(Right, none).0, ["login"]);
    press(Down, none);
    assert_eq!(press(Tab, none).0, ["login", "u-boot"]);
    assert_eq!(press(Char(' '), none).0, ["login"]);
    assert_eq!(press(BackTab, KeyModifiers::SHIFT).0, ["login", "u-boot"]);
    press(Up, none);
    assert_eq!(press(Char('h'), none).0, ["u-boot"]);
}

/// MAX-167: the trigger browser is as big as its text and buttons, every trigger's config
/// in it: it doesn't change with the selection, and a longer rule widens it; the screen's
/// size at most. MAX-183: a name longer than 20 columns is cut in the list.
/// How: the default triggers, the popup's top border measured, a second row selected,
/// then a long `run` and a narrow screen.
#[test]
fn trigger_browser_fits_its_text() {
    let popup_width = |h: &mut Harness| {
        let rows = h.draw();
        let top = rows.iter().find(|row| row.contains("┌ Triggers")).unwrap();
        top.trim().chars().count()
    };
    let open = |h: &mut Harness, toml: &str| {
        let config: crate::configs::TriggersConfig = toml::from_str(toml).unwrap();
        h.app.set_triggers(config.compile().unwrap());
        h.app.update(KeyBindingAction::Triggers.to_action(&h.app));
    };
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 200, 40);
    h.app.settings_mut().triggers_path = Some("/t.toml".into());
    open(
        &mut h,
        &toml::to_string(&crate::configs::TriggersConfig::default()).unwrap(),
    );
    // the buttons are wider than the list (`○ u-boot`), the gap and `Match  "Hit any key
    // to stop autoboot"` (8 + 2 + 37): them, a space and a corner on each side
    let buttons = "Enter Toggle · a Toggle all · c Copy path · Esc Close";
    let width = popup_width(&mut h);
    assert_eq!(width, buttons.chars().count() + 4);
    let rows = h.draw();
    assert!(has(&rows, &format!("└ {buttons} ┘")), "{rows:#?}");
    let top = rows
        .iter()
        .position(|row| row.contains("┌ Triggers"))
        .unwrap();
    let bottom = top
        + rows[top..]
            .iter()
            .position(|row| row.contains('└'))
            .unwrap();
    let height = bottom - top + 1;
    // the config and its title, no padding on top (MAX-183)
    assert_eq!(height, 4 + 1 + 2 + 3);
    h.app.update(Action::Overlay(DialogAction::Down));
    assert_eq!(popup_width(&mut h), width);

    let long = "x".repeat(80);
    open(&mut h, &format!("[r]\nmatch = 'x'\nrun = 'send {long}'\n"));
    // the list as wide as its title (MAX-183)
    assert_eq!(popup_width(&mut h), 5 + 2 + 7 + 5 + 80 + 4);

    // a long name cut in the list at 20 columns, in full over its rule (MAX-183)
    let name = "a-very-long-trigger-name-for-the-board-x";
    open(&mut h, &format!("[{name}]\nmatch = 'x'\nrun = 'send y'\n"));
    let rows = h.draw();
    assert!(has(&rows, "○ a-very-long-trigger…  State"), "{rows:#?}");
    assert!(
        has(&rows, &format!("Rules{}{name}", " ".repeat(2 + 20 + 2 - 5))),
        "{rows:#?}"
    );
    assert_eq!(popup_width(&mut h), 2 + 20 + 2 + name.len() + 4);
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 60, 20);
    open(&mut h, &format!("[r]\nmatch = 'x'\nrun = 'send {long}'\n"));
    assert_eq!(popup_width(&mut h), 60);
}

/// The session browser opened, in its list view.
fn open_session_list(h: &mut Harness) {
    h.app.update(Action::App(AppAction::OpenSessions));
    h.app.update(Action::Overlay(DialogAction::Pick(
        PopupTarget::SessionView,
    )));
}

/// MAX-202: the browser opens on its tree, the newest device open: an arrow per group, its
/// count and size on the right, its details beside it, and the marks of what it holds. A click
/// on the selected group closes it. A list narrower than its full width counts with a bare
/// number.
/// How: two past sessions on another port and the app's own running session, without time
/// levels through `group_by`; at 120 then 80 columns.
#[test]
fn session_browser_tree_rows() {
    let dir =
        std::env::temp_dir().join(format!("maxicom_test_{}_browser_tree", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let text = crate::capture::CaptureFormat::default();
    crate::capture::tests::write_test_session(&dir, "/dev/other", &text, b"x\r\n", b"");
    crate::capture::tests::write_test_session(&dir, "/dev/other", &text, b"y\r\n", b"");
    std::thread::sleep(std::time::Duration::from_millis(2));
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 120, 30);
    h.app.settings_mut().sessions.group_by = Vec::new();
    h.app.start_session(Some(dir.clone()));
    h.app.update(Action::App(AppAction::OpenSessions));
    let rows = h.draw();
    let row = |rows: &[String], text: &str| {
        let found = rows.iter().find(|row| row.contains(text));
        found
            .cloned()
            .unwrap_or_else(|| panic!("{text}: {rows:#?}"))
    };
    let device = row(&rows, "▾ ttyUSB0 115200 8N1 UTF8");
    assert!(device.contains('●'), "{rows:#?}");
    assert!(device.contains("1 session"), "{rows:#?}");
    let other = row(&rows, "▸ other 115200 8N1 UTF8");
    assert!(other.contains("2 sessions"), "{rows:#?}");
    assert!(!other.contains(['●', '○']), "{rows:#?}");
    assert!(has(&rows, "Group   ttyUSB0 115200 8N1 UTF8"), "{rows:#?}");
    assert!(has(&rows, "Holds   1 session"), "{rows:#?}");
    assert!(has(&rows, "Span    "), "{rows:#?}");
    assert!(has(&rows, "Size    "), "{rows:#?}");

    click(&mut h, cell_of(&rows, "ttyUSB0 115200"));
    let rows = h.draw();
    assert!(has(&rows, "▸ ttyUSB0 115200 8N1 UTF8"), "{rows:#?}");

    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    h.app.settings_mut().sessions.group_by = Vec::new();
    h.app.start_session(Some(dir.clone()));
    h.app.update(Action::App(AppAction::OpenSessions));
    let rows = h.draw();
    let other = row(&rows, "▸ other");
    assert!(!other.contains("session"), "{rows:#?}");
    assert!(other.contains(" 2 "), "{rows:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-202: in the list view, a port too long for the row is cut with a `…`, its full name
/// in the details.
/// How: a session on a long port name, at 80 columns.
#[test]
fn session_list_cuts_a_long_port() {
    let dir = std::env::temp_dir().join(format!("maxicom_test_{}_browser_cut", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let text = crate::capture::CaptureFormat::default();
    let port = "/dev/serial-by-id-usb-a-very-long-adapter-name";
    crate::capture::tests::write_test_session(&dir, port, &text, b"x\r\n", b"");
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    h.app.start_session(Some(dir.clone()));
    open_session_list(&mut h);
    let rows = h.draw();
    let row = rows.iter().find(|row| row.contains("serial-"));
    assert!(row.is_some_and(|row| row.contains('…')), "{rows:#?}");
    assert!(!has(&rows, "adapter-name "), "{rows:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-156: the browser shows a TCP session's address, and its line as `raw TCP` and the
/// encoding, as the status bar does.
/// How: a TCP session written, then selected with a click.
#[test]
fn session_browser_shows_a_tcp_session_without_line_settings() {
    let dir = std::env::temp_dir().join(format!("maxicom_test_{}_browser_tcp", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let text = crate::capture::CaptureFormat::default();
    let port = "tcp://192.168.1.50:4000";
    crate::capture::tests::write_test_session(&dir, port, &text, b"rx\r\n", b"");
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 120, 30);
    h.app.start_session(Some(dir.clone()));
    open_session_list(&mut h);
    let rows = h.draw();
    click(&mut h, cell_of(&rows, "192.168.1.50_4000"));
    let rows = h.draw();
    assert!(has(&rows, port), "{rows:#?}");
    let line = rows.iter().find(|row| row.contains("Line ")).unwrap();
    assert!(line.contains("raw TCP UTF8"), "{rows:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-175: the session browser draws a scrollbar after its list when the sessions don't
/// all fit, its thumb following the selection; none when they fit.
/// How: 15 sessions in a short screen, then the selection moved to the last one; then 1
/// session (and the app's own).
#[test]
fn session_browser_scrollbar() {
    let dir = std::env::temp_dir().join(format!("maxicom_test_{}_browser_bar", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let text = crate::capture::CaptureFormat::default();
    for i in 0..15 {
        let port = format!("/dev/p{i:02}");
        crate::capture::tests::write_test_session(&dir, &port, &text, b"x\r\n", b"");
    }
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 120, 20);
    h.app.start_session(Some(dir.clone()));
    open_session_list(&mut h);
    let rows = h.draw();
    let (bar_x, top) = cell_of(&rows, "▲");
    let thumb = |rows: &[String]| {
        (rows.iter().enumerate())
            .find(|(_, row)| row.chars().nth(bar_x as usize) == Some('█'))
            .map(|(y, _)| y)
            .unwrap()
    };
    let first = thumb(&rows);
    assert_eq!(first, top as usize + 1, "{rows:#?}");
    for _ in 0..15 {
        h.app.update(Action::Overlay(DialogAction::Down));
    }
    let rows = h.draw();
    assert!(thumb(&rows) > first, "{rows:#?}");
    let _ = std::fs::remove_dir_all(&dir);

    crate::capture::tests::write_test_session(&dir, "/dev/one", &text, b"x\r\n", b"");
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 120, 20);
    h.app.start_session(Some(dir.clone()));
    open_session_list(&mut h);
    let rows = h.draw();
    assert!(
        has(&rows, "Session backups") && has(&rows, "one"),
        "{rows:#?}"
    );
    assert!(!has(&rows, "▲"), "{rows:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The session browser's preview shows the last lines as Clean does, without their escape
/// sequences; a port is one column after the size, its row a column short of the
/// scrollbar's.
/// How: a session whose text has a colored line, the browser at 120x30.
#[test]
fn session_preview_is_clean() {
    let dir =
        std::env::temp_dir().join(format!("maxicom_test_{}_clean_preview", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let text = crate::capture::CaptureFormat::default();
    let rx = b"\x1b[32mgreen\x1b[0m line\r\n";
    crate::capture::tests::write_test_session(&dir, "/dev/colored", &text, rx, b"");
    std::thread::sleep(std::time::Duration::from_millis(2));
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 120, 30);
    h.app.start_session(Some(dir.clone()));
    open_session_list(&mut h);
    let mut rows = h.draw();
    if !has(&rows, "green line") {
        key_do(&mut h, crossterm::event::KeyCode::Down, KeyModifiers::NONE);
        rows = h.draw();
    }
    assert!(has(&rows, "green line"), "{rows:#?}");
    assert!(!has(&rows, "[32m"), "{rows:#?}");
    assert!(has(&rows, "B colored"), "{rows:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-202: only a session really restored is marked: a past session on this device that
/// wasn't restored has no mark, nor does its group, and the details say `past`.
/// How: a past session on this device, the app's session started without a restore.
#[test]
fn session_browser_marks_only_a_restored_session() {
    let dir =
        std::env::temp_dir().join(format!("maxicom_test_{}_browser_mark", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let text = crate::capture::CaptureFormat::default();
    let port = LinkConfig::default().address.to_string();
    crate::capture::tests::write_test_session(&dir, &port, &text, b"mine\r\n", b"");
    std::thread::sleep(std::time::Duration::from_millis(2));
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 120, 30);
    h.app.start_session(Some(dir.clone()));
    open_session_list(&mut h);
    let rows = h.draw();
    assert!(!has(&rows, "○"), "{rows:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-148: the session browser lists the sessions with their marks (`●` running, `○`
/// the session restored into the history) and previews the selected one; a click selects a row,
/// and the buttons switch the view, delete one, and delete all once the question is answered.
/// How: a session on this device, one on another port, and the app's own running
/// session backup, in a temp folder.
#[test]
fn session_browser_lists_previews_and_deletes() {
    let dir = std::env::temp_dir().join(format!("maxicom_test_{}_browser_ui", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let text = crate::capture::CaptureFormat::default();
    let port = LinkConfig::default().address.to_string();
    crate::capture::tests::write_test_session(&dir, &port, &text, b"mine\r\n", b"");
    crate::capture::tests::write_test_session(&dir, "/dev/other", &text, b"theirs\r\n", b"");
    // the names are to the millisecond: the app's session backup must not get the same one
    std::thread::sleep(std::time::Duration::from_millis(2));
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 120, 30);
    h.app.restore_last_session(Some(&dir), Some(true));
    h.app.start_session(Some(dir.clone()));
    open_session_list(&mut h);
    let rows = h.draw();
    assert!(has(&rows, "Session backups"), "{rows:#?}");
    // the app's own session backup runs, the past one on this port was restored
    let row_with = |rows: &[String], text: &str| -> String {
        rows.iter().find(|row| row.contains(text)).cloned().unwrap()
    };
    let row_has = |rows: &[String], parts: &[&str]| {
        rows.iter()
            .any(|row| parts.iter().all(|part| row.contains(part)))
    };
    assert!(row_has(&rows, &["●", "ttyUSB0"]), "{rows:#?}");
    assert!(row_has(&rows, &["○", "ttyUSB0"]), "{rows:#?}");
    let other = row_with(&rows, " other ");
    assert!(!other.contains(['●', '○', '?']), "{rows:#?}");

    click(&mut h, cell_of(&rows, " other "));
    let rows = h.draw();
    assert!(has(&rows, "/dev/other"), "{rows:#?}");
    assert!(has(&rows, "theirs"), "{rows:#?}");

    click(&mut h, cell_of(&rows, "Tree view"));
    let rows = h.draw();
    assert!(has(&rows, "List view"), "{rows:#?}");
    click(&mut h, cell_of(&rows, "List view"));
    let rows = h.draw();
    assert!(has(&rows, "Tree view"), "{rows:#?}");

    click(&mut h, cell_on_row(&rows, "Delete all", "Delete "));
    let rows = h.draw();
    assert!(!has(&rows, " other "), "{rows:#?}");
    assert!(has(&rows, "Deleted the session of"), "{rows:#?}");

    click(&mut h, cell_of(&rows, "Delete all"));
    let rows = h.draw();
    assert!(has(&rows, "Delete 1 session ("), "{rows:#?}");
    click(&mut h, cell_on_row(&rows, "Cancel", "Delete"));
    let rows = h.draw();
    assert!(!row_has(&rows, &["○", "ttyUSB0"]), "{rows:#?}");
    assert!(row_has(&rows, &["●", "ttyUSB0"]), "{rows:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-166: the browser's Replay button replays the selected session, with what its meta
/// says, and closes the browser. While the replay runs, the browser's Back to live button
/// and `L` through the keymap stop it, as `replay-stop` does; they aren't there before.
/// How: a past session on another port, selected, then a click on the button; the replay
/// started as the loop does, the browser opened again, a click on Back to live, then `L`.
#[test]
fn session_browser_replays_the_selected_session() {
    let dir = std::env::temp_dir().join(format!(
        "maxicom_test_{}_browser_replay_ui",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let text = crate::capture::CaptureFormat::default();
    crate::capture::tests::write_test_session(&dir, "/dev/other", &text, b"theirs\r\n", b"");
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 120, 30);
    h.app.start_session(Some(dir.clone()));
    open_session_list(&mut h);
    let rows = h.draw();
    assert!(!has(&rows, "Back to live"), "{rows:#?}");
    click(&mut h, cell_of(&rows, " other "));
    let rows = h.draw();
    let at = cell_of(&rows, "Replay");
    let effects = [
        mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), at),
        mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), at),
    ];
    let replay = effects.into_iter().find_map(|effect| match effect {
        UpdateEffect::Replay { path, session, .. } => Some((path, session)),
        _ => None,
    });
    let (path, session) = replay.expect("no replay");
    assert!(path.extension().is_some_and(|ext| ext == "raw"));
    assert_eq!(
        session.as_ref().unwrap().address,
        "/dev/other".parse::<Address>().unwrap()
    );
    assert!(h.app.overlay().is_none());

    h.app.start_replay(path, session.as_ref());
    open_session_list(&mut h);
    let rows = h.draw();
    let at = cell_of(&rows, "Back to live");
    let effects = [
        mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), at),
        mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), at),
    ];
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, UpdateEffect::StopReplay)),
        "no stop"
    );
    assert!(h.app.overlay().is_none());

    open_session_list(&mut h);
    let shift = KeyModifiers::SHIFT;
    let effect = key_do(&mut h, crossterm::event::KeyCode::Char('L'), shift);
    assert!(matches!(effect, UpdateEffect::StopReplay));
    assert!(h.app.overlay().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-153: a session row is colored as `lsd -l`: the date by the age of the last write,
/// the size by its magnitude, the port by its name, and the mark by its kind; the gaps
/// between them keep the text color. The preview's port, times and size take the same
/// colors.
/// How: one past session written now on this device, restored at the start, next to the app's
/// own running session backup.
#[test]
fn session_browser_row_colors() {
    let dir = std::env::temp_dir().join(format!(
        "maxicom_test_{}_browser_colors",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let port = LinkConfig::default().address.to_string();
    let text = crate::capture::CaptureFormat::default();
    crate::capture::tests::write_test_session(&dir, &port, &text, b"mine\r\n", b"");
    // the names are to the millisecond: the app's session backup must not get the same one
    std::thread::sleep(std::time::Duration::from_millis(2));
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 120, 30);
    h.app.restore_last_session(Some(&dir), Some(true));
    h.app.start_session(Some(dir.clone()));
    open_session_list(&mut h);
    let rows = h.draw();
    let colors = h.theme.session_browser;
    let fg_at = |(x, y): (u16, u16)| h.terminal.backend().buffer()[(x, y)].fg;
    let (x, y) = cell_of(&rows, "○");
    assert_eq!(fg_at((x, y)), colors.mark_restored, "{rows:#?}");
    assert_eq!(fg_at((x + 2, y)), colors.date_hour, "{rows:#?}");
    // the size is all its files', the meta and times too (MAX-175)
    let sessions = crate::capture::list_sessions(&dir).unwrap();
    let past = sessions.iter().find(|session| !session.running).unwrap();
    let size_text = crate::overlays::sessions::size(past.size);
    let (size_x, _) = cell_on_row(&rows, "○", &size_text);
    assert_eq!(fg_at((size_x, y)), colors.size_small, "{rows:#?}");
    // the gaps and the size's padding aren't colored, so a reversed column ends with its
    // text
    let text = h.theme.palette.text;
    assert_eq!(fg_at((x + 1, y)), text, "{rows:#?}");
    assert_eq!(fg_at((x + 2 + 16, y)), text, "{rows:#?}");
    assert_eq!(fg_at((size_x - 1, y)), text, "{rows:#?}");
    let tag = cell_on_row(&rows, "○", "ttyUSB0");
    assert_eq!(
        fg_at(tag),
        h.theme.palette.link_color("ttyUSB0"),
        "{rows:#?}"
    );

    // the preview's port, times and size take the same colors as the row
    click(&mut h, (x, y));
    let rows = h.draw();
    let fg_at = |(x, y): (u16, u16)| h.terminal.backend().buffer()[(x, y)].fg;
    let after = |rows: &[String], name: &str| {
        let (x, y) = cell_of(rows, name);
        (x + 8, y)
    };
    let port = after(&rows, "Port    ");
    assert_eq!(
        fg_at(port),
        h.theme.palette.link_color("ttyUSB0"),
        "{rows:#?}"
    );
    let (times_x, times_y) = cell_on_row(&rows, "Session 20", "Session 20");
    let times = (times_x + 8, times_y);
    assert_eq!(fg_at(times), colors.date_hour, "{rows:#?}");
    let size = after(&rows, "Size    ");
    assert_eq!(fg_at(size), colors.size_small, "{rows:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-154: the session browser shows its folder, and its Copy buttons copy the folder's
/// path and the selected session's log path, to the clipboard and the paste buffer, the
/// browser staying open.
#[test]
fn session_browser_copies_paths() {
    let dir =
        std::env::temp_dir().join(format!("maxicom_test_{}_browser_copy", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let text = crate::capture::CaptureFormat::default();
    let session =
        crate::capture::tests::write_test_session(&dir, "/dev/past", &text, b"x\r\n", b"");
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 120, 30);
    h.app.start_session(Some(dir.clone()));
    open_session_list(&mut h);
    let rows = h.draw();
    let folder = dir.display().to_string();
    assert!(has(&rows, &format!("Folder  {folder}")), "{rows:#?}");

    // a button answers on the press
    let press = |h: &mut Harness, at| match mouse_do(h, MouseEventKind::Down(MouseButton::Left), at)
    {
        UpdateEffect::CopyToClipboardAs { text, what } => (text, what),
        _ => panic!("no copy"),
    };
    let (text, what) = press(&mut h, cell_of(&rows, "Copy folder"));
    assert_eq!(
        (text.as_str(), what.as_str()),
        (folder.as_str(), "folder path")
    );
    assert!(matches!(h.app.overlay(), Some(Overlay::Sessions(_))));
    assert!(h.app.has_paste_buffer());

    click(&mut h, cell_of(&rows, " past "));
    let rows = h.draw();
    let (text, what) = press(&mut h, cell_of(&rows, "Copy path"));
    let log = session.text_path().unwrap().display().to_string();
    assert_eq!(
        (text.as_str(), what.as_str()),
        (log.as_str(), "session log path")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-154: on an 80-column terminal every button of the session browser shows, the
/// folder's under the folder line, and Reload reads the folder again with a toast.
#[test]
fn session_browser_buttons_fit_80_columns() {
    let dir = std::env::temp_dir().join(format!("maxicom_test_{}_browser_80", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    h.app.start_session(Some(dir.clone()));
    h.app.update(Action::App(AppAction::OpenSessions));
    let rows = h.draw();
    for label in [
        "Delete all",
        "List view",
        "Close",
        "Copy path",
        "Copy folder",
    ] {
        assert!(has(&rows, label), "{label}: {rows:#?}");
    }
    let folder_row = cell_of(&rows, "Folder  ").1;
    assert_eq!(cell_of(&rows, "Copy folder").1, folder_row + 1, "{rows:#?}");

    click(&mut h, cell_of(&rows, "Reload"));
    let rows = h.draw();
    assert!(has(&rows, "Reloaded: 1 session"), "{rows:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-148: the restore dialog names the port, the session's times and size; a click on
/// Restore restores it, and a click outside doesn't answer it.
#[test]
fn restore_dialog_shows_the_session() {
    let dir = std::env::temp_dir().join(format!(
        "maxicom_test_{}_restore_dialog",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    crate::capture::tests::write_test_session(
        &dir,
        "/dev/ttyUSB0",
        &crate::capture::CaptureFormat::default(),
        b"old line\r\n",
        b"",
    );
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    h.app.settings_mut().sessions.restore = crate::configs::settings::RestoreMode::Ask;
    h.app.restore_last_session(Some(&dir), None);
    let rows = h.draw();
    assert!(
        has(&rows, "Restore the last session on ttyUSB0?"),
        "{rows:#?}"
    );
    assert!(has(&rows, " → "), "{rows:#?}");
    assert!(has(&rows, "10B received"), "{rows:#?}");

    click(&mut h, (0, 0));
    assert!(matches!(h.app.overlay(), Some(Overlay::RestoreSession(_))));
    // the Restore hint, on the row of Skip (the text above says Restore too)
    let hint_row = cell_of(&rows, "Skip").1;
    let hint_col = rows[hint_row as usize].find("Restore").unwrap();
    let hint_col = rows[hint_row as usize][..hint_col].chars().count();
    click(&mut h, (hint_col as u16, hint_row));
    assert!(h.app.overlay().is_none());
    assert_eq!(h.app.line_mode.history()[0].text, "old line\r\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-148: a restored line without a time keeps the time column blank, its text lined up
/// with a timed live line, and a click and a drag on it work as on any line.
/// How: a session without timestamps restored, then a live line; timestamps shown.
#[test]
fn restored_line_without_time_lines_up_and_copies() {
    let dir = std::env::temp_dir().join(format!(
        "maxicom_test_{}_restored_click",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let session = crate::capture::tests::write_test_session(
        &dir,
        "/dev/ttyUSB0",
        &crate::capture::CaptureFormat::default(),
        b"restored words\r\n",
        b"",
    );
    // without its times, as a session before MAX-175
    std::fs::remove_file(session.times_path().unwrap()).unwrap();
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 70, 12, |settings| {
        settings.show_timestamps = true;
    });
    h.app.restore_session(&session).unwrap();
    h.feed(b"live words\r\n");
    // the restore starts the view on its separator (MAX-176): the line is a row above
    h.scroll(HistoryAction::ScrollUp(1));
    let rows = h.draw();
    let (restored_x, restored_y) = cell_of(&rows, "restored words");
    assert_eq!(restored_x, cell_of(&rows, "live words").0, "{rows:#?}");
    let gutter: String = rows[restored_y as usize]
        .chars()
        .take(restored_x as usize)
        .collect();
    assert!(!gutter.chars().any(|c| c.is_ascii_digit()), "{gutter:?}");

    click(&mut h, (restored_x + 3, restored_y));
    h.draw();
    assert_eq!(cursor_cell(&mut h), (restored_x + 3, restored_y));
    h.app.update(Action::CopyMode(CopyModeAction::Exit));

    let (x, y) = cell_of(&h.draw(), "words");
    mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), (x, y));
    mouse_do(&mut h, MouseEventKind::Drag(MouseButton::Left), (x + 4, y));
    match mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), (x + 4, y)) {
        UpdateEffect::CopyToClipboard(text) => assert_eq!(text, "words"),
        _ => panic!("expected a yank"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A drag selects from the press, and the release yanks the selection and leaves copy
/// mode; a drag out of the history keeps to its edge.
#[test]
fn mouse_drag_selects_and_release_yanks() {
    let mut h = wrapped_lines();
    let rows = h.draw();
    let (x, y) = cell_of(&rows, "alpha");
    mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), (x, y));
    mouse_do(&mut h, MouseEventKind::Drag(MouseButton::Left), (x + 4, y));
    match mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), (x + 4, y)) {
        UpdateEffect::CopyToClipboard(text) => assert_eq!(text, "alpha"),
        _ => panic!("expected a yank"),
    }
    assert!(!h.app.line_mode.copy_state().active);

    // dragged to the right of the history: up to the row's last char shown
    let rows = h.draw();
    let (x, y) = cell_of(&rows, "zeta");
    mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), (x, y));
    mouse_do(&mut h, MouseEventKind::Drag(MouseButton::Left), (200, y));
    match mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), (200, y)) {
        UpdateEffect::CopyToClipboard(text) => assert_eq!(text, "zeta eta theta"),
        _ => panic!("expected a yank"),
    }

    // a click without a drag yanks nothing
    let rows = h.draw();
    let at = cell_of(&rows, "beta");
    mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), at);
    let release = mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), at);
    assert!(matches!(release, UpdateEffect::None));
    assert!(h.app.line_mode.copy_state().active);
}

/// MAX-138: in terminal mode the rows below the cursor and the last content never had any,
/// so a drag into them stops at the end of the last line, and the copy cursor doesn't go
/// down into them.
/// How: three lines on a 24-row grid; a drag from "one" to a blank row far below; copy
/// mode and Down 20 times.
#[test]
fn terminal_selection_stops_at_the_last_content() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 24, |s| {
        s.mode = Mode::Terminal;
    });
    h.draw();
    h.app
        .terminal_mode
        .process_rx(TextEncoding::Utf8, b"one\r\ntwo\r\n$ ");
    let rows = h.draw();
    let (x, y) = cell_of(&rows, "one");
    mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), (x, y));
    mouse_do(&mut h, MouseEventKind::Drag(MouseButton::Left), (5, 15));
    match mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), (5, 15)) {
        UpdateEffect::CopyToClipboard(text) => assert_eq!(text, "one\ntwo\n$"),
        _ => panic!("expected a yank"),
    }

    h.app.update(Action::CopyMode(CopyModeAction::Exit));
    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    for _ in 0..20 {
        h.app.update(Action::CopyMode(CopyModeAction::MoveDown));
    }
    let last = h.app.terminal_mode.get_max_scrollback() + 2;
    assert_eq!(h.app.terminal_mode.copy_state().cursor.line_idx, last);
}

/// MAX-139: the default status bar fits by steps as the screen narrows: the full text, then
/// the compact forms, then without `prio` 4 (framing, time), 3 (port, RX/TX), 2 (baud), and
/// at last the right part cut from its left; the mode and the buttons stay, and the clicks
/// land on what is drawn.
/// How: line mode without borders at widths where each step starts, the last row read;
/// clicks on the baud rate at 30 columns and on `≡` at 11.
#[test]
fn status_bar_fits_by_steps() {
    let bar_at = |width| {
        let mut h = Harness::line_mode(LineDisplayMode::Pretty, width, 10);
        let rows = h.draw();
        (h, rows[9].clone())
    };
    let (_, full) = bar_at(69);
    for text in ["/dev/ttyUSB0", "8N1", "RX:    0 | TX:    0 | "] {
        assert!(full.contains(text), "{full:?}");
    }
    let (_, compact) = bar_at(68);
    assert!(compact.contains(" ttyUSB0 |") && !compact.contains("/dev/"));
    assert!(compact.contains("8N1"), "{compact:?}");
    let (_, without_4) = bar_at(50);
    assert!(!without_4.contains("8N1") && !without_4.contains("TX:    0 | "));
    assert!(without_4.contains("ttyUSB0") && without_4.contains("TX:"));
    let (mut h, without_3) = bar_at(30);
    assert!(!without_3.contains("ttyUSB0") && !without_3.contains("RX:"));
    assert!(without_3.contains(" LINE  115200"), "{without_3:?}");
    let baud = h.app.link_config().line.baud.to_string();
    let rows = h.draw();
    click(&mut h, status_cell(&rows, &baud));
    assert!(matches!(h.app.overlay(), Some(Overlay::LinkSettings(_))));

    let (_, without_2) = bar_at(15);
    assert!(without_2.starts_with(" LINE ") && !without_2.contains("115200"));
    assert!(without_2.ends_with("? ≡ "), "{without_2:?}");
    let (mut h, cut) = bar_at(11);
    assert_eq!(cut, " LINE  ? ≡ ");
    let rows = h.draw();
    click(&mut h, status_cell(&rows, "≡"));
    assert!(matches!(h.app.overlay(), Some(Overlay::Settings(_))));
}

/// MAX-166: the default bar's replay block is the first one left out when the bar doesn't
/// fit: wherever it shows, the file name, the replay's state and RX/TX show too, and it
/// shows on a wide screen.
/// How: a replay of a long session file name in terminal mode, every width from 60 to 200.
#[test]
fn status_bar_replay_block_goes_first() {
    let name = "20260101_000000000_ttyFAKE.raw";
    let mut shown = false;
    for width in 60..=200 {
        let mut h = Harness::line_mode(LineDisplayMode::Pretty, width, 10);
        h.app
            .start_replay(std::path::PathBuf::from(format!("/tmp/{name}")), None);
        let bar = h.draw()[9].clone();
        if bar.contains('▷') {
            shown = true;
            for text in [name, "replay RUN", "RX:"] {
                assert!(bar.contains(text), "{width}: {bar:?}");
            }
        }
    }
    assert!(shown);
}

/// MAX-163: the center part goes in the middle of the room the left and right parts
/// leave, not of the row, and its click lands where it is drawn; it stays while it fits
/// there, a column from each.
/// How: a 10-column left, a 2-column center, a 4-column right, no borders; at 40 columns
/// the room is 11..35, so the center starts at 22 (the row's middle would be 19); at 18 it
/// just fits at 11, at 17 it goes.
#[test]
fn status_bar_center_between_the_parts() {
    let bar_at = |width| {
        let mut h = Harness::line_mode(LineDisplayMode::Pretty, width, 10);
        let mut config = h.app.settings().ui.status_bar.clone();
        config.left = "LLLLLLLLLL".to_string();
        config.center = "[CC](click=settings)".to_string();
        config.right = "RRRR".to_string();
        h.status_bar = CompiledStatusBar::new(&config, &h.theme);
        let rows = h.draw();
        (h, rows)
    };
    let (mut h, rows) = bar_at(40);
    let middle = " ".repeat(12);
    let right = " ".repeat(12);
    assert_eq!(rows[9], format!("LLLLLLLLLL{middle}CC{right}RRRR"));
    click(&mut h, (21, 9));
    assert!(h.app.overlay().is_none());
    click(&mut h, (23, 9));
    assert!(matches!(h.app.overlay(), Some(Overlay::Settings(_))));

    let (_, rows) = bar_at(18);
    assert_eq!(rows[9], "LLLLLLLLLL CC RRRR");
    let (_, rows) = bar_at(17);
    assert_eq!(rows[9], "LLLLLLLLLL   RRRR");
}

/// MAX-139: whatever the width, the bar's parts never write over each other, in every
/// layout: the mode and the `?` and `≡` buttons, never dropped, are always whole.
/// How: line and terminal mode, each layout (borders on for `border` and `box`), every width
/// from 120 down to 16.
#[test]
fn status_bar_parts_never_overlap() {
    use crate::configs::StatusBarLayout;
    for (mode, label) in [(Mode::Line, " LINE "), (Mode::Terminal, " TERM ")] {
        for layout in [
            StatusBarLayout::Line,
            StatusBarLayout::Band,
            StatusBarLayout::Border,
            StatusBarLayout::Box,
        ] {
            for width in (16..=120).rev() {
                let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, width, 16, |s| {
                    s.mode = mode;
                    s.ui.show_borders = layout != StatusBarLayout::Line;
                    s.ui.status_bar.layout = layout;
                });
                let rows = h.draw();
                let bar = rows.iter().rev().find(|row| row.contains(label));
                let bar = bar.unwrap_or_else(|| panic!("{layout:?} {width}: {rows:#?}"));
                assert!(bar.contains("? ≡"), "{mode:?} {layout:?} {width}: {bar:?}");
            }
        }
    }
}

/// The smallest `<prefix>NN` number in the rows
fn first_number(rows: &[String], prefix: &str) -> usize {
    rows.iter()
        .filter_map(|row| {
            let at = row.find(prefix)? + prefix.len();
            row.get(at..at + 2)?.parse().ok()
        })
        .min()
        .unwrap()
}

/// Holds a drag on the history's top row for `ticks` drag scroll ticks, drawing between
/// them as the main loop does, then releases: the yanked text
fn drag_scroll_up(h: &mut Harness, from: (u16, u16), ticks: u64) -> String {
    let top = (from.0, h.app.frame_map().text_area.top());
    mouse_do(h, MouseEventKind::Down(MouseButton::Left), from);
    mouse_do(h, MouseEventKind::Drag(MouseButton::Left), top);
    let t0 = std::time::Instant::now();
    for tick in 0..ticks {
        let now = t0 + std::time::Duration::from_millis(60 * tick);
        assert!(h.app.drag_scroll(now), "tick {tick}");
        // too soon for the next one
        assert!(!h.app.drag_scroll(now));
        h.draw();
    }
    match mouse_do(h, MouseEventKind::Up(MouseButton::Left), top) {
        UpdateEffect::CopyToClipboard(text) => text,
        _ => panic!("expected a yank"),
    }
}

/// MAX-126: a drag held on the history's top row scrolls it up on a timer and the selection
/// follows onto the rows coming into view; inside the history, or once released, it stops.
/// In line and terminal mode.
/// How: drag scroll ticks at made-up times; the yank reaches lines above the first view.
#[test]
fn mouse_drag_at_the_edge_scrolls() {
    let mut h = wrapped_lines();
    let rows = h.draw();
    let first = first_number(&rows, "line ");
    let from = cell_of(&rows, "theta");
    let text = drag_scroll_up(&mut h, from, 6);
    assert!(text.contains(&format!("line {:02}", first - 1)), "{text}");
    assert!(!h.app.drag_scroll(std::time::Instant::now()));

    // held inside: no scroll
    h.draw();
    let area = h.app.frame_map().text_area;
    let (x, y) = (area.x + 1, area.y + area.height / 2);
    mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), (x, y));
    mouse_do(&mut h, MouseEventKind::Drag(MouseButton::Left), (x, y - 1));
    assert!(!h.app.drag_scroll(std::time::Instant::now()));
    mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), (x, y - 1));

    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 12, |s| {
        s.mode = Mode::Terminal;
    });
    let bytes: String = (0..40).map(|i| format!("\r\nrow {i:02}")).collect();
    h.app
        .terminal_mode
        .process_rx(TextEncoding::Utf8, bytes.as_bytes());
    let rows = h.draw();
    let first = first_number(&rows, "row ");
    let last = cell_of(&rows, "row 39");
    let text = drag_scroll_up(&mut h, last, 3);
    // the selection is a tick behind: on the row the last scroll brought in
    assert!(text.starts_with(&format!("row {:02}", first - 2)), "{text}");
    assert!(text.ends_with("row 38\nr"), "{text}");
}

/// MAX-191: a button released outside the window never comes as a release, the next
/// event is a move with no button. That move ends what the press started: the drag
/// selection yanks and stops scrolling, the scrollbar and a held click target let go.
/// In line and terminal mode.
/// How: a press and a drag to the top row (it scrolls), then a `Moved` and no `Up`.
#[test]
fn move_without_button_ends_a_lost_release() {
    let moved = |h: &mut Harness, at| mouse_do(h, MouseEventKind::Moved, at);
    let lines = wrapped_lines();
    let mut terminal = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 12, |s| {
        s.mode = Mode::Terminal;
    });
    let bytes: String = (0..40).map(|i| format!("\r\nrow {i:02}")).collect();
    terminal
        .app
        .terminal_mode
        .process_rx(TextEncoding::Utf8, bytes.as_bytes());

    for (mut h, mode) in [(lines, "line"), (terminal, "terminal")] {
        h.draw();
        let area = h.app.frame_map().text_area;
        let (from, top) = ((area.x + 2, area.y + area.height / 2), (area.x + 2, area.y));
        mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), from);
        mouse_do(&mut h, MouseEventKind::Drag(MouseButton::Left), top);
        assert!(h.app.drag_scroll(std::time::Instant::now()), "{mode}");
        h.draw();
        match moved(&mut h, (from.0, 0)) {
            UpdateEffect::CopyToClipboard(text) => {
                assert!(text.lines().count() > 2, "{mode}: {text:?}")
            }
            _ => panic!("{mode}: expected a yank"),
        }
        let later = std::time::Instant::now() + std::time::Duration::from_secs(1);
        assert!(!h.app.drag_scroll(later), "{mode}");
        // the next moves are hovers again
        assert!(matches!(moved(&mut h, from), UpdateEffect::None), "{mode}");
        assert!(matches!(moved(&mut h, top), UpdateEffect::None), "{mode}");
    }

    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 12, |_| {});
    for i in 0..60 {
        h.app.feed_rx(format!("line {i:02}\r\n").as_bytes());
    }
    h.draw();
    let bar = h.app.frame_map().scrollbar.expect("a scrollbar");
    mouse_do(
        &mut h,
        MouseEventKind::Down(MouseButton::Left),
        (bar.area.x, bar.area.y),
    );
    assert!(h.app.is_scrollbar_held());
    moved(&mut h, (5, 5));
    assert!(!h.app.is_scrollbar_held());

    let mut h = wide_terminal(None);
    let right = status_cell(&h.draw(), "▶");
    mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), right);
    assert!(h.app.is_click_held());
    moved(&mut h, right);
    assert!(!h.app.is_click_held());
}

/// MAX-189: `side-grow` / `side-shrink` change the raw panel's share by 1 %, the `-5`
/// ones by 5 %, kept in 10..=85; the drawn panel follows. With the panel off they still
/// change it, and warn.
/// How: a 100-column history with the Hex panel at 40 %, the actions as keys run them.
#[test]
fn side_actions_resize_the_raw_panel() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 100, 12, |s| {
        s.raw_side.view = RawSideView::Hex;
    });
    let run = |h: &mut Harness, action: KeyBindingAction| {
        let action = action.to_action(&h.app);
        h.app.update(action);
        h.draw();
        let width = h.app.frame_areas().raw_side.map(|area| area.width);
        (h.app.settings().raw_side.split_percent, width)
    };
    h.draw();
    let width = |h: &Harness| h.app.frame_areas().raw_side.unwrap().width;
    let start = width(&h);
    assert_eq!(
        run(&mut h, KeyBindingAction::SideGrow5),
        (45, Some(start + 5))
    );
    assert_eq!(
        run(&mut h, KeyBindingAction::SideShrink),
        (44, Some(start + 4))
    );
    assert_eq!(run(&mut h, KeyBindingAction::SideGrow).0, 45);
    assert_eq!(run(&mut h, KeyBindingAction::SideShrink5).0, 40);
    assert_eq!(width(&h), start);
    for _ in 0..20 {
        run(&mut h, KeyBindingAction::SideGrow5);
    }
    assert_eq!(h.app.settings().raw_side.split_percent, 85);
    for _ in 0..20 {
        run(&mut h, KeyBindingAction::SideShrink5);
    }
    assert_eq!(run(&mut h, KeyBindingAction::SideShrink).0, 10);
    assert!(crate::app::tests::toasts(&h.app).is_empty());

    h.app.settings_mut().raw_side.view = RawSideView::Off;
    assert_eq!(run(&mut h, KeyBindingAction::SideGrow), (11, None));
    assert_eq!(crate::app::tests::toasts(&h.app), ["Raw SideView is Off"]);
}

/// MAX-189: the raw panel's border is a handle: a drag from it puts it under the pointer,
/// on the right (a column) and at the bottom (a row), in 10..=85 % of the split; the
/// release, or a move with no button (MAX-191), lets go. The pointer over it shows it.
/// In line and terminal mode.
/// How: press on the border, drags, then the border drawn where the pointer is.
#[test]
fn side_handle_drag_resizes_the_raw_panel() {
    use crate::configs::settings::RawSideOrientation;
    use ratatui::style::Modifier;
    let press = |h: &mut Harness, at| mouse_do(h, MouseEventKind::Down(MouseButton::Left), at);
    let drag = |h: &mut Harness, at| {
        mouse_do(h, MouseEventKind::Drag(MouseButton::Left), at);
        h.draw();
        h.app.frame_map().side_handle.unwrap().area
    };

    for mode in [Mode::Line, Mode::Terminal] {
        let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 100, 30, |s| {
            s.mode = mode;
            s.raw_side.view = RawSideView::Hex;
        });
        h.draw();
        let handle = h.app.frame_map().side_handle.expect("a handle");
        let (x, y) = (handle.area.x, handle.area.y + 3);
        assert_eq!(handle.area.width, 1, "{mode:?}");
        press(&mut h, (x, y));
        assert!(h.app.is_side_handle_held(), "{mode:?}");
        for to in [x - 10, x + 7] {
            let area = drag(&mut h, (to, y));
            assert!(area.x.abs_diff(to) <= 1, "{mode:?} {to}: {area:?}");
        }
        // past the range's ends
        drag(&mut h, (0, y));
        assert_eq!(h.app.settings().raw_side.split_percent, 85, "{mode:?}");
        drag(&mut h, (99, y));
        assert_eq!(h.app.settings().raw_side.split_percent, 10, "{mode:?}");
        mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), (99, y));
        assert!(!h.app.is_side_handle_held(), "{mode:?}");
        // after the release a drag doesn't move it
        drag(&mut h, (50, y));
        assert_eq!(h.app.settings().raw_side.split_percent, 10, "{mode:?}");
    }

    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 100, 30, |s| {
        s.raw_side.view = RawSideView::Hex;
        s.raw_side.orientation = RawSideOrientation::Bottom;
    });
    h.draw();
    let handle = h.app.frame_map().side_handle.unwrap().area;
    assert_eq!(handle.height, 1);
    let (x, y) = (handle.x + 5, handle.y);
    press(&mut h, (x, y));
    let area = drag(&mut h, (x, y - 4));
    assert!(area.y.abs_diff(y - 4) <= 1, "{area:?}");
    mouse_do(&mut h, MouseEventKind::Moved, (x, 0));
    assert!(!h.app.is_side_handle_held());

    // the hover
    let area = h.app.frame_map().side_handle.unwrap().area;
    mouse_do(&mut h, MouseEventKind::Moved, (area.x + 2, area.y));
    h.draw();
    let buffer = h.terminal.backend().buffer();
    assert!(
        buffer[(area.x + 9, area.y)]
            .modifier
            .contains(Modifier::REVERSED)
    );
}

/// MAX-126: in terminal mode with a grid taller than the view (24 rows shown in 11), the
/// copy cursor reaching the view's top row keeps the view; one row further moves the view
/// one row (MAX-143). The live view is panned into the grid, which the in-view check left
/// out: the view jumped to center the cursor.
/// How: copy mode from the remote cursor on the last row, moved up with keys.
#[test]
fn terminal_copy_cursor_on_the_top_row_keeps_the_view() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 12, |s| {
        s.mode = Mode::Terminal;
    });
    let bytes: String = (0..40).map(|i| format!("\r\nrow {i:02}")).collect();
    h.app
        .terminal_mode
        .process_rx(TextEncoding::Utf8, bytes.as_bytes());
    let before = h.draw();
    let height = h.app.frame_areas().content.height as usize;
    assert!(before[0].starts_with("row 29"), "{before:#?}");

    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    for _ in 0..10 {
        h.app.update(Action::CopyMode(CopyModeAction::MoveUp));
    }
    let rows = h.draw();
    assert_eq!(before[..height], rows[..height]);
    assert_eq!(cursor_cell(&mut h).1, 0);

    h.app.update(Action::CopyMode(CopyModeAction::MoveUp));
    let rows = h.draw();
    assert!(rows[0].starts_with("row 28"), "{rows:#?}");
    assert_eq!(cursor_cell(&mut h).1, 0);
}

/// MAX-126: a double click yanks the word under it, a triple click the whole line, and a
/// drag after a double click extends by words; in terminal mode too.
/// How: quick clicks through the input path (well under the 400 ms limit).
#[test]
fn mouse_double_and_triple_click() {
    let mut h = wrapped_lines();
    let rows = h.draw();
    let (x, y) = cell_of(&rows, "alpha");
    click(&mut h, (x + 1, y));
    mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), (x + 1, y));
    match mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), (x + 1, y)) {
        UpdateEffect::CopyToClipboard(text) => assert_eq!(text, "alpha"),
        _ => panic!("expected a yank"),
    }

    let rows = h.draw();
    let (x, y) = cell_of(&rows, "alpha");
    click(&mut h, (x, y));
    click(&mut h, (x, y));
    mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), (x, y));
    match mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), (x, y)) {
        UpdateEffect::CopyToClipboard(text) => {
            assert!(text.starts_with("line "), "{text}");
            assert!(text.ends_with(" alpha beta gamma delta epsilon zeta eta theta"));
        }
        _ => panic!("expected a yank"),
    }

    let rows = h.draw();
    let (x, y) = cell_of(&rows, "alpha");
    click(&mut h, (x, y));
    mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), (x, y));
    mouse_do(&mut h, MouseEventKind::Drag(MouseButton::Left), (x + 7, y));
    match mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), (x + 7, y)) {
        UpdateEffect::CopyToClipboard(text) => assert_eq!(text, "alpha beta"),
        _ => panic!("expected a yank"),
    }

    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 12, |s| {
        s.mode = Mode::Terminal;
    });
    let mut bytes = b"\r\n".repeat(20);
    bytes.extend_from_slice(b"first row\r\nsecond row");
    h.app.terminal_mode.process_rx(TextEncoding::Utf8, &bytes);
    let rows = h.draw();
    let (x, y) = cell_of(&rows, "second");
    click(&mut h, (x + 2, y));
    mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), (x + 2, y));
    match mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), (x + 2, y)) {
        UpdateEffect::CopyToClipboard(text) => assert_eq!(text, "second"),
        _ => panic!("expected a yank"),
    }
}

/// A click on a byte of the raw panel puts the raw copy cursor on it, ESC bytes included,
/// and a drag there yanks bytes (MAX-113, raw side of MAX-71).
#[test]
fn mouse_click_on_a_raw_byte() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 12);
    h.app.settings_mut().raw_side.view = RawSideView::Hex;
    h.feed(b"\x1b[32mok\x1b[0m\r\nabc\r\n");
    let rows = h.draw();

    let (x, y) = cell_of(&rows, "6F 6B");
    click(&mut h, (x + 1, y));
    let rows = h.draw();
    assert_eq!(h.app.copy_side(), Some(CopySide::Raw));
    assert_eq!(under_cursor(&mut h, &rows), "6F");

    let (x, y) = cell_of(&rows, "5B 33");
    mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), (x, y));
    mouse_do(&mut h, MouseEventKind::Drag(MouseButton::Left), (x + 9, y));
    match mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), (x + 9, y)) {
        UpdateEffect::CopyToClipboard(text) => assert_eq!(text, "5B 33 32 6D"),
        _ => panic!("expected a yank"),
    }

    // the panel panned by 2 bytes: the click counts them
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 12);
    h.app.settings_mut().raw_side.view = RawSideView::Hex;
    h.feed(b"\x1b[32mok\x1b[0m and a line far wider than the raw panel\r\n");
    h.draw();
    h.app.update(Action::App(AppAction::SideView(
        crate::inputs::SideViewAction::PanRight(2),
    )));
    let rows = h.draw();
    assert_eq!(h.app.line_mode.raw_pan_x(), 6);
    let (x, y) = cell_of(&rows, "6F 6B");
    click(&mut h, (x, y));
    let rows = h.draw();
    assert_eq!(under_cursor(&mut h, &rows), "6F");
}

/// Terminal mode: in copy mode a click puts the copy cursor on the grid cell; a drag yanks
/// the text.
#[test]
fn mouse_in_terminal_mode() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 12, |s| {
        s.mode = Mode::Terminal;
    });
    let mut bytes = b"\r\n".repeat(20);
    bytes.extend_from_slice(b"first row\r\nsecond row");
    h.app.terminal_mode.process_rx(TextEncoding::Utf8, &bytes);
    let rows = h.draw();

    let (x, y) = cell_of(&rows, "second");
    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    click(&mut h, (x + 3, y));
    h.draw();
    assert!(h.app.terminal_mode.copy_state().active);
    assert_eq!(cursor_cell(&mut h), (x + 3, y));
    h.app.update(Action::CopyMode(CopyModeAction::Exit));

    let (x, y) = cell_of(&rows, "first");
    mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), (x, y));
    mouse_do(
        &mut h,
        MouseEventKind::Drag(MouseButton::Left),
        (x + 2, y + 1),
    );
    match mouse_do(
        &mut h,
        MouseEventKind::Up(MouseButton::Left),
        (x + 2, y + 1),
    ) {
        UpdateEffect::CopyToClipboard(text) => assert_eq!(text, "first row\nsec"),
        _ => panic!("expected a yank"),
    }
}

/// A click in the history while searching leaves the search for copy mode there; a click
/// below the history does nothing.
#[test]
fn mouse_click_while_searching_and_off_the_history() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 60, 12);
    h.feed(b"one\r\ntwo\r\n");
    search(&mut h, "two");
    let rows = h.draw();

    let (x, y) = cell_of(&rows, "one");
    let below = (x, y + 3);
    click(&mut h, below);
    assert!(!h.app.line_mode.copy_state().active);
    assert!(h.app.is_search_active());

    click(&mut h, (x + 1, y));
    assert!(!h.app.is_search_active());
    h.draw();
    assert_eq!(cursor_cell(&mut h), (x + 1, y));
}

/// A click in a copy view the keys moved keeps that view, and in truncate mode the pan
/// counts: the cursor lands on the char shown under the click.
#[test]
fn mouse_click_keeps_the_copy_view_and_counts_the_pan() {
    let mut h = wrapped_lines();
    h.draw();
    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    for _ in 0..8 {
        h.app.update(Action::CopyMode(CopyModeAction::MoveUp));
    }
    let centered = h.draw();
    let height = h.app.frame_areas().content.height as usize;
    let (x, y) = cell_of(&centered, "delta");
    click(&mut h, (x, y));
    let after = h.draw();
    assert_eq!(centered[..height], after[..height]);
    assert_eq!(cursor_cell(&mut h), (x, y));

    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 30, 8);
    h.app.settings_mut().line_mode.wrap_mode = crate::configs::WrapMode::Truncate;
    h.feed(b"0123456789abcdefghijklmnopqrstuvwxyz ABCDEFGHIJ\r\n");
    h.draw();
    h.scroll(HistoryAction::PanRight(10));
    let rows = h.draw();
    let (x, y) = cell_of(&rows, "uvw");
    click(&mut h, (x + 1, y));
    h.draw();
    assert_eq!(h.app.line_mode.copy_state().cursor.char_idx, 31);
    assert_eq!(cursor_cell(&mut h), (x + 1, y));
}

/// 100 lines `line 00` to `line 99` in `mode`, drawn on a 40x12 screen, and the history's
/// height
fn numbered_lines(mode: Mode) -> (Harness, usize) {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 40, 12, |s| s.mode = mode);
    let lines: Vec<String> = (0..100).map(|i| format!("line {i:02}")).collect();
    match mode {
        Mode::Line => lines
            .iter()
            .for_each(|line| h.feed(format!("{line}\r\n").as_bytes())),
        Mode::Terminal => h
            .app
            .terminal_mode
            .process_rx(TextEncoding::Utf8, lines.join("\r\n").as_bytes()),
    }
    h.draw();
    let height = h.app.frame_areas().content.height as usize;
    (h, height)
}

/// Applies a copy action and draws: the number of the history's top line
fn copy_top(h: &mut Harness, action: CopyModeAction, height: usize) -> usize {
    h.app.update(Action::CopyMode(action));
    let rows = h.draw();
    first_number(&rows[..height], "line ")
}

/// MAX-143: in copy mode the view follows a cursor step by the fewest rows, in both modes:
/// a step out of the view moves it one row, a jump to the top or the bottom shows that end,
/// and a click leaves it where it is.
/// How: 100 numbered lines; from the live view, Up to the top row then once more, `gg`,
/// Down to the bottom row then once more, `G`, then a click.
#[test]
fn copy_view_follows_a_step_by_one_row() {
    for mode in [Mode::Line, Mode::Terminal] {
        let (mut h, height) = numbered_lines(mode);
        let live = 100 - height;
        assert_eq!(copy_top(&mut h, CopyModeAction::Enter, height), live);
        for _ in 1..height {
            assert_eq!(
                copy_top(&mut h, CopyModeAction::MoveUp, height),
                live,
                "{mode:?}"
            );
        }
        assert_eq!(cursor_cell(&mut h).1, 0);
        assert_eq!(copy_top(&mut h, CopyModeAction::MoveUp, height), live - 1);
        assert_eq!(cursor_cell(&mut h).1, 0, "{mode:?}");

        assert_eq!(copy_top(&mut h, CopyModeAction::JumpToTop, height), 0);
        for _ in 1..height {
            assert_eq!(
                copy_top(&mut h, CopyModeAction::MoveDown, height),
                0,
                "{mode:?}"
            );
        }
        assert_eq!(
            copy_top(&mut h, CopyModeAction::MoveDown, height),
            1,
            "{mode:?}"
        );
        assert_eq!(cursor_cell(&mut h).1 as usize, height - 1);

        assert_eq!(copy_top(&mut h, CopyModeAction::JumpToBottom, height), live);
        for _ in 0..3 {
            copy_top(&mut h, CopyModeAction::MoveUp, height);
        }
        let before = h.draw();
        let (x, y) = cell_of(&before, "line 90");
        click(&mut h, (x + 5, y));
        let after = h.draw();
        assert_eq!(before[..height], after[..height], "{mode:?}");
        assert_eq!(cursor_cell(&mut h), (x + 5, y));
    }
}

/// MAX-143: a cursor out of the view (the view scrolled away from it) is brought back
/// centered by the next move, in both modes.
/// How: 100 numbered lines, copy mode, Up to line 69, the history scrolled to the top,
/// then Up: the cursor (line 68) is on the middle row.
#[test]
fn copy_move_out_of_view_centers() {
    for mode in [Mode::Line, Mode::Terminal] {
        let (mut h, height) = numbered_lines(mode);
        h.app.update(Action::CopyMode(CopyModeAction::Enter));
        for _ in 0..30 {
            copy_top(&mut h, CopyModeAction::MoveUp, height);
        }
        h.scroll(HistoryAction::ScrollToTop);
        h.draw();
        let top = copy_top(&mut h, CopyModeAction::MoveUp, height);
        assert_eq!(top, 68 - height / 2, "{mode:?}");
        assert_eq!(cursor_cell(&mut h).1 as usize, height / 2);
    }
}

/// MAX-143: copy mode entered from a scrolled view puts the cursor at the start of the
/// view's bottom row, and the view doesn't move, in both modes; a wrapped entry's
/// continuation row too. A step then follows from there.
/// How: 100 numbered lines scrolled to the top, then wrapped lines with timestamps
/// scrolled up one row; Enter, then Up.
#[test]
fn copy_mode_from_a_scrolled_view_starts_on_its_bottom_row() {
    for mode in [Mode::Line, Mode::Terminal] {
        let (mut h, height) = numbered_lines(mode);
        h.scroll(HistoryAction::ScrollToTop);
        let before = h.draw();
        h.app.update(Action::CopyMode(CopyModeAction::Enter));
        let rows = h.draw();
        assert_eq!(before[..height], rows[..height], "{mode:?}");
        let bottom = cell_of(&rows, &format!("line {:02}", height - 1));
        assert_eq!(cursor_cell(&mut h), bottom, "{mode:?}");
        assert_eq!(copy_top(&mut h, CopyModeAction::MoveUp, height), 0);
        assert_eq!(cursor_cell(&mut h).1 as usize, height - 2, "{mode:?}");
    }

    let mut h = wrapped_lines();
    h.draw();
    h.scroll(HistoryAction::ScrollUp(1));
    let before = h.draw();
    let height = h.app.frame_areas().content.height as usize;
    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    let rows = h.draw();
    assert_eq!(before[..height], rows[..height]);
    assert_eq!(cursor_cell(&mut h).1 as usize, height - 1, "{rows:#?}");
    assert!(h.app.line_mode.copy_state().cursor.char_idx > 0);
}

/// The copy cursor and the selection's anchor, in the current mode
fn copy_spot(h: &Harness) -> (LogicalPosition, Option<LogicalPosition>) {
    let state = match h.app.settings().mode {
        Mode::Line => h.app.line_mode.copy_state(),
        Mode::Terminal => h.app.terminal_mode.copy_state(),
    };
    (state.cursor, state.selection_anchor)
}

/// MAX-143: in copy mode the wheel, the scrollbar (arrow, track, thumb), `ctrl-y` /
/// `ctrl-e`, `shift-home` / `shift-end` and prefix + up move the view only: the cursor and
/// the selection stay. The next move brings the view back, centered on the cursor, and a
/// click still moves the cursor without moving the view; in both modes.
/// How: 100 numbered lines, a selection from line 94 to 92, each scroll through the input
/// path, the scroll offset and the copy cursor after each.
#[test]
fn copy_mode_scrolls_leave_the_cursor() {
    use crossterm::event::KeyCode::{Char, End, Home, Up};
    let (ctrl, shift) = (KeyModifiers::CONTROL, KeyModifiers::SHIFT);
    for mode in [Mode::Line, Mode::Terminal] {
        let (mut h, height) = numbered_lines(mode);
        let copy = |h: &mut Harness, action| h.app.update(Action::CopyMode(action));
        copy(&mut h, CopyModeAction::Enter);
        for _ in 0..5 {
            copy(&mut h, CopyModeAction::MoveUp);
        }
        copy(&mut h, CopyModeAction::Select);
        copy(&mut h, CopyModeAction::MoveUp);
        copy(&mut h, CopyModeAction::MoveUp);
        h.draw();
        let spot = copy_spot(&h);
        let offset = |h: &mut Harness| {
            h.draw();
            h.app.scroll_status().offset_from_bottom
        };
        let start = offset(&mut h);
        let max = h.app.scroll_status().max_scroll;
        let text = (5, 2);
        let bar = h.app.frame_map().scrollbar.expect("a scrollbar");
        let (x, top) = (bar.area.x, bar.area.y);

        mouse_do(&mut h, MouseEventKind::ScrollUp, text);
        assert_eq!(offset(&mut h), start + 3, "{mode:?} wheel");
        key_do(&mut h, Char('y'), ctrl);
        assert_eq!(offset(&mut h), start + 4, "{mode:?} ctrl-y");
        key_do(&mut h, Char('e'), ctrl);
        assert_eq!(offset(&mut h), start + 3, "{mode:?} ctrl-e");
        click(&mut h, (x, top));
        assert_eq!(offset(&mut h), start + 4, "{mode:?} ▲");
        key_do(&mut h, Home, shift);
        assert_eq!(offset(&mut h), max, "{mode:?} shift-home");
        key_do(&mut h, End, shift);
        assert_eq!(offset(&mut h), 0, "{mode:?} shift-end");
        key_do(&mut h, Char('a'), ctrl);
        key_do(&mut h, Up, KeyModifiers::NONE);
        assert_eq!(offset(&mut h), 1, "{mode:?} prefix up");
        // the prefix stays for more scroll keys, and takes no click
        h.app.update(Action::App(AppAction::SetPrefix(false)));
        click(&mut h, (x, top + 1));
        assert_eq!(offset(&mut h), max, "{mode:?} track top");
        let (first, _) = h.app.frame_map().scrollbar.unwrap().thumb();
        let thumb = (x, top + 1 + first as u16);
        mouse_do(&mut h, MouseEventKind::Down(MouseButton::Left), thumb);
        mouse_do(
            &mut h,
            MouseEventKind::Drag(MouseButton::Left),
            (x, top + 3),
        );
        mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), (x, top + 3));
        let dragged = offset(&mut h);
        assert!(
            dragged < max && dragged > start + 10,
            "{mode:?} thumb {dragged}"
        );
        assert_eq!(copy_spot(&h), spot, "{mode:?}");
        assert!(h.app.is_copy_mode_active());

        // the cursor (line 92) is out of view: `j` brings it back on the middle row
        key_do(&mut h, Char('j'), KeyModifiers::NONE);
        let rows = h.draw();
        assert_eq!(first_number(&rows[..height], "line "), 93 - height / 2);
        assert_eq!(cursor_cell(&mut h).1 as usize, height / 2, "{mode:?}");

        let (x, y) = cell_of(&rows, "line 90");
        click(&mut h, (x + 5, y));
        assert_eq!(h.draw()[..height], rows[..height], "{mode:?} click");
        assert_eq!(cursor_cell(&mut h), (x + 5, y));
    }
}

/// MAX-143: a search confirmed into copy mode brings the view back to the match, centered,
/// when it was scrolled away from it during the search; in both modes.
/// How: 100 numbered lines, search "line 50", scroll to the top, Enter.
#[test]
fn copy_mode_from_search_centers_a_scrolled_away_match() {
    for mode in [Mode::Line, Mode::Terminal] {
        let (mut h, height) = numbered_lines(mode);
        search(&mut h, "line 50");
        h.draw();
        h.scroll(HistoryAction::ScrollToTop);
        let rows = h.draw();
        assert_eq!(first_number(&rows[..height], "line "), 0, "{mode:?}");
        h.app.update(Action::Search(SearchModeAction::Confirm));
        let rows = h.draw();
        assert_eq!(first_number(&rows[..height], "line "), 50 - height / 2);
        assert_eq!(cursor_cell(&mut h).1 as usize, height / 2, "{mode:?}");
    }
}

/// MAX-143: with entries taller than the view, a step brings the cursor's row into view by
/// the fewest rows, not its whole entry.
/// How: tall Hex entries, copy mode on the last one (`F`), Up to `E`'s first row (above the
/// view): it is the top row; Down back to `F`'s first row (below it): the bottom row.
#[test]
fn copy_step_with_tall_entries_shows_the_cursor_row() {
    let mut h = tall_hex_lines();
    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    h.draw();
    h.app.update(Action::CopyMode(CopyModeAction::MoveUp));
    let rows = h.draw();
    assert_eq!(cursor_cell(&mut h).1, 0, "{rows:#?}");
    assert!(rows[0].contains("45 45"), "{rows:#?}");
    h.app.update(Action::CopyMode(CopyModeAction::MoveDown));
    let rows = h.draw();
    let height = h.app.frame_areas().content.height;
    assert_eq!(cursor_cell(&mut h).1, height - 1, "{rows:#?}");
    assert!(rows[height as usize - 1].contains("5A 5A"), "{rows:#?}");
}

/// A key through the input path, applied
fn key_do(h: &mut Harness, code: crossterm::event::KeyCode, mods: KeyModifiers) -> UpdateEffect {
    let event = Event::Key(crossterm::event::KeyEvent::new(code, mods));
    let action = crate::inputs::handle_event(event, &h.app);
    h.app.update(action)
}

/// MAX-129: Ctrl-C goes to the device. In terminal mode it is sent as ^C; in line mode it
/// clears what is typed first, then sends ^C with a toast. It no longer quits.
/// How: Ctrl-C through the default keymap in each mode, with and without typed text.
#[test]
fn ctrl_c_goes_to_the_device() {
    use crossterm::event::KeyCode::Char;
    let ctrl = KeyModifiers::CONTROL;
    let sent_etx =
        |effect: &UpdateEffect| matches!(effect, UpdateEffect::Transmit(b) if b == &[0x03]);

    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 24, |s| {
        s.mode = Mode::Terminal;
    });
    assert!(sent_etx(&key_do(&mut h, Char('c'), ctrl)));
    assert!(h.app.overlay().is_none());

    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    for ch in "ls".chars() {
        key_do(&mut h, Char(ch), KeyModifiers::NONE);
    }
    assert_eq!(h.app.line_mode.input().value(), "ls");
    assert!(matches!(
        key_do(&mut h, Char('c'), ctrl),
        UpdateEffect::None
    ));
    assert_eq!(h.app.line_mode.input().value(), "");
    assert!(h.app.notifications.active_toasts().is_empty());

    assert!(sent_etx(&key_do(&mut h, Char('c'), ctrl)));
    let toasts = h.app.notifications.active_toasts();
    assert!(toasts.iter().any(|t| t.message == "^C sent"));
    assert!(h.app.overlay().is_none());
}

/// MAX-125: quitting asks first, by key or by mouse: Esc or `n` cancels; Enter, `y`, the
/// quit key again or a click on Quit quits.
/// How: Ctrl-Q through the default keymap, answered each way.
#[test]
fn quit_asks_first() {
    use crossterm::event::KeyCode::{Char, Enter, Esc};
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let ctrl = KeyModifiers::CONTROL;
    let none = KeyModifiers::NONE;
    let is_quit = |effect| matches!(effect, UpdateEffect::Quit);

    assert!(!is_quit(key_do(&mut h, Char('q'), ctrl)));
    assert!(matches!(h.app.overlay(), Some(Overlay::ConfirmQuit)));
    assert!(!is_quit(key_do(&mut h, Esc, none)));
    assert!(h.app.overlay().is_none());

    key_do(&mut h, Char('q'), ctrl);
    assert!(!is_quit(key_do(&mut h, Char('n'), none)));
    assert!(h.app.overlay().is_none());

    for answer in [(Enter, none), (Char('y'), none), (Char('q'), ctrl)] {
        key_do(&mut h, Char('q'), ctrl);
        assert!(is_quit(key_do(&mut h, answer.0, answer.1)), "{answer:?}");
    }

    key_do(&mut h, Char('q'), ctrl);
    let rows = h.draw();
    assert!(rows.iter().any(|row| row.contains("Quit maxicom?")));
    let effect = mouse_do(
        &mut h,
        MouseEventKind::Down(MouseButton::Left),
        cell_of(&rows, "Quit ·"),
    );
    assert!(is_quit(effect));
}

/// MAX-130: a right-click opens the action menu at the pointer, moved back inside the
/// screen at its corner; a click or Enter runs a row as its key would ("Quit…" asks
/// first), and with another popup open a right-click does nothing. Its rows start right
/// after the left border, their keys short: `^A ?`, not `Ctrl-a ?`.
/// How: right-clicks in the history and at the bottom-right corner.
#[test]
fn right_click_menu() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let right_click = |h: &mut Harness, at| {
        mouse_do(h, MouseEventKind::Down(MouseButton::Right), at);
        h.draw()
    };

    let rows = right_click(&mut h, (10, 3));
    assert!(matches!(h.app.overlay(), Some(Overlay::ActionMenu(_))));
    assert_eq!(cell_of(&rows, "┌ Menu"), (10, 3));
    // the first row is selected, and shows its key
    assert!(rows[4].contains("│▶ Help"), "{:?}", rows[4]);
    assert!(rows[4].contains("^A ?"), "{:?}", rows[4]);
    assert!(!has(&rows, "Ctrl-a"), "{rows:#?}");
    click(&mut h, cell_of(&rows, "Settings"));
    assert!(matches!(h.app.overlay(), Some(Overlay::Settings(_))));

    right_click(&mut h, (10, 3));
    assert!(matches!(h.app.overlay(), Some(Overlay::Settings(_))));
    h.app.close_overlay();

    right_click(&mut h, (10, 3));
    h.app.update(Action::Overlay(DialogAction::Confirm));
    assert!(matches!(h.app.overlay(), Some(Overlay::Help(_))));
    h.app.close_overlay();

    let rows = right_click(&mut h, (79, 23));
    let area = h.app.frame_map().popup.unwrap();
    assert_eq!((area.right(), area.bottom()), (80, 24));
    click(&mut h, cell_of(&rows, "Quit…"));
    assert!(matches!(h.app.overlay(), Some(Overlay::ConfirmQuit)));
}

/// MAX-132: in terminal mode the menu has "Terminal size", which opens the resize popup on
/// the grid's size, as its key does.
/// How: a right-click in terminal mode, a click on the row.
#[test]
fn right_click_menu_opens_the_resize_popup() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 30, |s| {
        s.mode = Mode::Terminal;
    });
    h.draw();
    mouse_do(&mut h, MouseEventKind::Down(MouseButton::Right), (10, 3));
    let rows = h.draw();
    click(&mut h, cell_of(&rows, "Terminal size"));
    let (grid_rows, grid_cols) = h.app.terminal_mode.get_vt100_size();
    match h.app.overlay() {
        Some(Overlay::TerminalSize(d)) => {
            assert_eq!(d.rows_input.value(), grid_rows.to_string());
            assert_eq!(d.cols_input.value(), grid_cols.to_string());
        }
        _ => panic!("no resize popup"),
    }
}

/// MAX-133: the settings popup has a "Terminal Mode" section with the size mode; its key
/// `S` and a click cycle it (grow only, match view, fixed cols, fixed) and the grid follows
/// at once.
/// How: terminal mode at 80x24, the grid grown on a 100-column screen first; the
/// popup, `S` (match view: the blank grid shrinks to the view), a click (fixed cols).
#[test]
fn settings_popup_size_mode() {
    use crate::configs::TerminalDimensionMode::{FixedCols, MatchView};
    use crossterm::event::KeyCode::Char;
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 100, 24, |s| {
        s.mode = Mode::Terminal;
    });
    h.draw();
    h.terminal.backend_mut().resize(80, 24);
    h.draw();
    // the view is a column narrower than the screen: the scrollbar's
    assert_eq!(h.app.terminal_mode.get_vt100_size().1, 99);

    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::Settings(
            SettingsDialog::new(),
        ))));
    let rows = h.draw();
    assert!(rows.iter().any(|row| row.contains("Terminal Mode")));
    let size_mode = cell_of(&rows, "Size Mode");
    assert!(rows[size_mode.1 as usize].contains("Grow Only"));

    key_do(&mut h, Char('s'), KeyModifiers::NONE);
    let rows = h.draw();
    assert_eq!(h.app.settings().terminal_mode.dimension_mode, MatchView);
    assert!(rows[size_mode.1 as usize].contains("Match View"));
    assert_eq!(h.app.terminal_mode.get_vt100_size().1, 79);

    click(&mut h, size_mode);
    assert_eq!(h.app.settings().terminal_mode.dimension_mode, FixedCols);
    assert!(matches!(h.app.overlay(), Some(Overlay::Settings(_))));
}

/// MAX-163: `P` in the settings popup cycles `prefix_place` (inset, border, none) and the
/// row shows it; a click on the row cycles it too.
#[test]
fn settings_popup_prefix_place() {
    use crate::configs::settings::PrefixPlace::{Border, Inset, None};
    use crossterm::event::KeyCode::Char;
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 30);
    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::Settings(
            SettingsDialog::new(),
        ))));
    let rows = h.draw();
    let place = cell_of(&rows, "Prefix Place");
    assert!(rows[place.1 as usize].contains("Inset"), "{rows:#?}");

    key_do(&mut h, Char('p'), KeyModifiers::NONE);
    let rows = h.draw();
    assert_eq!(h.app.settings().line_mode.prefix_place, Border);
    assert!(rows[place.1 as usize].contains("Border"), "{rows:#?}");

    click(&mut h, place);
    assert_eq!(h.app.settings().line_mode.prefix_place, None);
    key_do(&mut h, Char('P'), KeyModifiers::NONE);
    assert_eq!(h.app.settings().line_mode.prefix_place, Inset);
}

/// MAX-130: in terminal mode a right-click on the grid opens the menu only while the
/// remote hasn't asked for the mouse (a shell); once it has (DECSET 1000, as htop or vim
/// do), the right-click is reported to it and no menu opens, while one on the status bar
/// still opens it.
/// How: right-clicks before and after the remote turns mouse reporting on.
#[test]
fn right_click_goes_to_a_remote_using_the_mouse() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 30, |s| {
        s.mode = Mode::Terminal;
    });
    let feed = |h: &mut Harness, bytes: &[u8]| {
        h.app.terminal_mode.process_rx(TextEncoding::Utf8, bytes);
        h.draw();
    };
    let right = MouseEventKind::Down(MouseButton::Right);
    feed(&mut h, b"$ ");
    let area = h.app.frame_areas().content;
    let grid = (area.x + 4, area.y + 2);

    mouse_do(&mut h, right, grid);
    assert!(matches!(h.app.overlay(), Some(Overlay::ActionMenu(_))));
    h.app.close_overlay();

    feed(&mut h, b"\x1b[?1000h\x1b[?1006h");
    match mouse_do(&mut h, right, grid) {
        UpdateEffect::Transmit(bytes) => assert_eq!(bytes, b"\x1b[<2;5;3M"),
        _ => panic!("expected a report to the remote"),
    }
    assert!(h.app.overlay().is_none());

    // the status bar, the last row
    mouse_do(&mut h, right, (2, 29));
    assert!(matches!(h.app.overlay(), Some(Overlay::ActionMenu(_))));
}

/// Drags over `len` chars from the cell of `text`, the release yanks them
fn yank(h: &mut Harness, text: &str, len: u16) {
    let rows = h.draw();
    let (x, y) = cell_of(&rows, text);
    mouse_do(h, MouseEventKind::Down(MouseButton::Left), (x, y));
    mouse_do(h, MouseEventKind::Drag(MouseButton::Left), (x + len - 1, y));
    let release = mouse_do(h, MouseEventKind::Up(MouseButton::Left), (x + len - 1, y));
    assert!(matches!(release, UpdateEffect::CopyToClipboard(_)));
}

/// MAX-144: in line mode a right-click on the TX input pastes the last yank into it, from
/// copy mode too (which it leaves, like a click there); before any yank, and elsewhere,
/// it opens the menu.
/// How: a right-click on the input before a yank, after one, in copy mode; one in the
/// history.
#[test]
fn right_click_pastes_into_the_prompt() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    h.feed(b"alpha beta\r\n");
    h.draw();
    let right = MouseEventKind::Down(MouseButton::Right);
    let area = h.app.frame_map().prompt.unwrap().text_area;
    let prompt = (area.x + 2, area.y);

    mouse_do(&mut h, right, prompt);
    assert!(matches!(h.app.overlay(), Some(Overlay::ActionMenu(_))));
    h.app.close_overlay();

    yank(&mut h, "alpha", 5);
    h.draw();
    assert!(matches!(
        mouse_do(&mut h, right, prompt),
        UpdateEffect::None
    ));
    assert!(h.app.overlay().is_none());
    assert_eq!(h.app.line_mode.input().value(), "alpha");

    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    h.draw();
    mouse_do(&mut h, right, prompt);
    assert!(!h.app.line_mode.copy_state().active);
    assert_eq!(h.app.line_mode.input().value(), "alphaalpha");

    let rows = h.draw();
    mouse_do(&mut h, right, cell_of(&rows, "beta"));
    assert!(matches!(h.app.overlay(), Some(Overlay::ActionMenu(_))));
}

/// MAX-212: a middle click pastes the last yank anywhere outside a popup: into the input in
/// line mode (from the history too), to the remote in terminal mode. Before a yank, or over
/// a popup, or with `middle_click_paste` off, it does nothing; a remote using the mouse gets
/// the click instead.
/// How: line mode, a middle click before and after yanking "alpha", one over the
/// right-click menu, one with the setting off; terminal mode, one on a row that isn't the cursor's, then with mouse reports on.
#[test]
fn middle_click_pastes() {
    let middle = MouseEventKind::Down(MouseButton::Middle);
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    h.feed(b"alpha beta\r\n");
    let rows = h.draw();
    let beta = cell_of(&rows, "beta");
    mouse_do(&mut h, middle, beta);
    assert_eq!(h.app.line_mode.input().value(), "");

    yank(&mut h, "alpha", 5);
    h.draw();
    mouse_do(&mut h, middle, beta);
    assert_eq!(h.app.line_mode.input().value(), "alpha");
    assert!(h.app.overlay().is_none());

    mouse_do(&mut h, MouseEventKind::Down(MouseButton::Right), beta);
    assert!(matches!(h.app.overlay(), Some(Overlay::ActionMenu(_))));
    h.draw();
    mouse_do(&mut h, middle, beta);
    assert_eq!(h.app.line_mode.input().value(), "alpha");
    h.app.close_overlay();

    // the setting off: nothing
    h.app.settings_mut().ui.middle_click_paste = false;
    mouse_do(&mut h, middle, beta);
    assert_eq!(h.app.line_mode.input().value(), "alpha");

    let mut t = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 30, |s| {
        s.mode = Mode::Terminal;
    });
    t.draw();
    t.app
        .terminal_mode
        .process_rx(TextEncoding::Utf8, b"one\r\n$ ");
    yank(&mut t, "one", 3);
    let rows = t.draw();
    match mouse_do(&mut t, middle, cell_of(&rows, "one")) {
        UpdateEffect::Transmit(bytes) => assert_eq!(bytes, b"one"),
        _ => panic!("expected a paste"),
    }

    t.app
        .terminal_mode
        .process_rx(TextEncoding::Utf8, b"\x1b[?1000h\x1b[?1006h");
    let area = t.app.frame_areas().content;
    match mouse_do(&mut t, middle, (area.x + 4, area.y + 2)) {
        UpdateEffect::Transmit(bytes) => assert_eq!(bytes, b"\x1b[<1;5;3M"),
        _ => panic!("expected a report to the remote"),
    }
}

/// MAX-144: in terminal mode a right-click on the cursor's row sends the last yank to the
/// remote, as a paste; on another row it opens the menu.
/// How: a shell prompt under a line, "one" yanked; right-clicks on both rows.
#[test]
fn right_click_pastes_on_the_cursor_row() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 30, |s| {
        s.mode = Mode::Terminal;
    });
    h.draw();
    h.app
        .terminal_mode
        .process_rx(TextEncoding::Utf8, b"one\r\n$ ");
    let right = MouseEventKind::Down(MouseButton::Right);
    yank(&mut h, "one", 3);

    let rows = h.draw();
    let (_, prompt_row) = cell_of(&rows, "$ ");
    match mouse_do(&mut h, right, (20, prompt_row)) {
        UpdateEffect::Transmit(bytes) => assert_eq!(bytes, b"one"),
        _ => panic!("expected a paste"),
    }
    assert!(h.app.overlay().is_none());

    mouse_do(&mut h, right, cell_of(&rows, "one"));
    assert!(matches!(h.app.overlay(), Some(Overlay::ActionMenu(_))));
}

/// MAX-131: the search bar's hints run on a click like their keys, in the border or on
/// the last row; they and the border buttons are drawn in the hover style under the
/// pointer, each on its own cells.
/// How: a search opened with borders off then on; hover and clicks on its hints, then hover
/// on `[≡]`.
#[test]
fn search_hints_and_border_buttons_hover() {
    use crate::app::search::SearchModeKind;
    use ratatui::style::Modifier;
    let moved = |h: &mut Harness, at| {
        let event = mouse(MouseEventKind::Moved, at, KeyModifiers::NONE);
        let action = crate::inputs::handle_event(event, &h.app);
        h.app.update(action);
        h.draw();
    };
    let reversed = |h: &Harness, (x, y): (u16, u16)| {
        h.terminal.backend().buffer()[(x, y)]
            .modifier
            .contains(Modifier::REVERSED)
    };

    for borders in [false, true] {
        let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 100, 24, |s| {
            s.ui.show_borders = borders;
        });
        h.feed(b"hi there\r\n");
        h.app.update(Action::Search(SearchModeAction::Start));
        let rows = h.draw();
        let mode = cell_of(&rows, "Mode");
        moved(&mut h, mode);
        assert!(reversed(&h, mode), "borders {borders}");
        assert!(!reversed(&h, cell_of(&rows, "Exit")), "borders {borders}");
        click(&mut h, mode);
        assert_eq!(h.app.line_mode.search().mode_kind, SearchModeKind::RawBytes);
        let rows = h.draw();
        click(&mut h, cell_of(&rows, "Exit"));
        assert!(!h.app.line_mode.search().active, "borders {borders}");

        if borders {
            let rows = h.draw();
            let (x, y) = cell_of(&rows, "[≡]");
            moved(&mut h, (x + 1, y));
            assert!((x..x + 3).all(|x| reversed(&h, (x, y))));
            assert!(!reversed(&h, (x - 1, y)));
            assert!(!reversed(&h, (x + 3, y)));
        }
    }
}

/// MAX-131: a status bar block under the pointer takes the `hover` option, on its own
/// cells only, or its own `hover=` (the default `?` and `≡` have theirs); `hover = "none"`
/// turns it off, and a popup open stops it. The port is no button.
/// How: hover on `{mode}` with the default bar, a bar with `hover=(bg=red)` on a block and
/// `hover = "none"`, then a popup open.
#[test]
fn status_bar_hover() {
    use ratatui::style::{Color, Modifier};
    let moved = |h: &mut Harness, at| {
        let event = mouse(MouseEventKind::Moved, at, KeyModifiers::NONE);
        let action = crate::inputs::handle_event(event, &h.app);
        h.app.update(action);
        h.draw();
    };
    let cell = |h: &Harness, (x, y): (u16, u16)| h.terminal.backend().buffer()[(x, y)].clone();

    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let rows = h.draw();
    let (x, y) = status_cell(&rows, "LINE");
    moved(&mut h, (x + 1, y));
    // the block is " LINE ", one column each side of the text
    assert!((x - 1..x + 5).all(|x| cell(&h, (x, y)).modifier.contains(Modifier::REVERSED)));
    assert!(!cell(&h, (x + 5, y)).modifier.contains(Modifier::REVERSED));

    // the default buttons: help on the primary color, settings on the secondary; the port
    // isn't clickable
    let palette = h.theme.palette;
    for (text, bg) in [("?", palette.main), ("≡", palette.secondary)] {
        let at = status_cell(&rows, text);
        moved(&mut h, at);
        assert_eq!(cell(&h, at).bg, bg, "{text}");
    }
    click(&mut h, status_cell(&rows, "/dev/"));
    assert!(h.app.overlay().is_none());

    let mut config = h.app.settings().ui.status_bar.clone();
    config.right = "[ A ](click=help hover=(bg=red)) [ B ](click=settings)".to_string();
    config.hover = "none".to_string();
    h.status_bar = CompiledStatusBar::new(&config, &h.theme);
    let rows = h.draw();
    let a = status_cell(&rows, "A");
    moved(&mut h, a);
    assert_eq!(cell(&h, a).bg, Color::Red);
    let b = status_cell(&rows, "B");
    moved(&mut h, b);
    assert!(!cell(&h, b).modifier.contains(Modifier::REVERSED));
    assert_ne!(cell(&h, b).bg, Color::Red);

    moved(&mut h, a);
    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::Settings(
            SettingsDialog::new(),
        ))));
    h.draw();
    assert_ne!(cell(&h, a).bg, Color::Red);
}

/// MAX-135: with borders on, the status bar is a footer inset one column each side; its
/// plain text is on the UI background unless the layout is `band`, with or without borders.
/// The mode pill in copy mode and the hover keep their bg, and clicks and hovers land on
/// the moved blocks.
/// How: line mode with borders, the cells around the bar, copy mode, a hover on `?` and
/// clicks on `≡` and `LINE`; then borders off, and `band` with each.
#[test]
fn status_bar_inset_and_background() {
    use crate::configs::StatusBarLayout;
    let moved = |h: &mut Harness, at| {
        let event = mouse(MouseEventKind::Moved, at, KeyModifiers::NONE);
        let action = crate::inputs::handle_event(event, &h.app);
        h.app.update(action);
        h.draw();
    };
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 24, |s| {
        s.ui.show_borders = true;
        s.ui.status_bar.layout = StatusBarLayout::Line;
    });
    h.feed(b"one\r\n");
    let background = h.theme.ui.background.bg.unwrap();
    let band = h.theme.ui.status_bar.bg.unwrap();
    assert_ne!(background, band);

    let rows = h.draw();
    let y = 23;
    let last = rows[y].chars().count() as u16 - 1;
    for x in [0, last] {
        assert_eq!(rows[y].chars().nth(x as usize), Some(' '));
        assert_eq!(bg_at(&h, (x, y as u16)), Some(background));
    }
    // " LINE " starts in the second column
    assert_eq!(status_cell(&rows, "LINE"), (2, 23));
    assert_eq!(bg_at(&h, status_cell(&rows, "/dev/")), Some(background));

    // copy mode takes the badge, in its color (MAX-208)
    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    let rows = h.draw();
    assert_eq!(status_cell(&rows, "COPY"), (2, 23));
    assert!(!rows[23].contains("LINE"), "{:?}", rows[23]);
    assert_eq!(
        bg_at(&h, status_cell(&rows, "COPY")),
        Some(ratatui::style::Color::Magenta)
    );
    h.app.update(Action::CopyMode(CopyModeAction::Exit));

    let rows = h.draw();
    let help = status_cell(&rows, "?");
    moved(&mut h, help);
    assert_eq!(bg_at(&h, help), Some(h.theme.palette.main));
    click(&mut h, status_cell(&rows, "≡"));
    assert!(matches!(h.app.overlay(), Some(Overlay::Settings(_))));
    h.app.close_overlay();
    click(&mut h, status_cell(&rows, "LINE"));
    assert_eq!(h.app.settings().mode, Mode::Terminal);

    h.app.settings_mut().mode = Mode::Line;
    for (borders, layout, line_x, bg) in [
        (false, StatusBarLayout::Line, 1, background),
        (false, StatusBarLayout::Band, 1, band),
        (true, StatusBarLayout::Band, 2, band),
    ] {
        let ui = &mut h.app.settings_mut().ui;
        ui.show_borders = borders;
        ui.status_bar.layout = layout;
        let rows = h.draw();
        assert_eq!(
            status_cell(&rows, "LINE"),
            (line_x, 23),
            "{borders} {layout:?}"
        );
        for x in [0, last] {
            assert_eq!(bg_at(&h, (x, 23)), Some(bg), "{borders} {layout:?}");
        }
        assert_eq!(bg_at(&h, status_cell(&rows, "/dev/")), Some(bg));
    }
}

/// MAX-135: the `border` layout draws the status bar in the TX box's bottom border in line
/// mode, so the history gets its row; the border's `└─`, `─┘` and the dashes between the
/// parts stay, a space apart from the text, and the clicks work there. Borders off, it is
/// the `line` layout.
/// How: line mode at 80x24 with `line` then `border`: where the TX box starts, its bottom
/// row, clicks on `≡` and `LINE`; then borders off with each.
#[test]
fn status_bar_in_the_tx_border() {
    use crate::configs::StatusBarLayout;
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 24, |s| {
        s.ui.show_borders = true;
        s.ui.status_bar.layout = StatusBarLayout::Line;
    });
    let tx_top = |rows: &[String]| cell_of(rows, "Transmit").1;
    let line_tx = tx_top(&h.draw());

    h.app.settings_mut().ui.status_bar.layout = StatusBarLayout::Border;
    let rows = h.draw();
    assert_eq!(tx_top(&rows), line_tx + 1);
    let bottom = &rows[23];
    assert!(bottom.starts_with("└─ LINE "), "{bottom:?}");
    assert!(bottom.ends_with(" ─┘"), "{bottom:?}");
    assert!(bottom.contains("8N1 ──"), "{bottom:?}");
    assert!(bottom.contains("── RX: "), "{bottom:?}");
    let background = h.theme.ui.background.bg;
    assert_eq!(bg_at(&h, status_cell(&rows, "/dev/")), background);

    click(&mut h, status_cell(&rows, "≡"));
    assert!(matches!(h.app.overlay(), Some(Overlay::Settings(_))));
    h.app.close_overlay();
    click(&mut h, status_cell(&rows, "LINE"));
    assert_eq!(h.app.settings().mode, Mode::Terminal);

    h.app.settings_mut().mode = Mode::Line;
    h.app.settings_mut().ui.show_borders = false;
    let border_rows = h.draw();
    h.app.settings_mut().ui.status_bar.layout = StatusBarLayout::Line;
    assert_eq!(border_rows, h.draw());
}

/// MAX-135: in terminal mode, the `border` layout draws the status bar in the history's
/// bottom border, without the pan arrows; an open search takes the border, and the bar
/// and its clicks come back when it closes.
/// How: terminal mode with `border`, the last row and a click on `≡`, before, during and
/// after a search.
#[test]
fn status_bar_in_the_history_border() {
    use crate::configs::StatusBarLayout;
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 24, |s| {
        s.mode = Mode::Terminal;
        s.ui.show_borders = true;
        s.ui.status_bar.layout = StatusBarLayout::Border;
    });
    let rows = h.draw();
    let bottom = &rows[23];
    assert!(bottom.starts_with("└─ TERM "), "{bottom:?}");
    assert!(bottom.ends_with(" ─┘"), "{bottom:?}");
    let settings = status_cell(&rows, "≡");

    h.app.update(Action::Search(SearchModeAction::Start));
    let rows = h.draw();
    assert!(rows[23].contains("SEARCH"), "{:?}", rows[23]);
    assert!(!rows[23].contains("TERM "), "{:?}", rows[23]);
    click(&mut h, settings);
    assert!(!matches!(h.app.overlay(), Some(Overlay::Settings(_))));

    h.app.update(Action::Search(SearchModeAction::Cancel));
    let rows = h.draw();
    assert!(rows[23].starts_with("└─ TERM "), "{:?}", rows[23]);
    click(&mut h, settings);
    assert!(matches!(h.app.overlay(), Some(Overlay::Settings(_))));
}

/// MAX-139: the pan shows once, in the status bar's center as `◀ offset/max ▶`, the numbers
/// padded so the center doesn't move while panning, each arrow in the secondary color while
/// it can pan that way (muted else) and panning on a click; the history's border arrows are
/// only there when the bar has no pan variable. With the `border` layout too.
/// How: a 200-column terminal in a 120-column screen panned right by 3, `line` layout: the
/// history's bottom row and the bar; a click on `◀`; a bar without the pan; `border`; then
/// line mode with a 200-char line.
#[test]
fn the_pan_shows_once_in_the_status_bar() {
    use crate::configs::StatusBarLayout;
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 120, 24, |s| {
        s.mode = Mode::Terminal;
        s.terminal_mode.cols = 200;
        s.ui.show_borders = true;
        s.ui.status_bar.layout = StatusBarLayout::Line;
    });
    h.draw();
    h.scroll(HistoryAction::PanRight(3));
    let rows = h.draw();
    let max = h.app.pan_right_status().unwrap().0 + 3;
    assert!(
        !rows[22].contains('◀') && !rows[22].contains('▶'),
        "{:?}",
        rows[22]
    );
    let pan = format!("◀ {:>3}/{:<3} ▶", 3, max);
    assert!(rows[23].contains(&pan), "{:?}", rows[23]);
    let fg = |h: &Harness, (x, y): (u16, u16)| h.terminal.backend().buffer()[(x, y)].fg;
    let (left, right) = (status_cell(&rows, "◀"), status_cell(&rows, "▶"));
    assert_eq!(fg(&h, left), h.theme.palette.secondary);
    assert_eq!(fg(&h, right), h.theme.palette.secondary);

    // the same place once back at the left edge, the left arrow muted
    click(&mut h, left);
    assert!(h.app.pan_left_status().unwrap_or(0) < 3);
    h.scroll(HistoryAction::PanLeft(10));
    let rows = h.draw();
    assert_eq!(status_cell(&rows, "◀"), left);
    assert!(rows[23].contains(&format!("◀ {:>3}/{:<3} ▶", 0, max)));
    assert_eq!(fg(&h, left), h.theme.palette.text_muted);

    // a bar without a pan variable leaves it to the border
    let mut config = h.app.settings().ui.status_bar.clone();
    config.center = "[ mid ]()".to_string();
    h.status_bar = CompiledStatusBar::new(&config, &h.theme);
    h.scroll(HistoryAction::PanRight(3));
    let rows = h.draw();
    assert!(rows[22].starts_with("└ ◀ (3)"), "{:?}", rows[22]);

    h.status_bar = CompiledStatusBar::new(&h.app.settings().ui.status_bar.clone(), &h.theme);
    h.app.settings_mut().ui.status_bar.layout = StatusBarLayout::Border;
    let rows = h.draw();
    let bottom = &rows[23];
    assert_eq!(bottom.matches('◀').count(), 1, "{bottom:?}");
    assert!(bottom.contains(&pan), "{bottom:?}");

    // line mode, where the bar is in the TX box's border: the history's has no arrows
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 24, |s| {
        s.ui.show_borders = true;
        s.line_mode.wrap_mode = crate::configs::WrapMode::Truncate;
    });
    h.feed(format!("{}\r\n", "x".repeat(200)).as_bytes());
    h.draw();
    h.scroll(HistoryAction::PanRight(3));
    let rows = h.draw();
    let pans = rows.iter().filter(|row| row.contains('◀')).count();
    assert_eq!(pans, 1, "{rows:#?}");
    assert!(rows[23].contains("◀   3/"), "{:?}", rows[23]);
}

/// Terminal mode 200 columns wide on a 120x24 screen with borders, the status bar on a line
/// of its own with `center`, panned 10 columns
fn wide_terminal(center: Option<&str>) -> Harness {
    use crate::configs::StatusBarLayout;
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 120, 24, |s| {
        s.mode = Mode::Terminal;
        s.terminal_mode.cols = 200;
        s.ui.show_borders = true;
        s.ui.status_bar.layout = StatusBarLayout::Line;
        if let Some(center) = center {
            s.ui.status_bar.center = center.to_string();
        }
    });
    h.app
        .terminal_mode
        .process_rx(TextEncoding::Utf8, "x".repeat(190).as_bytes());
    h.draw();
    h.scroll(HistoryAction::PanRight(10));
    h
}

fn panned(h: &Harness) -> usize {
    h.app.pan_left_status().unwrap_or(0)
}

/// MAX-143: the history border's pan arrows are clickable: `◀ (n)` pans left, `(n) ▶`
/// right, a column each, with the hover on all of them; the wheel over them pans; in copy
/// mode the cursor stays.
/// How: a wide terminal with a status bar without the pan, clicks, hovers and wheel turns
/// on the arrows of the history's bottom border.
#[test]
fn border_pan_arrows_are_clickable() {
    let mut h = wide_terminal(Some("[ mid ]()"));
    let rows = h.draw();
    let left = cell_of(&rows, "◀ (10)");
    let hover = h.app.frame_map().hover_area(left.0, left.1);
    assert_eq!(hover.map(|area| (area.x, area.width)), Some((left.0, 6)));
    assert!(h.app.frame_map().hover_area(left.0 - 1, left.1).is_none());
    click(&mut h, left);
    assert_eq!(panned(&h), 9);

    let rows = h.draw();
    let right = cell_of(&rows, ") ▶");
    let (x, y) = (right.0 + 2, right.1);
    assert!(h.app.frame_map().hover_area(x, y).is_some());
    click(&mut h, (x, y));
    assert_eq!(panned(&h), 10);

    h.draw();
    mouse_do(&mut h, MouseEventKind::ScrollUp, left);
    assert_eq!(panned(&h), 6, "the wheel up pans left");
    h.draw();
    mouse_do(&mut h, MouseEventKind::ScrollDown, (x, y));
    assert_eq!(panned(&h), 10, "the wheel down pans right");

    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    let rows = h.draw();
    let spot = copy_spot(&h);
    click(&mut h, cell_of(&rows, "◀ (10)"));
    assert_eq!(panned(&h), 9);
    assert_eq!(copy_spot(&h), spot);
}

/// MAX-143: a pan arrow held keeps panning, after a while, until the release or until the
/// pointer leaves it: the border's and the status bar's; a jump (`{pan_offset}`) doesn't
/// repeat.
/// How: a press on each arrow, `click_repeat` before and after the delay, a drag on the
/// arrow then off it, the release.
#[test]
fn held_pan_arrows_repeat() {
    use std::time::{Duration, Instant};
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    let press = |h: &mut Harness, at| mouse_do(h, MouseEventKind::Down(MouseButton::Left), at);
    let drag = |h: &mut Harness, at| mouse_do(h, MouseEventKind::Drag(MouseButton::Left), at);

    let mut h = wide_terminal(Some("[ mid ]()"));
    let rows = h.draw();
    let left = cell_of(&rows, "◀ (10)");
    press(&mut h, left);
    assert_eq!(panned(&h), 9);
    assert!(!h.app.click_repeat(at(100)));
    assert!(h.app.click_repeat(at(400)));
    assert_eq!(panned(&h), 8);
    assert!(!h.app.click_repeat(at(420)), "every 50 ms");
    drag(&mut h, (left.0 + 3, left.1));
    assert!(h.app.click_repeat(at(450)));
    assert_eq!(panned(&h), 7, "still on the arrow");
    drag(&mut h, (left.0, left.1 - 1));
    assert!(!h.app.is_click_held());
    assert!(!h.app.click_repeat(at(1000)));
    mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), left);
    assert_eq!(panned(&h), 7);

    let mut h = wide_terminal(None);
    let rows = h.draw();
    let right = status_cell(&rows, "▶");
    press(&mut h, right);
    assert!(h.app.click_repeat(at(1000)));
    assert_eq!(panned(&h), 12);
    mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), right);
    assert!(!h.app.click_repeat(at(2000)));
    assert_eq!(panned(&h), 12);

    let rows = h.draw();
    press(&mut h, (status_cell(&rows, " 12/").0 + 2, 23));
    assert_eq!(panned(&h), 0);
    assert!(!h.app.is_click_held());
}

/// MAX-143: the border title's `SCROLL +n` goes back to the live view and `PAN +n` to the
/// left edge when clicked, with the hover; in copy mode the cursor stays.
/// How: line mode with borders and the title details, long numbered lines truncated,
/// scrolled and panned, in copy mode.
#[test]
fn title_scroll_and_pan_are_clickable() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 24, |s| {
        s.ui.show_borders = true;
        s.ui.title_details = true;
        s.line_mode.wrap_mode = crate::configs::WrapMode::Truncate;
    });
    for i in 0..60 {
        h.feed(format!("line {i:02} {}\r\n", "x".repeat(150)).as_bytes());
    }
    h.draw();
    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    h.draw();
    h.scroll(HistoryAction::ScrollUp(3));
    h.scroll(HistoryAction::PanRight(10));
    // the title reads the scroll the last frame resolved
    h.draw();
    let rows = h.draw();
    let spot = copy_spot(&h);

    let pan = cell_of(&rows, "PAN +10");
    let hover = h.app.frame_map().hover_area(pan.0 + 6, pan.1);
    assert_eq!(hover.map(|area| (area.x, area.width)), Some((pan.0, 7)));
    click(&mut h, pan);
    assert_eq!(h.app.line_mode.x_offset(), 0);

    let rows = h.draw();
    assert!(!shows(&rows, "PAN +"), "{:?}", rows[0]);
    click(&mut h, cell_of(&rows, "SCROLL +3"));
    h.draw();
    let rows = h.draw();
    assert_eq!(h.app.scroll_status().offset_from_bottom, 0);
    assert!(shows(&rows, "LIVE"), "{:?}", rows[0]);
    assert_eq!(copy_spot(&h), spot);
}

/// MAX-143: the status bar's `{pan_offset}` goes back to the left edge when clicked, and the
/// wheel over a pan block (its arrows, its numbers) pans; over the history it scrolls.
/// How: the default center part, `◀ offset/max ▶`, in a wide terminal.
#[test]
fn status_bar_pan_block_clicks_and_wheel() {
    let mut h = wide_terminal(None);
    let rows = h.draw();
    let numbers = status_cell(&rows, " 10/");
    mouse_do(&mut h, MouseEventKind::ScrollUp, (numbers.0 + 2, numbers.1));
    assert_eq!(panned(&h), 6);
    let rows = h.draw();
    mouse_do(&mut h, MouseEventKind::ScrollDown, status_cell(&rows, "▶"));
    assert_eq!(panned(&h), 10);
    let rows = h.draw();
    click(&mut h, (status_cell(&rows, " 10/").0 + 2, 23));
    assert_eq!(panned(&h), 0);

    // over the history the wheel scrolls, the pan stays
    h.scroll(HistoryAction::PanRight(5));
    h.draw();
    mouse_do(&mut h, MouseEventKind::ScrollUp, (10, 5));
    assert_eq!(panned(&h), 5);
}

/// MAX-135: the `box` layout draws the status bar in a box of its own under the TX box,
/// 2 rows taller than `line`, its text inside on the UI background; clicks and hovers land
/// on it. Borders off, it is the `line` layout.
/// How: line mode at 80x24 with `line` then `box`: where the TX box starts, the last three
/// rows, a hover on `?` and a click on `≡`; then borders off with each.
#[test]
fn status_bar_box_layout() {
    use crate::configs::StatusBarLayout;
    let moved = |h: &mut Harness, at| {
        let event = mouse(MouseEventKind::Moved, at, KeyModifiers::NONE);
        let action = crate::inputs::handle_event(event, &h.app);
        h.app.update(action);
        h.draw();
    };
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 24, |s| {
        s.ui.show_borders = true;
        s.ui.status_bar.layout = StatusBarLayout::Line;
    });
    let tx_top = |rows: &[String]| cell_of(rows, "Transmit").1;
    let line_tx = tx_top(&h.draw());

    h.app.settings_mut().ui.status_bar.layout = StatusBarLayout::Box;
    let rows = h.draw();
    assert_eq!(tx_top(&rows), line_tx - 2);
    assert!(
        rows[21].starts_with("┌──") && rows[21].ends_with("──┐"),
        "{:?}",
        rows[21]
    );
    assert!(rows[22].starts_with("│ LINE "), "{:?}", rows[22]);
    assert!(rows[22].ends_with(" │"), "{:?}", rows[22]);
    assert!(
        rows[23].starts_with("└──") && rows[23].ends_with("──┘"),
        "{:?}",
        rows[23]
    );
    let background = h.theme.ui.background.bg;
    assert_eq!(bg_at(&h, cell_of(&rows, "/dev/")), background);

    // in the box's middle row, not the border buttons of the top row
    let bar_cell = |text: &str| {
        let x = rows[22].rfind(text).unwrap();
        (rows[22][..x].chars().count() as u16, 22)
    };
    let help = bar_cell("?");
    moved(&mut h, help);
    assert_eq!(bg_at(&h, help), Some(h.theme.palette.main));
    click(&mut h, bar_cell("≡"));
    assert!(matches!(h.app.overlay(), Some(Overlay::Settings(_))));
    h.app.close_overlay();

    h.app.settings_mut().ui.show_borders = false;
    let box_rows = h.draw();
    h.app.settings_mut().ui.status_bar.layout = StatusBarLayout::Line;
    assert_eq!(box_rows, h.draw());
}

/// MAX-131: mouse moves don't cancel a pending prefix: a move over nothing (ignored) or a
/// hover between Ctrl-A and its key still lets the key run.
/// How: Ctrl-A, moves over the history and onto the status bar's `?`, then `o`.
#[test]
fn mouse_moves_keep_the_prefix() {
    use crossterm::event::KeyCode::Char;
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let rows = h.draw();
    let moved = |h: &mut Harness, at| {
        let event = mouse(MouseEventKind::Moved, at, KeyModifiers::NONE);
        let action = crate::inputs::handle_event(event, &h.app);
        h.app.update(action);
    };
    key_do(&mut h, Char('a'), KeyModifiers::CONTROL);
    moved(&mut h, (5, 5));
    moved(&mut h, (6, 5));
    moved(&mut h, status_cell(&rows, "?"));
    assert!(h.app.is_prefix_active());
    key_do(&mut h, Char('o'), KeyModifiers::NONE);
    assert!(matches!(h.app.overlay(), Some(Overlay::Settings(_))));
}

/// MAX-131: the menu's selection follows the pointer, so Enter runs the row under it; in
/// the other popups the part under it is drawn in the hover style and the keyboard
/// selection stays; a move within the same part is ignored, so it doesn't redraw.
/// How: moves over the menu, the settings popup and the serial popup.
#[test]
fn hover() {
    use ratatui::style::Modifier;
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let moved = |h: &mut Harness, at| {
        let action = crate::inputs::handle_event(
            mouse(MouseEventKind::Moved, at, KeyModifiers::NONE),
            &h.app,
        );
        let ignored = matches!(action, Action::Ignore);
        h.app.update(action);
        ignored
    };
    let reversed = |h: &Harness, (x, y): (u16, u16)| {
        h.terminal.backend().buffer()[(x, y)]
            .modifier
            .contains(Modifier::REVERSED)
    };

    mouse_do(&mut h, MouseEventKind::Down(MouseButton::Right), (10, 3));
    let rows = h.draw();
    assert!(!moved(&mut h, cell_of(&rows, "Settings")));
    let rows = h.draw();
    assert!(rows[5].contains("▶ Settings"), "{:?}", rows[5]);
    h.app.update(Action::Overlay(DialogAction::Confirm));
    assert!(matches!(h.app.overlay(), Some(Overlay::Settings(_))));

    let rows = h.draw();
    let echo = cell_of(&rows, "Local Echo");
    assert!(!moved(&mut h, echo));
    h.draw();
    assert!(reversed(&h, echo));
    assert!(moved(&mut h, (echo.0 + 2, echo.1)));
    assert!(!moved(&mut h, (0, 0)));
    h.draw();
    assert!(!reversed(&h, echo));
    h.app.close_overlay();

    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::LinkSettings(
            LineSettingsDialog::new(&LinkConfig::default()),
        ))));
    let rows = h.draw();
    let before = match h.app.overlay() {
        Some(Overlay::LinkSettings(d)) => d.selected_field,
        _ => unreachable!(),
    };
    moved(&mut h, cell_of(&rows, "Parity"));
    h.draw();
    match h.app.overlay() {
        Some(Overlay::LinkSettings(d)) => assert_eq!(d.selected_field, before),
        _ => panic!("serial popup closed"),
    }
}

/// MAX-128: with borders on, the history border's top-right has `[?][≡][×]` inside its
/// corner: help, settings, and quit through the confirm popup; with borders off they are
/// neither drawn nor clickable.
/// How: each button clicked in line mode, then the same cell with borders off.
#[test]
fn border_buttons() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 24, |s| {
        s.ui.show_borders = true;
    });
    let rows = h.draw();
    assert!(rows[0].ends_with("[?][≡][×]┐"), "{:?}", rows[0]);

    let (x, y) = cell_of(&rows, "[?]");
    for (dx, expected) in [(1, "help"), (4, "settings"), (7, "quit")] {
        click(&mut h, (x + dx, y));
        let opened = match h.app.overlay() {
            Some(Overlay::Help(_)) => "help",
            Some(Overlay::Settings(_)) => "settings",
            Some(Overlay::ConfirmQuit) => "quit",
            _ => "none",
        };
        assert_eq!(opened, expected);
        h.app.close_overlay();
    }

    h.app.settings_mut().ui.show_borders = false;
    let rows = h.draw();
    assert!(!rows[0].contains("[?]"));
    click(&mut h, (x + 1, y));
    assert!(h.app.overlay().is_none());
}

/// MAX-137: the history's title is ` maxicom LINE ` / ` maxicom TERMINAL `; with
/// `title_details` the sizes, scroll and prefix follow.
/// How: line then terminal mode at 120x24, details off then on, the prefix active.
#[test]
fn border_title_details() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 120, 24, |s| {
        s.ui.show_borders = true;
    });
    let title = |h: &mut Harness| {
        let rows = h.draw();
        rows[0]
            .trim_start_matches('┌')
            .split('─')
            .next()
            .unwrap()
            .to_string()
    };
    assert_eq!(title(&mut h), " maxicom LINE ");
    h.app.settings_mut().ui.title_details = true;
    assert_eq!(title(&mut h), " maxicom LINE [VIEW 118x19 | LIVE] ");
    h.app.update(Action::App(AppAction::SetPrefix(true)));
    assert_eq!(
        title(&mut h),
        " maxicom LINE [VIEW 118x19 | LIVE] | PREFIX "
    );

    h.app.update(Action::App(AppAction::SetPrefix(false)));
    h.app.settings_mut().mode = Mode::Terminal;
    assert_eq!(
        title(&mut h),
        " maxicom TERMINAL [VT 118x24 | VIEW 118x22 | LIVE] "
    );
    h.app.settings_mut().ui.title_details = false;
    assert_eq!(title(&mut h), " maxicom TERMINAL ");
}

/// MAX-137: a title that doesn't fit loses whole details from the last one, then the
/// prefix, then everything but the name, and is gone without room for the name; it is never
/// cut inside a word, and the border buttons stay.
/// How: line mode with details and the prefix at each width from 120 down to 20; then the
/// exact titles at a few widths.
#[test]
fn border_title_fits_whole() {
    let title_at = |width| {
        let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, width, 24, |s| {
            s.ui.show_borders = true;
            s.ui.title_details = true;
        });
        h.app.update(Action::App(AppAction::SetPrefix(true)));
        let rows = h.draw();
        assert!(rows[0].ends_with("[?][≡][×]┐"), "{width}: {:?}", rows[0]);
        let title = rows[0].trim_start_matches('┌').split('─').next().unwrap();
        title.to_string()
    };
    let full = " maxicom LINE [VIEW 118x19 | LIVE] | PREFIX ";
    assert_eq!(title_at(120), full);
    for width in 20..=120 {
        let title = title_at(width);
        assert!(
            title.is_empty()
                || [" maxicom LINE ", "] ", "] | PREFIX ", "LINE | PREFIX "]
                    .iter()
                    .any(|end| title.ends_with(end)),
            "{width}: {title:?}"
        );
    }
    // 40 columns leave 28 for the title: the details go before the prefix
    assert_eq!(title_at(40), " maxicom LINE | PREFIX ");
    assert_eq!(title_at(30), " maxicom LINE ");
    assert_eq!(title_at(20), "");
}

/// MAX-136, MAX-181: the borders, the scrollbar and the status bar each hide on a screen
/// narrower or shorter than their `hide_*_below` size, the others staying; the settings are
/// left as they are, so a bigger screen brings them back. A `0` side never hides. The
/// border buttons aren't clickable while hidden.
/// How: line mode, borders below 50x14, scrollbar below 40x10, status bar below 30x6: each
/// size past one more of them, in width then in height; a `0` side; terminal mode below
/// them all.
#[test]
fn compact_screen() {
    use ratatui::layout::Rect;
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 24, |s| {
        s.ui.show_borders = true;
        s.ui.hide_borders_below = [50, 14];
        s.ui.hide_scrollbar_below = [40, 10];
        s.ui.hide_status_bar_below = [30, 6];
    });
    // borders, scrollbar, status bar
    let parts = |h: &mut Harness, w, rows| {
        h.terminal.backend_mut().resize(w, rows);
        let drawn = h.draw();
        let layout = ScreenLayout::compute(Rect::new(0, 0, w, rows), &h.app);
        (
            drawn[0].starts_with('┌'),
            layout.scrollbar_area.is_some(),
            layout.status_bar_area.is_some(),
        )
    };

    assert_eq!(parts(&mut h, 80, 24), (true, true, true));
    for (w, rows, shown) in [
        (45, 24, (false, true, true)),
        (35, 24, (false, false, true)),
        (25, 24, (false, false, false)),
        (80, 12, (false, true, true)),
        (80, 8, (false, false, true)),
        (80, 5, (false, false, false)),
    ] {
        assert_eq!(parts(&mut h, w, rows), shown, "{w}x{rows}");
    }
    let ui = &h.app.settings().ui;
    assert!(ui.show_borders && ui.show_status_bar && ui.show_scrollbar);

    // where `[≡]` is at 40 columns with borders
    parts(&mut h, 40, 24);
    click(&mut h, (33, 0));
    assert!(h.app.overlay().is_none());

    h.app.settings_mut().ui.hide_borders_below = [0, 14];
    assert_eq!(parts(&mut h, 45, 24), (true, true, true));
    assert_eq!(parts(&mut h, 80, 12), (false, true, true));

    h.app.settings_mut().mode = Mode::Terminal;
    parts(&mut h, 25, 5);
    assert_eq!(h.app.frame_areas().content, Rect::new(0, 0, 25, 5));
}

/// MAX-181: with the default `[ui]`, terminal mode keeps its borders on 80x24 (a 78x22
/// view, the status bar in the bottom border) down to 60x16; below, the status bar is a
/// row of its own and the scrollbar a column, down to 30x6 and 40x10. A hidden status bar
/// shows over the view's last row while the prefix is on, the view keeping its size.
/// How: 80x24 and 60x16 with borders, then 59x24 and 80x15, 39x24, 29x24 with and without
/// the prefix.
#[test]
fn default_compact_thresholds() {
    use ratatui::layout::Rect;
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 24, |s| {
        s.mode = Mode::Terminal;
        s.ui = crate::configs::settings::UiConfig::default();
    });
    let at = |h: &mut Harness, w, rows| {
        h.terminal.backend_mut().resize(w, rows);
        let drawn = h.draw();
        let layout = ScreenLayout::compute(Rect::new(0, 0, w, rows), &h.app);
        (
            drawn[0].starts_with('┌'),
            h.app.frame_areas().content,
            layout.status_bar_area,
        )
    };

    for (w, rows) in [(80, 24), (60, 16)] {
        let bar = Rect::new(2, rows - 1, w - 4, 1);
        let with_borders = (true, Rect::new(1, 1, w - 2, rows - 2), Some(bar));
        assert_eq!(at(&mut h, w, rows), with_borders, "{w}x{rows}");
    }
    for (w, rows) in [(59, 24), (80, 15)] {
        let bar = Rect::new(0, rows - 1, w, 1);
        let line_bar = (false, Rect::new(0, 0, w - 1, rows - 1), Some(bar));
        assert_eq!(at(&mut h, w, rows), line_bar, "{w}x{rows}");
    }
    let no_scrollbar = (
        false,
        Rect::new(0, 0, 39, 23),
        Some(Rect::new(0, 23, 39, 1)),
    );
    assert_eq!(at(&mut h, 39, 24), no_scrollbar);

    let view = Rect::new(0, 0, 29, 24);
    assert_eq!(at(&mut h, 29, 24), (false, view, None));
    h.app.update(Action::App(AppAction::SetPrefix(true)));
    let over = Some(Rect::new(0, 23, 29, 1));
    assert_eq!(at(&mut h, 29, 24), (false, view, over));
}

/// MAX-181 5b: while the prefix is on, a status bar the screen's size hides is drawn over
/// the view's last row, in the line layout, and it's clickable; the emulator keeps its
/// size, and the row comes back when the prefix ends. A status bar turned off in the
/// settings stays off.
/// How: terminal mode on 29x24, "last" on the bottom row; the prefix on, a click on `≡`,
/// the prefix off; then `show_status_bar = false` with the prefix.
#[test]
fn hidden_status_bar_over_the_view_with_the_prefix() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 29, 24, |s| {
        s.mode = Mode::Terminal;
        s.ui = crate::configs::settings::UiConfig::default();
    });
    h.draw();
    let size = h.app.terminal_mode.get_vt100_size();
    h.app
        .terminal_mode
        .process_rx(TextEncoding::Utf8, b"\x1b[24;1Hlast");
    assert!(h.draw()[23].starts_with("last"));

    h.app.update(Action::App(AppAction::SetPrefix(true)));
    let rows = h.draw();
    assert!(!rows[23].contains("last"), "{:?}", rows[23]);
    assert!(rows[23].contains('≡'), "{:?}", rows[23]);
    assert_eq!(h.app.terminal_mode.get_vt100_size(), size);
    assert_eq!(
        h.app.status_bar_layout(),
        crate::configs::StatusBarLayout::Line
    );
    let gear = rows[23].chars().position(|c| c == '≡').unwrap() as u16;
    click(&mut h, (gear, 23));
    assert!(matches!(h.app.overlay(), Some(Overlay::Settings(_))));
    h.app.close_overlay();

    h.app.update(Action::App(AppAction::SetPrefix(false)));
    assert!(h.draw()[23].starts_with("last"));

    h.app.settings_mut().ui.show_status_bar = false;
    h.app.update(Action::App(AppAction::SetPrefix(true)));
    assert!(h.draw()[23].starts_with("last"));
}

/// MAX-140: a `grow_only` grid taller than the view (the window shrank) shows its rows down
/// to the cursor, not its blank bottom: the prompt on the grid's first row stays on the
/// view's first row, the view follows the cursor down, and once the grid is full it is the
/// bottom view as before. Scrolling up reaches the top rows and back down the live view.
/// How: terminal mode grown to 49 rows on an 80x50 screen, then an 80x21 screen (20 rows of
/// view): a prompt, 25 lines, scroll up and down, then 60 lines.
#[test]
fn terminal_view_follows_the_cursor_in_a_taller_grid() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 50, |s| {
        s.mode = Mode::Terminal;
    });
    h.draw();
    assert_eq!(h.app.terminal_mode.get_vt100_size().0, 49);
    let feed = |h: &mut Harness, bytes: &[u8]| {
        h.app.terminal_mode.process_rx(TextEncoding::Utf8, bytes);
    };
    feed(&mut h, b"$ ");
    h.terminal.backend_mut().resize(80, 21);
    let rows = h.draw();
    assert_eq!(h.app.terminal_mode.get_vt100_size().0, 49);
    assert!(rows[0].starts_with("$ "), "{:?}", &rows[..3]);
    assert_eq!(cursor_cell(&mut h), (2, 0));

    for i in 1..=25 {
        feed(&mut h, format!("\r\nline {i:02}").as_bytes());
    }
    let rows = h.draw();
    assert!(rows[19].starts_with("line 25"), "{rows:#?}");
    assert!(rows[0].starts_with("line 06"), "{rows:#?}");
    assert_eq!(cursor_cell(&mut h), (7, 19));

    h.scroll(HistoryAction::ScrollToTop);
    let rows = h.draw();
    assert!(rows[0].starts_with("$ "), "{rows:#?}");
    h.scroll(HistoryAction::ScrollToBottom);
    let rows = h.draw();
    assert!(rows[19].starts_with("line 25"), "{rows:#?}");

    for i in 26..=60 {
        feed(&mut h, format!("\r\nline {i:02}").as_bytes());
    }
    let rows = h.draw();
    assert!(rows[19].starts_with("line 60"), "{rows:#?}");
    assert_eq!(cursor_cell(&mut h), (7, 19));
}

/// MAX-140: content the remote drew below the cursor stays in the live view while it fits
/// with the cursor.
/// How: a 49-row grid in a 20-row view, "menu" drawn on row 16, the prompt back on row 1.
#[test]
fn terminal_content_below_the_cursor_stays_in_view() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 50, |s| {
        s.mode = Mode::Terminal;
    });
    h.draw();
    h.app
        .terminal_mode
        .process_rx(TextEncoding::Utf8, b"\x1b[16;1Hmenu\x1b[1;1H$ ");
    h.terminal.backend_mut().resize(80, 21);
    let rows = h.draw();
    assert!(rows[0].starts_with("$ "), "{rows:#?}");
    assert!(rows[15].starts_with("menu"), "{rows:#?}");
    assert_eq!(cursor_cell(&mut h), (2, 0));
}

/// The cell of the last `text` in the status bar (the last row): the buttons are at its
/// end, after a port name that may hold the same letter
fn status_cell(rows: &[String], text: &str) -> (u16, u16) {
    let y = rows.len() - 1;
    let x = rows[y].rfind(text).unwrap();
    (rows[y][..x].chars().count() as u16, y as u16)
}

/// MAX-125: a click on a status bar block runs it, in its left, center and right parts:
/// `{mode}` switches mode, `{baud}` opens the serial popup, `?` help and `≡` settings by
/// their `click=`; a click between blocks does nothing.
/// How: the default status bar, and a center part with its own `click=`.
#[test]
fn status_bar_clicks() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let rows = h.draw();
    click(&mut h, status_cell(&rows, "LINE"));
    assert_eq!(h.app.settings().mode, Mode::Terminal);

    let rows = h.draw();
    let baud = h.app.link_config().line.baud.to_string();
    click(&mut h, status_cell(&rows, &baud));
    assert!(matches!(h.app.overlay(), Some(Overlay::LinkSettings(_))));
    h.app.close_overlay();

    for (text, help) in [("?", true), ("≡", false)] {
        let rows = h.draw();
        click(&mut h, status_cell(&rows, text));
        match h.app.overlay() {
            Some(Overlay::Help(_)) => assert!(help, "{text}"),
            Some(Overlay::Settings(_)) => assert!(!help, "{text}"),
            _ => panic!("{text}: no popup"),
        }
        h.app.close_overlay();
    }

    let mut config = h.app.settings().ui.status_bar.clone();
    config.center = "[ mid ](click=settings)".to_string();
    h.status_bar = CompiledStatusBar::new(&config, &h.theme);
    let rows = h.draw();
    let (x, y) = status_cell(&rows, "mid");
    click(&mut h, (x - 2, y));
    assert!(h.app.overlay().is_none());
    click(&mut h, (x + 3, y));
    assert!(matches!(h.app.overlay(), Some(Overlay::Settings(_))));
}

/// MAX-125: a click on a popup's key hint runs it as its key would: Apply and Cancel in
/// the forms (Cancel in the text input one, whose Apply would start a log), Close in Help,
/// Quit and Reattach in the alerts.
/// How: each popup opened, its hint label found in the drawn rows and clicked.
#[test]
fn mouse_clicks_on_popup_hints() {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let open = |h: &mut Harness, overlay| {
        h.app.update(Action::App(AppAction::OpenOverlay(overlay)));
        h.draw()
    };
    let press = |h: &mut Harness, at| {
        let effect = mouse_do(h, MouseEventKind::Down(MouseButton::Left), at);
        mouse_do(h, MouseEventKind::Up(MouseButton::Left), at);
        effect
    };
    let serial = || Overlay::LinkSettings(LineSettingsDialog::new(&LinkConfig::default()));

    let rows = open(&mut h, serial());
    click(&mut h, cell_of(&rows, "Parity"));
    click(&mut h, cell_of(&rows, "Parity"));
    let rows = h.draw();
    assert!(matches!(
        press(&mut h, cell_of(&rows, "Apply")),
        UpdateEffect::Reconfigure(_)
    ));
    assert!(h.app.overlay().is_none());

    let rows = open(&mut h, serial());
    assert!(matches!(
        press(&mut h, cell_of(&rows, "Cancel")),
        UpdateEffect::None
    ));
    assert!(h.app.overlay().is_none());

    let rows = open(&mut h, Overlay::Help(HelpDialog::new()));
    press(&mut h, cell_of(&rows, "Close"));
    assert!(h.app.overlay().is_none());

    let rows = open(
        &mut h,
        Overlay::FilePath(FilePathDialog::new(Stream::Text, "x".into()).with_session_backup()),
    );
    // the stream row switches the stream (MAX-147)
    press(&mut h, cell_of(&rows, " full "));
    assert!(matches!(
        h.app.overlay(),
        Some(Overlay::FilePath(dialog)) if dialog.stream == Stream::Full
    ));
    let rows = h.draw();
    assert!(has(&rows, "x/"), "{rows:#?}");
    // so does the include row, with a session backup
    press(&mut h, cell_of(&rows, "since session start"));
    assert!(matches!(
        h.app.overlay(),
        Some(Overlay::FilePath(dialog)) if dialog.include_session == Some(true)
    ));
    let rows = h.draw();
    assert!(has(&rows, "[x] since session start"), "{rows:#?}");
    press(&mut h, cell_of(&rows, "Cancel"));
    assert!(h.app.overlay().is_none());

    // the screenshot format row switches the format (MAX-147)
    let rows = open(
        &mut h,
        Overlay::Screenshot(ScreenshotDialog::new(
            ScreenshotFormat::Text,
            "shot",
            Screenshot::default(),
        )),
    );
    press(&mut h, cell_of(&rows, " ansi "));
    assert!(matches!(
        h.app.overlay(),
        Some(Overlay::Screenshot(dialog)) if dialog.format == ScreenshotFormat::Ansi
    ));
    let rows = h.draw();
    assert!(has(&rows, "shot.ans"), "{rows:#?}");
    press(&mut h, cell_of(&rows, "Cancel"));
    assert!(h.app.overlay().is_none());

    let rows = open(&mut h, Overlay::DeviceDisconnected);
    assert!(matches!(
        press(&mut h, cell_of(&rows, "Quit")),
        UpdateEffect::Quit
    ));

    let rows = open(&mut h, Overlay::DeviceDetached);
    assert!(matches!(
        press(&mut h, cell_of(&rows, "Reattach")),
        UpdateEffect::ToggleAttach
    ));
    // the last cell of an odd-width centered hint line: the paragraph's own centering
    let rows = open(&mut h, Overlay::DeviceDetached);
    let (x, y) = cell_of(&rows, "Quit");
    assert!(matches!(press(&mut h, (x + 3, y)), UpdateEffect::Quit));
}

/// A click in a popup picks the part under it: a help tab, a setting (toggled), a serial
/// field (a second click cycles it), a resize field. A click outside closes the popup, but
/// not a connection alert, where closing quits (MAX-113).
#[test]
fn mouse_clicks_in_popups() {
    use crate::link::serial::Parity;
    use crate::overlays::{HelpCategory, LineSettingField, ResizeField};
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    let open = |h: &mut Harness, overlay| {
        h.app.update(Action::App(AppAction::OpenOverlay(overlay)));
        h.draw()
    };

    let rows = open(&mut h, Overlay::Help(HelpDialog::new()));
    click(&mut h, cell_of(&rows, "Search"));
    assert!(
        matches!(h.app.overlay(), Some(Overlay::Help(d)) if d.category == HelpCategory::Search)
    );
    // the tabs overflow the popup: the arrow picks the hidden one past it (MAX-12)
    assert!(!has(&rows, "Dialogs"), "{rows:#?}");
    click(&mut h, cell_of(&rows, "▶"));
    assert!(
        matches!(h.app.overlay(), Some(Overlay::Help(d)) if d.category == HelpCategory::Dialog)
    );
    let rows = h.draw();
    assert!(has(&rows, "Dialogs") && !has(&rows, "▶"), "{rows:#?}");
    let (x, y) = cell_of(&rows, "Copy");
    click(&mut h, (x + 3, y));
    assert!(
        matches!(h.app.overlay(), Some(Overlay::Help(d)) if d.category == HelpCategory::CopyMode)
    );
    let rows = h.draw();
    click(&mut h, cell_of(&rows, "◀"));
    assert!(
        matches!(h.app.overlay(), Some(Overlay::Help(d)) if d.category == HelpCategory::Prefix)
    );
    click(&mut h, (0, 0));
    assert!(h.app.overlay().is_none());

    let rows = open(&mut h, Overlay::Settings(SettingsDialog::new()));
    assert!(!h.app.settings().local_echo);
    click(&mut h, cell_of(&rows, "Local Echo"));
    assert!(h.app.settings().local_echo);
    assert!(h.app.overlay().is_some());

    let rows = open(
        &mut h,
        Overlay::LinkSettings(LineSettingsDialog::new(&LinkConfig::default())),
    );
    let parity = cell_of(&rows, "Parity");
    click(&mut h, parity);
    click(&mut h, parity);
    match h.app.overlay() {
        Some(Overlay::LinkSettings(d)) => {
            assert_eq!(d.selected_field, LineSettingField::Parity);
            assert_ne!(d.line.framing.parity, Parity::None);
        }
        _ => panic!("serial popup closed"),
    }

    let rows = open(&mut h, Overlay::TerminalSize(ResizeDialog::new(24, 80)));
    click(&mut h, cell_of(&rows, "Columns"));
    assert!(
        matches!(h.app.overlay(), Some(Overlay::TerminalSize(d)) if d.active_field == ResizeField::Cols)
    );

    open(&mut h, Overlay::DeviceDisconnected);
    click(&mut h, (0, 0));
    assert!(matches!(h.app.overlay(), Some(Overlay::DeviceDisconnected)));
}

/// MAX-134: in the serial popup, a click on a field's `<` or `>` selects it and steps its
/// value back or forth; a click elsewhere on the row still selects, then cycles.
/// How: the parity row's `>` twice and `<` once, from the baud rate selected; then the
/// stop bits' `<`.
#[test]
fn serial_popup_arrows() {
    use crate::link::serial::Parity;
    use crate::overlays::LineSettingField;
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::LinkSettings(
            LineSettingsDialog::new(&LinkConfig::default()),
        ))));
    let rows = h.draw();
    // the arrows of a row: its first `<` and its last `>`
    let arrows = |rows: &[String], label: &str| {
        let y = cell_of(rows, label).1;
        let row: Vec<char> = rows[y as usize].chars().collect();
        let left = row.iter().position(|&c| c == '<').unwrap() as u16;
        let right = row.iter().rposition(|&c| c == '>').unwrap() as u16;
        ((left, y), (right, y))
    };
    let dialog = |h: &Harness| match h.app.overlay() {
        Some(Overlay::LinkSettings(d)) => d.clone(),
        _ => panic!("serial popup closed"),
    };
    let parity = dialog(&h).line.framing.parity;
    assert_eq!(parity, Parity::None);

    let (left, right) = arrows(&rows, "Parity");
    click(&mut h, right);
    assert_eq!(dialog(&h).selected_field, LineSettingField::Parity);
    assert_eq!(dialog(&h).line.framing.parity, Parity::Even);
    click(&mut h, right);
    assert_eq!(dialog(&h).line.framing.parity, Parity::Odd);
    click(&mut h, left);
    assert_eq!(dialog(&h).line.framing.parity, Parity::Even);

    let rows = h.draw();
    let (left, _) = arrows(&rows, "Stop Bits");
    let stop_bits = dialog(&h).line.framing.stop_bits;
    click(&mut h, left);
    assert_eq!(dialog(&h).selected_field, LineSettingField::StopBits);
    assert_ne!(dialog(&h).line.framing.stop_bits, stop_bits);
}

/// MAX-134: in the resize popup the sideways wheel (Shift + wheel too) steps the active
/// number, kept from 1 up, and only digits can be typed or pasted (4 at most).
/// How: Shift + wheel down and up, the side wheel, on the rows field at 24; a letter and a
/// digit typed, a mixed paste, then down to 1.
#[test]
fn resize_popup_wheel_and_digits() {
    use crossterm::event::KeyCode::Char;
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::TerminalSize(
            ResizeDialog::new(24, 80),
        ))));
    let rows = h.draw();
    let at = cell_of(&rows, "Rows");
    let rows_value = |h: &Harness| match h.app.overlay() {
        Some(Overlay::TerminalSize(d)) => d.rows_input.value().to_string(),
        _ => panic!("resize popup closed"),
    };
    let wheel = |h: &mut Harness, kind, modifiers| {
        let action = crate::inputs::handle_event(mouse(kind, at, modifiers), &h.app);
        h.app.update(action);
    };

    wheel(&mut h, MouseEventKind::ScrollDown, KeyModifiers::SHIFT);
    assert_eq!(rows_value(&h), "25");
    wheel(&mut h, MouseEventKind::ScrollUp, KeyModifiers::SHIFT);
    wheel(&mut h, MouseEventKind::ScrollLeft, KeyModifiers::NONE);
    assert_eq!(rows_value(&h), "23");
    wheel(&mut h, MouseEventKind::ScrollRight, KeyModifiers::NONE);
    assert_eq!(rows_value(&h), "24");

    key_do(&mut h, Char('x'), KeyModifiers::NONE);
    key_do(&mut h, Char('0'), KeyModifiers::NONE);
    assert_eq!(rows_value(&h), "240");
    h.app
        .update(Action::Overlay(DialogAction::Paste("a1".to_string())));
    assert_eq!(rows_value(&h), "2401");

    h.app
        .update(Action::App(AppAction::OpenOverlay(Overlay::TerminalSize(
            ResizeDialog::new(2, 80),
        ))));
    for _ in 0..3 {
        wheel(&mut h, MouseEventKind::ScrollLeft, KeyModifiers::NONE);
    }
    assert_eq!(rows_value(&h), "1");
}

/// MAX-141: sending returns to the live view, receiving never moves a scrolled one; the
/// `new_data` condition is on while data arrived scrolled up, unseen. Line mode: typing in
/// the TX box keeps the view, Enter returns. Terminal mode: a key or a paste returns, a key
/// that sends nothing doesn't. The default templates don't use it.
/// How: a bar showing `NEW` on `new_data`, 30 lines, scrolled up 3, then RX, keys, Enter,
/// paste, in each mode.
#[test]
fn sending_returns_receiving_doesnt() {
    use crate::inputs::{LineInputAction, TerminalAction};
    use crossterm::event::{KeyCode, KeyEvent};
    let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
    for mode in [Mode::Line, Mode::Terminal] {
        let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 10, |s| {
            s.mode = mode;
        });
        let mut config = h.app.settings().ui.status_bar.clone();
        config.center = "[NEW](new_data?warn:hide)".to_string();
        h.status_bar = CompiledStatusBar::new(&config, &h.theme);
        for i in 0..30 {
            h.app.feed_rx(format!("line {i}\r\n").as_bytes());
        }
        let new_data = |h: &mut Harness| h.draw()[9].contains("NEW");
        let offset = |h: &Harness| h.app.scroll_status().offset_from_bottom;
        assert!(!new_data(&mut h), "{mode:?} live");
        h.scroll(HistoryAction::ScrollUp(3));
        assert!(!new_data(&mut h), "{mode:?} scrolled, nothing new");
        assert!(offset(&h) > 0, "{mode:?}");
        let shown = h.draw()[..9].to_vec();

        h.app.feed_rx(b"more\r\nand more\r\n");
        assert!(new_data(&mut h), "{mode:?} new data");
        assert_eq!(h.draw()[..9], shown, "{mode:?} the view stays");

        match mode {
            Mode::Line => {
                h.app
                    .update(Action::LineInput(LineInputAction::HandleKey(key(
                        KeyCode::Char('x'),
                    ))));
                assert!(new_data(&mut h) && offset(&h) > 0, "typing keeps it");
                h.app.update(Action::LineInput(LineInputAction::Submit));
            }
            Mode::Terminal => {
                let caps = h.app.update(Action::Terminal(TerminalAction::Key(key(
                    KeyCode::CapsLock,
                ))));
                assert!(matches!(caps, UpdateEffect::None));
                assert!(new_data(&mut h) && offset(&h) > 0, "nothing sent");
                h.app
                    .update(Action::Terminal(TerminalAction::Key(key(KeyCode::Char(
                        'x',
                    )))));
            }
        }
        assert!(!new_data(&mut h), "{mode:?} seen");
        assert_eq!(offset(&h), 0, "{mode:?} back live");

        if mode == Mode::Terminal {
            h.scroll(HistoryAction::ScrollUp(3));
            h.draw();
            assert!(offset(&h) > 0);
            h.app
                .update(Action::Terminal(TerminalAction::Paste("ls".to_string())));
            assert_eq!(offset(&h), 0, "a paste returns");
        }
    }

    let d = crate::configs::StatusBarConfig::default();
    for template in [&d.left, &d.center, &d.right] {
        assert!(!template.contains("new_data"));
    }
}

/// MAX-142: the history's scrollbar takes the mouse: `▲` / `▼` scroll a row and repeat
/// while held, a press on the track puts the thumb there (its top row: the top, its bottom
/// row: the live view), the thumb dragged moves the view, off the scrollbar's column and
/// past its ends too; the arrows and the thumb take the hover style.
/// How: 40 lines in line mode, 80x12 without borders, then 60 lines in terminal mode.
#[test]
fn scrollbar_mouse() {
    use ratatui::style::Modifier;
    use std::time::{Duration, Instant};
    for mode in [Mode::Line, Mode::Terminal] {
        let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 12, |s| {
            s.mode = mode;
        });
        for i in 0..60 {
            h.app.feed_rx(format!("line {i:02}\r\n").as_bytes());
        }
        h.draw();
        let bar = h.app.frame_map().scrollbar.expect("a scrollbar");
        let (x, top, bottom) = (bar.area.x, bar.area.y, bar.area.bottom() - 1);
        let offset = |h: &mut Harness| {
            h.draw();
            h.app.scroll_status().offset_from_bottom
        };
        let press = |h: &mut Harness, at| mouse_do(h, MouseEventKind::Down(MouseButton::Left), at);
        let release = |h: &mut Harness, at| mouse_do(h, MouseEventKind::Up(MouseButton::Left), at);

        click(&mut h, (x, top));
        assert_eq!(offset(&mut h), 1, "{mode:?} ▲");
        click(&mut h, (x, bottom));
        assert_eq!(offset(&mut h), 0, "{mode:?} ▼");

        // held, it repeats after a while
        press(&mut h, (x, top));
        assert!(!h.app.scrollbar_repeat(Instant::now()));
        assert!(
            h.app
                .scrollbar_repeat(Instant::now() + Duration::from_millis(400))
        );
        release(&mut h, (x, top));
        assert!(!h.app.is_scrollbar_held());
        assert_eq!(offset(&mut h), 2, "{mode:?} repeated");

        click(&mut h, (x, top + 1));
        let max = h.app.scroll_status().max_scroll;
        assert_eq!(offset(&mut h), max, "{mode:?} track top");
        assert!(
            h.draw().iter().any(|row| row.contains("line 00")),
            "{mode:?}"
        );
        click(&mut h, (x, bottom - 1));
        assert_eq!(offset(&mut h), 0, "{mode:?} track bottom");

        // the thumb, at the bottom while live, dragged
        let bar = h.app.frame_map().scrollbar.unwrap();
        let (first, _) = bar.thumb();
        let thumb = top + 1 + first as u16;
        press(&mut h, (x, thumb));
        mouse_do(
            &mut h,
            MouseEventKind::Drag(MouseButton::Left),
            (x, top + 3),
        );
        let middle = offset(&mut h);
        assert!(0 < middle && middle < max, "{mode:?} {middle}/{max}");
        mouse_do(
            &mut h,
            MouseEventKind::Drag(MouseButton::Left),
            (5, top + 3),
        );
        assert_eq!(offset(&mut h), middle, "{mode:?} off the column");
        mouse_do(&mut h, MouseEventKind::Drag(MouseButton::Left), (5, 0));
        assert_eq!(offset(&mut h), max, "{mode:?} above");
        mouse_do(&mut h, MouseEventKind::Drag(MouseButton::Left), (5, 11));
        assert_eq!(offset(&mut h), 0, "{mode:?} below");
        release(&mut h, (5, 11));
        assert!(!h.app.is_scrollbar_held());

        // the hover: the arrow, then the whole thumb
        let reversed = |h: &Harness, y| {
            h.terminal.backend().buffer()[(x, y)]
                .modifier
                .contains(Modifier::REVERSED)
        };
        let moved = |h: &mut Harness, at| {
            let event = mouse(MouseEventKind::Moved, at, KeyModifiers::NONE);
            let action = crate::inputs::handle_event(event, &h.app);
            h.app.update(action);
            h.draw();
        };
        moved(&mut h, (x, top));
        assert!(reversed(&h, top) && !reversed(&h, top + 1), "{mode:?}");
        let (first, length) = h.app.frame_map().scrollbar.unwrap().thumb();
        moved(&mut h, (x, top + 1 + first as u16));
        for row in 0..length as u16 {
            assert!(reversed(&h, top + 1 + first as u16 + row), "{mode:?} thumb");
        }
    }

    // line mode with entries 3 rows high: a press on the track lands on the exact row
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 12);
    for i in 0..20 {
        h.feed(format!("{i:02} {}\r\n", "x".repeat(150)).as_bytes());
    }
    h.draw();
    let bar = h.app.frame_map().scrollbar.unwrap();
    for row in 1..bar.area.height - 1 {
        click(&mut h, (bar.area.x, bar.area.y + row));
        h.draw();
        let (_, length) = bar.thumb();
        let first = row as isize - 1 - (length / 2) as isize;
        let expected = bar.max - bar.position_at(first);
        assert_eq!(
            h.app.scroll_status().offset_from_bottom,
            expected,
            "row {row}"
        );
    }

    // nothing to scroll: no scrollbar, a click there goes to the history as before
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 12);
    h.feed(b"one\r\n");
    h.draw();
    assert!(h.app.frame_map().scrollbar.is_none());
    click(&mut h, (79, 0));
    assert!(!h.app.is_scrollbar_held());
}

/// A click in the search bar's query moves its cursor to the clicked char, the end past
/// the text; a click on the bar's label leaves it. Without borders in line mode, and in the
/// history's bottom border in terminal mode.
/// How: "hello world" pasted in the query, clicks on "world", past its end and on
/// "SEARCH", the cursor's index and its cell on screen.
#[test]
fn mouse_click_in_the_search_query() {
    for (mode, borders) in [(Mode::Line, false), (Mode::Terminal, true)] {
        let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 12, |s| {
            s.mode = mode;
            s.ui.show_borders = borders;
        });
        h.feed(b"one\r\n");
        h.app
            .terminal_mode
            .process_rx(TextEncoding::Utf8, b"one\r\n");
        h.draw();
        h.app.update(Action::Search(SearchModeAction::Start));
        h.app.update(Action::Search(SearchModeAction::Paste(
            "hello world".to_string(),
        )));
        let cursor = |h: &Harness| match mode {
            Mode::Line => h.app.line_mode.search().query.cursor(),
            Mode::Terminal => h.app.terminal_mode.search().query.cursor(),
        };
        let rows = h.draw();
        let world = cell_of(&rows, "world");
        click(&mut h, world);
        assert_eq!(cursor(&h), 6, "{mode:?}");
        h.draw();
        assert_eq!(cursor_cell(&mut h), world, "{mode:?}");

        click(&mut h, (world.0 + 5, world.1));
        assert_eq!(cursor(&h), 11, "{mode:?}");
        click(&mut h, cell_of(&rows, "SEARCH"));
        assert_eq!(cursor(&h), 11, "{mode:?}");
        assert!(h.app.is_search_active(), "{mode:?}");
    }
}

/// A click in the TX input moves its cursor to the clicked char (the end past the text),
/// and leaves copy mode for it.
#[test]
fn mouse_click_in_the_tx_input() {
    use crate::inputs::LineInputAction;
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 60, 12);
    h.feed(b"one\r\n");
    h.app.update(Action::LineInput(LineInputAction::Paste(
        "hello world".into(),
    )));
    let rows = h.draw();
    let (x, y) = cell_of(&rows, "world");

    click(&mut h, (x, y));
    assert_eq!(h.app.line_mode.input().cursor(), 6);
    h.draw();
    assert_eq!(cursor_cell(&mut h), (x, y));

    click(&mut h, (x + 30, y));
    assert_eq!(h.app.line_mode.input().cursor(), 11);

    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    h.draw();
    click(&mut h, (x + 1, y));
    assert!(!h.app.line_mode.copy_state().active);
    assert_eq!(h.app.line_mode.input().cursor(), 7);

    // an input wider than the prompt is scrolled: the click counts the scroll
    let long: String = (0..30).map(|i| format!("a{i:02} ")).collect();
    h.app.update(Action::LineInput(LineInputAction::ClearLine));
    h.app
        .update(Action::LineInput(LineInputAction::Paste(long.clone())));
    let rows = h.draw();
    click(&mut h, cell_of(&rows, "a27"));
    assert_eq!(h.app.line_mode.input().cursor(), long.find("a27").unwrap());
}

/// MAX-145: an input wider than the prompt shows a mark on each edge hiding text, `◀` on
/// the left and `▶` on the right, never on the cursor's cell; the prefix stays. The view
/// stays while the cursor moves in it, and scrolls by one when the cursor pushes past an
/// edge. A text that fits has none.
/// How: 100 digits in a 36-column prompt, the cursor at the end, moved left to the left
/// mark and once more, at the start, then 40 columns in; the input cleared to "abc".
#[test]
fn prompt_marks_hidden_text() {
    use crate::inputs::LineInputAction;
    use crossterm::event::KeyCode::{Home, Left, Right};
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 38, 12);
    let digits: String = (0..100).map(|i| char::from(b'0' + i % 10)).collect();
    h.app
        .update(Action::LineInput(LineInputAction::Paste(digits)));
    // the prompt's row from its prefix, and the cursor's column in the text
    let prompt = |h: &mut Harness| {
        let rows = h.draw();
        let area = h.app.frame_map().prompt.unwrap().text_area;
        assert_eq!(area.width, 36);
        let row: String = rows[area.y as usize]
            .chars()
            .skip(area.x as usize - 2)
            .take(area.width as usize + 2)
            .collect();
        (row, cursor_cell(h).0 - area.x)
    };

    // the mark, the last 34 digits, the cursor after them
    let (row, cursor) = prompt(&mut h);
    assert_eq!(row, "> ◀6789012345678901234567890123456789 ");
    assert_eq!(cursor, 35);

    // the view stays up to the left mark, then scrolls by one
    for _ in 0..34 {
        key_do(&mut h, Left, KeyModifiers::NONE);
    }
    let (row, cursor) = prompt(&mut h);
    assert_eq!(row, "> ◀6789012345678901234567890123456789 ");
    assert_eq!(cursor, 1);
    key_do(&mut h, Left, KeyModifiers::NONE);
    let (row, cursor) = prompt(&mut h);
    assert_eq!(row, "> ◀5678901234567890123456789012345678▶");
    assert_eq!(cursor, 1);

    key_do(&mut h, Home, KeyModifiers::NONE);
    let (row, cursor) = prompt(&mut h);
    assert_eq!(row, "> 01234567890123456789012345678901234▶");
    assert_eq!(cursor, 0);

    // on digit 40, in the last cell before the right mark
    for _ in 0..40 {
        key_do(&mut h, Right, KeyModifiers::NONE);
    }
    let (row, cursor) = prompt(&mut h);
    assert_eq!(row, "> ◀7890123456789012345678901234567890▶");
    assert_eq!(cursor, 34);

    h.app.update(Action::LineInput(LineInputAction::ClearLine));
    h.app
        .update(Action::LineInput(LineInputAction::Paste("abc".into())));
    let (row, _) = prompt(&mut h);
    assert_eq!(row.trim_end(), "> abc");
}

/// Terminal mode: once the remote program asks for the mouse (here DECSET 1000 + 1006),
/// events on the grid are sent to it as SGR reports; with copy mode on, scrolled into the
/// history, or before it asks, the mouse acts on maxicom (MAX-113).
#[test]
fn mouse_forwarded_to_a_remote_program() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 30, |s| {
        s.mode = Mode::Terminal;
    });
    let feed = |h: &mut Harness, bytes: &[u8]| {
        h.app.terminal_mode.process_rx(TextEncoding::Utf8, bytes);
        h.draw();
    };
    feed(&mut h, b"hello");
    let area = h.app.frame_areas().content;
    let at = (area.x + 4, area.y + 2);
    let press = MouseEventKind::Down(MouseButton::Left);
    let sent = |h: &mut Harness, kind| match mouse_do(h, kind, at) {
        UpdateEffect::Transmit(bytes) => Some(bytes),
        _ => None,
    };

    // maxicom's, a plain press waiting for a drag to enter copy mode
    assert_eq!(sent(&mut h, press), None);
    assert!(h.app.pending_press_side().is_some());
    mouse_do(&mut h, MouseEventKind::Up(MouseButton::Left), at);

    feed(&mut h, b"\x1b[?1000h\x1b[?1006h");
    assert_eq!(sent(&mut h, press).unwrap(), b"\x1b[<0;5;3M");
    let release = MouseEventKind::Up(MouseButton::Left);
    assert_eq!(sent(&mut h, release).unwrap(), b"\x1b[<0;5;3m");
    assert_eq!(
        sent(&mut h, MouseEventKind::ScrollUp).unwrap(),
        b"\x1b[<64;5;3M"
    );
    // not reported in this mode, and not acted on locally either
    let drag = MouseEventKind::Drag(MouseButton::Left);
    assert_eq!(sent(&mut h, drag), None);
    assert!(!h.app.terminal_mode.copy_state().active);

    // copy mode on: the mouse is maxicom's
    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    h.draw();
    assert_eq!(sent(&mut h, press), None);
    h.app.update(Action::CopyMode(CopyModeAction::Exit));

    // scrolled into the history: the wheel scrolls it
    feed(&mut h, &b"line\r\n".repeat(40));
    h.app.update(Action::History(HistoryAction::ScrollUp(3)));
    h.draw();
    assert_eq!(sent(&mut h, MouseEventKind::ScrollUp), None);
    assert!(h.app.terminal_mode.vt100_screen().scrollback() > 0);
}

/// 60 numbered lines, mark a on "line 19" and b on "line 39" as they arrive, the live view
/// drawn: the harness and the history's text rows count (MAX-164)
fn marked_lines() -> (Harness, usize) {
    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 40, 12);
    for i in 0..60 {
        h.feed(format!("line {i:02}\r\n").as_bytes());
        match i {
            19 => assert_eq!(h.app.line_mode.add_mark('a'), Some(19)),
            39 => assert_eq!(h.app.line_mode.add_mark('b'), Some(39)),
            _ => {}
        }
    }
    h.draw();
    let height = h.app.frame_areas().content.height as usize;
    (h, height)
}

/// MAX-164: out of copy mode a mark jump only scrolls: the mark's line on the view's middle
/// row, no copy mode; an unset mark moves nothing.
/// How: from the live view, jump to a, then to b, then to the unset z.
#[test]
fn mark_jump_centers_the_view() {
    let (mut h, height) = marked_lines();
    assert_eq!(h.app.line_mode.jump_to_mark('a'), Some(19));
    let rows = text_rows(&mut h, height);
    assert_eq!(rows[height / 2], "a line 19", "{rows:#?}");
    assert!(!h.app.is_copy_mode_active());

    h.app.line_mode.jump_to_mark('b');
    let rows = text_rows(&mut h, height);
    assert_eq!(rows[height / 2], "b line 39", "{rows:#?}");

    assert_eq!(h.app.line_mode.jump_to_mark('z'), None);
    assert_eq!(text_rows(&mut h, height), rows);
}

/// MAX-164: in copy mode a mark jump moves the copy cursor to the start of the mark's line,
/// which the view shows centered.
/// How: copy mode from the live view, jump to a.
#[test]
fn mark_jump_moves_the_copy_cursor() {
    let (mut h, height) = marked_lines();
    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    h.draw();
    h.app.line_mode.jump_to_mark('a');
    let cursor = h.app.line_mode.copy_state().cursor;
    assert_eq!((cursor.line_idx, cursor.char_idx), (19, 0));
    let rows = text_rows(&mut h, height);
    assert_eq!(rows[height / 2], "a line 19", "{rows:#?}");
}

/// MAX-164: previous / next mark walk the marks from the view's middle line, or from the copy
/// cursor in copy mode; past the last one nothing moves.
/// How: from the live view (middle around line 55) previous twice, once more, then next; in
/// copy mode from the live view's last line, previous twice, next twice.
#[test]
fn previous_and_next_mark() {
    let (mut h, height) = marked_lines();
    let walk = |h: &mut Harness, forward| {
        let letter = h.app.line_mode.jump_to_adjacent_mark(forward);
        (letter, text_rows(h, height)[height / 2].clone())
    };
    assert_eq!(walk(&mut h, false), (Some('b'), "b line 39".to_string()));
    assert_eq!(walk(&mut h, false), (Some('a'), "a line 19".to_string()));
    assert_eq!(walk(&mut h, false), (None, "a line 19".to_string()));
    assert_eq!(walk(&mut h, true), (Some('b'), "b line 39".to_string()));

    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    let line = |h: &Harness| h.app.line_mode.copy_state().cursor.line_idx;
    assert_eq!(h.app.line_mode.jump_to_adjacent_mark(false), Some('b'));
    assert_eq!(line(&h), 39);
    assert_eq!(h.app.line_mode.jump_to_adjacent_mark(false), Some('a'));
    assert_eq!(line(&h), 19);
    assert_eq!(h.app.line_mode.jump_to_adjacent_mark(true), Some('b'));
    assert_eq!(h.app.line_mode.jump_to_adjacent_mark(true), None);
    assert_eq!(line(&h), 39);
}

/// Terminal mode, 60 numbered lines, mark a on "line 19" and b on "line 39" as the cursor
/// reaches them, the live view drawn: the harness and the history's rows count (MAX-164)
fn terminal_marked_lines() -> (Harness, usize) {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 40, 12, |s| {
        s.mode = Mode::Terminal;
    });
    h.draw();
    for i in 0..60 {
        let sep = if i == 0 { "" } else { "\r\n" };
        let line = format!("{sep}line {i:02}");
        h.app
            .terminal_mode
            .process_rx(TextEncoding::Utf8, line.as_bytes());
        if matches!(i, 19 | 39) {
            let letter = if i == 19 { 'a' } else { 'b' };
            assert!(h.app.terminal_mode.add_mark(letter).is_some());
        }
    }
    h.draw();
    let height = h.app.frame_areas().content.height as usize;
    (h, height)
}

/// The terminal view's middle row: a jump centers its row there (MAX-164)
fn terminal_middle_row(h: &mut Harness, height: usize) -> String {
    let rows = text_rows(h, height);
    rows[(height - 1) / 2].clone()
}

/// MAX-164: in terminal mode, out of copy mode, a mark jump only scrolls: the mark's row on
/// the view's middle row, no copy mode; an unset mark moves nothing.
/// How: from the live view, jump to a, then to b, then to the unset z.
#[test]
fn terminal_mark_jump_centers_the_view() {
    let (mut h, height) = terminal_marked_lines();
    assert!(h.app.terminal_mode.jump_to_mark('a').is_some());
    assert_eq!(terminal_middle_row(&mut h, height), "line 19");
    assert!(!h.app.is_copy_mode_active());

    h.app.terminal_mode.jump_to_mark('b');
    assert_eq!(terminal_middle_row(&mut h, height), "line 39");

    let rows = text_rows(&mut h, height);
    assert_eq!(h.app.terminal_mode.jump_to_mark('z'), None);
    assert_eq!(text_rows(&mut h, height), rows);
}

/// MAX-164: in terminal copy mode a mark jump moves the copy cursor to the start of the mark's
/// row, which the view shows centered.
/// How: copy mode from the live view, jump to a.
#[test]
fn terminal_mark_jump_moves_the_copy_cursor() {
    let (mut h, height) = terminal_marked_lines();
    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    h.draw();
    let a = h.app.terminal_mode.jump_to_mark('a').unwrap();
    let cursor = h.app.terminal_mode.copy_state().cursor;
    assert_eq!((cursor.line_idx, cursor.char_idx), (a, 0));
    assert_eq!(terminal_middle_row(&mut h, height), "line 19");
}

/// MAX-164: in terminal mode previous / next mark walk the marks from the view's middle row,
/// or from the copy cursor in copy mode; past the last one nothing moves.
/// How: from the live view previous twice, once more, then next; copy mode from the live
/// view, previous twice, next twice.
#[test]
fn terminal_previous_and_next_mark() {
    let (mut h, height) = terminal_marked_lines();
    let walk = |h: &mut Harness, forward| {
        let letter = h.app.terminal_mode.jump_to_adjacent_mark(forward);
        (letter, terminal_middle_row(h, height))
    };
    assert_eq!(walk(&mut h, false), (Some('b'), "line 39".to_string()));
    assert_eq!(walk(&mut h, false), (Some('a'), "line 19".to_string()));
    assert_eq!(walk(&mut h, false), (None, "line 19".to_string()));
    assert_eq!(walk(&mut h, true), (Some('b'), "line 39".to_string()));

    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    let marks = h.app.terminal_mode.marks().clone();
    let line = |h: &Harness| h.app.terminal_mode.copy_state().cursor.line_idx;
    assert_eq!(h.app.terminal_mode.jump_to_adjacent_mark(false), Some('b'));
    assert_eq!(Some(line(&h)), marks.get('b'));
    assert_eq!(h.app.terminal_mode.jump_to_adjacent_mark(false), Some('a'));
    assert_eq!(Some(line(&h)), marks.get('a'));
    assert_eq!(h.app.terminal_mode.jump_to_adjacent_mark(true), Some('b'));
    assert_eq!(h.app.terminal_mode.jump_to_adjacent_mark(true), None);
    assert_eq!(Some(line(&h)), marks.get('b'));
}

/// MAX-164: from a scrolled view (no copy mode) a mark goes on the line on the view's middle
/// row, what is looked at, not on the last line received.
/// How: the marked lines scrolled up 20 rows, mark c, then the line on the middle row.
#[test]
fn mark_from_a_scrolled_view_takes_its_middle_line() {
    let (mut h, height) = marked_lines();
    h.scroll(HistoryAction::ScrollUp(20));
    let middle = text_rows(&mut h, height)[height / 2].clone();
    let line = h.app.line_mode.add_mark('c').unwrap();
    assert_eq!(middle, format!("│ line {line:02}"));
    assert!(line < 59, "{line}");
}

/// MAX-164: in terminal mode, from a scrolled view (no copy mode) a mark goes on the row on
/// the view's middle row, not on the terminal cursor's.
/// How: the marked lines scrolled up 20 rows, mark c, then jump back to it from the live view.
#[test]
fn terminal_mark_from_a_scrolled_view_takes_its_middle_row() {
    let (mut h, height) = terminal_marked_lines();
    h.scroll(HistoryAction::ScrollUp(20));
    let middle = terminal_middle_row(&mut h, height);
    assert!(h.app.terminal_mode.add_mark('c').is_some());
    h.scroll(HistoryAction::ScrollToBottom);
    h.draw();
    h.app.terminal_mode.jump_to_mark('c');
    assert_eq!(terminal_middle_row(&mut h, height), middle);
    assert_ne!(middle, "line 59");
}

/// MAX-164: the marks popup lists the marks by line, each with its letter, its line's time
/// and text, the buttons in its bottom border; a click on another row selects it, a second
/// click jumps there: the popup closes, the line centered. Empty, it says how to add one.
/// How: the marked lines (a on 19, b on 39, both received now), the popup opened by its
/// action; then a fresh harness.
#[test]
fn marks_popup_lists_and_jumps() {
    let (mut h, height) = marked_lines();
    h.app.update(KeyBindingAction::Marks.to_action(&h.app));
    let rows = h.draw();
    let time = chrono::Local::now().format("%H:%M").to_string();
    let row_a = rows.iter().find(|row| row.contains("a  ")).cloned();
    let row_a = row_a.expect("a row for a");
    assert!(
        row_a.contains("line 19") && row_a.contains(&time),
        "{rows:#?}"
    );
    assert!(has(&rows, "line 39"), "{rows:#?}");
    assert!(has(&rows, "Enter Jump · Delete Remove"), "{rows:#?}");

    click(&mut h, cell_of(&rows, "line 39"));
    assert!(h.app.overlay().is_some());
    click(&mut h, cell_of(&rows, "line 39"));
    assert!(h.app.overlay().is_none());
    assert_eq!(text_rows(&mut h, height)[height / 2], "b line 39");

    let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
    h.app.update(KeyBindingAction::Marks.to_action(&h.app));
    let rows = h.draw();
    assert!(
        has(&rows, "No mark yet: Ctrl-a , then a letter marks a line"),
        "{rows:#?}"
    );
}

/// MAX-164: the right-click menu has "Add mark" on a scrolled view or in copy mode only: it
/// marks the line clicked with the first free letter; opened off the history, the line a mark
/// takes anyway (in copy mode the cursor's). "Marks…" is there in any view while there are
/// marks, and opens the popup.
/// How: a right-click with no mark; the marked lines (a and b taken) on the live view, then
/// scrolled up on a history row; then in copy mode on the status bar.
#[test]
fn right_click_menu_adds_a_mark() {
    let right_click = |h: &mut Harness, at| {
        mouse_do(h, MouseEventKind::Down(MouseButton::Right), at);
        h.draw()
    };
    let mut none = Harness::line_mode(LineDisplayMode::Pretty, 40, 30);
    let rows = right_click(&mut none, (10, 3));
    assert!(
        !has(&rows, "Marks…") && !has(&rows, "Add mark"),
        "{rows:#?}"
    );

    let (mut h, height) = marked_lines();
    let rows = right_click(&mut h, (10, 3));
    assert!(!has(&rows, "Add mark"), "{rows:#?}");
    click(&mut h, cell_of(&rows, "Marks…"));
    assert!(matches!(h.app.overlay(), Some(Overlay::Marks(_))));
    h.app.close_overlay();

    h.scroll(HistoryAction::ScrollUp(20));
    let rows = text_rows(&mut h, height);
    let line: usize = rows[2]["│ line ".len()..].parse().unwrap();
    let rows = right_click(&mut h, (10, 2));
    assert!(has(&rows, "Marks…"), "{rows:#?}");
    click(&mut h, cell_of(&rows, "Add mark"));
    assert_eq!(h.app.line_mode.marks().get('c'), Some(line));
    let toasts = h.app.notifications.active_toasts();
    assert_eq!(toasts.last().unwrap().message, "mark c");

    let rows = right_click(&mut h, (10, 2));
    click(&mut h, cell_of(&rows, "Marks…"));
    assert!(matches!(h.app.overlay(), Some(Overlay::Marks(_))));
    h.app.close_overlay();

    h.app.update(Action::CopyMode(CopyModeAction::Enter));
    h.draw();
    let cursor = h.app.line_mode.copy_state().cursor.line_idx;
    let rows = right_click(&mut h, (5, 11));
    click(&mut h, cell_of(&rows, "Add mark"));
    assert_eq!(h.app.line_mode.marks().get('d'), Some(cursor));
}

/// `prefix_rows`' lines with mark b on "short" and a on the wrapped line (MAX-164)
fn marked_prefix_rows(tweak: impl FnOnce(&mut SettingsConfig)) -> Vec<String> {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 40, 12, |s| {
        s.line_mode.prefix_place = crate::configs::settings::PrefixPlace::Border;
        tweak(s);
    });
    h.feed(b"short\r\n");
    h.feed(format!("{}\r\n", "word ".repeat(10)).as_bytes());
    h.feed(b"part");
    h.app.line_mode.add_mark_at('b', 0);
    h.app.line_mode.add_mark_at('a', 1);
    h.draw()
}

/// MAX-164: a mark's letter goes where the rows' markers are, next to the text: on the
/// prefix's first column (the bar after the timestamp, or the inset prefix's), with borders
/// too; else the left border's cell. A wrapped row keeps its `┆`; with neither, no letter,
/// the text is never covered. `[ui.marks] letters = false` shows none.
/// How: the prefix rows with marks, in each place.
#[test]
fn mark_letters_in_the_left_column() {
    let rows = marked_prefix_rows(|s| s.ui.show_borders = true);
    let short = rows.iter().position(|row| row.contains("short")).unwrap();
    assert!(rows[short].starts_with("bshort"), "{rows:#?}");
    assert!(rows[short + 1].starts_with("aword word"), "{rows:#?}");
    assert!(rows[short + 2].starts_with("┆word"), "{rows:#?}");

    let rows = marked_prefix_rows(|_| {});
    let short = rows.iter().position(|row| row.contains("short")).unwrap();
    assert!(rows[short].starts_with("short"), "{rows:#?}");
    assert!(rows[short + 1].starts_with("word word"), "{rows:#?}");

    let rows = marked_prefix_rows(|s| {
        s.show_timestamps = true;
        s.timestamp_format = String::from("%M:%S");
        s.refresh_timestamp_width();
    });
    let short = rows.iter().position(|row| row.contains("short")).unwrap();
    let after_time = |row: usize| rows[row].chars().skip(5).collect::<String>();
    assert!(after_time(short + 1).starts_with("aword"), "{rows:#?}");
    assert!(after_time(short + 2).starts_with("┆word"), "{rows:#?}");

    // with borders and timestamps: on the bar after the time, the border stays plain
    let rows = marked_prefix_rows(|s| {
        s.ui.show_borders = true;
        s.show_timestamps = true;
        s.timestamp_format = String::from("%M:%S");
        s.refresh_timestamp_width();
    });
    let short = rows.iter().position(|row| row.contains("short")).unwrap();
    assert!(rows[short + 1].starts_with('│'), "{rows:#?}");
    let after_time = |row: usize| rows[row].chars().skip(6).collect::<String>();
    assert!(after_time(short + 1).starts_with("aword"), "{rows:#?}");

    let rows = marked_prefix_rows(|s| {
        s.line_mode.prefix_place = crate::configs::settings::PrefixPlace::Inset;
        s.line_mode.inset_prefix = crate::configs::settings::InsetPrefix::default();
    });
    let short = rows.iter().position(|row| row.contains("short")).unwrap();
    assert!(rows[short + 1].starts_with("a word"), "{rows:#?}");

    let rows = marked_prefix_rows(|s| {
        s.ui.show_borders = true;
        s.ui.marks.letters = false;
    });
    let short = rows.iter().position(|row| row.contains("short")).unwrap();
    assert!(rows[short + 1].starts_with("│word word"), "{rows:#?}");
}

/// MAX-164: in terminal mode a mark's letter goes on the bar after the row's time, with
/// borders too; without timestamps in the left border's cell; with neither, none.
/// How: three lines, b on the first, a on the cursor's; with borders, with timestamps, both.
#[test]
fn terminal_mark_letters_in_the_left_column() {
    let rows = |tweak: fn(&mut SettingsConfig)| {
        let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 40, 12, |s| {
            s.mode = Mode::Terminal;
            tweak(s);
        });
        h.draw();
        h.app
            .terminal_mode
            .process_rx(TextEncoding::Utf8, b"one\r\ntwo\r\nthree");
        h.app.terminal_mode.add_mark('a');
        let one = h.app.terminal_mode.marks().get('a').unwrap() - 2;
        h.app.terminal_mode.add_mark_at('b', one);
        h.draw()
    };
    let shown = rows(|s| s.ui.show_borders = true);
    let one = shown.iter().position(|row| row.contains("one")).unwrap();
    assert!(shown[one].starts_with("bone"), "{shown:#?}");
    assert!(shown[one + 1].starts_with("│two"), "{shown:#?}");
    assert!(shown[one + 2].starts_with("athree"), "{shown:#?}");

    let shown = rows(|s| {
        s.show_timestamps = true;
        s.timestamp_format = String::from("%M:%S");
        s.refresh_timestamp_width();
    });
    let one = shown.iter().position(|row| row.contains("one")).unwrap();
    let after_time = |row: usize| shown[row].chars().skip(5).collect::<String>();
    assert!(after_time(one).starts_with("bone"), "{shown:#?}");
    assert!(after_time(one + 1).starts_with("│two"), "{shown:#?}");

    let shown = rows(|s| {
        s.ui.show_borders = true;
        s.show_timestamps = true;
        s.timestamp_format = String::from("%M:%S");
        s.refresh_timestamp_width();
    });
    let one = shown.iter().position(|row| row.contains("one")).unwrap();
    assert!(shown[one].starts_with('│'), "{shown:#?}");
    let after_time = |row: usize| shown[row].chars().skip(6).collect::<String>();
    assert!(after_time(one).starts_with("bone"), "{shown:#?}");

    let shown = rows(|_| {});
    let one = shown.iter().position(|row| row.contains("one")).unwrap();
    assert!(shown[one].starts_with("one"), "{shown:#?}");
}

/// MAX-164: a marked line taller than the view still shows its letter: on its first row shown,
/// the top wrapped row when its first one is above the view; the rows under keep their `┆`.
/// How: a line of 200 words marked from the live view, with borders: the view shows its last
/// rows only.
#[test]
fn mark_letter_on_the_first_row_shown() {
    let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 40, 12, |s| {
        s.line_mode.prefix_place = crate::configs::settings::PrefixPlace::Border;
        s.ui.show_borders = true;
    });
    h.feed(format!("{}\r\n", "word ".repeat(200)).as_bytes());
    h.draw();
    assert_eq!(h.app.line_mode.add_mark('a'), Some(0));
    let rows = h.draw();
    let top = h.app.frame_areas().content.y as usize;
    assert!(rows[top].starts_with("aword"), "{rows:#?}");
    assert!(rows[top + 1].starts_with("┆word"), "{rows:#?}");
    assert!(
        !rows.iter().any(|row| row.starts_with("│word")),
        "{rows:#?}"
    );
}

/// The letters on the history's scrollbar and their row in its track, top down (MAX-164)
fn scrollbar_letters(h: &mut Harness) -> Vec<(usize, char)> {
    let rows = h.draw();
    let area = h.app.frame_map().scrollbar.expect("a scrollbar").area;
    (1..area.height as usize - 1)
        .filter_map(|row| {
            let y = area.y as usize + row;
            let c = rows[y].chars().nth(area.x as usize)?;
            c.is_ascii_lowercase().then_some((row - 1, c))
        })
        .collect()
}

/// MAX-164: each mark shows as its letter on the scrollbar, where its line is in the whole
/// history, in both modes; `[ui.marks] scrollbar = false` shows none.
/// On the thumb a letter has the thumb's color behind it, no hole in the thumb.
/// How: the marked lines (a on line 19, b on 39, of 60 one-row lines): a about a third down
/// the track, b two thirds; b on the thumb once jumped to; then the switch off; then terminal
/// mode's.
#[test]
fn scrollbar_shows_the_marks() {
    let (mut h, _) = marked_lines();
    let track = h.app.frame_map().scrollbar.unwrap().track();
    assert_eq!(
        scrollbar_letters(&mut h),
        [('a', 19 * track / 60), ('b', 39 * track / 60)].map(|(c, r)| (r, c))
    );
    // on the thumb, in the thumb's color: b's line is in the view once jumped to
    h.app.line_mode.jump_to_mark('b');
    h.draw();
    let area = h.app.frame_map().scrollbar.unwrap().area;
    let (first, length) = h.app.frame_map().scrollbar.unwrap().thumb();
    let b_row = scrollbar_letters(&mut h)[1].0;
    assert!(
        (first..first + length).contains(&b_row),
        "{first} {length} {b_row}"
    );
    let cell = &h.terminal.backend().buffer()[(area.x, area.y + 1 + b_row as u16)];
    let thumb = ThemeConfig::default().ui.scrollbar_thumb.fg;
    assert_eq!(cell.style().bg, thumb);

    h.app.settings_mut().ui.marks.scrollbar = false;
    assert_eq!(scrollbar_letters(&mut h), []);

    let (mut h, _) = terminal_marked_lines();
    let letters = scrollbar_letters(&mut h);
    assert_eq!(letters.iter().map(|&(_, c)| c).collect::<String>(), "ab");
    assert!(letters[0].0 < letters[1].0, "{letters:?}");
}

/// MAX-164: the line a mark jump lands on flashes, in `ui.flash`'s colors, its rows only, in
/// both modes, the tick asking for frames meanwhile; `[ui.marks] flash = false` doesn't.
/// How: a jump to a in the marked lines, the middle row's cells and the next row's; then
/// terminal mode's; then the switch off.
#[test]
fn mark_jump_flashes_its_line() {
    use crate::inputs::MarkAction;
    let flash_bg = ThemeConfig::default().ui.flash.bg;
    let bg_at = |h: &Harness, x: u16, y: u16| h.terminal.backend().buffer()[(x, y)].style().bg;

    let (mut h, height) = marked_lines();
    h.app
        .update(Action::App(AppAction::Mark(MarkAction::Jump('a'))));
    let rows = h.draw();
    let content = h.app.frame_areas().content;
    let middle = content.y + (height / 2) as u16;
    assert!(rows[middle as usize].contains("line 19"), "{rows:#?}");
    assert_eq!(bg_at(&h, content.x + 3, middle), flash_bg);
    assert_ne!(bg_at(&h, content.x + 3, middle + 1), flash_bg);
    assert!(h.app.tick());

    let (mut h, height) = terminal_marked_lines();
    h.app
        .update(Action::App(AppAction::Mark(MarkAction::Jump('a'))));
    let rows = h.draw();
    let content = h.app.frame_areas().content;
    let middle = content.y + ((height - 1) / 2) as u16;
    assert!(rows[middle as usize].contains("line 19"), "{rows:#?}");
    assert_eq!(bg_at(&h, content.x + 3, middle), flash_bg);
    assert_ne!(bg_at(&h, content.x + 3, middle + 1), flash_bg);

    let (mut h, height) = marked_lines();
    h.app.settings_mut().ui.marks.flash = false;
    h.app
        .update(Action::App(AppAction::Mark(MarkAction::Jump('a'))));
    h.draw();
    let content = h.app.frame_areas().content;
    let middle = content.y + (height / 2) as u16;
    assert_ne!(bg_at(&h, content.x + 3, middle), flash_bg);
}
