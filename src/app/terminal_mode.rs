use std::{
    borrow::Cow,
    collections::VecDeque,
    ops::Range,
    time::{Duration, Instant},
};

use chrono::{DateTime, Local};
use crossterm::event::KeyEvent;
use terminput::Encoding;
use terminput_crossterm::to_terminput_key;

use crate::{
    app::{
        UpdateEffect, ViewportSize,
        data_processing::build_display_to_raw_byte_map,
        marks::Marks,
        scroller::ViewportScroller,
        search::{SearchHost, SearchModeKind, SearchState, SearchView},
        selection::{
            CopyActionResult, CopyModeState, CopySide, LogicalPosition, Reveal, TextSource,
        },
        term_answers::{Answer, size_report},
    },
    capture::{Screenshot, TimedChunk},
    configs::{
        LineDisplayMode, SettingsConfig, TerminalDimensionMode, TextEncoding, settings::RawSideView,
    },
    inputs::{
        CopyModeAction, SideViewAction, TerminalAction, ViPending,
        local_dispatchers::dispatch_copy_key_vi,
    },
    invalid_bytes::{self, InvalidBytes, shown_string},
    overlays::MarkRow,
    raw_side::terminal::{
        Cell, Dump, DumpCopy, DumpResult, Write,
        tokens::{Effect, Kind},
        writes::Run,
    },
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

/// The earlier writes on the copy cursor's cell shown in the dump, past the one it shows: a
/// progress bar's cell can have thousands (MAX-256)
const OLD_GHOSTS: usize = 32;

/// The screen copy cursor's bytes in the dump, newest first, found for a cursor and the bytes
/// kept: `(line, column, stream end)` (MAX-256)
#[derive(Debug, Clone)]
struct Ghosts {
    at: (usize, usize, u64),
    bytes: Vec<Range<u64>>,
}

/// The dump copy cursor's cell on the screen, found for a cursor byte and the bytes kept:
/// `(offset, stream end)` (MAX-256)
#[derive(Debug, Clone, Copy)]
struct TraceCell {
    at: (u64, u64),
    cell: Option<((usize, usize), bool)>,
}

/// The screen copy selection's bytes in the dump, found for a selection and the bytes kept:
/// `(start, end, stream end)` (MAX-256)
#[derive(Debug, Clone)]
struct MirrorBytes {
    at: ((usize, usize), (usize, usize), u64),
    bytes: Vec<Range<u64>>,
}

/// The dump copy selection's cells on the screen, found for its bytes, the bytes kept and the
/// world lines shown (MAX-256)
#[derive(Debug, Clone)]
struct MirrorCells {
    at: (Range<u64>, u64, Range<usize>),
    cells: Vec<((usize, usize), bool)>,
}

/// Why `Y` on the screen copied the bytes vt100 rebuilds from its cells, not the ones received
/// (MAX-256): the byte trace is off, or none of the selection is traced (bytes dropped, text
/// moved)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UntracedYank {
    TraceOff,
    NotTraced,
}

/// How bytes reach the emulator (MAX-256): the device's, at `offset` in the dump's stream (none
/// when the byte trace is off: their runs aren't recorded) with add CR / add LF; or maxicom's own
/// (a restore's reset, the separator, `clear`), which aren't in the stream
#[derive(Debug, Clone, Copy)]
enum Feed {
    Device {
        offset: Option<u64>,
        add_cr: bool,
        add_lf: bool,
    },
    Own,
}

pub struct TerminalMode {
    vt100_parser: TerminalParser,
    max_content_scrollback: usize,
    max_history_lines: usize,
    search_state: SearchState,
    copy_state: CopyModeState,
    scroller: ViewportScroller,
    pan_offset_y: usize,
    /// The grid rows down to the cursor or the lowest row with content (MAX-140)
    live_bottom: usize,
    /// The grid columns up to the rightmost cell with content, measured when a shrink needs it
    /// and forgotten when the grid changes (MAX-140)
    content_cols: Option<u16>,
    vi_pending_copy: Option<ViPending>,
    flash_until: Option<Instant>,
    row_timestamps: VecDeque<Option<DateTime<Local>>>,
    /// The world lines' text a search reads, and the first line data may have changed since,
    /// re-read when the search next needs it: the rows above it went up into the scrollback,
    /// which doesn't change (MAX-218)
    search_text: Option<Vec<String>>,
    search_text_from: Option<usize>,
    /// Data came while the matches were shown: they're found again on the next frame
    search_stale: bool,
    /// The restore separator's world row, until the grid first takes the view's size: a
    /// taller grid pulls rows back from the scrollback and would move it down (MAX-176)
    restored_separator: Option<usize>,
    /// The last byte from the device was a CR, for add CR / add LF across chunks (MAX-181)
    rx_after_cr: bool,
    /// Invalid UTF-8 as stand-ins vt100 draws, a char cut between two reads kept (MAX-249)
    invalid_bytes: InvalidBytes,
    /// The device's bytes as received, the raw side panel's dump (MAX-252)
    dump: Dump,
    /// The screen scrolled up from live when last reconciled: going back down sends the dump
    /// live too (MAX-256)
    screen_scrolled: bool,
    /// The main screen's rows dropped from the top of the scrollback so far: a world row plus
    /// this is an absolute row, which scrolling doesn't change (MAX-256)
    rows_gone: u64,
    /// The screen copy cursor's bytes in the dump, as last found (MAX-256)
    trace_ghosts: Option<Ghosts>,
    /// The screen copy cursor the dump last followed to its bytes
    trace_followed: Option<(usize, usize)>,
    /// The dump copy cursor's cell on the screen, as last found (MAX-256)
    trace_cell: Option<TraceCell>,
    /// The dump copy cursor the screen last followed to its cell
    trace_followed_byte: Option<u64>,
    /// The screen copy selection's bytes in the dump, as last found (MAX-256)
    mirror_bytes: Option<MirrorBytes>,
    /// The dump copy selection's cells on the screen, as last found (MAX-256)
    mirror_cells: Option<MirrorCells>,
    /// `Y` on the screen copied the rebuilt bytes, and why: the app warns (MAX-256)
    untraced_yank: Option<UntracedYank>,
    /// Copy mode as a search found it when it opened, given back by Esc (MAX-252)
    search_from: Option<(CopyModeState, DumpCopy)>,
    /// The pan when copy mode started, given back when it ends (MAX-217)
    copy_pan: Option<usize>,
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

