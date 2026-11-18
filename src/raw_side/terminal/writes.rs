//! Which bytes wrote which cells (MAX-256): each printable run received, with the cell its first
//! char went to. A run's cells follow from its start by arithmetic, wraps included, so a run is
//! 16 bytes whatever its length. Kept in time order, dropped with the bytes or past a limit

use std::{collections::VecDeque, ops::Range};

// NOTE: review here

/// The alternate screen's flag, in a packed run's column
const ALTERNATE: u16 = 1 << 15;

/// A printable run: its bytes `start..end`, and where its first char went: an absolute row (rows
/// counted from the first one ever, so scrolling doesn't change it) and a column, on the main
/// screen or the alternate one (a grid row there, it has no scrollback). The column can be the
/// row's width: the cursor waits past the last cell, the first char wraps
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub start: u64,
    pub end: u64,
    pub row: u64,
    pub col: u16,
    pub alternate: bool,
}

/// A run as kept: its row from the log's base row (the alternate screen's as it is), its column
/// with the screen's flag
#[derive(Debug, Clone, Copy)]
struct Packed {
    start: u64,
    row: u32,
    len: u16,
    col: u16,
}

/// Where the last run's cells end, to extend it: its last byte, the cell after its last char
/// (a column past the row's width when it filled it) and its screen
#[derive(Debug, Clone, Copy)]
struct Tail {
    end: u64,
    row: u64,
    col: u64,
    alternate: bool,
}

#[derive(Debug)]
pub struct Writes {
    runs: VecDeque<Packed>,
    /// The main screen rows' base, set by the first main screen run
    row_base: Option<u64>,
    limit: usize,
    tail: Option<Tail>,
}

impl Writes {
    /// A log keeping at most `limit` runs, the oldest dropped first
    pub fn new(limit: usize) -> Self {
        Self {
            runs: VecDeque::new(),
            row_base: None,
            limit: limit.max(1),
            tail: None,
        }
    }

    /// A run taking `cells` cells on a screen `cols` wide. One going on from the last run (its
    /// bytes and its cells follow) makes it longer, so a run cut by small reads stays one. A run
    /// longer than a packed run holds keeps its first bytes only
    pub fn push(&mut self, run: Run, cells: u32, cols: u16) {
        let cols = u64::from(cols.max(1));
        let len = run.end - run.start;
        let next = |row: u64, col: u64| row * cols + col;
        if let Some(tail) = &mut self.tail
            && tail.end == run.start
            && tail.alternate == run.alternate
            && next(tail.row, tail.col) == next(run.row, u64::from(run.col))
            && let Some(last) = self.runs.back_mut()
            && u64::from(last.len) + len <= u64::from(u16::MAX)
        {
            last.len += len as u16;
            tail.end = run.end;
            tail.col += u64::from(cells);
            return;
        }
        let row = if run.alternate {
            run.row.min(u64::from(u32::MAX)) as u32
        } else {
            self.main_row(run.row)
        };
        let col = run.col.min(ALTERNATE - 1) | if run.alternate { ALTERNATE } else { 0 };
        // grown by doubling, but never past the limit: the last step takes what's left
        if self.runs.len() == self.runs.capacity() && self.runs.len() < self.limit {
            let more = self.runs.len().max(64).min(self.limit - self.runs.len());
            self.runs.reserve_exact(more);
        }
        self.runs.push_back(Packed {
            start: run.start,
            row,
            len: len.min(u64::from(u16::MAX)) as u16,
            col,
        });
        self.tail = Some(Tail {
            end: run.end,
            row: run.row,
            col: u64::from(run.col) + u64::from(cells),
            alternate: run.alternate,
        });
        if self.runs.len() > self.limit {
            self.runs.pop_front();
        }
    }

    /// A main screen row from the base row; the base moves up to the oldest run's row when a row
    /// gets too far from it (the rows kept span far less than `u32`)
    fn main_row(&mut self, row: u64) -> u32 {
        let old = *self.row_base.get_or_insert(row);
        if row.saturating_sub(old) > u64::from(u32::MAX) {
            let main = |run: &&Packed| run.col & ALTERNATE == 0;
            let base = (self.runs.iter().filter(main))
                .map(|run| old + u64::from(run.row))
                .min()
                .unwrap_or(row);
            for run in self.runs.iter_mut().filter(|run| run.col & ALTERNATE == 0) {
                run.row = (old + u64::from(run.row)).saturating_sub(base) as u32;
            }
            self.row_base = Some(base);
        }
        let base = self.row_base.unwrap_or(row);
        row.saturating_sub(base).min(u64::from(u32::MAX)) as u32
    }

    /// The runs whose bytes are all before `start` go
    pub fn trim(&mut self, start: u64) {
        while self
            .runs
            .front()
            .is_some_and(|run| run.start + u64::from(run.len) <= start)
        {
            self.runs.pop_front();
        }
    }

    pub fn clear(&mut self) {
        self.runs.clear();
        self.row_base = None;
        self.tail = None;
    }

    /// `clear`, the memory given back
    pub fn release(&mut self) {
        self.clear();
        self.runs = VecDeque::new();
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.runs.len()
    }

    /// The run holding byte `offset`
    pub fn at(&self, offset: u64) -> Option<Run> {
        let idx = (self.runs).partition_point(|run| run.start + u64::from(run.len) <= offset);
        let run = self.runs.get(idx).filter(|run| run.start <= offset)?;
        Some(self.unpack(run))
    }

    /// The runs holding bytes of `range`, in byte order
    pub fn within(&self, range: Range<u64>) -> impl Iterator<Item = Run> + '_ {
        let first =
            (self.runs).partition_point(|run| run.start + u64::from(run.len) <= range.start);
        (self.runs.range(first..))
            .take_while(move |run| run.start < range.end)
            .map(|run| self.unpack(run))
    }

    /// The runs, newest first
    pub fn newest_first(&self) -> impl Iterator<Item = Run> + '_ {
        self.runs.iter().rev().map(|run| self.unpack(run))
    }

    fn unpack(&self, run: &Packed) -> Run {
        let alternate = run.col & ALTERNATE != 0;
        let row = u64::from(run.row);
        Run {
            start: run.start,
            end: run.start + u64::from(run.len),
            row: if alternate {
                row
            } else {
                self.row_base.unwrap_or(0) + row
            },
            col: run.col & !ALTERNATE,
            alternate,
        }
    }
}

#[cfg(test)]
#[path = "../../tests/raw_side/terminal/writes.rs"]
mod tests;
