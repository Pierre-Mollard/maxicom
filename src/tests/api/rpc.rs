//! JSON-RPC lines read and written (MAX-74)
use super::*;

fn refusal(line: &str) -> Refusal {
    parse(line.as_bytes()).unwrap_err()
}

/// MAX-74 (step 1): a request reads its id, method and params; without an id it is a
/// notification; params are optional.
/// How: a request, a notification, one with array params.
#[test]
fn requests_read() {
    assert_eq!(
        parse(br#"{"jsonrpc":"2.0","id":1,"method":"hello","params":{"client":"x"}}"#),
        Ok(Request {
            id: Some(json!(1)),
            method: "hello".to_string(),
            params: Some(json!({"client": "x"})),
        })
    );
    let notification = parse(br#"{"jsonrpc":"2.0","method":"hello"}"#).unwrap();
    assert_eq!((notification.id, notification.params), (None, None));
    let request = parse(br#"{"jsonrpc":"2.0","id":"a","method":"m","params":[1]}"#).unwrap();
    assert_eq!(request.params, Some(json!([1])));
}

/// MAX-74 (step 1): what isn't JSON is a parse error that closes the connection; JSON that
/// isn't a request is an invalid request that keeps it, replied to its id when it has one; a
/// batch is refused.
/// How: one line of each kind.
#[test]
fn bad_lines_refused() {
    let not_json = refusal("hello");
    assert_eq!((not_json.error.code, not_json.close), (-32700, true));
    assert_eq!(not_json.id, Json::Null);
    let not_utf8 = parse(b"\xff\xfe").unwrap_err();
    assert_eq!((not_utf8.error.code, not_utf8.close), (-32700, true));

    for (line, id) in [
        (r#"[{"jsonrpc":"2.0","id":1,"method":"hello"}]"#, Json::Null),
        (r#""hello""#, Json::Null),
        (r#"{"id":1,"method":"hello"}"#, json!(1)),
        (r#"{"jsonrpc":"1.0","id":1,"method":"hello"}"#, json!(1)),
        (r#"{"jsonrpc":"2.0","id":2,"method":3}"#, json!(2)),
        (
            r#"{"jsonrpc":"2.0","id":"x","method":"m","params":4}"#,
            json!("x"),
        ),
        (r#"{"jsonrpc":"2.0","id":{},"method":"m"}"#, Json::Null),
    ] {
        let refused = refusal(line);
        assert_eq!(refused.error.code, -32600, "{line}");
        assert!(!refused.close, "{line}");
        assert_eq!(refused.id, id, "{line}");
    }
}

/// MAX-74 (step 1): a reply carries the request's id and its result or its error.
/// How: a result and a method not found.
#[test]
fn replies_written() {
    let ok: Json = serde_json::from_str(&reply(&json!(7), Ok(json!({"a": 1})))).unwrap();
    assert_eq!(ok, json!({"jsonrpc": "2.0", "id": 7, "result": {"a": 1}}));
    let error = reply(&json!("q"), Err(RpcError::method_not_found("nope")));
    let error: Json = serde_json::from_str(&error).unwrap();
    assert_eq!(
        error,
        json!({"jsonrpc": "2.0", "id": "q", "error": {"code": -32601, "message": "no method 'nope'"}})
    );
}
