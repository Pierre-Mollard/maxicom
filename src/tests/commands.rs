use super::*;

const TEST_ARGS: &[ArgSpec] = &[
    ArgSpec {
        name: "rate",
        kind: ArgKind::Int {
            min: 1,
            max: 4_000_000,
            suggestions: &["9600", "115200"],
        },
        optional: false,
    },
    ArgSpec {
        name: "state",
        kind: ArgKind::Bool,
        optional: false,
    },
    ArgSpec {
        name: "flow",
        kind: ArgKind::Enum(&["none", "hw", "sw"]),
        optional: false,
    },
    ArgSpec {
        name: "label",
        kind: ArgKind::String,
        optional: false,
    },
    ArgSpec {
        name: "file",
        kind: ArgKind::Path,
        optional: false,
    },
    ArgSpec {
        name: "bytes",
        kind: ArgKind::Hex,
        optional: true,
    },
];

/// A command with an argument of each kind, and one without
static TEST_COMMANDS: [CommandSpec; 2] = [
    CommandSpec {
        name: "test",
        args: TEST_ARGS,
        help: "",
        target: Target::Action(KeyBindingAction::Quit),
    },
    CommandSpec::action("quit", KeyBindingAction::Quit, "Quit"),
];

fn args(line: &str) -> Result<Vec<ArgValue>, CommandError> {
    let words = tokenizer::split(line, |_| false).unwrap().remove(0);
    parse_args(&TEST_COMMANDS[0], &words[1..])
}

fn bad_argument(line: &str) -> (usize, String) {
    match args(line) {
        Err(CommandError::BadArgument { index, reason, .. }) => (index, reason),
        other => panic!("{line}: {other:?}"),
    }
}

/// The position of an action in the enum. The match stops compiling when an action is added:
/// give it the next index and a command in the registry
fn action_index(action: KeyBindingAction) -> usize {
    use KeyBindingAction as A;
    match action {
        A::Quit => 0,
        A::SKInputMode => 1,
        A::SKLocalEcho => 2,
        A::SKLineWrap => 3,
        A::SKDisplayTimestamp => 4,
        A::SKTextEncoding => 5,
        A::SKIndentWrap => 6,
        A::SKDisplayMode => 7,
        A::SKDisplayRaw => 8,
        A::Help => 9,
        A::Settings => 10,
        A::Record => 11,
        A::ResizeDialog => 12,
        A::AutoResizeRemote => 13,
        A::AutoResizeRemoteLine => 14,
        A::AutoResizeEmulator => 15,
        A::ScreenshotTxt => 16,
        A::ScreenshotBytes => 17,
        A::LinkSettings => 18,
        A::EnterCopyMode => 19,
        A::EnterSearch => 20,
        A::ScrollUp => 21,
        A::ScrollDown => 22,
        A::ScrollToBottom => 23,
        A::ScrollToTop => 24,
        A::PageUp => 25,
        A::PageDown => 26,
        A::PanRight => 27,
        A::PanLeft => 28,
        A::PanHome => 29,
        A::PanSideRight => 30,
        A::PanSideLeft => 31,
        A::ClearHistory => 32,
        A::Sessions => 33,
        A::ToggleAttach => 34,
        A::ExitPrefix => 35,
        A::ReloadConfig => 36,
        A::ToggleMouse => 37,
        A::CommandLine => 38,
        A::CancelCommands => 39,
        A::ToggleTriggers => 40,
        A::Triggers => 41,
        A::Clear => 42,
        A::SKAddCr => 43,
        A::SKAddLf => 44,
        A::Directory => 45,
        A::SideGrow => 46,
        A::SideShrink => 47,
        A::SideGrow5 => 48,
        A::SideShrink5 => 49,
        A::DismissNotification => 50,
        A::DismissNotifications => 51,
        A::PrevMark => 52,
        A::NextMark => 53,
        A::Marks => 54,
        A::EnterSearchBackward => 55,
        A::PanEnd => 56,
    }
}

