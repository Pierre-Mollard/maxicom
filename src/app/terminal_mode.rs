use std::{
    borrow::Cow,
    collections::VecDeque,
    time::{Duration, Instant},
};

use chrono::{DateTime, Local};
use crossterm::event::KeyEvent;
use terminput::Encoding;
use terminput_crossterm::to_terminput_key;
use tui_input::Input;

use crate::{
    app::{
        UpdateEffect, ViewportSize,
        marks::Marks,
        raw_scroller::RawSideScroller,
        scroller::ViewportScroller,
        search::{SearchDirection, SearchState},
        selection::{CopyActionResult, CopyModeState, LogicalPosition, Reveal, TextSource},
        term_answers::{Answer, size_report},
    },
    capture::{Screenshot, TimedChunk},
    configs::{SettingsConfig, TerminalDimensionMode, TextEncoding, settings::RawSideView},
    inputs::{
        CopyModeAction, SearchModeAction, SideViewAction, TerminalAction, ViPending,
        click_input_at, dispatch_input_key, local_dispatchers::dispatch_copy_key_vi,
        paste_into_input,
    },
    overlays::MarkRow,
    terminal::{TerminalParser, new_parser, vt100_dim, vt100_size},
};

/// The modes a restored session may have left set, reset (MAX-148): the scroll region,
/// application cursor keys and keypad, mouse reports, bracketed paste, origin and insert modes,
/// autowrap back on, colors, and the cursor shown. The scroll region and origin mode move the
/// cursor home, so `TerminalMode::restore` puts it back after
const RESTORE_RESET: &str = concat!(
    "\x1b[r",
    "\x1b[?1l\x1b>",
    "\x1b[?9l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1005l\x1b[?1006l",
    "\x1b[?2004l",
    "\x1b[?6l\x1b[4l\x1b[?7h",
    "\x1b[0m\x1b[?25h",
);

pub struct TerminalMode {
    vt100_parser: TerminalParser,
    max_content_scrollback: usize,
    max_history_lines: usize,
    search_state: SearchState,
    copy_state: CopyModeState,
    scroller: ViewportScroller,
    raw_scroller: RawSideScroller,
    pan_offset_y: usize,
    /// The grid rows down to the cursor or the lowest row with content (MAX-140)
    live_bottom: usize,
    /// The grid columns up to the rightmost cell with content, measured when a shrink needs it
    /// and forgotten when the grid changes (MAX-140)
    content_cols: Option<u16>,
    vi_pending_copy: Option<ViPending>,
    flash_until: Option<Instant>,
    row_timestamps: VecDeque<Option<DateTime<Local>>>,
    search_text: Option<Vec<String>>,
    /// The restore separator's world row, until the grid first takes the view's size: a
    /// taller grid pulls rows back from the scrollback and would move it down (MAX-176)
    restored_separator: Option<usize>,
    /// The last byte from the device was a CR, for add CR / add LF across chunks (MAX-181)
    rx_after_cr: bool,
    /// Copy mode held the live view when rows arrived: leaving it goes back live (MAX-193)
    copy_held: bool,
    /// The marks on world rows (MAX-164)
    marks: Marks,
}

impl TerminalMode {
    pub fn new(max_history_lines: usize, rows: u16, cols: u16) -> Self {
        let vt100_parser = new_parser(rows, cols, max_history_lines);
        let search_state = SearchState::default();
        let copy_state = CopyModeState::default();
        let scroller = ViewportScroller::default();
        let raw_scroller = RawSideScroller::default();

        TerminalMode {
            vt100_parser,
            max_content_scrollback: 0,
            max_history_lines,
            search_state,
            copy_state,
            scroller,
            raw_scroller,
            pan_offset_y: 0,
            live_bottom: 1,
            content_cols: None,
            vi_pending_copy: None,
            flash_until: None,
            row_timestamps: VecDeque::from(vec![None; rows as usize]),
            search_text: None,
            restored_separator: None,
            rx_after_cr: false,
            copy_held: false,
            marks: Marks::default(),
        }
    }

    /// The device's bytes, with minicom's add CR / add LF when on (MAX-181)
    pub fn process_device_rx(
        &mut self,
        encoding: TextEncoding,
        raw_bytes: &[u8],
        add_cr: bool,
        add_lf: bool,
    ) {
        let bytes = add_newlines(raw_bytes, add_cr, add_lf, &mut self.rx_after_cr);
        self.process_rx(encoding, &bytes);
    }

    pub fn process_rx(&mut self, encoding: TextEncoding, raw_bytes: &[u8]) {
        self.process_rx_at(encoding, raw_bytes, Some(Local::now()));
    }

    /// `process_rx` for bytes received at `time`, none when it isn't known: the rows the cursor
    /// goes through get no time then (MAX-175)
    fn process_rx_at(
        &mut self,
        encoding: TextEncoding,
        raw_bytes: &[u8],
        time: Option<DateTime<Local>>,
    ) {
        self.scroller.on_rx_received();
        self.search_text = None;

        let started_on_main = !self.vt100_parser.screen().alternate_screen();
        let cursor_row_before = self.vt100_parser.screen().cursor_position().0 as usize;
        let live_rows_before = self.live_rows();

        let utf8_bytes = encoding.to_utf8_bytes(raw_bytes);
        self.vt100_parser.process(&utf8_bytes);
        let evicted = std::mem::take(&mut self.vt100_parser.callbacks_mut().evicted);
        self.update_live_bottom();
        self.content_cols = None;

        // the alternate screen has no scrollback and leaves the main one as it is: its rows are
        // synced once the remote is back on the main screen
        if self.vt100_parser.screen().alternate_screen() {
            return;
        }

        let scrollback_before = self.max_content_scrollback;
        self.refresh_max_scrollback();
        let scrollback = self.max_content_scrollback;
        let rows = self.get_vt100_size().0 as usize;
        let cursor_row = scrollback + self.vt100_parser.screen().cursor_position().0 as usize;

        let mut stamp_from = cursor_row;
        if started_on_main {
            // rows pushed into scrollback come in at the bottom, evicted ones leave at the top
            let pushed = (scrollback + evicted).saturating_sub(scrollback_before);
            self.row_timestamps
                .extend(std::iter::repeat_n(None, pushed));
            let evicted = evicted.min(self.row_timestamps.len());
            self.row_timestamps.drain(..evicted);
            self.rebase_line_refs(evicted);
            // the row the cursor started on moved up with the evicted rows
            stamp_from = (scrollback_before + cursor_row_before).saturating_sub(evicted);
        }
        // one slot per world row whatever happened above (back from the alternate screen, or a
        // case not tracked), so the range below can't go past the end
        self.row_timestamps.resize(scrollback + rows, None);

        // a view scrolled up stays on the rows it shows: its offset, counted from the live
        // view's last row, grows by the rows that came in below it (MAX-141). Copy mode holds
        // the live view too, so the selection doesn't move (MAX-193)
        let held = started_on_main && self.scroller.offset_y == 0 && self.copy_state.active;
        self.copy_held |= held;
        if started_on_main && (self.scroller.offset_y > 0 || held) {
            let added = (scrollback + self.live_rows() + evicted)
                .saturating_sub(scrollback_before + live_rows_before);
            self.scroller.offset_y += added;
            self.reconcile_both_vertical_scroll();
        }

        // every row the cursor went through gets this chunk's time, if it has none yet
        let Some(time) = time else {
            return;
        };
        let last_slot = self.row_timestamps.len().saturating_sub(1);
        let first = stamp_from.min(cursor_row).min(last_slot);
        let last = stamp_from.max(cursor_row).min(last_slot);
        for slot in self.row_timestamps.range_mut(first..=last) {
            slot.get_or_insert(time);
        }
    }

