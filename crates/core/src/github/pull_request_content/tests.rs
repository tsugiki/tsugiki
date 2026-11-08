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

/// A thread on line `line` of `Lib/ast.py` at the head, started by Review `review`, with `comments` in all
/// and its first one read.
fn thread(review: &str, line: u32, words: &str, comments: u32) -> Value {
    json!({
        "id": format!("T{line}"),
        "path": "Lib/ast.py",
        "line": line,
        "originalLine": line,
        "diffSide": "RIGHT",
        "subjectType": "LINE",
        "isResolved": false,
        "isOutdated": false,
        "comments": { "totalCount": comments, "nodes": [comment(review, words)] },
    })
}

/// A comment in a thread, as GitHub answers with it.
fn comment(review: &str, words: &str) -> Value {
    json!({
        "pullRequestReview": { "id": review },
        "author": { "login": "some-reviewer" },
        "createdAt": "2026-11-04T10:00:00Z",
        "bodyHTML": format!("<p>{words}</p>"),
        "bodyText": words,
        "isMinimized": false,
        "minimizedReason": null,
        "diffHunk": HUNK,
    })
}

/// The hunk GitHub stores with each comment in [`thread`]'s threads.
const HUNK: &str = "@@ -1,2 +1,2 @@\n x = 1\n-y = 2\n+y = 3";

/// The comment `comment` reads as, by `some-reviewer`.
fn read_comment(words: &str) -> ThreadComment {
    ThreadComment {
        comment: IssueComment {
            author: Some(Login::new("some-reviewer")),
            created_at: moment("2026-11-04T10:00:00Z"),
            body_html: format!("<p>{words}</p>"),
            hidden: None,
        },
        submission: Submission::Submitted,
    }
}

/// Tsugiki's read of a PR with these comments, Reviews and threads.
fn read(comments: Value, reviews: (u32, Value), threads: Value) -> PullRequestContent {
    let pr = json!({
        "bodyHTML": "<p>Fixes it</p>",
        "comments": { "totalCount": comments.as_array().unwrap().len(), "nodes": comments },
        "reviews": { "totalCount": reviews.0, "nodes": reviews.1 },
        "reviewThreads": { "nodes": threads },
        "headRefOid": "0123abcdef0123abcdef0123abcdef0123abcdef",
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
                id: ThreadId::new("T12"),
                path: "Lib/ast.py".to_owned(),
                opening: Opening::Words {
                    words: "Why not a set?".to_owned()
                },
                replies: 2,
                resolution: Resolution::Open,
                anchor: Anchor::Current,
                subject: Subject::Line {
                    side: Side::Right,
                    line: Some(12),
                    original: Some(12)
                },
                comments: vec![read_comment("Why not a set?\nIt reads better.")],
                hunk: DiffHunk::parse(HUNK),
            },
            ThreadLine {
                id: ThreadId::new("T40"),
                path: "Lib/ast.py".to_owned(),
                opening: Opening::Words {
                    words: "Typo".to_owned()
                },
                replies: 0,
                resolution: Resolution::Open,
                anchor: Anchor::Current,
                subject: Subject::Line {
                    side: Side::Right,
                    line: Some(40),
                    original: Some(40)
                },
                comments: vec![read_comment("Typo")],
                hunk: DiffHunk::parse(HUNK),
            },
        ]
    );
    assert!(content.reviews[1].threads.is_empty());
    assert_eq!(content.earlier_reviews, 0);
    assert_eq!(
        content.head.as_ref().map(CommitSha::as_str),
        Some("0123abcdef0123abcdef0123abcdef0123abcdef"),
        "the threads' lines count in this head"
    );
    assert_eq!(first.threads[0].later_comments(), 2);
    assert_eq!(first.threads[1].later_comments(), 0);
}

