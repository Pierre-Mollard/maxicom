/// View, render screen from state (app)
/// Using the 'ratatui' crate
pub mod common;
pub mod data_display;
pub mod layout;
pub mod lines;
pub mod popups;
pub mod raw_side;
pub mod status_bar;
pub mod terminal;

use ratatui::{
    Frame,
    layout::Rect,
    text::Line,
    widgets::{Block, Borders},
};

use crate::{
    app::{
        App,
        types::{ClickAction, ClickTarget, FrameAreas, ViewportSize},
    },
    configs::{KeyBindingAction, KeywordHighlight, Mode, ThemeConfig},
    overlays::{Overlay, PopupTarget},
    ui::{
        common::{border_style_for, monitor_bottom_title, monitor_title},
        layout::{LayoutWarning, ScreenLayout},
        lines::{render_input_prompt, render_monitor},
        popups::{
            alert::{
                render_confirm_quit_popup, render_detached_popup, render_disconnected_popup,
                render_error_popup,
            },
            filepath::render_filepath_popup,
            help::render_help_popup,
            menu::render_action_menu,
            notifications::render_notifications,
            reconfigure::render_reconfigure_popup,
            resize::render_resize_popup,
            settings::render_settings_popup,
        },
        status_bar::{CompiledStatusBar, render_status_bar},
        terminal::{render_timestamp_gutter, render_vt100_terminal},
    },
};

/// The buttons in the history border's top-right, with what a click runs: for the mouse,
/// with the status bar hidden too (MAX-128). Plain glyphs every font has
const BORDER_BUTTONS: [(&str, KeyBindingAction); 3] = [
    ("[?]", KeyBindingAction::Help),
    ("[≡]", KeyBindingAction::Settings),
    ("[×]", KeyBindingAction::Quit),
];

#[derive(Debug, PartialEq, Eq)]
pub enum PostRenderEffect {
    None,
    LayoutWarnings(Vec<LayoutWarning>),
    _CopyToClipboard(String),
}

