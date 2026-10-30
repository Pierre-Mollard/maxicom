/// Splits a command line into commands and their words (MAX-12)
use std::str::Chars;

use crate::commands::CommandError;

/// A word is bytes, not text: `\xNN` escapes can make any byte
pub type Word = Vec<u8>;

/// Splits on `;` between commands and on spaces between words. Bare words and double quotes take
/// escapes (`\r \n \t \xNN`, and `\\ \" \' \; \ ` for the character itself), single quotes are
/// literal. Empty commands are dropped
pub fn split(line: &str) -> Result<Vec<Vec<Word>>, CommandError> {
    let mut commands = Vec::new();
    let mut words = Vec::new();
    // `Some` once a word started, even empty: `""` is an empty word
    let mut word: Option<Word> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            ';' => {
                words.extend(word.take());
                if !words.is_empty() {
                    commands.push(std::mem::take(&mut words));
                }
            }
            c if c.is_whitespace() => words.extend(word.take()),
            '\'' => {
                let word = word.get_or_insert_default();
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => push_char(word, c),
                        None => return Err(CommandError::UnclosedQuote),
                    }
                }
            }
            '"' => {
                let word = word.get_or_insert_default();
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => escape(&mut chars, word)?,
                        Some(c) => push_char(word, c),
                        None => return Err(CommandError::UnclosedQuote),
                    }
                }
            }
            '\\' => escape(&mut chars, word.get_or_insert_default())?,
            c => push_char(word.get_or_insert_default(), c),
        }
    }
    words.extend(word);
    if !words.is_empty() {
        commands.push(words);
    }
    Ok(commands)
}

fn push_char(word: &mut Word, c: char) {
    word.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
}

/// The escape after a `\`
fn escape(chars: &mut Chars, word: &mut Word) -> Result<(), CommandError> {
    let byte = match chars.next() {
        Some('r') => b'\r',
        Some('n') => b'\n',
        Some('t') => b'\t',
        Some('x') => {
            let hex: String = chars.by_ref().take(2).collect();
            if hex.len() != 2 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(CommandError::BadEscape(format!("\\x{hex}")));
            }
            u8::from_str_radix(&hex, 16).expect("two hex digits")
        }
        Some(c @ ('\\' | '"' | '\'' | ';' | ' ')) => {
            push_char(word, c);
            return Ok(());
        }
        Some(c) => return Err(CommandError::BadEscape(format!("\\{c}"))),
        None => return Err(CommandError::BadEscape("\\".to_string())),
    };
    word.push(byte);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<Vec<String>> {
        split(line)
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
        assert_eq!(split("send \"abc"), Err(CommandError::UnclosedQuote));
        assert_eq!(split("send 'abc"), Err(CommandError::UnclosedQuote));
    }

    #[test]
    fn escapes() {
        assert_eq!(
            split("send AT\\r\\n \"\\t\\x1B\\xff\"").unwrap(),
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
                split(line),
                Err(CommandError::BadEscape(escape.into())),
                "{line}"
            );
        }
    }
}
