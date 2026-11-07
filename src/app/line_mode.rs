use chrono::{DateTime, Local};
use crossterm::event::KeyEvent;
use std::collections::VecDeque;
use tui_input::Input;

use crate::{
    app::{
        UpdateEffect,
        data_processing::{
            build_display_to_raw_byte_map, entry_display_text, entry_display_to_wire_map,
        },
        marks::Marks,
        raw_scroller::RawSideScroller,
        scroller::{ScrollStatus, ViewportScroller},
        search::SearchState,
        selection::{CopyActionResult, CopyModeState, Reveal, TextSource},
        types::{LogEntry, RxLineSplitter, ViewportSize},
    },
    capture::TimedChunk,
    configs::{LineDisplayMode, RawSideView, SettingsConfig, TextEncoding, WrapMode},
    inputs::{
        CopyModeAction, LineInputAction, SearchModeAction, SideViewAction, ViMode, ViState,
        click_input_at, dispatch_input_key, dispatch_input_key_vi,
        local_dispatchers::dispatch_copy_key_vi, paste_into_input, vi::ViPending,
    },
};

pub struct LineMode {
    /// Completed lines of device output (scrollback) String because already processed
    local_history: VecDeque<LogEntry>,
    /// Completed lines of device input sent
    local_send_history: VecDeque<String>,
    /// None = active editing, Some(i) browsing #i history line
    history_index: Option<usize>,
    /// In-progress line when user press up
    temp_input: Input,
    /// Max line history
    max_lines: usize,
    /// Max sent lines kept for history up/down
    max_send_lines: usize,
    /// Current line being received from device output
    partial_line: RxLineSplitter,
    /// Current line being typed by user as futur device input
    user_input: Input,
    /// Columns of the TX input scrolled out on the left, kept from frame to frame by the
    /// render: the view moves only when the cursor pushes past an edge (MAX-145)
    prompt_scroll: usize,
    vi_state_prompt: ViState,
    search_state: SearchState,
    copy_state: CopyModeState,
    scroller: ViewportScroller,
    /// Rows of the scrolled-to entry (`scroller.offset_y` entries from the bottom) hidden below
    /// the view: with it, scrolling moves by rows through entries taller than the view (#36)
    rows_below: usize,
    /// Rows to scroll (up > 0), applied by the render with the wrapped layout it computes
    pending_scroll_rows: isize,
    /// Rows each history entry takes on screen, aligned with `local_history` (0 = not counted
    /// yet), counted by the render for the scrollbar and the scroll status (#5d)
    row_heights: VecDeque<u32>,
    /// What `row_heights` were counted for (view width, wrap and display modes...): the render
    /// gives a new key when that changes, which resets every height to "not counted"
    row_heights_key: u64,
    /// Scroll position in rows, from the render (#5d)
    row_scroll_status: ScrollStatus,
    raw_scroller: RawSideScroller,
    vi_state_copymode: Option<ViPending>,
    evicted_lines: usize,
    /// How the next frame brings the view to the copy cursor, which only the render can place
    /// in rows (MAX-143)
    copy_reveal: Option<Reveal>,
    /// The copy cursor's row was on screen in the last frame
    copy_cursor_shown: bool,
    /// The entry and char starting the view's bottom row in the last frame
    bottom_row_start: Option<(usize, usize)>,
    /// Where the live view starts while the lines after it don't fill it, like after a
    /// `clear` (MAX-176)
    view_floor: Option<ViewFloor>,
    /// Copy mode held the live view when lines arrived: leaving it goes back live (MAX-193)
    copy_held: bool,
    /// The marks on history entries (MAX-164)
    marks: Marks,
    /// The entry a mark jump centers the view on, for the next frame
    mark_jump: Option<usize>,
    /// The entry on the view's middle row in the last frame: previous / next mark start there
    middle_line: Option<usize>,
}

/// The entry the live view starts on, and the rows scrolled above it while blank rows still
/// show under the last line
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewFloor {
    pub line_idx: usize,
    pub rows_above: usize,
}

