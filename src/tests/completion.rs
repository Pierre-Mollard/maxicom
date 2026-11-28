//! The shell completion's table of options.

use super::*;
use crate::commands::NamedCommand;

/// MAX-294: every option has a long name, unique, a short one of one letter when it has one,
/// unique too, and a help line of its own: one short line, no period, as the shells show it
/// next to the option.
#[test]
fn option_names_and_help() {
    let mut longs: Vec<&str> = OPTIONS.iter().map(|option| option.long).collect();
    let mut shorts: Vec<&str> = (OPTIONS.iter())
        .map(|option| option.short)
        .filter(|short| !short.is_empty())
        .collect();
    for option in OPTIONS {
        let OptionSpec {
            short, long, help, ..
        } = option;
        assert!(long.starts_with("--") && long.len() > 3, "{long}");
        assert!(
            short.is_empty() || short.len() == 2 && short.starts_with('-'),
            "{short}"
        );
        assert!(!help.is_empty() && !help.ends_with('.'), "{long}: {help}");
        assert!(help.len() <= 60, "{long}: help too long");
    }
    let count = (longs.len(), shorts.len());
    longs.sort_unstable();
    longs.dedup();
    shorts.sort_unstable();
    shorts.dedup();
    assert_eq!((longs.len(), shorts.len()), count, "a name given twice");
}

