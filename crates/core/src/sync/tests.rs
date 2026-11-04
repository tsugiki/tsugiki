use super::*;
use crate::error::ErrorKind;
use crate::github::testing::{Scripted, reply_ok, token};
use crate::github::{Reply, Transport, Unreachable};
use crate::store::{self, OpenReader, StoreReader};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

pub(super) const LOTS: &str = r#""rateLimit":{"remaining":4000,"resetAt":"2026-11-04T12:00:00Z"}"#;
pub(super) const FEW: &str = r#""rateLimit":{"remaining":10,"resetAt":"2999-01-01T00:00:00Z"}"#;

pub(super) fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

/// A Repository with no Filters, for the sync's other phases.
fn plain(name: RepositoryName) -> RepositorySettings {
    RepositorySettings {
        filters: Vec::new(),
        ..RepositorySettings::new(name)
    }
}

fn found(ids: &[&str], next: Option<&str>, rate: &str) -> Result<Reply, Unreachable> {
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

/// An answer to a fetch of PRs, each of whose titles names the Issue with the same number.
fn fetched_naming_issues(numbers: &[u32], rate: &str) -> Result<Reply, Unreachable> {
    fetched_titled(numbers, rate, |number| format!("gh-{number}: Fix it"))
}

fn fetched_titled(
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
                "latestReviews": { "nodes": [] },
                "lastCommit": { "nodes": [] },
                "mergeCommit": null,
                "commits": { "totalCount": 0, "nodes": [] },
            })
        })
        .collect();
    reply_ok(&format!(
        r#"{{"data":{{"nodes":{},{rate}}}}}"#,
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
        let github = GitHub::new(Box::new(script), token());
        Self {
            github,
            store,
            open,
            reader,
            asked,
            folder,
        }
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
    let nodes: Vec<Value> = numbers
        .iter()
        .map(|number| {
            json!({ "source": {
                    "__typename": "PullRequest",
                    "id": format!("id{number}"),
                    "number": number,
                    "repository": { "nameWithOwner": "python/cpython" },
                } })
        })
        .collect();
    let issue = json!({ "timelineItems": {
            "pageInfo": { "hasPreviousPage": older.is_some(), "startCursor": older },
            "nodes": nodes,
        } });
    reply_ok(&format!(
        r#"{{"data":{{"repository":{{"issue":{issue}}},{rate}}}}}"#
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
        "commits": { "totalCount": 0, "nodes": [] },
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

fn origin_answer(node: &Value, rate: &str) -> Result<Reply, Unreachable> {
    reply_ok(&format!(
        r#"{{"data":{{"repository":{{"pullRequest":{node}}},{rate}}}}}"#
    ))
}

/// PR `number`, a Backport of `origin` on 3.14.
fn backport_node(number: u32, origin: u32) -> Value {
    pr_node(
        number,
        &json!({
            "title": format!("[3.14] gh-1: Fix it (GH-{origin})"),
            "baseRefName": "3.14",
        }),
    )
}

/// PR `number`, a merged PR that fixes Issue 1.
fn merged_node(number: u32) -> Value {
    pr_node(
        number,
        &json!({ "state": "MERGED", "mergeCommit": { "oid": format!("{number:040x}") } }),
    )
}

/// The replies that bring the user's one PR, #1, and its one Issue, #1.
fn up_to_the_first_issue() -> Vec<Result<Reply, Unreachable>> {
    let mut replies = searches_finding(&["a"]);
    replies.push(fetched_nodes(&[pr_node(1, &json!({}))], LOTS));
    replies.push(issues(&[1], LOTS));
    replies
}

#[test]
fn a_sync_reads_each_shown_issues_cross_references_and_fetches_the_prs_they_list() {
    let mut replies = up_to_the_first_issue();
    replies.push(cross_references(&[2, 3], None, LOTS));
    replies.push(fetched_nodes(&[merged_node(2), merged_node(3)], LOTS));
    let harness = Harness::new("tree", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(
        harness.operations()[4..],
        [
            "FetchPullRequests",
            "FetchIssues",
            "FetchCrossReferences",
            "FetchTreePullRequests",
        ]
    );
    assert_eq!(harness.numbers(), [1, 2, 3]);
    harness.finish();
}

#[test]
fn a_sync_follows_an_issues_cross_references_page_by_page() {
    let mut replies = up_to_the_first_issue();
    replies.push(cross_references(&[3], Some("older"), LOTS));
    replies.push(cross_references(&[2], None, LOTS));
    replies.push(fetched_nodes(&[merged_node(3), merged_node(2)], LOTS));
    let harness = Harness::new("tree-pages", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    drop(run_to_the_end(&mut syncs, &harness));

    let befores: Vec<Value> = harness
        .asked
        .try_iter()
        .filter(|(operation, _)| operation == "FetchCrossReferences")
        .map(|(_, sent)| sent["before"].clone())
        .collect();
    assert_eq!(befores, [Value::Null, json!("older")]);
    assert_eq!(harness.numbers(), [1, 2, 3]);
    harness.finish();
}

#[test]
fn a_sync_reads_the_cross_references_of_each_issue_and_fetches_a_shared_pr_once() {
    let mut replies = searches_finding(&["a"]);
    replies.push(fetched_nodes(
        &[pr_node(1, &json!({ "title": "gh-1, gh-2: Fix both" }))],
        LOTS,
    ));
    replies.push(issues(&[1, 2], LOTS));
    replies.push(cross_references(&[3, 1], None, LOTS));
    replies.push(cross_references(&[3], None, LOTS));
    replies.push(fetched_nodes(&[merged_node(3)], LOTS));
    let harness = Harness::new("tree-two", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    drop(run_to_the_end(&mut syncs, &harness));

    let tree_ids: Vec<Value> = harness
        .asked
        .try_iter()
        .filter(|(operation, _)| operation == "FetchTreePullRequests")
        .map(|(_, sent)| sent["ids"].clone())
        .collect();
    assert_eq!(tree_ids, [json!(["id3"])]);
    harness.finish();
}

#[test]
fn a_merged_pr_is_fetched_once_across_syncs_and_an_open_one_again() {
    let mut replies = up_to_the_first_issue();
    replies.push(cross_references(&[2, 3], None, LOTS));
    replies.push(fetched_nodes(
        &[merged_node(2), pr_node(3, &json!({}))],
        LOTS,
    ));
    replies.extend(up_to_the_first_issue());
    replies.push(cross_references(&[2, 3], None, LOTS));
    replies.push(fetched_nodes(&[pr_node(3, &json!({}))], LOTS));
    let harness = Harness::new("frozen", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    drop(run_to_the_end(&mut syncs, &harness));
    drop(harness.operations());

    harness.forget_the_last_sync();
    syncs.add(plain(cpython()));
    drop(run_to_the_end(&mut syncs, &harness));

    let tree_ids: Vec<Value> = harness
        .asked
        .try_iter()
        .filter(|(operation, _)| operation == "FetchTreePullRequests")
        .map(|(_, sent)| sent["ids"].clone())
        .collect();
    assert_eq!(tree_ids, [json!(["id3"])]);
    harness.finish();
}

#[test]
fn a_sync_fetches_a_users_backports_missing_origin_pr_by_number() {
    let mut replies = searches_finding(&["a"]);
    replies.push(fetched_nodes(&[backport_node(2, 9)], LOTS));
    replies.push(origin_answer(&merged_node(9), LOTS));
    replies.push(issues(&[1], LOTS));
    replies.push(no_cross_references(LOTS));
    let harness = Harness::new("origin", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    let changed = run_to_the_end(&mut syncs, &harness);

    let (numbers, asked): (Vec<String>, Vec<Value>) = harness.asked.try_iter().unzip();
    assert_eq!(
        numbers[4..],
        [
            "FetchPullRequests",
            "FetchOriginPullRequest",
            "FetchIssues",
            "FetchCrossReferences",
        ]
    );
    assert_eq!(asked[5]["number"], 9);
    assert_eq!(harness.numbers(), [2, 9]);
    assert_eq!(changed.len(), 3);
    harness.finish();
}

#[test]
fn a_sync_fetches_the_issues_a_users_backports_origin_pr_names() {
    let mut replies = searches_finding(&["a"]);
    let untitled = pr_node(
        2,
        &json!({ "title": "[3.14] Fix it (GH-9)", "baseRefName": "3.14" }),
    );
    replies.push(fetched_nodes(&[untitled], LOTS));
    replies.push(origin_answer(&merged_node(9), LOTS));
    replies.push(issues(&[1], LOTS));
    replies.push(no_cross_references(LOTS));
    let harness = Harness::new("origin-issue", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    drop(run_to_the_end(&mut syncs, &harness));

    let facts = harness.reader.work_facts(&cpython()).unwrap();
    let stored: Vec<u32> = facts.issues.iter().map(|issue| issue.number).collect();
    assert_eq!(stored, [1]);
    harness.finish();
}

#[test]
fn a_sync_fetches_a_tree_backports_missing_origin_pr_after_the_tree() {
    let mut replies = up_to_the_first_issue();
    replies.push(cross_references(&[2], None, LOTS));
    replies.push(fetched_nodes(&[backport_node(2, 9)], LOTS));
    replies.push(origin_answer(&merged_node(9), LOTS));
    let harness = Harness::new("tree-origin", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(harness.numbers(), [1, 2, 9]);
    harness.finish();
}

#[test]
fn a_sync_doesnt_ask_for_an_origin_pr_it_has_fetched_or_frozen() {
    let mut replies = searches_finding(&["a"]);
    replies.push(fetched_nodes(
        &[pr_node(9, &json!({})), backport_node(2, 9)],
        LOTS,
    ));
    replies.push(issues(&[1], LOTS));
    replies.push(no_cross_references(LOTS));
    // The next sync finds only the Backport, and its Origin PR has merged by then.
    replies.extend(searches_finding(&["a"]));
    replies.push(fetched_nodes(&[backport_node(2, 8)], LOTS));
    replies.push(origin_answer(&merged_node(8), LOTS));
    replies.push(issues(&[1], LOTS));
    replies.push(no_cross_references(LOTS));
    replies.extend(searches_finding(&["a"]));
    replies.push(fetched_nodes(&[backport_node(2, 8)], LOTS));
    replies.push(issues(&[1], LOTS));
    replies.push(no_cross_references(LOTS));
    let harness = Harness::new("origin-known", replies);
    let mut syncs = Syncs::default();

    let origins_in_each_sync: Vec<usize> = (0..3)
        .map(|_| {
            harness.forget_the_last_sync();
            syncs.add(plain(cpython()));
            drop(run_to_the_end(&mut syncs, &harness));
            harness
                .operations()
                .iter()
                .filter(|operation| *operation == "FetchOriginPullRequest")
                .count()
        })
        .collect();

    assert_eq!(origins_in_each_sync, [0, 1, 0]);
    harness.finish();
}

#[test]
fn a_frozen_origin_pr_still_leads_a_new_backport_to_its_issue() {
    let untitled = |number: u32| {
        pr_node(
            number,
            &json!({ "title": "[3.14] Fix it (GH-9)", "baseRefName": "3.14" }),
        )
    };
    let mut replies = searches_finding(&["a"]);
    replies.push(fetched_nodes(&[untitled(2)], LOTS));
    replies.push(origin_answer(&merged_node(9), LOTS));
    replies.push(issues(&[1], LOTS));
    replies.push(no_cross_references(LOTS));
    replies.extend(searches_finding(&["b"]));
    replies.push(fetched_nodes(&[untitled(3)], LOTS));
    replies.push(issues(&[1], LOTS));
    replies.push(cross_references(&[], None, LOTS));
    let harness = Harness::new("origin-frozen", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    drop(run_to_the_end(&mut syncs, &harness));
    drop(harness.operations());

    harness.forget_the_last_sync();
    syncs.add(plain(cpython()));
    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(
        harness.operations()[4..],
        ["FetchPullRequests", "FetchIssues", "FetchCrossReferences"]
    );
    harness.finish();
}

#[test]
fn an_origin_number_that_names_no_pr_is_asked_for_once() {
    let mut replies = searches_finding(&["a"]);
    replies.push(fetched_nodes(&[backport_node(2, 9)], LOTS));
    replies.push(reply_ok(&format!(
        r#"{{"data":{{"repository":{{"pullRequest":null}},{LOTS}}},"errors":[{{"type":"NOT_FOUND",
                "message":"gone","path":["repository","pullRequest"]}}]}}"#
    )));
    replies.push(issues(&[1], LOTS));
    replies.push(no_cross_references(LOTS));
    let harness = Harness::new("origin-gone", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(harness.numbers(), [2]);
    harness.finish();
}

#[test]
fn a_tree_read_below_the_floor_holds_the_next_step_back() {
    let mut replies = up_to_the_first_issue();
    replies.push(cross_references(&[], Some("older"), FEW));
    let harness = Harness::new("tree-floor", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    let before = Instant::now();

    // Four searches, two moves, the PR, two moves, the Issue, a move and the first page.
    for _ in 0..12 {
        drop(syncs.step(&harness.github, &harness.store).unwrap());
    }

    let ready = syncs.ready_at().unwrap();
    assert!(ready > before + Duration::from_secs(3000), "{ready:?}");
    harness.finish();
}

#[test]
fn an_origin_fetch_below_the_floor_holds_the_next_step_back() {
    let mut replies = searches_finding(&["a"]);
    replies.push(fetched_nodes(&[backport_node(2, 9)], LOTS));
    replies.push(origin_answer(&merged_node(9), FEW));
    let harness = Harness::new("origin-floor", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    let before = Instant::now();

    // Four searches, two moves, the PR, a move and the Origin PR.
    for _ in 0..9 {
        drop(syncs.step(&harness.github, &harness.store).unwrap());
    }

    let ready = syncs.ready_at().unwrap();
    assert!(ready > before + Duration::from_secs(3000), "{ready:?}");
    harness.finish();
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

#[test]
fn a_sync_fetches_the_issues_its_prs_name_after_the_prs() {
    let mut replies = searches_finding(&["a", "b"]);
    replies.push(fetched_naming_issues(&[1, 2], LOTS));
    replies.push(issues(&[1, 2], LOTS));
    replies.push(no_cross_references(LOTS));
    replies.push(no_cross_references(LOTS));
    let harness = Harness::new("issues", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    let changed = run_to_the_end(&mut syncs, &harness);

    let asked: Vec<String> = harness.operations();
    assert_eq!(
        asked[4..],
        [
            "FetchPullRequests",
            "FetchIssues",
            "FetchCrossReferences",
            "FetchCrossReferences",
        ]
    );
    let facts = harness.reader.work_facts(&cpython()).unwrap();
    let stored: Vec<u32> = facts.issues.iter().map(|issue| issue.number).collect();
    assert_eq!(stored, [1, 2]);
    assert_eq!(changed, [cpython(), cpython()]);
    harness.finish();
}

#[test]
fn a_sync_with_no_named_issues_asks_for_none() {
    let mut replies = searches_finding(&["a"]);
    replies.push(fetched(&[1], LOTS));
    let harness = Harness::new("no-issues", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(harness.operations().len(), 5);
    harness.finish();
}

#[test]
fn a_sync_fetches_a_titles_issue_whatever_the_conventions_of_its_repository() {
    let mut replies = searches_finding(&["a"]);
    replies.push(fetched_naming_issues(&[1], LOTS));
    replies.push(issues(&[1], LOTS));
    replies.push(no_cross_references(LOTS));
    let harness = Harness::new("conventions", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(RepositoryName::parse("python/peps").unwrap()));

    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(
        harness.operations()[4..],
        ["FetchPullRequests", "FetchIssues", "FetchCrossReferences"]
    );
    harness.finish();
}

#[test]
fn a_fetch_of_issues_goes_batch_by_batch_and_pauses_below_the_floor() {
    let ids: Vec<u32> = (1..=30).collect();
    let mut replies = searches_finding(&["a"]);
    replies.push(fetched_naming_issues(&ids, LOTS));
    replies.push(issues(&[1], FEW));
    replies.push(issues(&[2], LOTS));
    replies.push(no_cross_references(LOTS));
    replies.push(no_cross_references(LOTS));
    let harness = Harness::new("issue-batches", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    let before = Instant::now();

    // Four searches, two moves, the PRs, two moves and the first batch of Issues.
    for _ in 0..10 {
        drop(syncs.step(&harness.github, &harness.store).unwrap());
    }
    let ready = syncs.ready_at().unwrap();
    assert!(ready > before + Duration::from_secs(3000), "{ready:?}");
    // The two batches take all 30 numbers.
    drop(run_to_the_end(&mut syncs, &harness));
    assert_eq!(syncs.ready_at(), None);

    let facts = harness.reader.work_facts(&cpython()).unwrap();
    assert_eq!(facts.issues.len(), 2);
    harness.finish();
}

#[test]
fn a_sync_searches_in_order_then_stores_what_it_found() {
    let harness = Harness::new(
        "order",
        vec![
            found(&["a"], None, LOTS),
            found(&["b"], None, LOTS),
            found(&["c"], None, LOTS),
            found(&["d"], None, LOTS),
            fetched(&[1, 2, 3, 4], LOTS),
        ],
    );
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    let changed = run_to_the_end(&mut syncs, &harness);

    assert_eq!(
        harness.operations(),
        [
            "SearchReviewRequests",
            "SearchReviewedPrs",
            "SearchOwnPrs",
            "SearchAssignedPrs",
            "FetchPullRequests",
        ]
    );
    assert_eq!(harness.numbers(), [1, 2, 3, 4]);
    assert_eq!(changed, [cpython()]);
    harness.finish();
}

#[test]
fn a_search_follows_its_pages_before_the_next_search() {
    let harness = Harness::new(
        "pages",
        vec![
            found(&["a"], Some("next"), LOTS),
            found(&["b"], None, LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            fetched(&[1, 2], LOTS),
        ],
    );
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    drop(run_to_the_end(&mut syncs, &harness));

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(asked[0].1["after"], Value::Null);
    assert_eq!(asked[1].0, "SearchReviewRequests");
    assert_eq!(asked[1].1["after"], "next");
    assert_eq!(asked[2].0, "SearchReviewedPrs");
    assert_eq!(asked[2].1["after"], Value::Null);
    assert_eq!(asked[5].1["ids"], json!(["a", "b"]));
    harness.finish();
}

#[test]
fn a_pr_two_searches_found_is_fetched_once() {
    let harness = Harness::new(
        "twice",
        vec![
            found(&["a"], None, LOTS),
            found(&["a"], None, LOTS),
            found(&["a", "b"], None, LOTS),
            nothing(LOTS),
            fetched(&[1, 2], LOTS),
        ],
    );
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    drop(run_to_the_end(&mut syncs, &harness));

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(asked[4].1["ids"], json!(["a", "b"]));
    harness.finish();
}

#[test]
fn a_retry_at_half_size_leaves_the_rest_for_the_next_batch() {
    let ids: Vec<String> = (0..4).map(|n| format!("id{n}")).collect();
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let harness = Harness::new(
        "half",
        vec![
            found(&ids, None, LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            Err(Unreachable),
            fetched(&[1, 2], LOTS),
            fetched(&[3, 4], LOTS),
        ],
    );
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    let changed = run_to_the_end(&mut syncs, &harness);

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(asked[5].1["ids"], json!(["id0", "id1"]));
    assert_eq!(asked[6].1["ids"], json!(["id2", "id3"]));
    assert_eq!(harness.numbers(), [1, 2, 3, 4]);
    assert_eq!(changed, [cpython(), cpython()]);
    harness.finish();
}

#[test]
fn a_failed_step_keeps_its_place_and_runs_again() {
    let harness = Harness::new(
        "failure",
        vec![
            found(&["a"], Some("next"), LOTS),
            Err(Unreachable),
            Err(Unreachable),
            found(&["b"], None, LOTS),
        ],
    );
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    assert_eq!(syncs.step(&harness.github, &harness.store).unwrap(), None);

    let failed = syncs.step(&harness.github, &harness.store).unwrap_err();
    drop(syncs.step(&harness.github, &harness.store).unwrap());

    assert_eq!(failed.kind, ErrorKind::Offline);
    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(asked[3].1["after"], "next");
    harness.finish();
}

#[test]
fn a_search_below_the_floor_holds_the_next_step_back() {
    let harness = Harness::new("floor-search", vec![nothing(FEW)]);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    let before = Instant::now();

    drop(syncs.step(&harness.github, &harness.store).unwrap());

    let ready = syncs.ready_at().unwrap();
    assert!(ready > before + Duration::from_secs(3000), "{ready:?}");
    harness.finish();
}

#[test]
fn a_fetch_below_the_floor_holds_the_next_step_back() {
    let harness = Harness::new(
        "floor-fetch",
        vec![
            found(&["a"], None, LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            fetched(&[1], FEW),
        ],
    );
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    let before = Instant::now();

    for _ in 0..7 {
        drop(syncs.step(&harness.github, &harness.store).unwrap());
    }

    let ready = syncs.ready_at().unwrap();
    assert!(ready > before + Duration::from_secs(3000), "{ready:?}");
    assert_eq!(harness.numbers(), [1]);
    harness.finish();
}

#[test]
fn a_held_back_step_says_when() {
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    let later = Instant::now() + Duration::from_secs(60);

    syncs.wait_until(later);

    assert_eq!(syncs.ready_at(), Some(later));
}

#[test]
fn nothing_queued_means_nothing_to_do() {
    let harness = Harness::new("empty", vec![]);
    let mut syncs = Syncs::default();

    assert_eq!(syncs.ready_at(), None);
    assert_eq!(syncs.step(&harness.github, &harness.store).unwrap(), None);
    assert_eq!(harness.operations(), [""; 0]);
    harness.finish();
}

#[test]
fn a_repository_already_queued_isnt_queued_twice() {
    let harness = Harness::new(
        "dedupe",
        vec![nothing(LOTS), nothing(LOTS), nothing(LOTS), nothing(LOTS)],
    );
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    syncs.add(plain(RepositoryName::parse("Python/CPython").unwrap()));

    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(harness.operations().len(), 4);
    harness.finish();
}

#[test]
fn a_sync_pauses_only_below_the_floor() {
    let reset_at = OffsetDateTime::UNIX_EPOCH;
    let at_the_floor = RateLimit {
        remaining: RATE_LIMIT_FLOOR,
        reset_at,
    };
    let below = RateLimit {
        remaining: RATE_LIMIT_FLOOR - 1,
        reset_at,
    };
    assert_eq!(pause_until(at_the_floor), None);
    assert_eq!(pause_until(below), Some(reset_at));
}

#[test]
fn every_search_finds_open_prs() {
    for search in WORK_SEARCHES {
        assert!(
            search.qualifiers.starts_with("is:pr is:open "),
            "{search:?}"
        );
    }
}

#[test]
fn a_sync_asks_for_pages_of_50_and_batches_of_25() {
    let names: Vec<String> = (0..30).map(|n| format!("id{n}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let harness = Harness::new(
        "sizes",
        vec![
            found(&names, None, LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            fetched(&[1], LOTS),
            fetched(&[2], LOTS),
        ],
    );
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    drop(run_to_the_end(&mut syncs, &harness));

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(asked[0].1["first"], 50);
    assert_eq!(
        asked[0].1["query"],
        "repo:python/cpython is:pr is:open user-review-requested:@me"
    );
    assert_eq!(asked[4].1["ids"].as_array().unwrap().len(), 25);
    assert_eq!(asked[5].1["ids"].as_array().unwrap().len(), 5);
    harness.finish();
}

#[test]
fn the_repository_asked_for_last_runs_first() {
    let harness = Harness::new(
        "queue",
        vec![
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
        ],
    );
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    syncs.add(plain(RepositoryName::parse("python/peps").unwrap()));

    drop(run_to_the_end(&mut syncs, &harness));

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    let query = |at: usize| asked[at].1["query"].as_str().unwrap().to_owned();
    assert!(query(3).starts_with("repo:python/peps "), "{}", query(3));
    assert!(query(4).starts_with("repo:python/cpython "), "{}", query(4));
    harness.finish();
}

#[test]
fn a_sync_keeps_its_progress_behind_a_newer_one() {
    let harness = Harness::new(
        "behind",
        vec![
            found(&["a"], Some("next"), LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            found(&[], None, LOTS),
        ],
    );
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    assert_eq!(syncs.step(&harness.github, &harness.store).unwrap(), None);
    syncs.add(plain(RepositoryName::parse("python/peps").unwrap()));

    // The newer sync's four searches, its seven moves from phase to phase and its end, then the older
    // sync's next page.
    for _ in 0..13 {
        let progress = syncs.step(&harness.github, &harness.store).unwrap();
        assert_eq!(announced(progress), None);
    }

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert!(
        asked[5].1["query"]
            .as_str()
            .unwrap()
            .starts_with("repo:python/cpython ")
    );
    assert_eq!(asked[5].1["after"], "next");
    harness.finish();
}

fn soon() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

#[test]
fn with_nothing_open_a_refresh_queues_nothing() {
    let harness = Harness::new("refresh-nothing", Vec::new());
    let mut syncs = Syncs::default();
    syncs.refresh_open(&harness.store);
    syncs.refresh_now();
    assert_eq!(syncs.ready_at(), None);
    harness.finish();
}

#[test]
fn the_timer_leaves_a_first_sync_that_gave_up_and_a_click_starts_it_again() {
    let harness = Harness::new("gave-up", Vec::new());
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    for _ in 0..TRIES {
        syncs.after_failure(&broken(), soon());
    }
    assert_eq!(syncs.ready_at(), None);

    syncs.refresh_open(&harness.store);
    assert_eq!(syncs.ready_at(), None);

    syncs.refresh_now();
    assert_eq!(syncs.queue.len(), 1);
    harness.finish();
}

#[test]
fn a_click_ends_a_wait_for_the_network_and_keeps_a_wait_for_points() {
    let later = Instant::now() + Duration::from_secs(3600);
    let offline = Error::new(ErrorKind::Offline, "No network.");
    let spent = Error::new(ErrorKind::RateLimited { resets_at: None }, "No points.");
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    syncs.after_failure(&offline, later);
    syncs.refresh_now();
    assert!(syncs.ready_at().unwrap() < later);

    syncs.after_failure(&spent, later);
    syncs.refresh_now();
    assert_eq!(syncs.ready_at(), Some(later));
}

#[test]
fn the_last_sync_says_when_it_finished_and_the_next_asks_from_when_it_started() {
    let harness = Harness::new("finish-time", searches_finding(&[]));
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    let began = OffsetDateTime::now_utc() - time::Duration::hours(1);
    syncs.queue.front_mut().unwrap().started_at = began;

    let mut finished = None;
    while syncs.ready_at().is_some() {
        if let Some(Progress::Finished(last_sync)) =
            syncs.step(&harness.github, &harness.store).unwrap()
        {
            finished = last_sync.at;
        }
    }

    let state = harness.store.sync_state(&cpython()).unwrap().unwrap();
    assert_eq!(finished, Some(state.finished_at));
    assert_eq!(state.started_at.unix_timestamp(), began.unix_timestamp());
    assert!(state.finished_at > began + time::Duration::minutes(59));
    harness.finish();
}

#[test]
fn a_refresh_of_the_open_repository_joins_the_queue_once() {
    let harness = Harness::new("refresh-open", searches_finding(&[]));
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    drop(run_to_the_end(&mut syncs, &harness));
    let operations = harness.operations().len();
    assert_eq!(operations, 4, "one sync's four searches");

    syncs.refresh_open(&harness.store);
    syncs.refresh_open(&harness.store);

    assert_eq!(syncs.queue.len(), 1);
    harness.finish();
}

#[test]
fn a_refresh_goes_behind_a_sync_of_another_repository() {
    let peps = RepositoryName::parse("python/peps").unwrap();
    let mut replies = searches_finding(&[]);
    replies.extend(searches_finding(&[]));
    let harness = Harness::new("refresh-behind", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    drop(run_to_the_end(&mut syncs, &harness));
    syncs.add(plain(peps.clone()));
    syncs.add(plain(cpython()));
    syncs.queue.retain(|sync| sync.repository == peps);
    drop(syncs.step(&harness.github, &harness.store).unwrap());

    syncs.refresh_open(&harness.store);

    assert_eq!(syncs.queue.len(), 2);
    assert_eq!(syncs.queue.front().unwrap().repository, peps);
    harness.finish();
}

#[test]
fn only_the_open_repository_says_its_sync_finished() {
    let peps = RepositoryName::parse("python/peps").unwrap();
    let mut replies = searches_finding(&[]);
    replies.extend(searches_finding(&[]));
    let harness = Harness::new("finished", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(peps));
    syncs.add(plain(cpython()));

    let mut finished = Vec::new();
    while syncs.ready_at().is_some() {
        let progress = syncs.step(&harness.github, &harness.store).unwrap();
        if let Some(Progress::Finished(last_sync)) = progress {
            finished.push(last_sync.repository);
        }
    }

    assert_eq!(finished, [cpython()]);
    harness.finish();
}

fn broken() -> Error {
    Error::new(ErrorKind::Internal, "GitHub answered with status 500.")
}

/// Whether a sync is still queued after `tries` failures for `problem`.
fn queued_after(tries: u8, problem: &Error) -> bool {
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    for _ in 0..tries {
        syncs.after_failure(problem, soon());
    }
    syncs.ready_at().is_some()
}

#[test]
fn a_step_that_keeps_failing_gives_way_to_the_next_sync() {
    let mut syncs = Syncs::default();
    syncs.add(plain(RepositoryName::parse("python/peps").unwrap()));
    syncs.add(plain(cpython()));

    for _ in 0..TRIES {
        syncs.after_failure(&broken(), soon());
    }

    assert_eq!(syncs.queue.len(), 1);
    assert_eq!(
        syncs.queue.front().unwrap().repository,
        RepositoryName::parse("python/peps").unwrap()
    );
    assert_eq!(syncs.failures, 0);
}

#[test]
fn failures_with_nothing_queued_change_nothing() {
    let mut syncs = Syncs::default();
    for _ in 0..TRIES {
        syncs.after_failure(&broken(), soon());
    }
    assert_eq!(syncs.ready_at(), None);
}

#[test]
fn an_outage_shorter_than_the_tries_doesnt_end_a_sync() {
    assert!(queued_after(TRIES - 1, &broken()));
    assert!(!queued_after(TRIES, &broken()));
}

#[test]
fn a_step_that_works_starts_the_count_again() {
    let harness = Harness::new("recovers", vec![nothing(LOTS)]);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    for _ in 0..TRIES - 1 {
        syncs.after_failure(&broken(), soon());
    }

    drop(syncs.step(&harness.github, &harness.store).unwrap());
    syncs.after_failure(&broken(), soon());

    assert_eq!(syncs.queue.len(), 1);
    harness.finish();
}

#[test]
fn asking_again_starts_the_count_again() {
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    for _ in 0..TRIES - 1 {
        syncs.after_failure(&broken(), soon());
    }

    syncs.add(plain(cpython()));
    syncs.after_failure(&broken(), soon());

    assert_eq!(syncs.queue.len(), 1);
}

#[test]
fn no_network_never_ends_a_sync() {
    let offline = Error::new(ErrorKind::Offline, "no network");
    assert!(queued_after(u8::MAX, &offline));
}

#[test]
fn a_rate_limit_never_ends_a_sync() {
    let limited = Error::new(ErrorKind::RateLimited { resets_at: None }, "limited");
    assert!(queued_after(u8::MAX, &limited));
}

#[test]
fn a_failure_waits_for_the_next_refresh() {
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    let next_refresh = soon();

    syncs.after_failure(&broken(), next_refresh);

    assert_eq!(syncs.ready_at(), Some(next_refresh));
}

fn limited_until(from_now: i64) -> Error {
    let resets_at = OffsetDateTime::now_utc().unix_timestamp() + from_now;
    let resets_at = Some(u32::try_from(resets_at).unwrap());
    Error::new(ErrorKind::RateLimited { resets_at }, "limited")
}

#[test]
fn a_rate_limit_waits_for_its_reset() {
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    syncs.after_failure(&limited_until(3000), soon());

    let wait = syncs.ready_at().unwrap() - Instant::now();
    assert!(wait > Duration::from_secs(2900), "{wait:?}");
    assert!(wait <= Duration::from_secs(3001), "{wait:?}");
}

#[test]
fn a_rate_limit_whose_reset_has_passed_still_waits_for_the_next_refresh() {
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    let next_refresh = soon();

    syncs.after_failure(&limited_until(-10), next_refresh);

    assert_eq!(syncs.ready_at(), Some(next_refresh));
}

#[test]
fn a_save_that_fails_is_the_steps_failure() {
    let harness = Harness::new(
        "save-fails",
        vec![
            found(&["a"], None, LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            fetched(&[1], LOTS),
        ],
    );
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    for _ in 0..5 {
        drop(syncs.step(&harness.github, &harness.store).unwrap());
    }
    let stopped = Store::stopped();

    let failed = syncs.step(&harness.github, &stopped).unwrap_err();
    stopped.close();

    assert_eq!(failed.kind, ErrorKind::Internal);
    harness.finish();
}

#[test]
fn a_pause_is_as_long_as_the_reset_is_away_within_reason() {
    let now = OffsetDateTime::UNIX_EPOCH;
    let minute = time::Duration::minutes(1);

    assert_eq!(pause_for(now + minute, now), Duration::from_secs(60));
    assert_eq!(pause_for(now - minute, now), Duration::ZERO);
    assert_eq!(pause_for(now + minute * 600, now), LONGEST_PAUSE);
}