impl LineMode {
    pub fn new(max_history_lines: usize, max_send_lines: usize) -> Self {
        let search_state = SearchState::default();
        let copy_state = CopyModeState::default();
        let scroller = ViewportScroller::default();
        let raw_scroller = RawSideScroller::default();
        LineMode {
            // grown on demand, the configured size is only a limit
            local_history: VecDeque::new(),
            local_send_history: VecDeque::new(),
            history_index: None,
            temp_input: Input::default(),
            max_lines: max_history_lines,
            max_send_lines,
            partial_line: RxLineSplitter::default(),
            user_input: Input::default(),
            prompt_scroll: 0,
            vi_state_prompt: ViState::default(),
            search_state,
            copy_state,
            scroller,
            rows_below: 0,
            pending_scroll_rows: 0,
            row_heights: VecDeque::new(),
            row_heights_key: 0,
            row_scroll_status: ScrollStatus::default(),
            raw_scroller,
            vi_state_copymode: None,
            evicted_lines: 0,
            copy_reveal: None,
            copy_cursor_shown: false,
            bottom_row_start: None,
            view_floor: None,
            copy_held: false,
            marks: Marks::default(),
            mark_jump: None,
            middle_line: None,
        }
    }

    #[inline]
    pub fn input(&self) -> &Input {
        &self.user_input
    }

    pub fn partial_line_timestamp(&self) -> Option<DateTime<Local>> {
        self.partial_line.timestamp()
    }

    /// The received bytes cut into lines, a lone CR ending one with `add_lf` (MAX-181)
    pub fn feed_rx(&mut self, add_lf: bool, encoding: TextEncoding, bytes: &[u8]) {
        self.feed_rx_at(add_lf, encoding, bytes, Some(Local::now()));
    }

    /// `feed_rx` for bytes received at `time`, none when it isn't known (MAX-175)
    fn feed_rx_at(
        &mut self,
        add_lf: bool,
        encoding: TextEncoding,
        bytes: &[u8],
        time: Option<DateTime<Local>>,
    ) {
        for &b in bytes {
            if self.partial_line.ends_at_cr(b, add_lf) {
                self.commit_partial_line(encoding);
            }
            if self.partial_line.push_at(b, time) {
                self.commit_partial_line(encoding);
            }
        }

        self.rebase_line_refs();
    }

    fn commit_partial_line(&mut self, encoding: TextEncoding) {
        let line = encoding.decode(self.partial_line.line());
        let timestamp = self.partial_line.timestamp();
        let entry = LogEntry::received(line, self.partial_line.line(), timestamp);
        self.partial_line.clear();
        self.push_entry(entry);
    }

    pub fn push_history(&mut self, line: String) {
        let entry = LogEntry::new(line, self.partial_line.take_timestamp());
        self.push_entry(entry);
        self.rebase_line_refs();
    }

    /// A past session's received chunks cut into lines as they were live, before any live line
    /// (MAX-175): line mode shows what it showed then, the lines sent aside (they aren't
    /// received), each line at its first byte's time when known. A last line not ended is one
    /// too, so the next one starts on its own
    pub fn restore_rx(&mut self, add_lf: bool, encoding: TextEncoding, chunks: &[TimedChunk]) {
        for chunk in chunks {
            self.feed_rx_at(add_lf, encoding, chunk.bytes, chunk.time);
        }
        if !self.partial_line.line().is_empty() {
            self.commit_partial_line(encoding);
            self.rebase_line_refs();
        }
    }

    /// Pushes lines restored from a session, before any live line (MAX-148)
    pub fn restore_entries(&mut self, entries: impl IntoIterator<Item = LogEntry>) {
        for entry in entries {
            self.push_entry(entry);
        }
        self.rebase_line_refs();
    }

