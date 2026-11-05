use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};

use crate::github::{Login, Reply, Unreachable};
use crate::repository::RepositorySettings;
use crate::status::SyncPart;
use crate::store::FirstSyncPlace;
use crate::sync::testing::{
    FEW, Harness, LOTS, comment_by, cpython, cross_references, empty_window_page, fetched_nodes,
    hours_ago, issue_references, issue_references_with_more, issues, plain, pr_node,
    run_to_the_end, searches_finding, window_issue, window_issue_referencing, window_page,
    window_pr,
};
use crate::sync::{Progress, Step, Syncs, UserWorkSync};

/// The replies up to the queue, for a user with no work.
fn up_to_the_queue() -> Vec<Result<Reply, Unreachable>> {
    searches_finding(&[])
}

/// The replies that bring the user's PR #1 and its Issue #1, up to the queue.
fn up_to_the_queue_with_an_issue() -> Vec<Result<Reply, Unreachable>> {
    let mut replies = searches_finding(&["a"]);
    replies.push(fetched_nodes(&[pr_node(1, &json!({}))], LOTS));
    replies.push(issues(&[1], LOTS));
    replies
}

/// PR `number`, merged, naming no Issue.
fn merged(number: u32) -> Value {
    pr_node(
        number,
        &json!({
            "title": format!("Fix it {number}"),
            "state": "MERGED",
            "mergeCommit": { "oid": format!("{number:040x}") },
        }),
    )
}

fn syncing() -> Syncs {
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    syncs
}

fn run_steps(syncs: &mut Syncs, harness: &Harness, steps: usize) {
    for _ in 0..steps {
        drop(syncs.step(&harness.github, &harness.store).unwrap());
    }
}

/// Runs until the sync has asked GitHub for `operation`.
fn run_until_asked(syncs: &mut Syncs, harness: &Harness, operation: &str) -> Vec<(String, Value)> {
    let mut asked = Vec::new();
    while !asked
        .iter()
        .any(|(name, _): &(String, Value)| name == operation)
    {
        drop(syncs.step(&harness.github, &harness.store).unwrap());
        asked.extend(harness.asked.try_iter());
    }
    asked
}

/// The PRs that each fetch asked for, in order.
fn fetched_ids(asked: &[(String, Value)]) -> Vec<Value> {
    asked
        .iter()
        .filter(|(name, _)| name == "FetchTreePullRequests")
        .map(|(_, sent)| sent["ids"].clone())
        .collect()
}

fn queue_operations(harness: &Harness) -> Vec<String> {
    let operations = harness.operations();
    let first = operations
        .iter()
        .position(|name| !name.starts_with("Search"))
        .unwrap_or(operations.len());
    operations.into_iter().skip(first).collect()
}

#[test]
fn the_users_own_trees_load_before_the_window() {
    let mut replies = up_to_the_queue_with_an_issue();
    replies.push(issue_references(&[(1, &[5])], LOTS));
    replies.push(fetched_nodes(&[merged(5)], LOTS));
    replies.push(empty_window_page());
    replies.push(empty_window_page());
    let harness = Harness::with_window("queue-own-first", replies);
    let mut syncs = syncing();

    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(
        queue_operations(&harness),
        [
            "FetchPullRequests",
            "FetchIssues",
            "FetchIssueReferences",
            "FetchTreePullRequests",
            "WindowIssues",
            "WindowPullRequests",
        ]
    );
    assert_eq!(harness.numbers(), [1, 5]);
    harness.finish();
}

#[test]
fn a_page_and_a_batch_take_turns() {
    let at = hours_ago(1);
    let mut replies = up_to_the_queue();
    replies.push(window_page(
        &[window_issue_referencing(10, &at, &[20])],
        Some("more"),
        LOTS,
    ));
    replies.push(fetched_nodes(&[merged(20)], LOTS));
    replies.push(window_page(
        &[window_issue_referencing(11, &at, &[21])],
        None,
        LOTS,
    ));
    replies.push(fetched_nodes(&[merged(21)], LOTS));
    replies.push(empty_window_page());
    let harness = Harness::with_window("queue-turns", replies);
    let mut syncs = syncing();

    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(
        queue_operations(&harness),
        [
            "WindowIssues",
            "FetchTreePullRequests",
            "WindowIssues",
            "FetchTreePullRequests",
            "WindowPullRequests",
        ]
    );
    harness.finish();
}

