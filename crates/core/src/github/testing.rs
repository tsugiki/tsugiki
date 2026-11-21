//! The fake GitHub that the core's tests talk to, and the answers they give it. Other crates' tests reach
//! it through the core's `testing` feature.

#![expect(clippy::unwrap_used, reason = "a failed setup step ends the test")]
#![expect(
    clippy::indexing_slicing,
    reason = "setting a field of a JSON object never panics"
)]

use tsugiki_token::testing::{gh_alone, gh_prints};

use super::*;
use crate::token::GhOutput;

/// The points field of an answer's data: one point of 1,000 spent, with plenty left, so nothing pauses, in an
/// hour that hasn't reset.
pub const RATE_LIMIT: &str = r#""rateLimit":{"cost":1,"limit":5000,"used":1000,"remaining":4000,
    "resetAt":"2999-01-01T00:00:00Z"}"#;

/// [`RATE_LIMIT`] as a value, for an answer built with `json!`.
pub fn rate_limit() -> Value {
    points(RATE_LIMIT)
}

/// The value of `rate`, a points field such as [`RATE_LIMIT`].
fn points(rate: &str) -> Value {
    let data: Value = serde_json::from_str(&format!("{{{rate}}}")).unwrap();
    data["rateLimit"].clone()
}

/// A client of GitHub that sends each request through `transport`.
pub fn github(transport: impl Transport + 'static) -> GitHub {
    GitHub::new(Box::new(transport), token())
}

/// Answers every request the same way: the small builder ADR 0009 allows for rare cases.
pub struct Canned(pub Result<Reply, Unreachable>);

impl Transport for Canned {
    fn post(&self, _: &Token, _: &Request<'_>) -> Result<Reply, Unreachable> {
        self.0.clone()
    }
}

pub fn answered(status: u16, body: &str) -> Canned {
    Canned(replied(status, body))
}

/// Answers from a list, in order, and records each operation and its variables for the test.
pub struct Scripted {
    replies: std::cell::RefCell<std::collections::VecDeque<Result<Reply, Unreachable>>>,
    asked: std::sync::mpsc::Sender<(String, Value)>,
}

impl Scripted {
    pub fn new(
        replies: Vec<Result<Reply, Unreachable>>,
    ) -> (Self, std::sync::mpsc::Receiver<(String, Value)>) {
        let (asked, seen) = std::sync::mpsc::channel();
        let replies = std::cell::RefCell::new(replies.into());
        (Self { replies, asked }, seen)
    }
}

impl Transport for Scripted {
    fn post(&self, _: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "a test that has dropped the receiver doesn't check what was asked"
        )]
        let _ = (self.asked).send((request.operation.to_owned(), request.variables.clone()));
        self.replies
            .borrow_mut()
            .pop_front()
            .unwrap_or(Err(Unreachable::Offline))
    }
}

/// GitHub's reply with `status` and `body`, and no time or reset.
pub fn replied(status: u16, body: &str) -> Result<Reply, Unreachable> {
    Ok(Reply {
        status,
        reset: None,
        date: None,
        body: body.to_owned(),
    })
}

pub fn reply_ok(body: &str) -> Result<Reply, Unreachable> {
    replied(200, body)
}

/// GitHub's answer whose data holds the fields of `data`, a JSON object, and `rate`, a points field such as
/// [`RATE_LIMIT`].
pub fn answer(data: Value, rate: &str) -> Result<Reply, Unreachable> {
    reply_ok(&with_points(data, rate).to_string())
}

/// GitHub's answer whose data holds the fields of `data` and [`RATE_LIMIT`], and that says the thing at
/// `path` in it doesn't exist, as GitHub says of a number that names nothing.
pub fn not_found(data: Value, path: Value) -> Result<Reply, Unreachable> {
    let mut body = with_points(data, RATE_LIMIT);
    body["errors"] = json!([{ "type": "NOT_FOUND", "message": "gone", "path": path }]);
    reply_ok(&body.to_string())
}

fn with_points(mut data: Value, rate: &str) -> Value {
    data["rateLimit"] = points(rate);
    json!({ "data": data })
}

/// The `pageInfo` of a page whose next page starts at the cursor `next`, or of the last page.
pub fn page_info(next: Option<&str>) -> Value {
    json!({ "hasNextPage": next.is_some(), "endCursor": next })
}

/// A page of the search for the user's work, which found the PRs `ids`.
pub fn search_page(ids: &[&str], next: Option<&str>, rate: &str) -> Result<Reply, Unreachable> {
    let nodes: Vec<Value> = ids.iter().map(|id| json!({ "id": id })).collect();
    answer(
        json!({ "search": { "pageInfo": page_info(next), "nodes": nodes } }),
        rate,
    )
}

