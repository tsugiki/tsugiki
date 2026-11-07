//! Test helpers for the sync: a harness and GitHub's answers.

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::{Progress, Syncs};
use crate::github::testing::{Scripted, reply_ok, token};
use crate::github::{GitHub, Reply, Request, Transport, Unreachable};
use crate::repository::{RepositoryCounts, RepositoryName, RepositorySettings};
use crate::store;
use crate::store::{OpenReader, Store, StoreReader};
use crate::token::Token;

pub(super) const LOTS: &str = r#""rateLimit":{"remaining":4000,"resetAt":"2026-11-04T12:00:00Z"}"#;

pub(super) const FEW: &str = r#""rateLimit":{"remaining":10,"resetAt":"2999-01-01T00:00:00Z"}"#;

pub(super) fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

/// A Repository with no Filters, for the sync's other phases.
pub(super) fn plain(name: RepositoryName) -> RepositorySettings {
    RepositorySettings {
        filters: Vec::new(),
        ..RepositorySettings::new(name)
    }
}

pub(super) fn found(ids: &[&str], next: Option<&str>, rate: &str) -> Result<Reply, Unreachable> {
    let nodes: Vec<Value> = ids.iter().map(|id| json!({ "id": id })).collect();
    let info = json!({ "hasNextPage": next.is_some(), "endCursor": next });
    reply_ok(&format!(
        r#"{{"data":{{"search":{{"pageInfo":{info},"nodes":{}}},{rate}}}}}"#,
        json!(nodes)
    ))
}

pub(super) fn fetched(numbers: &[u32], rate: &str) -> Result<Reply, Unreachable> {
    fetched_titled(numbers, rate, |number| format!("PR {number}"))
}

pub(super) fn fetched_titled(
    numbers: &[u32],
    rate: &str,
    title: impl Fn(u32) -> String,
) -> Result<Reply, Unreachable> {
    let nodes: Vec<Value> = numbers
        .iter()
        .map(|number| {
            json!({
                "number": number,
                "title": title(*number),
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
                "reviewRequests": { "nodes": [] },
                "timelineItems": { "nodes": [] },
                "latestReviews": { "nodes": [] },
                "lastCommit": { "nodes": [] },
                "mergeCommit": null,
                "headRefOid": "89abcdef0123456789abcdef0123456789abcdef",
                "commits": { "totalCount": 0, "nodes": [] },
            })
        })
        .collect();
    reply_ok(&format!(
        r#"{{"data":{{"nodes":{},{rate}}}}}"#,
        json!(nodes)
    ))
}

/// Answers from `Scripted`, except that a first sync's window is empty and goes unrecorded. Most tests are
/// about something else, and `Harness::with_window` is for those that aren't.
struct EmptyWindow(Scripted);

impl Transport for EmptyWindow {
    fn post(&self, token: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        if request.operation.starts_with("Window") {
            return empty_window_page();
        }
        if request.operation == "CheckRepository" {
            return counted(LARGE);
        }
        self.0.post(token, request)
    }
}

/// More open Issues and PRs than a small Repository has.
pub(super) const LARGE: u32 = 501;

/// The add dialog's count of a Repository with `open` open Issues and no open PR, which a first sync asks for
/// before its lists.
pub(super) fn counted(open: u32) -> Result<Reply, Unreachable> {
    counted_as(RepositoryCounts {
        open_issues: open,
        open_prs: 0,
        updated_in_window: 0,
        closed_in_window: 0,
    })
}

/// The add dialog's count of a Repository, as GitHub answers it.
pub(super) fn counted_as(counts: RepositoryCounts) -> Result<Reply, Unreachable> {
    let body = json!({ "data": {
        "repository": {
            "nameWithOwner": "python/cpython",
            "viewerPermission": "READ",
            "issues": { "totalCount": counts.open_issues },
            "pullRequests": { "totalCount": counts.open_prs },
        },
        "updated": { "issueCount": counts.updated_in_window },
        "closed": { "issueCount": counts.closed_in_window },
    } });
    reply_ok(&body.to_string())
}

