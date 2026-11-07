use std::fmt::Write as _;
use std::path::Path;

use serde_json::{Value, json};
use time::format_description::well_known::Rfc3339;

use super::*;
use crate::error::ErrorKind;
use crate::github::replay::Replayer;
use crate::github::testing::{Scripted, reply_ok, token};

fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

fn moment(text: &str) -> OffsetDateTime {
    OffsetDateTime::parse(text, &Rfc3339).unwrap()
}

/// A Review as GitHub answers with it.
fn review(id: &str, state: &str, at: &str, body_html: &str) -> Value {
    json!({
        "id": id,
        "author": { "login": "some-reviewer" },
        "state": state,
        "submittedAt": at,
        "bodyHTML": body_html,
        "isMinimized": false,
        "minimizedReason": null,
        "commit": { "oid": "0123abcdef0123abcdef0123abcdef0123abcdef" },
    })
}

/// A thread on line `line` of `Lib/ast.py`, started by Review `review`.
fn thread(review: &str, line: u32, words: &str, comments: u32) -> Value {
    json!({
        "path": "Lib/ast.py",
        "line": line,
        "originalLine": line,
        "subjectType": "LINE",
        "isResolved": false,
        "isOutdated": false,
        "comments": { "totalCount": comments, "nodes": [{
            "pullRequestReview": { "id": review },
            "bodyText": words,
            "isMinimized": false,
            "minimizedReason": null,
        }] },
    })
}

/// Tsugiki's read of a PR with these comments, Reviews and threads.
fn read(comments: Value, reviews: (u32, Value), threads: Value) -> PullRequestContent {
    let pr = json!({
        "bodyHTML": "<p>Fixes it</p>",
        "comments": { "totalCount": comments.as_array().unwrap().len(), "nodes": comments },
        "reviews": { "totalCount": reviews.0, "nodes": reviews.1 },
        "reviewThreads": { "nodes": threads },
    });
    let body =
        json!({ "data": { "repository": { "pullRequest": pr }, "rateLimit": rate_limit() } })
            .to_string();
    let (script, _asked) = Scripted::new(vec![reply_ok(&body)]);
    GitHub::new(Box::new(script), token())
        .pull_request_content(&cpython(), 7)
        .unwrap()
        .content
}

/// GitHub's count of the points left, with plenty of them.
fn rate_limit() -> Value {
    json!({ "remaining": 4000, "resetAt": "2999-01-01T00:00:00Z" })
}

#[test]
fn reviews_read_oldest_first_each_with_the_threads_it_started() {
    let content = read(
        json!([]),
        (
            2,
            json!([
                review("R2", "APPROVED", "2026-11-04T12:00:00Z", "<p>Thanks</p>"),
                review(
                    "R1",
                    "CHANGES_REQUESTED",
                    "2026-11-04T10:00:00Z",
                    "<p>Some notes</p>"
                ),
            ]),
        ),
        json!([
            thread("R1", 12, "Why not a set?\nIt reads better.", 3),
            null,
            thread("R1", 40, "Typo", 1),
        ]),
    );

    let states: Vec<ReviewState> = content.reviews.iter().map(|review| review.state).collect();
    assert_eq!(
        states,
        [ReviewState::ChangesRequested, ReviewState::Approved]
    );
    let first = &content.reviews[0];
    assert_eq!(first.author, Some(Login::new("some-reviewer")));
    assert_eq!(first.submitted_at, moment("2026-11-04T10:00:00Z"));
    assert_eq!(
        first.commit.as_ref().map(CommitSha::as_str),
        Some("0123abcdef0123abcdef0123abcdef0123abcdef")
    );
    assert_eq!(
        first.threads,
        [
            ThreadLine {
                path: "Lib/ast.py".to_owned(),
                line: Some(12),
                opening: Opening::Words {
                    words: "Why not a set?".to_owned()
                },
                replies: 2,
                resolution: Resolution::Open,
                anchor: Anchor::Current,
            },
            ThreadLine {
                path: "Lib/ast.py".to_owned(),
                line: Some(40),
                opening: Opening::Words {
                    words: "Typo".to_owned()
                },
                replies: 0,
                resolution: Resolution::Open,
                anchor: Anchor::Current,
            },
        ]
    );
    assert!(content.reviews[1].threads.is_empty());
    assert_eq!(content.earlier_reviews, 0);
}