#[test]
fn a_thread_holds_its_replies_from_other_reviews_and_its_side() {
    let mut on_the_base = thread("R1", 5, "Why remove this?", 3);
    on_the_base["diffSide"] = json!("LEFT");
    on_the_base["comments"]["nodes"] = json!([
        comment("R1", "Why remove this?"),
        comment("R2", "It was dead code"),
        null,
    ]);
    on_the_base["comments"]["nodes"][1]["isMinimized"] = json!(true);
    on_the_base["comments"]["nodes"][1]["minimizedReason"] = json!("resolved");

    let content = read(
        json!([]),
        (
            1,
            json!([review("R1", "COMMENTED", "2026-11-04T10:00:00Z", "")]),
        ),
        json!([on_the_base]),
    );

    let thread = &content.reviews[0].threads[0];
    assert_eq!(
        thread.subject,
        Subject::Line {
            side: Side::Left,
            line: Some(5),
            original: Some(5)
        }
    );
    let mut reply = read_comment("It was dead code");
    reply.comment.hidden = Some(HiddenReason::Resolved);
    assert_eq!(
        thread.comments,
        [read_comment("Why remove this?"), reply],
        "a reply belongs to the thread, not to the Review that wrote it"
    );
    assert_eq!(thread.later_comments(), 1);
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
        (threads[0].subject, threads[0].anchor, threads[0].resolution),
        (
            Subject::Line {
                side: Side::Right,
                line: None,
                original: Some(7)
            },
            Anchor::Outdated,
            Resolution::Resolved
        ),
        "an Outdated thread keeps the line it began on, and Files can't place it by its line"
    );
    assert_eq!(threads[1].subject, Subject::File);
    assert_eq!(
        (threads[0].hunk.as_ref(), threads[1].hunk.as_ref()),
        (DiffHunk::parse(HUNK).as_ref(), None),
        "an Outdated thread keeps the code it was written against, and a thread on a file has none"
    );
}

#[test]
fn the_users_pending_review_keeps_its_threads_and_its_drafts_in_others_threads() {
    let mut pending = review("R2", "PENDING", "", "");
    pending["submittedAt"] = Value::Null;
    let mut answered = thread("R1", 3, "Why?", 2);
    answered["comments"]["nodes"] = json!([comment("R1", "Why?"), comment("R2", "Because")]);

    let content = read(
        json!([]),
        (
            2,
            json!([
                review("R1", "COMMENTED", "2026-11-04T10:00:00Z", ""),
                pending
            ]),
        ),
        json!([thread("R2", 9, "A draft", 1), answered]),
    );

    let states: Vec<ReviewState> = content.reviews.iter().map(|review| review.state).collect();
    assert_eq!(
        states,
        [ReviewState::Commented],
        "only the user sees a Pending review"
    );
    let drafted: Vec<(&str, Submission)> = content
        .pending
        .iter()
        .flat_map(|thread| &thread.comments)
        .map(|comment| (comment.comment.body_html.as_str(), comment.submission))
        .collect();
    assert_eq!(drafted, [("<p>A draft</p>", Submission::Pending)]);
    let answers: Vec<Submission> = content.reviews[0].threads[0]
        .comments
        .iter()
        .map(|comment| comment.submission)
        .collect();
    assert_eq!(
        answers,
        [Submission::Submitted, Submission::Pending],
        "a draft reply sits in the thread it answers"
    );
    assert_eq!(
        content.reviews[0].threads[0].replies, 0,
        "the Conversation counts no draft among the replies"
    );
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
/// last 100 threads with their first 100 comments each are about 100 requests, and the rest a few more.
#[test]
fn a_conversation_asks_for_pages_that_cost_about_a_point() {
    let query = Thread::PullRequest.query();

    for page in [
        "comments(last: 30)",
        "reviews(last: 100)",
        "reviewThreads(last: 100)",
        "comments(first: 100)",
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
            let placed = match thread.subject {
                Subject::File => "file".to_owned(),
                Subject::Line {
                    side,
                    line,
                    original,
                } => format!("{side:?} {line:?}, began at {original:?}"),
            };
            writeln!(
                outline,
                "  {} {opening:?} +{} resolved={} outdated={} at {placed}, {} read",
                thread.path,
                thread.replies,
                thread.resolution == Resolution::Resolved,
                thread.anchor == Anchor::Outdated,
                thread.comments.len()
            )
            .unwrap();
        }
    }
    insta::assert_snapshot!(outline);
}

#[test]
fn a_hunk_without_its_start_has_no_rows_and_an_empty_line_is_context() {
    assert_eq!(DiffHunk::parse("@@ nothing here @@\n x"), None);
    assert_eq!(DiffHunk::parse(""), None);
    let rows = DiffHunk::parse("@@ -1,2 +1,2 @@\n\n+y").map(|hunk| hunk.rows().to_vec());
    assert_eq!(
        rows,
        Some(vec![
            CodeRow::Hunk {
                text: "@@ -1,2 +1,2 @@".to_owned()
            },
            CodeRow::Context {
                old: 1,
                new: 1,
                text: String::new()
            },
            CodeRow::Added {
                new: 2,
                text: "y".to_owned()
            },
        ])
    );
}
