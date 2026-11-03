use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tui_input::{Input, InputRequest, backend::crossterm::EventHandler};

use crate::inputs::{
    CopyModeAction,
    vi::{
        ViMode, ViMotion, ViOperator, ViPending, ViState, apply_vi_motion_to_input, char_to_byte,
        handle_pending_operator, resolve_vi_motion,
    },
};

/// Used to handle for copy mode navigation
pub fn dispatch_copy_key_vi(
    pending: &mut Option<ViPending>,
    current_line_text: &str,
    cursor_char: usize,
    key: KeyEvent,
) -> Option<CopyModeAction> {
    if let Some(motion) = resolve_vi_motion(pending, current_line_text, cursor_char, key) {
        return Some(match motion {
            ViMotion::Left => CopyModeAction::MoveLeft,
            ViMotion::Down => CopyModeAction::MoveDown,
            ViMotion::Up => CopyModeAction::MoveUp,
            ViMotion::Right => CopyModeAction::MoveRight,
            ViMotion::NextWord => CopyModeAction::NextWord,
            ViMotion::NextBigWord => CopyModeAction::NextBigWord,
            ViMotion::PrevWord => CopyModeAction::PrevWord,
            ViMotion::PrevBigWord => CopyModeAction::PrevBigWord,
            ViMotion::WordEnd => CopyModeAction::WordEnd,
            ViMotion::BigWordEnd => CopyModeAction::BigWordEnd,
            ViMotion::PrevWordEnd => CopyModeAction::PrevWordEnd,
            ViMotion::PrevBigWordEnd => CopyModeAction::PrevBigWordEnd,
            ViMotion::StartOfLine => CopyModeAction::StartOfLine,
            ViMotion::EndOfLine => CopyModeAction::EndOfLine,
            ViMotion::JumpToTop => CopyModeAction::JumpToTop,
            ViMotion::JumpToBottom => CopyModeAction::JumpToBottom,
            ViMotion::PageUp => CopyModeAction::PageUp,
            ViMotion::PageDown => CopyModeAction::PageDown,
            ViMotion::TextObject { start, end } => CopyModeAction::SelectRange(start, end),
        });
    }

    // select, yank and exit come from `[copy_mode]` (MAX-155)
    None
}

/// Used to handle input fields in popups and other direct use case
pub fn dispatch_input_key(input: &mut Input, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('a') | KeyCode::Home => {
                input.handle(InputRequest::GoToStart);
                return;
            }
            KeyCode::Char('e') | KeyCode::End => {
                input.handle(InputRequest::GoToEnd);
                return;
            }
            KeyCode::Char('u') => {
                input.reset();
                return;
            }
            KeyCode::Char('w') => {
                input.handle(InputRequest::DeletePrevWord);
                return;
            }
            KeyCode::Char('d') | KeyCode::Delete => {
                input.handle(InputRequest::DeleteNextChar);
                return;
            }
            KeyCode::Char('k') => {
                let cursor = input.cursor();
                let text = input.value()[..char_to_byte(input.value(), cursor)].to_string();
                *input = Input::new(text).with_cursor(cursor);
                return;
            }
            KeyCode::Left => {
                input.handle(InputRequest::GoToPrevWord);
                return;
            }
            KeyCode::Right => {
                input.handle(InputRequest::GoToNextWord);
                return;
            }
            KeyCode::Char('h') => {
                input.handle(InputRequest::DeletePrevChar);
                return;
            }
            _ => {}
        }
    }

    if key.modifiers.contains(KeyModifiers::ALT) {
        match key.code {
            KeyCode::Char('b') | KeyCode::Left => {
                input.handle(InputRequest::GoToPrevWord);
                return;
            }
            KeyCode::Char('f') | KeyCode::Right => {
                input.handle(InputRequest::GoToNextWord);
                return;
            }
            KeyCode::Backspace | KeyCode::Char('d') => {
                input.handle(InputRequest::DeleteNextWord);
                return;
            }
            _ => {}
        }
    }

    input.handle_event(&Event::Key(key));
}

/// Puts an input field's cursor on the char drawn on `column` (counted from the text's start),
/// else at the end of the text: a click in it (MAX-113)
pub fn click_input_at(input: &mut Input, column: usize) {
    let mut start = 0;
    let cursor = input
        .value()
        .chars()
        .position(|ch| {
            start += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            start > column
        })
        .unwrap_or(input.value().chars().count());
    *input = std::mem::take(input).with_cursor(cursor);
}

/// Inserts pasted text at the cursor of a single line input field. Line breaks and tabs become
/// spaces (trailing line breaks are dropped), other control chars are skipped
pub fn paste_into_input(input: &mut Input, text: &str) {
    let text = text.trim_end_matches(['\r', '\n']);
    let value = input.value();
    let cursor = input.cursor();
    let at = char_to_byte(value, cursor);

    let mut new_value = String::with_capacity(value.len() + text.len());
    new_value.push_str(&value[..at]);
    let mut inserted = 0;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let c = match c {
            '\r' if chars.peek() == Some(&'\n') => continue,
            '\r' | '\n' | '\t' => ' ',
            c if c.is_control() => continue,
            c => c,
        };
        new_value.push(c);
        inserted += 1;
    }
    new_value.push_str(&value[at..]);
    *input = Input::new(new_value).with_cursor(cursor + inserted);
}

/// Used to handle input fields with vi motion for line mode prompt
pub fn dispatch_input_key_vi(input: &mut Input, state: &mut ViState, key: KeyEvent) {
    match state.mode {
        ViMode::Insert => {
            if key.code == KeyCode::Esc {
                state.mode = ViMode::Normal;
                state.pending = None;
                input.handle(InputRequest::GoToPrevChar);
                return;
            }
            dispatch_input_key(input, key);
        }

        ViMode::Normal => {
            if key.code == KeyCode::Esc {
                state.pending = None;
                return;
            }

            if let Some(pending) = state.pending.take() {
                handle_pending_operator(input, state, pending, key);
                return;
            }

            if let Some(motion) =
                resolve_vi_motion(&mut state.pending, input.value(), input.cursor(), key)
            {
                apply_vi_motion_to_input(input, motion);
                return;
            }

            match key.code {
                KeyCode::Char('i') => state.mode = ViMode::Insert,
                KeyCode::Char('I') => {
                    input.handle(InputRequest::GoToStart);
                    state.mode = ViMode::Insert;
                }
                KeyCode::Char('a') => {
                    input.handle(InputRequest::GoToNextChar);
                    state.mode = ViMode::Insert;
                }
                KeyCode::Char('A') => {
                    input.handle(InputRequest::GoToEnd);
                    state.mode = ViMode::Insert;
                }

                KeyCode::Char('x') => {
                    input.handle(InputRequest::DeleteNextChar);
                }
                KeyCode::Char('D') => {
                    let cursor = input.cursor();
                    let text = input.value()[..char_to_byte(input.value(), cursor)].to_string();
                    *input = Input::new(text).with_cursor(cursor);
                }
                KeyCode::Char('C') => {
                    let cursor = input.cursor();
                    let text = input.value()[..char_to_byte(input.value(), cursor)].to_string();
                    *input = Input::new(text).with_cursor(cursor);
                    state.mode = ViMode::Insert;
                }

                KeyCode::Char('d') => state.pending = Some(ViPending::Operator(ViOperator::Delete)),
                KeyCode::Char('c') => state.pending = Some(ViPending::Operator(ViOperator::Change)),

                _ => {}
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/inputs/local_dispatchers.rs"]
mod tests;
