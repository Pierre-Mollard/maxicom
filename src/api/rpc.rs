//! JSON-RPC 2.0 on the API socket (MAX-74): one message per line. Reading a line into a request
//! and writing replies; no socket here, so it is tested alone
use serde_json::{Map, Value as Json, json};

/// The longest line read, its newline included: past it the connection closes, so a client
/// that never sends a newline can't grow the buffer forever
pub const MAX_LINE: usize = 1024 * 1024;

/// A request read from a line: a notification (no `id`) gets no reply
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub id: Option<Json>,
    pub method: String,
    pub params: Option<Json>,
}

/// A JSON-RPC error: its code and message
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcError {
    /// Not JSON, or not UTF-8
    pub fn parse(message: impl Into<String>) -> Self {
        Self {
            code: -32700,
            message: message.into(),
        }
    }

    /// JSON, but not a request
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: -32600,
            message: message.into(),
        }
    }

    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("no method '{method}'"),
        }
    }
}

/// A line that isn't a request: the error to reply with (to `id`, null when unknown), and
/// whether the connection closes, the stream being out of step
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    pub id: Json,
    pub error: RpcError,
    pub close: bool,
}

/// Reads a line, its end of line taken off. Not JSON closes the connection; JSON that isn't a
/// request, or a batch (not supported), is refused and the connection goes on
pub fn parse(line: &[u8]) -> Result<Request, Refusal> {
    let refuse = |id: Json, error: RpcError, close: bool| Refusal { id, error, close };
    let json: Json = match serde_json::from_slice(line) {
        Ok(json) => json,
        Err(err) => return Err(refuse(Json::Null, RpcError::parse(err.to_string()), true)),
    };
    let Json::Object(mut object) = json else {
        let message = match json {
            Json::Array(_) => "batches aren't supported: one request per line",
            _ => "a request is an object",
        };
        return Err(refuse(
            Json::Null,
            RpcError::invalid_request(message),
            false,
        ));
    };
    let id = object.remove("id");
    let invalid = |message: &str| {
        let id = id.clone().unwrap_or(Json::Null);
        Err(refuse(id, RpcError::invalid_request(message), false))
    };
    if !matches!(
        &id,
        None | Some(Json::String(_) | Json::Number(_) | Json::Null)
    ) {
        return Err(refuse(
            Json::Null,
            RpcError::invalid_request("an id is a string, a number or null"),
            false,
        ));
    }
    if object.get("jsonrpc").and_then(Json::as_str) != Some("2.0") {
        return invalid("\"jsonrpc\" must be \"2.0\"");
    }
    let Some(Json::String(method)) = object.remove("method") else {
        return invalid("\"method\" must be a string");
    };
    let params = object.remove("params");
    if !matches!(&params, None | Some(Json::Object(_) | Json::Array(_))) {
        return invalid("\"params\" must be an object or an array");
    }
    Ok(Request { id, method, params })
}

/// The reply line to `id`: its result, or its error
pub fn reply(id: &Json, result: Result<Json, RpcError>) -> String {
    let mut message = Map::new();
    message.insert("jsonrpc".to_string(), json!("2.0"));
    message.insert("id".to_string(), id.clone());
    match result {
        Ok(result) => message.insert("result".to_string(), result),
        Err(error) => message.insert(
            "error".to_string(),
            json!({ "code": error.code, "message": error.message }),
        ),
    };
    Json::Object(message).to_string()
}

#[cfg(test)]
#[path = "../tests/api/rpc.rs"]
mod tests;
