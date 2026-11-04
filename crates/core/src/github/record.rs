//! Captures real answers as fixtures for the replayer, with tokens scrubbed and each person's login replaced
//! wherever an answer names them (ADR 0009).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};

use super::{ConditionalGet, ConditionalReply, Reply, Request, Transport, Unreachable};
use crate::conventions::CPYTHON_BACKPORT_BOT;
use crate::token::Token;

const PREFIXES: [&str; 6] = ["gho_", "ghp_", "ghs_", "ghu_", "ghr_", "github_pat_"];
const SCRUBBED: &str = "[token]";

/// Written in place of the viewer's login, so a fixture doesn't depend on who recorded it.
const VIEWER_STAND_IN: &str = "octocat";
/// Stands in for a description nothing reads, with the line breaks GitHub's own answers have.
const FILLER: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit.\r\n\r\n";
/// The markers of a line of a description that names a person, with their email.
const PEOPLE_LINES: [&str; 2] = ["co-authored-by:", "signed-off-by:"];
/// The one owner a recording's Repository names stay under. An answer also names other Repositories, such
/// as a fork that mentions an Issue, whose owner is a person.
const RECORDED_OWNER: &str = "python";
/// The start of the one line Tsugiki reads in a commit message.
const CHERRY_PICK: &str = "(cherry picked from commit ";
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

/// Keeps a recording to what Tsugiki reads, because the rest can name people. A description on the default
/// branch becomes filler of the same length (ADR 0011), and the length keeps a recording as large as GitHub's
/// answer. A description off that branch loses the lines that credit a person, and a commit message keeps
/// only its cherry-pick lines.
fn keep_only_what_is_read(body: &mut Value) {
    if let Some(Value::Array(nodes)) = body.pointer_mut("/data/nodes") {
        nodes.iter_mut().for_each(keep_what_a_pr_reads);
    }
    if let Some(node) = body.pointer_mut("/data/repository/pullRequest") {
        keep_what_a_pr_reads(node);
    }
}

fn keep_what_a_pr_reads(node: &mut Value) {
    let base = node.pointer("/baseRefName");
    let on_default = base.is_some() && base == node.pointer("/repository/defaultBranchRef/name");
    if let Some(Value::String(description)) = node.get_mut("body") {
        *description = if on_default {
            FILLER
                .chars()
                .cycle()
                .take(description.chars().count())
                .collect()
        } else {
            without_lines(description, |line| {
                let line = line.to_ascii_lowercase();
                PEOPLE_LINES.iter().any(|marker| line.contains(marker))
            })
        };
    }
    if let Some(Value::Array(commits)) = node.pointer_mut("/commits/nodes") {
        for commit in commits {
            if let Some(Value::String(message)) = commit.pointer_mut("/commit/message") {
                *message = without_lines(message, |line| !line.starts_with(CHERRY_PICK));
            }
        }
    }
}

/// `text` without the lines `unwanted` picks, with its line breaks kept.
fn without_lines(text: &str, unwanted: impl Fn(&str) -> bool) -> String {
    text.split_inclusive('\n')
        .filter(|line| !unwanted(line.trim_start()))
        .collect()
}

/// True for a character a GitHub login can hold.
fn is_login_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

/// The logins a run of recordings has met, in lower case, and the stand-in for each. One person keeps one
/// stand-in across recordings, so a PR's author and the same person's Issue still match.
struct StandIns {
    by_login: BTreeMap<String, String>,
}