    /// Push without rebasing line indexes, the caller rebases once for a whole batch
    fn push_entry(&mut self, entry: LogEntry) {
        self.local_history.push_back(entry);
        self.row_heights.push_back(0);

        // A view scrolled up (by entries, by rows of the last entry, or by a scroll not drawn
        // yet) keeps showing the same rows: its position counts from the bottom, so it moves up
        // by the new entry. Only the live view follows the new lines (MAX-124)
        let is_scrolled =
            self.scroller.offset_y > 0 || self.rows_below > 0 || self.pending_scroll_rows > 0;
        // copy mode holds the live view too, so the selection doesn't move (MAX-193)
        if !is_scrolled && self.copy_state.active {
            self.copy_held = true;
        }
        if is_scrolled || self.copy_state.active {
            self.scroller.offset_y = self.scroller.offset_y.saturating_add(1);
        }
        self.scroller.on_rx_received();
        while self.local_history.len() > self.max_lines {
            self.local_history.pop_front();
            self.row_heights.pop_front();
            self.evicted_lines += 1;
        }
    }

    fn rebase_line_refs(&mut self) {
        let evicted = std::mem::take(&mut self.evicted_lines);
        if evicted == 0 {
            return;
        }
        self.search_state.rebase_after_eviction(evicted);
        self.copy_state.rebase_after_eviction(evicted);
        self.view_floor = self.view_floor.and_then(|floor| {
            Some(ViewFloor {
                line_idx: floor.line_idx.checked_sub(evicted)?,
                ..floor
            })
        });
        self.marks.rebase_after_eviction(evicted);
        self.mark_jump = self.mark_jump.and_then(|line| line.checked_sub(evicted));
        self.middle_line = self.middle_line.and_then(|line| line.checked_sub(evicted));
    }

    pub fn clear_history(&mut self) {
        self.local_history.clear();
        self.row_heights.clear();
        self.view_floor = None;
        self.marks.clear();
        self.mark_jump = None;
        self.middle_line = None;
    }

    #[inline]
    pub fn marks(&self) -> &Marks {
        &self.marks
    }

    /// Puts mark `letter` on the copy cursor's entry in copy mode, on the last one from the
    /// live view (what just arrived), else on the view's middle one (what is looked at);
    /// returns that entry, none when there is no line or the letter isn't `a`..`z` (MAX-164)
    pub fn add_mark(&mut self, letter: char) -> Option<usize> {
        let last = self.local_history.len().checked_sub(1)?;
        let live = self.scroller.offset_y == 0 && self.rows_below == 0;
        let line = if self.copy_state.active {
            self.copy_state.cursor.line_idx
        } else if live {
            last
        } else {
            self.view_middle_line()
        };
        self.marks.set(letter, line).then_some(line)
    }

    /// The entry on the view's middle row in the last frame, the last one before any frame
    fn view_middle_line(&self) -> usize {
        let last = self.local_history.len().saturating_sub(1);
        self.middle_line.unwrap_or(last).min(last)
    }

    pub fn remove_mark(&mut self, letter: char) -> Option<usize> {
        self.marks.remove(letter)
    }

    /// Goes to mark `letter`'s entry: the copy cursor moves to its start in copy mode, else the
    /// next frame centers the view on it. None when the mark isn't set
    pub fn jump_to_mark(&mut self, letter: char) -> Option<usize> {
        let line = self.marks.get(letter)?;
        self.go_to_line(line);
        Some(line)
    }

    /// Goes to the closest mark below (`forward`) or above the copy cursor in copy mode, else
    /// the view's middle row; returns its letter, none when there's none that way
    pub fn jump_to_adjacent_mark(&mut self, forward: bool) -> Option<char> {
        let from = if self.copy_state.active {
            self.copy_state.cursor.line_idx
        } else {
            self.view_middle_line()
        };
        let (letter, line) = if forward {
            self.marks.next(from)
        } else {
            self.marks.prev(from)
        }?;
        self.go_to_line(line);
        Some(letter)
    }

    fn go_to_line(&mut self, line: usize) {
        if self.copy_state.active {
            let cursor = &mut self.copy_state.cursor;
            cursor.line_idx = line;
            cursor.char_idx = 0;
            cursor.preferred_col = 0;
            cursor.byte_idx = 0;
            self.copy_reveal = Some(Reveal::Center);
        } else {
            self.mark_jump = Some(line);
        }
    }

