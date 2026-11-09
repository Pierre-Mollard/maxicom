use super::*;

fn keys(resolver: &KeyResolver, action: KeyBindingDialogAction) -> Vec<String> {
    resolver
        .dialog_keys(action)
        .iter()
        .map(|k| k.to_string())
        .collect()
}

/// The keys of an action come in the same order whatever the map's: plain keys before
/// modified ones, named keys before letters (MAX-73).
/// How: several resolvers from the default keymap, each map with its own random seed.
#[test]
fn dialog_keys_in_a_fixed_order() {
    for _ in 0..8 {
        let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
        assert_eq!(keys(&resolver, KeyBindingDialogAction::Close), ["Esc", "q"]);
        assert_eq!(
            keys(&resolver, KeyBindingDialogAction::CycleNext),
            ["Tab", "Space", "Ctrl-n"]
        );
        assert_eq!(
            keys(&resolver, KeyBindingDialogAction::CyclePrev),
            ["Shift-Tab", "Ctrl-p"]
        );
    }

    let mut config = KeyBindingsConfig::default();
    config.dialog_actions.retain(|_, action| action != "close");
    let resolver = KeyResolver::try_new(&config).unwrap();
    assert!(keys(&resolver, KeyBindingDialogAction::Close).is_empty());
}

/// Reload is on prefix + r (MAX-112), and the resize dialog on prefix + W: `w` is
/// minicom's line wrap (MAX-181).
#[test]
fn reload_and_resize_default_keys() {
    let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
    let prefix_action = |key: &str| resolver.prefix_actions[&key.parse().unwrap()];
    assert_eq!(prefix_action("r"), KeyBindingAction::ReloadConfig);
    assert_eq!(prefix_action("W"), KeyBindingAction::ResizeDialog);
    assert_eq!(
        "reload-config".parse::<KeyBindingAction>().unwrap(),
        KeyBindingAction::ReloadConfig
    );
    assert_eq!(KeyBindingAction::ReloadConfig.to_string(), "reload-config");
}

/// MAX-129: Ctrl-C is no global quit key: in line mode it is `interrupt`, in terminal mode
/// unbound so it is sent to the device. Ctrl-Q still quits.
#[test]
fn ctrl_c_default_key() {
    let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
    let ctrl_c = "ctrl-c".parse().unwrap();
    assert!(!resolver.direct_actions.contains_key(&ctrl_c));
    assert!(!resolver.terminal_actions.contains_key(&ctrl_c));
    let action = resolver.line_actions[&ctrl_c];
    assert_eq!(action, KeyBindingLineAction::Interrupt);
    assert_eq!(action.to_string(), "interrupt");
    assert_eq!("interrupt".parse::<KeyBindingLineAction>().unwrap(), action);
    assert_eq!(
        resolver.direct_actions[&"ctrl-q".parse().unwrap()],
        KeyBindingAction::Quit
    );
}

/// MAX-127: `V` selects whole lines in copy mode by default.
#[test]
fn select_line_default_key() {
    let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
    let action = resolver.copy_actions[&"V".parse().unwrap()];
    assert_eq!(action, KeyBindingCopyAction::SelectLine);
    assert_eq!(action.to_string(), "select-line");
    assert_eq!(
        "select-line".parse::<KeyBindingCopyAction>().unwrap(),
        action
    );
}

/// MAX-148: prefix + `j` opens the session browser (MAX-181: `d` is the directory, as
/// minicom's), and `o` the settings, minicom's configure key.
#[test]
fn sessions_default_key() {
    let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
    let action = resolver.prefix_actions[&"j".parse().unwrap()];
    assert_eq!(action, KeyBindingAction::Sessions);
    assert_eq!(action.to_string(), "sessions");
    assert_eq!("sessions".parse::<KeyBindingAction>().unwrap(), action);
    let settings = resolver.prefix_actions[&"o".parse().unwrap()];
    assert_eq!(settings, KeyBindingAction::Settings);
}

/// MAX-155: clear_history moved from Ctrl-D (stolen from the remote) to prefix + `C`,
/// shifted because it wipes the history; `c` is the `clear` that keeps it (MAX-176).
#[test]
fn clear_history_default_key() {
    let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
    let action = resolver.prefix_actions[&"C".parse().unwrap()];
    assert_eq!(action, KeyBindingAction::ClearHistory);
    assert!(
        !resolver
            .direct_actions
            .contains_key(&"ctrl-d".parse().unwrap())
    );
    let action = resolver.prefix_actions[&"c".parse().unwrap()];
    assert_eq!(action, KeyBindingAction::Clear);
}

