//! JSON-RPC 2.0 on the API socket (MAX-74): one message per line. Reading a line into a request
//! and writing replies; no socket here, so it is tested alone
use serde_json::{Map, Value as Json, json};

use crate::commands::CommandError;

/// The longest line read, its newline included: past it the connection closes, so a client
/// that never sends a newline can't grow the buffer forever
pub const MAX_LINE: usize = 1024 * 1024;

/// The error codes: JSON-RPC's own for the protocol, maxicom's from 100 for the commands
/// (MAX-284), one per kind of `CommandError`
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const UNCLOSED_QUOTE: i64 = 101;
pub const BAD_ESCAPE: i64 = 102;
pub const UNKNOWN_COMMAND: i64 = 103;
pub const MISSING_ARGUMENT: i64 = 104;
pub const TOO_MANY_ARGUMENTS: i64 = 105;
pub const BAD_ARGUMENT: i64 = 106;
pub const COMMAND_FAILED: i64 = 107;
pub const NAMED_WITH_ARGUMENTS: i64 = 108;
/// `query`'s line isn't one query
pub const NOT_A_QUERY: i64 = 110;
/// Refused by the access levels the user set in `[api]` (MAX-287)
pub const REFUSED: i64 = 111;

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
            code: PARSE_ERROR,
            message: message.into(),
        }
    }

    /// JSON, but not a request
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: INVALID_REQUEST,
            message: message.into(),
        }
    }

    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: METHOD_NOT_FOUND,
            message: format!("no method '{method}'"),
        }
    }

    /// The method's params aren't what it takes
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: INVALID_PARAMS,
            message: message.into(),
        }
    }

    /// A command line refused (MAX-284): one code per kind of error, the message the TUI's
    pub fn command(error: &CommandError) -> Self {
        let code = match error {
            CommandError::UnclosedQuote => UNCLOSED_QUOTE,
            CommandError::BadEscape(_) => BAD_ESCAPE,
            CommandError::UnknownCommand(_) => UNKNOWN_COMMAND,
            CommandError::MissingArgument { .. } => MISSING_ARGUMENT,
            CommandError::TooManyArguments { .. } => TOO_MANY_ARGUMENTS,
            CommandError::BadArgument { .. } => BAD_ARGUMENT,
            CommandError::Failed { .. } => COMMAND_FAILED,
            CommandError::NamedWithArguments(_) => NAMED_WITH_ARGUMENTS,
        };
        Self {
            code,
            message: error.to_string(),
        }
    }

    /// Refused by the user's `[api]` levels (MAX-287)
    pub fn refused(message: impl Into<String>) -> Self {
        Self {
            code: REFUSED,
            message: message.into(),
        }
    }

    /// `query`'s line isn't one query (MAX-284): an action, or several commands
    pub fn not_a_query(message: impl Into<String>) -> Self {
        Self {
            code: NOT_A_QUERY,
            message: message.into(),
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

/// A notification line, sent with no request: an event to a subscriber (MAX-284)
pub fn notification(method: &str, params: Json) -> String {
    json!({ "jsonrpc": "2.0", "method": method, "params": params }).to_string()
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
