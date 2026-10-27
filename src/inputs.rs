/// Keyboard event polling and translation
/// Using the 'crossterm' crate
use std::{io, time::Duration};

use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use ratatui::layout::Rect;

use crate::{
    app::{
        App, InputContext,
        selection::{CopySide, LogicalPosition, MouseUnit},
    },
    configs::{KeyBindingAction, KeyPattern, Mode},
    inputs::mouse::handle_mouse,
    overlays::{Overlay, PopupTarget, SettingKey},
};

pub mod global_dispatchers;
pub mod local_dispatchers;
pub mod mouse;
pub mod vi;

// Re-exports
pub use local_dispatchers::{
    click_input_at, dispatch_input_key, dispatch_input_key_vi, paste_into_input,
};
pub use vi::{
    ViMode, ViPending, ViState, find_next_word, find_prev_word, find_prev_word_end, find_word_end,
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
    Overlay(DialogAction),
    Ignore,
}

#[derive(Debug)]
pub enum AppAction {
    SetPrefix(bool),
    FocusGained(bool),
    ToggleSetting(SettingKey),
    ToggleLogging,
    DetachSerial,
    ReloadConfig,
    OpenOverlay(Overlay),
    SideView(SideViewAction),
    Hover(u16, u16),
    Scrollbar(ScrollbarAction),
    ClickHold(ClickHoldAction),
    Quit,
}

/// The left button on the history's scrollbar (MAX-142): a press on one of its rows (`0` the
/// `▲`), a drag held from it on a row (above or below it too), the release
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollbarAction {
    Press(u16),
    Drag(i32),
    Release,
}

