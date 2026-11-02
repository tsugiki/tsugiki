//! The errors the UI can act on (ADR 0008).

use serde::Serialize;
use specta::Type;

/// What the UI can do something about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Offline,
    Unauthorized,
    WriteRefused,
    RateLimited,
    NotFound,
    Conflict,
    Internal,
}

/// A failure in plain words. Details go only to a local log, with tokens scrubbed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
    /// When a rate limit resets, in seconds since the Unix epoch, if GitHub said.
    pub resets_at: Option<u32>,
}

impl Error {
    pub fn new(kind: ErrorKind, message: &str) -> Self {
        Self {
            kind,
            message: message.to_owned(),
            resets_at: None,
        }
    }

    pub fn rate_limited(message: &str, resets_at: Option<u32>) -> Self {
        Self {
            resets_at,
            ..Self::new(ErrorKind::RateLimited, message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_reaches_the_ui_as_a_kind_and_a_message() {
        let json = serde_json::to_value(Error::new(ErrorKind::NotFound, "No such Issue.")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "kind": "not_found", "message": "No such Issue.", "resets_at": null })
        );
    }

    #[test]
    fn the_kinds_are_the_ones_adr_0008_names() {
        let kinds = [
            (ErrorKind::Offline, "offline"),
            (ErrorKind::Unauthorized, "unauthorized"),
            (ErrorKind::WriteRefused, "write_refused"),
            (ErrorKind::RateLimited, "rate_limited"),
            (ErrorKind::NotFound, "not_found"),
            (ErrorKind::Conflict, "conflict"),
            (ErrorKind::Internal, "internal"),
        ];
        for (kind, name) in kinds {
            assert_eq!(serde_json::to_value(kind).unwrap(), name);
        }
    }

    #[test]
    fn a_rate_limit_says_when_it_resets() {
        let error = Error::rate_limited("Wait for the reset.", Some(1_800_000_000));
        assert_eq!(error.kind, ErrorKind::RateLimited);
        assert_eq!(error.resets_at, Some(1_800_000_000));
    }
}
