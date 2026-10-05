use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use tui_input::{Input, InputRequest, backend::crossterm::EventHandler};

use crate::inputs::vi::{ViMode, ViOperator, ViPending, ViState, handle_pending_operator};

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
                let text = input.value()[..cursor].to_string();
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

                KeyCode::Char('h') | KeyCode::Left => {
                    input.handle(InputRequest::GoToPrevChar);
                }
                KeyCode::Char('l') | KeyCode::Right => {
                    input.handle(InputRequest::GoToNextChar);
                }
                KeyCode::Char('w') => {
                    input.handle(InputRequest::GoToNextWord);
                }
                KeyCode::Char('b') => {
                    input.handle(InputRequest::GoToPrevWord);
                }
                KeyCode::Char('0') | KeyCode::Char('^') => {
                    input.handle(InputRequest::GoToStart);
                }
                KeyCode::Char('$') => {
                    input.handle(InputRequest::GoToEnd);
                }

                KeyCode::Char('x') => {
                    input.handle(InputRequest::DeleteNextChar);
                }
                KeyCode::Char('D') => {
                    let cursor = input.cursor();
                    let text = input.value()[..cursor].to_string();
                    *input = Input::new(text).with_cursor(cursor);
                }
                KeyCode::Char('C') => {
                    let cursor = input.cursor();
                    let text = input.value()[..cursor].to_string();
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