    /// The entry a mark jump asked to center the view on
    #[inline]
    pub fn take_mark_jump(&mut self) -> Option<usize> {
        self.mark_jump.take()
    }

    #[inline]
    pub fn set_middle_line(&mut self, line: usize) {
        self.middle_line = Some(line);
    }

    /// The `clear` command: the live view starts on the next entry (the partial RX line, a
    /// prompt, while it isn't ended), blank rows under it until new lines fill them, the
    /// entries before it a scroll up away (MAX-176)
    pub fn clear_view(&mut self) {
        self.scroll_to_bottom();
        self.view_floor = Some(ViewFloor {
            line_idx: self.local_history.len(),
            rows_above: 0,
        });
    }
    #[inline]
    pub fn view_floor(&self) -> Option<ViewFloor> {
        self.view_floor
    }
    /// Kept by the render while the view is on it, dropped once the view is past it
    #[inline]
    pub fn set_view_floor(&mut self, floor: Option<ViewFloor>) {
        self.view_floor = floor;
    }

    /// Read-only slice/iterator of completed lines
    #[inline]
    pub fn history(&self) -> &VecDeque<LogEntry> {
        &self.local_history
    }

    /// Read-only view of current incoming RX slice
    #[inline]
    pub fn partial_rx(&self) -> &[u8] {
        self.partial_line.line()
    }

    pub fn history_up(&mut self) {
        if self.local_send_history.is_empty() {
            return;
        }

        match self.history_index {
            None => {
                self.temp_input = self.user_input.clone();
                let last = self.local_send_history.len() - 1;
                self.history_index = Some(last);
                let text = self.local_send_history[last].clone();
                self.user_input = Input::new(text);
            }
            Some(i) if i > 0 => {
                let next = i - 1;
                self.history_index = Some(next);
                let text = self.local_send_history[next].clone();
                self.user_input = Input::new(text);
            }
            _ => {}
        }
    }

    pub fn history_down(&mut self) {
        match self.history_index {
            Some(i) if i + 1 < self.local_send_history.len() => {
                let next = i + 1;
                self.history_index = Some(next);
                let text = self.local_send_history[next].clone();
                self.user_input = Input::new(text);
            }
            Some(_) => {
                self.history_index = None;
                self.user_input = std::mem::take(&mut self.temp_input);
            }
            None => {}
        }
    }

    pub fn update_action_lineinput(
        &mut self,
        settings: &SettingsConfig,
        action: LineInputAction,
    ) -> UpdateEffect {
        match action {
            LineInputAction::Submit => {
                self.scroller.scroll_to_bottom();

                let text = self.user_input.value().to_string();
                self.user_input.reset();
                self.temp_input.reset();
                self.history_index = None;

                if !text.is_empty() && self.local_send_history.back() != Some(&text) {
                    self.local_send_history.push_back(text.clone());
                    if self.local_send_history.len() > self.max_send_lines {
                        self.local_send_history.pop_front();
                    }
                }

                if settings.local_echo {
                    self.push_history(text.clone());
                }

                // consumming string!
                let mut bytes = text.into_bytes();
                bytes.extend_from_slice(settings.line_mode.tx_eol.as_bytes());

                UpdateEffect::Transmit(bytes)
            }
            LineInputAction::HistoryUp => {
                self.history_up();
                UpdateEffect::None
            }
            LineInputAction::HistoryDown => {
                self.history_down();
                UpdateEffect::None
            }
            LineInputAction::AutoResizeRemoteToWidth => {
                let gutter = settings.line_gutter_width();
                let cols = self.scroller.view_width.saturating_sub(gutter).max(1);
                let cmd = format!("stty cols {cols}; export COLUMNS={cols}\n");
                UpdateEffect::Transmit(cmd.into_bytes())
            }
            LineInputAction::ClearLine => {
                self.user_input.reset();
                UpdateEffect::None
            }
            LineInputAction::Interrupt if self.user_input.value().is_empty() => {
                UpdateEffect::Transmit(vec![0x03])
            }
            LineInputAction::Interrupt => {
                self.user_input.reset();
                self.history_index = None;
                UpdateEffect::None
            }
            LineInputAction::ClickAt(column) => {
                click_input_at(&mut self.user_input, column);
                UpdateEffect::None
            }
            LineInputAction::HandleKey(key_event) => {
                self.history_index = None;
                dispatch_input_key_vi(&mut self.user_input, &mut self.vi_state_prompt, key_event);
                UpdateEffect::None
            }
            LineInputAction::Paste(text) => {
                self.history_index = None;
                let text = text.replace("\r\n", "\n");
                let mut lines = text.split(['\r', '\n']);
                let mut current = lines.next().unwrap_or_default();
                let mut tx = Vec::new();
                for next in lines {
                    paste_into_input(&mut self.user_input, current);
                    if let UpdateEffect::Transmit(bytes) =
                        self.update_action_lineinput(settings, LineInputAction::Submit)
                    {
                        tx.extend_from_slice(&bytes);
                    }
                    current = next;
                }
                paste_into_input(&mut self.user_input, current);
                if tx.is_empty() {
                    UpdateEffect::None
                } else {
                    UpdateEffect::Transmit(tx)
                }
            }
        }
    }

