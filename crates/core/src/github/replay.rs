//! Answers GraphQL requests from recorded JSON, in place of the network (ADR 0009).

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

use super::{Reply, Request, Transport, Unreachable};
use crate::token::Token;

/// One recorded exchange, in `<operation>.json`.
#[derive(Deserialize)]
struct Recording {
    query: String,
    status: u16,
    reset: Option<u32>,
    body: Value,
}

/// Answers each request from the recording named after its operation.
pub struct Replayer {
    dir: PathBuf,
}

impl Replayer {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }
}

impl Transport for Replayer {
    #[expect(
        clippy::expect_used,
        clippy::panic_in_result_fn,
        clippy::unwrap_in_result,
        reason = "a test helper: a missing or stale recording should fail the test"
    )]
    fn post(&self, _: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        let path = self.dir.join(format!("{}.json", request.operation));
        let text = std::fs::read_to_string(&path).expect("no recording for this operation");
        let recording: Recording = serde_json::from_str(&text).expect("an unreadable recording");
        assert_eq!(
            recording.query, request.query,
            "the query changed since {path:?} was recorded: record it again"
        );
        let body = recording
            .body
            .as_str()
            .map_or_else(|| recording.body.to_string(), ToOwned::to_owned);
        Ok(Reply {
            status: recording.status,
            reset: recording.reset,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{Request, Transport};
    use crate::token::{GhOutput, Token};

    fn fixture(name: &str, contents: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tsugiki-replay-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Op.json"), contents).unwrap();
        dir
    }

    fn token() -> Token {
        Token::from_gh(|_| {
            Ok(GhOutput {
                success: true,
                stdout: "t".to_owned(),
            })
        })
        .unwrap()
    }

    #[test]
    fn a_json_body_comes_back_as_text() {
        let dir = fixture(
            "json",
            r#"{"query":"q","status":200,"reset":7,"body":{"a":1}}"#,
        );
        let reply = Replayer::new(dir.clone())
            .post(
                &token(),
                &Request {
                    operation: "Op",
                    query: "q",
                },
            )
            .unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        assert_eq!((reply.status, reply.reset), (200, Some(7)));
        assert_eq!(reply.body, r#"{"a":1}"#);
    }

    #[test]
    fn a_body_that_isnt_json_comes_back_unchanged() {
        let dir = fixture(
            "text",
            r#"{"query":"q","status":502,"reset":null,"body":"<html>"}"#,
        );
        let reply = Replayer::new(dir.clone())
            .post(
                &token(),
                &Request {
                    operation: "Op",
                    query: "q",
                },
            )
            .unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        assert_eq!(reply.body, "<html>");
    }

    #[test]
    #[should_panic(expected = "record it again")]
    fn a_changed_query_needs_a_new_recording() {
        let dir = fixture(
            "stale",
            r#"{"query":"old","status":200,"reset":null,"body":{}}"#,
        );
        let request = Request {
            operation: "Op",
            query: "new",
        };
        let _ = Replayer::new(dir).post(&token(), &request).unwrap();
    }
}
