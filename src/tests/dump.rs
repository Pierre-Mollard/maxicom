//! Copy mode in the dump (MAX-252): moves and selections over byte offsets.

use super::*;

/// 8 bytes a line on 4 rows, bytes 5..30 kept: line 0 lost its first 5, line 3 holds 6
fn bounds() -> DumpBounds {
    DumpBounds {
        start: 5,
        end: 30,
        width: 8,
        rows: 4,
    }
}

fn moved(from: u64, actions: impl IntoIterator<Item = CopyModeAction>) -> u64 {
    let mut copy = DumpCopy::at(from);
    for action in actions {
        copy.apply(action, &bounds());
    }
    copy.cursor
}

/// The cursor moves by byte and by line over the bytes kept: left and right cross lines, up
/// and down keep the column and stay put past the first or last byte, line start and end stop
/// at the bytes kept, top and bottom are the first and last byte, a page is a view less a line.
/// How: from bytes 9, 12, 26, 29 of `bounds`
#[test]
fn moves_over_the_bytes_kept() {
    use CopyModeAction::*;
    assert_eq!(moved(16, [MoveLeft]), 15);
    assert_eq!(moved(29, [MoveRight]), 29);
    assert_eq!(moved(5, [MoveLeft]), 5);
    assert_eq!(moved(12, [MoveDown]), 20);
    assert_eq!(moved(12, [MoveUp]), 12, "byte 4 was dropped");
    assert_eq!(moved(26, [MoveDown]), 26, "byte 34 isn't there yet");
    assert_eq!(moved(12, [MoveUp, MoveUp]), 12);
    assert_eq!(moved(9, [StartOfLine]), 8);
    assert_eq!(moved(6, [StartOfLine]), 5);
    assert_eq!(moved(26, [EndOfLine]), 29);
    assert_eq!(moved(12, [JumpToBottom]), 29);
    assert_eq!(moved(29, [JumpToTop]), 5);
    assert_eq!(moved(29, [PageUp]), 5);
    assert_eq!(moved(5, [PageDown]), 29);
    assert_eq!(moved(12, [NextWord]), 12, "no words in bytes");
}

/// `v` selects from the anchor to the cursor either way, `V` whole lines (the bytes kept),
/// the same key again ends it; a yank gives the bytes selected, none without a selection.
/// How: anchor on 12, cursor on 9; then `V` from 12 to 26
#[test]
fn selections_and_yanks() {
    use CopyModeAction::*;
    let b = bounds();
    let mut copy = DumpCopy::at(12);
    copy.apply(Select, &b);
    for _ in 0..3 {
        copy.apply(MoveLeft, &b);
    }
    assert_eq!(copy.selection(&b), Some(9..13));
    copy.apply(Select, &b);
    assert_eq!(copy.selection(&b), None);
    assert_eq!(copy.apply(Yank, &b), DumpResult::Exited);

    let mut copy = DumpCopy::at(12);
    copy.apply(SelectLine, &b);
    copy.apply(MoveDown, &b);
    copy.apply(MoveDown, &b);
    assert_eq!(copy.selection(&b), Some(8..30));
    assert_eq!(
        copy.apply(YankRaw, &b),
        DumpResult::Yank {
            bytes: 8..30,
            as_received: true
        }
    );
}

/// A press puts the cursor on its byte, a drag selects from there and the release yanks; a
/// press then a release without a drag yanks nothing. Offsets off the bytes kept are clamped.
/// How: press 2 (dropped) and drag to 9; press 12 and release
#[test]
fn press_drag_release() {
    let b = bounds();
    let mut copy = DumpCopy::default();
    copy.press(2, &b);
    assert_eq!(copy.cursor, 5);
    copy.drag(9, &b);
    assert_eq!(
        copy.release(&b),
        DumpResult::Yank {
            bytes: 5..10,
            as_received: false
        }
    );

    copy.press(12, &b);
    assert_eq!(copy.release(&b), DumpResult::None);
    assert_eq!(copy.selection(&b), None);
}
