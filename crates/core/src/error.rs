//! The errors the UI can act on (ADR 0008).

use serde::Serialize;
use specta::Type;

/// What the UI can do something about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ErrorKind {
    Offline,
    Unauthorized,
    WriteRefused,
    RateLimited {
        /// When the limit resets, in seconds since the Unix epoch, if GitHub said.
        resets_at: Option<u32>,
    },
    NotFound,
    Conflict,
    /// GitHub took the request and answered with an error about it. A failed status is `Internal`.
    QueryRefused,
    Internal,
}

/// A failure in plain words. Details go only to a local log, with tokens scrubbed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Error {
    #[serde(flatten)]
    pub kind: ErrorKind,
    pub message: String,
}

impl Error {
    pub fn new(kind: ErrorKind, message: &str) -> Self {
        Self {
            kind,
            message: message.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_error_reaches_the_ui_as_a_kind_and_a_message() {
        let error = Error::new(ErrorKind::NotFound, "No such Issue.");
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({ "kind": "not_found", "message": "No such Issue." })
        );
    }

    #[test]
    fn the_kinds_are_the_ones_adr_0008_names() {
        let kinds = [
            (ErrorKind::Offline, "offline"),
            (ErrorKind::Unauthorized, "unauthorized"),
            (ErrorKind::WriteRefused, "write_refused"),
            (ErrorKind::RateLimited { resets_at: None }, "rate_limited"),
            (ErrorKind::NotFound, "not_found"),
            (ErrorKind::Conflict, "conflict"),
            (ErrorKind::QueryRefused, "query_refused"),
            (ErrorKind::Internal, "internal"),
        ];
        for (kind, name) in kinds {
            assert_eq!(serde_json::to_value(kind).unwrap()["kind"], name);
        }
    }

    #[test]
    fn only_a_rate_limit_says_when_it_resets() {
        let limited = ErrorKind::RateLimited {
            resets_at: Some(1_800_000_000),
        };
        assert_eq!(
            serde_json::to_value(Error::new(limited, "Wait.")).unwrap(),
            json!({ "kind": "rate_limited", "resets_at": 1_800_000_000, "message": "Wait." })
        );
    }
}