    pub fn update_viewport(
        &mut self,
        viewport: ViewportSize,
        dim_mode: TerminalDimensionMode,
        conf_rows: u16,
        conf_cols: u16,
    ) {
        self.scroller.update_view_size(viewport.cols, viewport.rows);

        let avail_h = viewport.rows as u16;
        let avail_w = viewport.cols as u16;

        if avail_h > 0 && avail_w > 0 {
            let (cur_rows, cur_cols) = self.get_vt100_size();

            match dim_mode {
                TerminalDimensionMode::GrowOnly => {
                    let target_rows = cur_rows.max(avail_h);
                    let target_cols = cur_cols.max(avail_w);
                    if (target_rows, target_cols) != (cur_rows, cur_cols) {
                        self.set_vt100_size(target_rows, target_cols);
                    }
                }
                TerminalDimensionMode::FixedCols => {
                    let target_rows = self.rows_to_keep(avail_h);
                    let target_cols = conf_cols;
                    if (target_rows, target_cols) != (cur_rows, cur_cols) {
                        self.set_vt100_size(target_rows, target_cols);
                    }
                }

                TerminalDimensionMode::MatchView => {
                    let target_rows = self.rows_to_keep(avail_h);
                    let target_cols = self.cols_to_keep(avail_w);
                    if (target_rows, target_cols) != (cur_rows, cur_cols) {
                        self.set_vt100_size(target_rows, target_cols);
                    }
                }

                TerminalDimensionMode::Fixed => {
                    if (conf_rows, conf_cols) != (cur_rows, cur_cols) {
                        self.set_vt100_size(conf_rows, conf_cols);
                    }
                }
            }
        }

        self.restore_separator_on_top();
        self.clamp_pan_x(viewport.cols);
        self.reconcile_both_vertical_scroll();
    }

    /// Once the grid has the view's size after a restore: the separator back on the top row if
    /// growing the grid pulled rows down from the scrollback, the lines after it under it
    /// (MAX-176)
    fn restore_separator_on_top(&mut self) {
        let Some(world_row) = self.restored_separator.take() else {
            return;
        };
        let Some(row) = world_row.checked_sub(self.max_content_scrollback) else {
            return;
        };
        let cursor_row = self.vt100_parser.screen().cursor_position().0;
        self.push_rows_up((row as u16).min(cursor_row));
    }

    /// The `clear` command, like a shell's but the history kept: the rows above the cursor's go
    /// up into the scrollback, the cursor's row (a prompt) on top (MAX-176)
    pub fn clear_view(&mut self) {
        self.scroll_to_bottom();
        let cursor_row = self.vt100_parser.screen().cursor_position().0;
        self.push_rows_up(cursor_row);
    }

    /// `up` rows into the scrollback, line feeds on the bottom row pushing the screen up, the
    /// cursor going up with its row. Not on the alternate screen: it has no scrollback
    fn push_rows_up(&mut self, up: u16) {
        let screen = self.vt100_parser.screen();
        let (cursor_row, cursor_col) = screen.cursor_position();
        if up == 0 || screen.alternate_screen() {
            return;
        }
        let rows = self.get_vt100_size().0;
        let cursor_time = self.cursor_row_time();
        let feeds = "\n".repeat(up as usize);
        let moves = format!(
            "\x1b[{rows};1H{feeds}\x1b[{};{}H",
            cursor_row - up + 1,
            cursor_col + 1
        );
        self.process_rx(TextEncoding::Utf8, moves.as_bytes());
        // the cursor didn't go through a row: its own keeps the time it had
        let slot =
            self.max_content_scrollback + self.vt100_parser.screen().cursor_position().0 as usize;
        if let Some(time) = self.row_timestamps.get_mut(slot) {
            *time = cursor_time;
        }
    }

    fn cursor_row_time(&self) -> Option<DateTime<Local>> {
        let row = self.vt100_parser.screen().cursor_position().0 as usize;
        self.get_timeline_timestamp(self.max_content_scrollback + row)
    }

    pub fn row_has_content(&self, row: u16) -> bool {
        let screen = self.vt100_parser.screen();
        let cols = vt100_size(screen).1;
        for c in 0..cols {
            if let Some(cell) = screen.cell(row, c)
                && cell.has_contents()
            {
                return true;
            }
        }
        false
    }

    fn rebase_line_refs(&mut self, evicted: usize) {
        if evicted == 0 {
            return;
        }
        self.search_state.rebase_after_eviction(evicted);
        self.copy_state.rebase_after_eviction(evicted);
        self.restored_separator = self
            .restored_separator
            .and_then(|row| row.checked_sub(evicted));
        self.marks.rebase_after_eviction(evicted);
    }

    pub fn total_timeline_len(&self) -> usize {
        self.row_timestamps.len()
    }

    pub fn get_timeline_timestamp(&self, global_idx: usize) -> Option<DateTime<Local>> {
        self.row_timestamps.get(global_idx).copied().flatten()
    }