/// MAX-155: a key bound in a mode section is reported when the prefix or `[direct_actions]`
/// take it first; the default keymap has none.
#[test]
fn shadowed_keys() {
    let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
    assert!(resolver.shadowed_keys().is_empty());

    let mut config = KeyBindingsConfig::default();
    let bind = |section: &mut BTreeMap<String, String>, key: &str, action: &str| {
        section.insert(key.to_string(), action.to_string());
    };
    bind(&mut config.direct_actions, "alt-x", "quit");
    bind(&mut config.line_mode, "alt-x", "submit");
    bind(&mut config.terminal_mode, "alt-x", "quit");
    bind(&mut config.terminal_mode, "ctrl-a", "quit");
    bind(&mut config.direct_actions, "ctrl-a", "quit");
    let resolver = KeyResolver::try_new(&config).unwrap();
    assert_eq!(
        resolver.shadowed_keys(),
        [
            "Alt-x in [line_mode] (taken by [direct_actions])",
            "Alt-x in [terminal_mode] (taken by [direct_actions])",
            "Ctrl-a in [direct_actions] (taken by the prefix)",
            "Ctrl-a in [terminal_mode] (taken by the prefix)",
        ]
    );
}

/// MAX-155: only these dialog actions work in a text input popup.
#[test]
fn text_input_dialog_actions() {
    use KeyBindingDialogAction as A;
    for action in [A::Close, A::Confirm, A::CycleNext, A::CyclePrev] {
        assert!(action.is_text_input_action(), "{action}");
    }
    for action in [
        A::PageUp,
        A::PageDown,
        A::Home,
        A::End,
        A::NavUp,
        A::NavLeft,
    ] {
        assert!(!action.is_text_input_action(), "{action}");
    }
}

#[test]
fn mouse_toggle_default_key() {
    let resolver = KeyResolver::try_new(&KeyBindingsConfig::default()).unwrap();
    let action = resolver.prefix_actions[&"M".parse().unwrap()];
    assert_eq!(action, KeyBindingAction::ToggleMouse);
    assert_eq!(action.to_string(), "toggle-mouse");
    assert_eq!("toggle-mouse".parse::<KeyBindingAction>().unwrap(), action);
}

/// The action names of `keymap.toml` come from the command registry (MAX-12), with `-`
/// between words: the old `_` names are refused
#[test]
fn action_names() {
    const NAMES: [(&str, KeyBindingAction); 54] = [
        ("quit", KeyBindingAction::Quit),
        ("toggle-input-mode", KeyBindingAction::SKInputMode),
        ("toggle-local-echo", KeyBindingAction::SKLocalEcho),
        ("toggle-line-wrap", KeyBindingAction::SKLineWrap),
        (
            "toggle-display-timestamp",
            KeyBindingAction::SKDisplayTimestamp,
        ),
        ("toggle-text-encoding", KeyBindingAction::SKTextEncoding),
        ("toggle-indent-wrap", KeyBindingAction::SKIndentWrap),
        ("toggle-display-mode", KeyBindingAction::SKDisplayMode),
        ("toggle-display-raw", KeyBindingAction::SKDisplayRaw),
        ("screenshot-bytes", KeyBindingAction::ScreenshotBytes),
        ("screenshot-text", KeyBindingAction::ScreenshotTxt),
        ("help", KeyBindingAction::Help),
        ("settings", KeyBindingAction::Settings),
        ("record", KeyBindingAction::Record),
        ("resize-dialog", KeyBindingAction::ResizeDialog),
        ("auto-resize-remote", KeyBindingAction::AutoResizeRemote),
        (
            "auto-resize-remote-line",
            KeyBindingAction::AutoResizeRemoteLine,
        ),
        ("auto-resize-emulator", KeyBindingAction::AutoResizeEmulator),
        ("link-settings", KeyBindingAction::LinkSettings),
        ("enter-copy-mode", KeyBindingAction::EnterCopyMode),
        ("enter-search", KeyBindingAction::EnterSearch),
        (
            "enter-search-backward",
            KeyBindingAction::EnterSearchBackward,
        ),
        ("scroll-up", KeyBindingAction::ScrollUp),
        ("scroll-down", KeyBindingAction::ScrollDown),
        ("scroll-to-bottom", KeyBindingAction::ScrollToBottom),
        ("scroll-to-top", KeyBindingAction::ScrollToTop),
        ("page-up", KeyBindingAction::PageUp),
        ("page-down", KeyBindingAction::PageDown),
        ("pan-right", KeyBindingAction::PanRight),
        ("pan-left", KeyBindingAction::PanLeft),
        ("pan-home", KeyBindingAction::PanHome),
        ("pan-end", KeyBindingAction::PanEnd),
        ("pan-side-right", KeyBindingAction::PanSideRight),
        ("pan-side-left", KeyBindingAction::PanSideLeft),
        ("clear-history", KeyBindingAction::ClearHistory),
        ("sessions", KeyBindingAction::Sessions),
        ("exit-prefix", KeyBindingAction::ExitPrefix),
        ("toggle-attach", KeyBindingAction::ToggleAttach),
        ("reload-config", KeyBindingAction::ReloadConfig),
        ("toggle-mouse", KeyBindingAction::ToggleMouse),
        ("command-line", KeyBindingAction::CommandLine),
        ("cancel", KeyBindingAction::CancelCommands),
        ("toggle-triggers", KeyBindingAction::ToggleTriggers),
        ("triggers", KeyBindingAction::Triggers),
        ("clear", KeyBindingAction::Clear),
        ("toggle-add-cr", KeyBindingAction::SKAddCr),
        ("toggle-add-lf", KeyBindingAction::SKAddLf),
        ("directory", KeyBindingAction::Directory),
        ("side-grow", KeyBindingAction::SideGrow),
        ("side-shrink", KeyBindingAction::SideShrink),
        ("side-grow-5", KeyBindingAction::SideGrow5),
        ("side-shrink-5", KeyBindingAction::SideShrink5),
        (
            "dismiss-notification",
            KeyBindingAction::DismissNotification,
        ),
        (
            "dismiss-notifications",
            KeyBindingAction::DismissNotifications,
        ),
    ];
    for (name, action) in NAMES {
        assert_eq!(name.parse::<KeyBindingAction>().unwrap(), action, "{name}");
        assert_eq!(action.to_string(), name);
    }
    assert!("toggle_local_echo".parse::<KeyBindingAction>().is_err());
    assert!("".parse::<KeyBindingAction>().is_err());
}

