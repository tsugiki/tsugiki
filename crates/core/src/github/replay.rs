//! Answers GraphQL requests from recorded JSON, in place of the network (ADR 0009).

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use super::{Reply, Request, Selection, Transport, Unreachable, Write};
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

/// The answer to a read by numbers that no recording holds: none of them are there.
const NO_ISSUES: &str = r#"{"data":{"repository":{},"rateLimit":{"cost":1,"limit":5000,"used":1000,
    "remaining":4000,"resetAt":"2999-01-01T00:00:00Z"}}}"#;

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
    fn post(&self, _: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        let (path, recording) = self.recording(request.operation);
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
        Ok(recording.answer(&path, &request.query))
    }

    /// Answers a write from the recording named after its operation, as `Https` sends it: a selection the
    /// token crate refuses is refused here too.
    fn write(
        &self,
        _: &Token,
        write: &Write,
        selection: Selection<'_>,
    ) -> Result<Reply, Unreachable> {
        let query = write
            .query(selection)
            .map_err(|_refused| Unreachable::Refused)?;
        let (path, recording) = self.recording(write.operation());
        Ok(recording.answer(&path, &query))
    }
}

impl Replayer {
    /// The recording of `operation`, and where it is.
    #[expect(
        clippy::expect_used,
        reason = "a test helper: a missing or unreadable recording should fail the test"
    )]
    fn recording(&self, operation: &str) -> (PathBuf, Recording) {
        let path = self.dir.join(format!("{operation}.json"));
        let text = std::fs::read_to_string(&path).expect("no recording for this operation");
        let recording = serde_json::from_str(&text).expect("an unreadable recording");
        (path, recording)
    }
}

impl Recording {
    /// The recorded answer, to `query`, which the recording at `path` must have been made of.
    fn answer(self, path: &Path, query: &str) -> Reply {
        assert_eq!(
            self.query, query,
            "the query changed since {path:?} was recorded: record it again"
        );
        let body = (self.body.as_str()).map_or_else(|| self.body.to_string(), ToOwned::to_owned);
        Reply {
            status: self.status,
            reset: self.reset,
            date: None,
            body,
        }
    }
}

#[cfg(test)]
mod tests;