    pub fn clear_history(&mut self) {
        let (rows, cols) = self.get_vt100_size();

        self.vt100_parser = new_parser(rows, cols, self.max_history_lines);
        self.row_timestamps = VecDeque::from(vec![None; rows as usize]);
        self.search_text = None;
        self.restored_separator = None;
        self.marks.clear();

        self.max_content_scrollback = 0;
        self.pan_offset_y = 0;
        self.live_bottom = 1;
        self.content_cols = None;

        self.scroller.scroll_to_bottom();
        self.scroller.max_scroll_y = 0;
        self.scroller.offset_x = 0;
        self.scroller.pan_right_explored = 0;
    }

    #[inline]
    pub fn marks(&self) -> &Marks {
        &self.marks
    }

    /// Puts mark `letter` on the copy cursor's row in copy mode, on the terminal cursor's from
    /// the live view, else on the view's middle row; returns that world row, none when the
    /// letter isn't `a`..`z` (MAX-164)
    pub fn add_mark(&mut self, letter: char) -> Option<usize> {
        let row = if self.copy_state.active {
            self.copy_state.cursor.line_idx
        } else if self.scroller.offset_y == 0 {
            self.max_content_scrollback + self.vt100_screen().cursor_position().0 as usize
        } else {
            self.view_middle_row()
        };
        self.marks.set(letter, row).then_some(row)
    }

    /// Puts mark `letter` on world row `row`, none when there's no such row (MAX-164)
    pub fn add_mark_at(&mut self, letter: char, row: usize) -> Option<usize> {
        let exists = row < self.max_content_scrollback + self.get_vt100_size().0 as usize;
        (exists && self.marks.set(letter, row)).then_some(row)
    }

    pub fn remove_mark(&mut self, letter: char) -> Option<usize> {
        self.marks.remove(letter)
    }

    /// The marks as the popup lists them, by row: each row's time and its text with its
    /// styles' escape sequences, for the popup to show as Pretty does (MAX-164)
    pub fn mark_rows(&mut self) -> Vec<MarkRow> {
        let marks = self.marks.sorted();
        marks
            .into_iter()
            .map(|(letter, row)| {
                let text = read_vt100_world_line_formatted(
                    &mut self.vt100_parser,
                    self.max_content_scrollback,
                    row,
                );
                MarkRow {
                    letter,
                    time: self.get_timeline_timestamp(row),
                    text: String::from_utf8_lossy(&text.unwrap_or_default()).into_owned(),
                }
            })
            .collect()
    }

    /// Goes to mark `letter`'s row: the copy cursor moves to its start in copy mode, else the
    /// view is centered on it. None when the mark isn't set
    pub fn jump_to_mark(&mut self, letter: char) -> Option<usize> {
        let row = self.marks.get(letter)?;
        self.go_to_row(row);
        Some(row)
    }

    /// Goes to the closest mark below (`forward`) or above the copy cursor in copy mode, else
    /// the view's middle row; returns its letter, none when there's none that way
    pub fn jump_to_adjacent_mark(&mut self, forward: bool) -> Option<char> {
        let from = if self.copy_state.active {
            self.copy_state.cursor.line_idx
        } else {
            self.view_middle_row()
        };
        let (letter, row) = if forward {
            self.marks.next(from)
        } else {
            self.marks.prev(from)
        }?;
        self.go_to_row(row);
        Some(letter)
    }

    fn view_middle_row(&self) -> usize {
        let (top, bottom) = self.visible_lines();
        (top + bottom) / 2
    }

    fn go_to_row(&mut self, row: usize) {
        if self.copy_state.active {
            let cursor = &mut self.copy_state.cursor;
            cursor.line_idx = row;
            cursor.char_idx = 0;
            cursor.preferred_col = 0;
            cursor.byte_idx = 0;
            self.reveal_copy_cursor(Reveal::Center);
        } else {
            let (width, height) = (self.scroller.view_width, self.scroller.view_height);
            self.center_on_target_match(row, 0, width, height);
        }
    }

    #[inline]
    pub fn vt100_screen(&self) -> &vt100::Screen {
        self.vt100_parser.screen()
    }

    #[inline]
    pub fn pan_offset(&self) -> usize {
        self.scroller.offset_x
    }

    #[inline]
    pub fn pan_offset_x(&self) -> usize {
        self.scroller.offset_x
    }

    #[inline]
    pub fn pan_offset_y(&self) -> usize {
        self.pan_offset_y
    }

    #[inline]
    pub fn scrollback_offset(&self) -> usize {
        self.scroller.offset_y
    }

    // for 2D terminal mode the metrics and clamping is handled within the emulator
    fn clamp_pan_x(&mut self, visible_cols: usize) {
        let vt_cols = vt100_size(self.vt100_parser.screen()).1 as usize;
        let max_pan = vt_cols.saturating_sub(visible_cols);
        self.scroller.max_pan_x = max_pan;
        self.scroller.offset_x = self.scroller.offset_x.min(max_pan);
    }

    pub fn pan_to_right(&mut self, cols: usize, visible_cols: usize) {
        self.scroller.pan_right(cols);
        self.clamp_pan_x(visible_cols);
    }

    pub fn pan_to_left(&mut self, cols: usize, visible_cols: usize) {
        self.scroller.pan_left(cols);
        self.clamp_pan_x(visible_cols);
    }

    pub fn can_pan_right(&self) -> Option<(usize, usize)> {
        self.scroller.can_pan_right()
    }

    pub fn can_pan_left(&self) -> Option<usize> {
        self.scroller.can_pan_left()
    }

    /// Updates `live_bottom` from the screen as the remote left it, whatever the view is
    /// scrolled to HACK: move scrollback to get line number then reset it
    fn update_live_bottom(&mut self) {
        let scrolled = self.vt100_parser.screen().scrollback();
        self.vt100_parser.screen_mut().set_scrollback(0);
        let rows = self.get_vt100_size().0;
        let cursor = self.vt100_parser.screen().cursor_position().0;
        let last = (cursor + 1..rows)
            .rev()
            .find(|&row| self.row_has_content(row))
            .unwrap_or(cursor);
        self.live_bottom = last as usize + 1;
        self.vt100_parser.screen_mut().set_scrollback(scrolled);
    }

    /// The rows a view of `rows` can shrink the grid to without losing any: vt100 drops the rows
    /// below the cursor first, so never fewer than the content there; the rows above the cursor
    /// go to scrollback. The grid shrinks the rest of the way once that content is gone (MAX-140)
    fn rows_to_keep(&self, rows: u16) -> u16 {
        let cursor = self.vt100_parser.screen().cursor_position().0 as usize;
        if self.live_bottom > cursor + 1 {
            rows.max(self.live_bottom as u16)
        } else {
            rows
        }
    }

