//! `maxicom cli`'s arguments.

use super::*;

fn parsed(args: &[&str]) -> Result<CliArgs, String> {
    parse(args.iter().map(|arg| arg.to_string())).map_err(|err| err.to_string())
}

fn action(args: &[&str]) -> CliAction {
    parsed(args).unwrap().action
}

fn line_of(args: &[&str]) -> String {
    match action(args) {
        CliAction::Line(line) => line,
        other => panic!("not a line: {other:?}"),
    }
}

/// MAX-285: the options come before the action, `--instance` takes its value glued or next,
/// and `--help` stops there.
#[test]
fn options_before_the_action() {
    let args = parsed(&["--instance", "board", "--json", "status"]).unwrap();
    assert_eq!(args.instance.as_deref(), Some("board"));
    assert!(args.json);
    assert_eq!(args.action, CliAction::Line("status".to_string()));
    let args = parsed(&["--instance=1234", "status"]).unwrap();
    assert_eq!(args.instance.as_deref(), Some("1234"));
    assert!(!args.json);
    assert_eq!(action(&["--help", "whatever"]), CliAction::Help);
    assert_eq!(action(&["-h"]), CliAction::Help);
}

/// MAX-285: a missing action, a missing value and an unknown option are refused, an option
/// after the action is the line's.
#[test]
fn bad_calls() {
    assert_eq!(
        parsed(&[]).unwrap_err(),
        "No command given: maxicom cli <command line>."
    );
    assert_eq!(
        parsed(&["--instance"]).unwrap_err(),
        "Missing value for option '--instance'."
    );
    assert_eq!(
        parsed(&["--nope", "status"]).unwrap_err(),
        "Unknown argument '--nope'."
    );
    assert_eq!(
        parsed(&["--json=yes", "status"]).unwrap_err(),
        "Unknown argument '--json=yes'."
    );
    assert_eq!(line_of(&["notify", "--json"]), "notify --json");
}

/// MAX-285: one argument is the line as typed; several are re-quoted so each stays one word,
/// a lone `;` still ending a command.
#[test]
fn line_from_the_words() {
    assert_eq!(line_of(&["status; get baud"]), "status; get baud");
    assert_eq!(line_of(&["get", "baud"]), "get baud");
    assert_eq!(
        line_of(&["send", "hello world", ";", "status"]),
        "send \"hello world\" ; status"
    );
    assert_eq!(line_of(&["send", "a;b"]), "send \"a;b\"");
    assert_eq!(line_of(&["send", "say \"hi\""]), "send \"say \\\"hi\\\"\"");
    assert_eq!(line_of(&["send", "it's"]), "send \"it's\"");
    assert_eq!(line_of(&["send", ""]), "send \"\"");
    // escapes are the command line's, kept for it
    assert_eq!(line_of(&["send", "reboot\\r"]), "send reboot\\r");
}

/// MAX-285: the lines given that way read back as the words given.
#[test]
fn quoted_words_split_back() {
    let words = ["send", "hello world", "a;b", "say \"hi\"", "it's", ""];
    let line = line_of(&words);
    let split = crate::commands::tokenizer::split(&line, |_| false).unwrap();
    let split: Vec<Vec<String>> = (split.into_iter())
        .map(|words| {
            words
                .into_iter()
                .map(|w| String::from_utf8(w).unwrap())
                .collect()
        })
        .collect();
    assert_eq!(split, [words.map(str::to_string).to_vec()]);
}

/// MAX-285: the client's own verbs and their options.
#[test]
fn verbs() {
    assert_eq!(action(&["list"]), CliAction::List);
    assert_eq!(parsed(&["list", "x"]).unwrap_err(), "Unknown argument 'x'.");
    assert_eq!(action(&["events"]), CliAction::Events(Vec::new()));
    assert_eq!(
        action(&["events", "link-opened", "link-lost"]),
        CliAction::Events(vec!["link-opened".to_string(), "link-lost".to_string()])
    );
    assert_eq!(
        parsed(&["events", "--all"]).unwrap_err(),
        "Unknown argument '--all'."
    );
    let rx = |since, follow, hex| CliAction::Rx { since, follow, hex };
    assert_eq!(action(&["rx"]), rx(None, false, false));
    assert_eq!(
        action(&["rx", "--since", "42", "--follow", "--hex"]),
        rx(Some(42), true, true)
    );
    assert_eq!(action(&["rx", "--since=7"]), rx(Some(7), false, false));
    assert_eq!(
        parsed(&["rx", "--since", "-1"]).unwrap_err(),
        "Invalid offset '-1'."
    );
    assert_eq!(parsed(&["rx", "x"]).unwrap_err(), "Unknown argument 'x'.");
}

/// MAX-285: no command of the registry takes the name of one of the client's own verbs, so
/// every command line reaches maxicom.
#[test]
fn verbs_are_not_commands() {
    for verb in ["list", "events", "rx"] {
        assert!(crate::commands::find_by_name(verb).is_none(), "{verb}");
    }
}

fn found(pid: u32, name: Option<&str>, address: &str) -> Found<()> {
    let link = match name {
        Some(name) => format!("{name} ({address})"),
        None => address.to_string(),
    };
    let hello = json!({ "pid": pid, "link": link, "name": name, "address": address });
    Found {
        pid,
        hello,
        connection: (),
    }
}