/// Every action is one zero-argument command, found by its name and by its action
#[test]
fn every_action_has_one_command() {
    let mut indexes: Vec<usize> = COMMANDS
        .iter()
        .filter_map(|spec| match spec.target {
            Target::Action(action) => {
                assert!(spec.args.is_empty(), "{}", spec.name);
                assert_eq!(find_by_name(spec.name), Some(spec));
                assert_eq!(find_by_action(action), Some(spec));
                Some(action_index(action))
            }
            Target::Build(_) => None,
        })
        .collect();
    indexes.sort_unstable();
    assert_eq!(indexes, (0..57).collect::<Vec<_>>());
}

/// The help writes a required argument in angle brackets, an optional one in square ones
#[test]
fn usage() {
    let usage = |name| find_by_name(name).unwrap().usage();
    assert_eq!(usage("quit"), "quit");
    assert_eq!(usage("dtr"), "dtr <state>");
    assert_eq!(usage("send-file"), "send-file <path> [line-delay-ms]");
}

/// `send` takes any bytes, escapes and all, `send-hex` hex digits; both are bytes
#[test]
fn send_arguments() {
    let bytes = |line: &str| {
        let invocations = parse(line).unwrap();
        assert_eq!(invocations[0].args.len(), 1);
        invocations[0].args[0].clone()
    };
    assert_eq!(bytes("send AT\\r\\n"), ArgValue::Bytes(b"AT\r\n".to_vec()));
    assert_eq!(bytes("send '\\xff' "), ArgValue::Bytes(b"\\xff".to_vec()));
    assert_eq!(bytes("send \\xff\\x00"), ArgValue::Bytes(vec![0xff, 0]));
    assert_eq!(bytes("send-hex 0D0a"), ArgValue::Bytes(vec![0x0d, 0x0a]));
    assert_eq!(
        parse("send").unwrap_err().to_string(),
        "send: missing argument <text>"
    );
    assert_eq!(
        parse("send-hex 0d 0a").unwrap_err().to_string(),
        "send-hex takes 1 argument(s) at most"
    );
    assert!("send".parse::<KeyBindingAction>().is_err());
}

/// The arguments of the line commands: `on`/`off`, a break in 1..=10000 ms or none, a baud
/// rate in 50..=4000000 with the usual ones completed, `none`/`hw`/`sw`
#[test]
fn line_command_arguments() {
    for line in [
        "dtr on",
        "rts off",
        "dtr 1",
        "break",
        "break 100",
        "baud 115200",
        "flow hw",
        "framing 7e1",
    ] {
        assert!(parse(line).is_ok(), "{line}");
    }
    for line in [
        "dtr",
        "rts maybe",
        "break 0",
        "break 10001",
        "baud 10",
        "flow rtscts",
    ] {
        assert!(parse(line).is_err(), "{line}");
    }
    assert_eq!(
        values(&complete("baud 1", &[], &[], &[], &[])),
        ["19200", "115200"]
    );
    assert_eq!(
        values(&complete("flow ", &[], &[], &[], &[])),
        ["none", "hw", "sw"]
    );
    assert_eq!(
        values(&complete("dtr o", &[], &[], &[], &[])),
        ["on", "off"]
    );
}

/// MAX-6: `trigger` takes on, off or toggle, then a trigger's name, completed from the
/// names it is given, escaped like a path.
#[test]
fn trigger_names() {
    let names = [
        "u-boot".to_string(),
        "uart menu".to_string(),
        "login".to_string(),
    ];
    assert_eq!(
        values(&complete("trigger t", &names, &[], &[], &[])),
        ["toggle"]
    );
    assert_eq!(
        values(&complete("trigger on u", &names, &[], &[], &[])),
        ["u-boot", "uart\\ menu"]
    );
    assert_eq!(
        values(&complete("trigger off ", &names, &[], &[], &[])).len(),
        3
    );
    assert!(
        complete("trigger on ", &[], &[], &[], &[])
            .candidates
            .is_empty()
    );
    assert!(parse("trigger toggle u-boot").is_ok());
    assert!(parse("trigger flip").is_err());
}