    /// The columns a view of `cols` can shrink the grid to without losing any: vt100 cuts every
    /// row at the new width, so never fewer than the rightmost cell with content; the pan shows
    /// the rest (MAX-140) HACK: use scroll back then reset to get line number
    fn cols_to_keep(&mut self, cols: u16) -> u16 {
        let (rows, grid_cols) = self.get_vt100_size();
        if cols >= grid_cols {
            return cols;
        }
        if self.content_cols.is_none() {
            // the grid as the remote left it, whatever the view is scrolled to
            let scrolled = self.vt100_parser.screen().scrollback();
            self.vt100_parser.screen_mut().set_scrollback(0);
            let screen = self.vt100_parser.screen();
            let widest = (0..rows)
                .filter_map(|row| {
                    (0..grid_cols)
                        .rev()
                        .find(|&col| screen.cell(row, col).is_some_and(|c| c.has_contents()))
                })
                .max()
                .map_or(0, |col| col + 1);
            self.content_cols = Some(widest);
            self.vt100_parser.screen_mut().set_scrollback(scrolled);
        }
        cols.max(self.content_cols.unwrap_or(0))
    }

    /// The grid rows the live view ends at: down to the cursor or the lowest row with content,
    /// and at least the view's height. The rows below are blank, so a grid taller than the view
    /// (`grow_only` after the window shrank) shows the cursor, not its blank bottom (MAX-140)
    fn live_rows(&self) -> usize {
        let grid_rows = self.get_vt100_size().0 as usize;
        self.live_bottom
            .max(self.scroller.view_height)
            .min(grid_rows)
    }

    pub fn max_screen_pan_y(&self) -> usize {
        self.live_rows().saturating_sub(self.scroller.view_height)
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

        self.scroller.max_scroll_y = max_total;
        self.scroller.clamp();

        let (y_offset, vt_scrollback) = if self.scroller.offset_y <= max_pan_y {
            // In the active screen: pan from bottom to top
            let y = max_pan_y - self.scroller.offset_y;
            (y as u16, 0)
        } else {
            // Past the active screen: camera is at top (y=0), vt100 scrolls history
            let history_offset = self.scroller.offset_y - max_pan_y;
            (0, history_offset)
        };

        self.pan_offset_y = y_offset as usize;
        self.vt100_parser.screen_mut().set_scrollback(vt_scrollback);
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroller.max_scroll_y = self.max_vertical_scroll();
        self.scroller.scroll_up(lines);
        self.reconcile_both_vertical_scroll();
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroller.max_scroll_y = self.max_vertical_scroll();
        self.scroller.scroll_down(lines);
        self.reconcile_both_vertical_scroll();
    }

    pub fn scroll_up_page(&mut self) {
        self.scroll_up(self.scroller.view_height.saturating_sub(1));
    }

    pub fn scroll_down_page(&mut self) {
        self.scroll_down(self.scroller.view_height.saturating_sub(1));
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroller.scroll_to_bottom();
        self.reconcile_both_vertical_scroll();
    }

    /// To `rows` above the live view (MAX-142)
    pub fn scroll_to_row(&mut self, rows: usize) {
        self.scroller.max_scroll_y = self.max_vertical_scroll();
        self.scroller.scroll_to(rows);
        self.reconcile_both_vertical_scroll();
    }

    pub fn scroll_to_top(&mut self) {
        self.scroller.max_scroll_y = self.max_vertical_scroll();
        self.scroller.scroll_to_top();
        self.reconcile_both_vertical_scroll();
    }

    pub fn scroll_to_target_line(&mut self, target_line_idx: usize, view_height: usize) {
        let (grid_rows, _) = self.get_vt100_size();
        let half_canvas = (grid_rows as usize).min(view_height) / 2;
        self.scroll_to_bottom_line(target_line_idx + half_canvas);
    }

    /// Scrolls so the view's last row is world row `line_idx` (clamped)
    fn scroll_to_bottom_line(&mut self, line_idx: usize) {
        // the offsets count from the live view's last row, not the grid's
        let total_world_lines = self.max_content_scrollback + self.live_rows();
        if total_world_lines == 0 {
            return;
        }
        let target_offset = (total_world_lines - 1).saturating_sub(line_idx);

        self.scroller.max_scroll_y = self.max_vertical_scroll();
        self.scroller.scroll_to(target_offset);
        self.reconcile_both_vertical_scroll();
    }

    pub fn refresh_max_scrollback(&mut self) {
        self.max_content_scrollback = self.vt100_parser.screen().scrollback_len();
        self.scroller.max_scroll_y = self.max_vertical_scroll();
    }

    pub fn set_vt100_size(&mut self, rows: u16, cols: u16) {
        let (old_rows, old_cols) = self.get_vt100_size();
        if (old_rows, old_cols) == (rows, cols) || rows == 0 || cols == 0 {
            return;
        }

        self.search_text = None;
        let scrollback_before = self.max_content_scrollback;
        let cursor_row_before = self.vt100_parser.screen().cursor_position().0 as usize;
        // the parser's `set_size` (not the screen's) reports the rows it evicts
        self.vt100_parser.set_size(vt100_dim(rows), vt100_dim(cols));
        let events = self.vt100_parser.callbacks_mut();
        let evicted = std::mem::take(&mut events.evicted);
        // the remote app asked for each new size (mode 2048), sent with the answers (MAX-159)
        if events.size_reports {
            events.answers.push(size_report(rows, cols));
        }
        self.update_live_bottom();
        self.content_cols = None;
        // in the alternate screen the main rows can't be measured: the next RX on the main
        // screen fixes the slot count (see `process_rx`)
        if self.vt100_parser.screen().alternate_screen() {
            return;
        }
        self.refresh_max_scrollback();
        let scrollback = self.max_content_scrollback;

        // since vt100 0.18 a smaller height pushes the top rows into scrollback (evicting the
        // oldest when it is full) and drops rows at the bottom; a taller one pulls rows back from
        // scrollback and adds rows at the bottom. The world rows keep their order, only the
        // scrollback/screen boundary moves
        if rows < old_rows {
            let evicted = evicted.min(self.row_timestamps.len());
            self.row_timestamps.drain(..evicted);
            self.rebase_line_refs(evicted);
        } else {
            // the cursor moved down by the rows inserted at the top. Those not pulled back from
            // scrollback are blank rows vt100 adds for scrollback it no longer holds (evicted)
            let cursor_row = self.vt100_parser.screen().cursor_position().0 as usize;
            let inserted = cursor_row.saturating_sub(cursor_row_before);
            let pulled = scrollback_before.saturating_sub(scrollback);
            let blank = inserted.saturating_sub(pulled);
            let at = scrollback.min(self.row_timestamps.len());
            for _ in 0..blank {
                self.row_timestamps.insert(at, None);
            }
        }
        self.row_timestamps.resize(scrollback + rows as usize, None);
    }

