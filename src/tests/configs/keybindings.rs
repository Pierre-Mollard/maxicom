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

/// MAX-239: a key takes a name: an action of its section, else a named command of
/// `[commands]`. A command line on a key fails the load, saying where it goes; a word that is
/// neither is an unknown action. A mode key under a direct one is warned (MAX-12).
/// How: the ESP32 reset into the bootloader named and bound on F6 and F5, the default keys,
/// then lines and typos.
#[test]
fn keys_bind_names_only() {
    let mut config = KeyBindingsConfig::default();
    let esp32 = "dtr off; rts on; sleep 100; dtr on; rts off; sleep 50; dtr off";
    config
        .commands
        .insert("bootloader".into(), named(esp32, ""));
    config
        .prefix_actions
        .insert("f6".into(), "bootloader".into());
    config
        .direct_actions
        .insert("f5".into(), " bootloader ".into());
    config.line_mode.insert("f5".into(), "submit".into());
    let resolver = KeyResolver::try_new(&config).unwrap();
    let key = |name: &str| name.parse::<KeyPattern>().unwrap();
    let f6 = &resolver.prefix_commands[&key("f6")];
    assert_eq!((f6.name.as_str(), f6.invocations.len()), ("bootloader", 7));
    assert_eq!(f6.help, esp32);
    assert_eq!(resolver.direct_commands[&key("f5")].name, "bootloader");
    assert_eq!(
        resolver.prefix_actions[&key("D")],
        KeyBindingAction::ToggleAttach
    );
    assert!(!resolver.prefix_commands.contains_key(&key("D")));
    // the default ones: `break` alone is an action, `h` and `U` named commands
    assert_eq!(resolver.prefix_actions[&key("f")].to_string(), "break");
    assert_eq!(resolver.prefix_commands[&key("h")].name, "board-reset");
    assert_eq!(resolver.prefix_commands[&key("U")].name, "u-boot-toggle");
    assert_eq!(
        resolver.shadowed_keys(),
        ["F5 in [line_mode] (taken by [direct_actions])"]
    );

    let error = |value: &str| {
        let mut config = KeyBindingsConfig::default();
        config.prefix_actions.insert("f7".into(), value.into());
        KeyResolver::try_new(&config).err().unwrap().to_string()
    };
    let line = "in [prefix_actions]: f7: a command line goes in [commands] under a name, then bind the name";
    for value in ["send-hex 03", "dtr off; dtr on", "board-reset 3", " ; "] {
        assert_eq!(error(value), line, "{value}");
    }
    assert_eq!(error("rst"), "in [prefix_actions]: unknown action 'rst'");
    assert_eq!(
        error("trigger"),
        "in [prefix_actions]: unknown action 'trigger'"
    );
}

/// MAX-239: a keymap action name is a registry command run by a `KeyBindingAction`, and
/// back; so is a command built from its arguments that runs without any (`replay-stop`) or
/// whose arguments are all optional (`mark`, `plot`, `break`). One that needs an argument
/// isn't an action.
/// How: every command of the registry.
#[test]
fn keymap_actions_are_the_action_commands() {
    use crate::commands::{COMMANDS, Target};
    for spec in COMMANDS {
        let parsed = spec.name.parse::<KeyBindingAction>();
        match spec.target {
            Target::Action(action) => {
                assert_eq!(parsed.unwrap(), action, "{}", spec.name);
                assert_eq!(action.to_string(), spec.name);
            }
            Target::Build(_) if spec.runs_bare() => {
                let action = parsed.unwrap();
                assert_eq!(action, KeyBindingAction::Command(spec));
                assert_eq!(action.to_string(), spec.name);
            }
            Target::Build(_) => assert!(parsed.is_err(), "{}", spec.name),
        }
    }
    let bare: Vec<_> = (COMMANDS.iter())
        .filter(|spec| matches!(spec.target, Target::Build(_)) && spec.runs_bare())
        .map(|spec| spec.name)
        .collect();
    assert_eq!(
        bare,
        [
            "mark",
            "jump-mark",
            "link-save",
            "session-name",
            "replay-stop",
            "replay-pause",
            "replay-resume",
            "replay-restart",
            "break",
            "plot"
        ]
    );
}

