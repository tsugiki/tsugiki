//! JSON-RPC 2.0, which LSP messages are: what a server sends, parsed into what kind of message it is, and
//! what the client sends, written out.

use std::fmt;

use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

#[cfg(test)]
mod tests;

/// A server that doesn't know a method the client asked for, or the client one the server asked for.
pub(crate) const METHOD_NOT_FOUND: i64 = -32601;
/// A message whose `params` don't have the shape its method needs.
pub(crate) const INVALID_PARAMS: i64 = -32602;

/// The id of a request. The client numbers its own, and a server may use text for its.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(untagged)]
pub(crate) enum RequestId {
    Number(i64),
    Text(String),
}

/// The error a request ended with.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub(crate) struct ResponseError {
    pub(crate) code: i64,
    pub(crate) message: String,
}

/// The reason a reply carries no result.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReplyError {
    /// The server answered with an error.
    Server(ResponseError),
    /// The reply isn't one JSON-RPC allows, as this says.
    Malformed(String),
}

/// One message a server sent.
#[derive(Debug, PartialEq)]
pub(crate) enum Incoming {
    /// The answer to one of the client's requests.
    Reply {
        id: RequestId,
        outcome: Result<Value, ReplyError>,
    },
    /// A request from the server, which waits for the client's answer.
    Request {
        id: RequestId,
        method: String,
        params: Value,
    },
    /// A message that needs no answer.
    Notification { method: String, params: Value },
}

/// The reason a body isn't a message.
#[derive(Debug)]
pub(crate) enum Unreadable {
    Json(serde_json::Error),
    /// JSON, but neither a request, a reply nor a notification.
    NotAMessage(String),
}

impl fmt::Display for Unreadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(cause) => write!(f, "isn't JSON: {cause}"),
            Self::NotAMessage(body) => write!(f, "isn't a JSON-RPC message: {body}"),
        }
    }
}

/// The fields a message can have. The ones it has tell its kind.
#[derive(Deserialize)]
struct Fields {
    id: Option<RequestId>,
    method: Option<String>,
    #[serde(default)]
    params: Value,
    /// `Some(Value::Null)` for a reply whose result is `null`, which is a reply all the same.
    #[serde(default, deserialize_with = "present")]
    result: Option<Value>,
    error: Option<ResponseError>,
}

fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

/// Parses one message's body. A body with one of the client's request ids and no method is a reply to that
/// request, and one that isn't a well-formed reply fails the request.
pub(crate) fn parse(body: &[u8]) -> Result<Incoming, Unreadable> {
    let value: Value = serde_json::from_slice(body).map_err(Unreadable::Json)?;
    let fields = Fields::deserialize(&value);
    match fields {
        Ok(Fields {
            id: Some(id),
            method: Some(method),
            params,
            ..
        }) => Ok(Incoming::Request { id, method, params }),
        Ok(Fields {
            id: None,
            method: Some(method),
            params,
            ..
        }) => Ok(Incoming::Notification { method, params }),
        Ok(Fields {
            id: Some(id),
            method: None,
            error: Some(error),
            ..
        }) => Ok(Incoming::Reply {
            id,
            outcome: Err(ReplyError::Server(error)),
        }),
        Ok(Fields {
            id: Some(id),
            method: None,
            result: Some(result),
            error: None,
            ..
        }) => Ok(Incoming::Reply {
            id,
            outcome: Ok(result),
        }),
        Ok(_) | Err(_) => match (value.get("id").and_then(Value::as_i64), value.get("method")) {
            (Some(id), None) => Ok(Incoming::Reply {
                id: RequestId::Number(id),
                outcome: Err(ReplyError::Malformed(format!(
                    "a reply of the wrong shape: {value}"
                ))),
            }),
            _ => Err(Unreadable::NotAMessage(value.to_string())),
        },
    }
}

/// A request from the client. One whose params are `null` goes without them, as `shutdown` does.
pub(crate) fn request(id: i64, method: &str, params: Value) -> Vec<u8> {
    with_params(
        json!({"jsonrpc": "2.0", "id": id, "method": method}),
        params,
    )
}

/// A notification from the client. One whose params are `null` goes without them, as `exit` does.
pub(crate) fn notification(method: &str, params: Value) -> Vec<u8> {
    with_params(json!({"jsonrpc": "2.0", "method": method}), params)
}

fn with_params(mut message: Value, params: Value) -> Vec<u8> {
    if let (Value::Object(fields), false) = (&mut message, params.is_null()) {
        let _replaced = fields.insert("params".to_owned(), params);
    }
    message.to_string().into_bytes()
}

/// The client's answer to a server's request.
pub(crate) fn reply(id: &RequestId, outcome: Result<Value, ResponseError>) -> Vec<u8> {
    let message = match outcome {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(ResponseError { code, message }) => {
            json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
        }
    };
    message.to_string().into_bytes()
}