    /// Return (rows, cols)
    pub fn get_vt100_size(&self) -> (u16, u16) {
        vt100_size(self.vt100_parser.screen())
    }

    pub fn get_max_scrollback(&self) -> usize {
        self.max_content_scrollback
    }

    pub fn is_new_content_missed(&self) -> bool {
        self.scroller.new_content_missed
    }

    pub fn all_text_lines(&mut self) -> Vec<String> {
        let max_sb = self.max_content_scrollback;
        let (rows, cols) = self.get_vt100_size();
        let current_sb = self.vt100_parser.screen().scrollback();

        let mut lines = Vec::with_capacity(max_sb + rows as usize);

        let mut sb = max_sb;
        while sb > 0 {
            self.vt100_parser.screen_mut().set_scrollback(sb);
            if let Some(first_row) = self.vt100_parser.screen().rows(0, cols).next() {
                lines.push(first_row);
            }
            sb -= 1;
        }

        self.vt100_parser.screen_mut().set_scrollback(0);
        for row in self.vt100_parser.screen().rows(0, cols) {
            lines.push(row);
        }

        self.vt100_parser.screen_mut().set_scrollback(current_sb);

        lines
    }

    pub fn update_action_terminal(&mut self, action: TerminalAction) -> UpdateEffect {
        match action {
            TerminalAction::MouseReport(bytes) => UpdateEffect::Transmit(bytes),
            // the emulator's size, where the lines wrap, as the query answers say (MAX-151)
            TerminalAction::AutoResizeRemoteToEmulator => {
                let (rows, cols) = self.get_vt100_size();
                let cmd =
                    format!("stty rows {rows} cols {cols}; export LINES={rows} COLUMNS={cols}\n");
                UpdateEffect::Transmit(cmd.into_bytes())
            }
            // taken by the app, which has the capture settings and the dialog (MAX-147)
            TerminalAction::Screenshot(_) => UpdateEffect::None,
            TerminalAction::AutoResizeEmulatorToViewer => {
                self.set_vt100_size(
                    self.scroller.view_height as u16,
                    self.scroller.view_width as u16,
                );
                self.scroller.offset_x = 0;
                self.scroller.max_pan_x = 0;
                UpdateEffect::None
            }
            // what is sent to the remote brings the view back to the live screen, as typing
            // there blind is no use; what it sends never moves a scrolled view (MAX-141)
            TerminalAction::Key(key_event) => {
                let application_cursor = self.vt100_screen().application_cursor();
                match encode_terminal_key(key_event, application_cursor) {
                    Some(bytes) => {
                        self.scroll_to_bottom();
                        UpdateEffect::Transmit(bytes)
                    }
                    None => UpdateEffect::None,
                }
            }
            TerminalAction::Paste(text) => {
                self.scroll_to_bottom();
                let bracketed = self.vt100_screen().bracketed_paste();
                UpdateEffect::Transmit(terminal_paste_bytes(&text, bracketed))
            }
        }
    }

    pub fn handle_search_action(&mut self, action: SearchModeAction, raw_view: RawSideView) {
        match action {
            SearchModeAction::Start(direction) => {
                self.search_state.direction = direction;
                self.search_state.origin = self.copy_origin();
                self.search_state.toggle_itself();
                if self.search_state.active {
                    self.update_search_matches(raw_view);
                }
            }
            SearchModeAction::Next => {
                if self.search_state.active
                    && let Some((row, col)) = self.search_state.next_match()
                {
                    self.copy_state.jump_to(row, col);
                }
            }
            SearchModeAction::Previous => {
                if self.search_state.active
                    && let Some((row, col)) = self.search_state.prev_match()
                {
                    self.copy_state.jump_to(row, col);
                }
            }
            SearchModeAction::Cancel => {
                self.search_state.toggle(false);
                self.search_state.kept = false;
                // from copy mode, back where the search started (MAX-215)
                if let Some(origin) = self.search_state.origin.take() {
                    self.copy_state.cursor = origin;
                    self.reveal_copy_cursor(Reveal::Center);
                }
            }
            SearchModeAction::Confirm => {
                let match_pos = self.search_state.current_match_position();
                self.search_state.toggle(false);
                self.search_state.kept = true;

                // from copy mode the cursor is already on the match, the selection kept
                if self.search_state.origin.take().is_none() {
                    let (row, col) = match_pos.unwrap_or((0, 0));
                    self.copy_state.enter_at(row, col);
                }
                self.reveal_copy_cursor(Reveal::Center);
            }
            SearchModeAction::HandleKey(key_event) => {
                if self.search_state.active {
                    self.handle_input_search(key_event, raw_view);
                }
            }
            SearchModeAction::Paste(text) => {
                if self.search_state.active {
                    paste_into_input(&mut self.search_state.query, &text);
                    self.update_search_matches(raw_view);
                }
            }
            SearchModeAction::Query(text) => {
                self.search_state.direction = SearchDirection::Forward;
                self.search_state.origin = self.copy_origin();
                self.search_state.toggle(true);
                self.search_state.query = Input::new(text);
                self.update_search_matches(raw_view);
            }
            SearchModeAction::Kind => {
                self.search_state.toggle_mode();
                self.update_search_matches(raw_view);
            }
            SearchModeAction::ClickAt(column) => {
                click_input_at(&mut self.search_state.query, column);
            }
            SearchModeAction::Repeat(_) => unreachable!("run by App, for its toast"),
        }
    }