/// MAX-239: a command that runs without arguments, bound bare, is an action. With its
/// arguments it is a command line, which a key doesn't take: a named command does.
/// How: `mark`, `replay-stop` and `session-name` bare on prefix keys, the defaults, then
/// `mark`, `plot`, `break` and `session-name` with an argument.
#[test]
fn optional_argument_commands_bound_bare_are_actions() {
    let mut config = KeyBindingsConfig::default();
    for (key, name) in [
        ("f1", "mark"),
        ("f5", "replay-stop"),
        ("f6", "session-name"),
    ] {
        config.prefix_actions.insert(key.into(), name.into());
    }
    let resolver = KeyResolver::try_new(&config).unwrap();
    let key = |name: &str| name.parse::<KeyPattern>().unwrap();
    let action = |name: &str| {
        assert!(!resolver.prefix_commands.contains_key(&key(name)), "{name}");
        resolver.prefix_actions[&key(name)].to_string()
    };
    assert_eq!(action("f1"), "mark");
    assert_eq!(action("f5"), "replay-stop");
    assert_eq!(action("f6"), "session-name");
    // `mark` alone is copy mode's own action
    assert_eq!(resolver.copy_actions[&key(",")], KeyBindingCopyAction::Mark);
    // the default bare ones
    for (name, command) in [
        (",", "mark"),
        ("f", "break"),
        ("g", "plot"),
        ("L", "replay-stop"),
    ] {
        assert_eq!(action(name), command);
    }

    for line in ["mark a", "plot off", "break 100", "session-name boot"] {
        let mut config = KeyBindingsConfig::default();
        config.prefix_actions.insert("f2".into(), line.into());
        let error = KeyResolver::try_new(&config).err().unwrap();
        assert!(matches!(error, KeymapError::LineOnKey { .. }), "{line}");
    }
}

/// MAX-239: every action of every keymap section has a help for the help popup: a global
/// action its command's, the others their own.
/// How: every command of the registry, and every name of the other sections.
#[test]
fn every_action_has_a_help() {
    for spec in crate::commands::COMMANDS {
        if let Ok(action) = spec.name.parse::<KeyBindingAction>() {
            assert_eq!(action.help(), spec.help, "{}", spec.name);
            assert!(!action.help().is_empty(), "{}", spec.name);
        }
    }
    fn helps<A: FromStr + Copy>(names: &[&str], help: fn(A) -> &'static str) {
        for name in names {
            let action = name.parse::<A>().ok().expect(name);
            assert!(!help(action).is_empty(), "{name}");
        }
    }
    helps::<KeyBindingLineAction>(
        &[
            "submit",
            "history-prev",
            "history-next",
            "auto-resize-remote-line",
            "clear-line",
            "interrupt",
            "page-up",
            "page-down",
            "quit",
        ],
        KeyBindingLineAction::help,
    );
    helps::<KeyBindingTerminalAction>(
        &["auto-resize-remote", "auto-resize-emulator", "quit"],
        KeyBindingTerminalAction::help,
    );
    helps::<KeyBindingSearchAction>(
        &[
            "next",
            "previous",
            "cancel",
            "confirm",
            "search-kind",
            "history-prev",
            "history-next",
        ],
        KeyBindingSearchAction::help,
    );
    helps::<KeyBindingCopyAction>(
        &[
            "up",
            "down",
            "left",
            "right",
            "page-up",
            "page-down",
            "start-of-line",
            "end-of-line",
            "select",
            "select-line",
            "yank",
            "yank-side-inspector",
            "toggle-side",
            "exit",
            "scroll-up",
            "scroll-down",
            "scroll-page-up",
            "scroll-page-down",
            "mark",
            "marks",
            "prev-mark",
            "next-mark",
            "search",
            "search-backward",
            "search-next",
            "search-prev",
            "search-word",
            "search-word-backward",
        ],
        KeyBindingCopyAction::help,
    );
    helps::<KeyBindingDialogAction>(
        &[
            "close",
            "confirm",
            "up",
            "down",
            "left",
            "right",
            "cycle-next",
            "cycle-prev",
            "page-up",
            "page-down",
            "home",
            "end",
        ],
        KeyBindingDialogAction::help,
    );
}

fn named(run: &str, help: &str) -> NamedCommandConfig {
    NamedCommandConfig {
        run: run.to_string(),
        help: help.to_string(),
    }
}