fn picked(id: Option<&str>, found: Vec<Found<()>>) -> Result<u32, String> {
    pick(id, found).map(|found| found.pid)
}

/// MAX-285: `--instance` names a maxicom by its pid, its saved link's name or its address;
/// without it the only one running is picked. None running, none matching or several give
/// why, with the ones to pick from.
#[test]
fn instance_picked() {
    let two = || {
        vec![
            found(101, Some("board"), "/dev/ttyUSB0"),
            found(202, None, "tcp://127.0.0.1:4000"),
        ]
    };
    assert_eq!(picked(Some("101"), two()), Ok(101));
    assert_eq!(picked(Some("board"), two()), Ok(101));
    assert_eq!(picked(Some("/dev/ttyUSB0"), two()), Ok(101));
    assert_eq!(picked(Some("tcp://127.0.0.1:4000"), two()), Ok(202));
    assert_eq!(picked(None, vec![found(303, None, "COM3")]), Ok(303));

    assert_eq!(
        picked(None, Vec::new()),
        Err("no maxicom is running, or its [api] isn't enabled".to_string())
    );
    assert_eq!(
        picked(Some("board"), Vec::new()),
        Err("no maxicom is running, so none is 'board', or its [api] isn't enabled".to_string())
    );
    assert_eq!(
        picked(Some("other"), two()),
        Err(
            "no maxicom running is 'other': pick one by its pid, its link's name or its \
             address:\n      101  board (/dev/ttyUSB0)\n      202  tcp://127.0.0.1:4000"
                .to_string()
        )
    );
    assert_eq!(
        picked(None, two()),
        Err(
            "several maxicoms match: pick one with --instance, by its pid, its link's name or \
             its address:\n      101  board (/dev/ttyUSB0)\n      202  tcp://127.0.0.1:4000"
                .to_string()
        )
    );
    // two maxicoms on the same saved link can't be, but the same address twice can (a replay)
    let same = vec![found(1, None, "file://a"), found(2, None, "file://a")];
    assert!(
        picked(Some("file://a"), same)
            .unwrap_err()
            .starts_with("several")
    );
}

/// MAX-285: a `run`'s result as text: its notes, then one value bare or several under their
/// queries, as the TUI shows them; nothing for a run with neither.
#[test]
fn result_as_text() {
    let result = json!({
        "job": 3,
        "notes": ["Local echo on"],
        "values": [{ "query": "get baud", "value": 115200 }],
    });
    assert_eq!(result_lines(&result), ["Local echo on", "115200"]);
    let result = json!({
        "job": 4,
        "notes": [],
        "values": [
            { "query": "get baud", "value": 115200 },
            { "query": "status", "value": { "state": "open", "jobs": [], "speed": 1.5 } },
        ],
    });
    assert_eq!(
        result_lines(&result),
        [
            "get baud: 115200",
            "status:",
            "  state: open",
            "  jobs: none",
            "  speed: 1.5"
        ]
    );
    let result = json!({ "job": null, "notes": [], "values": [] });
    assert!(result_lines(&result).is_empty());
}

/// MAX-285: refusals by the levels or the user exit 4, a cancelled job 5, every other error
/// 1 (a failed command, a line that doesn't parse).
#[test]
fn exit_codes_of_errors() {
    let error = |code| RpcError {
        code,
        message: String::new(),
    };
    assert_eq!(exit_of(&error(rpc::REFUSED)), Exit::Refused);
    assert_eq!(exit_of(&error(rpc::REFUSED_BY_USER)), Exit::Refused);
    assert_eq!(exit_of(&error(rpc::CANCELLED)), Exit::Cancelled);
    assert_eq!(exit_of(&error(rpc::COMMAND_FAILED)), Exit::Failed);
    assert_eq!(exit_of(&error(rpc::UNKNOWN_COMMAND)), Exit::Failed);
}

/// MAX-285: `list`'s table: a header, the pids right-aligned, the links in a column as wide
/// as the widest, no space left at the ends of the lines.
#[test]
fn list_table() {
    let row = |pid: &str, link: &str, since: &str| [pid, link, since].map(str::to_string);
    let rows = [
        row("812", "board (/dev/ttyUSB0)", "2026-11-27 17:25:32"),
        row("10450", "tcp://127.0.0.1:4000", "not answering"),
        row("7", "COM3", ""),
    ];
    assert_eq!(
        table(&rows),
        [
            "  PID  LINK                  SINCE",
            "  812  board (/dev/ttyUSB0)  2026-11-27 17:25:32",
            "10450  tcp://127.0.0.1:4000  not answering",
            "    7  COM3",
        ]
    );
}

/// MAX-285: `rx --hex`: 16 bytes a line, each line its first byte's offset (as `--since` takes
/// it), the hex padded to a full line's, then the printable ASCII, `.` for the others.
#[test]
fn hex_dump() {
    let bytes = b"Hello, world!\r\nboot: ok\x1b[0m";
    assert_eq!(
        hex_lines(100, bytes),
        [
            "       100  48 65 6c 6c 6f 2c 20 77 6f 72 6c 64 21 0d 0a 62  Hello, world!..b",
            "       116  6f 6f 74 3a 20 6f 6b 1b 5b 30 6d                 oot: ok.[0m",
        ]
    );
    assert!(hex_lines(0, b"").is_empty());
}