    /// `n` in copy mode, `N` with `opposite` (MAX-215): the cursor to the next match of the
    /// last search, its matches shown again. None without a match, `Some(true)` when it went
    /// around the history's end
    pub fn repeat_search(&mut self, opposite: bool) -> Option<bool> {
        let cursor = (
            self.copy_state.cursor.line_idx,
            self.copy_state.cursor.char_idx,
        );
        let wrapped = self.search_state.step_from(cursor, opposite)?;
        let (row, col) = self.search_state.current_match_position()?;
        self.copy_state.jump_to(row, col);
        self.search_state.kept = true;
        Some(wrapped)
    }

    pub fn handle_copy_action(
        &mut self,
        action: CopyModeAction,
        settings: &SettingsConfig,
    ) -> Option<String> {
        if let CopyModeAction::HandleKey(key) = action {
            let line_text = read_vt100_world_line_text(
                &mut self.vt100_parser,
                self.max_content_scrollback,
                self.copy_state.cursor.line_idx,
            )
            .unwrap_or_default();
            if let Some(sub_action) = dispatch_copy_key_vi(
                &mut self.vi_pending_copy,
                &line_text,
                self.copy_state.cursor.char_idx,
                key,
            ) {
                return self.handle_copy_action(sub_action, settings);
            }
            return None;
        }

        if action == CopyModeAction::Enter {
            self.copy_held = false;
            let (c_row, c_col) = self.vt100_screen().cursor_position();
            let max_col = self.get_vt100_size().1.saturating_sub(1) as usize;
            // entering copy mode leaves the view where it is (MAX-143): on the remote cursor
            // from the live view, else on the start of the view's bottom row
            if self.scroller.offset_y > 0 {
                self.copy_state.enter_at(self.visible_lines().1, 0);
            } else {
                self.copy_state.enter_at(
                    self.max_content_scrollback + (c_row as usize),
                    (c_col as usize).min(max_col),
                );
            }
            return None;
        }
        let reveal = Reveal::for_move(&action, self.copy_cursor_shown());

        // adapter for the TextSource trait
        let mut source = TerminalModeTextSource {
            parser: &mut self.vt100_parser,
            max_content_scrollback: self.max_content_scrollback,
            live_bottom: self.live_bottom,
            raw_view: settings.raw_side.view,
            view_height: self.scroller.view_height,
        };

        let result = self.copy_state.apply_action(action, &mut source);
        // out of copy mode (Esc, a yank), a view it held goes back live (MAX-193), and the
        // matches kept after a search are no longer shown (MAX-215)
        if !self.copy_state.active {
            self.search_state.kept = false;
            if std::mem::take(&mut self.copy_held) {
                self.scroll_to_bottom();
            }
        }
        match result {
            CopyActionResult::Moved => {
                self.reveal_copy_cursor(reveal);
                None
            }
            CopyActionResult::Yanked(text) => Some(text),
            _ => None,
        }
    }

    /// The world rows on screen, first and last
    pub fn visible_lines(&self) -> (usize, usize) {
        let (grid_rows, _) = self.get_vt100_size();
        let current_sb = self.vt100_screen().scrollback();
        // on the live screen the view starts `pan_offset_y` rows into a grid taller than it: a
        // cursor on its top rows was taken as out of view and the view jumped (MAX-126)
        let top_visible = if current_sb > 0 {
            self.max_content_scrollback.saturating_sub(current_sb)
        } else {
            self.max_content_scrollback + self.pan_offset_y
        };
        let effective_h = (grid_rows as usize).min(self.scroller.view_height);
        (top_visible, top_visible + effective_h.saturating_sub(1))
    }

    fn copy_cursor_shown(&self) -> bool {
        let (top, bottom) = self.visible_lines();
        (top..=bottom).contains(&self.copy_state.cursor.line_idx)
    }

    /// Brings the view to the copy cursor (MAX-143): a step out of the view moves it by the
    /// fewest rows, a jump centers it; the pan follows the cursor's column the same way
    fn reveal_copy_cursor(&mut self, reveal: Reveal) {
        let cursor_row = self.copy_state.cursor.line_idx;
        let cursor_col = self.copy_state.cursor.char_idx;
        let view_h = self.scroller.view_height;
        let view_w = self.scroller.view_width; // or passed-in visible columns width
        let (_, grid_cols) = self.get_vt100_size();

        let (top_visible, bottom_visible) = self.visible_lines();
        let shown = (top_visible..=bottom_visible).contains(&cursor_row);
        match reveal {
            _ if shown => {}
            Reveal::Edge if cursor_row < top_visible => {
                self.scroll_to_bottom_line(cursor_row + (bottom_visible - top_visible));
            }
            Reveal::Edge => self.scroll_to_bottom_line(cursor_row),
            Reveal::Center => self.scroll_to_target_line(cursor_row, view_h),
        }

        if view_w > 0 && (grid_cols as usize) > view_w {
            let max_pan_x = (grid_cols as usize).saturating_sub(view_w);

            if cursor_col < self.scroller.offset_x {
                self.scroller.offset_x = cursor_col;
            } else if cursor_col >= self.scroller.offset_x + view_w {
                self.scroller.offset_x = (cursor_col + 1).saturating_sub(view_w).min(max_pan_x);
            }
        } else {
            self.scroller.offset_x = 0;
        }
    }

    pub fn update_search_matches(&mut self, raw_view: RawSideView) {
        if self.search_text.is_none() {
            self.search_text = Some(self.all_text_lines());
        }
        let text = self.search_text.as_deref().unwrap_or_default();
        self.search_state
            .update_matches_from_strings(text, raw_view);
        self.follow_search_match();
    }

    /// The copy cursor a search opened from copy mode starts from (MAX-215)
    fn copy_origin(&self) -> Option<LogicalPosition> {
        self.copy_state.active.then_some(self.copy_state.cursor)
    }

    /// From copy mode the cursor goes to the current match as the query changes, so the view
    /// follows it; back to where the search started without one (MAX-215)
    fn follow_search_match(&mut self) {
        let Some(origin) = self.search_state.origin else {
            return;
        };
        match self.search_state.current_match_position() {
            Some((row, col)) => self.copy_state.jump_to(row, col),
            None => self.copy_state.cursor = origin,
        }
    }

    pub fn handle_input_search(&mut self, key_event: KeyEvent, raw_view: RawSideView) {
        let before = self.search_state.query.value().to_owned();
        dispatch_input_key(&mut self.search_state.query, key_event);
        if self.search_state.query.value() != before {
            self.update_search_matches(raw_view);
        }
    }