#[test]
fn a_bare_approval_stays_and_a_review_that_only_replied_goes() {
    let content = read(
        json!([]),
        (
            5,
            json!([
                review("R1", "APPROVED", "2026-11-04T10:00:00Z", ""),
                review("R2", "COMMENTED", "2026-11-04T11:00:00Z", ""),
                review("R3", "PENDING", "2026-11-04T12:00:00Z", "<p>Draft</p>"),
            ]),
        ),
        json!([]),
    );

    let states: Vec<ReviewState> = content.reviews.iter().map(|review| review.state).collect();
    assert_eq!(states, [ReviewState::Approved]);
    assert_eq!(content.earlier_reviews, 2, "5 Reviews, 3 of them read");
}

#[test]
fn a_hidden_comment_review_or_thread_says_why() {
    let mut hidden_review = review("R1", "COMMENTED", "2026-11-04T10:00:00Z", "<p>Spam</p>");
    hidden_review["isMinimized"] = json!(true);
    hidden_review["minimizedReason"] = json!("spam");
    let mut hidden_thread = thread("R1", 3, "Buy now", 1);
    hidden_thread["comments"]["nodes"][0]["isMinimized"] = json!(true);
    hidden_thread["comments"]["nodes"][0]["minimizedReason"] = json!("off-topic");
    let comments = json!([
        { "author": null, "createdAt": "2026-11-04T09:00:00Z", "bodyHTML": "<p>Old</p>",
          "isMinimized": true, "minimizedReason": "OUTDATED" },
        { "author": null, "createdAt": "2026-11-04T09:30:00Z", "bodyHTML": "<p>Hm</p>",
          "isMinimized": true, "minimizedReason": null },
        { "author": null, "createdAt": "2026-11-04T09:40:00Z", "bodyHTML": "<p>Fine</p>",
          "isMinimized": false, "minimizedReason": null },
    ]);

    let content = read(
        comments,
        (1, json!([hidden_review])),
        json!([hidden_thread]),
    );

    let hidden: Vec<Option<HiddenReason>> = content
        .conversation
        .comments
        .iter()
        .map(|comment| comment.hidden)
        .collect();
    assert_eq!(
        hidden,
        [
            Some(HiddenReason::Outdated),
            Some(HiddenReason::Unstated),
            None
        ]
    );
    assert_eq!(content.reviews[0].hidden, Some(HiddenReason::Spam));
    assert_eq!(
        content.reviews[0].threads[0].opening,
        Opening::Hidden {
            reason: HiddenReason::OffTopic
        }
    );
}

#[test]
fn an_outdated_thread_keeps_its_old_line_and_a_thread_on_a_file_has_none() {
    let mut outdated = thread("R1", 0, "Gone now", 1);
    outdated["line"] = Value::Null;
    outdated["originalLine"] = json!(7);
    outdated["isOutdated"] = json!(true);
    outdated["isResolved"] = json!(true);
    let mut on_the_file = thread("R1", 0, "Rename this file", 1);
    on_the_file["subjectType"] = json!("FILE");
    on_the_file["line"] = json!(1);
    let mut on_an_earlier_review = thread("R0", 3, "Earlier", 1);
    on_an_earlier_review["path"] = json!("Lib/old.py");
    let started = review("R1", "COMMENTED", "2026-11-04T10:00:00Z", "");

    let content = read(
        json!([]),
        (1, json!([started])),
        json!([outdated, on_the_file, on_an_earlier_review]),
    );

    let threads = &content.reviews[0].threads;
    assert_eq!(
        threads.len(),
        2,
        "a thread whose Review wasn't read has nowhere to go"
    );
    assert_eq!(
        (threads[0].line, threads[0].anchor, threads[0].resolution),
        (Some(7), Anchor::Outdated, Resolution::Resolved)
    );
    assert_eq!(threads[1].line, None);
}