#[test]
fn a_pr_many_lists_bring_is_fetched_once() {
    let at = hours_ago(1);
    let mut replies = up_to_the_queue_with_an_issue();
    replies.push(issue_references(&[(1, &[5])], LOTS));
    replies.push(fetched_nodes(&[merged(5)], LOTS));
    replies.push(window_page(
        &[
            window_issue_referencing(10, &at, &[5, 20]),
            window_issue_referencing(11, &at, &[20]),
        ],
        None,
        LOTS,
    ));
    replies.push(fetched_nodes(&[merged(20)], LOTS));
    replies.push(window_page(
        &[window_pr(1, &at), window_pr(5, &at), window_pr(20, &at)],
        None,
        LOTS,
    ));
    let harness = Harness::with_window("queue-once", replies);
    let mut syncs = syncing();

    drop(run_to_the_end(&mut syncs, &harness));

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(fetched_ids(&asked), [json!(["id5"]), json!(["id20"])]);
    harness.finish();
}

#[test]
fn the_list_of_prs_brings_what_no_issue_reached() {
    let at = hours_ago(1);
    let mut replies = up_to_the_queue();
    replies.push(empty_window_page());
    replies.push(window_page(&[window_pr(30, &at)], None, LOTS));
    replies.push(fetched_nodes(
        &[pr_node(30, &json!({ "title": "A change with no Issue" }))],
        LOTS,
    ));
    let harness = Harness::with_window("queue-standalone", replies);
    let mut syncs = syncing();

    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(harness.numbers(), [30]);
    harness.finish();
}

/// Issue 10 as a page lists it: PR 20 inside the window, PR 21 older than it, and older references besides.
fn issue_with_a_pr_older_than_the_window() -> Value {
    let mut issue = window_issue_with_more(10, "older");
    issue["references"]["nodes"] = json!([older_reference(21), reference(20)]);
    issue
}

fn reference(number: u32) -> Value {
    window_issue_referencing(0, &hours_ago(1), &[number])["references"]["nodes"][0].clone()
}

fn older_reference(number: u32) -> Value {
    let mut older = reference(number);
    older["source"]["updatedAt"] = json!("2000-01-01T00:00:00Z");
    older
}

#[test]
fn a_page_queues_only_the_prs_inside_the_window() {
    let mut replies = up_to_the_queue();
    replies.push(window_page(
        &[issue_with_a_pr_older_than_the_window()],
        None,
        LOTS,
    ));
    replies.push(fetched_nodes(&[merged(20)], LOTS));
    replies.push(empty_window_page());
    let harness = Harness::with_window("queue-older-waits", replies);
    let mut syncs = syncing();

    drop(run_to_the_end(&mut syncs, &harness));

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(fetched_ids(&asked), [json!(["id20"])]);
    assert_eq!(long_reads(&asked), Vec::<(Value, Value)>::new());
    harness.finish();
}

#[test]
fn opening_an_issue_brings_its_older_prs_and_older_references() {
    let mut replies = up_to_the_queue();
    replies.push(window_page(
        &[issue_with_a_pr_older_than_the_window()],
        None,
        LOTS,
    ));
    replies.push(issue_references_with_more(10, &[20, 21], "older", LOTS));
    replies.push(cross_references(&[22], Some("oldest"), LOTS));
    replies.push(cross_references(&[], None, LOTS));
    replies.push(fetched_nodes(&[merged(20), merged(21), merged(22)], LOTS));
    replies.push(empty_window_page());
    let harness = Harness::with_window("queue-older-opened", replies);
    let mut syncs = syncing();
    drop(run_until_asked(&mut syncs, &harness, "WindowIssues"));

    syncs.open_issue(&cpython(), 10);
    drop(run_to_the_end(&mut syncs, &harness));

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(fetched_ids(&asked), [json!(["id21", "id20", "id22"])]);
    assert_eq!(
        long_reads(&asked),
        [(json!(10), json!("older")), (json!(10), json!("oldest"))]
    );
    harness.finish();
}

#[test]
fn opening_the_users_own_issue_doesnt_read_its_tree_again() {
    let mut replies = up_to_the_queue_with_an_issue();
    replies.push(issue_references(&[(1, &[])], LOTS));
    replies.push(empty_window_page());
    replies.push(empty_window_page());
    let harness = Harness::with_window("queue-open-own", replies);
    let mut syncs = syncing();
    drop(run_until_asked(
        &mut syncs,
        &harness,
        "FetchIssueReferences",
    ));

    syncs.open_issue(&cpython(), 1);
    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(harness.operations(), ["WindowIssues", "WindowPullRequests"]);
    harness.finish();
}

