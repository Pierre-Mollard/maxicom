use vt100::Parser;

use crate::{
    app::{UpdateEffect, ViewportSize},
    input::TerminalAction,
};

pub struct TerminalMode {
    vt100_parser: Parser,
    scrollback_offset: usize,
    pan_offset_x: usize,
    pan_offset_y: usize,
    max_content_scrollback: usize,
    view_height: usize,
    view_width: usize,
    max_pan: usize,
}

impl TerminalMode {
    pub fn new(vt100_parser: Parser) -> Self {
        TerminalMode {
            vt100_parser,
            scrollback_offset: 0,
            pan_offset_x: 0,
            pan_offset_y: 0,
            view_height: 0,
            view_width: 0,
            max_content_scrollback: 0,
            max_pan: 0,
        }
    }

    pub fn process_rx(&mut self, bytes: &[u8]) {
        // TODO: save as raw capture stream when in interactive mode
        self.vt100_parser.process(bytes);
        if !self.vt100_parser.screen().alternate_screen() {
            self.refresh_max_scrollback();
        }
    }

    pub fn update_viewport(&mut self, viewport: ViewportSize) {
        self.view_height = viewport.rows;
        self.view_width = viewport.cols;
        self.clamp_pan_x(viewport.cols);
        self.reconcile_both_vertical_scroll();
    }

    #[inline]
    pub fn vt100_screen(&self) -> &vt100::Screen {
        self.vt100_parser.screen()
    }

    #[inline]
    pub fn pan_offset(&self) -> usize {
        self.pan_offset_x
    }

    #[inline]
    pub fn pan_offset_x(&self) -> usize {
        self.pan_offset_x
    }

    #[inline]
    pub fn pan_offset_y(&self) -> usize {
        self.pan_offset_y
    }

    #[inline]
    pub fn scrollback_offset(&self) -> usize {
        self.scrollback_offset
    }

    // for 2D terminal mode the metrics and clamping is handled within the emulator
    fn clamp_pan_x(&mut self, visible_cols: usize) {
        let vt_cols = self.vt100_parser.screen().size().1 as usize;
        self.max_pan = vt_cols.saturating_sub(visible_cols);
        self.pan_offset_x = self.pan_offset_x.min(self.max_pan);
    }

    pub fn pan_to_right(&mut self, cols: usize, visible_cols: usize) {
        self.pan_offset_x = self.pan_offset_x.saturating_add(cols);
        self.clamp_pan_x(visible_cols);
    }

    pub fn pan_to_left(&mut self, cols: usize, visible_cols: usize) {
        self.pan_offset_x = self.pan_offset_x.saturating_sub(cols);
        self.clamp_pan_x(visible_cols);
    }

    pub fn max_screen_pan_y(&self) -> usize {
        let vt_rows = self.vt100_parser.screen().size().0 as usize;
        vt_rows.saturating_sub(self.view_height)
    }

    pub fn max_vertical_scroll(&self) -> usize {
        let max_pan_y = self.max_screen_pan_y();
        if self.vt100_parser.screen().alternate_screen() {
            max_pan_y
        } else {
            max_pan_y + self.max_content_scrollback
        }
    }

    // for 2D terminal mode the metrics and clamping is handled within the emulator
    pub fn reconcile_both_vertical_scroll(&mut self) {
        let max_pan_y = self.max_screen_pan_y();
        let max_total = self.max_vertical_scroll();

        // clamping from last
        self.scrollback_offset = self.scrollback_offset.min(max_total);

        let (y_offset, vt_scrollback) = if self.scrollback_offset <= max_pan_y {
            // In the active screen: pan from bottom to top
            let y = max_pan_y - self.scrollback_offset;
            (y as u16, 0)
        } else {
            // Past the active screen: camera is at top (y=0), vt100 scrolls history
            let history_offset = self.scrollback_offset - max_pan_y;
            (0, history_offset)
        };

        self.pan_offset_y = y_offset as usize;
        self.vt100_parser.screen_mut().set_scrollback(vt_scrollback);
    }

    pub fn scroll_up(&mut self, lines: usize) {
        let max = self.max_vertical_scroll();
        self.scrollback_offset = self.scrollback_offset.saturating_add(lines).min(max);
        self.reconcile_both_vertical_scroll();
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scrollback_offset = self.scrollback_offset.saturating_sub(lines);
        self.reconcile_both_vertical_scroll();
    }

    pub fn scroll_up_page(&mut self) {
        self.scroll_up(self.view_height.saturating_sub(1));
    }

    pub fn scroll_down_page(&mut self) {
        self.scroll_down(self.view_height.saturating_sub(1));
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scrollback_offset = 0;
        self.reconcile_both_vertical_scroll();
    }

    pub fn scroll_to_top(&mut self) {
        self.scrollback_offset = self.max_vertical_scroll();
        self.reconcile_both_vertical_scroll();
    }

    /// Return Some(pan possible, max pan explored since start)
    pub fn can_pan_right(&self) -> Option<usize> {
        if self.max_pan.saturating_sub(self.pan_offset_x) > 0 {
            Some(self.max_pan.saturating_sub(self.pan_offset_x))
        } else {
            None
        }
    }

    /// Return Some(pan possible)
    pub fn can_pan_left(&self) -> Option<usize> {
        if self.pan_offset_x > 0 {
            Some(self.pan_offset_x)
        } else {
            None
        }
    }

    // HACK: there is no scrollback current len from the crate, make PR?
    pub fn refresh_max_scrollback(&mut self) {
        let current = self.vt100_parser.screen().scrollback();
        self.vt100_parser.screen_mut().set_scrollback(usize::MAX);
        self.max_content_scrollback = self.vt100_parser.screen().scrollback();
        self.vt100_parser.screen_mut().set_scrollback(current);
        self.scrollback_offset = current;
    }

    pub fn set_vt100_size(&mut self, rows: u16, cols: u16) {
        self.vt100_parser.screen_mut().set_size(rows, cols);
    }

    /// Return (rows, cols)
    pub fn get_vt100_size(&self) -> (u16, u16) {
        self.vt100_parser.screen().size()
    }

    pub fn update_action_terminal(&mut self, action: TerminalAction) -> UpdateEffect {
        match action {
            TerminalAction::AutoResizeRemoteToViewer => {
                let (rows, cols) = self.get_vt100_size();
                let cmd =
                    format!("stty rows {rows} cols {cols}; export LINES={rows} COLUMNS={cols}\n");
                UpdateEffect::Transmit(cmd.into_bytes())
            }
            TerminalAction::AutoResizeEmulatorToViewer => {
                self.set_vt100_size(self.view_height as u16, self.view_width as u16);
                UpdateEffect::None
            }
        }
    }
}
