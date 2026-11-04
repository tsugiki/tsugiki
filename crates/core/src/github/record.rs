//! Captures real answers as fixtures for the replayer, with tokens scrubbed and the viewer's login replaced
//! wherever an answer names them (ADR 0009).

use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::{Value, json};

use super::{Reply, Request, Transport, Unreachable};
use crate::token::Token;

const PREFIXES: [&str; 6] = ["gho_", "ghp_", "ghs_", "ghu_", "ghr_", "github_pat_"];
const SCRUBBED: &str = "[token]";

/// Written in place of the viewer's login, so a fixture doesn't depend on who recorded it.
const STAND_IN_LOGIN: &str = "octocat";

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

/// Puts the stand-in wherever a whole string is `login`, whatever its case. A login inside a longer string
/// stays: replacing part of a string could change a node's id. A PR's title or description can name someone
/// that way, so read those before you commit a recording.
fn replace_login(value: &mut Value, login: &str) {
    match value {
        Value::String(text) if text.eq_ignore_ascii_case(login) => {
            STAND_IN_LOGIN.clone_into(text);
        }
        Value::Array(items) => {
            for item in items {
                replace_login(item, login);
            }
        }
        Value::Object(fields) => {
            for field in fields.values_mut() {
                replace_login(field, login);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Passes requests on to `inner`, and writes each good answer to `<dir>/<operation>.json`. A failure
/// keeps the recording that is there.
pub struct Recorder {
    inner: Box<dyn Transport>,
    dir: PathBuf,
    /// The first answer that says who the viewer is sets this. Later answers name them as an author or a
    /// reviewer, with nothing to show that it's them.
    viewer: OnceLock<String>,
}

impl Recorder {
    pub fn new(inner: Box<dyn Transport>, dir: PathBuf) -> Self {
        Self {
            inner,
            dir,
            viewer: OnceLock::new(),
        }
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
        let mut body = serde_json::from_str(&text).unwrap_or(Value::String(text));
        if reply.status != 200 || body.get("errors").is_some() {
            return Ok(reply);
        }
        let named = body.pointer("/data/viewer/login").and_then(Value::as_str);
        let viewer = named
            .map(|login| self.viewer.get_or_init(|| login.to_owned()))
            .or(self.viewer.get())
            .cloned();
        if let Some(login) = viewer {
            replace_login(&mut body, &login);
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
        query: std::borrow::Cow::Borrowed("query Op { a }"),
        variables: serde_json::Value::Null,
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
        crate::testing::temp_path(&format!("record-{name}"))
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
    fn a_recording_doesnt_keep_who_made_it() {
        let dir = temp_dir("viewer");
        let body = r#"{"data":{"viewer":{"login":"whoever-recorded"}}}"#;
        let recorder = Recorder::new(Box::new(Canned(Ok(reply(body)))), dir.clone());

        let live = recorder.post(&token(), &REQUEST).unwrap();
        let replayed = Replayer::new(dir.clone()).post(&token(), &REQUEST).unwrap();
        std::fs::remove_dir_all(dir).unwrap();

        assert_eq!(live.body, body, "the caller still gets the real answer");
        assert_eq!(replayed.body, r#"{"data":{"viewer":{"login":"octocat"}}}"#);
    }

    /// Answers one request with each of `bodies`, in order, through one recorder, and returns what it wrote.
    fn recorded(name: &str, bodies: [&str; 2]) -> String {
        let dir = temp_dir(name);
        let replies = bodies
            .into_iter()
            .map(crate::github::testing::reply_ok)
            .collect();
        let (scripted, _requests) = crate::github::testing::Scripted::new(replies);
        let recorder = Recorder::new(Box::new(scripted), dir.clone());
        for _ in bodies {
            let _ = recorder.post(&token(), &REQUEST).unwrap();
        }
        let replayed = Replayer::new(dir.clone()).post(&token(), &REQUEST).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        replayed.body
    }

    #[test]
    fn a_later_recording_doesnt_keep_who_made_it_either() {
        let body = recorded(
            "later",
            [
                r#"{"data":{"viewer":{"login":"Whoever-Recorded"}}}"#,
                r#"{"data":{"nodes":[{"author":{"login":"whoever-recorded"},"n":1,"none":null,"ok":true}]}}"#,
            ],
        );
        assert_eq!(
            body,
            r#"{"data":{"nodes":[{"author":{"login":"octocat"},"n":1,"none":null,"ok":true}]}}"#
        );
    }

    #[test]
    fn a_recording_keeps_everyone_else_and_longer_strings() {
        let body = recorded(
            "others",
            [
                r#"{"data":{"viewer":{"login":"bob"}}}"#,
                r#"{"data":{"author":{"login":"bobcat"},"id":"PR_bob"}}"#,
            ],
        );
        assert_eq!(
            body,
            r#"{"data":{"author":{"login":"bobcat"},"id":"PR_bob"}}"#
        );
    }

    #[test]
    fn a_recording_before_anyone_says_who_the_viewer_is_stays_as_it_came() {
        let body = recorded(
            "unknown",
            [
                r#"{"data":{"a":"b"}}"#,
                r#"{"data":{"author":{"login":"bob"}}}"#,
            ],
        );
        assert_eq!(body, r#"{"data":{"author":{"login":"bob"}}}"#);
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
