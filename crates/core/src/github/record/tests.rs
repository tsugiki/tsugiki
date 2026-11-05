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
        found["data"]["repository"]["issue"]["nodes"][at]["source"]["repository"]["nameWithOwner"]
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
