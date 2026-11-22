//! Test helpers for the sync: a harness and GitHub's answers.

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use time::format_description::well_known::Rfc3339;

use super::{Progress, Syncs};
pub(super) use crate::github::testing::{RATE_LIMIT, fetched_nodes, pr_node, search_page};
use crate::github::testing::{Scripted, answer, github, issue_node, page_info, reply_ok, token};
use crate::github::{GitHub, Reply, Request, Transport, Unreachable};
use crate::repository::{OpenItems, RepositoryCounts, RepositoryName};
use crate::repository_settings::RepositorySettings;
use crate::store::testing::OpenStore;
pub(super) use crate::testing::cpython;
use crate::token::Token;

pub(super) const FEW: &str = r#""rateLimit":{"cost":1,"limit":5000,"used":4990,"remaining":10,
    "resetAt":"2999-01-01T00:00:00Z"}"#;

/// A Repository with no Filters, for the sync's other phases.
pub(super) fn plain(name: RepositoryName) -> RepositorySettings {
    RepositorySettings {
        filters: Vec::new(),
        ..RepositorySettings::new(name)
    }
}

pub(super) fn fetched(numbers: &[u32], rate: &str) -> Result<Reply, Unreachable> {
    fetched_titled(numbers, rate, |number| format!("PR {number}"))
}

pub(super) fn fetched_titled(
    numbers: &[u32],
    rate: &str,
    title: impl Fn(u32) -> String,
) -> Result<Reply, Unreachable> {
    let nodes: Vec<Value> = (numbers.iter())
        .map(|&number| pr_node(number, &json!({ "title": title(number) })))
        .collect();
    fetched_nodes(&nodes, rate)
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
        disk_kb: 0,
    })
}

/// The add dialog's count of a Repository, as GitHub answers it.
pub(super) fn counted_as(counts: RepositoryCounts) -> Result<Reply, Unreachable> {
    let body = json!({ "data": {
        "repository": {
            "nameWithOwner": "python/cpython",
            "diskUsage": counts.disk_kb,
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
    window_page(&[], None, RATE_LIMIT)
}

/// The time `hours` ago as GitHub writes it, for an item inside a window that moves with the clock.
pub(super) fn hours_ago_text(hours: i64) -> String {
    crate::testing::hours_ago(hours).format(&Rfc3339).unwrap()
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
        "state": "OPEN",
        "stateReason": null,
        "assignees": { "nodes": [] },
        "labels": { "nodes": [] },
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
    let list = json!({ "totalCount": nodes.len(), "pageInfo": page_info(next), "nodes": nodes });
    answer(json!({ "repository": { "list": list } }), rate)
}

pub(super) struct Harness {
    pub(super) github: GitHub,
    pub(super) db: OpenStore,
    pub(super) asked: Receiver<(String, Value)>,
}

impl Harness {
    /// The store holds the count `counted(LARGE)` answers with, so a first sync's count is no news to the
    /// window. Most tests are about something else, and a test of the count gives another.
    pub(super) fn new(name: &str, replies: Vec<Result<Reply, Unreachable>>) -> Self {
        let (script, asked) = Scripted::new(replies);
        let db = OpenStore::new(&format!("sync-{name}"));
        let counted = OpenItems {
            issues: LARGE,
            prs: 0,
        };
        assert!(db.store.save_open_items(&cpython(), counted).unwrap());
        Self {
            github: github(EmptyWindow(script)),
            db,
            asked,
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
        let facts = self.db.reader.work_facts(&cpython()).unwrap();
        let mut numbers: Vec<u32> = facts.pull_requests.iter().map(|pr| pr.number).collect();
        numbers.sort_unstable();
        numbers
    }

    /// The numbers of the Issues the store holds, in the order it reads them.
    pub(super) fn issue_numbers(&self) -> Vec<u32> {
        let facts = self.db.reader.work_facts(&cpython()).unwrap();
        facts.issues.iter().map(|issue| issue.number).collect()
    }

    /// The login behind each event the store holds of Issue or PR `number`.
    pub(super) fn activity_by(&self, number: u32) -> Vec<String> {
        let events = self.db.store.activity(&cpython(), number).unwrap();
        events
            .iter()
            .map(|event| event.who.as_str().to_owned())
            .collect()
    }

    pub(super) fn operations(&self) -> Vec<String> {
        self.asked
            .try_iter()
            .map(|(operation, _)| operation)
            .collect()
    }

    /// Makes the next sync a whole one, as a first sync that was interrupted leaves it.
    pub(super) fn forget_the_last_sync(&self) {
        self.db.store.forget_sync_state(&cpython()).unwrap();
    }

    pub(super) fn finish(self) {
        self.db.close();
    }
}

/// Runs every step that's left, and says which Repositories it announced.
pub(super) fn run_to_the_end(syncs: &mut Syncs, harness: &Harness) -> Vec<RepositoryName> {
    let mut changed = Vec::new();
    while syncs.ready_at().is_some() {
        changed.extend(announced(
            syncs.step(&harness.github, &harness.db.store).unwrap(),
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
    search_page(&[], None, rate)
}

pub(super) fn issues(numbers: &[u32], rate: &str) -> Result<Reply, Unreachable> {
    let fields: serde_json::Map<String, Value> = numbers
        .iter()
        .map(|&number| (format!("i{number}"), issue_node(number)))
        .collect();
    answer(json!({ "repository": fields }), rate)
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
    answer(json!({ "repository": { "issue": issue } }), rate)
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
                    "updatedAt": hours_ago_text(1),
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
    let fields: serde_json::Map<String, Value> = (issues.iter())
        .map(|(number, prs)| {
            let issue = json!({
                "__typename": "Issue",
                "number": number,
                "timelineItems": reference_timeline(prs, None),
            });
            (format!("i{number}"), issue)
        })
        .collect();
    answer(json!({ "repository": fields }), rate)
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
    answer(
        json!({ "repository": { format!("i{number}"): field } }),
        rate,
    )
}

/// An answer to a read of Issues by number, none of which has a cross-reference.
pub(super) fn no_issue_references(rate: &str) -> Result<Reply, Unreachable> {
    issue_references(&[], rate)
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
    replies.push(fetched_nodes(&[pr_node(1, &json!({}))], RATE_LIMIT));
    replies.push(issues(&[1], RATE_LIMIT));
    replies
}

/// The replies of [`up_to_the_first_issue`], then Issue #1's cross-references over two pages: merged PR #3
/// on the first, merged PR #2 on the second, and those two PRs.
pub(super) fn a_tree_over_two_pages() -> Vec<Result<Reply, Unreachable>> {
    let mut replies = up_to_the_first_issue();
    replies.push(issue_references_with_more(1, &[3], "older", RATE_LIMIT));
    replies.push(cross_references(&[2], None, RATE_LIMIT));
    replies.push(fetched_nodes(&[merged_node(3), merged_node(2)], RATE_LIMIT));
    replies
}

/// The replies up to and including the search that finds `ids`.
pub(super) fn searches_finding(ids: &[&str]) -> Vec<Result<Reply, Unreachable>> {
    vec![
        search_page(ids, None, RATE_LIMIT),
        nothing(RATE_LIMIT),
        nothing(RATE_LIMIT),
        nothing(RATE_LIMIT),
    ]
}

pub(super) fn soon() -> Instant {
    Instant::now() + Duration::from_secs(60)
}