/// Answers a first sync's count with `open` open items, and passes each other request to `inner`, which
/// keeps its own record of what's asked.
struct Counted {
    open: u32,
    inner: Scripted,
}

impl Transport for Counted {
    fn post(&self, token: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        if request.operation == "CheckRepository" {
            return counted(self.open);
        }
        self.inner.post(token, request)
    }
}

/// A page of a first sync's window with nothing on it.
pub(super) fn empty_window_page() -> Result<Reply, Unreachable> {
    window_page(&[], None, LOTS)
}

/// The time `hours` ago as GitHub writes it, for an item inside a window that moves with the clock.
pub(super) fn hours_ago(hours: i64) -> String {
    (OffsetDateTime::now_utc() - time::Duration::hours(hours))
        .format(&Rfc3339)
        .unwrap()
}

/// An Issue as a page of a first sync's window lists it.
pub(super) fn window_issue(number: u32, updated_at: &str) -> Value {
    window_issue_referencing(number, updated_at, &[])
}

/// An Issue as a page of a first sync's window lists it, with these PRs as its cross-references.
pub(super) fn window_issue_referencing(number: u32, updated_at: &str, prs: &[u32]) -> Value {
    json!({
        "number": number,
        "title": format!("Issue {number}"),
        "updatedAt": updated_at,
        "assignees": { "nodes": [] },
        "timelineItems": { "nodes": [] },
        "author": { "login": "octocat" },
        "references": reference_timeline(prs, None),
    })
}

/// A timeline item: `login` commenting at `at`.
pub(super) fn comment_by(login: &str, at: &str) -> Value {
    json!({
        "__typename": "IssueComment",
        "createdAt": at,
        "author": { "__typename": "User", "login": login },
        "bodyText": "Looks good",
    })
}

/// A PR as a page of a first sync's window lists it.
pub(super) fn window_pr(number: u32, updated_at: &str) -> Value {
    json!({ "id": format!("id{number}"), "number": number, "updatedAt": updated_at })
}

/// A page of a first sync's window, holding `nodes`, of either list.
pub(super) fn window_page(
    nodes: &[Value],
    next: Option<&str>,
    rate: &str,
) -> Result<Reply, Unreachable> {
    let info = json!({ "hasNextPage": next.is_some(), "endCursor": next });
    let total = nodes.len();
    reply_ok(&format!(
        r#"{{"data":{{"repository":{{"list":{{"totalCount":{total},"pageInfo":{info},"nodes":{}}}}},{rate}}}}}"#,
        json!(nodes)
    ))
}

pub(super) struct Harness {
    pub(super) github: GitHub,
    pub(super) store: Store,
    pub(super) open: OpenReader,
    pub(super) reader: StoreReader,
    pub(super) asked: Receiver<(String, Value)>,
    pub(super) folder: PathBuf,
}

impl Harness {
    pub(super) fn new(name: &str, replies: Vec<Result<Reply, Unreachable>>) -> Self {
        let folder = crate::testing::temp_path(&format!("sync-{name}"));
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("tsugiki.db");
        let store = Store::open(&path).unwrap();
        let (to_open, reader) = store::reader();
        let open = to_open.open(&path).unwrap();
        let (script, asked) = Scripted::new(replies);
        let github = GitHub::new(Box::new(EmptyWindow(script)), token());
        Self {
            github,
            store,
            open,
            reader,
            asked,
            folder,
        }
    }

    /// A harness whose replies include the first sync's window, for a large Repository.
    pub(super) fn with_window(name: &str, replies: Vec<Result<Reply, Unreachable>>) -> Self {
        let (inner, asked) = Scripted::new(replies);
        let mut harness = Self::answering(name, Box::new(Counted { open: LARGE, inner }));
        harness.asked = asked;
        harness
    }

