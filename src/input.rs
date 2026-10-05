/// Keyboard event polling and translation
/// Using the 'crossterm' crate
use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use terminput::Encoding;
use terminput_crossterm::to_terminput_key;
use tui_input::{Input, InputRequest, backend::crossterm::EventHandler};

use crate::{
    app::{App, InputContext},
    configs::{
        InputMode, KeyBindingAction, KeyBindingCopyAction, KeyBindingLineAction,
        KeyBindingSearchAction, KeyBindingTerminalAction, KeyPattern,
    },
    overlays::{HelpDialog, Overlay, ResizeDialog, SerialConfigDialog, SettingKey},
};

#[derive(Debug)]
pub enum Action {
    App(AppAction),
    TransmitRaw(Vec<u8>),
    LineInput(LineInputAction),
    Terminal(TerminalAction),
    History(HistoryAction),
    CopyMode(CopyModeAction),
    Search(SearchModeAction),
    OverlayKey(KeyEvent),
    Ignore,
}

#[derive(Debug)]
pub enum AppAction {
    SetPrefix(bool),
    FocusGained(bool),
    ToggleSetting(SettingKey),
    ToggleLogging,
    OpenOverlay(Overlay),
    Quit,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LineInputAction {
    Submit,
    HistoryUp,
    HistoryDown,
    ClearLine,
    HandleKey(KeyEvent),
}

#[derive(Debug)]
pub enum TerminalAction {
    AutoResizeRemoteToViewer,
    AutoResizeEmulatorToViewer,
}

#[derive(Debug, PartialEq, Eq)]
pub enum HistoryAction {
    ScrollUp(usize),
    ScrollDown(usize),
    ScrollUpPage,
    ScrollDownPage,
    ScrollToBottom,
    ScrollToTop,
    PanLeft(usize),
    PanRight(usize),
    Clear,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SearchModeAction {
    Start,
    Next,
    Previous,
    Cancel,
    Confirm,
    HandleKey(KeyEvent),
}

#[derive(Debug, PartialEq, Eq)]
pub enum CopyModeAction {
    Enter,
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    PageUp,
    PageDown,
    StartOfLine,
    EndOfLine,
    Select,
    Yank,
    Exit,
}

/// Using crossterm's event API instead of stdin.read() to handle multi-byte ANSI sequence like
/// arrows and page up and prefix
pub fn poll_event(timeout: Duration) -> io::Result<Option<Event>> {
    if event::poll(timeout)? {
        Ok(Some(event::read()?))
    } else {
        Ok(None)
    }
}

/// Pure translation: takes a crossterm Event and turns it into a typed Action
pub fn handle_event(event: Event, app: &App) -> Action {
    // non key events
    match event {
        crossterm::event::Event::FocusGained => {
            return Action::App(AppAction::FocusGained(true));
        }
        crossterm::event::Event::FocusLost => {
            return Action::App(AppAction::FocusGained(false));
        }
        _ => {}
    }

    // key event from here
    let Event::Key(key_event) = event else {
        return Action::Ignore;
    };

    // ignores release events if emitted (only processing press/repeat)
    if key_event.kind == KeyEventKind::Release {
        return Action::Ignore;
    }

    let pattern = KeyPattern::from_event(&key_event);
    let resolver = &app.key_resolver;
    let ctx = app.input_context();

    // Actions using or ignoring with prefix
    match ctx {
        InputContext::Overlay => {
            return Action::OverlayKey(key_event);
        }
        InputContext::Search => {
            if let Some(search_action_binding) = resolver.search_actions.get(&pattern) {
                return search_action_binding.to_action();
            } else {
                return Action::Search(SearchModeAction::HandleKey(key_event));
            }
        }
        InputContext::Prefix => {
            // Prefix pressed twice (terminal send, line mode forward)
            if pattern == resolver.prefix {
                match app.settings().input_mode {
                    InputMode::Line => {
                        return Action::LineInput(LineInputAction::HandleKey(key_event));
                    }
                    InputMode::Terminal => {
                        let bytes = resolver.prefix.to_raw_bytes();
                        if !bytes.is_empty() {
                            return Action::TransmitRaw(bytes);
                        }
                        return Action::App(AppAction::SetPrefix(false));
                    }
                }
            }

            if let Some(prefix_action_binding) = resolver.prefix_actions.get(&pattern) {
                return prefix_action_binding.to_action(app);
            } else {
                return Action::App(AppAction::SetPrefix(false));
            }
        }
        _ => {}
    }

    if pattern == resolver.prefix {
        return Action::App(AppAction::SetPrefix(true));
    }

    // Actions unreachable with prefix
    match ctx {
        InputContext::CopyMode => {
            if let Some(copy_action_binding) = resolver.copy_actions.get(&pattern) {
                copy_action_binding.to_action()
            } else {
                Action::Ignore
            }
        }
        InputContext::LineInput => {
            if let Some(action_binding) = resolver.direct_actions.get(&pattern) {
                action_binding.to_action(app)
            } else if let Some(line_action_binding) = resolver.line_actions.get(&pattern) {
                line_action_binding.to_action()
            } else {
                Action::LineInput(LineInputAction::HandleKey(key_event))
            }
        }
        InputContext::Terminal => {
            if let Some(action_binding) = resolver.direct_actions.get(&pattern) {
                action_binding.to_action(app)
            } else if let Some(term_action_binding) = resolver.terminal_actions.get(&pattern) {
                term_action_binding.to_action()
            } else {
                handle_terminal_mode(key_event)
            }
        }
        InputContext::Overlay | InputContext::Search | InputContext::Prefix => unreachable!(),
    }
}

impl KeyBindingAction {
    pub fn to_action(self, app: &App) -> Action {
        match self {
            KeyBindingAction::Quit => Action::App(AppAction::Quit),
            KeyBindingAction::SKInputMode => {
                Action::App(AppAction::ToggleSetting(SettingKey::InputMode))
            }
            KeyBindingAction::SKLocalEcho => {
                Action::App(AppAction::ToggleSetting(SettingKey::LocalEcho))
            }
            KeyBindingAction::SKLineWrap => {
                Action::App(AppAction::ToggleSetting(SettingKey::LineWrap))
            }
            KeyBindingAction::SKDisplayTimestamp => {
                Action::App(AppAction::ToggleSetting(SettingKey::DisplayTimestamp))
            }
            KeyBindingAction::SKTextEncoding => {
                Action::App(AppAction::ToggleSetting(SettingKey::TextEncoding))
            }
            KeyBindingAction::Help => {
                Action::App(AppAction::OpenOverlay(Overlay::Help(HelpDialog::new())))
            }
            KeyBindingAction::Settings => Action::App(AppAction::OpenOverlay(Overlay::Settings)),
            KeyBindingAction::ToggleLogging => Action::App(AppAction::ToggleLogging),
            KeyBindingAction::ResizeDialog => {
                let (r, c) = app.terminal_mode.get_vt100_size();
                Action::App(AppAction::OpenOverlay(Overlay::TerminalSize(
                    ResizeDialog::new(r, c),
                )))
            }

            KeyBindingAction::AutoResizeRemote => {
                Action::Terminal(TerminalAction::AutoResizeRemoteToViewer)
            }
            KeyBindingAction::AutoResizeEmulator => {
                Action::Terminal(TerminalAction::AutoResizeEmulatorToViewer)
            }
            KeyBindingAction::SerialSettings => {
                let dialog = SerialConfigDialog::new(&app.serial_config);
                Action::App(AppAction::OpenOverlay(Overlay::SerialSettings(dialog)))
            }

            KeyBindingAction::ScrollUp => Action::History(HistoryAction::ScrollUp(1)),
            KeyBindingAction::ScrollDown => Action::History(HistoryAction::ScrollDown(1)),
            KeyBindingAction::ScrollToBottom => Action::History(HistoryAction::ScrollToBottom),
            KeyBindingAction::ScrollToTop => Action::History(HistoryAction::ScrollToTop),
            KeyBindingAction::PanLeft => Action::History(HistoryAction::PanLeft(1)),
            KeyBindingAction::PanRight => Action::History(HistoryAction::PanRight(1)),
            KeyBindingAction::PageUp => Action::History(HistoryAction::ScrollUpPage),
            KeyBindingAction::PageDown => Action::History(HistoryAction::ScrollDownPage),

            KeyBindingAction::EnterCopyMode => Action::CopyMode(CopyModeAction::Enter),
            KeyBindingAction::EnterSearch => Action::Search(SearchModeAction::Start),
            KeyBindingAction::ClearHistory => Action::History(HistoryAction::Clear),
            KeyBindingAction::ExitPrefix => Action::App(AppAction::SetPrefix(false)),
        }
    }
}

impl KeyBindingLineAction {
    pub fn to_action(self) -> Action {
        match self {
            Self::Submit => Action::LineInput(LineInputAction::Submit),
            Self::HistoryPrev => Action::LineInput(LineInputAction::HistoryUp),
            Self::HistoryNext => Action::LineInput(LineInputAction::HistoryDown),
            Self::ClearLine => Action::LineInput(LineInputAction::ClearLine),
            Self::Quit => Action::App(AppAction::Quit),
        }
    }
}

impl KeyBindingTerminalAction {
    pub fn to_action(self) -> Action {
        match self {
            Self::AutoResizeRemote => Action::Terminal(TerminalAction::AutoResizeRemoteToViewer),
            Self::AutoResizeEmulator => {
                Action::Terminal(TerminalAction::AutoResizeEmulatorToViewer)
            }
            Self::Quit => Action::App(AppAction::Quit),
        }
    }
}

impl KeyBindingSearchAction {
    pub fn to_action(self) -> Action {
        match self {
            Self::Next => Action::Search(SearchModeAction::Next),
            Self::Previous => Action::Search(SearchModeAction::Previous),
            Self::Cancel => Action::Search(SearchModeAction::Cancel),
            Self::Confirm => Action::Search(SearchModeAction::Confirm),
        }
    }
}

impl KeyBindingCopyAction {
    pub fn to_action(self) -> Action {
        match self {
            Self::Up => Action::CopyMode(CopyModeAction::MoveUp),
            Self::Down => Action::CopyMode(CopyModeAction::MoveDown),
            Self::Left => Action::CopyMode(CopyModeAction::MoveLeft),
            Self::Right => Action::CopyMode(CopyModeAction::MoveRight),
            Self::PageUp => Action::CopyMode(CopyModeAction::PageUp),
            Self::PageDown => Action::CopyMode(CopyModeAction::PageDown),
            Self::StartOfLine => Action::CopyMode(CopyModeAction::StartOfLine),
            Self::EndOfLine => Action::CopyMode(CopyModeAction::EndOfLine),
            Self::Select => Action::CopyMode(CopyModeAction::Select),
            Self::Yank => Action::CopyMode(CopyModeAction::Yank),
            Self::Exit => Action::CopyMode(CopyModeAction::Exit),
        }
    }
}

/// forward everything to the remote through serial
fn handle_terminal_mode(key_event_crossterm: KeyEvent) -> Action {
    // Convert Crossterm KeyEvent -> Terminput KeyEvent
    if let Ok(key_event_terminput) = to_terminput_key(key_event_crossterm) {
        let event_terminput = terminput::Event::Key(key_event_terminput);
        let mut buf = [0u8; 32];

        if let Ok(len) = event_terminput.encode(&mut buf, Encoding::Xterm) {
            return Action::TransmitRaw(buf[..len].to_vec());
        }
    }

    Action::Ignore
}

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

// TODO: create sub module for it vi motions

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

fn handle_pending_operator(
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