    pub fn handle_copy_action(
        &mut self,
        action: CopyModeAction,
        settings: &SettingsConfig,
    ) -> Option<String> {
        let display_mode = settings.line_mode.display_mode;
        let wrap_mode = settings.line_mode.wrap_mode;
        let gutter_width = settings.line_gutter_width();

        if let CopyModeAction::HandleKey(key) = action {
            let display_text = self
                .local_history
                .get(self.copy_state.cursor.line_idx)
                .map(|e| entry_display_text(e, display_mode))
                .unwrap_or_default();
            if let Some(sub_action) = dispatch_copy_key_vi(
                &mut self.vi_state_copymode,
                &display_text,
                self.copy_state.cursor.char_idx,
                key,
            ) {
                return self.handle_copy_action(sub_action, settings);
            }
            return None;
        }

        let is_mouse = matches!(
            action,
            CopyModeAction::MousePress(..)
                | CopyModeAction::MouseDrag(..)
                | CopyModeAction::MouseRelease
        );

        // entering copy mode leaves the view where it is (MAX-143): a scrolled view gets the
        // cursor on its bottom row. From the live view the cursor, on the first row of the last
        // entry, is shown: that entry can be taller than the view (MAX-123)
        if action == CopyModeAction::Enter {
            self.copy_held = false;
            let max_line = self.local_history.len().saturating_sub(1);
            let live = self.scroller.offset_y == 0 && self.rows_below == 0;
            match self.bottom_row_start {
                Some((line, char_idx)) if !live && line <= max_line => {
                    self.copy_state.enter_at(line, char_idx);
                }
                _ => {
                    self.copy_state.enter_at(max_line, 0);
                    self.copy_reveal = Some(Reveal::Edge);
                }
            }
            return None;
        }
        let reveal = Reveal::for_move(&action, self.copy_cursor_shown);

        // adapter for the TextSource trait
        let mut source = LineModeTextSource {
            history: &self.local_history,
            display_mode,
            raw_view: settings.raw_side.view,
            view_height: self.scroller.view_height,
        };

        let result = self.copy_state.apply_action(action, &mut source);
        // out of copy mode (Esc, a yank), a view it held goes back live (MAX-193)
        if !self.copy_state.active && std::mem::take(&mut self.copy_held) {
            self.scroll_to_bottom();
        }
        match result {
            // the mouse moves the cursor to where it is shown
            CopyActionResult::Moved if is_mouse => None,
            CopyActionResult::Moved => {
                self.copy_reveal = Some(reveal);

                if wrap_mode == WrapMode::Truncate {
                    self.sync_pan_with_cursor(display_mode, gutter_width);
                }
                None
            }
            CopyActionResult::Yanked(text) => Some(text),
            _ => None,
        }
    }

