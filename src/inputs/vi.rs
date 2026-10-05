use crossterm::event::{KeyCode, KeyEvent};
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
    Inner(ViOperator),
    Around(ViOperator),
}

#[derive(Debug, Clone, Default)]
pub struct ViState {
    pub mode: ViMode,
    pub pending: Option<ViPending>,
}

/// Finds the start and end byte indices of the word surrounding the cursor
fn find_word_bounds(text: &str, cursor: usize) -> (usize, usize) {
    if text.is_empty() {
        return (0, 0);
    }
    let cursor = cursor.min(text.len().saturating_sub(1));
    let bytes = text.as_bytes();

    let is_word_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let target_kind = is_word_char(bytes[cursor]);

    let mut start = cursor;
    while start > 0
        && is_word_char(bytes[start - 1]) == target_kind
        && !bytes[start - 1].is_ascii_whitespace()
    {
        start -= 1;
    }

    let mut end = cursor;
    while end < text.len()
        && is_word_char(bytes[end]) == target_kind
        && !bytes[end].is_ascii_whitespace()
    {
        end += 1;
    }

    (start, end)
}

/// Finds boundaries between quote pairs (`"`, `'`) or brackets (`(`, `[`, `{`)
fn find_paired_bounds(
    text: &str,
    cursor: usize,
    open: char,
    close: char,
    inner: bool,
) -> Option<(usize, usize)> {
    let mut open_idx = None;
    for (i, ch) in text[..cursor].char_indices().rev() {
        if ch == open {
            open_idx = Some(i);
            break;
        }
    }

    let mut close_idx = None;
    for (i, ch) in text[cursor..].char_indices() {
        if ch == close {
            close_idx = Some(cursor + i);
            break;
        }
    }

    if let (Some(start), Some(end)) = (open_idx, close_idx)
        && start < end
    {
        if inner {
            return Some((start + 1, end));
        } else {
            return Some((start, (end + 1).min(text.len())));
        }
    }
    None
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

            // 'd$' or 'c$' -> to end of line
            '$' => {
                let cursor = input.cursor();
                let text = input.value()[..cursor].to_string();
                *input = Input::new(text).with_cursor(cursor);
                if op == ViOperator::Change {
                    state.mode = ViMode::Insert;
                }
            }

            // 'd0' or 'c0' -> to beginning of line
            '0' | '^' => {
                let cursor = input.cursor();
                let text = input.value()[cursor..].to_string();
                *input = Input::new(text).with_cursor(0);
                if op == ViOperator::Change {
                    state.mode = ViMode::Insert;
                }
            }

            // 'di' or 'ci' -> enter inner text object mode
            'i' => {
                state.pending = Some(ViPending::Inner(op));
            }

            // 'da' or 'ca' -> enter around text object mode
            'a' => {
                state.pending = Some(ViPending::Around(op));
            }

            _ => {}
        },

        ViPending::Inner(op) | ViPending::Around(op) => {
            let inner = matches!(pending, ViPending::Inner(_));
            let cursor = input.cursor();
            let text = input.value();

            let bounds = match ch {
                'w' => Some(find_word_bounds(text, cursor)),
                '"' => find_paired_bounds(text, cursor, '"', '"', inner),
                '\'' => find_paired_bounds(text, cursor, '\'', '\'', inner),
                '(' | ')' | 'b' => find_paired_bounds(text, cursor, '(', ')', inner),
                '[' | ']' => find_paired_bounds(text, cursor, '[', ']', inner),
                '{' | '}' | 'B' => find_paired_bounds(text, cursor, '{', '}', inner),
                _ => None,
            };

            if let Some((start, end)) = bounds {
                let mut new_text = String::new();
                new_text.push_str(&text[..start]);
                new_text.push_str(&text[end..]);
                *input = Input::new(new_text).with_cursor(start);

                if op == ViOperator::Change {
                    state.mode = ViMode::Insert;
                }
            }
        }
    }
}