/// A page of a Filter's search, which found `nodes` of `total` items.
pub fn filter_page(
    total: usize,
    nodes: &[Value],
    next: Option<&str>,
    rate: &str,
) -> Result<Reply, Unreachable> {
    let search = json!({ "issueCount": total, "pageInfo": page_info(next), "nodes": nodes });
    answer(json!({ "search": search }), rate)
}

/// GitHub's answer to a fetch of `nodes` by their ids.
pub fn fetched_nodes(nodes: &[Value], rate: &str) -> Result<Reply, Unreachable> {
    answer(json!({ "nodes": nodes }), rate)
}

/// PR `number` of `python/cpython` as a fetch answers with it, with `changes` on top.
pub fn pr_node(number: u32, changes: &Value) -> Value {
    let node = json!({
        "number": number,
        "title": format!("gh-1: Fix it {number}"),
        "state": "OPEN",
        "isDraft": false,
        "updatedAt": "2026-11-04T10:00:00Z",
        "baseRefName": "main",
        "body": "",
        "repository": {
            "nameWithOwner": "python/cpython",
            "defaultBranchRef": { "name": "main" },
        },
        "closingIssuesReferences": { "nodes": [] },
        "author": { "login": "octocat" },
        "assignees": { "nodes": [] },
        "labels": { "nodes": [] },
        "reviewRequests": { "nodes": [] },
        "latestReviews": { "nodes": [] },
        "lastCommit": { "nodes": [] },
        "mergeCommit": null,
        "baseRefOid": "0123456789abcdef0123456789abcdef01234567",
        "headRefOid": "89abcdef0123456789abcdef0123456789abcdef",
        "commits": { "totalCount": 0, "nodes": [] },
        "timelineItems": { "nodes": [] },
    });
    changed(node, changes)
}

/// Open issue `number`, with no labels, assignees or events, as a fetch of issues answers with it.
pub fn issue_node(number: u32) -> Value {
    json!({
        "__typename": "Issue",
        "number": number,
        "title": format!("Issue {number}"),
        "updatedAt": "2026-11-04T10:00:00Z",
        "state": "OPEN",
        "stateReason": null,
        "assignees": { "nodes": [] },
        "labels": { "nodes": [] },
        "timelineItems": { "nodes": [] },
        "author": { "login": "some-author" },
    })
}

/// A PR's Conversation as `FetchPullRequestContent` answers with it, a body and nothing else, with `changes`
/// on top.
pub fn pr_content(changes: &Value) -> Value {
    let pull_request = json!({
        "bodyHTML": "<p>Fixes the crash</p>",
        "createdAt": "2026-11-01T09:00:00Z",
        "comments": { "totalCount": 0, "nodes": [] },
        "reviews": { "totalCount": 0, "nodes": [] },
        "reviewThreads": { "nodes": [] },
        "updatedAt": "2026-11-04T10:00:00Z",
        "headRefOid": "0123abc",
    });
    changed(pull_request, changes)
}

/// GitHub's answer to a question about one PR, such as `FetchPullRequestContent`, with the PR
/// `pull_request`.
pub fn pull_request_answer(pull_request: Value, rate: &str) -> Result<Reply, Unreachable> {
    answer(
        json!({ "repository": { "pullRequest": pull_request } }),
        rate,
    )
}

/// `node` with each field of `changes` set on it.
fn changed(mut node: Value, changes: &Value) -> Value {
    for (name, value) in changes.as_object().unwrap() {
        node[name] = value.clone();
    }
    node
}

/// `reply`, which GitHub dated `date`.
pub fn dated(
    reply: Result<Reply, Unreachable>,
    date: OffsetDateTime,
) -> Result<Reply, Unreachable> {
    reply.map(|reply| Reply {
        date: Some(date),
        ..reply
    })
}

/// GitHub's answer that `login` is signed in.
pub fn signed_in_as(login: &str) -> Result<Reply, Unreachable> {
    reply_ok(&format!(r#"{{"data":{{"viewer":{{"login":"{login}"}}}}}}"#))
}

/// gh, signed in.
fn signed_in_gh(_: &[&str]) -> std::io::Result<GhOutput> {
    gh_prints("gho_example")
}

/// A machine with no `GH_TOKEN` and gh signed in.
pub const SIGNED_IN: TokenFrom = gh_alone(signed_in_gh);

pub fn token() -> Token {
    crate::token::read_token(SIGNED_IN).unwrap()
}
