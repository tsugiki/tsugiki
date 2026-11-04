//! Captures real answers as fixtures for the replayer, with tokens scrubbed and each person's login replaced
//! wherever an answer names them (ADR 0009).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};

use super::{Reply, Request, Transport, Unreachable};
use crate::token::Token;

const PREFIXES: [&str; 6] = ["gho_", "ghp_", "ghs_", "ghu_", "ghr_", "github_pat_"];
const SCRUBBED: &str = "[token]";

/// Written in place of the viewer's login, so a fixture doesn't depend on who recorded it.
const VIEWER_STAND_IN: &str = "octocat";
/// The fields of an answer that people write, where a login can sit inside the text.
const PROSE_FIELDS: [&str; 2] = ["title", "body"];

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

/// Empties the description of each PR on its default branch. Tsugiki reads a description only off that branch
/// (ADR 0011), so here it would add nothing but text that can name people.
fn blank_unread_descriptions(body: &mut Value) {
    let Some(Value::Array(nodes)) = body.pointer_mut("/data/nodes") else {
        return;
    };
    for node in nodes {
        let base = node.pointer("/baseRefName");
        let on_default =
            base.is_some() && base == node.pointer("/repository/defaultBranchRef/name");
        if on_default && let Some(description) = node.get_mut("body") {
            *description = Value::String(String::new());
        }
    }
}

/// True for a character a GitHub login can hold.
fn is_login_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

/// The logins a run of recordings has met, in lower case, and the stand-in for each. One person keeps one
/// stand-in across recordings, so a PR's author and the same person's Issue still match.
#[derive(Default)]
struct StandIns {
    by_login: BTreeMap<String, String>,
}

impl StandIns {
    fn add_viewer(&mut self, login: &str) {
        drop(
            self.by_login
                .insert(login.to_ascii_lowercase(), VIEWER_STAND_IN.to_owned()),
        );
    }

    /// The stand-in for `login`, a new one the first time.
    fn for_login(&mut self, login: &str) -> String {
        let next = format!("user-{}", self.by_login.len());
        self.by_login
            .entry(login.to_ascii_lowercase())
            .or_insert(next)
            .clone()
    }

    fn known(&self, word: &str) -> Option<&String> {
        self.by_login.get(&word.to_ascii_lowercase())
    }