#[test]
fn opening_an_issue_puts_its_prs_next_and_takes_them_from_the_queue() {
    let at = hours_ago(1);
    let mut replies = up_to_the_queue();
    // Twenty-six PRs: more than a batch holds, so 45 waits for the second one unless it's opened.
    let waiting: Vec<u32> = (20..45).collect();
    let mut page = vec![window_issue_referencing(10, &at, &waiting)];
    page.push(window_issue_referencing(11, &at, &[45]));
    replies.push(window_page(&page, None, LOTS));
    replies.push(issue_references(&[(11, &[45])], LOTS));
    replies.push(fetched_nodes(&[merged(45), merged(44)], LOTS));
    replies.push(empty_window_page());
    replies.push(fetched_nodes(&[merged(20)], LOTS));
    let harness = Harness::with_window("queue-open", replies);
    let mut syncs = syncing();
    let before_it_opened = run_until_asked(&mut syncs, &harness, "WindowIssues");
    assert_eq!(fetched_ids(&before_it_opened), Vec::<Value>::new());

    syncs.open_issue(&cpython(), 11);
    drop(run_to_the_end(&mut syncs, &harness));

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    let ids = fetched_ids(&asked);
    let first = ids[0].as_array().unwrap();
    assert_eq!(first[0], json!("id45"));
    assert_eq!(asked[0].0, "FetchIssueReferences");
    let all: Vec<&Value> = ids
        .iter()
        .flat_map(|batch| batch.as_array().unwrap())
        .collect();
    assert_eq!(all.iter().filter(|id| ***id == json!("id45")).count(), 1);
    assert_eq!(all.len(), 26);
    harness.finish();
}

/// Issue `number` on a page of the window, with references older than the page holds.
fn window_issue_with_more(number: u32, older: &str) -> Value {
    let mut issue = window_issue_referencing(number, &hours_ago(1), &[]);
    issue["references"]["pageInfo"] = json!({ "hasPreviousPage": true, "startCursor": older });
    issue
}

/// The Issue and cursor of each read of older references.
fn long_reads(asked: &[(String, Value)]) -> Vec<(Value, Value)> {
    asked
        .iter()
        .filter(|(name, _)| name == "FetchCrossReferences")
        .map(|(_, sent)| (sent["number"].clone(), sent["before"].clone()))
        .collect()
}

#[test]
fn a_view_that_asks_again_doesnt_read_its_issue_again() {
    let mut replies = up_to_the_queue();
    replies.push(window_page(&[window_issue(11, &hours_ago(1))], None, LOTS));
    replies.push(issue_references(&[(11, &[])], LOTS));
    replies.push(empty_window_page());
    let harness = Harness::with_window("queue-open-twice", replies);
    let mut syncs = syncing();
    drop(run_until_asked(&mut syncs, &harness, "WindowIssues"));

    syncs.open_issue(&cpython(), 11);
    syncs.open_issue(&cpython(), 11);
    drop(run_until_asked(
        &mut syncs,
        &harness,
        "FetchIssueReferences",
    ));
    // A view on screen asks about once a minute, and by now the tree has been read.
    syncs.open_issue(&cpython(), 11);
    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(
        harness.operations().last().map(String::as_str),
        Some("WindowPullRequests")
    );
    harness.finish();
}

#[test]
fn an_opened_issues_older_references_go_before_the_queue() {
    let at = hours_ago(1);
    let mut replies = up_to_the_queue();
    replies.push(window_page(
        &[window_issue_referencing(10, &at, &[20])],
        None,
        LOTS,
    ));
    replies.push(issue_references_with_more(11, &[], "older-11", LOTS));
    replies.push(cross_references(&[], None, LOTS));
    replies.push(fetched_nodes(&[merged(20)], LOTS));
    replies.push(empty_window_page());
    let harness = Harness::with_window("queue-long-urgent", replies);
    let mut syncs = syncing();
    drop(run_until_asked(&mut syncs, &harness, "WindowIssues"));

    syncs.open_issue(&cpython(), 11);
    drop(run_until_asked(
        &mut syncs,
        &harness,
        "FetchIssueReferences",
    ));
    let activity = syncs.activity().unwrap();
    assert_eq!(activity.part, SyncPart::Trees);
    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(
        harness.operations(),
        [
            "FetchCrossReferences",
            "FetchTreePullRequests",
            "WindowPullRequests"
        ]
    );
    harness.finish();
}

#[test]
fn a_page_doesnt_read_the_older_references_of_the_users_own_issue_again() {
    let mut replies = up_to_the_queue_with_an_issue();
    replies.push(issue_references_with_more(1, &[], "older", LOTS));
    replies.push(cross_references(&[], None, LOTS));
    replies.push(window_page(
        &[window_issue_with_more(1, "older")],
        None,
        LOTS,
    ));
    replies.push(empty_window_page());
    let harness = Harness::with_window("queue-long-own", replies);
    let mut syncs = syncing();

    drop(run_to_the_end(&mut syncs, &harness));

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(long_reads(&asked), [(json!(1), json!("older"))]);
    harness.finish();
}