    /// A harness whose replies answer every request, the count included.
    pub(super) fn scripted(name: &str, replies: Vec<Result<Reply, Unreachable>>) -> Self {
        let (script, asked) = Scripted::new(replies);
        let mut harness = Self::answering(name, Box::new(script));
        harness.asked = asked;
        harness
    }

    /// A harness whose GitHub is `transport`, which keeps its own record of what's asked.
    pub(super) fn answering(name: &str, transport: Box<dyn Transport>) -> Self {
        let mut harness = Self::new(name, Vec::new());
        harness.github = GitHub::new(transport, token());
        harness
    }

    pub(super) fn numbers(&self) -> Vec<u32> {
        let facts = self.reader.work_facts(&cpython()).unwrap();
        let mut numbers: Vec<u32> = facts.pull_requests.iter().map(|pr| pr.number).collect();
        numbers.sort_unstable();
        numbers
    }

    pub(super) fn operations(&self) -> Vec<String> {
        self.asked
            .try_iter()
            .map(|(operation, _)| operation)
            .collect()
    }

    /// Makes the next sync a whole one, as a first sync that was interrupted leaves it.
    pub(super) fn forget_the_last_sync(&self) {
        self.store.forget_sync_state(&cpython()).unwrap();
    }

    pub(super) fn finish(self) {
        self.open.close();
        self.store.close();
        fs::remove_dir_all(self.folder).unwrap();
    }
}

/// Runs every step that's left, and says which Repositories it announced.
pub(super) fn run_to_the_end(syncs: &mut Syncs, harness: &Harness) -> Vec<RepositoryName> {
    let mut changed = Vec::new();
    while syncs.ready_at().is_some() {
        changed.extend(announced(
            syncs.step(&harness.github, &harness.store).unwrap(),
        ));
    }
    changed
}

/// The Repository a step says changed, if it says one did.
pub(super) fn announced(progress: Option<Progress>) -> Option<RepositoryName> {
    match progress? {
        Progress::Changed(change) => Some(change.repository),
        Progress::Finished(_) => None,
    }
}

pub(super) fn nothing(rate: &str) -> Result<Reply, Unreachable> {
    found(&[], None, rate)
}

pub(super) fn issues(numbers: &[u32], rate: &str) -> Result<Reply, Unreachable> {
    let fields: serde_json::Map<String, Value> = numbers
        .iter()
        .map(|number| {
            let issue = json!({
                "__typename": "Issue",
                "number": number,
                "title": format!("Issue {number}"),
                "updatedAt": "2026-11-04T10:00:00Z",
                "assignees": { "nodes": [] },
                "timelineItems": { "nodes": [] },
                "author": { "login": "some-author" },
            });
            (format!("i{number}"), issue)
        })
        .collect();
    reply_ok(&format!(
        r#"{{"data":{{"repository":{},{rate}}}}}"#,
        Value::Object(fields)
    ))
}

pub(super) fn no_cross_references(rate: &str) -> Result<Reply, Unreachable> {
    cross_references(&[], None, rate)
}

/// A page of an Issue's cross-references that lists these PRs as `id<number>`.
pub(super) fn cross_references(
    numbers: &[u32],
    older: Option<&str>,
    rate: &str,
) -> Result<Reply, Unreachable> {
    let issue = json!({ "timelineItems": reference_timeline(numbers, older) });
    reply_ok(&format!(
        r#"{{"data":{{"repository":{{"issue":{issue}}},{rate}}}}}"#
    ))
}