#[test]
fn the_first_words_are_cut_to_fit_a_line() {
    let long = "word ".repeat(40);
    let content = read(
        json!([]),
        (
            1,
            json!([review("R1", "COMMENTED", "2026-11-04T10:00:00Z", "")]),
        ),
        json!([thread("R1", 1, &format!("\n  \n{long}"), 1)]),
    );

    let Opening::Words { words } = &content.reviews[0].threads[0].opening else {
        panic!("the thread's first comment isn't hidden");
    };
    assert_eq!(words.chars().count(), FIRST_WORDS);
    assert!(words.starts_with("word word"), "{words}");
}

#[test]
fn a_pr_github_doesnt_have_is_not_found() {
    let body =
        json!({ "data": { "repository": { "pullRequest": null }, "rateLimit": rate_limit() } })
            .to_string();
    let (script, asked) = Scripted::new(vec![reply_ok(&body)]);
    let github = GitHub::new(Box::new(script), token());

    let problem = github.pull_request_content(&cpython(), 7).unwrap_err();

    assert_eq!(problem.kind, ErrorKind::NotFound);
    assert!(problem.message.contains("PR #7"), "{}", problem.message);
    assert_eq!(asked.recv().unwrap().0, "FetchPullRequestContent");
}

/// Reading a Conversation ahead costs a point a PR: GitHub counts a request per connection it walks, so the
/// last 100 threads with a first comment each are about 100 requests, and the rest a few more.
#[test]
fn a_conversation_asks_for_pages_that_cost_about_a_point() {
    let query = Thread::PullRequest.query();

    for page in [
        "comments(last: 30)",
        "reviews(last: 100)",
        "reviewThreads(last: 100)",
        "comments(first: 1)",
    ] {
        assert!(query.contains(page), "{query}");
    }
    assert_eq!(
        query.matches("(last: ").count() + query.matches("(first: ").count(),
        4,
        "{query}"
    );
}

/// A recorded CPython PR, as the Conversation lists it: each Review, then a line for each of its threads.
#[test]
fn a_recorded_pr_reads_as_its_reviews_and_their_threads() {
    let recordings = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github");
    let github = GitHub::new(Box::new(Replayer::new(recordings)), token());

    let content = github
        .pull_request_content(&cpython(), 152_326)
        .unwrap()
        .content;

    let mut outline = format!(
        "{} comments, {} earlier; {} earlier Reviews\n",
        content.conversation.comments.len(),
        content.conversation.earlier_comments,
        content.earlier_reviews
    );
    for review in &content.reviews {
        let commit = review.commit.as_ref().and_then(|sha| sha.as_str().get(..7));
        let said = if review.body_html.is_empty() {
            ""
        } else {
            " with a body"
        };
        let author = review.author.as_ref().map(Login::as_str);
        writeln!(outline, "{author:?} {:?} at {commit:?}{said}", review.state).unwrap();
        for thread in &review.threads {
            let opening = match &thread.opening {
                Opening::Words { words } => words.chars().take(30).collect::<String>(),
                Opening::Hidden { reason } => format!("hidden as {reason:?}"),
            };
            writeln!(
                outline,
                "  {}:{:?} {opening:?} +{} resolved={} outdated={}",
                thread.path,
                thread.line,
                thread.replies,
                thread.resolution == Resolution::Resolved,
                thread.anchor == Anchor::Outdated
            )
            .unwrap();
        }
    }
    insta::assert_snapshot!(outline);
}