#[test]
fn an_opened_issues_batch_fills_up_from_the_queue() {
    let at = hours_ago(1);
    let mut replies = up_to_the_queue();
    replies.push(window_page(
        &[window_issue_referencing(10, &at, &[8, 20])],
        None,
        LOTS,
    ));
    replies.push(issue_references(&[(11, &[45])], LOTS));
    replies.push(fetched_nodes(
        &[backport(45, 8), merged(8), merged(20)],
        LOTS,
    ));
    replies.push(empty_window_page());
    let harness = Harness::with_window("queue-open-origin", replies);
    let mut syncs = syncing();
    drop(run_until_asked(&mut syncs, &harness, "WindowIssues"));

    syncs.open_issue(&cpython(), 11);
    drop(run_to_the_end(&mut syncs, &harness));

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(fetched_ids(&asked), [json!(["id45", "id20", "id8"])]);
    assert!(
        !asked
            .iter()
            .any(|(name, _)| name == "FetchOriginPullRequest")
    );
    harness.finish();
}

#[test]
fn an_urgent_origin_waiting_in_the_queue_leaves_it() {
    let at = hours_ago(1);
    let waiting: Vec<u32> = (20..45).collect();
    let mut replies = up_to_the_queue();
    replies.push(window_page(
        &[window_issue_referencing(10, &at, &waiting)],
        Some("more"),
        LOTS,
    ));
    replies.push(issue_references(&[(11, &[45])], LOTS));
    replies.push(fetched_nodes(&[backport(45, 20)], LOTS));
    replies.push(crate::sync::testing::origin_answer(&merged(20), LOTS));
    let harness = Harness::with_window("queue-origin-waiting", replies);
    let mut syncs = syncing();
    drop(run_until_asked(&mut syncs, &harness, "WindowIssues"));
    syncs.open_issue(&cpython(), 11);

    drop(run_until_asked(
        &mut syncs,
        &harness,
        "FetchOriginPullRequest",
    ));

    let load = syncs.activity().unwrap().load.unwrap();
    // The batch took 45 and the 24 newest of the queue, and the Origin PR no longer waits behind them.
    assert_eq!((load.prs_fetched, load.prs_known), (26, 26));
    harness.finish();
}

#[test]
fn an_origin_that_names_no_pr_is_asked_for_once() {
    let at = hours_ago(1);
    let mut replies = up_to_the_queue();
    replies.push(empty_window_page());
    replies.push(window_page(&[window_pr(9, &at)], None, LOTS));
    replies.push(fetched_nodes(&[backport(9, 7)], LOTS));
    replies.push(crate::sync::testing::origin_answer(&Value::Null, LOTS));
    let harness = Harness::with_window("queue-no-origin", replies);
    let mut syncs = syncing();

    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(harness.numbers(), [9]);
    harness.finish();
}

#[test]
fn a_frozen_origin_still_brings_the_issue_it_names() {
    let at = hours_ago(1);
    let on_old_issue = pr_node(
        8,
        &json!({
            "title": "gh-900: Fix it",
            "state": "MERGED",
            "mergeCommit": { "oid": "a".repeat(40) },
        }),
    );
    let mut replies = up_to_the_queue();
    replies.push(empty_window_page());
    replies.push(window_page(&[window_pr(8, &at)], None, LOTS));
    replies.push(fetched_nodes(&[on_old_issue], LOTS));
    replies.push(issues(&[900], LOTS));
    replies.extend(searches_finding(&[]));
    replies.push(empty_window_page());
    replies.push(window_page(&[window_pr(9, &at)], None, LOTS));
    replies.push(fetched_nodes(&[backport(9, 8)], LOTS));
    replies.push(issues(&[900], LOTS));
    let harness = Harness::with_window("queue-frozen-origin", replies);
    let mut syncs = syncing();
    drop(run_to_the_end(&mut syncs, &harness));
    drop(harness.operations());

    harness.forget_the_last_sync();
    syncs.add(plain(cpython()));
    drop(run_to_the_end(&mut syncs, &harness));

    let operations = harness.operations();
    assert_eq!(operations.last().unwrap(), "FetchIssues");
    assert!(
        !operations
            .iter()
            .any(|name| name == "FetchOriginPullRequest")
    );
    harness.finish();
}