    /// Replaces each `login` field, so the next pass knows everyone this answer names.
    fn replace_logins(&mut self, value: &mut Value) {
        match value {
            Value::Array(items) => {
                for item in items {
                    self.replace_logins(item);
                }
            }
            Value::Object(fields) => {
                for (name, field) in fields.iter_mut() {
                    if name == "login"
                        && let Value::String(login) = &mut *field
                    {
                        *login = self.for_login(login);
                    } else {
                        self.replace_logins(field);
                    }
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }

    /// Replaces known logins in `value`'s other strings. Elsewhere only a whole string counts, because
    /// replacing part of one could change a node's id.
    fn replace_in_strings(&mut self, value: &mut Value) {
        match value {
            Value::String(text) => {
                if let Some(stand_in) = self.known(text) {
                    text.clone_from(stand_in);
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.replace_in_strings(item);
                }
            }
            Value::Object(fields) => {
                for (name, field) in fields.iter_mut() {
                    if PROSE_FIELDS.contains(&name.as_str())
                        && let Value::String(text) = &mut *field
                    {
                        *text = self.in_prose(text);
                    } else {
                        self.replace_in_strings(field);
                    }
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }

    /// `text` with each known login and each `@mention` replaced. A word with `@` right after another word
    /// is an email's domain, which names nobody. A real name written out in `text` stays, so read a new
    /// recording before you commit it.
    fn in_prose(&mut self, text: &str) -> String {
        let mut replaced = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find(is_login_char) {
            let (before, tail) = rest.split_at(start);
            let end = tail.find(|c| !is_login_char(c)).unwrap_or(tail.len());
            let (word, after) = tail.split_at(end);
            replaced.push_str(before);
            let is_mention = before
                .strip_suffix('@')
                .is_some_and(|head| !head.is_empty() || replaced.len() == before.len());
            if is_mention {
                replaced.push_str(&self.for_login(word));
            } else {
                replaced.push_str(self.known(word).map_or(word, String::as_str));
            }
            rest = after;
        }
        replaced.push_str(rest);
        replaced
    }
}

/// Passes requests on to `inner`, and writes each good answer to `<dir>/<operation>.json`. A failure
/// keeps the recording that is there.
pub struct Recorder {
    inner: Box<dyn Transport>,
    dir: PathBuf,
    /// Later answers name the viewer as an author or a reviewer, with nothing to show that it's them, so
    /// the answer that says who they are adds them here.
    stand_ins: RefCell<StandIns>,
}

impl Recorder {
    pub fn new(inner: Box<dyn Transport>, dir: PathBuf) -> Self {
        Self {
            inner,
            dir,
            stand_ins: RefCell::default(),
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
        blank_unread_descriptions(&mut body);
        let mut stand_ins = self.stand_ins.borrow_mut();
        if let Some(login) = body.pointer("/data/viewer/login").and_then(Value::as_str) {
            stand_ins.add_viewer(login);
        }
        stand_ins.replace_logins(&mut body);
        stand_ins.replace_in_strings(&mut body);
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
    fn a_recording_gives_everyone_else_a_stand_in_and_keeps_longer_strings() {
        let body = recorded(
            "others",
            [
                r#"{"data":{"viewer":{"login":"bob"}}}"#,
                r#"{"data":{"author":{"login":"bobcat"},"id":"PR_bob"}}"#,
            ],
        );
        assert_eq!(
            body,
            r#"{"data":{"author":{"login":"user-1"},"id":"PR_bob"}}"#
        );
    }

    #[test]
    fn a_login_met_before_anyone_says_who_the_viewer_is_gets_a_stand_in_too() {
        let body = recorded(
            "unknown",
            [
                r#"{"data":{"a":"b"}}"#,
                r#"{"data":{"author":{"login":"bob"}}}"#,
            ],
        );
        assert_eq!(body, r#"{"data":{"author":{"login":"user-0"}}}"#);
    }

    #[test]
    fn one_person_keeps_one_stand_in_across_recordings_whatever_the_case() {
        let body = recorded(
            "same",
            [
                r#"{"data":{"author":{"login":"Some-Author"}}}"#,
                r#"{"data":{"a":{"login":"someone-else"},"b":{"login":"some-author"}}}"#,
            ],
        );
        assert_eq!(
            body,
            r#"{"data":{"a":{"login":"user-1"},"b":{"login":"user-0"}}}"#
        );
    }

    #[test]
    fn a_recording_replaces_logins_and_mentions_inside_prose() {
        let body = recorded(
            "prose",
            [
                r#"{"data":{"viewer":{"login":"recorder"}}}"#,
                r#"{"data":{"author":{"login":"some-author"},
                    "body":"Thanks @Some-Author and @stranger, cc RECORDER and tsugi木: some-author@example.com",
                    "title":"@recorder not-some-author"}}"#,
            ],
        );
        let body: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            body["data"]["body"],
            "Thanks @user-1 and @user-2, cc octocat and tsugi木: user-1@example.com"
        );
        assert_eq!(body["data"]["title"], "@octocat not-some-author");
    }

    #[test]
    fn a_recording_keeps_a_description_only_off_the_default_branch() {
        let dir = temp_dir("descriptions");
        let on = |base: &str| {
            json!({
                "baseRefName": base,
                "body": "Fixes #5",
                "repository": { "defaultBranchRef": { "name": "main" } },
            })
        };
        let nodes = json!({ "data": { "nodes": [null, on("main"), on("3.14"), { "body": "x" }] } });
        let answer = reply(&nodes.to_string());
        let recorder = Recorder::new(Box::new(Canned(Ok(answer))), dir.clone());

        let live = recorder.post(&token(), &REQUEST).unwrap();
        let replayed = Replayer::new(dir.clone()).post(&token(), &REQUEST).unwrap();
        std::fs::remove_dir_all(dir).unwrap();

        let body: Value = serde_json::from_str(&replayed.body).unwrap();
        let bodies = |at: usize| body["data"]["nodes"][at]["body"].clone();
        assert_eq!(bodies(0), Value::Null);
        assert_eq!(bodies(1), "");
        assert_eq!(bodies(2), "Fixes #5");
        assert_eq!(bodies(3), "x");
        assert!(
            live.body.contains("Fixes #5"),
            "the caller still gets the real answer"
        );
    }

    #[test]
    fn a_whole_string_that_is_a_known_login_gets_its_stand_in_in_any_field() {
        let body = recorded(
            "whole",
            [
                r#"{"data":{"viewer":{"login":"recorder"}}}"#,
                r#"{"data":{"owner":"Recorder","other":"recorder and more"}}"#,
            ],
        );
        assert_eq!(
            body,
            r#"{"data":{"other":"recorder and more","owner":"octocat"}}"#
        );
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
