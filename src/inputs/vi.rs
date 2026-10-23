use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_input::{Input, InputRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViMode {
    #[default]
    Insert,
    Normal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViOperator {
    Delete,
    Change,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViPending {
    Operator(ViOperator),
    Inner(Option<ViOperator>),
    Around(Option<ViOperator>),
    Goto,
}

#[derive(Debug, Clone, Default)]
pub struct ViState {
    pub mode: ViMode,
    pub pending: Option<ViPending>,
}

/// Byte offset of the char at `char_idx` (or the end of `text`). The tui-input cursor and the
/// vi bounds count chars, string slices take bytes
pub fn char_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map_or(text.len(), |(i, _)| i)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViMotion {
    Left,
    Down,
    Up,
    Right,
    NextWord,       // `w`
    NextBigWord,    // `W`
    PrevWord,       // `b`
    PrevBigWord,    // `B`
    WordEnd,        // `e`
    BigWordEnd,     // `E`
    PrevWordEnd,    // `ge`
    PrevBigWordEnd, // `gE`
    StartOfLine,    // `0`, `^`, `gh`
    EndOfLine,      // `$`, `gl`
    JumpToTop,      // `gg`
    JumpToBottom,   // `G`
    PageUp,         // `Ctrl+u`, `Ctrl+b`
    PageDown,       // `Ctrl+d`, `Ctrl+f`
    TextObject { start: usize, end: usize },
}

#[inline]
fn same_word_class(a: char, b: char, big_word: bool) -> bool {
    if a.is_whitespace() || b.is_whitespace() {
        return false;
    }
    if big_word {
        true
    } else {
        let is_alphanumeric_or_under = |c: char| c.is_alphanumeric() || c == '_';
        is_alphanumeric_or_under(a) == is_alphanumeric_or_under(b)
    }
}

pub fn find_next_word(text: &str, cursor: usize, big_word: bool) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    if cursor >= total.saturating_sub(1) {
        return total.saturating_sub(1);
    }
    let mut idx = cursor;
    while idx + 1 < total && same_word_class(chars[idx], chars[idx + 1], big_word) {
        idx += 1;
    }
    idx += 1;
    while idx < total && chars[idx].is_whitespace() {
        idx += 1;
    }

    idx.min(total.saturating_sub(1))
}

pub fn find_prev_word(text: &str, cursor: usize, big_word: bool) -> usize {
    let chars: Vec<char> = text.chars().collect();
    if cursor == 0 || chars.is_empty() {
        return 0;
    }
    let mut idx = cursor.min(chars.len() - 1);
    idx = idx.saturating_sub(1);
    while idx > 0 && chars[idx].is_whitespace() {
        idx -= 1;
    }
    while idx > 0 && same_word_class(chars[idx], chars[idx - 1], big_word) {
        idx -= 1;
    }
    idx
}

pub fn find_word_end(text: &str, cursor: usize, big_word: bool) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    if cursor >= total.saturating_sub(1) {
        return total.saturating_sub(1);
    }
    let mut idx = cursor + 1;
    while idx < total && chars[idx].is_whitespace() {
        idx += 1;
    }
    while idx + 1 < total && same_word_class(chars[idx], chars[idx + 1], big_word) {
        idx += 1;
    }
    idx.min(total.saturating_sub(1))
}

pub fn find_prev_word_end(text: &str, cursor: usize, big_word: bool) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    if cursor == 0 || total == 0 {
        return 0;
    }
    let mut idx = cursor.min(total - 1);
    if !chars[idx].is_whitespace() {
        while idx > 0 && same_word_class(chars[idx], chars[idx - 1], big_word) {
            idx -= 1;
        }
        if idx == 0 {
            return 0;
        }
        idx -= 1;
    }
    while idx > 0 && chars[idx].is_whitespace() {
        idx -= 1;
    }
    if chars[idx].is_whitespace() { 0 } else { idx }
}

pub fn find_word_bounds(text: &str, cursor: usize, big_word: bool, inner: bool) -> (usize, usize) {
    if text.is_empty() {
        return (0, 0);
    }
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    let cursor = cursor.min(total.saturating_sub(1));
    if chars[cursor].is_whitespace() {
        return (cursor, cursor);
    }
    let mut start = cursor;
    while start > 0 && same_word_class(chars[cursor], chars[start - 1], big_word) {
        start -= 1;
    }
    let mut end = cursor;
    while end < total && same_word_class(chars[cursor], chars[end], big_word) {
        end += 1;
    }
    if !inner {
        while end < total && chars[end].is_whitespace() {
            end += 1;
        }
    }
    (start, end)
}