#[test]
fn opening_an_issue_of_another_repository_changes_nothing() {
    let harness = Harness::with_window("queue-open-other", up_to_the_queue());
    let mut syncs = syncing();

    syncs.open_issue(
        &crate::repository::RepositoryName::parse("python/peps").unwrap(),
        11,
    );
    syncs.open_issue(&cpython(), 11);

    assert_eq!(harness.operations(), Vec::<String>::new());
    harness.finish();
}

#[test]
fn a_pr_on_an_issue_outside_the_window_brings_that_issue() {
    let at = hours_ago(1);
    let fresh_on_old = pr_node(30, &json!({ "title": "gh-900: Fix it", "updatedAt": at }));
    let mut replies = up_to_the_queue();
    replies.push(empty_window_page());
    replies.push(window_page(&[window_pr(30, &at)], None, LOTS));
    replies.push(fetched_nodes(&[fresh_on_old], LOTS));
    replies.push(issues(&[900], LOTS));
    let harness = Harness::with_window("queue-old-issue", replies);
    let mut syncs = syncing();

    drop(run_to_the_end(&mut syncs, &harness));

    let held: Vec<u32> = harness
        .store
        .issues(&cpython())
        .unwrap()
        .iter()
        .map(|issue| issue.number)
        .collect();
    assert_eq!(held, [900]);
    harness.finish();
}

#[test]
fn an_issue_a_later_page_brings_isnt_asked_for_again() {
    let at = hours_ago(1);
    let names_later = pr_node(20, &json!({ "title": "gh-11: Fix it" }));
    let mut replies = up_to_the_queue();
    replies.push(window_page(
        &[window_issue_referencing(10, &at, &[20])],
        Some("more"),
        LOTS,
    ));
    replies.push(fetched_nodes(&[names_later], LOTS));
    replies.push(window_page(&[window_issue(11, &at)], None, LOTS));
    replies.push(empty_window_page());
    let harness = Harness::with_window("queue-later-issue", replies);
    let mut syncs = syncing();

    drop(run_to_the_end(&mut syncs, &harness));

    assert!(
        !harness
            .operations()
            .iter()
            .any(|name| name == "FetchIssues")
    );
    harness.finish();
}

fn backport(number: u32, origin: u32) -> Value {
    pr_node(
        number,
        &json!({
            "title": format!("[3.13] Fix it (GH-{origin})"),
            "baseRefName": "3.13",
            "author": { "login": "miss-islington" },
        }),
    )
}

#[test]
fn an_urgent_backport_gets_its_origin_before_the_window() {
    let mut replies = up_to_the_queue_with_an_issue();
    replies.push(issue_references(&[(1, &[5])], LOTS));
    replies.push(fetched_nodes(&[backport(5, 8)], LOTS));
    replies.push(crate::sync::testing::origin_answer(&merged(8), LOTS));
    replies.push(empty_window_page());
    replies.push(empty_window_page());
    let harness = Harness::with_window("queue-urgent-origin", replies);
    let mut syncs = syncing();

    drop(run_to_the_end(&mut syncs, &harness));

    let operations = queue_operations(&harness);
    assert_eq!(operations[4], "FetchOriginPullRequest");
    assert_eq!(operations[5], "WindowIssues");
    harness.finish();
}

#[test]
fn another_backports_origin_waits_for_the_lists_and_often_needs_no_query() {
    let at = hours_ago(1);
    let mut replies = up_to_the_queue();
    replies.push(window_page(
        &[window_issue_referencing(10, &at, &[5])],
        None,
        LOTS,
    ));
    replies.push(fetched_nodes(&[backport(5, 8)], LOTS));
    replies.push(window_page(
        &[window_pr(8, &at), window_pr(9, &at)],
        None,
        LOTS,
    ));
    replies.push(fetched_nodes(&[merged(8), backport(9, 7)], LOTS));
    replies.push(crate::sync::testing::origin_answer(&merged(7), LOTS));
    let harness = Harness::with_window("queue-late-origin", replies);
    let mut syncs = syncing();

    drop(run_to_the_end(&mut syncs, &harness));

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    let origins: Vec<&Value> = asked
        .iter()
        .filter(|(name, _)| name == "FetchOriginPullRequest")
        .map(|(_, sent)| &sent["number"])
        .collect();
    assert_eq!(origins, [&json!(7)]);
    assert_eq!(asked.last().unwrap().0, "FetchOriginPullRequest");
    harness.finish();
}

