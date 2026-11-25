//! What a query gives back (MAX-165): a small tree of values, shown as text lines in the TUI
//! and turned into JSON by the API (MAX-284). Its kinds come with their first query: a
//! `Float` waits for one that gives it

/// A query's value
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A text, shown as it is: one on several lines (a screen) on lines of its own
    Text(String),
    /// A whole number: a count, a rate
    Int(i64),
    /// Yes or no, shown as `true` or `false`
    Bool(bool),
    /// Values in order, shown on one line when they are all scalars
    List(Vec<Value>),
    /// Named values in order, one `name: value` line each
    Map(Vec<(String, Value)>),
    /// Bytes as received (MAX-284): base64 in JSON, hex and escaped text in the TUI
    Bytes(Vec<u8>),
}

impl Value {
    /// A text, from anything that gives one
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    /// The lines the TUI shows: `name: value` for a map's scalars, a list of scalars on one
    /// line (`none` when empty, a map too), what is nested below its name, indented; a text
    /// on several lines one line each. Control characters show as their pictures (`␛`): an
    /// escape sequence would move the popup's drawing
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        self.push_lines("", &mut lines);
        lines
    }

    fn push_lines(&self, indent: &str, lines: &mut Vec<String>) {
        if let Some(scalar) = self.scalar() {
            lines.push(format!("{indent}{scalar}"));
            return;
        }
        match self {
            Self::List(items) => {
                for item in items {
                    item.push_lines(indent, lines);
                }
            }
            Self::Map(entries) => {
                for (name, value) in entries {
                    match value.scalar() {
                        Some(scalar) => lines.push(format!("{indent}{name}: {scalar}")),
                        None => {
                            lines.push(format!("{indent}{name}:"));
                            value.push_lines(&format!("{indent}  "), lines);
                        }
                    }
                }
            }
            Self::Text(text) => {
                for line in text.lines() {
                    lines.push(format!("{indent}{}", pictures(line)));
                }
            }
            Self::Bytes(bytes) => {
                for row in bytes.chunks(BYTES_PER_LINE) {
                    lines.push(format!("{indent}{}", bytes_line(row)));
                }
            }
            Self::Int(_) | Self::Bool(_) => {}
        }
    }

    /// The value on one line, when it fits there: a text, a number, a yes or no, a list of
    /// those, an empty map
    fn scalar(&self) -> Option<String> {
        match self {
            Self::Text(text) if text.contains('\n') => None,
            Self::Text(text) => Some(pictures(text)),
            Self::Int(n) => Some(n.to_string()),
            Self::Bool(yes) => Some(yes.to_string()),
            Self::List(items) if items.is_empty() => Some("none".to_string()),
            Self::Map(entries) if entries.is_empty() => Some("none".to_string()),
            Self::Bytes(bytes) if bytes.is_empty() => Some("none".to_string()),
            Self::List(items) => {
                let scalars: Option<Vec<String>> = items.iter().map(Self::scalar).collect();
                scalars.map(|scalars| scalars.join(", "))
            }
            Self::Map(_) | Self::Bytes(_) => None,
        }
    }
}

/// The bytes on one line of the TUI
const BYTES_PER_LINE: usize = 16;

/// A line of bytes: their hex, padded to a full line's, then their escaped text
fn bytes_line(row: &[u8]) -> String {
    let hex: Vec<String> = row.iter().map(|byte| format!("{byte:02x}")).collect();
    let width = BYTES_PER_LINE * 3 - 1;
    format!("{:width$}  {}", hex.join(" "), escaped(row))
}

/// Bytes as text the way `send` reads its escapes: printable ASCII as it is, `\\` for a
/// backslash, `\r` `\n` `\t`, `\xHH` for any other byte
fn escaped(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len());
    for &byte in bytes {
        match byte {
            b'\\' => text.push_str("\\\\"),
            b'\r' => text.push_str("\\r"),
            b'\n' => text.push_str("\\n"),
            b'\t' => text.push_str("\\t"),
            b' '..=b'~' => text.push(char::from(byte)),
            _ => text.push_str(&format!("\\x{byte:02x}")),
        }
    }
    text
}

/// A control character as its picture, `␛` for ESC, for the TUI's lines
fn pictures(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\0'..='\x1f' => char::from_u32(0x2400 + c as u32).unwrap_or(c),
            '\x7f' => '\u{2421}',
            c => c,
        })
        .collect()
}

#[cfg(test)]
#[path = "../tests/commands/value.rs"]
mod tests;
