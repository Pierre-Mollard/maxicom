//! Keys reaching navigation while the search bar is open (MAX-72), and popup keys (MAX-73).

use super::*;
use crate::{
    configs::{KeyBindingsConfig, KeyResolver, Mode, SettingsConfig},
    link::LinkConfig,
    overlays::SettingsDialog,
};
use crossterm::event::{KeyCode, KeyModifiers};

fn app() -> App {
    App::new(
        SettingsConfig::default(),
        KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap(),
        LinkConfig::default(),
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
        LinkConfig::default(),
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
    let file =
        crate::overlays::FilePathDialog::new(crate::capture::Stream::Text, "log.txt".to_string());
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

    app.update(Action::App(AppAction::OpenOverlay(Overlay::Settings(
        SettingsDialog::new(),
    ))));
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
/// mode keys or the popup). MAX-181: the prefix keys follow minicom's.
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
            "' => run jump-mark",
            "( => App(Mark(Adjacent(false)))",
            ") => App(Mark(Adjacent(true)))",
            ", => run mark",
            "/ => Search(Start)",
            ": => open CommandLine",
            "< => App(ResizeSidePanel(-5))",
            "> => App(ResizeSidePanel(5))",
            "? => open Help",
            "B => App(ToggleSetting(DisplayRaw))",
            "C => History(Clear)",
            "Ctrl-a => TransmitRaw([1])",
            "D => App(ToggleAttach)",
            "Down => History(ScrollDown(1))",
            "E => Terminal(AutoResizeEmulatorToViewer)",
            "K => Terminal(Screenshot(Text))",
            "L => run replay-stop",
            "Left => History(PanLeft(1))",
            "M => App(ToggleSetting(Mouse))",
            "N => App(DismissToasts)",
            "O => App(ToggleSetting(TextEncoding))",
            "PageDown => History(ScrollDownPage)",
            "PageUp => History(ScrollUpPage)",
            "Right => History(PanRight(1))",
            "Shift-Left => App(SideView(PanLeft(2)))",
            "Shift-Right => App(SideView(PanRight(2)))",
            "T => open Triggers",
            "U => run trigger",
            "Up => History(ScrollUp(1))",
            "W => open TerminalSize",
            "[ => CopyMode(Enter)",
            "a => App(ToggleSetting(AddLf))",
            "b => CopyMode(Enter)",
            "c => History(ClearView)",
            "d => open Directory",
            "e => App(ToggleSetting(LocalEcho))",
            "f => run break",
            "h => run dtr; sleep; dtr",
            "j => App(OpenSessions)",
            "k => Terminal(Screenshot(Ansi))",
            "l => App(ToggleRecording)",
            "m => App(ToggleSetting(Mode))",
            "n => App(DismissToast(None))",
            "o => open Settings",
            "p => open LinkSettings",
            "q => App(Quit)",
            "r => App(ReloadConfig)",
            "s => open Settings",
            "t => App(ToggleSetting(DisplayTimestamp))",
            "u => App(ToggleSetting(AddCr))",
            "w => App(ToggleSetting(LineWrap))",
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
            "' => run jump-mark",
            "( => App(Mark(Adjacent(false)))",
            ") => App(Mark(Adjacent(true)))",
            ", => run mark",
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
            "Space => CopyMode(Select)",
            "Tab => CopyMode(ToggleSide)",
            "Up => CopyMode(MoveUp)",
            "V => CopyMode(SelectLine)",
            "Y => CopyMode(YankRaw)",
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

    let file =
        crate::overlays::FilePathDialog::new(crate::capture::Stream::Text, "log.txt".to_string());
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

    app.update(Action::App(AppAction::OpenOverlay(Overlay::Settings(
        SettingsDialog::new(),
    ))));
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
            "G => Overlay(End)",
            "Home => Overlay(Home)",
            "Left => Overlay(Left)",
            "PageDown => Overlay(PageDown)",
            "PageUp => Overlay(PageUp)",
            "Right => Overlay(Right)",
            "Shift-Tab => Overlay(CyclePrev)",
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
            "Ctrl-n => LineInput(HistoryDown)",
            "Ctrl-p => LineInput(HistoryUp)",
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

/// MAX-181: line wrap cycles line mode's wrap in both modes; in terminal mode it also
/// warns that the emulator isn't wrapped by it (it always wraps).
/// How: prefix + `w` in line mode, then in terminal mode.
#[test]
fn line_wrap_warns_in_terminal_mode() {
    use crate::{app::tests::toasts, configs::WrapMode};
    let mut app = app();
    let wrap_key = |app: &mut App| {
        app.update(Action::App(AppAction::SetPrefix(true)));
        let action = handle_event(plain(KeyCode::Char('w')), app);
        app.update(action);
        (app.settings().line_mode.wrap_mode, toasts(app))
    };
    app.settings_mut().mode = Mode::Line;
    assert_eq!(wrap_key(&mut app), (WrapMode::Char, vec![]));
    app.settings_mut().mode = Mode::Terminal;
    let warned = vec!["Line wrap: line mode only".to_string()];
    assert_eq!(wrap_key(&mut app), (WrapMode::Truncate, warned));
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

/// MAX-180: a resize asks for a redraw: the old frame stayed up to the next frame due
/// (a second), so a slow resize lagged.
/// How: a resize event through `handle_event`.
#[test]
fn resize_redraws() {
    let action = handle_event(Event::Resize(150, 35), &app());
    assert!(matches!(action, Action::Redraw), "{action:?}");
}

/// The marks of the current mode's view (MAX-164)
fn marks(app: &App) -> crate::app::marks::Marks {
    match app.settings().mode {
        Mode::Line => app.line_mode.marks().clone(),
        Mode::Terminal => app.terminal_mode.marks().clone(),
    }
}

fn last_toast(app: &App) -> String {
    let toasts = app.notifications.active_toasts();
    toasts.last().map(|toast| toast.message.clone()).unwrap()
}

/// MAX-164: prefix `,` (`mark` without a letter) waits for a mark's letter: a letter marks the
/// line, Enter takes the first free letter, anything else (Esc, the prefix) cancels; a toast
/// says what happened. Prefix `'` (`jump-mark`) waits for the letter to jump to. In copy mode
/// the same keys run the same command lines: `,` and `'` wait the same way, `(` and `)` walk
/// the marks. In both modes.
/// How: two lines received, then the keys one by one, as the loop runs them (a bound command
/// line queued, then run).
#[test]
fn mark_keys_wait_for_a_letter() {
    for mode in [Mode::Line, Mode::Terminal] {
        let mut app = app();
        app.settings_mut().mode = mode;
        app.feed_rx(b"one\r\ntwo\r\n");
        let press = |app: &mut App, event| {
            let action = handle_event(event, app);
            app.update(action);
            while let Some(action) = app.next_command_action(std::time::Instant::now()) {
                app.update(action);
            }
        };
        let char_key = |c| plain(KeyCode::Char(c));

        press(&mut app, ctrl('a'));
        press(&mut app, char_key(','));
        assert_eq!(app.pending_mark(), Some(PendingMark::Add), "{mode:?}");
        press(&mut app, char_key('c'));
        assert_eq!(app.pending_mark(), None);
        assert!(marks(&app).get('c').is_some(), "{mode:?}");
        assert_eq!(last_toast(&app), "mark c");

        press(&mut app, ctrl('a'));
        press(&mut app, char_key(','));
        press(&mut app, plain(KeyCode::Enter));
        assert!(marks(&app).get('a').is_some(), "{mode:?}");
        assert_eq!(last_toast(&app), "mark a");

        for cancel in [plain(KeyCode::Esc), ctrl('a'), char_key('A')] {
            press(&mut app, ctrl('a'));
            press(&mut app, char_key(','));
            press(&mut app, cancel);
            assert_eq!(app.pending_mark(), None);
            assert!(!app.is_prefix_active(), "{mode:?}");
            assert_eq!(marks(&app).sorted().len(), 2, "{mode:?}");
        }

        press(&mut app, ctrl('a'));
        press(&mut app, char_key('\''));
        assert_eq!(app.pending_mark(), Some(PendingMark::Jump), "{mode:?}");
        press(&mut app, char_key('c'));
        assert!(!app.is_copy_mode_active());

        app.update(Action::CopyMode(CopyModeAction::Enter));
        press(&mut app, char_key('\''));
        assert_eq!(app.pending_mark(), Some(PendingMark::Jump));
        press(&mut app, char_key('z'));
        assert_eq!(last_toast(&app), "mark z is not set");
        press(&mut app, char_key(','));
        press(&mut app, char_key('d'));
        assert_eq!(last_toast(&app), "mark d");
        press(&mut app, char_key('('));
        assert_eq!(last_toast(&app), "no mark above", "{mode:?}");
        assert!(app.is_copy_mode_active());
    }
}
