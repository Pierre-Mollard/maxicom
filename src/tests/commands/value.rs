use super::*;

/// MAX-165 (A1 step 5): a value shows as lines: a map's scalars as `name: value`, a list of
/// scalars on one line (`none` when empty), what is nested below its name, indented; a bare
/// scalar on a line of its own.
/// How: a map holding each kind once.
#[test]
fn value_lines() {
    let name = |name: &str| name.to_string();
    let value = Value::Map(vec![
        (name("link"), Value::text("/dev/ttyUSB0")),
        (name("rx"), Value::Int(42)),
        (
            name("profiles"),
            Value::List(vec![Value::text("esp32"), Value::text("log")]),
        ),
        (name("none"), Value::List(Vec::new())),
        (
            name("line"),
            Value::Map(vec![(name("baud"), Value::Int(115_200))]),
        ),
        (
            name("jobs"),
            Value::List(vec![Value::Map(vec![(name("origin"), Value::text("you"))])]),
        ),
    ]);
    assert_eq!(
        value.lines(),
        [
            "link: /dev/ttyUSB0",
            "rx: 42",
            "profiles: esp32, log",
            "none: none",
            "line:",
            "  baud: 115200",
            "jobs:",
            "  origin: you",
        ]
    );
    assert_eq!(Value::Int(3).lines(), ["3"]);
}
