//! The printable runs kept, which bytes wrote which cells (MAX-256).

use super::*;

fn run(start: u64, end: u64, row: u64, col: u16) -> Run {
    Run {
        start,
        end,
        row,
        col,
        alternate: false,
    }
}

/// A run going on from the last one (its bytes and its cells follow) makes it longer; a gap in
/// the bytes, another cell or the other screen starts a new one.
/// How: 80 columns; "hello" then "!!" right after it, then runs that don't follow
#[test]
fn continued_runs_merge() {
    let mut writes = Writes::new(100);
    writes.push(run(0, 5, 10, 0), 5, 80);
    writes.push(run(5, 7, 10, 5), 2, 80);
    assert_eq!(writes.len(), 1);
    assert_eq!(writes.at(6), Some(run(0, 7, 10, 0)));

    // a cell further (a cursor move between): a new run
    writes.push(run(7, 8, 10, 9), 1, 80);
    // bytes further (a sequence between)
    writes.push(run(12, 13, 10, 10), 1, 80);
    // the alternate screen
    writes.push(
        Run {
            alternate: true,
            ..run(13, 14, 10, 11)
        },
        1,
        80,
    );
    assert_eq!(writes.len(), 4);

    // a run filling its row ends past it: the next one at the next row's start follows it
    let mut writes = Writes::new(100);
    writes.push(run(0, 80, 3, 0), 80, 80);
    writes.push(run(80, 82, 3, 80), 2, 80);
    assert_eq!(writes.len(), 1);
}

/// The run holding a byte is found by its offset; a byte between runs (a control byte, a
/// sequence) has none.
/// How: runs at 0..3, 10..12 and 20..25
#[test]
fn a_byte_finds_its_run() {
    let mut writes = Writes::new(100);
    for (start, end) in [(0, 3), (10, 12), (20, 25)] {
        writes.push(run(start, end, start, 0), (end - start) as u32, 80);
    }
    assert_eq!(writes.at(0).map(|run| run.start), Some(0));
    assert_eq!(writes.at(11).map(|run| run.start), Some(10));
    assert_eq!(writes.at(24).map(|run| run.start), Some(20));
    for between in [3, 9, 12, 19, 25] {
        assert_eq!(writes.at(between), None, "{between}");
    }
    let newest: Vec<u64> = writes.newest_first().map(|run| run.start).collect();
    assert_eq!(newest, [20, 10, 0]);
}

/// The oldest runs go past the limit, and with the bytes they were made of.
/// How: a limit of 3, 5 runs; then trim at 35
#[test]
fn old_runs_go() {
    let mut writes = Writes::new(3);
    for i in 0..5 {
        writes.push(run(i * 10, i * 10 + 2, i, 0), 2, 80);
    }
    let starts = |writes: &Writes| {
        writes
            .newest_first()
            .map(|run| run.start)
            .collect::<Vec<_>>()
    };
    assert_eq!(starts(&writes), [40, 30, 20]);
    writes.trim(35);
    assert_eq!(starts(&writes), [40]);
    writes.clear();
    assert_eq!(writes.len(), 0);
}

/// Rows are kept as they were given, alternate screen ones too, even when the rows kept are
/// far from the first one ever.
/// How: a row past `u32` after the first runs went; an alternate screen run
#[test]
fn rows_come_back_as_given() {
    let mut writes = Writes::new(100);
    writes.push(run(0, 1, 7, 0), 1, 80);
    writes.trim(1);
    let far = 7 + u64::from(u32::MAX) + 100;
    writes.push(run(1, 2, far, 3), 1, 80);
    writes.push(run(2, 3, far + 1, 0), 1, 80);
    let alternate = Run {
        alternate: true,
        ..run(3, 4, 5, 6)
    };
    writes.push(alternate, 1, 80);
    assert_eq!(writes.at(1), Some(run(1, 2, far, 3)));
    assert_eq!(writes.at(2), Some(run(2, 3, far + 1, 0)));
    assert_eq!(writes.at(3), Some(alternate));
}