        TerminalMode {
            vt100_parser,
            max_content_scrollback: 0,
            max_history_lines,
            search_state,
            copy_state,
            scroller,
            pan_offset_y: 0,
            live_bottom: 1,
            content_cols: None,
            vi_pending_copy: None,
            flash_until: None,
            row_timestamps: VecDeque::from(vec![None; rows as usize]),
            search_text: None,
            search_text_from: None,
            search_stale: false,
            restored_separator: None,
            rx_after_cr: false,
            invalid_bytes: InvalidBytes::default(),
            dump: Dump::new(max_history_lines),
            rows_gone: 0,
            screen_scrolled: false,
            trace_ghosts: None,
            trace_followed: None,
            trace_cell: None,
            trace_followed_byte: None,
            mirror_bytes: None,
            mirror_cells: None,
            untraced_yank: None,
            search_from: None,
            copy_pan: None,
            copy_held: false,
            marks: Marks::default(),
        }
    }

    /// The device's bytes, with minicom's add CR / add LF when on (MAX-181); the stream keeps
    /// them as received (MAX-252), and with `trace` which cells they drew (MAX-256)
    pub fn process_device_rx(
        &mut self,
        encoding: TextEncoding,
        raw_bytes: &[u8],
        add_cr: bool,
        add_lf: bool,
        trace: bool,
    ) {
        let offset = self.dump.push(raw_bytes);
        let feed = Feed::Device {
            offset: trace.then_some(offset),
            add_cr,
            add_lf,
        };
        self.process_rx_at(encoding, raw_bytes, Some(Local::now()), feed);
    }

    /// Local echo (MAX-270): bytes sent shown on the screen as maxicom's own, not in the stream.
    /// Enter's CR shows as CR LF, as minicom does, and a DEL (Backspace) as BS: the cursor goes
    /// back, the next char writes over
    pub fn echo(&mut self, sent: &[u8]) {
        let mut shown = Vec::with_capacity(sent.len() + 1);
        let mut bytes = sent.iter().peekable();
        while let Some(&byte) = bytes.next() {
            match byte {
                b'\r' if bytes.peek() != Some(&&b'\n') => shown.extend_from_slice(b"\r\n"),
                0x7f => shown.push(0x08),
                _ => shown.push(byte),
            }
        }
        self.process_rx(TextEncoding::Utf8, &shown);
    }

    /// Local echo of a paste: its text with the line breaks sent, without paste markers
    pub fn echo_paste(&mut self, text: &str) {
        self.echo(&terminal_paste_bytes(text, false));
    }

    /// Bytes not from the device (maxicom's own, tests'): not in the stream, nothing recorded
    pub fn process_rx(&mut self, encoding: TextEncoding, raw_bytes: &[u8]) {
        self.process_rx_at(encoding, raw_bytes, Some(Local::now()), Feed::Own);
    }

    /// Bytes to the emulator: add CR / add LF applied, invalid bytes as stand-ins shown `�` like
    /// line mode, vt100 drops its own (MAX-249)
    fn feed(&mut self, encoding: TextEncoding, raw_bytes: &[u8], (add_cr, add_lf): (bool, bool)) {
        let raw_bytes = add_newlines(raw_bytes, add_cr, add_lf, &mut self.rx_after_cr);
        let utf8_bytes = match encoding {
            TextEncoding::Utf8 => self.invalid_bytes.utf8(&raw_bytes),
            TextEncoding::Ascii => invalid_bytes::ascii(&raw_bytes),
            TextEncoding::Latin1 | TextEncoding::Cp437 => encoding.to_utf8_bytes(&raw_bytes),
        };
        self.vt100_parser.process(&utf8_bytes);
    }

    /// `feed` for the stream's bytes from `offset`, cut where a printable run starts: the cursor
    /// there is the cell its first char goes to, recorded with the run (MAX-256). After a run
    /// and only styles (colors), the next run starts where it ended: no cut needed
    fn feed_runs(
        &mut self,
        encoding: TextEncoding,
        raw_bytes: &[u8],
        offset: u64,
        newlines: (bool, bool),
    ) {
        let tokens = self.dump.tokens(encoding, raw_bytes, offset);
        let (rows, cols) = self.get_vt100_size();
        let wide = u64::from(cols.max(1));
        let mut done = 0;
        // where the next char goes when known without asking vt100: (row, column, alternate)
        let mut next: Option<(u64, u64, bool)> = None;
        for token in &tokens {
            match token.kind {
                Kind::Text => {}
                Kind::Sequence(Effect::Style) => continue,
                _ => {
                    next = None;
                    continue;
                }
            }
            let (row, col, alternate) = match next {
                Some(at) => at,
                None => {
                    // a run can start in an earlier read: its bytes there are fed already
                    let start = token.start.saturating_sub(offset) as usize;
                    self.feed(encoding, &raw_bytes[done..start], newlines);
                    done = start;
                    // a broken char before the run, still held back, is drawn first; held
                    // bytes of a run that started in an earlier read are its first char
                    if token.start >= offset {
                        let broken = self.invalid_bytes.flush();
                        if !broken.is_empty() {
                            self.vt100_parser.process(&broken);
                        }
                    }
                    self.cursor_cell()
                }
            };
            let run = Run {
                start: token.start,
                end: token.end,
                row,
                col: col as u16,
                alternate,
            };
            let cells = self.dump.record(run, encoding, cols);
            // past the row's end the next char wraps, as vt100 places it; the cursor waits past
            // the last cell when a row is just full
            let (mut row, mut col) = (row, col + u64::from(cells));
            while col > wide {
                row += 1;
                col -= wide;
            }
            // the alternate screen scrolls its grid rows: past its bottom, ask vt100
            let fits = !alternate || row < u64::from(rows);
            next = fits.then_some((row, col, alternate));
        }
        self.feed(encoding, &raw_bytes[done..], newlines);
    }

    /// The cursor as a run's first cell: an absolute row on the main screen (rows evicted so far,
    /// the history's rows, the cursor's), a grid row on the alternate one
    fn cursor_cell(&self) -> (u64, u64, bool) {
        let screen = self.vt100_parser.screen();
        let (row, col) = screen.cursor_position();
        let alternate = screen.alternate_screen();
        let row = if alternate {
            u64::from(row)
        } else {
            let evicted = self.vt100_parser.callbacks().evicted as u64;
            self.rows_gone + evicted + screen.scrollback_len() as u64 + u64::from(row)
        };
        (row, u64::from(col), alternate)
    }

    /// The cell a world position is, as the writes count rows (MAX-256): on the alternate
    /// screen a grid row, else an absolute row
    fn cell_at(&self, line_idx: usize, col: usize) -> Cell {
        let alternate = self.vt100_screen().alternate_screen();
        let row = if alternate {
            line_idx.saturating_sub(self.max_content_scrollback) as u64
        } else {
            self.rows_gone + line_idx as u64
        };
        Cell {
            row,
            col: col.min(usize::from(u16::MAX)) as u16,
            alternate,
        }
    }

    /// A cell's world position, none when it isn't kept or its screen isn't shown
    fn line_of(&self, cell: Cell) -> Option<(usize, usize)> {
        let alternate = self.vt100_screen().alternate_screen();
        let rows = self.get_vt100_size().0 as usize;
        let line = if cell.alternate {
            let row = usize::try_from(cell.row).ok().filter(|&row| row < rows)?;
            self.max_content_scrollback + row
        } else {
            let line = usize::try_from(cell.row.checked_sub(self.rows_gone)?).ok()?;
            (line < self.max_content_scrollback + rows).then_some(line)?
        };
        (alternate == cell.alternate).then_some((line, usize::from(cell.col)))
    }

    /// Whether a cell still shows the char a write put there: an erase, a scroll region or an
    /// insert moves or removes it without telling (MAX-256)
    fn still_shows(&mut self, write: &Write) -> bool {
        let Some((line, col)) = self.line_of(write.cell) else {
            return false;
        };
        self.shows_on_line(line, &[(col, write.ch)])[0]
    }

    /// `still_shows` for chars on one world line, `(column, char)` each: the line read once
    fn shows_on_line(&mut self, line: usize, chars: &[(usize, char)]) -> Vec<bool> {
        let sb = self.max_content_scrollback;
        with_world_row(&mut self.vt100_parser, sb, line, |screen, view_row, _| {
            (chars.iter())
                .map(|&(col, ch)| {
                    // a `�` received shows as its stand-in (MAX-249)
                    let cell = screen.cell(view_row as u16, col as u16);
                    cell.is_some_and(|cell| invalid_bytes::shown(cell.contents()).starts_with(ch))
                })
                .collect()
        })
        .unwrap_or_else(|| vec![false; chars.len()])
    }

    /// The bytes making the screen copy selection (MAX-256), in byte order: of each selected
    /// cell, the bytes of the char it shows now (none for a cell showing no traced char), joined
    /// across the gaps that draw nothing (escape sequences, colors, line ends) but not across
    /// bytes that drew other text. Empty off copy mode, in the dump, without a selection
    pub fn selection_bytes(&mut self, encoding: TextEncoding) -> Vec<Range<u64>> {
        let state = &self.copy_state;
        if !state.active || state.side != CopySide::Text {
            return Vec::new();
        }
        let Some((start, end)) = state.normalized_selection() else {
            return Vec::new();
        };
        let at = (
            (start.line_idx, start.char_idx),
            (end.line_idx, end.char_idx),
            self.dump.stream().end(),
        );
        if let Some(found) = self.mirror_bytes.as_ref().filter(|found| found.at == at) {
            return found.bytes.clone();
        }
        let cols = self.get_vt100_size().1;
        let last_col = usize::from(cols.max(1)) - 1;
        let (first, last) = (
            self.cell_at(start.line_idx, 0),
            self.cell_at(end.line_idx, 0),
        );
        let rows = first.row..=last.row;
        let writes = (self.dump).newest_writes_on(rows, first.alternate, encoding, cols);
        let mut pieces = Vec::new();
        for line in start.line_idx..=end.line_idx {
            let from = if line == start.line_idx {
                start.char_idx
            } else {
                0
            };
            let to = if line == end.line_idx {
                end.char_idx
            } else {
                last_col
            };
            let row = self.cell_at(line, 0).row;
            let found: Vec<&Write> = (from..=to.min(last_col))
                .filter_map(|col| writes.get(&(row, col as u16)))
                .collect();
            let chars: Vec<_> = (found.iter())
                .map(|write| (usize::from(write.cell.col), write.ch))
                .collect();
            let shown = self.shows_on_line(line, &chars);
            let still_shown = found.iter().zip(shown).filter(|&(_, shown)| shown);
            pieces.extend(still_shown.map(|(write, _)| write.bytes.clone()));
        }
        pieces.sort_by_key(|piece| piece.start);
        let mut bytes: Vec<Range<u64>> = Vec::new();
        for piece in pieces {
            match bytes.last_mut() {
                Some(last)
                    if piece.start <= last.end || !self.dump.draws_in(last.end..piece.start) =>
                {
                    last.end = last.end.max(piece.end);
                }
                _ => bytes.push(piece),
            }
        }
        self.mirror_bytes = Some(MirrorBytes {
            at,
            bytes: bytes.clone(),
        });
        bytes
    }

    /// The cells the dump copy selection's bytes drew (MAX-256), on the world `lines` given
    /// (the ones shown), each with whether it still shows its char (else written over). Empty
    /// off copy mode, on the screen, without a selection
    pub fn selection_cells(
        &mut self,
        encoding: TextEncoding,
        lines: Range<usize>,
    ) -> Vec<((usize, usize), bool)> {
        let Some(copy) = self.dump_copy() else {
            return Vec::new();
        };
        let Some(selected) = copy.selection(&self.dump.bounds()) else {
            return Vec::new();
        };
        let at = (selected.clone(), self.dump.stream().end(), lines.clone());
        if let Some(found) = self.mirror_cells.as_ref().filter(|found| found.at == at) {
            return found.cells.clone();
        }
        let cols = self.get_vt100_size().1;
        let mut by_line: std::collections::BTreeMap<usize, Vec<(usize, char)>> =
            std::collections::BTreeMap::new();
        for write in self.dump.written_in(selected, encoding, cols) {
            if let Some((line, col)) = self.line_of(write.cell)
                && lines.contains(&line)
            {
                by_line.entry(line).or_default().push((col, write.ch));
            }
        }
        let mut cells = Vec::new();
        for (line, chars) in by_line {
            let shown = self.shows_on_line(line, &chars);
            cells.extend((chars.iter().zip(shown)).map(|(&(col, _), shown)| ((line, col), shown)));
        }
        self.mirror_cells = Some(MirrorCells {
            at,
            cells: cells.clone(),
        });
        cells
    }

    /// The bytes behind a world position (MAX-256): the write it shows now, then the earlier
    /// writes there, newest first, `max` in all. None when the newest write recorded there isn't
    /// what it shows (moved by a scroll region, erased): no bytes rather than wrong ones
    pub fn writes_at(
        &mut self,
        line_idx: usize,
        col: usize,
        encoding: TextEncoding,
        max: usize,
    ) -> Option<Vec<Write>> {
        let cell = self.cell_at(line_idx, col);
        let cols = self.get_vt100_size().1;
        let writes = self.dump.writes_to(cell, encoding, cols, max);
        let shown = writes.first()?;
        self.still_shows(&shown.clone()).then_some(writes)
    }

    /// The bytes behind the screen copy cursor (MAX-256), newest first: the write it shows,
    /// then the earlier ones on its cell, `OLD_GHOSTS` at most; none off copy mode, in the
    /// dump, or when the cell shows no write traced. And whether the cursor moved since the
    /// last call: the dump follows it then, to the bytes it shows (copy mode only, new bytes
    /// don't move it)
    pub fn trace_ghosts(&mut self, encoding: TextEncoding) -> (Vec<Range<u64>>, bool) {
        let state = &self.copy_state;
        if !state.active || state.side != CopySide::Text {
            return (Vec::new(), false);
        }
        let (line, col) = (state.cursor.line_idx, state.cursor.char_idx);
        let key = (line, col, self.dump.stream().end());
        let ghosts = match &self.trace_ghosts {
            Some(found) if found.at == key => found.bytes.clone(),
            _ => {
                let writes = self.writes_at(line, col, encoding, 1 + OLD_GHOSTS);
                let bytes: Vec<_> = writes.into_iter().flatten().map(|w| w.bytes).collect();
                self.trace_ghosts = Some(Ghosts {
                    at: key,
                    bytes: bytes.clone(),
                });
                bytes
            }
        };
        let moved = self.trace_followed != Some((line, col));
        self.trace_followed = Some((line, col));
        if moved && let Some(shown) = ghosts.first() {
            self.dump.keep_in_middle(shown.start);
        }
        (ghosts, moved)
    }

    /// The cell the dump copy cursor's byte drew (MAX-256), as a world position, and whether it
    /// still shows that byte's char (else written over); none off copy mode, on the screen's
    /// side, for a byte that draws nothing or a cell no longer kept. When the dump cursor moved
    /// since the last call, the screen follows it to that cell (copy mode only)
    pub fn trace_cell(&mut self, encoding: TextEncoding) -> Option<((usize, usize), bool)> {
        let offset = self.dump_copy()?.cursor;
        let key = (offset, self.dump.stream().end());
        let cell = match &self.trace_cell {
            Some(found) if found.at == key => found.cell,
            _ => {
                let cell = self.written_at(offset, encoding);
                self.trace_cell = Some(TraceCell { at: key, cell });
                cell
            }
        };
        if self.trace_followed_byte != Some(offset) {
            self.trace_followed_byte = Some(offset);
            if let Some(((line, col), _)) = cell {
                // kept in the middle half of the rows shown, else centered, as the dump does;
                // the pan as for the copy cursor
                let (top, bottom) = self.visible_lines();
                let margin = (bottom - top + 1) / 4;
                if !(top + margin..=bottom - margin).contains(&line) {
                    self.scroll_to_target_line(line, self.scroller.view_height);
                }
                self.reveal_at(line, col, Reveal::Edge);
            }
        }
        cell
    }

    /// Why the last `Y` on the screen copied the screen's rebuilt bytes, not the ones received
    /// (MAX-256), once
    pub fn take_untraced_yank(&mut self) -> Option<UntracedYank> {
        self.untraced_yank.take()
    }

    /// Where byte `offset` drew (MAX-256): its world position and whether it still shows there.
    /// None for a byte that draws nothing, or a cell no longer kept or on the screen not shown
    pub fn written_at(
        &mut self,
        offset: u64,
        encoding: TextEncoding,
    ) -> Option<((usize, usize), bool)> {
        let cols = self.get_vt100_size().1;
        let write = self.dump.written(offset, encoding, cols)?;
        let at = self.line_of(write.cell)?;
        Some((at, self.still_shows(&write)))
    }

    /// Bytes to the emulator, received at `time` (none when it isn't known: the rows the cursor
    /// goes through get no time then, MAX-175)
    fn process_rx_at(
        &mut self,
        encoding: TextEncoding,
        raw_bytes: &[u8],
        time: Option<DateTime<Local>>,
        feed: Feed,
    ) {
        self.scroller.on_rx_received();

        let started_on_main = !self.vt100_parser.screen().alternate_screen();
        let cursor_row_before = self.vt100_parser.screen().cursor_position().0 as usize;
        let live_rows_before = self.live_rows();
        let lines_before = self.max_content_scrollback;

        match feed {
            Feed::Device {
                offset: Some(offset),
                add_cr,
                add_lf,
            } => self.feed_runs(encoding, raw_bytes, offset, (add_cr, add_lf)),
            Feed::Device {
                offset: None,
                add_cr,
                add_lf,
            } => self.feed(encoding, raw_bytes, (add_cr, add_lf)),
            Feed::Own => self.feed(encoding, raw_bytes, (false, false)),
        }
        let evicted = std::mem::take(&mut self.vt100_parser.callbacks_mut().evicted);
        self.rows_gone += evicted as u64;
        self.search_text_changed(started_on_main, lines_before, evicted);
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

    /// The view's size, and the grid's from it; `bar_rows` under the view, which the search bar
    /// takes, are still the grid's: the remote isn't resized, the view pans to the rows hidden
    /// (MAX-218)
    pub fn update_viewport(
        &mut self,
        viewport: ViewportSize,
        bar_rows: u16,
        dim_mode: TerminalDimensionMode,
        conf_rows: u16,
        conf_cols: u16,
    ) {
        self.scroller.update_view_size(viewport.cols, viewport.rows);

        let avail_h = viewport.rows as u16 + bar_rows;
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
        self.dump.clear();
        self.forget_search_text();
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

    /// Puts mark `letter` on the cursor's row, where the bytes are being received (MAX-220)
    pub fn mark_cursor_row(&mut self, letter: char) {
        let row = self.max_content_scrollback + self.vt100_screen().cursor_position().0 as usize;
        self.marks.set(letter, row);
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
                    text: shown_string(text.unwrap_or_default()),
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

        // the screen scrolled back down to live: the dump goes back live with it
        let scrolled = self.scroller.offset_y > 0;
        if self.screen_scrolled && !scrolled {
            self.dump_back_live();
        }
        self.screen_scrolled = scrolled;
    }

    /// The dump back to its live end, as the screen goes live (MAX-256); not while copy mode's
    /// cursor is in it, which the screen follows
    fn dump_back_live(&mut self) {
        if self.dump_copy().is_none() {
            self.dump.follow();
        }
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

    /// The screen back live (a key typed, a paste, `Shift-End`...): the dump too
    pub fn scroll_to_bottom(&mut self) {
        self.scroller.scroll_to_bottom();
        self.reconcile_both_vertical_scroll();
        self.dump_back_live();
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
        let below = self.rows_below_middle(view_height);
        self.scroll_to_bottom_line(target_line_idx + below);
    }

    /// The rows under a centered line: `h / 2` above it, as line mode centers, so an even
    /// view puts it on the lower of its two middle rows (MAX-218)
    fn rows_below_middle(&self, view_height: usize) -> usize {
        let shown = (self.get_vt100_size().0 as usize).min(view_height);
        shown.saturating_sub(1) - shown / 2
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

        self.forget_search_text();
        let scrollback_before = self.max_content_scrollback;
        let cursor_row_before = self.vt100_parser.screen().cursor_position().0 as usize;
        // the parser's `set_size` (not the screen's) reports the rows it evicts
        self.vt100_parser.set_size(vt100_dim(rows), vt100_dim(cols));
        let events = self.vt100_parser.callbacks_mut();
        let evicted = std::mem::take(&mut events.evicted);
        self.rows_gone += evicted as u64;
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
        self.text_lines_from(0)
    }

    /// The text of the world lines from `from` on. The alternate screen has no scrollback: the
    /// main one's lines read blank then, so the screen's rows keep their world line (MAX-218)
    fn text_lines_from(&mut self, from: usize) -> Vec<String> {
        let max_sb = self.max_content_scrollback;
        let (rows, cols) = self.get_vt100_size();
        let current_sb = self.vt100_parser.screen().scrollback();
        let alternate = self.vt100_parser.screen().alternate_screen();

        let mut lines = Vec::with_capacity((max_sb + rows as usize).saturating_sub(from));

        let mut sb = max_sb - from.min(max_sb);
        while sb > 0 {
            if alternate {
                lines.push(String::new());
            } else {
                self.vt100_parser.screen_mut().set_scrollback(sb);
                if let Some(first_row) = self.vt100_parser.screen().rows(0, cols).next() {
                    lines.push(shown_string(first_row));
                }
            }
            sb -= 1;
        }

        self.vt100_parser.screen_mut().set_scrollback(0);
        let screen_from = from.saturating_sub(max_sb);
        for row in self.vt100_parser.screen().rows(0, cols).skip(screen_from) {
            lines.push(shown_string(row));
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

    pub fn handle_copy_action(
        &mut self,
        action: CopyModeAction,
        settings: &SettingsConfig,
    ) -> Option<String> {
        if let CopyModeAction::HandleKey(key) = action {
            let line_text = self.copy_line_text();
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
        let raw_view = settings.raw_side.view;
        if action == CopyModeAction::ToggleSide {
            self.toggle_dump_side(raw_view, settings.encoding);
            return None;
        }
        // copy mode starting (a key or a mouse press): its pan comes back when it ends
        if !self.copy_state.active {
            self.copy_pan = Some(self.scroller.offset_x);
        }
        // the dump's side (MAX-252), until a press on the screen
        let on_dump = match action {
            CopyModeAction::MousePress(_, side, _) => side == CopySide::Raw,
            _ => self.dump_copy().is_some(),
        };
        if on_dump {
            return self.dump_copy_action(action, raw_view);
        }

        if action == CopyModeAction::Enter {
            self.copy_held = false;
            let (c_row, c_col) = self.vt100_screen().cursor_position();
            let max_col = self.get_vt100_size().1.saturating_sub(1) as usize;
            // entering copy mode leaves the view where it is (MAX-143): on the remote cursor
            // from the live view, else on the start of the view's bottom row
            let cursor_line = self.max_content_scrollback + (c_row as usize);
            if self.scroller.offset_y > 0 {
                self.copy_state.enter_at(self.visible_lines().1, 0);
            } else if let Some(line) = self.last_line_with_content(cursor_line) {
                // a cursor on a blank row (output ended with a newline): the start of the
                // last row with content instead (MAX-218)
                self.copy_state.enter_at(line, 0);
            } else {
                self.copy_state
                    .enter_at(cursor_line, (c_col as usize).min(max_col));
            }
            return None;
        }
        // `Y` on the screen: the bytes received for the selection, in the raw panel's view (hex
        // with the panel closed), from the byte trace; without it, the screen's rebuilt bytes and
        // a warning (MAX-256)
        let raw_view = match (action == CopyModeAction::YankRaw, raw_view) {
            (true, RawSideView::Off) => RawSideView::Hex,
            _ => raw_view,
        };
        if action == CopyModeAction::YankRaw && self.copy_state.selection_anchor.is_some() {
            let traced = match settings.terminal_mode.byte_trace {
                true => self.selection_bytes(settings.encoding),
                false => Vec::new(),
            };
            if !traced.is_empty() {
                let text = (traced.into_iter())
                    .map(|bytes| self.dump.yank_text(bytes, false, raw_view))
                    .collect::<Vec<_>>()
                    .join("\n");
                self.copy_state.exit();
                self.copy_mode_ended();
                return Some(text);
            }
            self.untraced_yank = Some(match settings.terminal_mode.byte_trace {
                true => UntracedYank::NotTraced,
                false => UntracedYank::TraceOff,
            });
        }
        let reveal = Reveal::for_move(&action, self.copy_cursor_shown());

        // adapter for the TextSource trait
        let mut source = TerminalModeTextSource {
            parser: &mut self.vt100_parser,
            max_content_scrollback: self.max_content_scrollback,
            live_bottom: self.live_bottom,
            raw_view,
            view_height: self.scroller.view_height,
        };

        let result = self.copy_state.apply_action(action, &mut source);
        if !self.copy_state.active {
            self.copy_mode_ended();
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
        let cursor = self.copy_state.cursor;
        self.reveal_at(cursor.line_idx, cursor.char_idx, reveal);
    }

    /// Brings the view to a world position, as `reveal_copy_cursor` does to the copy cursor
    fn reveal_at(&mut self, cursor_row: usize, cursor_col: usize, reveal: Reveal) {
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

    /// After data: the search text's lines from the first one it may have changed are to be
    /// read again, all of them when the remote went to or from the alternate screen; shown
    /// matches are found again on the next frame (MAX-218). Without a search open or kept, the
    /// text is dropped, as big as the history
    fn search_text_changed(&mut self, started_on_main: bool, lines_before: usize, evicted: usize) {
        let on_main = !self.vt100_parser.screen().alternate_screen();
        let searching = self.search_state.active || self.search_state.kept;
        if started_on_main != on_main || !searching {
            self.forget_search_text();
            return;
        }
        self.search_stale |= self.search_state.shows_matches();
        let Some(text) = self.search_text.as_mut() else {
            return;
        };
        let evicted = evicted.min(text.len());
        text.drain(..evicted);
        let from = lines_before.saturating_sub(evicted);
        let earlier = self.search_text_from.map(|f| f.saturating_sub(evicted));
        self.search_text_from = Some(earlier.map_or(from, |f| f.min(from)));
    }

    /// Every line of the search text to read again (a new grid, a new size), the shown matches
    /// found again on the next frame
    fn forget_search_text(&mut self) {
        self.search_text = None;
        self.search_text_from = None;
        self.search_stale |= self.search_state.shows_matches();
    }

    /// Once a frame: the matches shown found again when data came (MAX-218). The current one
    /// stays the copy cursor's, unless the bar is open: then it follows the query as typed
    pub fn refresh_stale_matches(&mut self, raw_view: RawSideView) {
        if !self.search_stale {
            return;
        }
        let typing = self.search_state.active;
        self.update_search_matches(raw_view);
        if !typing {
            let cursor = self.match_cursor();
            self.search_state.select_at(cursor);
            // the view stays where it is: only typing jumps to a match
            self.search_state.target_jump = None;
        }
    }

    pub fn handle_side_view_action(&mut self, action: SideViewAction, raw_view: RawSideView) {
        match action {
            SideViewAction::PanLeft(bytes) => self.dump.pan_left(bytes, raw_view),
            SideViewAction::PanRight(bytes) => self.dump.pan_right(bytes, raw_view),
            SideViewAction::ScrollUp(lines) => self.dump.scroll_up(lines),
            SideViewAction::ScrollDown(lines) => self.dump.scroll_down(lines),
            SideViewAction::PageUp => self.dump.scroll_up(self.dump.page()),
            SideViewAction::PageDown => self.dump.scroll_down(self.dump.page()),
        }
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

    pub fn copy_state_mut(&mut self) -> &mut CopyModeState {
        &mut self.copy_state
    }

    /// When world line `line` is blank, the last line above it with content (MAX-218)
    fn last_line_with_content(&mut self, line: usize) -> Option<usize> {
        let mut has_content = |idx| {
            read_vt100_world_line_text(&mut self.vt100_parser, self.max_content_scrollback, idx)
                .is_some_and(|text| !text.trim().is_empty())
        };
        if has_content(line) {
            return None;
        }
        (0..line).rev().find(|&idx| has_content(idx))
    }

    /// The text of the copy cursor's line
    pub fn copy_line_text(&mut self) -> String {
        read_vt100_world_line_text(
            &mut self.vt100_parser,
            self.max_content_scrollback,
            self.copy_state.cursor.line_idx,
        )
        .unwrap_or_default()
    }

    #[inline]
    pub fn scroller(&self) -> &ViewportScroller {
        &self.scroller
    }

    #[inline]
    pub fn dump(&self) -> &Dump {
        &self.dump
    }

    #[inline]
    pub fn dump_mut(&mut self) -> &mut Dump {
        &mut self.dump
    }

    /// The pan showing the dump cursor's byte in a body `cols` wide, while copy mode is there
    pub fn pan_to_dump_cursor(&mut self, raw_view: RawSideView, cols: usize) {
        if self.dump_copy().is_some() {
            self.dump.pan_to_cursor(raw_view, cols);
        }
    }

    /// The search's matches are in the dump: a raw search goes over the bytes received
    /// (MAX-252), a text search over the screen and the scrollback
    pub fn dump_search(&self) -> bool {
        self.search_state.mode_kind == SearchModeKind::RawBytes
    }

    /// Copy mode's cursor in the dump, while it's on the raw side
    pub fn dump_copy(&self) -> Option<&DumpCopy> {
        let state = &self.copy_state;
        (state.active && state.side == CopySide::Raw).then_some(self.dump.copy())
    }

    /// Copy mode switching to the dump: its cursor on the bytes the screen cursor's cell shows
    /// (MAX-256), else on the last byte of the bottom line shown (no bytes: it stays on the
    /// screen); back to the screen, on the dump cursor's cell when it still shows its byte, else
    /// where the screen cursor was. Neither side keeps the other's selection, neither view jumps
    fn toggle_dump_side(&mut self, raw_view: RawSideView, encoding: TextEncoding) {
        let state = &mut self.copy_state;
        if !state.active || raw_view == RawSideView::Off {
            return;
        }
        state.selection_anchor = None;
        state.line_select = false;
        if state.side == CopySide::Raw {
            let cursor = self.dump.copy().cursor;
            if let Some(((line, col), true)) = self.written_at(cursor, encoding) {
                self.copy_state.jump_to(line, col);
                self.trace_followed = Some((line, col));
            }
            self.copy_state.side = CopySide::Text;
            return;
        }
        let cursor = state.cursor;
        let traced = self.writes_at(cursor.line_idx, cursor.char_idx, encoding, 1);
        if let Some(shown) = traced.and_then(|writes| writes.into_iter().next()) {
            self.dump.set_copy(DumpCopy::at(shown.bytes.start));
            self.dump.reveal(shown.bytes.start);
            self.trace_followed_byte = Some(shown.bytes.start);
            self.copy_state.side = CopySide::Raw;
        } else if self.dump.start_copy() {
            self.copy_state.side = CopySide::Raw;
        }
    }

    /// A copy action on the dump's side (MAX-252): a press there starts it, a press on the
    /// screen leaves it for the screen's copy mode
    fn dump_copy_action(
        &mut self,
        action: CopyModeAction,
        raw_view: RawSideView,
    ) -> Option<String> {
        if let CopyModeAction::MousePress(..) = action {
            self.copy_state.active = true;
            self.copy_state.side = CopySide::Raw;
            self.copy_state.selection_anchor = None;
        }
        let text = match self.dump.copy_action(action) {
            DumpResult::Yank { bytes, as_received } => {
                Some(self.dump.yank_text(bytes, as_received, raw_view))
            }
            DumpResult::Exited => None,
            DumpResult::Moved | DumpResult::None => return None,
        };
        self.copy_state.exit();
        self.copy_mode_ended();
        text
    }

    /// Out of copy mode (Esc, a yank), a view it held goes back live (MAX-193), and the
    /// matches kept after a search are no longer shown (MAX-215)
    fn copy_mode_ended(&mut self) {
        self.search_state.kept = false;
        self.trace_ghosts = None;
        self.trace_followed = None;
        self.trace_cell = None;
        self.trace_followed_byte = None;
        self.mirror_bytes = None;
        self.mirror_cells = None;
        if let Some(pan) = self.copy_pan.take() {
            self.scroller.offset_x = pan;
        }
        if std::mem::take(&mut self.copy_held) {
            self.scroll_to_bottom();
        }
    }

    /// Replays a past session's received chunks, each row at the time of the chunk that reached
    /// it when known (MAX-175), then resets the modes they may have left set (the alternate
    /// screen, application cursor keys, mouse reports, bracketed paste, a scroll region, a
    /// hidden cursor, colors) so the keys and the mouse work. Then, like a `clear`, the screen
    /// goes up into the scrollback and `separator` is written dimmed on the top row, the cursor
    /// under it (MAX-176). With `trace`, which cells the chunks drew is recorded (MAX-256)
    pub fn restore(
        &mut self,
        encoding: TextEncoding,
        chunks: &[TimedChunk],
        separator: &str,
        trace: bool,
    ) {
        for chunk in chunks {
            let offset = self.dump.push(chunk.bytes);
            let feed = Feed::Device {
                offset: trace.then_some(offset),
                add_cr: false,
                add_lf: false,
            };
            self.process_rx_at(encoding, chunk.bytes, chunk.time, feed);
        }
        // leaving it restores a saved cursor, so only when the session is in it
        if self.vt100_parser.screen().alternate_screen() {
            self.process_rx_at(TextEncoding::Utf8, b"\x1b[?1049l", None, Feed::Own);
        }
        let (row, col) = self.vt100_parser.screen().cursor_position();
        let mut reset = String::from(RESTORE_RESET);
        reset.push_str(&format!("\x1b[{};{}H", row + 1, col + 1));
        // the cursor's line is kept: a new one only when the cursor isn't at the start of one
        if col > 0 {
            reset.push_str("\r\n");
        }
        self.process_rx_at(TextEncoding::Utf8, reset.as_bytes(), None, Feed::Own);

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
        self.process_rx_at(TextEncoding::Utf8, end.as_bytes(), None, Feed::Own);
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
            text: shown_string(screen.contents()),
            ansi: shown_string(screen.contents_formatted()),
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

    pub fn center_on_target_match(
        &mut self,
        target_row: usize,
        target_col: usize,
        view_w: usize,
        view_h: usize,
    ) {
        let (_, grid_cols) = self.get_vt100_size();
        let max_sb = self.max_content_scrollback;
        // the offsets count from the live view's last row, not the grid's
        let total_world_lines = max_sb + self.live_rows();

        if total_world_lines == 0 || target_row >= total_world_lines {
            return;
        }

        let lines_from_bottom = (total_world_lines - 1).saturating_sub(target_row);
        let target_scroll_y = lines_from_bottom.saturating_sub(self.rows_below_middle(view_h));

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
    // move leave are spaces, so a char index is a screen column (#33); a stand-in is one `�`
    with_world_row(parser, max_sb, world_row, |screen, view_row, cols| {
        shown_string(screen.rows(0, cols).nth(view_row).unwrap_or_default())
    })
}

/// vt100's formatted row (its text with the escape sequences of its styles), stand-ins in it:
/// [`raw_row`] gives the bytes the raw side panel shows (MAX-71)
pub fn read_vt100_world_line_formatted(
    parser: &mut TerminalParser,
    max_sb: usize,
    world_row: usize,
) -> Option<String> {
    with_world_row(parser, max_sb, world_row, |screen, view_row, cols| {
        screen
            .rows_formatted(0, cols)
            .nth(view_row)
            .unwrap_or_default()
    })
}

/// A row as the raw side panel shows it, from vt100's formatted row: its bytes, invalid bytes
/// as they were received (MAX-249), and for each byte of its text as shown (`rows()` through
/// [`shown_string`]) and its end, the raw byte it comes from. The map is built on the shown
/// text, valid UTF-8, then carried on to the bytes received, as line mode does (#20)
pub fn raw_row(formatted: &str) -> (Cow<'_, [u8]>, Vec<usize>) {
    let shown_text = invalid_bytes::shown(formatted);
    let map = build_display_to_raw_byte_map(shown_text.as_bytes(), LineDisplayMode::Pretty);
    match invalid_bytes::received(formatted) {
        None => (Cow::Borrowed(formatted.as_bytes()), map),
        Some((bytes, offsets)) => {
            let map = map.into_iter().map(|byte| offsets[byte]).collect();
            (Cow::Owned(bytes), map)
        }
    }
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
        let formatted =
            read_vt100_world_line_formatted(self.parser, self.max_content_scrollback, line_idx)?;
        Some(raw_row(&formatted).0.into_owned())
    }

    /// The map of the row's raw bytes, built on its formatted text: its bytes received are not
    /// UTF-8 when it holds invalid bytes (MAX-249)
    fn get_line_raw_map(&mut self, line_idx: usize, _raw_bytes: &[u8]) -> Vec<usize> {
        let formatted =
            read_vt100_world_line_formatted(self.parser, self.max_content_scrollback, line_idx);
        raw_row(&formatted.unwrap_or_default()).1
    }

    #[inline]
    fn raw_view(&mut self) -> RawSideView {
        self.raw_view
    }

    fn view_height(&mut self) -> usize {
        self.view_height
    }
}

impl SearchHost for TerminalMode {
    fn search_and_copy(&mut self) -> (&mut SearchState, &mut CopyModeState) {
        (&mut self.search_state, &mut self.copy_state)
    }

    /// From the screen and the scrollback's text, only the lines data may have changed read
    /// again (MAX-218)
    fn find_matches(&mut self, view: SearchView) {
        self.search_stale = false;
        if self.dump_search() {
            // no dump to show them in: no match, the app warns (MAX-252)
            let stream = self.dump.stream();
            let bytes = match view.raw_view {
                RawSideView::Off => (&[][..], &[][..]),
                _ => stream.slices(stream.start()..stream.end()),
            };
            (self.search_state).update_matches_from_bytes(bytes, stream.start(), view.raw_view);
            return;
        }
        match (self.search_text.is_some(), self.search_text_from.take()) {
            (false, _) => self.search_text = Some(self.all_text_lines()),
            (true, Some(from)) => {
                let fresh = self.text_lines_from(from);
                if let Some(text) = self.search_text.as_mut() {
                    text.truncate(from);
                    text.extend(fresh);
                }
            }
            (true, None) => {}
        }
        let text = self.search_text.as_deref().unwrap_or_default();
        self.search_state
            .update_matches_from_strings(text, view.raw_view);
    }

    fn center_copy_cursor(&mut self) {
        if self.copy_state.side == CopySide::Raw {
            let cursor = self.dump.copy().cursor;
            self.dump.cursor_to(cursor);
        } else {
            self.reveal_copy_cursor(Reveal::Center);
        }
    }

    /// Copy mode kept as it was, for Esc; the origin is its cursor on the side of the
    /// matches, none from the other side: `/` starts at the oldest match, `?` at the newest
    /// (MAX-252)
    fn search_origin(&mut self, opening: bool) -> Option<LogicalPosition> {
        if opening {
            let copy = &self.copy_state;
            self.search_from = copy.active.then(|| (copy.clone(), *self.dump.copy()));
        }
        let (copy, dump) = self.search_from.as_ref()?;
        match (copy.side, self.dump_search()) {
            (CopySide::Text, false) => Some(copy.cursor),
            (CopySide::Raw, true) => Some(LogicalPosition {
                line_idx: dump.cursor as usize,
                ..LogicalPosition::default()
            }),
            _ => None,
        }
    }

    /// The dump cursor's offset for a raw search, even from the screen
    fn match_cursor(&mut self) -> (usize, usize) {
        let cursor = self.copy_state.cursor;
        if self.dump_search() {
            (self.dump.copy().cursor as usize, 0)
        } else {
            (cursor.line_idx, cursor.char_idx)
        }
    }

    /// A raw match moves the dump cursor, a text match the screen's: copy mode switches to
    /// the match's side, neither side keeping the other's selection (MAX-252)
    fn cursor_to_match(&mut self, row: usize, col: usize) {
        let to_dump = self.dump_search();
        let state = &mut self.copy_state;
        if (state.side == CopySide::Raw) != to_dump {
            state.selection_anchor = None;
            state.line_select = false;
            state.side = if to_dump {
                CopySide::Raw
            } else {
                CopySide::Text
            };
            self.dump.set_copy(DumpCopy::default());
        }
        if to_dump {
            self.dump.cursor_to(row as u64);
        } else {
            self.copy_state.jump_to(row, col);
        }
    }

    fn back_to_origin(&mut self) -> bool {
        let Some((copy, dump)) = self.search_from.clone() else {
            return false;
        };
        self.copy_state = copy;
        self.dump.set_copy(dump);
        true
    }
}

#[cfg(test)]
#[path = "../tests/app/terminal_mode.rs"]
mod tests;
