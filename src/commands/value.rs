//! What a query gives back (MAX-165): a small tree of values, shown as text lines in the TUI
//! and turned into JSON by the API (MAX-284). Its kinds come with their first query: a
//! `Bool` or a `Float` waits for one that gives it

/// A query's value
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A text, shown as it is
    Text(String),
    /// A whole number: a count, a rate
    Int(i64),
    /// Values in order, shown on one line when they are all scalars
    List(Vec<Value>),
    /// Named values in order, one `name: value` line each
    Map(Vec<(String, Value)>),
}

impl Value {
    /// A text, from anything that gives one
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    /// The lines the TUI shows: `name: value` for a map's scalars, a list of scalars on one
    /// line (`none` when empty), what is nested below its name, indented
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
            Self::Text(_) | Self::Int(_) => {}
        }
    }

    /// The value on one line, when it fits there: a text, a number, a list of those
    fn scalar(&self) -> Option<String> {
        match self {
            Self::Text(text) => Some(text.clone()),
            Self::Int(n) => Some(n.to_string()),
            Self::List(items) if items.is_empty() => Some("none".to_string()),
            Self::List(items) => {
                let scalars: Option<Vec<String>> = items.iter().map(Self::scalar).collect();
                scalars.map(|scalars| scalars.join(", "))
            }
            Self::Map(_) => None,
        }
    }
}

#[cfg(test)]
#[path = "../tests/commands/value.rs"]
mod tests;