    fn sync_pan_with_cursor(&mut self, display_mode: LineDisplayMode, gutter_width: usize) {
        let view_w = self.scroller.view_width;
        if view_w == 0 {
            return;
        }

        let cursor_char_idx = self.copy_state.cursor.char_idx;
        let content_width =
            if let Some(entry) = self.local_history.get(self.copy_state.cursor.line_idx) {
                let display_text = entry_display_text(entry, display_mode);
                let chars_before: String = display_text.chars().take(cursor_char_idx).collect();
                unicode_width::UnicodeWidthStr::width(chars_before.as_str())
            } else {
                cursor_char_idx
            };

        let total_cursor_col = gutter_width + content_width;

        if total_cursor_col < self.scroller.offset_x + gutter_width {
            self.scroller.offset_x = total_cursor_col.saturating_sub(gutter_width);
        } else if total_cursor_col >= self.scroller.offset_x + view_w {
            self.scroller.offset_x = total_cursor_col - view_w + 1;
        }

        self.scroller.offset_x = self.scroller.offset_x.min(self.scroller.max_pan_x);
    }

    pub fn update_search_matches(&mut self, display_mode: LineDisplayMode, raw_view: RawSideView) {
        self.search_state
            .update_matches_from_entries(&self.local_history, display_mode, raw_view);
    }

    pub fn handle_search_action(&mut self, action: SearchModeAction, settings: &SettingsConfig) {
        let display_mode = settings.line_mode.display_mode;
        let raw_view = settings.raw_side.view;
        match action {
            SearchModeAction::Start => {
                self.search_state.toggle_itself();
                if self.search_state.active {
                    self.update_search_matches(display_mode, raw_view);
                }
            }
            SearchModeAction::Next => {
                if self.search_state.active
                    && let Some((row, col)) = self.search_state.next_match()
                {
                    self.copy_state.cursor.line_idx = row;
                    self.copy_state.cursor.char_idx = col;
                    self.copy_state.cursor.preferred_col = col;
                }
            }
            SearchModeAction::Previous => {
                if self.search_state.active
                    && let Some((row, col)) = self.search_state.prev_match()
                {
                    self.copy_state.cursor.line_idx = row;
                    self.copy_state.cursor.char_idx = col;
                    self.copy_state.cursor.preferred_col = col;
                }
            }
            SearchModeAction::Cancel => {
                self.search_state.toggle(false);
            }
            SearchModeAction::Confirm => {
                let match_pos = self.search_state.current_match_position();
                self.search_state.toggle(false);

                if let Some((row, col)) = match_pos {
                    self.copy_state.enter_at(row, col);
                } else {
                    self.copy_state.enter_at(0, 0);
                }
                self.copy_reveal = Some(Reveal::Center);
            }
            SearchModeAction::HandleKey(key_event) => {
                if self.search_state.active {
                    self.handle_input_search(key_event, display_mode, raw_view);
                }
            }
            SearchModeAction::Paste(text) => {
                if self.search_state.active {
                    paste_into_input(&mut self.search_state.query, &text);
                    self.update_search_matches(display_mode, raw_view);
                }
            }
            SearchModeAction::Query(text) => {
                self.search_state.toggle(true);
                self.search_state.query = Input::new(text);
                self.update_search_matches(display_mode, raw_view);
            }
            SearchModeAction::Kind => {
                self.search_state.toggle_mode();
                self.update_search_matches(display_mode, raw_view);
            }
            SearchModeAction::ClickAt(column) => {
                click_input_at(&mut self.search_state.query, column);
            }
        }
    }