impl Default for StandIns {
    /// The backport bot stays as it is, because the Backport rules have to recognise it in a recording.
    fn default() -> Self {
        let bot = CPYTHON_BACKPORT_BOT.to_owned();
        Self {
            by_login: BTreeMap::from([(bot.clone(), bot)]),
        }
    }
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
        // The bot's entry takes no number.
        let next = format!("user-{}", self.by_login.len().saturating_sub(1));
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
                    if let Value::String(text) = &mut *field
                        && PROSE_FIELDS.contains(&name.as_str())
                    {
                        *text = self.in_prose(text);
                    } else if let Value::String(text) = &mut *field
                        && name == "nameWithOwner"
                    {
                        *text = self.in_repository_name(text);
                    } else {
                        self.replace_in_strings(field);
                    }
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }

    /// `name` with an owner other than the recorded one replaced, because that owner is a person's login.
    fn in_repository_name(&mut self, name: &str) -> String {
        match name.split_once('/') {
            Some((owner, rest)) if !owner.eq_ignore_ascii_case(RECORDED_OWNER) => {
                format!("{}/{rest}", self.for_login(owner))
            }
            _ => name.to_owned(),
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
    /// Passes the request on and records nothing, because recordings hold GraphQL only (ADR 0009).
    fn get(
        &self,
        token: &Token,
        request: &ConditionalGet<'_>,
    ) -> Result<ConditionalReply, Unreachable> {
        self.inner.get(token, request)
    }

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
        keep_only_what_is_read(&mut body);
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
    fn a_list_check_passes_through_and_records_nothing() {
        let dir = temp_dir("list-check");
        let recorder = Recorder::new(Box::new(Canned(Ok(reply("{}")))), dir.clone());
        let asked = ConditionalGet {
            path: "/repos/python/cpython/issues",
            etag: None,
        };

        let answer = recorder.get(&token(), &asked);

        assert!(answer.is_err(), "`Canned` has no REST to pass on");
        assert!(!dir.join("Op.json").exists());
        drop(std::fs::remove_dir_all(dir));
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
    fn a_recording_keeps_a_descriptions_words_only_off_the_default_branch() {
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
        assert_eq!(bodies(1), "Lorem ip");
        assert_eq!(bodies(2), "Fixes #5");
        assert_eq!(bodies(3), "x");
        assert!(
            live.body.contains("Fixes #5"),
            "the caller still gets the real answer"
        );
    }

    /// The part of `node`, answered at `pointer`, that a recording keeps.
    fn recorded_pr(name: &str, answer: &Value) -> Value {
        let dir = temp_dir(name);
        let recorder = Recorder::new(
            Box::new(Canned(Ok(reply(&answer.to_string())))),
            dir.clone(),
        );
        drop(recorder.post(&token(), &REQUEST).unwrap());
        let replayed = Replayer::new(dir.clone()).post(&token(), &REQUEST).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        serde_json::from_str(&replayed.body).unwrap()
    }

    fn pr_off_default(body: &str, message: &str) -> Value {
        json!({
            "baseRefName": "3.14",
            "body": body,
            "repository": { "defaultBranchRef": { "name": "main" } },
            "commits": { "nodes": [
                { "commit": { "message": message, "committer": { "user": { "login": "miss-islington" } } } },
                null,
                { "commit": {} },
            ] },
        })
    }

    #[test]
    fn a_recording_keeps_a_read_description_without_the_lines_that_credit_people() {
        let body = "(cherry picked from commit 0123abc)\r\n\r\nco-authored-by: A Person <a@example.com>\r\n> Co-Authored-By: B <b@example.com>\r\nSigned-off-by: C <c@example.com>\r\nFixes #5\r\n";
        let found = recorded_pr(
            "co-authors",
            &json!({ "data": { "nodes": [pr_off_default(body, "")] } }),
        );
        assert_eq!(
            found["data"]["nodes"][0]["body"],
            "(cherry picked from commit 0123abc)\r\n\r\nFixes #5\r\n"
        );
    }

    #[test]
    fn a_recording_keeps_only_the_cherry_pick_lines_of_a_commit_message() {
        let message = "gh-1: Fix it\n\n(cherry picked from commit 0123abc)\n\nCo-authored-by: A Person <a@example.com>";
        let found = recorded_pr(
            "messages",
            &json!({ "data": { "nodes": [pr_off_default("", message)] } }),
        );
        let commits = &found["data"]["nodes"][0]["commits"]["nodes"];
        assert_eq!(
            commits[0]["commit"]["message"],
            "(cherry picked from commit 0123abc)\n"
        );
        assert_eq!(commits[1], Value::Null);
    }

    #[test]
    fn a_recording_treats_an_origin_fetched_by_number_like_any_pr() {
        let on_default = json!({
            "baseRefName": "main",
            "body": "Fixes #5",
            "repository": { "defaultBranchRef": { "name": "main" } },
        });
        let found = recorded_pr(
            "by-number",
            &json!({ "data": { "repository": { "pullRequest": on_default } } }),
        );
        assert_eq!(
            found["data"]["repository"]["pullRequest"]["body"],
            "Lorem ip"
        );
    }

    #[test]
    fn a_recording_with_no_pr_in_it_stays_as_it_was() {
        let answer = json!({ "data": { "repository": { "pullRequest": null } } });
        assert_eq!(recorded_pr("no-pr", &answer), answer);
    }

    #[test]
    fn a_recording_keeps_only_the_recorded_owner_in_a_repository_name() {
        let answer = json!({ "data": { "repository": { "issue": { "nodes": [
            { "source": { "repository": { "nameWithOwner": "python/cpython" } } },
            { "source": { "repository": { "nameWithOwner": "Python/peps" } } },
            { "source": { "repository": { "nameWithOwner": "some-person/cpython" } } },
            { "source": { "repository": { "nameWithOwner": "no-slash" } } },
        ] } } } });
        let found = recorded_pr("repositories", &answer);
        let named = |at: usize| {
            found["data"]["repository"]["issue"]["nodes"][at]["source"]["repository"]
                ["nameWithOwner"]
                .clone()
        };
        assert_eq!(named(0), "python/cpython");
        assert_eq!(named(1), "Python/peps");
        assert_eq!(named(2), "user-0/cpython");
        assert_eq!(named(3), "no-slash");
    }

    #[test]
    fn the_backport_bot_keeps_its_login_in_a_recording() {
        let answer = json!({ "data": { "nodes": [
            { "author": { "login": "Miss-Islington" }, "title": "by miss-islington" },
            { "author": { "login": "some-author" } },
        ] } });
        let found = recorded_pr("bot", &answer);
        let nodes = &found["data"]["nodes"];
        assert_eq!(nodes[0]["author"]["login"], "miss-islington");
        assert_eq!(nodes[0]["title"], "by miss-islington");
        assert_ne!(nodes[1]["author"]["login"], "some-author");
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