pub fn find_paired_bounds(
    text: &str,
    cursor: usize,
    open: char,
    close: char,
    inner: bool,
) -> Option<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    let cursor = cursor.min(total);

    let mut open_idx = None;
    for (i, &ch) in chars[..cursor].iter().enumerate().rev() {
        if ch == open {
            open_idx = Some(i);
            break;
        }
    }

    let mut close_idx = None;
    for (i, &ch) in chars[cursor..].iter().enumerate() {
        if ch == close {
            close_idx = Some(cursor + i);
            break;
        }
    }

    if let (Some(start), Some(end)) = (open_idx, close_idx)
        && start < end
    {
        if inner {
            Some((start + 1, end))
        } else {
            Some((start, (end + 1).min(total)))
        }
    } else {
        None
    }
}

pub fn find_text_object_bounds(
    text: &str,
    cursor: usize,
    ch: char,
    inner: bool,
) -> Option<(usize, usize)> {
    match ch {
        'w' => Some(find_word_bounds(text, cursor, false, inner)),
        'W' => Some(find_word_bounds(text, cursor, true, inner)),
        '"' => find_paired_bounds(text, cursor, '"', '"', inner),
        '\'' => find_paired_bounds(text, cursor, '\'', '\'', inner),
        '(' | ')' | 'b' => find_paired_bounds(text, cursor, '(', ')', inner),
        '[' | ']' => find_paired_bounds(text, cursor, '[', ']', inner),
        '{' | '}' | 'B' => find_paired_bounds(text, cursor, '{', '}', inner),
        _ => None,
    }
}

pub fn resolve_vi_motion(
    pending: &mut Option<ViPending>,
    current_line_text: &str,
    cursor_char: usize,
    key: KeyEvent,
) -> Option<ViMotion> {
    if let Some(state) = pending.take() {
        match state {
            ViPending::Goto => match key.code {
                KeyCode::Char('g') => return Some(ViMotion::JumpToTop),
                KeyCode::Char('e') => return Some(ViMotion::PrevWordEnd),
                KeyCode::Char('E') => return Some(ViMotion::PrevBigWordEnd),
                KeyCode::Char('h') => return Some(ViMotion::StartOfLine),
                KeyCode::Char('l') => return Some(ViMotion::EndOfLine),
                _ => return None,
            },

            ViPending::Inner(_) | ViPending::Around(_) => {
                let inner = matches!(state, ViPending::Inner(_));
                if let KeyCode::Char(ch) = key.code
                    && let Some((start, end)) =
                        find_text_object_bounds(current_line_text, cursor_char, ch, inner)
                {
                    return Some(ViMotion::TextObject { start, end });
                }
                return None;
            }

            ViPending::Operator(_) => {}
        }
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('u') | KeyCode::Char('b') => return Some(ViMotion::PageUp),
            KeyCode::Char('d') | KeyCode::Char('f') => return Some(ViMotion::PageDown),
            _ => return None,
        }
    }

    match key.code {
        KeyCode::Char('h') | KeyCode::Left => Some(ViMotion::Left),
        KeyCode::Char('j') | KeyCode::Down => Some(ViMotion::Down),
        KeyCode::Char('k') | KeyCode::Up => Some(ViMotion::Up),
        KeyCode::Char('l') | KeyCode::Right => Some(ViMotion::Right),

        KeyCode::Char('w') => Some(ViMotion::NextWord),
        KeyCode::Char('b') => Some(ViMotion::PrevWord),
        KeyCode::Char('e') => Some(ViMotion::WordEnd),

        KeyCode::Char('W') => Some(ViMotion::NextBigWord),
        KeyCode::Char('B') => Some(ViMotion::PrevBigWord),
        KeyCode::Char('E') => Some(ViMotion::BigWordEnd),

        KeyCode::Char('0') | KeyCode::Char('^') | KeyCode::Home => Some(ViMotion::StartOfLine),
        KeyCode::Char('$') | KeyCode::End => Some(ViMotion::EndOfLine),
        KeyCode::Char('G') => Some(ViMotion::JumpToBottom),

        KeyCode::Char('g') => {
            *pending = Some(ViPending::Goto);
            None
        }

        KeyCode::Char('i') => {
            *pending = Some(ViPending::Inner(None));
            None
        }
        KeyCode::Char('a') => {
            *pending = Some(ViPending::Around(None));
            None
        }

        _ => None,
    }
}