/// MAX-239: `[commands]` names command lines. A key bound to a name runs its line and has its
/// help, or its line without one, in every section but the search bar and the popups. The
/// default `Ctrl-a h` runs `board-reset`, `Ctrl-a U` `u-boot-toggle`.
/// How: `boot` with a help and `ping` without, bound in each section.
#[test]
fn named_commands_bound_to_keys() {
    let mut config = KeyBindingsConfig::default();
    config.commands.insert(
        "boot".into(),
        named("dtr off; rts on", "Into the bootloader"),
    );
    config
        .commands
        .insert("ping".into(), named(" send AT ", ""));
    config.prefix_actions.insert("f6".into(), "boot".into());
    config.direct_actions.insert("f8".into(), "ping".into());
    config.line_mode.insert("f9".into(), "ping".into());
    config.terminal_mode.insert("f9".into(), "boot".into());
    config.copy_mode.insert("x".into(), "boot".into());
    let resolver = KeyResolver::try_new(&config).unwrap();
    let key = |name: &str| name.parse::<KeyPattern>().unwrap();
    let names = |command: &BoundCommand| -> Vec<&str> {
        command.invocations.iter().map(|i| i.spec.name).collect()
    };

    let f6 = &resolver.prefix_commands[&key("f6")];
    assert_eq!(names(f6), ["dtr", "rts"]);
    assert_eq!(f6.help, "Into the bootloader");
    let f8 = &resolver.direct_commands[&key("f8")];
    assert_eq!((names(f8), f8.help.as_str()), (vec!["send"], "send AT"));
    assert_eq!(resolver.line_commands[&key("f9")].name, "ping");
    assert_eq!(resolver.terminal_commands[&key("f9")].name, "boot");
    assert_eq!(
        resolver.copy_commands[&key("x")].help,
        "Into the bootloader"
    );

    let commands: Vec<_> = resolver.commands.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(commands, ["board-reset", "boot", "ping", "u-boot-toggle"]);
    let h = &resolver.prefix_commands[&key("h")];
    assert_eq!(names(h), ["dtr", "sleep", "dtr"]);
    assert_eq!(h.help, "Pulse DTR, which resets most boards");
    let u = &resolver.prefix_commands[&key("U")];
    assert_eq!(names(u), ["trigger"]);
}

/// MAX-239: a `[commands]` entry that can't be stops the keymap from loading, naming it: a
/// name that isn't one word or that a built-in command or a mode's action has, a line that
/// doesn't parse or is empty, a named command running another.
#[test]
fn named_command_errors() {
    let error = |name: &str, run: &str| {
        let mut config = KeyBindingsConfig::default();
        config.commands.insert("inner".into(), named("break", ""));
        config.commands.insert(name.into(), named(run, ""));
        KeyResolver::try_new(&config).err().unwrap().to_string()
    };
    assert_eq!(
        error("my reset", "break"),
        "in [commands]: my reset: a name takes letters, digits, - and _"
    );
    assert_eq!(
        error("break", "break 100"),
        "in [commands]: break: a built-in command has this name"
    );
    for action in ["select", "up", "submit", "close"] {
        assert_eq!(
            error(action, "break"),
            format!("in [commands]: {action}: an action has this name")
        );
    }
    assert_eq!(
        error("x", "rst on"),
        "in [commands]: x: unknown command 'rst'"
    );
    assert_eq!(error("x", " ; "), "in [commands]: x: nothing to run");
    assert_eq!(
        error("x", "break; inner"),
        "in [commands]: x: runs 'inner': a named command can't run another"
    );
}

/// MAX-239: a keymap without `[commands]` gets the default ones; one with its own keeps
/// them, the default ones it lacks added as new defaults (`--write-config`).
#[test]
fn named_command_defaults() {
    let config: KeyBindingsConfig = toml::from_str("prefix = \"ctrl-a\"").unwrap();
    assert!(config.commands.contains_key("board-reset"));

    let mut config: KeyBindingsConfig =
        toml::from_str("[commands]\nboot = { run = \"break\" }").unwrap();
    assert_eq!(config.commands.keys().collect::<Vec<_>>(), ["boot"]);
    assert_eq!(config.commands["boot"].help, "");
    let added = config.add_new_defaults();
    assert!(
        added.contains(&"commands: board-reset = dtr off; sleep 100; dtr on".to_string()),
        "{added:?}"
    );
    assert_eq!(
        config.commands.keys().collect::<Vec<_>>(),
        ["board-reset", "boot", "u-boot-toggle"]
    );
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