/// One page of an Issue's cross-references that lists these PRs as `id<number>`.
fn reference_timeline(numbers: &[u32], older: Option<&str>) -> Value {
    let nodes: Vec<Value> = numbers
        .iter()
        .map(|number| {
            json!({ "source": {
                    "__typename": "PullRequest",
                    "id": format!("id{number}"),
                    "number": number,
                    "updatedAt": hours_ago(1),
                    "repository": { "nameWithOwner": "python/cpython" },
                } })
        })
        .collect();
    json!({
        "pageInfo": { "hasPreviousPage": older.is_some(), "startCursor": older },
        "nodes": nodes,
    })
}

/// An answer to a read of Issues by number, each with the PRs that list beside it as cross-references.
pub(super) fn issue_references(issues: &[(u32, &[u32])], rate: &str) -> Result<Reply, Unreachable> {
    let mut fields = serde_json::Map::new();
    for (number, prs) in issues {
        drop(fields.insert(
            format!("i{number}"),
            json!({
                "__typename": "Issue",
                "number": number,
                "timelineItems": reference_timeline(prs, None),
            }),
        ));
    }
    reply_ok(&format!(
        r#"{{"data":{{"repository":{},{rate}}}}}"#,
        Value::Object(fields)
    ))
}

/// An answer to a read of one Issue by number whose cross-references go on past this page.
pub(super) fn issue_references_with_more(
    number: u32,
    prs: &[u32],
    older: &str,
    rate: &str,
) -> Result<Reply, Unreachable> {
    let field = json!({
        "__typename": "Issue",
        "number": number,
        "timelineItems": reference_timeline(prs, Some(older)),
    });
    reply_ok(&format!(
        r#"{{"data":{{"repository":{{"i{number}":{field}}},{rate}}}}}"#
    ))
}

/// An answer to a read of Issues by number, none of which has a cross-reference.
pub(super) fn no_issue_references(rate: &str) -> Result<Reply, Unreachable> {
    issue_references(&[], rate)
}

pub(super) fn origin_answer(node: &Value, rate: &str) -> Result<Reply, Unreachable> {
    reply_ok(&format!(
        r#"{{"data":{{"repository":{{"pullRequest":{node}}},{rate}}}}}"#
    ))
}

/// PR `number` as a fetch answers with it, with `changes` on top.
pub(super) fn pr_node(number: u32, changes: &Value) -> Value {
    let mut node = json!({
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
        "reviewRequests": { "nodes": [] },
        "latestReviews": { "nodes": [] },
        "lastCommit": { "nodes": [] },
        "mergeCommit": null,
        "headRefOid": "89abcdef0123456789abcdef0123456789abcdef",
        "commits": { "totalCount": 0, "nodes": [] },
        "timelineItems": { "nodes": [] },
    });
    for (name, value) in changes.as_object().unwrap() {
        node[name] = value.clone();
    }
    node
}

pub(super) fn fetched_nodes(nodes: &[Value], rate: &str) -> Result<Reply, Unreachable> {
    reply_ok(&format!(
        r#"{{"data":{{"nodes":{},{rate}}}}}"#,
        json!(nodes)
    ))
}

/// PR `number`, a merged PR that fixes Issue 1.
pub(super) fn merged_node(number: u32) -> Value {
    pr_node(
        number,
        &json!({ "state": "MERGED", "mergeCommit": { "oid": format!("{number:040x}") } }),
    )
}

/// The replies that bring the user's one PR, #1, and its one Issue, #1.
pub(super) fn up_to_the_first_issue() -> Vec<Result<Reply, Unreachable>> {
    let mut replies = searches_finding(&["a"]);
    replies.push(fetched_nodes(&[pr_node(1, &json!({}))], LOTS));
    replies.push(issues(&[1], LOTS));
    replies
}

/// The replies up to and including the search that finds `ids`.
pub(super) fn searches_finding(ids: &[&str]) -> Vec<Result<Reply, Unreachable>> {
    vec![
        found(ids, None, LOTS),
        nothing(LOTS),
        nothing(LOTS),
        nothing(LOTS),
    ]
}

pub(super) fn soon() -> Instant {
    Instant::now() + Duration::from_secs(60)
}