pub fn handle_pending_operator(
    input: &mut Input,
    state: &mut ViState,
    pending: ViPending,
    key: KeyEvent,
) {
    let KeyCode::Char(ch) = key.code else {
        return;
    };

    match pending {
        ViPending::Operator(op) => match ch {
            // 'dd' or 'cc' -> delete/clear entire line
            'd' if op == ViOperator::Delete => {
                input.reset();
            }
            'c' if op == ViOperator::Change => {
                input.reset();
                state.mode = ViMode::Insert;
            }

            // 'dw' or 'cw' -> delete/change next word
            'w' => {
                input.handle(InputRequest::DeleteNextWord);
                if op == ViOperator::Change {
                    state.mode = ViMode::Insert;
                }
            }

            // 'db' or 'cb' -> delete/change prev word
            'b' => {
                input.handle(InputRequest::DeletePrevWord);
                if op == ViOperator::Change {
                    state.mode = ViMode::Insert;
                }
            }

            // 'd$' or 'c$' -> delete/change to end of line
            '$' => {
                let cursor = input.cursor();
                let text = input.value()[..char_to_byte(input.value(), cursor)].to_string();
                *input = Input::new(text).with_cursor(cursor);
                if op == ViOperator::Change {
                    state.mode = ViMode::Insert;
                }
            }

            // 'd0' or 'c0' -> delete/change to beginning of line
            '0' | '^' => {
                let text = input.value()[char_to_byte(input.value(), input.cursor())..].to_string();
                *input = Input::new(text).with_cursor(0);
                if op == ViOperator::Change {
                    state.mode = ViMode::Insert;
                }
            }

            // 'di' or 'ci' -> enter inner text object mode
            'i' => {
                state.pending = Some(ViPending::Inner(Some(op)));
            }

            // 'da' or 'ca' -> enter around text object mode
            'a' => {
                state.pending = Some(ViPending::Around(Some(op)));
            }

            _ => {}
        },

        ViPending::Inner(op) | ViPending::Around(op) => {
            let inner = matches!(pending, ViPending::Inner(_));
            let cursor = input.cursor();
            let text = input.value();

            if let Some((start, end)) = find_text_object_bounds(text, cursor, ch, inner) {
                let mut new_text = String::new();
                new_text.push_str(&text[..char_to_byte(text, start)]);
                new_text.push_str(&text[char_to_byte(text, end)..]);
                *input = Input::new(new_text).with_cursor(start);

                if op == Some(ViOperator::Change) {
                    state.mode = ViMode::Insert;
                }
            }
        }

        ViPending::Goto => {}
    }
}

pub fn apply_vi_motion_to_input(input: &mut Input, motion: ViMotion) {
    let move_cursor_to = |input: &mut Input, target: usize| {
        let current = input.cursor();
        if target > current {
            for _ in 0..(target - current) {
                input.handle(InputRequest::GoToNextChar);
            }
        } else if current > target {
            for _ in 0..(current - target) {
                input.handle(InputRequest::GoToPrevChar);
            }
        }
    };

    match motion {
        ViMotion::Left => {
            input.handle(InputRequest::GoToPrevChar);
        }
        ViMotion::Right => {
            input.handle(InputRequest::GoToNextChar);
        }
        ViMotion::StartOfLine => {
            input.handle(InputRequest::GoToStart);
        }
        ViMotion::EndOfLine => {
            input.handle(InputRequest::GoToEnd);
        }
        ViMotion::NextWord => {
            move_cursor_to(input, find_next_word(input.value(), input.cursor(), false))
        }
        ViMotion::NextBigWord => {
            move_cursor_to(input, find_next_word(input.value(), input.cursor(), true))
        }

        ViMotion::PrevWord => {
            move_cursor_to(input, find_prev_word(input.value(), input.cursor(), false))
        }
        ViMotion::PrevBigWord => {
            move_cursor_to(input, find_prev_word(input.value(), input.cursor(), true))
        }

        ViMotion::WordEnd => {
            move_cursor_to(input, find_word_end(input.value(), input.cursor(), false))
        }
        ViMotion::BigWordEnd => {
            move_cursor_to(input, find_word_end(input.value(), input.cursor(), true))
        }

        ViMotion::PrevWordEnd => move_cursor_to(
            input,
            find_prev_word_end(input.value(), input.cursor(), false),
        ),
        ViMotion::PrevBigWordEnd => move_cursor_to(
            input,
            find_prev_word_end(input.value(), input.cursor(), true),
        ),
        ViMotion::TextObject { start, .. } => {
            let current = input.cursor();
            if start > current {
                for _ in 0..(start - current) {
                    input.handle(InputRequest::GoToNextChar);
                }
            } else if current > start {
                for _ in 0..(current - start) {
                    input.handle(InputRequest::GoToPrevChar);
                }
            }
        }
        ViMotion::Up
        | ViMotion::Down
        | ViMotion::PageUp
        | ViMotion::PageDown
        | ViMotion::JumpToTop
        | ViMotion::JumpToBottom => { /* doesnt apply in input field */ }
    }
}
