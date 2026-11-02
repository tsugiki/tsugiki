//! Captures real answers as fixtures for the replayer, with tokens scrubbed (ADR 0009).

use std::path::PathBuf;

use serde_json::{Value, json};

use super::{Reply, Request, Transport, Unreachable};
use crate::token::Token;

const PREFIXES: [&str; 6] = ["gho_", "ghp_", "ghs_", "ghu_", "ghr_", "github_pat_"];
const SCRUBBED: &str = "[token]";

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// What follows the token that `text` starts with, if it does.
fn after_token(text: &str) -> Option<&str> {
    PREFIXES.iter().find_map(|prefix| {
        let tail = text.strip_prefix(prefix)?;
        let after = tail.trim_start_matches(is_token_char);
        (after.len() < tail.len()).then_some(after)
    })
}

/// Removes `token`, and anything else shaped like a GitHub token, wherever it starts: the text is raw
/// JSON, so a token can follow an escape such as `\n`.
pub fn scrub(text: &str, token: &str) -> String {
    let text = text.replace(token, SCRUBBED);
    let mut scrubbed = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while !rest.is_empty() {
        if let Some(after) = after_token(rest) {
            scrubbed.push_str(SCRUBBED);
            rest = after;
        } else {
            let mut chars = rest.chars();
            scrubbed.extend(chars.next());
            rest = chars.as_str();
        }
    }
    scrubbed
}

/// Passes requests on to `inner`, and writes each good answer to `<dir>/<operation>.json`. A failure
/// keeps the recording that is there.
pub struct Recorder {
    inner: Box<dyn Transport>,
    dir: PathBuf,
}

impl Recorder {
    pub fn new(inner: Box<dyn Transport>, dir: PathBuf) -> Self {
        Self { inner, dir }
    }
}

impl Transport for Recorder {
    #[expect(
        clippy::expect_used,
        clippy::unwrap_in_result,
        reason = "a dev tool: a recording it can't write should stop it, and a failed request isn't one"
    )]
    fn post(&self, token: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        let reply = self.inner.post(token, request)?;
        let text = scrub(&reply.body, token.expose());
        let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
        if reply.status != 200 || body.get("errors").is_some() {
            return Ok(reply);
        }
        let recording = json!({
            "query": request.query,
            "status": reply.status,
            "reset": reply.reset,
            "body": body,
        });
        let pretty =
            serde_json::to_string_pretty(&recording).expect("JSON values always serialize");
        std::fs::create_dir_all(&self.dir).expect("create the fixtures folder");
        let path = self.dir.join(format!("{}.json", request.operation));
        std::fs::write(path, pretty + "\n").expect("write the recording");
        Ok(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::replay::Replayer;
    use crate::github::testing::Canned;
    use crate::token::GhOutput;
    use std::path::PathBuf;

    const REQUEST: Request<'static> = Request {
        operation: "Op",
        query: "query Op { a }",
    };

    fn token() -> Token {
        Token::from_gh(|_| {
            Ok(GhOutput {
                success: true,
                stdout: "secret-token-value".to_owned(),
            })
        })
        .unwrap()
    }

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("tsugiki-record-{name}-{}", std::process::id()))
    }

    fn reply(body: &str) -> Reply {
        Reply {
            status: 200,
            reset: Some(9),
            body: body.to_owned(),
        }
    }

    #[test]
    fn scrubbing_removes_the_token_in_use() {
        assert_eq!(
            scrub("a secret-token-value b", "secret-token-value"),
            "a [token] b"
        );
    }

    #[test]
    fn scrubbing_removes_anything_shaped_like_a_github_token() {
        let text = "gho_abc123 ghp_ABC ghs_a_b ghu_1 ghr_2 github_pat_11A_b0, other";
        assert_eq!(
            scrub(text, "unused"),
            "[token] [token] [token] [token] [token] [token], other"
        );
    }

    #[test]
    fn scrubbing_leaves_words_that_only_start_like_a_token() {
        assert_eq!(scrub("gho_ ghost", "unused"), "gho_ ghost");
    }

    #[test]
    fn scrubbing_removes_a_token_right_after_an_escape() {
        assert_eq!(
            scrub(r#"see:\ngho_abc %3Dghp_def"#, "unused"),
            r#"see:\n[token] %3D[token]"#
        );
    }

    #[test]
    fn a_failed_answer_leaves_the_recording_alone() {
        for (status, body) in [(502, "<html>"), (200, r#"{"errors":[{"message":"x"}]}"#)] {
            let dir = temp_dir(&format!("failed-{status}"));
            let answer = Reply {
                status,
                reset: None,
                body: body.to_owned(),
            };
            let recorder = Recorder::new(Box::new(Canned(Ok(answer))), dir.clone());
            assert_eq!(recorder.post(&token(), &REQUEST).unwrap().status, status);
            assert!(!dir.exists(), "{status} {body}");
        }
    }

    #[test]
    fn scrubbing_leaves_other_text_alone_including_non_ascii() {
        assert_eq!(scrub("tsugi木 ok", "unused"), "tsugi木 ok");
    }

    #[test]
    fn a_recording_replays_with_the_token_scrubbed() {
        let dir = temp_dir("roundtrip");
        let body = r#"{"data":{"note":"secret-token-value"}}"#;
        let recorder = Recorder::new(Box::new(Canned(Ok(reply(body)))), dir.clone());

        let live = recorder.post(&token(), &REQUEST).unwrap();
        let replayed = Replayer::new(dir.clone()).post(&token(), &REQUEST).unwrap();
        let on_disk = std::fs::read_to_string(dir.join("Op.json")).unwrap();
        std::fs::remove_dir_all(dir).unwrap();

        assert_eq!(live.body, body, "the caller still gets the real answer");
        assert_eq!(replayed.status, 200);
        assert_eq!(replayed.reset, Some(9));
        assert_eq!(replayed.body, r#"{"data":{"note":"[token]"}}"#);
        assert!(!on_disk.contains("secret-token-value"));
        assert!(on_disk.ends_with('\n'));
    }

    #[test]
    fn a_body_that_isnt_json_is_recorded_as_text() {
        let dir = temp_dir("text");
        let recorder = Recorder::new(Box::new(Canned(Ok(reply("<html>")))), dir.clone());
        let _ = recorder.post(&token(), &REQUEST).unwrap();
        let replayed = Replayer::new(dir.clone()).post(&token(), &REQUEST).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        assert_eq!(replayed.body, "<html>");
    }

    #[test]
    fn no_connection_records_nothing() {
        let dir = temp_dir("offline");
        let recorder = Recorder::new(Box::new(Canned(Err(Unreachable))), dir.clone());
        assert!(recorder.post(&token(), &REQUEST).is_err());
        assert!(!dir.exists());
    }
}