fn owned(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

/// The values `complete` offers after `before`, `current` being typed, over a setup with two
/// saved links, two ports, a plot and two profiles; with their help lines in `helped`
fn offered(before: &[&str], current: &str) -> Vec<String> {
    helped(before, current)
        .into_iter()
        .map(|(value, _)| value)
        .collect()
}

/// Two saved links, two ports, a plot, two profiles, a trigger, a named command, and two
/// maxicoms running
fn setup() -> Setup {
    Setup {
        links: owned(&["board", "router"]),
        ports: owned(&["/dev/ttyACM0", "/dev/ttyUSB0"]),
        plots: owned(&["pid"]),
        profiles: owned(&["debug", "project"]),
        triggers: owned(&["boot"]),
        named: vec![NamedCommand {
            name: "reflash".to_string(),
            run: "send flash".to_string(),
            help: "Flash the board".to_string(),
            invocations: Vec::new(),
        }],
        instances: vec![
            candidate("101", "/dev/ttyUSB0"),
            candidate("202", "tcp://127.0.0.1:4000"),
        ],
    }
}

fn helped(before: &[&str], current: &str) -> Vec<(String, String)> {
    match complete(&owned(before), current, &setup()) {
        Reply::Values(found) => (found.into_iter())
            .map(|found| (found.value, found.help))
            .collect(),
        files => panic!("not values: {files:?}"),
    }
}

/// MAX-294: a word starting with `-` completes as the options spelled so, short and long,
/// in the table's order with their help; `-b` is whole already, and offered as is.
#[test]
fn options_by_their_names() {
    // 25 long names, 8 short ones
    assert_eq!(offered(&[], "-").len(), 33);
    assert_eq!(offered(&[], "--b"), ["--baudrate", "--bits"]);
    assert_eq!(offered(&[], "--no"), ["--no-restore"]);
    assert_eq!(offered(&["board"], "-b"), ["-b"]);
    assert_eq!(
        helped(&[], "--dt"),
        [("--dtr".to_string(), "DTR set on open".to_string())]
    );
    assert!(offered(&[], "--port").is_empty());
}

/// MAX-294: after an option taking a value, its values: the fixed ones, the suggested baud
/// rates, the plots and profiles of the setup, the ports for `--device` (never a saved link,
/// as the parser takes it), none for `--tcp` and `--delay`. The value of an option is never
/// read as one: `-C -b` names a file `-b`.
#[test]
fn values_after_their_option() {
    assert_eq!(offered(&["--parity"], ""), ["none", "even", "odd"]);
    assert_eq!(offered(&["-b"], "11"), ["115200"]);
    assert_eq!(offered(&["board", "--flow"], "h"), ["hw"]);
    assert_eq!(offered(&["--dtr"], "o"), ["on", "off"]);
    assert_eq!(offered(&["-R"], ""), ["utf8", "ascii", "cp437", "latin1"]);
    assert_eq!(offered(&["--profile"], ""), ["debug", "project"]);
    assert_eq!(offered(&["--plot"], "p"), ["pid"]);
    assert_eq!(offered(&["-D"], ""), ["/dev/ttyACM0", "/dev/ttyUSB0"]);
    assert!(offered(&["--tcp"], "").is_empty());
    assert!(offered(&["--delay"], "").is_empty());
    assert_eq!(offered(&["-C", "-b"], "b"), ["board"]);
    assert_eq!(offered(&["-b", "9600"], "r"), ["router"]);
}

/// MAX-294: a value glued to its long option completes glued, whole; a flag or an unknown
/// option takes none.
#[test]
fn glued_values() {
    assert_eq!(offered(&[], "--parity=e"), ["--parity=even"]);
    assert_eq!(
        offered(&[], "--baudrate=9"),
        ["--baudrate=9600", "--baudrate=921600"]
    );
    assert_eq!(
        offered(&[], "--profile="),
        ["--profile=debug", "--profile=project"]
    );
    assert!(offered(&[], "--restore=").is_empty());
    assert!(offered(&[], "--port=x").is_empty());
}

/// MAX-294: a path is the shell's to list, after the option glued to it when it is.
#[test]
fn paths_left_to_the_shell() {
    let reply = |before: &[&str], current| complete(&owned(before), current, &Setup::default());
    let files = |prefix: &str| Reply::Files {
        prefix: prefix.to_string(),
    };
    assert_eq!(reply(&["-C"], ""), files(""));
    assert_eq!(reply(&["--replay"], "/tmp/b"), files(""));
    assert_eq!(reply(&["board", "--config"], "es"), files(""));
    assert_eq!(reply(&[], "--config=es"), files("--config="));
    assert_eq!(reply(&[], "--capturefile="), files("--capturefile="));
}

/// MAX-294: any other word is the link to open: the saved links first, then the ports, each
/// one said so.
#[test]
fn links_then_ports() {
    assert_eq!(
        offered(&[], ""),
        ["board", "router", "/dev/ttyACM0", "/dev/ttyUSB0"]
    );
    assert_eq!(
        helped(&["-b", "9600"], "/dev/ttyU"),
        [("/dev/ttyUSB0".to_string(), "serial port".to_string())]
    );
    assert_eq!(helped(&[], "b")[0].1, "saved link");
    assert!(offered(&["mcp"], "").is_empty());
}

/// MAX-294: the reply as the wrappers read it: a `values` line then one line per candidate,
/// its help after a tab when it has one; or the one `files` line with the glued prefix.
#[test]
fn printed_reply() {
    let values = Reply::Values(vec![
        candidate("--dtr", "DTR set on open"),
        candidate("9600", ""),
    ]);
    assert_eq!(printed(&values), "values\n--dtr\tDTR set on open\n9600\n");
    assert_eq!(printed(&Reply::Values(Vec::new())), "values\n");
    let files = Reply::Files {
        prefix: "--config=".to_string(),
    };
    assert_eq!(printed(&files), "files\t--config=\n");
}

/// MAX-294: the setup of a settings file: its saved links (for this machine), its plots,
/// the profiles that are on next to it, its triggers and named commands; with no settings
/// file there, nothing is written.
/// How: a config folder with each file, then a settings file that isn't there.
#[test]
fn setup_read_never_written() {
    let dir = crate::capture::tests::temp_dir("completion_setup");
    fs::create_dir_all(dir.join("profiles")).unwrap();
    let config = dir.join("config.toml");
    let links = dir.join("links.toml");
    let plots = dir.join("plots.toml");
    let triggers = dir.join("triggers.toml");
    let keymap = dir.join("keymap.toml");
    let settings = format!(
        "links_path = {links:?}\nplots_path = {plots:?}\n\
         triggers_path = {triggers:?}\nkeybindings_path = {keymap:?}\n"
    );
    fs::write(&config, settings).unwrap();
    fs::write(
        &links,
        "[[link]]\nname = \"board\"\naddress = \"/dev/ttyUSB0\"\n\n\
         [[link]]\nname = \"elsewhere\"\nhosts = [\"no-such-host\"]\naddress = \"COM3\"\n",
    )
    .unwrap();
    fs::write(&plots, "[pid]\nseparator = \"=\"\n").unwrap();
    fs::write(
        &triggers,
        "[login]\nmatch = \"login: \"\nrun = \"send root\"\n",
    )
    .unwrap();
    fs::write(
        &keymap,
        "[commands]\nreset = { run = \"dtr off; dtr on\", help = \"Pulse DTR\" }\n",
    )
    .unwrap();
    fs::write(dir.join("profiles/debug.toml"), "enabled = true\n").unwrap();
    fs::write(dir.join("profiles/off.toml"), "enabled = false\n").unwrap();

    let setup = Setup::read(&config, vec!["COM1".to_string()]);
    assert_eq!(
        setup,
        Setup {
            links: vec!["board".to_string()],
            ports: vec!["COM1".to_string()],
            plots: vec!["pid".to_string()],
            profiles: vec!["debug".to_string()],
            triggers: vec!["login".to_string()],
            named: vec![NamedCommand {
                name: "reset".to_string(),
                run: "dtr off; dtr on".to_string(),
                help: "Pulse DTR".to_string(),
                invocations: Vec::new(),
            }],
            instances: Vec::new(),
        }
    );

    let missing = dir.join("none");
    let setup = Setup::read(&missing.join("config.toml"), Vec::new());
    assert!(setup.profiles.is_empty());
    assert!(!missing.exists(), "a file was written");
    let _ = fs::remove_dir_all(&dir);
}

/// MAX-294: `maxicom cli` completes its options, their values (the maxicoms running by pid,
/// with their link, glued too; the socket a file), then its actions with the command names,
/// the named ones of the keymap too.
#[test]
fn cli_options_and_actions() {
    let first = offered(&["cli"], "");
    assert_eq!(first[..3], ["list", "events", "rx"]);
    assert!(first.contains(&"status".to_string()));
    assert_eq!(
        helped(&["cli"], "refl"),
        [("reflash".to_string(), "Flash the board".to_string())]
    );
    assert_eq!(
        offered(&["cli"], "-"),
        ["--instance", "--socket", "--json", "-h", "--help"]
    );
    assert_eq!(
        helped(&["cli", "--instance"], ""),
        [
            ("101".to_string(), "/dev/ttyUSB0".to_string()),
            ("202".to_string(), "tcp://127.0.0.1:4000".to_string())
        ]
    );
    assert_eq!(offered(&["cli"], "--instance=2"), ["--instance=202"]);
    let socket = complete(&owned(&["cli", "--socket"]), "/tmp/", &setup());
    assert_eq!(
        socket,
        Reply::Files {
            prefix: String::new()
        }
    );
    assert_eq!(
        offered(&["cli", "--json", "--instance", "101"], "li"),
        ["list", "link-delete", "link-save", "link-settings"]
    );
}

/// MAX-294: after its action, `events` completes the kinds not given yet, `rx` its options,
/// `list` nothing.
#[test]
fn cli_verbs() {
    assert_eq!(offered(&["cli", "events"], "link-l"), ["link-lost"]);
    assert!(offered(&["cli", "events", "link-lost"], "link-l").is_empty());
    assert_eq!(
        offered(&["cli", "rx"], "--"),
        ["--since", "--follow", "--hex"]
    );
    assert!(offered(&["cli", "rx", "--since"], "").is_empty());
    assert_eq!(offered(&["cli", "rx", "--since", "4"], "--h"), ["--hex"]);
    assert!(offered(&["cli", "list"], "").is_empty());
}

/// MAX-294: any other action is a command line, completed as the command line does, each
/// candidate whole for the shell's word: one word holding the line (`'status; ba`) keeps
/// what comes before the word completed.
#[test]
fn cli_command_lines() {
    assert_eq!(offered(&["cli", "baud"], "11"), ["115200"]);
    assert_eq!(offered(&["cli", "--instance", "101", "flow"], "h"), ["hw"]);
    assert_eq!(offered(&["cli", "plot"], "p"), ["pid"]);
    assert_eq!(offered(&["cli"], "status; bau"), ["status; baud"]);
}

/// MAX-294: `maxicom api` completes its options and the maxicoms running, nothing else.
#[test]
fn api_options() {
    assert_eq!(
        offered(&["api"], "-"),
        ["--instance", "--socket", "-h", "--help"]
    );
    assert_eq!(offered(&["api", "--instance"], "2"), ["202"]);
    assert!(offered(&["api"], "").is_empty());
}

/// MAX-294: each shell of `--completions` has its script, which calls `--complete --` back
/// and reads both kinds of reply; another shell has none.
#[test]
fn scripts_of_the_shells() {
    for shell in SHELLS {
        let script = script(shell).unwrap();
        assert!(script.contains("maxicom --complete -- "), "{shell}");
        assert!(script.contains("values"), "{shell}");
    }
    assert!(script("tcsh").is_none());
    assert_eq!(offered(&["--completions"], ""), SHELLS);
}
