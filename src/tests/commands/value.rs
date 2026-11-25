use super::*;

/// MAX-165 (A1 step 5): a value shows as lines: a map's scalars as `name: value`, a list of
/// scalars on one line (`none` when empty, an empty map too), a yes or no as `true` or `false`, what is nested below its name, indented; a bare
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
        (name("optional"), Value::Bool(true)),
        (name("args"), Value::Map(Vec::new())),
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
            "optional: true",
            "args: none",
            "line:",
            "  baud: 115200",
            "jobs:",
            "  origin: you",
        ]
    );
    assert_eq!(Value::Int(3).lines(), ["3"]);
    // MAX-283: a text on several lines one line each, below its name; a control character
    // as its picture
    let screen = Value::Map(vec![(name("screen"), Value::text("a\x1b[1mb\nc"))]);
    assert_eq!(screen.lines(), ["screen:", "  a␛[1mb", "  c"]);
    assert_eq!(Value::text("x\ty").lines(), ["x␉y"]);
}

/// MAX-284 (step 2a): bytes show 16 a line, their hex padded to a full line's, then their
/// text escaped as `send` reads it (printable ASCII as is, `\\`, `\r` `\n` `\t`, `\xHH`);
/// none when empty.
/// How: 18 bytes below a name, an empty one.
#[test]
fn bytes_lines() {
    let bytes = Value::Map(vec![(
        "bytes".to_string(),
        Value::Bytes(b"ab\\\r\n\t\x1b\xff 0123456789".to_vec()),
    )]);
    assert_eq!(
        bytes.lines(),
        [
            "bytes:",
            r"  61 62 5c 0d 0a 09 1b ff 20 30 31 32 33 34 35 36  ab\\\r\n\t\x1b\xff 0123456",
            r"  37 38 39                                         789",
        ]
    );
    assert_eq!(Value::Bytes(Vec::new()).lines(), ["none"]);
}