    pub fn handle_side_view_action(&mut self, action: SideViewAction, raw_view: RawSideView) {
        match action {
            SideViewAction::PanLeft(bytes) => {
                self.raw_scroller.pan_left(bytes, raw_view);
            }
            SideViewAction::PanRight(bytes) => {
                self.raw_scroller.pan_right(bytes, raw_view);
            }
        }
    }

    pub fn update_raw_limits(&mut self, overflow: usize, raw_view: RawSideView) {
        self.raw_scroller.set_limit(overflow, raw_view);
    }

    #[inline]
    pub fn search(&self) -> &SearchState {
        &self.search_state
    }

    #[inline]
    pub fn search_mut(&mut self) -> &mut SearchState {
        &mut self.search_state
    }

    #[inline]
    pub fn copy_state(&self) -> &CopyModeState {
        &self.copy_state
    }

    #[inline]
    pub fn scroller(&self) -> &ViewportScroller {
        &self.scroller
    }

    #[inline]
    pub fn raw_pan_x(&self) -> usize {
        self.raw_scroller.pan_x
    }

    #[inline]
    pub fn raw_pan_x_max(&self) -> usize {
        self.raw_scroller.max_pan_x
    }

    /// Replays a past session's received chunks, each row at the time of the chunk that reached
    /// it when known (MAX-175), then resets the modes they may have left set (the alternate
    /// screen, application cursor keys, mouse reports, bracketed paste, a scroll region, a
    /// hidden cursor, colors) so the keys and the mouse work. Then, like a `clear`, the screen
    /// goes up into the scrollback and `separator` is written dimmed on the top row, the cursor
    /// under it (MAX-176)
    pub fn restore(&mut self, encoding: TextEncoding, chunks: &[TimedChunk], separator: &str) {
        for chunk in chunks {
            self.process_rx_at(encoding, chunk.bytes, chunk.time);
        }
        // leaving it restores a saved cursor, so only when the session is in it
        if self.vt100_parser.screen().alternate_screen() {
            self.process_rx_at(TextEncoding::Utf8, b"\x1b[?1049l", None);
        }
        let (row, col) = self.vt100_parser.screen().cursor_position();
        let mut reset = String::from(RESTORE_RESET);
        reset.push_str(&format!("\x1b[{};{}H", row + 1, col + 1));
        // the cursor's line is kept: a new one only when the cursor isn't at the start of one
        if col > 0 {
            reset.push_str("\r\n");
        }
        self.process_rx_at(TextEncoding::Utf8, reset.as_bytes(), None);

        // the rows up to the cursor's and the last one with text go into the scrollback, line
        // feeds on the bottom row pushing them up
        let rows = self.get_vt100_size().0;
        let cursor_row = self.vt100_parser.screen().cursor_position().0;
        let below_text = (0..rows)
            .rev()
            .find(|&row| self.row_has_content(row))
            .map_or(0, |last| last + 1);
        let mut end = format!("\x1b[{rows};1H");
        end.push_str(&"\n".repeat(cursor_row.max(below_text) as usize));
        end.push_str("\x1b[1;1H\x1b[2m");
        end.push_str(separator);
        end.push_str("\x1b[0m\r\n");
        self.process_rx_at(TextEncoding::Utf8, end.as_bytes(), None);
        // on the top row: its world row is the first after the scrollback
        self.restored_separator = Some(self.max_content_scrollback);
        // the queries of a past session are not answered (MAX-151), and the size reports its
        // app asked for are not this run's to send (MAX-159)
        self.take_answers();
        self.vt100_parser.callbacks_mut().size_reports = false;
    }

    /// The replies to the queries received since the last call, to send or drop (MAX-151)
    pub fn take_answers(&mut self) -> Vec<Answer> {
        std::mem::take(&mut self.vt100_parser.callbacks_mut().answers)
    }

    /// The screen as it is now, and a flash to show it was taken
    pub fn screenshot(&mut self) -> Screenshot {
        self.trigger_flash();
        let screen = self.vt100_parser.screen();
        Screenshot {
            text: screen.contents(),
            ansi: screen.contents_formatted(),
        }
    }

    pub fn trigger_flash(&mut self) {
        self.flash_until = Some(Instant::now() + Duration::from_millis(120));
    }

    pub fn has_pending_flash(&self) -> bool {
        self.flash_until.is_some()
    }

    pub fn is_flashing(&mut self) -> bool {
        if let Some(until) = self.flash_until {
            if Instant::now() < until {
                return true;
            } else {
                self.flash_until = None;
            }
        }
        false
    }

    pub fn center_raw_pan_on_byte(
        &mut self,
        raw_view: RawSideView,
        usable_panel_width: usize,
        local_byte_offset: usize,
        total_sub_bytes: usize,
    ) {
        self.raw_scroller.center_on_byte(
            raw_view,
            usable_panel_width,
            local_byte_offset,
            total_sub_bytes,
        );
    }

    pub fn sync_raw_pan_with_local_col(
        &mut self,
        raw_view: RawSideView,
        usable_panel_width: usize,
        local_byte_offset: usize,
        total_sub_bytes: usize,
    ) {
        self.raw_scroller.sync_with_cursor(
            raw_view,
            usable_panel_width,
            local_byte_offset,
            total_sub_bytes,
        );
    }

    pub fn center_on_target_match(
        &mut self,
        target_row: usize,
        target_col: usize,
        view_w: usize,
        view_h: usize,
    ) {
        let (grid_rows, grid_cols) = self.get_vt100_size();
        let max_sb = self.max_content_scrollback;
        // the offsets count from the live view's last row, not the grid's
        let total_world_lines = max_sb + self.live_rows();

        if total_world_lines == 0 || target_row >= total_world_lines {
            return;
        }

        let lines_from_bottom = (total_world_lines - 1).saturating_sub(target_row);
        let half_h = (grid_rows as usize).min(view_h) / 2;
        let target_scroll_y = lines_from_bottom.saturating_sub(half_h);

        self.scroller.max_scroll_y = self.max_vertical_scroll();
        self.scroller.scroll_to(target_scroll_y);
        self.reconcile_both_vertical_scroll();

        if view_w > 0 && (grid_cols as usize) > view_w {
            let max_pan_x = (grid_cols as usize).saturating_sub(view_w);
            let half_w = view_w / 2;
            self.scroller.offset_x = target_col.saturating_sub(half_w).min(max_pan_x);
        }
    }
}