pub fn render(
    app: &mut App,
    theme: &ThemeConfig,
    frame: &mut Frame,
    highlights: &[KeywordHighlight],
    status_bar: &CompiledStatusBar,
) -> PostRenderEffect {
    let root_area = frame.area();
    let root_backdrop = Block::default().style(*theme.ui.background);
    frame.render_widget(root_backdrop, root_area);

    app.set_screen_size(root_area.width, root_area.height);
    let layout = ScreenLayout::compute(root_area, app);
    app.set_frame_areas(FrameAreas {
        content: layout.content_area,
        raw_side: layout.raw_side_area,
    });
    app.frame_map_mut().clear();
    app.update_viewport(ViewportSize::new(
        layout.content_area.width,
        layout.content_area.height,
    ));

    if app.show_borders() {
        let border_style = border_style_for(
            layout.focus_target,
            app.focus_target(),
            app.is_app_focused(),
            theme,
        );

        // the title gets the top border between its corners, but for the buttons and a dash
        // before them (MAX-137)
        let outer = layout.monitor_outer;
        let buttons: String = BORDER_BUTTONS.iter().map(|(text, _)| *text).collect();
        let buttons_width = buttons.chars().count() as u16;
        let room = outer.width.saturating_sub(3 + buttons_width);
        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .style(*theme.ui.background)
            .title(monitor_title(app, &outer, room));

        // the pan shows once: in the status bar when it is drawn with a pan variable, else here;
        // and in terminal mode the bar can take this bottom border (MAX-135, MAX-139)
        let bar_in_border = layout
            .status_bar_area
            .is_some_and(|bar| bar.y + 1 == outer.bottom());
        let pan_in_bar = layout.status_bar_area.is_some() && status_bar.shows_pan();
        if !bar_in_border && !pan_in_bar {
            for bottom_title in monitor_bottom_title(app, theme) {
                block = block.title_bottom(bottom_title);
            }
        }

        // right-aligned in the top border, inside its corner
        let mut x = outer.right().saturating_sub(1 + buttons_width);
        block = block.title(Line::styled(buttons, border_style).right_aligned());
        for (text, action) in BORDER_BUTTONS {
            let width = text.chars().count() as u16;
            let cell = Rect::new(x, outer.y, width, 1).intersection(outer);
            if !cell.is_empty() {
                app.frame_map_mut().click_targets.push(ClickTarget {
                    area: cell,
                    action: ClickAction::Run(action),
                    hover: Some(*theme.ui.hover),
                });
            }
            x += width;
        }

        frame.render_widget(block, layout.monitor_outer);
    }

    let post_effect = match app.settings().mode {
        Mode::Terminal => {
            if let Some(g_area) = layout.timestamp_gutter {
                render_timestamp_gutter(app, frame, g_area, theme);
            }
            render_vt100_terminal(
                app,
                frame,
                layout.content_area,
                layout.raw_side_area,
                layout.scrollbar_area,
                theme,
                highlights,
            );
            PostRenderEffect::None
        }
        Mode::Line => render_monitor(
            app,
            frame,
            layout.content_area,
            layout.raw_side_area,
            layout.scrollbar_area,
            theme,
            highlights,
        ),
    };

    if let Some(prompt_rect) = layout.prompt_area {
        render_input_prompt(app, frame, prompt_rect, theme);
    }

    if let Some(status_rect) = layout.status_bar_area {
        let targets = render_status_bar(app, frame, status_rect, theme, status_bar);
        app.frame_map_mut().click_targets.extend(targets);
    }

    render_active_overlay(app, frame, theme);

    // the part under the pointer: of the popup when one is open, the menu showing it as its
    // selection; else a border button or search bar hint (MAX-131)
    if let Some((x, y)) = app.pointer() {
        let map = app.frame_map();
        let hovered = if map.popup.is_some() {
            map.popup_target_at(x, y)
                .filter(|(_, target)| !matches!(target, PopupTarget::MenuItem(_)))
                .map(|(area, _)| (area, *theme.ui.hover))
        } else {
            map.click_target_at(x, y)
                .and_then(|target| Some((target.area, target.hover?)))
        };
        if let Some((area, style)) = hovered {
            frame.buffer_mut().set_style(area, style);
        }
    }

    render_notifications(&app.notifications, frame, theme);

    if !layout.warnings.is_empty() {
        PostRenderEffect::LayoutWarnings(layout.warnings)
    } else {
        post_effect
    }
}

fn render_active_overlay(app: &mut App, frame: &mut Frame, theme: &ThemeConfig) {
    let mut targets = std::mem::take(&mut app.frame_map_mut().popup_targets);
    let resolver = &app.key_resolver;
    let area = match app.overlay() {
        Some(Overlay::Settings) => render_settings_popup(app, frame, theme, &mut targets),
        Some(Overlay::Help(dialog)) => render_help_popup(app, dialog, frame, theme, &mut targets),
        Some(Overlay::TerminalSize(dialog)) => {
            render_resize_popup(dialog, resolver, frame, theme, &mut targets)
        }
        Some(Overlay::SerialSettings(dialog)) => {
            render_reconfigure_popup(dialog, resolver, frame, theme, &mut targets)
        }
        Some(Overlay::FilePath(dialog)) => {
            render_filepath_popup(dialog, resolver, frame, theme, &mut targets)
        }
        Some(Overlay::DeviceDisconnected(port)) => {
            render_disconnected_popup(port, resolver, frame, theme, &mut targets)
        }
        Some(Overlay::DeviceDetached(port)) => {
            render_detached_popup(port, resolver, frame, theme, &mut targets)
        }
        Some(Overlay::Error(err)) => render_error_popup(err, resolver, frame, theme, &mut targets),
        Some(Overlay::ConfirmQuit) => {
            render_confirm_quit_popup(resolver, frame, theme, &mut targets)
        }
        Some(Overlay::ActionMenu(menu)) => {
            render_action_menu(menu, resolver, frame, theme, &mut targets)
        }
        None => return,
    };
    let map = app.frame_map_mut();
    map.popup = Some(area);
    map.popup_targets = targets;
}

#[cfg(test)]
mod tests {
    //! Whole-frame rendering through `render`, on a test backend.

