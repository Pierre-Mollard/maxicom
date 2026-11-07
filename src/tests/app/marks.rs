use super::*;

/// MAX-164: a mark is set, read, moved and removed by its letter, `a` to `z` only.
/// How: marks set on lines, one moved, one removed, letters out of range.
#[test]
fn set_get_move_remove() {
    let mut marks = Marks::default();
    assert!(marks.sorted().is_empty());
    assert!(marks.set('a', 10));
    assert!(marks.set('z', 3));
    assert_eq!(marks.get('a'), Some(10));
    assert_eq!(marks.get('z'), Some(3));
    assert_eq!(marks.get('b'), None);

    assert!(marks.set('a', 12));
    assert_eq!(marks.get('a'), Some(12));

    assert_eq!(marks.remove('a'), Some(12));
    assert_eq!(marks.remove('a'), None);
    assert_eq!(marks.get('a'), None);

    for letter in ['A', '1', '\'', 'é'] {
        assert!(!marks.set(letter, 1), "{letter}");
        assert_eq!(marks.get(letter), None);
        assert_eq!(marks.remove(letter), None);
    }
    assert_eq!(marks.sorted(), [('z', 3)]);
}

/// MAX-164: the first free letter fills the gaps from `a`, none when all 26 are taken.
/// How: a, b set then a removed; then every letter set.
#[test]
fn first_free_letter() {
    let mut marks = Marks::default();
    assert_eq!(marks.first_free(), Some('a'));
    marks.set('a', 1);
    marks.set('b', 2);
    assert_eq!(marks.first_free(), Some('c'));
    marks.remove('a');
    assert_eq!(marks.first_free(), Some('a'));
    for letter in 'a'..='z' {
        marks.set(letter, 0);
    }
    assert_eq!(marks.first_free(), None);
}

/// MAX-164: marks list by line then letter; previous / next are the closest mark lines
/// strictly above / below, the first letter when several share a line.
/// How: c and a on line 5, b on 20, d on 40; walked from lines around them.
#[test]
fn sorted_prev_next() {
    let mut marks = Marks::default();
    marks.set('d', 40);
    marks.set('c', 5);
    marks.set('b', 20);
    marks.set('a', 5);
    assert_eq!(marks.sorted(), [('a', 5), ('c', 5), ('b', 20), ('d', 40)]);

    assert_eq!(marks.prev(5), None);
    assert_eq!(marks.prev(6), Some(('a', 5)));
    assert_eq!(marks.prev(20), Some(('a', 5)));
    assert_eq!(marks.prev(100), Some(('d', 40)));

    assert_eq!(marks.next(0), Some(('a', 5)));
    assert_eq!(marks.next(5), Some(('b', 20)));
    assert_eq!(marks.next(39), Some(('d', 40)));
    assert_eq!(marks.next(40), None);

    assert_eq!(Marks::default().prev(10), None);
    assert_eq!(Marks::default().next(0), None);
}

/// MAX-164: when the oldest lines drop out of the history the marks follow their lines, and
/// the ones on dropped lines go; clear takes them all.
/// How: marks on lines 2, 5, 9, then 5 lines evicted, then cleared.
#[test]
fn rebase_and_clear() {
    let mut marks = Marks::default();
    marks.set('a', 2);
    marks.set('b', 5);
    marks.set('c', 9);
    marks.rebase_after_eviction(5);
    assert_eq!(marks.sorted(), [('b', 0), ('c', 4)]);
    marks.rebase_after_eviction(0);
    assert_eq!(marks.sorted(), [('b', 0), ('c', 4)]);

    marks.clear();
    assert!(marks.sorted().is_empty());
}