/// A first sync that leaves merged PR #5 frozen, then a second that lists it updated at `listed_at`.
fn a_frozen_pr_listed_at(
    name: &str,
    listed_at: &str,
    then: Vec<Result<Reply, Unreachable>>,
) -> Vec<(String, Value)> {
    let at = "2026-11-04T10:00:00Z";
    let mut replies = up_to_the_queue_with_an_issue();
    replies.push(issue_references(&[(1, &[5])], LOTS));
    replies.push(fetched_nodes(
        &[pr_node(
            5,
            &json!({
                "title": "Fix it",
                "state": "MERGED",
                "updatedAt": at,
                "mergeCommit": { "oid": "a".repeat(40) },
            }),
        )],
        LOTS,
    ));
    replies.push(empty_window_page());
    replies.push(empty_window_page());
    replies.extend(searches_finding(&[]));
    replies.push(window_page(
        &[window_issue_referencing(10, &hours_ago(1), &[5])],
        None,
        LOTS,
    ));
    replies.push(window_page(&[window_pr(5, listed_at)], None, LOTS));
    replies.extend(then);
    let harness = Harness::with_window(name, replies);
    let mut syncs = syncing();
    drop(run_to_the_end(&mut syncs, &harness));
    harness.forget_the_last_sync();
    let mut again = syncing();
    drop(run_to_the_end(&mut again, &harness));
    let asked = harness.asked.try_iter().collect();
    harness.finish();
    asked
}

#[test]
fn a_frozen_pr_that_hasnt_moved_isnt_fetched_again() {
    let asked = a_frozen_pr_listed_at("queue-frozen", &hours_ago(9000), Vec::new());
    assert_eq!(fetched_ids(&asked), [json!(["id5"])]);
}

#[test]
fn a_frozen_pr_that_moved_is_fetched_again() {
    let moved = hours_ago(1);
    let then = vec![fetched_nodes(&[merged(5)], LOTS)];
    let asked = a_frozen_pr_listed_at("queue-frozen-moved", &moved, then);
    assert_eq!(fetched_ids(&asked), [json!(["id5"]), json!(["id5"])]);
}

