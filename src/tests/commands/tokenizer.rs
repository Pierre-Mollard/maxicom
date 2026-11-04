use super::*;

fn no_paths(_: &[Word]) -> bool {
    false
}

fn words(line: &str) -> Vec<Vec<String>> {
    split(line, no_paths)
        .unwrap()
        .into_iter()
        .map(|command| {
            command
                .into_iter()
                .map(|word| String::from_utf8(word).unwrap())
                .collect()
        })
        .collect()
}

#[test]
fn words_and_commands() {
    assert_eq!(words("  dtr   off "), [["dtr", "off"]]);
    assert_eq!(
        words("dtr off; rts on;break 100;"),
        [vec!["dtr", "off"], vec!["rts", "on"], vec!["break", "100"]]
    );
    assert!(words("").is_empty());
    assert!(words(" ;; ; ").is_empty());
}

#[test]
fn quotes() {
    assert_eq!(words("send \"a b; c\""), [["send", "a b; c"]]);
    assert_eq!(words("send 'a \"b\" \\n'"), [["send", "a \"b\" \\n"]]);
    assert_eq!(words("send a\"b c\"d"), [["send", "ab cd"]]);
    assert_eq!(words("mark \"\" ''"), [["mark", "", ""]]);
    assert_eq!(words("mark é"), [["mark", "é"]]);
    assert_eq!(
        split("send \"abc", no_paths),
        Err(CommandError::UnclosedQuote)
    );
    assert_eq!(
        split("send 'abc", no_paths),
        Err(CommandError::UnclosedQuote)
    );
}

/// MAX-6: only the escapes are read, the rest is kept as is.
#[test]
fn unescaped() {
    assert_eq!(unescape("=> ").unwrap(), b"=> ");
    assert_eq!(unescape("'a;b' \\x1b[0m\\r").unwrap(), b"'a;b' \x1b[0m\r");
    assert_eq!(unescape("\\q"), Err(CommandError::BadEscape("\\q".into())));
}

#[test]
fn escapes() {
    assert_eq!(
        split("send AT\\r\\n \"\\t\\x1B\\xff\"", no_paths).unwrap(),
        [vec![
            b"send".to_vec(),
            b"AT\r\n".to_vec(),
            vec![b'\t', 0x1b, 0xff]
        ]]
    );
    assert_eq!(words("send a\\;b\\ c"), [["send", "a;b c"]]);
    assert_eq!(words("send \\\\ \\\" \\'"), [["send", "\\", "\"", "'"]]);
    for (line, escape) in [
        ("send \\q", "\\q"),
        ("send \\x4", "\\x4"),
        ("send \\xg0", "\\xg0"),
        ("send \\xé0", "\\xé0"),
        ("send \\", "\\"),
    ] {
        assert_eq!(
            split(line, no_paths),
            Err(CommandError::BadEscape(escape.into())),
            "{line}"
        );
    }
}