#[test]
fn registry_is_well_formed() {
    let mut names: Vec<_> = COMMANDS.iter().map(|spec| spec.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), COMMANDS.len(), "duplicate names");
    for spec in COMMANDS {
        assert!(!spec.help.is_empty(), "{}", spec.name);
        let first_optional = spec.args.iter().position(|arg| arg.optional);
        if let Some(first) = first_optional {
            assert!(spec.args[first..].iter().all(|arg| arg.optional));
        }
    }
}

#[test]
fn parse_chained_commands() {
    let invocations = parse("help; toggle-mouse;;").unwrap();
    let names: Vec<_> = invocations.iter().map(|inv| inv.spec.name).collect();
    assert_eq!(names, ["help", "toggle-mouse"]);
    assert!(invocations.iter().all(|inv| inv.args.is_empty()));
    assert!(parse("").unwrap().is_empty());
    assert!(parse("  ; ").unwrap().is_empty());
}

#[test]
fn parse_errors() {
    assert_eq!(
        parse("help; nope"),
        Err(CommandError::UnknownCommand("nope".into()))
    );
    assert_eq!(
        parse("\\xff"),
        Err(CommandError::UnknownCommand("\u{fffd}".into()))
    );
    assert_eq!(
        parse("quit now"),
        Err(CommandError::TooManyArguments {
            command: "quit",
            max: 0
        })
    );
    assert_eq!(parse("help \"x"), Err(CommandError::UnclosedQuote));
    assert_eq!(
        parse("quit now").unwrap_err().to_string(),
        "quit takes no argument"
    );
}

#[test]
fn arguments_of_each_kind() {
    assert_eq!(
        args("test 115200 on hw 'a label' /tmp/f 0d0A").unwrap(),
        [
            ArgValue::Int(115200),
            ArgValue::Bool(true),
            ArgValue::Enum("hw"),
            ArgValue::String("a label".into()),
            ArgValue::Path("/tmp/f".into()),
            ArgValue::Bytes(vec![0x0d, 0x0a]),
        ]
    );
    assert_eq!(args("test 1 off none x y").unwrap().len(), 5);
    for (word, value) in [
        ("on", true),
        ("true", true),
        ("1", true),
        ("off", false),
        ("false", false),
        ("0", false),
    ] {
        let line = format!("test 1 {word} none x y");
        assert_eq!(args(&line).unwrap()[1], ArgValue::Bool(value), "{word}");
    }
}

#[test]
fn argument_errors() {
    assert_eq!(
        args("test 1 on none x"),
        Err(CommandError::MissingArgument {
            command: "test",
            name: "file"
        })
    );
    assert_eq!(
        args("test 1 on none x y 00 z"),
        Err(CommandError::TooManyArguments {
            command: "test",
            max: 6
        })
    );
    assert_eq!(bad_argument("test fast on none x y").0, 1);
    assert_eq!(
        bad_argument("test 0 on none x y"),
        (1, "0 is out of 1..=4000000".into())
    );
    assert_eq!(
        bad_argument("test 1 yes none x y"),
        (2, "'yes' is not on/true/1 or off/false/0".into())
    );
    assert_eq!(
        bad_argument("test 1 on rts x y"),
        (3, "'rts' is not one of none, hw, sw".into())
    );
    assert_eq!(bad_argument("test 1 on none \\xff y").1, "not valid UTF-8");
    assert_eq!(
        bad_argument("test 1 on none x y 0g").1,
        "'g' is not a hex digit"
    );
    assert_eq!(
        bad_argument("test 1 on none x y 0d0").1,
        "odd number of hex digits"
    );
    assert_eq!(bad_argument("test 1 on none x y ''").1, "no hex digits");
    assert_eq!(
        bad_argument("test 1 on none x y '0d 0a'").1,
        "' ' is not a hex digit"
    );
    let err = args("test 1 on none x y 0g").unwrap_err();
    assert_eq!(
        err.to_string(),
        "test: bad argument 6: 'g' is not a hex digit"
    );
}

fn values(completion: &Completion) -> Vec<&str> {
    completion
        .candidates
        .iter()
        .map(|c| c.value.as_str())
        .collect()
}