/// MAX-12: `[prefix_actions]` and `[direct_actions]` take a command line where an action
/// name goes, parsed when the keymap loads; an action name stays an action. A value that
/// is neither fails the load, naming its key. A mode key under a direct line is warned.
/// How: the ESP32 reset into the bootloader on F6, `send-hex` on F5, the default break and
/// DTR pulse, then typos.
#[test]
fn bound_command_lines() {
    let mut config = KeyBindingsConfig::default();
    let esp32 = "dtr off; rts on; sleep 100; dtr on; rts off; sleep 50; dtr off";
    config.prefix_actions.insert("f6".into(), esp32.into());
    config
        .direct_actions
        .insert("f5".into(), " send-hex 03 ".into());
    config.line_mode.insert("f5".into(), "submit".into());
    let resolver = KeyResolver::try_new(&config).unwrap();
    let key = |name: &str| name.parse::<KeyPattern>().unwrap();
    let f6 = &resolver.prefix_commands[&key("f6")];
    assert_eq!((f6.text.as_str(), f6.invocations.len()), (esp32, 7));
    assert_eq!(resolver.direct_commands[&key("f5")].text, "send-hex 03");
    assert_eq!(
        resolver.prefix_actions[&key("D")],
        KeyBindingAction::ToggleAttach
    );
    assert!(!resolver.prefix_commands.contains_key(&key("D")));
    // the default ones
    assert_eq!(resolver.prefix_commands[&key("f")].invocations.len(), 1);
    assert_eq!(resolver.prefix_commands[&key("h")].invocations.len(), 3);
    assert_eq!(
        resolver.shadowed_keys(),
        ["F5 in [line_mode] (taken by [direct_actions])"]
    );

    let error = |value: &str| {
        let mut config = KeyBindingsConfig::default();
        config.prefix_actions.insert("f7".into(), value.into());
        KeyResolver::try_new(&config).err().unwrap().to_string()
    };
    assert_eq!(
        error("rst on"),
        "in [prefix_actions]: f7: unknown command 'rst'"
    );
    assert_eq!(
        error("baud 10"),
        "in [prefix_actions]: f7: baud: bad argument 1: 10 is out of 50..=4000000"
    );
    assert_eq!(error(" ; "), "in [prefix_actions]: unknown action ' ; '");
}

/// Keys a text input popup still sends to `[dialog_actions]`: the others are typed.
#[test]
fn modal_keys() {
    for key in ["esc", "enter", "tab", "backtab", "ctrl-n", "alt-x"] {
        assert!(key.parse::<KeyPattern>().unwrap().is_modal(), "{key}");
    }
    for key in ["q", "space", "down", "shift-x"] {
        assert!(!key.parse::<KeyPattern>().unwrap().is_modal(), "{key}");
    }
}

/// The help's key order: letters with each shifted one after its plain one, symbols, named
/// keys each followed by its Shift version, then the Ctrl keys in the same order.
#[test]
fn help_order_pairs_shifted_keys() {
    let mut keys: Vec<KeyPattern> = [
        "ctrl-a",
        "Shift-Left",
        "?",
        "C",
        "Left",
        "k",
        "/",
        "pageup",
        "K",
        "c",
        "ctrl-C",
        "f5",
        "enter",
        "Shift-PageUp",
    ]
    .iter()
    .map(|key| key.parse().unwrap())
    .collect();
    keys.sort_by_key(KeyPattern::help_order);
    let labels: Vec<String> = keys.iter().map(|key| key.to_string()).collect();
    assert_eq!(
        labels,
        [
            "c",
            "C",
            "k",
            "K",
            "/",
            "?",
            "Left",
            "Shift-Left",
            "PageUp",
            "Shift-PageUp",
            "Enter",
            "F5",
            "Ctrl-a",
            "Ctrl-C",
        ]
    );
}
