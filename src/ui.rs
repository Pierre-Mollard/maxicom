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
    widgets::{Block, Borders},
};

use crate::{
    app::{App, types::ViewportSize},
    configs::{KeywordHighlight, Mode, ThemeConfig},
    overlays::Overlay,
    ui::{
        common::{border_style_for, monitor_bottom_title, monitor_title},
        layout::{LayoutWarning, ScreenLayout},
        lines::{render_input_prompt, render_monitor},
        popups::{
            alert::{render_detached_popup, render_disconnected_popup, render_error_popup},
            filepath::render_filepath_popup,
            help::render_help_popup,
            notifications::render_notifications,
            reconfigure::render_reconfigure_popup,
            resize::render_resize_popup,
            settings::render_settings_popup,
        },
        status_bar::{CompiledStatusBar, render_status_bar},
        terminal::{render_timestamp_gutter, render_vt100_terminal},
    },
};

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

    let layout = ScreenLayout::compute(root_area, app);
    app.update_viewport(ViewportSize::new(
        layout.content_area.width,
        layout.content_area.height,
    ));

    if app.settings().ui.show_borders {
        let border_style = border_style_for(
            layout.focus_target,
            app.focus_target(),
            app.is_app_focused(),
            theme,
        );

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .style(*theme.ui.background)
            .title(monitor_title(app, &layout.monitor_outer));

        for bottom_title in monitor_bottom_title(app, theme) {
            block = block.title_bottom(bottom_title);
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
        render_status_bar(app, frame, status_rect, theme, status_bar);
    }

    render_active_overlay(app, frame, theme);

    render_notifications(&app.notifications, frame, theme);

    if !layout.warnings.is_empty() {
        PostRenderEffect::LayoutWarnings(layout.warnings)
    } else {
        post_effect
    }
}

fn render_active_overlay(app: &App, frame: &mut Frame, theme: &ThemeConfig) {
    match app.overlay() {
        Some(Overlay::Settings) => render_settings_popup(app, frame, theme),
        Some(Overlay::Help(dialog)) => render_help_popup(app, dialog, frame, theme),
        Some(Overlay::TerminalSize(dialog)) => render_resize_popup(dialog, frame, theme),
        Some(Overlay::SerialSettings(dialog)) => render_reconfigure_popup(dialog, frame, theme),
        Some(Overlay::FilePath(dialog)) => render_filepath_popup(dialog, frame, theme),
        Some(Overlay::DeviceDisconnected(port)) => render_disconnected_popup(port, frame, theme),
        Some(Overlay::DeviceDetached(port)) => render_detached_popup(port, frame, theme),
        Some(Overlay::Error(err)) => render_error_popup(err, frame, theme),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    //! Whole-frame rendering through `render`, on a test backend.

    use super::*;
    use crate::{
        app::UpdateEffect,
        configs::{
            KeyBindingsConfig, KeyResolver, LineDisplayMode, RawSideView, RxNewlineMode,
            SettingsConfig, TextEncoding,
        },
        inputs::{Action, HistoryAction},
        serial::SerialConfig,
    };
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
            let mut settings = SettingsConfig::default();
            settings.mode = Mode::Line;
            settings.show_timestamps = false;
            settings.line_mode.display_mode = display_mode;
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

    /// Screen rows showing Binary data (at least one 8 digit byte), as drawn.
    fn binary_rows(rows: &[String]) -> Vec<String> {
        let is_byte = |word: &str| word.len() == 8 && word.bytes().all(|b| b == b'0' || b == b'1');
        rows.iter()
            .filter(|row| row.split(|c: char| !c.is_ascii_digit()).any(is_byte))
            .cloned()
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
}