/// The left button held on a click target that repeats (MAX-143): the press on its cells runs
/// its action, a drag off them stops the repeat, the release
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickHoldAction {
    Press(KeyBindingAction, Rect),
    Drag(u16, u16),
    Release,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LineInputAction {
    Submit,
    HistoryUp,
    HistoryDown,
    AutoResizeRemoteToWidth,
    ClearLine,
    Interrupt,
    ClickAt(usize),
    HandleKey(KeyEvent),
    Paste(String),
}

#[derive(Debug)]
pub enum TerminalAction {
    AutoResizeRemoteToViewer,
    AutoResizeEmulatorToViewer,
    ScreenshotBytes,
    ScreenshotTxt,
    Key(KeyEvent),
    Paste(String),
    MouseReport(Vec<u8>),
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
    ScrollTo(usize),
}

#[derive(Debug, PartialEq, Eq)]
pub enum SideViewAction {
    PanLeft(usize),
    PanRight(usize),
}

#[derive(Debug, PartialEq, Eq)]
pub enum SearchModeAction {
    Start,
    Next,
    Previous,
    Cancel,
    Confirm,
    Kind,
    HandleKey(KeyEvent),
    Paste(String),
    ClickAt(usize),
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
    NextWord,
    NextBigWord,
    PrevWord,
    PrevBigWord,
    WordEnd,
    BigWordEnd,
    PrevWordEnd,
    PrevBigWordEnd,
    JumpToTop,
    JumpToBottom,
    Select,
    SelectLine,
    SelectRange(usize, usize),
    Yank,
    YankRaw,
    ToggleSide,
    MousePress(LogicalPosition, CopySide, MouseUnit),
    MouseDrag(LogicalPosition, (u16, u16)),
    MouseRelease,
    HandleKey(KeyEvent),
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogAction {
    Close,
    Confirm,
    Up,
    Down,
    Left,
    Right,
    CycleNext,
    CyclePrev,
    PageUp,
    PageDown,
    Home,
    End,
    HandleKey(KeyEvent),
    Paste(String),
    Pick(PopupTarget),
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
        Event::FocusGained => {
            return Action::App(AppAction::FocusGained(true));
        }
        Event::FocusLost => {
            return Action::App(AppAction::FocusGained(false));
        }
        Event::Paste(text) => return handle_paste(text, app),
        Event::Mouse(mouse_event) => return handle_mouse(mouse_event, app),
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
            // a quit key again confirms the quit: Ctrl-C twice still quits fast (MAX-125)
            if matches!(app.overlay(), Some(Overlay::ConfirmQuit))
                && resolver.direct_actions.get(&pattern) == Some(&KeyBindingAction::Quit)
            {
                return Action::Overlay(DialogAction::Confirm);
            }
            let is_text_input = app.overlay().map_or_else(|| false, |o| o.is_text_input());

            if is_text_input {
                if pattern.is_modal()
                    && let Some(dialog_action_binding) = resolver.dialog_actions.get(&pattern)
                {
                    return dialog_action_binding.to_action();
                }
                // force typing text for input fields
                return Action::Overlay(DialogAction::HandleKey(key_event));
            }

            if let Some(dialog_action_binding) = resolver.dialog_actions.get(&pattern) {
                return dialog_action_binding.to_action();
            } else {
                return Action::Overlay(DialogAction::HandleKey(key_event));
            }
        }
        InputContext::Search => {
            if let Some(search_action_binding) = resolver.search_actions.get(&pattern) {
                return search_action_binding.to_action();
            }
            if pattern == resolver.prefix {
                return Action::App(AppAction::SetPrefix(true));
            }
            if let Some(action_binding) = resolver.direct_actions.get(&pattern)
                && action_binding.is_navigation()
            {
                return action_binding.to_action(app);
            }
            return Action::Search(SearchModeAction::HandleKey(key_event));
        }
        InputContext::Prefix if app.is_search_active() => {
            if pattern == resolver.prefix {
                return Action::Search(SearchModeAction::HandleKey(key_event));
            }
            return match resolver.prefix_actions.get(&pattern) {
                Some(action_binding) if action_binding.is_navigation() => {
                    action_binding.to_action(app)
                }
                _ => Action::App(AppAction::SetPrefix(false)),
            };
        }
        InputContext::Prefix => {
            // Prefix pressed twice (terminal send, line mode forward)
            if pattern == resolver.prefix {
                match app.settings().mode {
                    Mode::Line => {
                        return Action::LineInput(LineInputAction::HandleKey(key_event));
                    }
                    Mode::Terminal => {
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
            } else if let Some(action_binding) = resolver.direct_actions.get(&pattern)
                && action_binding.is_navigation()
            {
                // the direct scroll and pan keys move the view, the cursor stays (MAX-143)
                action_binding.to_action(app)
            } else {
                Action::CopyMode(CopyModeAction::HandleKey(key_event))
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
                Action::Terminal(TerminalAction::Key(key_event))
            }
        }
        InputContext::Overlay | InputContext::Search | InputContext::Prefix => unreachable!(),
    }
}

fn handle_paste(text: String, app: &App) -> Action {
    if text.is_empty() {
        return Action::Ignore;
    }
    match app.input_context() {
        InputContext::Overlay => {
            if app.overlay().is_some_and(|o| o.is_text_input()) {
                Action::Overlay(DialogAction::Paste(text))
            } else {
                Action::Ignore
            }
        }
        InputContext::Search => Action::Search(SearchModeAction::Paste(text)),
        InputContext::Prefix => Action::App(AppAction::SetPrefix(false)),
        InputContext::CopyMode => Action::Ignore,
        InputContext::LineInput => Action::LineInput(LineInputAction::Paste(text)),
        InputContext::Terminal => Action::Terminal(TerminalAction::Paste(text)),
    }
}

#[cfg(test)]
mod tests {
    //! Keys reaching navigation while the search bar is open (MAX-72), and popup keys (MAX-73).

    use super::*;
    use crate::{
        configs::{KeyBindingsConfig, KeyResolver, SettingsConfig},
        serial::SerialConfig,
    };
    use crossterm::event::{KeyCode, KeyModifiers};

    fn app() -> App {
        App::new(
            SettingsConfig::default(),
            KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap(),
            SerialConfig::default(),
        )
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, modifiers))
    }

    fn ctrl(c: char) -> Event {
        key(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn plain(code: KeyCode) -> Event {
        key(code, KeyModifiers::NONE)
    }

    /// In search mode the global navigation keys (direct and after the prefix) scroll and pan;
    /// other global actions are not reachable, and the other keys still edit the query.
    #[test]
    fn navigation_keys_work_in_search_mode() {
        let mut app = app();
        app.update(Action::Search(SearchModeAction::Start));
        assert!(app.is_search_active());

        let pageup = handle_event(plain(KeyCode::PageUp), &app);
        assert!(matches!(
            pageup,
            Action::History(HistoryAction::ScrollUpPage)
        ));
        let side = handle_event(key(KeyCode::Right, KeyModifiers::CONTROL), &app);
        assert!(matches!(
            side,
            Action::App(AppAction::SideView(SideViewAction::PanRight(2)))
        ));
        let typed = handle_event(plain(KeyCode::Char('x')), &app);
        assert!(matches!(
            typed,
            Action::Search(SearchModeAction::HandleKey(_))
        ));
        // ctrl-f is a direct action, but not navigation: it edits the query
        let search = handle_event(ctrl('f'), &app);
        assert!(matches!(
            search,
            Action::Search(SearchModeAction::HandleKey(_))
        ));

        let prefix = handle_event(ctrl('a'), &app);
        assert!(matches!(prefix, Action::App(AppAction::SetPrefix(true))));
        app.update(prefix);
        let up = handle_event(plain(KeyCode::Up), &app);
        assert!(matches!(up, Action::History(HistoryAction::ScrollUp(1))));
        // repeatable: the prefix stays on after a scroll
        app.update(up);
        let left = handle_event(plain(KeyCode::Left), &app);
        assert!(matches!(left, Action::History(HistoryAction::PanLeft(1))));
        // not navigation: dropped, not Quit
        let quit = handle_event(plain(KeyCode::Char('q')), &app);
        assert!(matches!(quit, Action::App(AppAction::SetPrefix(false))));
        // prefix twice: to the query, not the TX input
        let twice = handle_event(ctrl('a'), &app);
        assert!(matches!(
            twice,
            Action::Search(SearchModeAction::HandleKey(_))
        ));
    }

    /// Outside search mode the same keys keep their global actions.
    #[test]
    fn global_keys_unchanged_outside_search() {
        let mut app = app();
        let search = handle_event(ctrl('f'), &app);
        assert!(matches!(search, Action::Search(SearchModeAction::Start)));
        app.update(Action::App(AppAction::SetPrefix(true)));
        let quit = handle_event(plain(KeyCode::Char('q')), &app);
        assert!(matches!(quit, Action::App(AppAction::Quit)));
    }

    /// A text input popup gets its `[dialog_actions]` only from modal keys, the others are
    /// typed; other popups get them from every key.
    #[test]
    fn text_input_popups_type_non_modal_keys() {
        let mut app = app();
        let file = crate::overlays::FilePathDialog::new("log.txt");
        app.update(Action::App(AppAction::OpenOverlay(Overlay::FilePath(file))));
        let dialog = |event| handle_event(event, &app);

        let typed = |action| matches!(action, Action::Overlay(DialogAction::HandleKey(_)));
        assert!(typed(dialog(plain(KeyCode::Char('q')))));
        assert!(typed(dialog(plain(KeyCode::Char('j')))));
        assert!(matches!(
            dialog(plain(KeyCode::Esc)),
            Action::Overlay(DialogAction::Close)
        ));
        assert!(matches!(
            dialog(ctrl('n')),
            Action::Overlay(DialogAction::CycleNext)
        ));
        assert!(matches!(
            dialog(key(KeyCode::BackTab, KeyModifiers::SHIFT)),
            Action::Overlay(DialogAction::CyclePrev)
        ));

        app.update(Action::App(AppAction::OpenOverlay(Overlay::Settings)));
        assert!(matches!(
            handle_event(plain(KeyCode::Char('q')), &app),
            Action::Overlay(DialogAction::Close)
        ));
    }
}
