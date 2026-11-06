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
mod tests {
    use super::*;
    use crate::github::{Request, Transport};
    use crate::token::{GhOutput, Token};

    fn fixture(name: &str, contents: &str) -> std::path::PathBuf {
        let dir = crate::testing::temp_path(&format!("replay-{name}"));
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
                    query: "q".into(),
                    variables: serde_json::Value::Null,
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
                    query: "q".into(),
                    variables: serde_json::Value::Null,
                },
            )
            .unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        assert_eq!(reply.body, "<html>");
    }

    /// A read by number of `numbers`, each field asking for `fields`.
    fn numbered(numbers: &[u32], fields: &str) -> String {
        let asked: Vec<String> = numbers
            .iter()
            .map(|n| format!("i{n}: issueOrPullRequest(number: {n}) {{ {fields} }}"))
            .collect();
        format!(
            "query Op {{ repository {{ {} }} rateLimit {{ remaining }} }}",
            asked.join(" ")
        )
    }

    /// A replay's answer to `query`, with `recorded` as the recording of `operation`.
    #[expect(
        clippy::unwrap_in_result,
        reason = "a test helper: the `Result` is whether the replay panicked"
    )]
    fn replayed(
        name: &str,
        operation: &str,
        recorded: &str,
        query: String,
    ) -> std::thread::Result<Reply> {
        let text =
            serde_json::json!({ "query": recorded, "status": 200, "reset": null, "body": {} });
        let dir = fixture(name, &text.to_string());
        std::fs::rename(dir.join("Op.json"), dir.join(format!("{operation}.json"))).unwrap();
        let request = Request {
            operation,
            query: query.into(),
            variables: serde_json::Value::Null,
        };
        let replayer = Replayer::new(dir.clone());
        let token = token();
        let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            replayer.post(&token, &request).unwrap()
        }));
        std::fs::remove_dir_all(dir).unwrap();
        answer
    }

    #[test]
    fn a_read_of_numbers_no_recording_holds_finds_none() {
        for operation in ["FetchIssues", "FetchIssueReferences"] {
            let recorded = numbered(&[7, 8, 9], "number");
            let name = format!("other-{operation}");
            let reply = replayed(&name, operation, &recorded, numbered(&[120], "number")).unwrap();
            assert_eq!(reply.body, NO_ISSUES);
        }
    }

    #[test]
    fn a_read_by_number_that_asks_for_other_fields_needs_a_new_recording() {
        let recorded = numbered(&[7, 8], "number");
        let asked = numbered(&[7, 8], "number title");
        assert!(replayed("changed-fields", "FetchIssues", &recorded, asked).is_err());
    }

    #[test]
    fn another_operation_with_other_numbers_needs_a_new_recording() {
        let recorded = numbered(&[7], "number");
        assert!(replayed("other-operation", "Op", &recorded, numbered(&[8], "number")).is_err());
    }

    #[test]
    fn a_changed_query_needs_a_new_recording() {
        let dir = fixture(
            "stale",
            r#"{"query":"old","status":200,"reset":null,"body":{}}"#,
        );
        let request = Request {
            operation: "Op",
            query: "new".into(),
            variables: serde_json::Value::Null,
        };
        let replayer = Replayer::new(dir.clone());
        let token = token();
        // Caught rather than `should_panic`, so the test can still remove its folder.
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            replayer.post(&token, &request)
        }))
        .unwrap_err();
        std::fs::remove_dir_all(dir).unwrap();
        let message = panic.downcast_ref::<String>().unwrap();
        assert!(message.contains("record it again"), "{message}");
    }
}