    pub fn handle_input_search(
        &mut self,
        key_event: KeyEvent,
        display_mode: LineDisplayMode,
        raw_view: RawSideView,
    ) {
        let before = self.search_state.query.value().to_owned();
        dispatch_input_key(&mut self.search_state.query, key_event);
        if self.search_state.query.value() != before {
            self.update_search_matches(display_mode, raw_view);
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
    pub fn is_prompt_vi(&self) -> bool {
        self.vi_state_prompt.mode == ViMode::Normal
    }

    pub fn update_viewport(&mut self, wrap_mode: WrapMode, viewport: ViewportSize) {
        self.scroller.update_view_size(viewport.cols, viewport.rows);
        if wrap_mode == WrapMode::Word || wrap_mode == WrapMode::Char {
            self.scroller.offset_x = 0;
        }
    }

    pub fn update_limits(&mut self, max_scroll: usize, max_pan: usize) {
        self.scroller.update_limits(max_scroll, max_pan);
    }

    #[inline]
    pub fn scroll_offset(&self) -> usize {
        self.scroller.offset_y
    }
    #[inline]
    pub fn x_offset(&self) -> usize {
        self.scroller.offset_x
    }
    #[inline]
    pub fn can_pan_left(&self) -> Option<usize> {
        self.scroller.can_pan_left()
    }
    #[inline]
    pub fn can_pan_right(&self) -> Option<(usize, usize)> {
        self.scroller.can_pan_right()
    }
    #[inline]
    pub fn is_new_content_missed(&self) -> bool {
        self.scroller.new_content_missed
    }

    #[inline]
    pub fn scroll_up(&mut self, rows: usize) {
        self.pending_scroll_rows = self.pending_scroll_rows.saturating_add_unsigned(rows);
    }
    #[inline]
    pub fn scroll_down(&mut self, rows: usize) {
        self.pending_scroll_rows = self.pending_scroll_rows.saturating_sub_unsigned(rows);
    }
    #[inline]
    pub fn scroll_up_page(&mut self) {
        self.scroll_up(self.scroller.view_height.saturating_sub(1));
    }
    #[inline]
    pub fn scroll_down_page(&mut self) {
        self.scroll_down(self.scroller.view_height.saturating_sub(1));
    }
    #[inline]
    pub fn scroll_to_bottom(&mut self) {
        self.scroller.scroll_to_bottom();
        self.rows_below = 0;
        self.pending_scroll_rows = 0;
    }
    /// Every row of the first entry hidden below: the render clamps that to the first row of
    /// the history at the top of the view
    #[inline]
    pub fn scroll_to_top(&mut self) {
        self.scroller.scroll_to_top();
        self.rows_below = usize::MAX;
        self.pending_scroll_rows = 0;
    }
    /// Scrolls to the last row of the entry `lines` entries from the bottom
    #[inline]
    /// To `rows` above the live view, as the scroll status counts them (MAX-142): the entry
    /// holding the view's bottom row then, and its rows below it, from the heights the last
    /// frame counted. If they aren't all counted, by rows from where the last frame was (the
    /// render moves at most a view height per frame)
    pub fn scroll_to_row(&mut self, rows: usize) {
        let rows = rows.min(self.row_scroll_status.max_scroll);
        if rows == 0 {
            self.scroll_to_bottom();
            return;
        }
        let mut below = 0;
        for (idx, &height) in self.row_heights.iter().enumerate().rev() {
            let height = height as usize;
            if height == 0 {
                break;
            }
            if below + height > rows {
                self.set_scroll_position(self.row_heights.len() - 1 - idx, rows - below);
                self.pending_scroll_rows = 0;
                return;
            }
            below += height;
        }
        let drawn = self.row_scroll_status.offset_from_bottom;
        self.pending_scroll_rows = rows as isize - drawn as isize;
    }

    pub fn scroll_to(&mut self, lines: usize) {
        self.scroller.scroll_to(lines);
        self.rows_below = 0;
        self.pending_scroll_rows = 0;
    }
    #[inline]
    pub fn rows_below(&self) -> usize {
        self.rows_below
    }
    /// Rows the user scrolled since the last frame, for the render to apply
    #[inline]
    pub fn take_pending_scroll_rows(&mut self) -> isize {
        std::mem::take(&mut self.pending_scroll_rows)
    }
    /// How this frame brings the view to the copy cursor, if a move asked for it (MAX-143)
    #[inline]
    pub fn take_copy_reveal(&mut self) -> Option<Reveal> {
        self.copy_reveal.take()
    }
    #[inline]
    pub fn prompt_scroll(&self) -> usize {
        self.prompt_scroll
    }
    #[inline]
    pub fn set_prompt_scroll(&mut self, scroll: usize) {
        self.prompt_scroll = scroll;
    }
    #[inline]
    pub fn set_copy_cursor_shown(&mut self, shown: bool) {
        self.copy_cursor_shown = shown;
    }
    #[inline]
    pub fn set_bottom_row_start(&mut self, start: (usize, usize)) {
        self.bottom_row_start = Some(start);
    }
    /// Where the render resolved the scroll to
    pub fn set_scroll_position(&mut self, entries_from_bottom: usize, rows_below: usize) {
        self.scroller.scroll_to(entries_from_bottom);
        self.rows_below = rows_below;
    }
    /// Scroll position in rows (scrollbar, `Bot` / `%`, `SCROLL +n`), as the last frame
    /// resolved it (#5d)
    pub fn scroll_status(&self) -> ScrollStatus {
        self.row_scroll_status
    }
    pub fn set_scroll_status(&mut self, status: ScrollStatus) {
        self.row_scroll_status = status;
    }
    /// The entry heights for the render to fill and read: reset to "not counted" when `key`
    /// (what they depend on) changed since the last frame. Given back with `put_row_heights`
    pub fn take_row_heights(&mut self, key: u64) -> VecDeque<u32> {
        if key != self.row_heights_key {
            self.row_heights_key = key;
            self.row_heights.iter_mut().for_each(|height| *height = 0);
        }
        std::mem::take(&mut self.row_heights)
    }
    pub fn put_row_heights(&mut self, row_heights: VecDeque<u32>) {
        self.row_heights = row_heights;
    }
    #[inline]
    pub fn pan_to_right(&mut self, cols: usize) {
        self.scroller.pan_right(cols);
    }
    #[inline]
    pub fn pan_to_left(&mut self, cols: usize) {
        self.scroller.pan_left(cols);
    }

    #[inline]
    pub fn raw_pan_x_max(&self) -> usize {
        self.raw_scroller.max_pan_x
    }
    #[inline]
    pub fn raw_pan_x(&self) -> usize {
        self.raw_scroller.pan_x
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
    #[cfg(test)]
    pub fn scroller(&self) -> &ViewportScroller {
        &self.scroller
    }

    #[inline]
    pub fn scroller_mut(&mut self) -> &mut ViewportScroller {
        &mut self.scroller
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
}

struct LineModeTextSource<'a> {
    history: &'a VecDeque<LogEntry>,
    display_mode: LineDisplayMode,
    raw_view: RawSideView,
    view_height: usize,
}

impl<'a> TextSource for LineModeTextSource<'a> {
    fn total_lines(&mut self) -> usize {
        self.history.len()
    }

    fn get_line_text(&mut self, line_idx: usize) -> Option<String> {
        self.history
            .get(line_idx)
            .map(|e| entry_display_text(e, self.display_mode).into_owned())
    }

    fn get_line_raw_bytes(&mut self, line_idx: usize) -> Option<Vec<u8>> {
        self.history
            .get(line_idx)
            .map(|entry| entry.wire_bytes().to_vec())
    }

    fn get_line_raw_map(&mut self, line_idx: usize, raw_bytes: &[u8]) -> Vec<usize> {
        match self.history.get(line_idx) {
            Some(entry) => entry_display_to_wire_map(entry, self.display_mode),
            None => build_display_to_raw_byte_map(raw_bytes, self.display_mode),
        }
    }

    #[inline]
    fn raw_view(&mut self) -> RawSideView {
        self.raw_view
    }
    #[inline]
    fn display_mode(&mut self) -> LineDisplayMode {
        self.display_mode
    }

    fn view_height(&mut self) -> usize {
        self.view_height
    }
}

#[cfg(test)]
#[path = "../tests/app/line_mode.rs"]
mod tests;