    use super::*;
    use crate::{
        app::{UpdateEffect, selection::CopySide},
        configs::{
            KeyBindingsConfig, KeyResolver, LineDisplayMode, RawSideView, RxNewlineMode,
            SettingsConfig, TextEncoding,
        },
        inputs::{
            Action, AppAction, CopyModeAction, DialogAction, HistoryAction, SearchModeAction,
        },
        overlays::{ErrorDialog, FilePathDialog, HelpDialog, ResizeDialog, SerialConfigDialog},
        serial::SerialConfig,
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
            // the rows the tests expect have no border, whatever the default, and their small
            // screens aren't compact (MAX-136)
            settings.ui.show_borders = false;
            settings.ui.compact_cols = 0;
            settings.ui.compact_rows = 0;
            settings.line_mode.display_mode = display_mode;
            tweak(&mut settings);
            let theme = ThemeConfig::default();
            let status_bar = CompiledStatusBar::new(&settings.ui.status_bar, &theme);
            let app = App::new(
                settings,
                KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap(),
                SerialConfig::default(),
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
            self.app
                .line_mode
                .feed_rx(RxNewlineMode::Auto, encoding, "%H", bytes);
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
    /// raw side covers its bytes, the ones the text does not show included.
    /// How: green "ok" and "abc", Hex panel. Copy mode on "abc": ghost on 61. Raw side, up to
    /// 1B: ghost on the "o" of "ok". Select 1B to 6D (the 5 bytes of ESC[32m): selected, 6F not
    #[test]
    fn copy_ghosts_and_raw_selection() {
        let ghost = h_theme().ui.cursor_ghost.bg;
        let selection = h_theme().ui.selection.bg;
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
        let serial = || Overlay::SerialSettings(SerialConfigDialog::new(&SerialConfig::default()));

        let rows = popup_rows(serial(), &[]);
        assert!(has(&rows, "Enter Apply · Esc Cancel"), "{rows:#?}");

        let rows = popup_rows(serial(), &[("x", "close"), ("ctrl-s", "confirm")]);
        assert!(has(&rows, "Ctrl-s Apply · x Cancel"), "{rows:#?}");

        let rows = popup_rows(Overlay::Help(HelpDialog::new()), &[]);
        assert!(
            has(&rows, "Left/Right Category · Up/Down Move · Esc Close"),
            "{rows:#?}"
        );

        let rows = popup_rows(Overlay::Settings, &[("x", "close")]);
        assert!(has(&rows, "x Return"), "{rows:#?}");
    }

    /// A text input popup types the keys that are not Esc, Enter, Tab or modified, so they
    /// are not hints there.
    /// How: Cancel bound to `q` only, then to `q` and Ctrl-q (`q` comes first in the order).
    #[test]
    fn text_input_popups_hint_only_keys_they_route() {
        let file = || Overlay::FilePath(FilePathDialog::new("log.txt"));
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
        let rows = popup_rows(Overlay::DeviceDisconnected("/dev/x".into()), &[]);
        assert!(has(&rows, "Esc/Enter Quit"), "{rows:#?}");
        assert!(!has(&rows, "Ctrl"), "{rows:#?}");

        // a long port path wraps instead of being cut
        let port = "/dev/serial/by-id/usb-FTDI_FT232R_USB_UART_A50285BI-if00-port0";
        let rows = popup_rows(Overlay::DeviceDisconnected(port.into()), &[]);
        assert!(has(&rows, "if00-port0"), "{rows:#?}");
        assert!(has(&rows, "Esc/Enter Quit"), "{rows:#?}");

        let rows = popup_rows(Overlay::DeviceDetached("/dev/x".into()), &[]);
        assert!(has(&rows, "Enter Reattach · Esc Quit"), "{rows:#?}");

        let error = ErrorDialog::new("Oops", "it broke");
        let rows = popup_rows(Overlay::Error(error), &[("x", "close")]);
        assert!(has(&rows, "x Dismiss"), "{rows:#?}");
    }

    /// The help popup has a tab for the popup bindings, the last one (MAX-73).
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
        assert!(bound_to_close(&rows, "Esc"), "{rows:#?}");
        assert!(bound_to_close(&rows, "Ctrl-x"), "{rows:#?}");
        assert!(has(&rows, "-> cycle_next"), "{rows:#?}");

        // the letters come last, below the view
        h.app.update(Action::Overlay(DialogAction::End));
        let rows = h.draw();
        assert!(bound_to_close(&rows, "q"), "{rows:#?}");
    }

    /// Rows inside the popup frame (between its top and bottom borders), without the borders
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
            Overlay::Settings,
            Overlay::SerialSettings(SerialConfigDialog::new(&SerialConfig::default())),
            Overlay::FilePath(FilePathDialog::new("log.txt")),
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
        let dialog = SerialConfigDialog::new(&SerialConfig::default());
        let rows = popup_rows(Overlay::SerialSettings(dialog), &[]);
        assert!(has(&rows, "< 8 >"), "{rows:#?}");
        assert!(has(&rows, "< 1 >"), "{rows:#?}");
    }

