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

const NO_ISSUES: &str =
    r#"{"data":{"repository":{},"rateLimit":{"remaining":4000,"resetAt":"2026-11-04T23:00:00Z"}}}"#;

impl Replayer {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }
}

/// `query` with its numbers blanked and one field left of a read by number, whose fields differ only in
/// those.
fn without_numbers(query: &str) -> String {
    const FIELD: &str = "i#: issueOrPullRequest(number: #)";
    let mut blanked = String::new();
    for character in query.chars() {
        if !character.is_ascii_digit() {
            blanked.push(character);
        } else if !blanked.ends_with('#') {
            blanked.push('#');
        }
    }
    let before = blanked.split(FIELD).next().unwrap_or_default();
    let last = blanked.rsplit(FIELD).next().unwrap_or_default();
    format!("{before}{last}")
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
        // A sync also reads Issues by number, and each read's query holds its numbers. No recording holds
        // those, so a replay answers that they aren't there. A query that asks for something else still has
        // to match.
        let by_number = matches!(request.operation, "FetchIssues" | "FetchIssueReferences");
        if by_number
            && recording.query != request.query
            && without_numbers(&recording.query) == without_numbers(&request.query)
        {
            return Ok(Reply {
                status: 200,
                reset: None,
                date: None,
                body: NO_ISSUES.to_owned(),
            });
        }
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
            date: None,
            body,
        })
    }
}

#[cfg(test)]
mod tests;