#[test]
fn the_status_bar_hears_how_far_the_queue_has_got() {
    let at = hours_ago(1);
    let mut page = window_page(
        &[window_issue_referencing(10, &at, &[20, 21])],
        Some("more"),
        LOTS,
    );
    if let Ok(reply) = &mut page {
        reply.body = reply
            .body
            .replace(r#""totalCount":1"#, r#""totalCount":40"#);
    }
    let mut replies = up_to_the_queue();
    replies.push(page);
    let harness = Harness::with_window("queue-progress", replies);
    let mut syncs = syncing();
    assert_eq!(syncs.activity().unwrap().load, None);

    drop(run_until_asked(&mut syncs, &harness, "WindowIssues"));

    let load = syncs.activity().unwrap().load.unwrap();
    assert_eq!(load.issues_read, 1);
    assert_eq!(load.issues_total, Some(40));
    assert_eq!((load.prs_fetched, load.prs_known), (0, 2));
    harness.finish();
}

#[test]
fn the_status_bar_says_trees_while_the_users_own_come_first() {
    let mut replies = up_to_the_queue_with_an_issue();
    replies.push(issue_references(&[(1, &[5])], LOTS));
    let harness = Harness::with_window("queue-part", replies);
    let mut syncs = syncing();

    drop(run_until_asked(
        &mut syncs,
        &harness,
        "FetchIssueReferences",
    ));

    assert_eq!(
        syncs.activity().unwrap().part,
        crate::status::SyncPart::Trees
    );
    harness.finish();
}

#[test]
fn quitting_resumes_the_queue_without_fetching_a_pr_twice() {
    let at = hours_ago(1);
    let mut replies = up_to_the_queue();
    replies.push(window_page(
        &[window_issue_referencing(10, &at, &[20])],
        Some("more"),
        LOTS,
    ));
    replies.push(fetched_nodes(&[merged(20)], LOTS));
    replies.push(window_page(
        &[window_issue_referencing(11, &at, &[20, 21])],
        None,
        LOTS,
    ));
    replies.push(fetched_nodes(&[merged(21)], LOTS));
    replies.push(empty_window_page());
    let harness = Harness::with_window("queue-quit", replies);
    let mut quitting = syncing();
    drop(run_until_asked(
        &mut quitting,
        &harness,
        "FetchTreePullRequests",
    ));
    drop(quitting);

    let mut back = syncing();
    drop(run_to_the_end(&mut back, &harness));

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(asked[0].1["after"], json!("more"));
    assert_eq!(fetched_ids(&asked), [json!(["id21"])]);
    harness.finish();
}

#[test]
fn the_window_asks_for_the_last_thirty_days_of_issues() {
    let mut replies = up_to_the_queue();
    replies.push(empty_window_page());
    replies.push(empty_window_page());
    let harness = Harness::with_window("queue-since", replies);
    let mut syncs = syncing();

    drop(run_to_the_end(&mut syncs, &harness));

    let asked: Vec<(String, Value)> = harness
        .asked
        .try_iter()
        .filter(|(operation, _)| operation == "WindowIssues")
        .collect();
    let since = asked[0].1["since"].as_str().unwrap();
    let since =
        OffsetDateTime::parse(since, &time::format_description::well_known::Rfc3339).unwrap();
    let age = OffsetDateTime::now_utc() - since;
    assert!(age >= Duration::days(30) && age < Duration::days(30) + Duration::minutes(1));
    harness.finish();
}

#[test]
fn a_pr_older_than_the_window_ends_its_list_and_stays_out() {
    let mut replies = up_to_the_queue();
    replies.push(empty_window_page());
    replies.push(window_page(
        &[
            window_pr(21, &hours_ago(2)),
            window_pr(22, "2020-01-01T10:00:00Z"),
        ],
        Some("older"),
        LOTS,
    ));
    replies.push(fetched_nodes(&[merged(21)], LOTS));
    let harness = Harness::with_window("queue-older", replies);
    let mut syncs = syncing();

    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(harness.numbers(), [21]);
    harness.finish();
}

/// Quits after the first page, lets `change` happen, and answers with what the next run asks.
fn asked_after_coming_back(name: &str, change: impl Fn(&Harness)) -> Vec<String> {
    let harness = Harness::with_window(name, searches_finding(&[]));
    let mut quitting = syncing();
    drop(quitting.step(&harness.github, &harness.store).unwrap());
    change(&harness);

    let mut back = syncing();
    drop(back.step(&harness.github, &harness.store).unwrap());

    let operations = harness.operations();
    harness.finish();
    operations
}

const STARTED_OVER: [&str; 2] = ["SearchReviewRequests", "SearchReviewRequests"];

#[test]
fn a_place_goes_on_when_nothing_has_changed() {
    let asked = asked_after_coming_back("window-same", |_| {});
    assert_eq!(asked, ["SearchReviewRequests", "SearchReviewedPrs"]);
}

#[test]
fn signing_in_as_someone_else_starts_the_sync_over() {
    let asked = asked_after_coming_back("window-account", |harness| {
        let other = Login::new("another-account");
        harness.store.remember_login(&other).unwrap();
    });
    assert_eq!(asked, STARTED_OVER);
}

#[test]
fn a_place_from_another_version_starts_the_sync_over() {
    let asked = asked_after_coming_back("window-version", |harness| {
        let place = harness.store.first_sync_place(&cpython()).unwrap().unwrap();
        let version = env!("CARGO_PKG_VERSION");
        let older = place.as_str().replace(version, "0.0.0-older");
        assert_ne!(older, place.as_str());
        harness
            .store
            .save_first_sync_place(&cpython(), FirstSyncPlace::new(older))
            .unwrap();
    });
    assert_eq!(asked, STARTED_OVER);
}

#[test]
fn editing_the_filters_starts_the_sync_over() {
    let harness = Harness::with_window("window-filters", searches_finding(&[]));
    let mut quitting = syncing();
    drop(quitting.step(&harness.github, &harness.store).unwrap());

    let mut back = Syncs::default();
    back.add(RepositorySettings::new(cpython()));
    drop(back.step(&harness.github, &harness.store).unwrap());

    assert_eq!(harness.operations(), STARTED_OVER);
    harness.finish();
}

#[test]
fn the_window_tells_a_screen_of_each_page_that_saved_something() {
    let at = hours_ago(1);
    let mut replies = up_to_the_queue();
    replies.push(window_page(&[window_issue(1, &at)], Some("more"), LOTS));
    replies.push(window_page(&[window_issue(3, &at)], None, LOTS));
    replies.push(window_page(&[window_pr(2, &at)], None, LOTS));
    replies.push(fetched_nodes(&[merged(2)], LOTS));
    let harness = Harness::with_window("window-news", replies);
    let mut syncs = syncing();

    let mut told = Vec::new();
    while syncs.ready_at().is_some() {
        if let Some(Progress::Changed(change)) =
            syncs.step(&harness.github, &harness.store).unwrap()
        {
            told.push(change.items);
        }
    }

    assert_eq!(told, [[1], [3], [2]]);
    harness.finish();
}

#[test]
fn a_window_page_below_the_floor_holds_the_next_step_back() {
    let mut replies = up_to_the_queue();
    replies.push(window_page(
        &[window_issue(1, &hours_ago(1))],
        Some("more"),
        FEW,
    ));
    let harness = Harness::with_window("window-floor", replies);
    let mut syncs = syncing();
    let before = std::time::Instant::now();

    drop(run_until_asked(&mut syncs, &harness, "WindowIssues"));

    let ready = syncs.ready_at().unwrap();
    assert!(ready > before + std::time::Duration::from_secs(3000));
    harness.finish();
}

#[test]
fn a_first_sync_taken_up_again_keeps_when_it_started() {
    let long_ago = OffsetDateTime::now_utc() - Duration::days(2);
    let harness = Harness::with_window("window-started", up_to_the_queue());
    let mut first = UserWorkSync::new(plain(cpython()));
    first.started_at = long_ago;
    drop(first.step(&harness.github, &harness.store).unwrap());

    let mut taken_up = UserWorkSync::new(plain(cpython()));
    drop(taken_up.step(&harness.github, &harness.store).unwrap());

    assert_eq!(taken_up.started_at, long_ago);
    assert_eq!(taken_up.first_sync.map(|sync| sync.pages), Some(2));
    harness.finish();
}

#[test]
fn a_finished_sync_has_no_place_to_come_back_to() {
    let mut replies = up_to_the_queue();
    replies.push(empty_window_page());
    replies.push(empty_window_page());
    let harness = Harness::with_window("window-finished", replies);
    let mut syncs = syncing();
    run_steps(&mut syncs, &harness, 1);
    assert!(
        harness
            .store
            .first_sync_place(&cpython())
            .unwrap()
            .is_some()
    );

    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(harness.store.first_sync_place(&cpython()).unwrap(), None);
    harness.finish();
}

#[test]
fn a_place_it_cant_read_starts_the_sync_over() {
    let mut replies = up_to_the_queue();
    replies.push(empty_window_page());
    replies.push(empty_window_page());
    let harness = Harness::with_window("window-unreadable", replies);
    harness
        .store
        .save_first_sync_place(&cpython(), FirstSyncPlace::new("{\"from\":0}".to_owned()))
        .unwrap();
    let mut syncs = syncing();

    drop(run_to_the_end(&mut syncs, &harness));

    let operations = harness.operations();
    assert_eq!(operations[0], "SearchReviewRequests");
    assert!(harness.store.sync_state(&cpython()).unwrap().is_some());
    harness.finish();
}

#[test]
fn the_queue_keeps_what_happened_on_its_items() {
    let at = hours_ago(1);
    let mut issue = window_issue_referencing(1, &at, &[2]);
    issue["timelineItems"] = json!({ "nodes": [comment_by("issue-author", &at)] });
    let mut pr = merged(2);
    pr["timelineItems"] = json!({ "nodes": [comment_by("some-author", &at)] });
    let mut replies = up_to_the_queue();
    replies.push(window_page(&[issue], None, LOTS));
    replies.push(fetched_nodes(&[pr], LOTS));
    replies.push(empty_window_page());
    let harness = Harness::with_window("window-activity", replies);
    let mut syncs = syncing();

    drop(run_to_the_end(&mut syncs, &harness));

    let who = |number: u32| -> Vec<String> {
        let events = harness.store.activity(&cpython(), number).unwrap();
        events
            .iter()
            .map(|event| event.who.as_str().to_owned())
            .collect()
    };
    assert_eq!(who(1), ["issue-author"]);
    assert_eq!(who(2), ["some-author"]);
    harness.finish();
}

#[test]
fn a_step_whose_place_cant_be_saved_keeps_its_news_for_the_next_try() {
    let mut replies = up_to_the_queue();
    replies.push(window_page(&[window_issue(1, &hours_ago(1))], None, LOTS));
    let harness = Harness::with_window("window-unsaved", replies);
    let mut sync = UserWorkSync::new(plain(cpython()));
    // Four searches and the moves between them leave the page to read.
    while !matches!(
        sync.phase,
        crate::sync::Phase::Resumable(crate::sync::Resumable::Loading(_))
    ) {
        drop(sync.step(&harness.github, &harness.store).unwrap());
    }
    // Reading the frozen PRs and this page's save land, but saving its place doesn't.
    let (saves_once, _calls) = harness.store.failing_at_call(Some(2));

    let failed = sync.step(&harness.github, &saves_once);
    let again = sync.step(&harness.github, &saves_once).unwrap();

    saves_once.close();
    assert!(failed.is_err());
    assert!(matches!(again, Step::Saved { .. }));
    assert_eq!(harness.operations().len(), 5);
    harness.finish();
}
