/// Keyboard event polling and translation
/// Using the 'crossterm' crate
use std::{io, path::PathBuf, time::Duration};

use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use ratatui::layout::Rect;

use crate::{
    app::{
        App, InputContext,
        mash::Mash,
        selection::{CopySide, LogicalPosition, MouseUnit},
        transfer::FileTransfer,
    },
    capture::{ScreenshotFormat, Stream},
    commands::Invocation,
    configs::{KeyBindingAction, KeyPattern, Mode, TextEncoding},
    inputs::mouse::handle_mouse,
    link::{
        Address,
        serial::{LineControl, SerialConfig},
    },
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
    ToggleRecording(Stream),
    DetachSerial,
    ReloadConfig,
    OpenOverlay(Overlay),
    SideView(SideViewAction),
    Hover(u16, u16),
    Scrollbar(ScrollbarAction),
    ClickHold(ClickHoldAction),
    PasteBuffer,
    OpenSessions,
    StartTransfer(Box<FileTransfer>),
    CancelCommands,
    Reconfigure(SerialConfig),
    LineControl(LineControl),
    PauseCommands(Duration),
    Open(Address),
    Replay { path: PathBuf, delay: Duration },
    StopReplay,
    SetEncoding(TextEncoding),
    SaveScreenshot(PathBuf),
    QueueCommands(Vec<Invocation>),
    ToggleTriggers,
    ArmTriggers { on: bool, name: Option<String> },
    StartMash(Box<Mash>),
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
    AutoResizeRemoteToEmulator,
    AutoResizeEmulatorToViewer,
    Screenshot(ScreenshotFormat),
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
    Query(String),
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
    ClickAt(usize),
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
            // a quit key again confirms the quit: Ctrl-Q twice quits fast (MAX-125)
            if matches!(app.overlay(), Some(Overlay::ConfirmQuit))
                && resolver.direct_actions.get(&pattern) == Some(&KeyBindingAction::Quit)
            {
                return Action::Overlay(DialogAction::Confirm);
            }
            let is_text_input = app.overlay().map_or_else(|| false, |o| o.is_text_input());

            if is_text_input {
                // only close, confirm and cycle: the field keeps Ctrl-U, Ctrl-D, Home, End (MAX-155)
                if pattern.is_modal()
                    && let Some(dialog_action_binding) = resolver.dialog_actions.get(&pattern)
                    && dialog_action_binding.is_text_input_action()
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
            } else if let Some(line) = resolver.prefix_commands.get(&pattern) {
                return Action::App(AppAction::QueueCommands(line.invocations.clone()));
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
            } else if let Some(line) = resolver.direct_commands.get(&pattern) {
                Action::App(AppAction::QueueCommands(line.invocations.clone()))
            } else if let Some(line_action_binding) = resolver.line_actions.get(&pattern) {
                line_action_binding.to_action()
            } else {
                Action::LineInput(LineInputAction::HandleKey(key_event))
            }
        }
        InputContext::Terminal => {
            if let Some(action_binding) = resolver.direct_actions.get(&pattern) {
                action_binding.to_action(app)
            } else if let Some(line) = resolver.direct_commands.get(&pattern) {
                Action::App(AppAction::QueueCommands(line.invocations.clone()))
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
    use crate::configs::{KeyBindingsConfig, KeyResolver, SettingsConfig};
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

    /// MAX-12: a key bound to a command line queues it after the prefix, and as a direct key in
    /// line input and terminal mode; in copy mode and search it does nothing of its own, as
    /// any global key but navigation.
    /// How: F6 after the prefix, F5 direct, in each context.
    #[test]
    fn bound_lines_run_where_global_keys_do() {
        let mut config = KeyBindingsConfig::default();
        config
            .prefix_actions
            .insert("f6".into(), "help; settings".into());
        config
            .direct_actions
            .insert("f5".into(), "send-hex 03".into());
        let mut settings = SettingsConfig::default();
        settings.mode = Mode::Line;
        let mut app = App::new(
            settings,
            KeyResolver::try_new(&config).unwrap(),
            SerialConfig::default(),
        );
        let commands = |action: Action| match action {
            Action::App(AppAction::QueueCommands(invocations)) => invocations
                .iter()
                .map(|invocation| invocation.spec.name)
                .collect::<Vec<_>>(),
            other => panic!("{other:?}"),
        };
        let f5 = || plain(KeyCode::F(5));

        assert_eq!(commands(handle_event(f5(), &app)), ["send-hex"]);
        app.update(Action::App(AppAction::SetPrefix(true)));
        let f6 = handle_event(plain(KeyCode::F(6)), &app);
        assert_eq!(commands(f6), ["help", "settings"]);
        app.update(Action::App(AppAction::SetPrefix(false)));

        app.update(Action::App(AppAction::ToggleSetting(SettingKey::Mode)));
        assert_eq!(commands(handle_event(f5(), &app)), ["send-hex"]);

        app.update(Action::Search(SearchModeAction::Start));
        assert!(matches!(
            handle_event(f5(), &app),
            Action::Search(SearchModeAction::HandleKey(_))
        ));
        app.update(Action::Search(SearchModeAction::Cancel));
        app.update(Action::CopyMode(CopyModeAction::Enter));
        assert!(matches!(
            handle_event(f5(), &app),
            Action::CopyMode(CopyModeAction::HandleKey(_))
        ));
    }

    /// In search mode the global navigation keys (direct and after the prefix) scroll and pan;
    /// other global actions are not reachable, and the other keys still edit the query.
    #[test]
    fn navigation_keys_work_in_search_mode() {
        let mut app = app();
        app.update(Action::Search(SearchModeAction::Start));
        assert!(app.is_search_active());

        let pageup = handle_event(key(KeyCode::PageUp, KeyModifiers::SHIFT), &app);
        assert!(matches!(
            pageup,
            Action::History(HistoryAction::ScrollUpPage)
        ));
        let typed = handle_event(plain(KeyCode::Char('x')), &app);
        assert!(matches!(
            typed,
            Action::Search(SearchModeAction::HandleKey(_))
        ));
        // ctrl-q is a direct action, but not navigation: it edits the query
        let quit = handle_event(ctrl('q'), &app);
        assert!(matches!(
            quit,
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
        app.update(left);
        let side = handle_event(key(KeyCode::Right, KeyModifiers::SHIFT), &app);
        assert!(matches!(
            side,
            Action::App(AppAction::SideView(SideViewAction::PanRight(2)))
        ));
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
        let quit = handle_event(ctrl('q'), &app);
        assert!(matches!(quit, Action::App(AppAction::Quit)));
        app.update(Action::App(AppAction::SetPrefix(true)));
        let quit = handle_event(plain(KeyCode::Char('q')), &app);
        assert!(matches!(quit, Action::App(AppAction::Quit)));
    }

    /// A text input popup gets its `[dialog_actions]` only from modal keys, the others are
    /// typed; other popups get them from every key.
    #[test]
    fn text_input_popups_type_non_modal_keys() {
        let mut app = app();
        let file = crate::overlays::FilePathDialog::new(
            Stream::Text,
            "log.txt".to_string(),
            "log.raw".to_string(),
        );
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

    /// The event crossterm sends for a key: an uppercase letter comes with Shift
    fn event_of(key: KeyPattern) -> Event {
        match key.code {
            KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::SHIFT) => Event::Key(
                KeyEvent::new(KeyCode::Char(c.to_ascii_uppercase()), key.modifiers),
            ),
            code => Event::Key(KeyEvent::new(code, key.modifiers)),
        }
    }

    /// What a key does, short: the key handed on as typed, or the action
    fn label(action: &Action) -> String {
        match action {
            Action::Terminal(TerminalAction::Key(_)) => "device".to_string(),
            Action::LineInput(LineInputAction::HandleKey(_)) => "editor".to_string(),
            Action::Search(SearchModeAction::HandleKey(_)) => "query".to_string(),
            Action::CopyMode(CopyModeAction::HandleKey(_)) => "copy keys".to_string(),
            Action::Overlay(DialogAction::HandleKey(_)) => "field".to_string(),
            Action::App(AppAction::OpenOverlay(overlay)) => {
                let overlay = format!("{overlay:?}");
                let name = overlay.split('(').next().unwrap_or_default();
                format!("open {name}")
            }
            Action::App(AppAction::QueueCommands(invocations)) => {
                let names: Vec<_> = invocations.iter().map(|i| i.spec.name).collect();
                format!("run {}", names.join("; "))
            }
            other => format!("{other:?}"),
        }
    }

    /// The keys of the default keymap (every section) and a few the remote or the input
    /// fields use, which the default keymap may steal (MAX-155)
    fn probe_keys() -> Vec<KeyPattern> {
        let config = KeyBindingsConfig::default();
        let extra = [
            "ctrl-d",
            "ctrl-f",
            "ctrl-left",
            "ctrl-right",
            "alt-q",
            "alt-v",
            "ctrl-w",
            "ctrl-k",
            "ctrl-h",
            "shift-pageup",
            "shift-pagedown",
            "shift-left",
            "shift-right",
        ];
        let mut keys: Vec<KeyPattern> = [
            &config.direct_actions,
            &config.prefix_actions,
            &config.line_mode,
            &config.terminal_mode,
            &config.search_mode,
            &config.copy_mode,
            &config.dialog_actions,
        ]
        .into_iter()
        .flat_map(|section| section.keys().map(String::as_str))
        .chain([config.prefix.as_str()])
        .chain(extra)
        .map(|key| key.parse().unwrap())
        .collect();
        keys.sort_by_cached_key(|key| key.to_string());
        keys.dedup();
        keys
    }

    /// One line per key that doesn't do what `typed` says (the context's fallback)
    fn walk(app: &App, typed: &str) -> Vec<String> {
        probe_keys()
            .into_iter()
            .map(|key| (key, label(&handle_event(event_of(key), app))))
            .filter(|(_, action)| action != typed)
            .map(|(key, action)| format!("{key} => {action}"))
            .collect()
    }

    /// MAX-155 step 0: what each key of the default keymap does in each context. A key not
    /// listed is handed on as typed (to the device, the line editor, the search query, the copy
    /// mode keys or the popup). The next steps of MAX-155 change these tables.
    #[test]
    fn default_keymap_per_context() {
        let mut app = app();
        assert_eq!(
            walk(&app, "device"),
            [
                "Alt-r => Terminal(AutoResizeRemoteToEmulator)",
                "Ctrl-a => App(SetPrefix(true))",
                "Ctrl-q => App(Quit)",
                "Shift-End => History(ScrollToBottom)",
                "Shift-Home => History(ScrollToTop)",
                "Shift-PageDown => History(ScrollDownPage)",
                "Shift-PageUp => History(ScrollUpPage)"
            ]
        );

        app.update(Action::App(AppAction::SetPrefix(true)));
        assert_eq!(
            walk(&app, "App(SetPrefix(false))"),
            [
                "/ => Search(Start)",
                ": => open CommandLine",
                "? => open Help",
                "Ctrl-a => TransmitRaw([1])",
                "Down => History(ScrollDown(1))",
                "Left => History(PanLeft(1))",
                "PageDown => History(ScrollDownPage)",
                "PageUp => History(ScrollUpPage)",
                "Right => History(PanRight(1))",
                "Shift-Left => App(SideView(PanLeft(2)))",
                "Shift-Right => App(SideView(PanRight(2)))",
                "Shift-b => App(ToggleSetting(DisplayRaw))",
                "Shift-c => History(Clear)",
                "Shift-m => App(ToggleSetting(Mouse))",
                "Shift-s => App(OpenSessions)",
                "Shift-t => open Triggers",
                "Up => History(ScrollUp(1))",
                "[ => CopyMode(Enter)",
                "d => open SerialSettings",
                "f => run break",
                "h => App(DetachSerial)",
                "k => Terminal(Screenshot(Ansi))",
                "l => App(ToggleLogging)",
                "m => App(ToggleSetting(Mode))",
                "o => App(ToggleSetting(TextEncoding))",
                "p => run dtr; sleep; dtr",
                "q => App(Quit)",
                "r => App(ReloadConfig)",
                "s => open Settings",
                "t => Terminal(AutoResizeEmulatorToViewer)",
                "u => run trigger",
                "w => open TerminalSize",
                "x => App(Quit)",
                "z => open Help"
            ]
        );
        app.update(Action::App(AppAction::SetPrefix(false)));

        app.update(Action::CopyMode(CopyModeAction::Enter));
        assert_eq!(
            walk(&app, "copy keys"),
            [
                "$ => CopyMode(EndOfLine)",
                "0 => CopyMode(StartOfLine)",
                "Ctrl-a => App(SetPrefix(true))",
                "Ctrl-e => History(ScrollDown(1))",
                "Ctrl-y => History(ScrollUp(1))",
                "Down => CopyMode(MoveDown)",
                "Enter => CopyMode(Yank)",
                "Esc => CopyMode(Exit)",
                "Left => CopyMode(MoveLeft)",
                "PageDown => CopyMode(PageDown)",
                "PageUp => CopyMode(PageUp)",
                "Right => CopyMode(MoveRight)",
                "Shift-End => History(ScrollToBottom)",
                "Shift-Home => History(ScrollToTop)",
                "Shift-PageDown => History(ScrollDownPage)",
                "Shift-PageUp => History(ScrollUpPage)",
                "Shift-v => CopyMode(SelectLine)",
                "Shift-y => CopyMode(YankRaw)",
                "Space => CopyMode(Select)",
                "Tab => CopyMode(ToggleSide)",
                "Up => CopyMode(MoveUp)",
                "h => CopyMode(MoveLeft)",
                "j => CopyMode(MoveDown)",
                "k => CopyMode(MoveUp)",
                "l => CopyMode(MoveRight)",
                "q => CopyMode(Exit)",
                "v => CopyMode(Select)",
                "y => CopyMode(Yank)"
            ]
        );
        app.update(Action::CopyMode(CopyModeAction::Exit));

        app.update(Action::Search(SearchModeAction::Start));
        assert_eq!(
            walk(&app, "query"),
            [
                "Ctrl-a => App(SetPrefix(true))",
                "Ctrl-n => Search(Next)",
                "Ctrl-p => Search(Previous)",
                "Enter => Search(Confirm)",
                "Esc => Search(Cancel)",
                "Shift-End => History(ScrollToBottom)",
                "Shift-Home => History(ScrollToTop)",
                "Shift-PageDown => History(ScrollDownPage)",
                "Shift-PageUp => History(ScrollUpPage)",
                "Tab => Search(Kind)"
            ]
        );
        app.update(Action::Search(SearchModeAction::Cancel));

        let file = crate::overlays::FilePathDialog::new(
            Stream::Text,
            "log.txt".to_string(),
            "log.raw".to_string(),
        );
        app.update(Action::App(AppAction::OpenOverlay(Overlay::FilePath(file))));
        assert_eq!(
            walk(&app, "field"),
            [
                "Ctrl-n => Overlay(CycleNext)",
                "Ctrl-p => Overlay(CyclePrev)",
                "Enter => Overlay(Confirm)",
                "Esc => Overlay(Close)",
                "Shift-Tab => Overlay(CyclePrev)",
                "Tab => Overlay(CycleNext)"
            ]
        );
        app.update(Action::Overlay(DialogAction::Close));

        app.update(Action::App(AppAction::OpenOverlay(Overlay::Settings)));
        assert_eq!(
            walk(&app, "field"),
            [
                "Ctrl-d => Overlay(PageDown)",
                "Ctrl-n => Overlay(CycleNext)",
                "Ctrl-p => Overlay(CyclePrev)",
                "Ctrl-u => Overlay(PageUp)",
                "Down => Overlay(Down)",
                "End => Overlay(End)",
                "Enter => Overlay(Confirm)",
                "Esc => Overlay(Close)",
                "Home => Overlay(Home)",
                "Left => Overlay(Left)",
                "PageDown => Overlay(PageDown)",
                "PageUp => Overlay(PageUp)",
                "Right => Overlay(Right)",
                "Shift-Tab => Overlay(CyclePrev)",
                "Shift-g => Overlay(End)",
                "Space => Overlay(CycleNext)",
                "Tab => Overlay(CycleNext)",
                "Up => Overlay(Up)",
                "g => Overlay(Home)",
                "h => Overlay(Left)",
                "j => Overlay(Down)",
                "k => Overlay(Up)",
                "l => Overlay(Right)",
                "q => Overlay(Close)"
            ]
        );
        app.update(Action::Overlay(DialogAction::Close));

        app.settings_mut().mode = Mode::Line;
        assert_eq!(
            walk(&app, "editor"),
            [
                "Alt-r => LineInput(AutoResizeRemoteToWidth)",
                "Ctrl-a => App(SetPrefix(true))",
                "Ctrl-c => LineInput(Interrupt)",
                "Ctrl-q => App(Quit)",
                "Ctrl-u => LineInput(ClearLine)",
                "Down => LineInput(HistoryDown)",
                "Enter => LineInput(Submit)",
                "PageDown => History(ScrollDownPage)",
                "PageUp => History(ScrollUpPage)",
                "Shift-End => History(ScrollToBottom)",
                "Shift-Home => History(ScrollToTop)",
                "Shift-PageDown => History(ScrollDownPage)",
                "Shift-PageUp => History(ScrollUpPage)",
                "Up => LineInput(HistoryUp)"
            ]
        );
    }

    /// MAX-155: in terminal mode these keys reach the device as they were typed, the ones the
    /// default keymap took before too (Ctrl-D, Ctrl-F, Ctrl-Left/Right, PageUp/PageDown, Alt-V,
    /// Alt-Q).
    #[test]
    fn keys_reach_the_device() {
        let app = app();
        let keys = [
            "ctrl-c",
            "ctrl-d",
            "ctrl-f",
            "ctrl-left",
            "ctrl-right",
            "pageup",
            "pagedown",
            "alt-v",
            "alt-q",
            "ctrl-u",
            "ctrl-w",
            "ctrl-e",
            "ctrl-k",
            "alt-b",
            "home",
            "end",
            "shift-left",
            "shift-right",
        ];
        for key in keys {
            let pattern: KeyPattern = key.parse().unwrap();
            let Action::Terminal(TerminalAction::Key(sent)) = handle_event(event_of(pattern), &app)
            else {
                panic!("{key} doesn't reach the device");
            };
            assert_eq!(KeyPattern::from_event(&sent), pattern, "{key}");
        }
    }
}