#[test]
fn complete_command_names() {
    let complete = |line| complete(line, &[], &[], &[], &[]);
    let all = complete("");
    assert_eq!(all.start, 0);
    assert_eq!(all.candidates.len(), COMMANDS.len());
    assert!(values(&all).is_sorted());
    assert_eq!(
        values(&complete("toggle-d")),
        [
            "toggle-display-mode",
            "toggle-display-raw",
            "toggle-display-timestamp"
        ]
    );
    let after_chain = complete("help; toggle-d");
    assert_eq!(after_chain.start, 6);
    assert_eq!(after_chain.candidates[0].value, "toggle-display-mode");
    assert_eq!(after_chain.candidates[0].help, "Cycle the display mode");
    assert!(complete("zz").candidates.is_empty());
    // a whole name, or an argument of a command without any
    assert!(complete("quit").candidates.is_empty());
    assert_eq!(
        complete("help "),
        Completion {
            start: 5,
            candidates: vec![]
        }
    );
}

#[test]
fn complete_argument_values() {
    let complete = |line| complete_in(&TEST_COMMANDS, line, &[], &[], &[], &[]);
    assert_eq!(values(&complete("test 1 ")), ["on", "off"]);
    assert_eq!(values(&complete("test  1   o")), ["on", "off"]);
    let flow = complete("quit; test 1 on h");
    assert_eq!((flow.start, values(&flow)), (16, vec!["hw"]));
    assert_eq!(values(&complete("test 1")), ["115200"]);
    assert_eq!(complete("test 1 on hw").candidates, []);
    // a quote or an escape: no completion
    for line in ["test 1 \"o", "test 1 'o", "test 1 \\o", "test \"a b\" o\\"] {
        assert!(complete(line).candidates.is_empty(), "{line}");
    }
    assert_eq!(values(&complete("test \"a b\" o")), ["on", "off"]);
    assert!(complete("test 1 on hw x y z ").candidates.is_empty());
}