    /// The text cursor of the text input popups sits right after the typed text.
    #[test]
    fn text_input_cursor_follows_the_text() {
        let popups = [
            (Overlay::FilePath(FilePathDialog::new("log.txt")), "log.txt"),
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
    /// Shift+wheel pans the panel. In copy mode it moves the copy cursor (the view follows
    /// it), and in a popup it moves up and down (MAX-113).
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
        let wheel =
            |h: &Harness, kind, at, modifiers| handle_event(mouse(kind, at, modifiers), &h.app);

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
            Action::CopyMode(CopyModeAction::MoveUp)
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
            h.feed(
                format!("line {i:02} alpha beta gamma delta epsilon zeta eta theta\r\n").as_bytes(),
            );
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

        // a key moves the cursor again as before: the view follows it
        h.app.update(Action::CopyMode(CopyModeAction::MoveUp));
        h.draw();
        assert!(!h.app.line_mode.is_mouse_view());
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
        let (_, full) = bar_at(68);
        for text in ["/dev/ttyUSB0", "8N1", "RX:    0 | TX:    0 | "] {
            assert!(full.contains(text), "{full:?}");
        }
        let (_, compact) = bar_at(67);
        assert!(compact.contains(" ttyUSB0 |") && !compact.contains("/dev/"));
        assert!(compact.contains("8N1"), "{compact:?}");
        let (_, without_4) = bar_at(50);
        assert!(!without_4.contains("8N1") && !without_4.contains("TX:    0 | "));
        assert!(without_4.contains("ttyUSB0") && without_4.contains("TX:"));
        let (mut h, without_3) = bar_at(30);
        assert!(!without_3.contains("ttyUSB0") && !without_3.contains("RX:"));
        assert!(without_3.contains(" LINE  115200"), "{without_3:?}");
        let baud = h.app.serial_config.baudrate.to_string();
        let rows = h.draw();
        click(&mut h, status_cell(&rows, &baud));
        assert!(matches!(h.app.overlay(), Some(Overlay::SerialSettings(_))));

        let (_, without_2) = bar_at(15);
        assert!(without_2.starts_with(" LINE ") && !without_2.contains("115200"));
        assert!(without_2.ends_with("? ≡ "), "{without_2:?}");
        let (mut h, cut) = bar_at(11);
        assert_eq!(cut, " LINE  ? ≡ ");
        let rows = h.draw();
        click(&mut h, status_cell(&rows, "≡"));
        assert!(matches!(h.app.overlay(), Some(Overlay::Settings)));
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

    /// MAX-126: in terminal mode with a grid taller than the view (24 rows shown in 11), the
    /// copy cursor reaching the view's top row keeps the view; one row further centers the view
    /// on it, as before. The live view is panned into the grid, which the in-view check left out: the view
    /// jumped to center the cursor.
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
        assert!(rows[0].starts_with("row 23"), "{rows:#?}");
        assert!(rows[cursor_cell(&mut h).1 as usize].starts_with("row 28"));
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

    /// Terminal mode: a click puts the copy cursor on the grid cell, a drag yanks the text.
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
        click(&mut h, (x + 3, y));
        h.draw();
        assert!(h.app.terminal_mode.copy_state().active);
        assert_eq!(cursor_cell(&mut h), (x + 3, y));

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

    /// A click in a copy view the keys centered keeps that view, and in truncate mode the pan
    /// counts: the cursor lands on the char shown under the click.
    #[test]
    fn mouse_click_keeps_a_centered_view_and_counts_the_pan() {
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

    /// A key through the input path, applied
    fn key_do(
        h: &mut Harness,
        code: crossterm::event::KeyCode,
        mods: KeyModifiers,
    ) -> UpdateEffect {
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
    /// first), and with another popup open a right-click does nothing.
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
        assert!(rows[4].contains("▶ Help"), "{:?}", rows[4]);
        click(&mut h, cell_of(&rows, "Settings"));
        assert!(matches!(h.app.overlay(), Some(Overlay::Settings)));

        right_click(&mut h, (10, 3));
        assert!(matches!(h.app.overlay(), Some(Overlay::Settings)));
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
            .update(Action::App(AppAction::OpenOverlay(Overlay::Settings)));
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
        assert!(matches!(h.app.overlay(), Some(Overlay::Settings)));
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
        assert!(!cell(&h, (x + 6, y)).modifier.contains(Modifier::REVERSED));

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
            .update(Action::App(AppAction::OpenOverlay(Overlay::Settings)));
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

        h.app.update(Action::CopyMode(CopyModeAction::Enter));
        let rows = h.draw();
        assert_eq!(
            bg_at(&h, status_cell(&rows, "LINE")),
            Some(ratatui::style::Color::Magenta)
        );
        h.app.update(Action::CopyMode(CopyModeAction::Exit));

        let rows = h.draw();
        let help = status_cell(&rows, "?");
        moved(&mut h, help);
        assert_eq!(bg_at(&h, help), Some(h.theme.palette.main));
        click(&mut h, status_cell(&rows, "≡"));
        assert!(matches!(h.app.overlay(), Some(Overlay::Settings)));
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
        assert!(matches!(h.app.overlay(), Some(Overlay::Settings)));
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
        assert!(!matches!(h.app.overlay(), Some(Overlay::Settings)));

        h.app.update(Action::Search(SearchModeAction::Cancel));
        let rows = h.draw();
        assert!(rows[23].starts_with("└─ TERM "), "{:?}", rows[23]);
        click(&mut h, settings);
        assert!(matches!(h.app.overlay(), Some(Overlay::Settings)));
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
        assert!(matches!(h.app.overlay(), Some(Overlay::Settings)));
        h.app.close_overlay();

        h.app.settings_mut().ui.show_borders = false;
        let box_rows = h.draw();
        h.app.settings_mut().ui.status_bar.layout = StatusBarLayout::Line;
        assert_eq!(box_rows, h.draw());
    }

    /// MAX-131: mouse moves don't cancel a pending prefix: a move over nothing (ignored) or a
    /// hover between Ctrl-A and its key still lets the key run.
    /// How: Ctrl-A, moves over the history and onto the status bar's `?`, then `s`.
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
        key_do(&mut h, Char('s'), KeyModifiers::NONE);
        assert!(matches!(h.app.overlay(), Some(Overlay::Settings)));
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
        assert!(matches!(h.app.overlay(), Some(Overlay::Settings)));

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

        h.app.update(Action::App(AppAction::OpenOverlay(
            Overlay::SerialSettings(SerialConfigDialog::new(&SerialConfig::default())),
        )));
        let rows = h.draw();
        let before = match h.app.overlay() {
            Some(Overlay::SerialSettings(d)) => d.selected_field,
            _ => unreachable!(),
        };
        moved(&mut h, cell_of(&rows, "Parity"));
        h.draw();
        match h.app.overlay() {
            Some(Overlay::SerialSettings(d)) => assert_eq!(d.selected_field, before),
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
                Some(Overlay::Settings) => "settings",
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

    /// MAX-136: a screen narrower than `compact_cols` or shorter than `compact_rows` is drawn
    /// without borders, status bar and scrollbar, the history and TX input taking their room;
    /// the settings are left as they are, so a bigger screen brings everything back. A `0`
    /// turns its side off. The border buttons aren't clickable while hidden.
    /// How: line mode with borders at 80x24, then 40x24, 80x12 and back; the thresholds at
    /// `0`; terminal mode at 40x24.
    #[test]
    fn compact_screen() {
        use ratatui::layout::Rect;
        let mut h = Harness::line_mode_with(LineDisplayMode::Pretty, 80, 24, |s| {
            s.ui.show_borders = true;
        });
        h.app.settings_mut().ui.compact_cols = 50;
        h.app.settings_mut().ui.compact_rows = 14;
        let at = |h: &mut Harness, w, rows| {
            h.terminal.backend_mut().resize(w, rows);
            let drawn = h.draw();
            drawn[0].starts_with('┌')
        };
        let content = |h: &Harness| h.app.frame_areas().content;

        assert!(at(&mut h, 80, 24));
        for (w, rows) in [(40, 24), (80, 12)] {
            assert!(!at(&mut h, w, rows), "{w}x{rows}");
            let layout = ScreenLayout::compute(Rect::new(0, 0, w, rows), &h.app);
            assert_eq!(layout.status_bar_area, None, "{w}x{rows}");
            assert_eq!(layout.scrollbar_area, None, "{w}x{rows}");
            assert_eq!(layout.prompt_area.map(|p| p.height), Some(1));
            assert_eq!(content(&h), Rect::new(0, 0, w, rows - 1), "{w}x{rows}");
        }
        let ui = &h.app.settings().ui;
        assert!(ui.show_borders && ui.show_status_bar && ui.show_scrollbar);

        // where `[≡]` is at 40 columns with borders
        at(&mut h, 40, 24);
        click(&mut h, (33, 0));
        assert!(h.app.overlay().is_none());

        assert!(at(&mut h, 80, 24));
        h.app.settings_mut().ui.compact_cols = 0;
        assert!(at(&mut h, 40, 24));
        assert!(!at(&mut h, 80, 12));
        h.app.settings_mut().ui.compact_rows = 0;
        assert!(at(&mut h, 40, 12));

        let ui = &mut h.app.settings_mut().ui;
        ui.compact_cols = 50;
        ui.compact_rows = 14;
        h.app.settings_mut().mode = Mode::Terminal;
        assert!(!at(&mut h, 40, 24));
        assert_eq!(content(&h), Rect::new(0, 0, 40, 24));
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
        let baud = h.app.serial_config.baudrate.to_string();
        click(&mut h, status_cell(&rows, &baud));
        assert!(matches!(h.app.overlay(), Some(Overlay::SerialSettings(_))));
        h.app.close_overlay();

        for (text, help) in [("?", true), ("≡", false)] {
            let rows = h.draw();
            click(&mut h, status_cell(&rows, text));
            match h.app.overlay() {
                Some(Overlay::Help(_)) => assert!(help, "{text}"),
                Some(Overlay::Settings) => assert!(!help, "{text}"),
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
        assert!(matches!(h.app.overlay(), Some(Overlay::Settings)));
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
        let serial = || Overlay::SerialSettings(SerialConfigDialog::new(&SerialConfig::default()));

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

        let rows = open(&mut h, Overlay::FilePath(FilePathDialog::new("x.log")));
        press(&mut h, cell_of(&rows, "Cancel"));
        assert!(h.app.overlay().is_none());

        let rows = open(&mut h, Overlay::DeviceDisconnected("/dev/x".into()));
        assert!(matches!(
            press(&mut h, cell_of(&rows, "Quit")),
            UpdateEffect::Quit
        ));

        let rows = open(&mut h, Overlay::DeviceDetached("/dev/x".into()));
        assert!(matches!(
            press(&mut h, cell_of(&rows, "Reattach")),
            UpdateEffect::ToggleAttachSerial
        ));
        // the last cell of an odd-width centered hint line: the paragraph's own centering
        let rows = open(&mut h, Overlay::DeviceDetached("/dev/x".into()));
        let (x, y) = cell_of(&rows, "Quit");
        assert!(matches!(press(&mut h, (x + 3, y)), UpdateEffect::Quit));
    }

    /// A click in a popup picks the part under it: a help tab, a setting (toggled), a serial
    /// field (a second click cycles it), a resize field. A click outside closes the popup, but
    /// not a connection alert, where closing quits (MAX-113).
    #[test]
    fn mouse_clicks_in_popups() {
        use crate::overlays::{HelpCategory, ResizeField, SerialSettingField};
        use crate::serial::Parity;
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
        let (x, y) = cell_of(&rows, "Dialogs");
        click(&mut h, (x + 6, y));
        assert!(
            matches!(h.app.overlay(), Some(Overlay::Help(d)) if d.category == HelpCategory::Dialog)
        );
        click(&mut h, (0, 0));
        assert!(h.app.overlay().is_none());

        let rows = open(&mut h, Overlay::Settings);
        assert!(!h.app.settings().local_echo);
        click(&mut h, cell_of(&rows, "Local Echo"));
        assert!(h.app.settings().local_echo);
        assert!(h.app.overlay().is_some());

        let rows = open(
            &mut h,
            Overlay::SerialSettings(SerialConfigDialog::new(&SerialConfig::default())),
        );
        let parity = cell_of(&rows, "Parity");
        click(&mut h, parity);
        click(&mut h, parity);
        match h.app.overlay() {
            Some(Overlay::SerialSettings(d)) => {
                assert_eq!(d.selected_field, SerialSettingField::Parity);
                assert_ne!(d.config.parity, Parity::None);
            }
            _ => panic!("serial popup closed"),
        }

        let rows = open(&mut h, Overlay::TerminalSize(ResizeDialog::new(24, 80)));
        click(&mut h, cell_of(&rows, "Columns"));
        assert!(
            matches!(h.app.overlay(), Some(Overlay::TerminalSize(d)) if d.active_field == ResizeField::Cols)
        );

        open(&mut h, Overlay::DeviceDisconnected("/dev/x".into()));
        click(&mut h, (0, 0));
        assert!(matches!(
            h.app.overlay(),
            Some(Overlay::DeviceDisconnected(_))
        ));
    }

    /// MAX-134: in the serial popup, a click on a field's `<` or `>` selects it and steps its
    /// value back or forth; a click elsewhere on the row still selects, then cycles.
    /// How: the parity row's `>` twice and `<` once, from the baud rate selected; then the
    /// stop bits' `<`.
    #[test]
    fn serial_popup_arrows() {
        use crate::overlays::SerialSettingField;
        use crate::serial::Parity;
        let mut h = Harness::line_mode(LineDisplayMode::Pretty, 80, 24);
        h.app.update(Action::App(AppAction::OpenOverlay(
            Overlay::SerialSettings(SerialConfigDialog::new(&SerialConfig::default())),
        )));
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
            Some(Overlay::SerialSettings(d)) => d.clone(),
            _ => panic!("serial popup closed"),
        };
        let parity = dialog(&h).config.parity;
        assert_eq!(parity, Parity::None);

        let (left, right) = arrows(&rows, "Parity");
        click(&mut h, right);
        assert_eq!(dialog(&h).selected_field, SerialSettingField::Parity);
        assert_eq!(dialog(&h).config.parity, Parity::Even);
        click(&mut h, right);
        assert_eq!(dialog(&h).config.parity, Parity::Odd);
        click(&mut h, left);
        assert_eq!(dialog(&h).config.parity, Parity::Even);

        let rows = h.draw();
        let (left, _) = arrows(&rows, "Stop Bits");
        let stop_bits = dialog(&h).config.stop_bits;
        click(&mut h, left);
        assert_eq!(dialog(&h).selected_field, SerialSettingField::StopBits);
        assert_ne!(dialog(&h).config.stop_bits, stop_bits);
    }

    /// MAX-134: in the resize popup the sideways wheel (Shift + wheel too) steps the active
    /// number, kept from 1 up, and only digits can be typed or pasted.
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
            .update(Action::Overlay(DialogAction::Paste("1a2".to_string())));
        assert_eq!(rows_value(&h), "24012");

        h.app
            .update(Action::App(AppAction::OpenOverlay(Overlay::TerminalSize(
                ResizeDialog::new(2, 80),
            ))));
        for _ in 0..3 {
            wheel(&mut h, MouseEventKind::ScrollLeft, KeyModifiers::NONE);
        }
        assert_eq!(rows_value(&h), "1");
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

        assert_eq!(sent(&mut h, press), None);
        assert!(h.app.terminal_mode.copy_state().active);
        h.app.update(Action::CopyMode(CopyModeAction::Exit));

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
}