// HACK: there is no function to fetch string content from history so sb everywhere
pub fn read_vt100_world_line_text(
    parser: &mut TerminalParser,
    max_sb: usize,
    world_row: usize,
) -> Option<String> {
    // vt100's own row text, like search and the selection read it: the gaps a tab or a cursor
    // move leave are spaces, so a char index is a screen column (#33)
    with_world_row(parser, max_sb, world_row, |screen, view_row, cols| {
        screen.rows(0, cols).nth(view_row).unwrap_or_default()
    })
}

/// The bytes of a world row as the raw side panel shows them: vt100's formatted row (its text
/// with the escape sequences of its styles), so the raw copy cursor walks the panel's bytes
/// (MAX-71)
pub fn read_vt100_world_line_formatted(
    parser: &mut TerminalParser,
    max_sb: usize,
    world_row: usize,
) -> Option<Vec<u8>> {
    with_world_row(parser, max_sb, world_row, |screen, view_row, cols| {
        screen
            .rows_formatted(0, cols)
            .nth(view_row)
            .unwrap_or_default()
            .into_bytes()
    })
}

/// Reads world row `world_row` (history first, then the screen) from the screen scrolled back
/// so the row is shown, then restores the scrollback
fn with_world_row<T>(
    parser: &mut TerminalParser,
    max_sb: usize,
    world_row: usize,
    read: impl FnOnce(&vt100::Screen, usize, u16) -> T,
) -> Option<T> {
    // this function is not a member to simlify ownership with TerminalModeTextSource
    let (rows, cols) = vt100_size(parser.screen());
    let total_world = max_sb + rows as usize;

    if world_row >= total_world {
        return None;
    }

    let current_sb = parser.screen().scrollback();

    let (view_row, target_sb) = if world_row < max_sb {
        (0, max_sb - world_row)
    } else {
        ((world_row - max_sb) as u16, 0)
    };

    if current_sb != target_sb {
        parser.screen_mut().set_scrollback(target_sb);
    }

    // `rows()` / `rows_formatted()` are lazy but build every row before `view_row`: one row for
    // history (always at the top), a screen at most
    let value = read(parser.screen(), view_row as usize, cols);
    if current_sb != target_sb {
        parser.screen_mut().set_scrollback(current_sb);
    }

    Some(value)
}

/// minicom's add CR (a lone LF made CR LF) and add LF (a CR made CR LF) for the emulator, the
/// device's own CR LF left as one line break: its LF is dropped when add LF already put one.
/// `after_cr` carries the last byte's CR across chunks. CR and LF are the same byte in every
/// encoding the app reads (MAX-181)
pub fn add_newlines<'a>(
    bytes: &'a [u8],
    add_cr: bool,
    add_lf: bool,
    after_cr: &mut bool,
) -> Cow<'a, [u8]> {
    if !add_cr && !add_lf {
        if let Some(&last) = bytes.last() {
            *after_cr = last == b'\r';
        }
        return Cow::Borrowed(bytes);
    }
    let mut out = Vec::with_capacity(bytes.len() + bytes.len() / 8);
    for &byte in bytes {
        match byte {
            b'\r' if add_lf => out.extend_from_slice(b"\r\n"),
            b'\n' if *after_cr => {
                if !add_lf {
                    out.push(b'\n');
                }
            }
            b'\n' if add_cr => out.extend_from_slice(b"\r\n"),
            _ => out.push(byte),
        }
        *after_cr = byte == b'\r';
    }
    Cow::Owned(out)
}

/// Bytes sent to the remote for a key typed in terminal mode (legacy xterm encoding). When the
/// remote turned on application cursor mode (DECCKM, `ESC[?1h`), unmodified arrows, Home and End
/// are sent as `ESC O x` instead of `ESC [ x`, like xterm does. Modified ones (`ESC[1;5A`) are the
/// same in both modes
fn encode_terminal_key(key_event: KeyEvent, application_cursor: bool) -> Option<Vec<u8>> {
    let key_event = to_terminput_key(key_event).ok()?;
    let mut buf = [0u8; 32];
    let len = terminput::Event::Key(key_event)
        .encode(&mut buf, Encoding::Xterm)
        .ok()?;
    let mut bytes = buf[..len].to_vec();
    if application_cursor
        && let [
            0x1b,
            introducer @ b'[',
            b'A' | b'B' | b'C' | b'D' | b'H' | b'F',
        ] = bytes.as_mut_slice()
    {
        *introducer = b'O';
    }
    Some(bytes)
}

/// Paste sent to the remote in one write. Line breaks are sent as `\r`, like the Enter key, and
/// the text is wrapped in paste markers when the remote asked for them (`ESC[?2004h`)
fn terminal_paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() + 12);
    if bracketed {
        bytes.extend_from_slice(b"\x1b[200~");
    }
    let mut prev_cr = false;
    for &b in text.as_bytes() {
        match b {
            b'\n' if prev_cr => {}
            b'\n' => bytes.push(b'\r'),
            _ => bytes.push(b),
        }
        prev_cr = b == b'\r';
    }
    if bracketed {
        bytes.extend_from_slice(b"\x1b[201~");
    }
    bytes
}

struct TerminalModeTextSource<'a> {
    pub parser: &'a mut TerminalParser,
    pub max_content_scrollback: usize,
    pub live_bottom: usize,
    pub raw_view: RawSideView,
    pub view_height: usize,
}

impl<'a> TextSource for TerminalModeTextSource<'a> {
    /// The rows down to the cursor or the last one with content: the blank rows below never had
    /// any, so the copy cursor and a selection stop above them (MAX-138)
    fn total_lines(&mut self) -> usize {
        let (rows, _) = vt100_size(self.parser.screen());
        self.max_content_scrollback + self.live_bottom.min(rows as usize)
    }

    fn get_line_text(&mut self, line_idx: usize) -> Option<String> {
        read_vt100_world_line_text(self.parser, self.max_content_scrollback, line_idx)
    }

    fn get_line_raw_bytes(&mut self, line_idx: usize) -> Option<Vec<u8>> {
        read_vt100_world_line_formatted(self.parser, self.max_content_scrollback, line_idx)
    }

    #[inline]
    fn raw_view(&mut self) -> RawSideView {
        self.raw_view
    }

    fn view_height(&mut self) -> usize {
        self.view_height
    }
}

#[cfg(test)]
#[path = "../tests/app/terminal_mode.rs"]
mod tests;