/// A path completes from the folder typed: its entries starting like the last part, a
/// folder with a `/`, hidden ones for a part starting with `.` only, a space escaped so the
/// word parses back to the name.
/// How: a temporary folder with files, a subfolder and a hidden file.
#[test]
fn complete_paths() {
    let dir = std::env::temp_dir().join(format!("maxicom_test_{}_paths", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("logs")).unwrap();
    for file in ["log.txt", "my file.bin", ".hidden", "other"] {
        std::fs::write(dir.join(file), "").unwrap();
    }
    // `/` between the parts on every system: `\` is the tokenizer's escape
    let folder = format!("{}/", dir.display()).replace('\\', "/");
    let complete = |typed: &str| {
        let line = format!("test 1 on none x {folder}{typed}");
        let completion = complete_in(&TEST_COMMANDS, &line, &[], &[], &[], &[]);
        assert_eq!(completion.start, 17);
        completion
            .candidates
            .iter()
            .map(|c| c.value.strip_prefix(&folder).unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(complete("lo"), ["log.txt", "logs/"]);
    assert_eq!(complete(""), ["log.txt", "logs/", "my\\ file.bin", "other"]);
    assert_eq!(complete("."), [".hidden"]);
    assert!(complete("logs/").is_empty());
    assert!(complete("other").is_empty());

    let line = format!("test 1 on none x {folder}my\\ file.bin");
    // the escaped name is whole, and the argument is the name
    assert!(
        complete_in(&TEST_COMMANDS, &line, &[], &[], &[], &[])
            .candidates
            .is_empty()
    );
    let parsed = parse_args(
        &TEST_COMMANDS[0],
        &tokenizer::split(&line, |_| false).unwrap()[0][1..],
    );
    assert_eq!(
        parsed.unwrap()[4],
        ArgValue::Path(PathBuf::from(format!("{folder}my file.bin")))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// MAX-168: `open`'s address is checked when the line is read, and completes with the
/// schemes and the listed ports, then paths after `file://` or a `/`, nothing after
/// `tcp://`.
/// How: good and bad addresses; completions with two fake ports and a temporary folder.
#[test]
fn open_takes_an_address() {
    assert_eq!(
        parse("open tcp://h:1").unwrap()[0].args,
        [ArgValue::Address("tcp://h:1".parse().unwrap())]
    );
    for (line, reason) in [
        ("open tcp://host", "'host' is not host:port"),
        ("open file://", "file:// needs a path"),
    ] {
        let err = parse(line).unwrap_err().to_string();
        assert_eq!(err, format!("open: bad argument 1: {reason}"));
    }

    let ports = ["/dev/ttyUSB0".to_string(), "COM3".to_string()];
    let complete = |line: &str| complete(line, &[], &[], &ports, &[]);
    assert_eq!(
        values(&complete("open ")),
        ["tcp://", "file://", "/dev/ttyUSB0", "COM3"]
    );
    assert_eq!(values(&complete("open t")), ["tcp://"]);
    assert_eq!(values(&complete("open C")), ["COM3"]);
    assert!(complete("open tcp://192.").candidates.is_empty());
    // MAX-185: this machine's saved links first, escaped; `link-delete` takes them only
    let names = ["esp32".to_string(), "router console".to_string()];
    let saved = |line: &str| super::complete(line, &[], &names, &ports, &[]);
    assert_eq!(
        values(&saved("open ")),
        [
            "esp32",
            "router\\ console",
            "tcp://",
            "file://",
            "/dev/ttyUSB0",
            "COM3"
        ]
    );
    assert_eq!(values(&saved("link-delete r")), ["router\\ console"]);
    assert_eq!(values(&saved("link-delete ")).len(), 2);

    let dir = std::env::temp_dir().join(format!("maxicom_test_{}_address", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("capture.raw"), "").unwrap();
    let folder = format!("{}/", dir.display()).replace('\\', "/");
    assert_eq!(
        values(&complete(&format!("open file://{folder}c"))),
        [format!("file://{folder}capture.raw")]
    );
    assert_eq!(
        values(&complete(&format!("open {folder}c"))),
        [format!("{folder}capture.raw")]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn argument_candidates() {
    assert_eq!(TEST_ARGS[0].kind.candidates("1"), ["115200"]);
    assert_eq!(TEST_ARGS[1].kind.candidates(""), ["on", "off"]);
    assert!(TEST_ARGS[1].kind.candidates("t").is_empty());
    assert_eq!(TEST_ARGS[2].kind.candidates("h"), ["hw"]);
    assert!(TEST_ARGS[3].kind.candidates("").is_empty());
}

/// Windows paths: a path or an address argument keeps its `\`, bare or in double quotes; bare,
/// `\ ` still escapes a space. The other words still take escapes, also in the same line
#[test]
fn windows_paths_keep_backslashes() {
    let first_args = |line: &str| -> Vec<ArgValue> {
        parse(line)
            .unwrap()
            .into_iter()
            .map(|invocation| invocation.args[0].clone())
            .collect()
    };
    assert_eq!(
        first_args(r"replay C:\Users\me\boot.raw"),
        [ArgValue::Path(r"C:\Users\me\boot.raw".into())]
    );
    assert_eq!(
        first_args(r#"replay "C:\My Files\new.raw" 10"#),
        [ArgValue::Path(r"C:\My Files\new.raw".into())]
    );
    assert_eq!(
        first_args(r"replay C:\temp\x.raw; send a\x41"),
        [
            ArgValue::Path(r"C:\temp\x.raw".into()),
            ArgValue::Bytes(b"aA".to_vec()),
        ]
    );
    assert_eq!(
        first_args(r"open \\.\COM10"),
        [ArgValue::Address(r"\\.\COM10".parse().unwrap())]
    );
    assert_eq!(
        first_args(r"replay C:\My\ Files\a.raw; replay \\server\share\b.raw"),
        [
            ArgValue::Path(r"C:\My Files\a.raw".into()),
            ArgValue::Path(r"\\server\share\b.raw".into()),
        ]
    );
    assert_eq!(
        parse(r"send C:\Users").unwrap_err(),
        CommandError::BadEscape(r"\U".into())
    );
}
