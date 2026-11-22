use super::*;
use crate::error::ErrorKind;
use crate::facts::WorkFacts;
use crate::facts::activity::{Activity, ActivityKind, ItemKey, PutAway};
use crate::facts::filters::FilterProblem;
use crate::facts::issues::IssueState;
use crate::facts::labels::Label;
use crate::facts::pull_requests::{CiState, ReviewState};
use crate::filters::{Filter, OnWorkbench};
use crate::github::testing::{
    answer, dated, erring, filter_page, github, not_found, page_info, pr_content,
    pull_request_answer, replied, reply_ok, signed_in_as,
};
use crate::github::{ConditionalGet, ConditionalReply, Reply, Request, Transport, Unreachable};
use crate::repository::RepositoryName;
use crate::repository_settings::RepositorySettings;
use crate::settings::ListOrder;
use crate::status::Change;
use crate::store::testing::seeing;
use crate::store::{Archiving, Muting, Store};
use crate::sync::recovery::{Scenario, recovers_from_each_failure};
use crate::sync::testing::{
    FEW, Harness, RATE_LIMIT, cpython, cross_references, fetched_nodes, issues,
    no_cross_references, pr_node, searches_finding,
};
use crate::sync::{Progress, Syncs};
use crate::testing::{issue_facts, me, moment};
use crate::token::Token;
use crate::work::Why;
use crate::workbench::inbox::{Inbox, InboxPage};
use crate::workbench::{Hidden, ListedItem, Workbench, WorkbenchView};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::mpsc::{self, Sender};
use time::format_description::well_known::Rfc3339;
use tsugiki_test_git::pass_on;

const LAST_SYNC: &str = "2026-11-04T10:00:00Z";
const AFTER_IT: &str = "2026-11-04T10:30:00Z";
const LONG_BEFORE: &str = "2026-11-01T10:00:00Z";

fn filtering(queries: &[&str]) -> RepositorySettings {
    RepositorySettings {
        filters: queries
            .iter()
            .map(|text| Filter {
                name: format!("Named {text}").try_into().unwrap(),
                query: query(text),
                on_workbench: OnWorkbench::On,
            })
            .collect(),
        ..RepositorySettings::new(cpython())
    }
}

fn query(text: &str) -> FilterQuery {
    text.to_owned().try_into().unwrap()
}

/// A harness whose Repository finished a sync at `LAST_SYNC`, and whose user is `octocat`.
fn after_a_sync(name: &str, replies: Vec<Result<Reply, Unreachable>>) -> Harness {
    let harness = Harness::new(name, replies);
    synced(&harness, ChangeCheck::Skipped);
    harness.db.store.remember_login(&me()).unwrap();
    harness
}

fn synced(harness: &Harness, check: ChangeCheck) {
    let state = SyncState {
        started_at: moment(LAST_SYNC),
        finished_at: moment(LAST_SYNC),
        check,
        refresh_under_way: false,
    };
    harness.db.store.save_sync_state(&cpython(), state).unwrap();
}

fn holding_issue(harness: &Harness, number: u32, updated_at: &str) {
    let issue = IssueFacts {
        updated_at: moment(updated_at),
        ..issue_facts(number)
    };
    harness
        .db
        .store
        .save_issues(&cpython(), vec![issue])
        .unwrap();
}

/// PR `number` as a fetch would read it from `node`.
fn read_pr(node: &Value) -> Vec<PullRequestFacts> {
    read_pr_and_events(node).0
}

/// PR `number` as a fetch would read it from `node`, with the events of its timeline.
fn read_pr_and_events(node: &Value) -> (Vec<PullRequestFacts>, Vec<ItemActivity>) {
    let script = fetched_nodes(std::slice::from_ref(node), RATE_LIMIT);
    let (script, _asked) = crate::github::testing::Scripted::new(vec![script]);
    let github = github(script);
    let id: NodeId = serde_json::from_value(json!("id")).unwrap();
    let batch = github
        .fetch_pull_requests(PrFetch::Work, &[id], 25)
        .unwrap();
    (batch.pull_requests, batch.activity)
}

/// Saves the PR a fetch would read from `node`.
fn holding_pr(harness: &Harness, node: &Value) {
    harness
        .db
        .store
        .save_pull_requests(&cpython(), read_pr(node))
        .unwrap();
}

/// Saves the PR a fetch would read from `node` as a refresh that found it changed saves it, then stops
/// before it reads the PR's Conversation.
fn saved_by_a_stopped_refresh(harness: &Harness, node: &Value) {
    let pull_requests = read_pr(node);
    let changed = pull_requests.iter().map(|pr| pr.number).collect();
    harness
        .db
        .store
        .save_changed_pull_requests(&cpython(), (pull_requests, vec![]), changed)
        .unwrap();
    let state = SyncState {
        started_at: moment(LAST_SYNC),
        finished_at: moment(LAST_SYNC),
        check: ChangeCheck::Skipped,
        refresh_under_way: true,
    };
    harness.db.store.save_sync_state(&cpython(), state).unwrap();
}

fn own_open_pr(number: u32, changes: &Value) -> Value {
    pr_node(number, changes)
}

fn strangers_pr(number: u32, updated_at: &str) -> Value {
    pr_node(
        number,
        &json!({ "author": { "login": "some-author" }, "updatedAt": updated_at, "title": "A change" }),
    )
}

fn changed_pr(number: u32, updated_at: &str) -> Value {
    json!({ "id": format!("id{number}"), "number": number, "updatedAt": updated_at })
}

fn issue_node(number: u32, updated_at: &str) -> Value {
    json!({
        "number": number,
        "title": format!("Issue {number} now"),
        "updatedAt": updated_at,
        "state": "OPEN",
        "stateReason": null,
        "assignees": { "nodes": [] },
        "labels": { "nodes": [] },
        "timelineItems": { "nodes": [] },
        "author": null,
    })
}

fn list(nodes: &[Value], next: Option<&str>) -> Value {
    json!({ "pageInfo": page_info(next), "nodes": nodes })
}

fn search(nodes: &[Value], next: Option<&str>) -> Value {
    let mut answer = list(nodes, next);
    answer["issueCount"] = json!(nodes.len());
    answer
}

/// GitHub's answer to a Filter's search on its own, which found `nodes`.
fn searched(nodes: &[Value], next: Option<&str>) -> Result<Reply, Unreachable> {
    filter_page(nodes.len(), nodes, next, RATE_LIMIT)
}

fn issue_hit(number: u32) -> Value {
    json!({
        "__typename": "Issue",
        "number": number,
        "updatedAt": AFTER_IT,
        "repository": { "nameWithOwner": "python/cpython" },
    })
}

/// An answer to a poll of a large Repository. `searches` holds one answer for each field `f<position>`.
fn polled(issues: &Value, prs: &Value, searches: &[Value]) -> Result<Reply, Unreachable> {
    polled_with_open(9_000, issues, prs, searches)
}

fn polled_with_open(
    open: u32,
    issues: &Value,
    prs: &Value,
    searches: &[Value],
) -> Result<Reply, Unreachable> {
    let mut data = json!({ "repository": {
        "openIssues": { "totalCount": open },
        "openPrs": { "totalCount": 0 },
        "issues": issues,
        "pullRequests": prs,
    } });
    for (position, found) in searches.iter().enumerate() {
        data[format!("f{position}")] = found.clone();
    }
    answer(data, RATE_LIMIT)
}

fn quiet_poll() -> Result<Reply, Unreachable> {
    polled(&list(&[], None), &list(&[], None), &[])
}

/// An answer to a probe: the PRs numbered here, with these CI states.
fn probed(states: &[(u32, &str)]) -> Result<Reply, Unreachable> {
    let fields: serde_json::Map<String, Value> = states
        .iter()
        .map(|(number, state)| {
            let probe = json!({
                "number": number,
                "reviewRequests": { "nodes": [] },
                "timelineItems": { "nodes": [] },
                "latestReviews": { "nodes": [] },
                "lastCommit": { "nodes": [
                    { "commit": { "statusCheckRollup": { "state": state } } }
                ] },
            });
            (format!("p{number}"), probe)
        })
        .collect();
    answer(json!({ "repository": fields }), RATE_LIMIT)
}

/// GitHub's answer about the checks of PR `number`, of which `names` failed.
fn failed_checks(number: u32, names: &[&str]) -> Result<Reply, Unreachable> {
    let runs: Vec<Value> = names
        .iter()
        .map(|name| {
            json!({ "__typename": "CheckRun", "name": name, "conclusion": "FAILURE",
                            "checkSuite": null })
        })
        .collect();
    let pull_request = json!({
        "number": number,
        "lastCommit": { "nodes": [
            { "commit": { "statusCheckRollup": { "contexts": { "nodes": runs } } } }
        ] },
    });
    answer(
        json!({ "repository": { format!("p{number}"): pull_request } }),
        RATE_LIMIT,
    )
}

/// Runs the refresh to its end, and answers with everything it announced.
fn run(syncs: &mut Syncs, harness: &Harness) -> Vec<Change> {
    let mut changes = Vec::new();
    while syncs.ready_at().is_some() {
        if let Some(Progress::Changed(change)) =
            syncs.step(&harness.github, &harness.db.store).unwrap()
        {
            changes.push(change);
        }
    }
    changes
}

fn refreshing(settings: RepositorySettings) -> Syncs {
    let mut syncs = Syncs::default();
    syncs.add_listed(settings);
    syncs
}

fn facts(harness: &Harness) -> WorkFacts {
    harness.db.reader.work_facts(&cpython()).unwrap()
}

/// A Repository that finished a sync, holding what a poll and a probe can move.
fn holding_what_can_move(harness: &Harness) {
    synced(harness, ChangeCheck::Skipped);
    harness.db.store.remember_login(&me()).unwrap();
    holding_issue(harness, 1, LONG_BEFORE);
    holding_pr(harness, &strangers_pr(3, LONG_BEFORE));
    holding_pr(harness, &own_open_pr(4, &json!({})));
    stored_filter(harness, "a", &[1, 9]);
    stored_filter(harness, "b", &[]);
    stored_filter(harness, "c", &[]);
}

#[test]
fn a_refresh_recovers_from_a_failure_at_each_point() {
    let posts = vec![
        polled(
            &list(&[issue_node(1, AFTER_IT)], None),
            &list(&[changed_pr(3, AFTER_IT)], None),
            &[
                search(&[issue_hit(2)], Some("more")),
                search(&[issue_hit(6)], None),
                search(&[issue_hit(8)], None),
            ],
        ),
        fetched_nodes(&[strangers_pr(3, AFTER_IT)], RATE_LIMIT),
        probed(&[(4, "FAILURE")]),
        failed_checks(4, &["Tests"]),
        searched(&[issue_hit(2), issue_hit(5)], None),
        issues(&[2, 5, 6, 8], RATE_LIMIT),
        cross_references(&[7], None, RATE_LIMIT),
        no_cross_references(RATE_LIMIT),
        no_cross_references(RATE_LIMIT),
        no_cross_references(RATE_LIMIT),
        no_cross_references(RATE_LIMIT),
        fetched_nodes(&[strangers_pr(7, AFTER_IT)], RATE_LIMIT),
        approved(),
    ];
    let scenario = Scenario {
        name: "recovery-refresh",
        settings: filtering(&["a", "b", "c"]),
        prepare: holding_what_can_move,
        posts,
        checks: Vec::new(),
    };

    let operations = recovers_from_each_failure(&scenario);

    assert_eq!(
        operations,
        [
            "RefreshRepository",
            "FetchChangedPullRequests",
            "ProbePullRequests",
            "FetchFailedChecks",
            "SearchFilter",
            "FetchIssues",
            "FetchCrossReferences",
            "FetchCrossReferences",
            "FetchCrossReferences",
            "FetchCrossReferences",
            "FetchCrossReferences",
            "FetchTreePullRequests",
            "FetchPullRequestContent",
        ]
    );
}

#[test]
fn a_refresh_that_redoes_an_interrupted_one_recovers_from_a_failure_at_each_point() {
    let posts = vec![
        polled(
            &list(&[issue_node(1, AFTER_IT)], Some("more")),
            &list(&[changed_pr(4, AFTER_IT)], None),
            &[],
        ),
        quiet_poll(),
        fetched_nodes(&[pr_node(4, &json!({ "updatedAt": AFTER_IT }))], RATE_LIMIT),
        issues(&[1], RATE_LIMIT),
        no_cross_references(RATE_LIMIT),
        approved(),
    ];
    let scenario = Scenario {
        name: "recovery-redo",
        settings: filtering(&[]),
        prepare: |harness| {
            let state = SyncState {
                started_at: moment(LAST_SYNC),
                finished_at: moment(LAST_SYNC),
                check: ChangeCheck::Skipped,
                refresh_under_way: true,
            };
            harness.db.store.save_sync_state(&cpython(), state).unwrap();
            harness.db.store.remember_login(&me()).unwrap();
            holding_issue(harness, 1, LONG_BEFORE);
            holding_pr(harness, &own_open_pr(4, &json!({})));
        },
        posts,
        checks: Vec::new(),
    };

    let operations = recovers_from_each_failure(&scenario);

    assert_eq!(
        operations,
        [
            "RefreshRepository",
            "RefreshRepository",
            "FetchChangedPullRequests",
            "FetchIssues",
            "FetchCrossReferences",
            "FetchPullRequestContent",
        ]
    );
}

#[test]
fn a_refresh_of_a_small_repository_recovers_from_a_failure_at_each_point() {
    let scenario = Scenario {
        name: "recovery-small",
        settings: filtering(&["a"]),
        prepare: |harness| {
            synced(harness, ChangeCheck::Tagged(Etag::new("\"same\"")));
            harness.db.store.remember_login(&me()).unwrap();
            holding_pr(harness, &own_open_pr(1, &json!({})));
            stored_filter(harness, "a", &[]);
        },
        posts: vec![
            probed(&[(1, "FAILURE")]),
            failed_checks(1, &["Tests"]),
            approved(),
        ],
        checks: vec![check_reply(304, None)],
    };

    let operations = recovers_from_each_failure(&scenario);

    assert_eq!(operations[1], "ProbePullRequests");
}

#[test]
fn a_later_sync_asks_only_what_changed_since_the_last_one() {
    let harness = after_a_sync("poll-only", vec![quiet_poll()]);
    let mut syncs = refreshing(filtering(&[]));

    let changes = run(&mut syncs, &harness);

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0].0, "RefreshRepository");
    assert_eq!(asked[0].1["since"], "2026-11-04T09:58:00Z");
    assert_eq!(changes, []);
    harness.finish();
}

/// Saves Issue `number` with no state, as the migration that added states left the Issues saved before it.
fn holding_issue_with_no_state(harness: &Harness, number: u32) {
    let issue = IssueFacts {
        updated_at: moment(LONG_BEFORE),
        state: None,
        ..issue_facts(number)
    };
    harness
        .db
        .store
        .save_issues(&cpython(), vec![issue])
        .unwrap();
}

#[test]
fn a_refresh_fetches_the_issues_saved_before_tsugiki_kept_states_once() {
    let harness = after_a_sync(
        "states",
        vec![quiet_poll(), issues(&[1, 2], RATE_LIMIT), quiet_poll()],
    );
    holding_issue_with_no_state(&harness, 1);
    holding_issue_with_no_state(&harness, 2);
    holding_issue(&harness, 3, LONG_BEFORE);
    let mut syncs = refreshing(filtering(&[]));

    let changes = run(&mut syncs, &harness);
    let first = harness.operations();
    syncs.add_listed(filtering(&[]));
    drop(run(&mut syncs, &harness));

    let states: Vec<Option<IssueState>> = harness
        .db
        .store
        .issues(&cpython())
        .unwrap()
        .into_iter()
        .map(|issue| issue.state)
        .collect();
    assert_eq!(first, ["RefreshRepository", "FetchIssues"]);
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!(states, [Some(IssueState::Open); 3]);
    assert_eq!(harness.operations(), ["RefreshRepository"]);
    harness.finish();
}

#[test]
fn an_issue_github_no_longer_has_is_asked_for_once_a_launch() {
    let harness = after_a_sync(
        "states-gone",
        vec![quiet_poll(), issues(&[1], RATE_LIMIT), quiet_poll()],
    );
    holding_issue_with_no_state(&harness, 1);
    holding_issue_with_no_state(&harness, 2);
    let mut syncs = refreshing(filtering(&[]));

    drop(run(&mut syncs, &harness));
    syncs.add_listed(filtering(&[]));
    drop(run(&mut syncs, &harness));

    let states: Vec<Option<IssueState>> = harness
        .db
        .store
        .issues(&cpython())
        .unwrap()
        .into_iter()
        .map(|issue| issue.state)
        .collect();
    assert_eq!(states, [Some(IssueState::Open), None]);
    assert_eq!(
        harness.operations(),
        ["RefreshRepository", "FetchIssues", "RefreshRepository"]
    );
    harness.finish();
}

/// Steps the open Repository's sync until it finishes.
fn until_finished(syncs: &mut Syncs, harness: &Harness) {
    while !matches!(
        syncs.step(&harness.github, &harness.db.store).unwrap(),
        Some(Progress::Finished(_))
    ) {}
}

#[test]
fn a_refresh_goes_before_the_fetch_of_the_issues_with_no_state() {
    let harness = after_a_sync(
        "states-behind",
        vec![quiet_poll(), quiet_poll(), issues(&[1], RATE_LIMIT)],
    );
    holding_issue_with_no_state(&harness, 1);
    let mut syncs = refreshing(filtering(&[]));

    until_finished(&mut syncs, &harness);
    syncs.add_listed(filtering(&[]));
    drop(run(&mut syncs, &harness));

    assert_eq!(
        harness.operations(),
        ["RefreshRepository", "RefreshRepository", "FetchIssues"]
    );
    harness.finish();
}

#[test]
fn a_store_that_cant_say_which_issues_have_no_state_leaves_them_for_the_next_launch() {
    let harness = after_a_sync("states-unread", vec![quiet_poll()]);
    holding_issue_with_no_state(&harness, 1);
    let mut syncs = refreshing(filtering(&[]));
    until_finished(&mut syncs, &harness);
    let stopped = Store::stopped();

    let stepped = syncs.step(&harness.github, &stopped).unwrap();
    stopped.close();

    assert_eq!(stepped, None);
    assert_eq!(syncs.ready_at(), None);
    assert_eq!(harness.operations(), ["RefreshRepository"]);
    harness.finish();
}

#[test]
fn a_finished_refresh_moves_the_last_sync_on_and_a_failed_one_leaves_it() {
    // A request gets a second try at half the size, so a failure takes two.
    let harness = after_a_sync(
        "poll-time",
        vec![
            Err(Unreachable::Offline),
            Err(Unreachable::Offline),
            quiet_poll(),
        ],
    );
    let mut syncs = refreshing(filtering(&[]));

    let failed = syncs.step(&harness.github, &harness.db.store).unwrap_err();
    let during = harness.db.store.sync_state(&cpython()).unwrap().unwrap();
    drop(run(&mut syncs, &harness));
    let after = harness.db.store.sync_state(&cpython()).unwrap().unwrap();

    assert_eq!(failed.problem.kind, ErrorKind::Offline);
    assert_eq!(during.started_at, moment(LAST_SYNC));
    assert!(during.refresh_under_way);
    assert!(after.started_at > moment(LAST_SYNC));
    assert!(!after.refresh_under_way);
    harness.finish();
}

#[test]
fn an_issue_the_poll_changed_is_saved_named_and_gets_its_tree_read() {
    let issues_list = list(&[issue_node(1, AFTER_IT)], None);
    let replies = vec![
        polled(&issues_list, &list(&[], None), &[]),
        cross_references(&[7], None, RATE_LIMIT),
        fetched_nodes(&[strangers_pr(7, AFTER_IT)], RATE_LIMIT),
    ];
    let harness = after_a_sync("issue-moved", replies);
    holding_issue(&harness, 1, LONG_BEFORE);
    let mut syncs = refreshing(filtering(&[]));

    let changes = run(&mut syncs, &harness);

    assert_eq!(
        harness.operations(),
        [
            "RefreshRepository",
            "FetchCrossReferences",
            "FetchTreePullRequests"
        ]
    );
    assert_eq!(facts(&harness).issues[0].title, "Issue 1 now");
    assert_eq!(
        changes.first(),
        Some(&Change {
            repository: cpython(),
            items: vec![1],
        })
    );
    harness.finish();
}

fn label_nodes(names: &[&str]) -> Value {
    let nodes: Vec<Value> = names.iter().map(|name| json!({ "name": name })).collect();
    json!({ "nodes": nodes })
}

fn labels(names: &[&str]) -> Vec<Label> {
    names.iter().map(|name| Label::new(name)).collect()
}

#[test]
fn a_refresh_keeps_the_labels_an_issue_gained_and_drops_those_it_lost() {
    let mut relabelled = issue_node(1, AFTER_IT);
    relabelled["labels"] = label_nodes(&["type-bug", "3.14"]);
    let replies = vec![
        polled(&list(&[relabelled], None), &list(&[], None), &[]),
        cross_references(&[], None, RATE_LIMIT),
    ];
    let harness = after_a_sync("issue-relabelled", replies);
    let held = IssueFacts {
        labels: labels(&["pending", "type-bug"]),
        updated_at: moment(LONG_BEFORE),
        ..issue_facts(1)
    };
    harness
        .db
        .store
        .save_issues(&cpython(), vec![held])
        .unwrap();
    let mut syncs = refreshing(filtering(&[]));

    drop(run(&mut syncs, &harness));

    assert_eq!(
        facts(&harness).issues[0].labels,
        labels(&["3.14", "type-bug"])
    );
    harness.finish();
}

#[test]
fn a_refresh_keeps_the_labels_a_pr_gained_and_drops_those_it_lost() {
    let relabelled = |names: &[&str], updated_at: &str| {
        let mut node = strangers_pr(1, updated_at);
        node["labels"] = label_nodes(names);
        node
    };
    let replies = vec![
        polled(
            &list(&[], None),
            &list(&[changed_pr(1, AFTER_IT)], None),
            &[],
        ),
        fetched_nodes(&[relabelled(&["awaiting merge"], AFTER_IT)], RATE_LIMIT),
    ];
    let harness = after_a_sync("pr-relabelled", replies);
    let held = read_pr(&relabelled(&["awaiting review", "skip news"], LONG_BEFORE));
    harness
        .db
        .store
        .save_pull_requests(&cpython(), held)
        .unwrap();
    let mut syncs = refreshing(filtering(&[]));

    drop(run(&mut syncs, &harness));

    assert_eq!(
        facts(&harness).pull_requests[0].labels,
        labels(&["awaiting merge"])
    );
    harness.finish();
}

#[test]
fn an_issue_nothing_shows_stays_out_and_an_unchanged_one_says_nothing() {
    let issues_list = list(&[issue_node(1, AFTER_IT), issue_node(2, LONG_BEFORE)], None);
    let harness = after_a_sync(
        "issue-ignored",
        vec![polled(&issues_list, &list(&[], None), &[])],
    );
    let same = IssueFacts {
        title: "Issue 2 now".to_owned(),
        updated_at: moment(LONG_BEFORE),
        ..issue_facts(2)
    };
    harness
        .db
        .store
        .save_issues(&cpython(), vec![same])
        .unwrap();
    let mut syncs = refreshing(filtering(&[]));

    let changes = run(&mut syncs, &harness);

    assert_eq!(harness.operations(), ["RefreshRepository"]);
    assert_eq!(facts(&harness).issues.len(), 1);
    assert_eq!(changes, []);
    harness.finish();
}

#[test]
fn a_long_list_of_changes_reads_each_page_with_its_cursor() {
    let replies = vec![
        polled(
            &list(&[issue_node(1, AFTER_IT)], Some("issues")),
            &list(&[changed_pr(5, AFTER_IT)], Some("prs")),
            &[],
        ),
        polled(
            &list(&[], None),
            &list(&[changed_pr(6, AFTER_IT)], None),
            &[],
        ),
        fetched_nodes(
            &[strangers_pr(5, AFTER_IT), strangers_pr(6, AFTER_IT)],
            RATE_LIMIT,
        ),
    ];
    let harness = after_a_sync("pages", replies);
    let mut syncs = refreshing(filtering(&[]));

    drop(run(&mut syncs, &harness));

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(asked[1].1["issuesAfter"], "issues");
    assert_eq!(asked[1].1["prsAfter"], "prs");
    assert_eq!(asked[2].0, "FetchChangedPullRequests");
    assert_eq!(asked[2].1["ids"], json!(["id5", "id6"]));
    harness.finish();
}

#[test]
fn a_stale_pr_is_fetched_and_a_current_or_merged_one_isnt() {
    let replies = vec![
        polled(
            &list(&[], None),
            &list(
                &[
                    changed_pr(1, AFTER_IT),
                    changed_pr(2, LAST_SYNC),
                    changed_pr(3, AFTER_IT),
                    changed_pr(4, AFTER_IT),
                ],
                None,
            ),
            &[],
        ),
        fetched_nodes(
            &[
                strangers_pr(1, AFTER_IT),
                strangers_pr(3, AFTER_IT),
                strangers_pr(4, AFTER_IT),
            ],
            RATE_LIMIT,
        ),
    ];
    let harness = after_a_sync("stale", replies);
    holding_pr(&harness, &strangers_pr(1, LONG_BEFORE));
    holding_pr(&harness, &strangers_pr(2, LAST_SYNC));
    let merged = json!({
        "author": { "login": "some-author" },
        "state": "MERGED",
        "mergeCommit": { "oid": "0".repeat(40) },
        "updatedAt": LONG_BEFORE,
    });
    holding_pr(&harness, &pr_node(3, &merged));
    let mut syncs = refreshing(filtering(&[]));

    let changes = run(&mut syncs, &harness);

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(asked[1].1["ids"], json!(["id1", "id3", "id4"]));
    let touched: Vec<u32> = changes
        .into_iter()
        .flat_map(|change| change.items)
        .collect();
    assert_eq!(touched, [1, 3, 4]);
    harness.finish();
}

#[test]
fn a_fetch_that_brings_what_tsugiki_holds_says_nothing() {
    let replies = vec![
        polled(
            &list(&[], None),
            &list(&[changed_pr(1, AFTER_IT)], None),
            &[],
        ),
        fetched_nodes(&[strangers_pr(1, LONG_BEFORE)], RATE_LIMIT),
    ];
    let harness = after_a_sync("same-again", replies);
    holding_pr(&harness, &strangers_pr(1, LONG_BEFORE));
    let mut syncs = refreshing(filtering(&[]));

    let changes = run(&mut syncs, &harness);

    assert_eq!(changes, []);
    harness.finish();
}

#[test]
fn a_fetch_older_than_what_tsugiki_holds_says_nothing_and_owes_no_read() {
    let replies = vec![
        polled(
            &list(&[], None),
            &list(&[changed_pr(1, AFTER_IT)], None),
            &[],
        ),
        // A replica of GitHub's that lags.
        fetched_nodes(&[strangers_pr(1, LONG_BEFORE)], RATE_LIMIT),
    ];
    let harness = after_a_sync("older-again", replies);
    holding_pr(&harness, &strangers_pr(1, LAST_SYNC));
    let mut syncs = refreshing(filtering(&[]));

    let changes = run(&mut syncs, &harness);

    assert_eq!(changes, []);
    let owed = harness.db.store.read_ahead_owed(&cpython()).unwrap();
    assert_eq!(owed, BTreeSet::new());
    harness.finish();
}

#[test]
fn the_users_changed_pr_brings_its_issue_and_tree_and_a_strangers_only_its_issue() {
    let replies = vec![
        polled(
            &list(&[], None),
            &list(&[changed_pr(1, AFTER_IT), changed_pr(2, AFTER_IT)], None),
            &[],
        ),
        fetched_nodes(
            &[
                own_open_pr(1, &json!({ "title": "gh-40: Mine", "updatedAt": AFTER_IT })),
                pr_node(
                    2,
                    &json!({
                        "title": "gh-50: Theirs",
                        "author": { "login": "some-author" },
                        "updatedAt": AFTER_IT,
                    }),
                ),
            ],
            RATE_LIMIT,
        ),
        issues(&[40], RATE_LIMIT),
        no_cross_references(RATE_LIMIT),
        issues(&[50], RATE_LIMIT),
        quiet_conversation(),
    ];
    let harness = after_a_sync("whose", replies);
    let mut syncs = refreshing(filtering(&[]));

    drop(run(&mut syncs, &harness));

    assert_eq!(
        harness.operations(),
        [
            "RefreshRepository",
            "FetchChangedPullRequests",
            "FetchIssues",
            "FetchCrossReferences",
            "FetchIssues",
            "FetchPullRequestContent"
        ]
    );
    assert_eq!(harness.issue_numbers(), [40, 50]);
    harness.finish();
}

#[test]
fn github_refusing_to_name_the_failed_checks_leaves_them_for_the_next_refresh() {
    let refused = reply_ok(r#"{"errors":[{"message":"Resource not accessible"}]}"#);
    let replies = vec![quiet_poll(), probed(&[(1, "FAILURE")]), refused, approved()];
    let harness = after_a_sync("checks-refused", replies);
    holding_pr(&harness, &own_open_pr(1, &json!({})));
    let mut syncs = refreshing(filtering(&[]));

    let changes = run(&mut syncs, &harness);

    assert_eq!(
        harness.operations(),
        [
            "RefreshRepository",
            "ProbePullRequests",
            "FetchFailedChecks",
            "FetchPullRequestContent",
        ]
    );
    assert_eq!(
        changes.len(),
        2,
        "the probe's CI, and then the Conversation"
    );
    let unread = harness
        .db
        .store
        .pull_requests_with_unread_checks(&cpython(), &me())
        .unwrap();
    assert_eq!(unread, [1]);
    harness.finish();
}

#[test]
fn a_probe_of_the_users_open_prs_saves_ci_and_names_only_what_moved() {
    let replies = vec![
        quiet_poll(),
        probed(&[(1, "SUCCESS"), (2, "FAILURE")]),
        failed_checks(2, &["Tests", "Docs"]),
        approved(),
    ];
    let harness = after_a_sync("probe", replies);
    let green = json!({
        "lastCommit": { "nodes": [
            { "commit": { "statusCheckRollup": { "state": "SUCCESS" } } }
        ] },
    });
    holding_pr(&harness, &own_open_pr(1, &green));
    holding_pr(&harness, &own_open_pr(2, &green));
    holding_pr(&harness, &strangers_pr(3, LONG_BEFORE));
    let mut syncs = refreshing(filtering(&[]));

    let changes = run(&mut syncs, &harness);

    assert_eq!(
        harness.operations(),
        [
            "RefreshRepository",
            "ProbePullRequests",
            "FetchFailedChecks",
            "FetchPullRequestContent",
        ]
    );
    let read = harness.db.reader.work_facts(&cpython()).unwrap();
    assert_eq!(
        read.activity.failed_checks.get(&2).unwrap(),
        &["Docs", "Tests"]
    );
    // The probe's CI, the names of the checks that failed, and then its Conversation.
    let only_2 = Change {
        repository: cpython(),
        items: vec![2],
    };
    assert_eq!(changes, [only_2.clone(), only_2.clone(), only_2]);
    harness.finish();
}

#[test]
fn a_probe_with_many_prs_runs_in_batches_and_pauses_at_the_floor() {
    let many: Vec<u32> = (1..=26).collect();
    let first: Vec<(u32, &str)> = many
        .iter()
        .take(25)
        .map(|number| (*number, "SUCCESS"))
        .collect();
    let few_left = answer(json!({ "repository": {} }), FEW);
    let replies = vec![quiet_poll(), probed(&first), few_left];
    let harness = after_a_sync("probe-batches", replies);
    for number in &many {
        holding_pr(&harness, &own_open_pr(*number, &json!({})));
    }
    let mut syncs = refreshing(filtering(&[]));

    drop(syncs.step(&harness.github, &harness.db.store).unwrap());
    drop(syncs.step(&harness.github, &harness.db.store).unwrap());
    drop(syncs.step(&harness.github, &harness.db.store).unwrap());
    drop(syncs.step(&harness.github, &harness.db.store).unwrap());
    drop(syncs.step(&harness.github, &harness.db.store).unwrap());
    let paused = syncs.ready_at().unwrap();

    assert_eq!(
        harness.operations(),
        [
            "RefreshRepository",
            "ProbePullRequests",
            "ProbePullRequests"
        ]
    );
    assert!(paused > std::time::Instant::now() + std::time::Duration::from_secs(60));
    harness.finish();
}

#[test]
fn a_pr_the_refresh_just_fetched_skips_the_probe() {
    let replies = vec![
        polled(
            &list(&[], None),
            &list(&[changed_pr(1, AFTER_IT)], None),
            &[],
        ),
        fetched_nodes(
            &[own_open_pr(
                1,
                &json!({ "title": "A change", "updatedAt": AFTER_IT }),
            )],
            RATE_LIMIT,
        ),
        approved(),
    ];
    let harness = after_a_sync("probe-skips", replies);
    let mut syncs = refreshing(filtering(&[]));

    drop(run(&mut syncs, &harness));

    assert_eq!(
        harness.operations(),
        [
            "RefreshRepository",
            "FetchChangedPullRequests",
            "FetchPullRequestContent"
        ]
    );
    harness.finish();
}

/// GitHub's answer about a PR's Conversation: no comments, and an approval from `some-reviewer`.
fn approved() -> Result<Reply, Unreachable> {
    let review = json!({
        "id": "review",
        "author": { "login": "some-reviewer" },
        "state": "APPROVED",
        "submittedAt": AFTER_IT,
        "bodyHTML": "",
        "isMinimized": false,
        "minimizedReason": null,
        "commit": { "oid": "0123abc" },
    });
    let pull_request = pr_content(&json!({ "reviews": { "totalCount": 1, "nodes": [review] } }));
    pull_request_answer(pull_request, RATE_LIMIT)
}

/// GitHub's answer to a read of a PR's Conversation that holds no Review or comment.
fn quiet_conversation() -> Result<Reply, Unreachable> {
    pull_request_answer(pr_content(&json!({})), RATE_LIMIT)
}

/// The states of the Reviews Tsugiki holds of PR `number`, or nothing if it hasn't read its Conversation.
fn reviews_held(harness: &Harness, number: u32) -> Option<Vec<ReviewState>> {
    let content = (harness.db.reader)
        .pull_request_content(&cpython(), number)
        .unwrap();
    content.map(|content| content.reviews.iter().map(|review| review.state).collect())
}

#[test]
fn a_changed_pr_the_users_work_reaches_has_its_reviews_read_before_it_opens() {
    let replies = vec![
        polled(
            &list(&[], None),
            &list(&[changed_pr(1, AFTER_IT), changed_pr(2, AFTER_IT)], None),
            &[],
        ),
        fetched_nodes(
            &[
                own_open_pr(1, &json!({ "title": "A change", "updatedAt": AFTER_IT })),
                strangers_pr(2, AFTER_IT),
            ],
            RATE_LIMIT,
        ),
        approved(),
    ];
    let harness = after_a_sync("read-ahead", replies);
    let mut syncs = refreshing(filtering(&[]));

    let changes = run(&mut syncs, &harness);

    assert_eq!(
        harness.operations(),
        [
            "RefreshRepository",
            "FetchChangedPullRequests",
            "FetchPullRequestContent"
        ]
    );
    assert_eq!(reviews_held(&harness, 1), Some(vec![ReviewState::Approved]));
    assert_eq!(
        reviews_held(&harness, 2),
        None,
        "the user's work doesn't reach a stranger's PR"
    );
    let last = changes.last().map(|change| change.items.clone());
    assert_eq!(last, Some(vec![1]), "an open PR view reads it again");
    harness.finish();
}

#[test]
fn a_pr_github_wont_give_the_conversation_of_leaves_the_rest_to_read() {
    let replies = vec![
        polled(
            &list(&[], None),
            &list(&[changed_pr(1, AFTER_IT), changed_pr(2, AFTER_IT)], None),
            &[],
        ),
        fetched_nodes(
            &[
                own_open_pr(1, &json!({ "title": "A change", "updatedAt": AFTER_IT })),
                own_open_pr(2, &json!({ "title": "Another", "updatedAt": AFTER_IT })),
            ],
            RATE_LIMIT,
        ),
        // The query ran out of its 10 seconds on this PR.
        replied(502, ""),
        approved(),
    ];
    let harness = after_a_sync("read-ahead-refused", replies);
    let mut syncs = refreshing(filtering(&[]));

    drop(run(&mut syncs, &harness));

    assert_eq!(reviews_held(&harness, 1), None);
    assert_eq!(reviews_held(&harness, 2), Some(vec![ReviewState::Approved]));
    let state = harness.db.store.sync_state(&cpython()).unwrap().unwrap();
    assert!(!state.refresh_under_way, "the refresh finished");
    harness.finish();
}

#[test]
fn with_nobody_signed_in_nothing_counts_as_the_users() {
    let replies = vec![
        polled(
            &list(&[], None),
            &list(&[changed_pr(1, AFTER_IT)], None),
            &[],
        ),
        fetched_nodes(
            &[own_open_pr(1, &json!({ "title": "gh-40: A change" }))],
            RATE_LIMIT,
        ),
        issues(&[40], RATE_LIMIT),
    ];
    let harness = Harness::new("nobody", replies);
    synced(&harness, ChangeCheck::Skipped);
    let mut syncs = refreshing(filtering(&[]));

    drop(run(&mut syncs, &harness));

    // The Issue a stored PR names comes whoever is signed in, but with no tree read and nothing read ahead.
    assert_eq!(
        harness.operations(),
        [
            "RefreshRepository",
            "FetchChangedPullRequests",
            "FetchIssues"
        ]
    );
    harness.finish();
}

fn stored_filter(harness: &Harness, text: &str, numbers: &[u32]) {
    let results = crate::facts::filters::FilterResults {
        numbers: numbers.iter().copied().collect(),
        problem: None,
    };
    harness
        .db
        .store
        .save_filter_results(&cpython(), &query(text), results)
        .unwrap();
}

#[test]
fn a_change_too_new_for_the_search_keeps_its_item_in_the_filter() {
    let now = OffsetDateTime::now_utc().format(&Rfc3339).unwrap();
    let replies = vec![
        polled(
            &list(&[issue_node(1, &now)], None),
            &list(&[changed_pr(2, &now)], None),
            &[search(&[], None)],
        ),
        fetched_nodes(&[strangers_pr(2, &now)], RATE_LIMIT),
        approved(),
    ];
    let harness = after_a_sync("too-new", replies);
    stored_filter(&harness, "a", &[1, 2]);
    let mut syncs = refreshing(filtering(&["a"]));

    drop(run(&mut syncs, &harness));

    let results = &facts(&harness).filters[&query("a")];
    assert_eq!(results.numbers, BTreeSet::from([1, 2]));
    harness.finish();
}

#[test]
fn a_filter_switched_on_during_a_refresh_has_it_read_ahead_what_the_filter_reaches() {
    let now = OffsetDateTime::now_utc().format(&Rfc3339).unwrap();
    let replies = vec![
        polled(
            &list(&[], None),
            &list(&[changed_pr(2, &now)], None),
            &[search(&[], None)],
        ),
        fetched_nodes(&[strangers_pr(2, &now)], RATE_LIMIT),
        approved(),
    ];
    let harness = after_a_sync("switched-mid-refresh", replies);
    stored_filter(&harness, "a", &[2]);
    let mut off = filtering(&["a"]);
    off.filters[0].on_workbench = OnWorkbench::Off;
    let mut syncs = refreshing(off);
    drop(syncs.step(&harness.github, &harness.db.store).unwrap());

    syncs.list(filtering(&["a"]));
    drop(run(&mut syncs, &harness));

    assert_eq!(
        harness.operations(),
        [
            "RefreshRepository",
            "FetchChangedPullRequests",
            "FetchPullRequestContent"
        ]
    );
    assert_eq!(reviews_held(&harness, 2), Some(vec![ReviewState::Approved]));
    harness.finish();
}

#[test]
fn a_filter_switched_on_during_a_first_sync_has_the_next_refresh_read_what_the_switch_owed() {
    let hit = json!({
        "__typename": "PullRequest",
        "id": "id2",
        "number": 2,
        "updatedAt": LONG_BEFORE,
        "repository": { "nameWithOwner": "python/cpython" },
    });
    let mut replies = searches_finding(&[]);
    replies.push(searched(&[hit], None));
    replies.push(fetched_nodes(&[strangers_pr(2, LONG_BEFORE)], RATE_LIMIT));
    replies.push(polled(
        &list(&[], None),
        &list(&[], None),
        &[search(&[], None)],
    ));
    replies.push(approved());
    let harness = Harness::new("switched-mid-first-sync", replies);
    harness.db.store.remember_login(&me()).unwrap();
    let mut off = filtering(&["a"]);
    off.filters[0].on_workbench = OnWorkbench::Off;
    let mut syncs = refreshing(off);
    for steps in 0.. {
        if harness.numbers().contains(&2) {
            break;
        }
        assert!(steps < 20, "the first sync never fetched PR 2");
        drop(syncs.step(&harness.github, &harness.db.store).unwrap());
    }
    let first_sync_finished = harness.db.store.sync_state(&cpython()).unwrap();

    // As the session does when the user switches "a" on.
    syncs.list(filtering(&["a"]));
    harness
        .db
        .store
        .owe_read_ahead(&cpython(), vec![2])
        .unwrap();
    drop(run(&mut syncs, &harness));
    syncs.add_listed(filtering(&["a"]));
    drop(run(&mut syncs, &harness));

    assert_eq!(
        first_sync_finished, None,
        "the switch came during the first sync"
    );
    assert_eq!(
        harness.operations()[4..],
        [
            "SearchFilter",
            "FetchPullRequests",
            "RefreshRepository",
            "FetchPullRequestContent"
        ]
    );
    assert_eq!(reviews_held(&harness, 2), Some(vec![ReviewState::Approved]));
    harness.finish();
}

#[test]
fn a_filter_switched_on_while_a_refresh_reads_ahead_has_it_read_what_the_switch_owed() {
    let quiet = polled(&list(&[], None), &list(&[], None), &[search(&[], None)]);
    let harness = after_a_sync(
        "switched-reading-ahead",
        vec![quiet, approved(), approved()],
    );
    holding_pr(&harness, &strangers_pr(2, LONG_BEFORE));
    holding_pr(&harness, &strangers_pr(3, LONG_BEFORE));
    stored_filter(&harness, "a", &[2]);
    stored_filter(&harness, "b", &[3]);
    let mut b_off = filtering(&["a", "b"]);
    b_off.filters[1].on_workbench = OnWorkbench::Off;
    harness
        .db
        .store
        .owe_read_ahead(&cpython(), vec![2])
        .unwrap();
    let mut syncs = refreshing(b_off);
    read_ahead_up_to(&mut syncs, &harness, 2);

    // As the session does when the user switches "b" on.
    syncs.list(filtering(&["a", "b"]));
    harness
        .db
        .store
        .owe_read_ahead(&cpython(), vec![3])
        .unwrap();
    drop(run(&mut syncs, &harness));

    assert_eq!(
        harness.operations(),
        [
            "RefreshRepository",
            "FetchPullRequestContent",
            "FetchPullRequestContent"
        ]
    );
    assert_eq!(reviews_held(&harness, 3), Some(vec![ReviewState::Approved]));
    harness.finish();
}

/// Steps `syncs` until the store holds PR `number`'s Reviews.
fn read_ahead_up_to(syncs: &mut Syncs, harness: &Harness, number: u32) {
    while reviews_held(harness, number).is_none() {
        drop(syncs.step(&harness.github, &harness.db.store).unwrap());
    }
}

/// A Repository whose Filters "a" and "b" find strangers' PRs 2 and 3, of which the store owes a read.
fn owing_two_reads(name: &str) -> Harness {
    let quiet = polled(&list(&[], None), &list(&[], None), &[search(&[], None)]);
    let harness = after_a_sync(name, vec![quiet, approved()]);
    holding_pr(&harness, &strangers_pr(2, LONG_BEFORE));
    holding_pr(&harness, &strangers_pr(3, LONG_BEFORE));
    stored_filter(&harness, "a", &[2]);
    stored_filter(&harness, "b", &[3]);
    harness
        .db
        .store
        .owe_read_ahead(&cpython(), vec![2, 3])
        .unwrap();
    harness
}

#[test]
fn a_filter_switched_off_while_a_refresh_reads_ahead_leaves_what_it_reached_unread() {
    let harness = owing_two_reads("switched-off-reading-ahead");
    let mut syncs = refreshing(filtering(&["a", "b"]));
    read_ahead_up_to(&mut syncs, &harness, 2);

    let mut b_off = filtering(&["a", "b"]);
    b_off.filters[1].on_workbench = OnWorkbench::Off;
    syncs.list(b_off);
    drop(run(&mut syncs, &harness));

    assert_eq!(
        harness.operations(),
        ["RefreshRepository", "FetchPullRequestContent"]
    );
    assert_eq!(reviews_held(&harness, 3), None);
    harness.finish();
}

#[test]
fn a_conversation_the_pr_view_read_while_a_refresh_reads_ahead_isnt_read_again() {
    let harness = owing_two_reads("read-by-the-pr-view");
    let mut syncs = refreshing(filtering(&["a", "b"]));
    read_ahead_up_to(&mut syncs, &harness, 2);

    // As the PR view does when the user opens PR 3, with what GitHub answered for PR 2.
    let read = (harness.db.reader)
        .pull_request_content(&cpython(), 2)
        .unwrap();
    harness
        .db
        .store
        .save_pull_request_content(&cpython(), 3, (read.unwrap(), OffsetDateTime::UNIX_EPOCH))
        .unwrap();
    drop(run(&mut syncs, &harness));

    assert_eq!(
        harness.operations(),
        ["RefreshRepository", "FetchPullRequestContent"]
    );
    harness.finish();
}

#[test]
fn a_quiet_refresh_reads_ahead_what_a_switch_owed_while_it_is_in_reach() {
    let quiet = polled(&list(&[], None), &list(&[], None), &[search(&[], None)]);
    let harness = after_a_sync("switch-owed", vec![quiet, approved()]);
    holding_pr(&harness, &strangers_pr(2, LONG_BEFORE));
    holding_pr(&harness, &strangers_pr(3, LONG_BEFORE));
    stored_filter(&harness, "a", &[2]);
    // As a Filter switched on would leave PR 2, and a Filter switched on and then off again PR 3.
    harness
        .db
        .store
        .owe_read_ahead(&cpython(), vec![2, 3])
        .unwrap();
    let mut syncs = refreshing(filtering(&["a"]));

    drop(run(&mut syncs, &harness));

    assert_eq!(
        harness.operations(),
        ["RefreshRepository", "FetchPullRequestContent"]
    );
    assert_eq!(reviews_held(&harness, 2), Some(vec![ReviewState::Approved]));
    assert_eq!(reviews_held(&harness, 3), None);
    assert_eq!(
        harness.db.store.read_ahead_owed(&cpython()).unwrap(),
        BTreeSet::new()
    );
    harness.finish();
}

#[test]
fn a_backport_a_refresh_brought_gets_its_origin_pr_when_a_tree_lists_it() {
    let backport = pr_node(
        10,
        &json!({
            "title": "[3.13] gh-1: Fix it (GH-9)",
            "baseRefName": "3.13",
            "author": { "login": "miss-islington" },
            "updatedAt": AFTER_IT,
        }),
    );
    let merged = json!({ "state": "MERGED", "mergeCommit": { "oid": "0".repeat(40) } });
    let origin = pull_request_answer(pr_node(9, &merged), RATE_LIMIT);
    let replies = vec![
        polled(
            &list(&[issue_node(1, AFTER_IT)], None),
            &list(&[changed_pr(10, AFTER_IT)], None),
            &[],
        ),
        fetched_nodes(&[backport], RATE_LIMIT),
        cross_references(&[10], None, RATE_LIMIT),
        origin,
    ];
    let harness = after_a_sync("refresh-origin", replies);
    holding_issue(&harness, 1, LONG_BEFORE);

    drop(run(&mut refreshing(filtering(&[])), &harness));

    assert_eq!(harness.numbers(), [9, 10]);
    harness.finish();
}

#[test]
fn a_refresh_that_stopped_reads_the_trees_it_owed_at_the_next_start() {
    let moved = || {
        polled(
            &list(&[issue_node(1, AFTER_IT)], None),
            &list(&[], None),
            &[],
        )
    };
    let replies = vec![moved(), moved(), no_cross_references(RATE_LIMIT)];
    let harness = after_a_sync("stopped-issue", replies);
    holding_issue(&harness, 1, LONG_BEFORE);

    let mut stopped = refreshing(filtering(&[]));
    drop(stopped.step(&harness.github, &harness.db.store).unwrap());
    drop(stopped);
    let mut syncs = refreshing(filtering(&[]));
    let changes = run(&mut syncs, &harness);

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(asked[1].0, "RefreshRepository");
    assert_eq!(asked[1].1["since"], "2026-11-04T09:58:00Z");
    assert_eq!(asked[2].0, "FetchCrossReferences");
    assert_eq!(changes, []);
    let state = harness.db.store.sync_state(&cpython()).unwrap().unwrap();
    assert!(!state.refresh_under_way);
    harness.finish();
}

#[test]
fn a_refresh_that_stopped_follows_the_users_pr_it_had_saved() {
    let mine = own_open_pr(1, &json!({ "title": "gh-40: Mine", "updatedAt": AFTER_IT }));
    let theirs = strangers_pr(2, AFTER_IT);
    let replies = vec![
        polled(
            &list(&[], None),
            &list(&[changed_pr(1, AFTER_IT), changed_pr(2, AFTER_IT)], None),
            &[],
        ),
        probed(&[]),
        issues(&[40], RATE_LIMIT),
        no_cross_references(RATE_LIMIT),
        approved(),
    ];
    let harness = after_a_sync("stopped-pr", replies);
    saved_by_a_stopped_refresh(&harness, &mine);
    saved_by_a_stopped_refresh(&harness, &theirs);
    let mut syncs = refreshing(filtering(&[]));

    drop(run(&mut syncs, &harness));

    assert_eq!(
        harness.operations(),
        [
            "RefreshRepository",
            "ProbePullRequests",
            "FetchIssues",
            "FetchCrossReferences",
            "FetchPullRequestContent",
        ]
    );
    assert_eq!(reviews_held(&harness, 1), Some(vec![ReviewState::Approved]));
    assert_eq!(
        reviews_held(&harness, 2),
        None,
        "the user's work doesn't reach a stranger's PR"
    );
    assert_eq!(
        harness.db.store.read_ahead_owed(&cpython()).unwrap(),
        BTreeSet::new()
    );
    harness.finish();
}

/// GitHub's answer to a probe of PR `number`, which `some-reviewer` approved without moving `updatedAt`.
fn probed_approved(number: u32) -> Result<Reply, Unreachable> {
    let fields = json!({ format!("p{number}"): {
        "number": number,
        "reviewRequests": { "nodes": [] },
        "timelineItems": { "nodes": [] },
        "latestReviews": { "nodes": [
            { "author": { "login": "some-reviewer" }, "state": "APPROVED" },
        ] },
        "lastCommit": { "nodes": [] },
    } });
    answer(json!({ "repository": fields }), RATE_LIMIT)
}

#[test]
fn a_refresh_that_stopped_after_the_probe_reads_ahead_what_the_probe_found() {
    let replies = vec![
        quiet_poll(),
        probed_approved(1),
        quiet_poll(),
        probed_approved(1),
        approved(),
    ];
    let harness = after_a_sync("stopped-after-probe", replies);
    holding_pr(
        &harness,
        &own_open_pr(1, &json!({ "updatedAt": LONG_BEFORE })),
    );
    let mut stopped = refreshing(filtering(&[]));
    let mut asked = Vec::new();
    while !asked
        .iter()
        .any(|operation| operation == "ProbePullRequests")
    {
        drop(stopped.step(&harness.github, &harness.db.store).unwrap());
        asked.extend(harness.operations());
    }
    drop(stopped);

    drop(run(&mut refreshing(filtering(&[])), &harness));

    assert_eq!(
        harness.operations(),
        [
            "RefreshRepository",
            "ProbePullRequests",
            "FetchPullRequestContent"
        ],
        "the second probe finds PR 1 as the first one saved it"
    );
    assert_eq!(reviews_held(&harness, 1), Some(vec![ReviewState::Approved]));
    harness.finish();
}

#[test]
fn a_tree_the_user_opened_leaves_what_a_stopped_refresh_owes_to_the_refresh() {
    let replies = vec![
        cross_references(&[], None, RATE_LIMIT),
        quiet_poll(),
        probed(&[]),
        approved(),
    ];
    let harness = after_a_sync("tree-leaves-read-ahead", replies);
    saved_by_a_stopped_refresh(&harness, &own_open_pr(1, &json!({})));
    let mut syncs = refreshing(filtering(&[]));
    syncs.open_issue(&cpython(), 7);

    drop(run(&mut syncs, &harness));

    assert_eq!(
        harness.operations(),
        [
            "FetchCrossReferences",
            "RefreshRepository",
            "ProbePullRequests",
            "FetchPullRequestContent"
        ]
    );
    assert_eq!(reviews_held(&harness, 1), Some(vec![ReviewState::Approved]));
    harness.finish();
}

#[test]
fn a_refresh_that_stopped_reading_ahead_leaves_the_next_only_what_it_hadnt_read() {
    let replies = vec![
        polled(
            &list(&[], None),
            &list(&[changed_pr(1, AFTER_IT), changed_pr(2, AFTER_IT)], None),
            &[],
        ),
        fetched_nodes(
            &[
                own_open_pr(1, &json!({ "title": "A change", "updatedAt": AFTER_IT })),
                own_open_pr(2, &json!({ "title": "Another", "updatedAt": AFTER_IT })),
            ],
            RATE_LIMIT,
        ),
        approved(),
        quiet_poll(),
        probed(&[]),
        approved(),
    ];
    let harness = after_a_sync("stopped-reading-ahead", replies);
    let mut stopped = refreshing(filtering(&[]));
    let mut asked = Vec::new();
    while !asked
        .iter()
        .any(|operation| operation == "FetchPullRequestContent")
    {
        drop(stopped.step(&harness.github, &harness.db.store).unwrap());
        asked.extend(harness.operations());
    }
    drop(stopped);

    drop(run(&mut refreshing(filtering(&[])), &harness));

    assert_eq!(
        harness.operations(),
        [
            "RefreshRepository",
            "ProbePullRequests",
            "FetchPullRequestContent"
        ],
        "PR 1's Conversation was read before the refresh stopped"
    );
    assert_eq!(reviews_held(&harness, 2), Some(vec![ReviewState::Approved]));
    harness.finish();
}

#[test]
fn a_probe_that_lists_people_in_another_order_says_nothing() {
    let people = json!({
        "reviewRequests": { "nodes": [
            { "requestedReviewer": { "login": "some-reviewer" } },
            { "requestedReviewer": { "login": "Another-reviewer" } },
        ] },
        "timelineItems": { "nodes": [] },
        "latestReviews": { "nodes": [
            { "author": { "login": "some-author" }, "state": "COMMENTED" },
            { "author": { "login": "Another-author" }, "state": "APPROVED" },
        ] },
    });
    let probe = json!({
        "number": 1,
        "reviewRequests": { "nodes": [
            { "requestedReviewer": { "login": "Another-reviewer" } },
            { "requestedReviewer": { "login": "some-reviewer" } },
        ] },
        "timelineItems": { "nodes": [] },
        "latestReviews": { "nodes": [
            { "author": { "login": "Another-author" }, "state": "APPROVED" },
            { "author": { "login": "some-author" }, "state": "COMMENTED" },
        ] },
        "lastCommit": { "nodes": [] },
    });
    let probed = answer(json!({ "repository": { "p1": probe } }), RATE_LIMIT);
    let harness = after_a_sync("probe-order", vec![quiet_poll(), probed]);
    holding_pr(&harness, &own_open_pr(1, &people));
    let mut syncs = refreshing(filtering(&[]));

    let changes = run(&mut syncs, &harness);

    assert_eq!(changes, []);
    harness.finish();
}

#[test]
fn a_filter_merges_its_narrowed_answer_and_brings_what_it_newly_reached() {
    let replies = vec![
        polled(
            &list(&[issue_node(1, AFTER_IT)], None),
            &list(&[], None),
            &[search(&[issue_hit(2)], None)],
        ),
        issues(&[2], RATE_LIMIT),
        no_cross_references(RATE_LIMIT),
    ];
    let harness = after_a_sync("filter-merge", replies);
    stored_filter(&harness, "a", &[1, 9]);
    let mut syncs = refreshing(filtering(&["a"]));

    let changes = run(&mut syncs, &harness);

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(
        asked[0].1["filter0"],
        "repo:python/cpython (a) updated:>=2026-11-04T09:58:00Z"
    );
    let results = &facts(&harness).filters[&query("a")];
    // 1 changed and the search didn't find it, so it stopped matching. Nothing said 9 changed.
    assert_eq!(results.numbers, BTreeSet::from([2, 9]));
    assert_eq!(changes[0].items, [1, 2]);
    harness.finish();
}

/// Runs a refresh whose poll saves an Issue and merges two Filters, up to its first failure, and checks the
/// window has heard of what's saved by then. Answers with the store calls the run made.
fn the_window_hears_of_each_save_with_call_lost(lost: Option<usize>) -> usize {
    let replies = vec![
        polled(
            &list(&[issue_node(1, AFTER_IT)], None),
            &list(&[], None),
            &[search(&[issue_hit(2)], None), search(&[issue_hit(3)], None)],
        ),
        issues(&[2, 3], RATE_LIMIT),
        no_cross_references(RATE_LIMIT),
        no_cross_references(RATE_LIMIT),
        no_cross_references(RATE_LIMIT),
    ];
    let harness = after_a_sync(&format!("poll-news-{lost:?}"), replies);
    holding_issue(&harness, 1, LONG_BEFORE);
    stored_filter(&harness, "a", &[]);
    stored_filter(&harness, "b", &[]);
    let mut syncs = refreshing(filtering(&["a", "b"]));
    let (flaky, calls) = harness.db.store.failing_at_call(lost);

    let mut heard = BTreeSet::new();
    while syncs.ready_at().is_some() {
        match syncs.step(&harness.github, &flaky) {
            Ok(Some(Progress::Changed(change))) => heard.extend(change.items),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    flaky.close();

    let facts = facts(&harness);
    let mut saved = &facts.filters[&query("a")].numbers | &facts.filters[&query("b")].numbers;
    saved.extend((facts.issues[0].title == "Issue 1 now").then_some(1));
    assert!(saved.is_subset(&heard), "{lost:?}: {saved:?} {heard:?}");
    harness.finish();
    calls.try_iter().count()
}

#[test]
fn the_window_hears_of_each_save_in_a_poll_before_the_next_can_fail() {
    let calls = the_window_hears_of_each_save_with_call_lost(None);

    for lost in 0..calls {
        assert!(the_window_hears_of_each_save_with_call_lost(Some(lost)) <= calls);
    }
}

#[test]
fn a_filter_with_more_than_one_page_walks_narrowed_after_the_poll() {
    let replies = vec![
        polled(
            &list(&[issue_node(1, AFTER_IT)], None),
            &list(&[], None),
            &[search(&[issue_hit(2)], Some("more"))],
        ),
        searched(&[issue_hit(2), issue_hit(3)], None),
        issues(&[2, 3], RATE_LIMIT),
        no_cross_references(RATE_LIMIT),
        no_cross_references(RATE_LIMIT),
    ];
    let harness = after_a_sync("filter-walks", replies);
    stored_filter(&harness, "a", &[1, 9]);
    let mut syncs = refreshing(filtering(&["a"]));

    drop(run(&mut syncs, &harness));

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(asked[1].0, "SearchFilter");
    assert_eq!(
        asked[1].1["query"],
        "repo:python/cpython (a) updated:>=2026-11-04T09:58:00Z"
    );
    let results = &facts(&harness).filters[&query("a")];
    assert_eq!(results.numbers, BTreeSet::from([2, 3, 9]));
    harness.finish();
}

#[test]
fn a_filter_that_has_never_run_searches_whole() {
    let replies = vec![
        quiet_poll(),
        searched(&[issue_hit(2)], None),
        issues(&[2], RATE_LIMIT),
        no_cross_references(RATE_LIMIT),
    ];
    let harness = after_a_sync("filter-new", replies);
    let mut syncs = refreshing(filtering(&["new"]));

    drop(run(&mut syncs, &harness));

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(asked[0].1.get("filter0"), None);
    assert_eq!(asked[1].1["query"], "repo:python/cpython (new)");
    let results = &facts(&harness).filters[&query("new")];
    assert_eq!(results.numbers, BTreeSet::from([2]));
    harness.finish();
}

#[test]
fn a_search_edited_away_loses_its_results_at_the_refresh() {
    let harness = after_a_sync("filter-gone", vec![quiet_poll()]);
    stored_filter(&harness, "old", &[1]);
    let mut syncs = refreshing(filtering(&[]));

    drop(run(&mut syncs, &harness));

    assert_eq!(facts(&harness).filters.len(), 0);
    harness.finish();
}

#[test]
fn a_filter_github_refused_last_time_searches_whole_and_clears_its_problem() {
    let replies = vec![quiet_poll(), searched(&[], None)];
    let harness = after_a_sync("filter-refused-before", replies);
    stored_filter(&harness, "a", &[1]);
    harness
        .db
        .store
        .save_filter_problem(&cpython(), &query("a"), "GitHub said no")
        .unwrap();
    let mut syncs = refreshing(filtering(&["a"]));

    drop(run(&mut syncs, &harness));

    let results = &facts(&harness).filters[&query("a")];
    assert_eq!(results.numbers, BTreeSet::new());
    assert_eq!(results.problem, None);
    harness.finish();
}

#[test]
fn a_refused_poll_goes_again_without_its_searches_and_they_walk_alone() {
    let refused = reply_ok(r#"{"data":null,"errors":[{"message":"Bad search"}]}"#);
    let replies = vec![
        refused.clone(),
        quiet_poll(),
        refused,
        searched(&[issue_hit(2)], None),
        issues(&[2], RATE_LIMIT),
        no_cross_references(RATE_LIMIT),
    ];
    let harness = after_a_sync("poll-refused", replies);
    stored_filter(&harness, "bad", &[1]);
    stored_filter(&harness, "good", &[]);
    let mut syncs = refreshing(filtering(&["bad", "good"]));

    drop(run(&mut syncs, &harness));

    let facts = facts(&harness);
    assert_eq!(
        facts.filters[&query("bad")].problem,
        Some(FilterProblem::Refused("GitHub said: Bad search".to_owned()))
    );
    assert_eq!(facts.filters[&query("bad")].numbers, BTreeSet::from([1]));
    assert_eq!(facts.filters[&query("good")].numbers, BTreeSet::from([2]));
    harness.finish();
}

#[test]
fn a_poll_refused_with_no_searches_is_a_failure() {
    let refused = reply_ok(r#"{"data":null,"errors":[{"message":"Bad query"}]}"#);
    let harness = after_a_sync("poll-refused-alone", vec![refused]);
    let mut syncs = refreshing(filtering(&[]));

    let problem = syncs.step(&harness.github, &harness.db.store).unwrap_err();

    assert_eq!(problem.problem.kind, ErrorKind::QueryRefused);
    harness.finish();
}

#[test]
fn a_poll_that_fails_with_searches_in_it_is_a_failure_too() {
    let harness = after_a_sync("poll-offline", vec![Err(Unreachable::Offline)]);
    stored_filter(&harness, "a", &[]);
    let mut syncs = refreshing(filtering(&["a"]));

    let problem = syncs.step(&harness.github, &harness.db.store).unwrap_err();

    assert_eq!(problem.problem.kind, ErrorKind::Offline);
    harness.finish();
}

/// Answers list checks from a list, then GraphQL from another, and says what it was asked.
struct SmallRepository {
    checks: RefCell<VecDeque<ConditionalReply>>,
    posts: RefCell<VecDeque<Result<Reply, Unreachable>>>,
    tags: Sender<Option<String>>,
}

impl Transport for SmallRepository {
    fn post(&self, _: &Token, _: &Request<'_>) -> Result<Reply, Unreachable> {
        self.posts
            .borrow_mut()
            .pop_front()
            .unwrap_or(Err(Unreachable::Offline))
    }

    fn get(
        &self,
        _: &Token,
        request: &ConditionalGet<'_>,
    ) -> Result<ConditionalReply, Unreachable> {
        pass_on(&self.tags, request.etag.map(str::to_owned));
        self.checks
            .borrow_mut()
            .pop_front()
            .ok_or(Unreachable::Offline)
    }
}

fn small_repository(
    name: &str,
    checks: Vec<ConditionalReply>,
    posts: Vec<Result<Reply, Unreachable>>,
) -> (Harness, mpsc::Receiver<Option<String>>) {
    let (tags, asked) = mpsc::channel();
    let transport = SmallRepository {
        checks: RefCell::new(checks.into()),
        posts: RefCell::new(posts.into()),
        tags,
    };
    let harness = Harness::answering(name, Box::new(transport));
    harness.db.store.remember_login(&me()).unwrap();
    (harness, asked)
}

fn check_reply(status: u16, etag: Option<&str>) -> ConditionalReply {
    ConditionalReply {
        date: None,
        status,
        etag: etag.map(str::to_owned),
    }
}

fn small_poll() -> Result<Reply, Unreachable> {
    polled_with_open(10, &list(&[], None), &list(&[], None), &[])
}

fn check_after_a_refresh(harness: &Harness) -> ChangeCheck {
    harness
        .db
        .store
        .sync_state(&cpython())
        .unwrap()
        .unwrap()
        .check
}

#[test]
fn a_poll_of_a_small_repository_asks_for_a_tag_at_the_next_refresh() {
    let (harness, _tags) = small_repository("small-found", Vec::new(), vec![small_poll()]);
    synced(&harness, ChangeCheck::Skipped);
    let mut syncs = refreshing(filtering(&[]));

    drop(run(&mut syncs, &harness));

    assert_eq!(check_after_a_refresh(&harness), ChangeCheck::Untagged);
    harness.finish();
}

#[test]
fn a_small_repository_keeps_the_tag_from_before_its_poll() {
    let (harness, tags) = small_repository(
        "small-tag",
        vec![check_reply(200, Some("\"new\""))],
        vec![small_poll()],
    );
    synced(&harness, ChangeCheck::Untagged);
    let mut syncs = refreshing(filtering(&[]));

    drop(run(&mut syncs, &harness));

    assert_eq!(tags.recv().unwrap(), None);
    assert_eq!(
        check_after_a_refresh(&harness),
        ChangeCheck::Tagged(Etag::new("\"new\""))
    );
    harness.finish();
}

#[test]
fn a_304_skips_the_poll_and_the_probe_still_runs() {
    let (harness, tags) = small_repository(
        "small-same",
        vec![check_reply(304, None)],
        vec![
            probed(&[(1, "FAILURE")]),
            failed_checks(1, &["Tests"]),
            approved(),
        ],
    );
    synced(&harness, ChangeCheck::Tagged(Etag::new("\"same\"")));
    holding_pr(&harness, &own_open_pr(1, &json!({})));
    stored_filter(&harness, "a", &[]);
    let mut syncs = refreshing(filtering(&["a"]));

    let changes = run(&mut syncs, &harness);

    assert_eq!(tags.recv().unwrap().as_deref(), Some("\"same\""));
    assert_eq!(changes[0].items, [1]);
    let after = harness.db.store.sync_state(&cpython()).unwrap().unwrap();
    assert_eq!(after.check, ChangeCheck::Tagged(Etag::new("\"same\"")));
    assert!(after.started_at > moment(LAST_SYNC));
    harness.finish();
}

#[test]
fn a_304_still_runs_a_filter_that_has_never_searched() {
    let search_answer = searched(&[], None);
    let (harness, _tags) = small_repository(
        "small-new-filter",
        vec![check_reply(304, None)],
        vec![search_answer],
    );
    synced(&harness, ChangeCheck::Tagged(Etag::new("\"same\"")));
    let mut syncs = refreshing(filtering(&["new"]));

    drop(run(&mut syncs, &harness));

    assert_eq!(facts(&harness).filters.len(), 1);
    harness.finish();
}

#[test]
fn a_repository_that_grew_stops_checking() {
    let (harness, _tags) = small_repository(
        "small-grew",
        vec![check_reply(200, Some("\"new\""))],
        vec![quiet_poll()],
    );
    synced(&harness, ChangeCheck::Tagged(Etag::new("\"old\"")));
    let mut syncs = refreshing(filtering(&[]));

    drop(run(&mut syncs, &harness));

    assert_eq!(check_after_a_refresh(&harness), ChangeCheck::Skipped);
    harness.finish();
}

#[test]
fn a_tag_github_didnt_give_leaves_a_small_repository_asking() {
    let (harness, _tags) = small_repository(
        "small-no-tag",
        vec![check_reply(500, None)],
        vec![small_poll()],
    );
    synced(&harness, ChangeCheck::Tagged(Etag::new("\"old\"")));
    let mut syncs = refreshing(filtering(&[]));

    drop(run(&mut syncs, &harness));

    assert_eq!(check_after_a_refresh(&harness), ChangeCheck::Untagged);
    harness.finish();
}

#[test]
fn a_name_in_another_repository_shares_nothing() {
    let other = RepositoryName::parse("python/peps").unwrap();
    let harness = after_a_sync("other", Vec::new());

    let state = harness.db.store.sync_state(&other).unwrap();

    assert_eq!(state, None);
    harness.finish();
}

/// The comment at `AFTER_IT` that these tests read back.
fn comment_after_it(who: &str) -> Activity {
    Activity {
        kind: ActivityKind::Comment,
        who: Login::new(who),
        at: moment(AFTER_IT),
    }
}

/// PR 3 by `author`, merged, as GitHub answers with it at `updated_at`, its timeline holding `timeline`. It
/// names no Issue, so it stays a Standalone PR.
fn merged_pr(author: &str, updated_at: &str, timeline: &[Value]) -> Value {
    let changes = json!({
        "title": "A change",
        "author": { "login": author },
        "state": "MERGED",
        "mergeCommit": { "oid": "0".repeat(40) },
        "updatedAt": updated_at,
        "timelineItems": { "nodes": timeline },
    });
    pr_node(3, &changes)
}

/// A harness that holds PR 3 by `author`, merged, whose next poll says it moved, and whose GitHub then
/// answers `replies`.
fn holding_a_merged_pr_that_moves(
    name: &str,
    author: &str,
    replies: Vec<Result<Reply, Unreachable>>,
) -> Harness {
    let moved = list(&[changed_pr(3, AFTER_IT)], None);
    let mut all = vec![polled(&list(&[], None), &moved, &[])];
    all.extend(replies);
    let harness = after_a_sync(name, all);
    holding_pr(&harness, &merged_pr(author, LONG_BEFORE, &[]));
    harness
}

/// The Bot's comment at `AFTER_IT` that it could not backport a PR to 3.14.
fn backport_conflict() -> Value {
    json!({
        "__typename": "IssueComment",
        "createdAt": AFTER_IT,
        "author": { "__typename": "Bot", "login": "miss-islington-app" },
        "bodyText": "Sorry, I could not cleanly backport this to `3.14` due to a conflict.",
    })
}

#[test]
fn the_bots_comment_after_a_sync_saw_the_merge_still_reaches_the_store() {
    let after = merged_pr("some-author", AFTER_IT, &[backport_conflict()]);
    let replies = vec![fetched_nodes(&[after], RATE_LIMIT)];
    let harness = holding_a_merged_pr_that_moves("merged-moved", "some-author", replies);

    drop(run(&mut refreshing(filtering(&[])), &harness));

    let events = harness.db.store.activity(&cpython(), 3).unwrap();
    assert_eq!(
        events,
        [Activity {
            kind: ActivityKind::BackportFailed {
                target: "3.14".to_owned()
            },
            who: Login::new("miss-islington-app"),
            at: moment(AFTER_IT),
        }]
    );
    harness.finish();
}

/// Saves the PR a fetch would read from `node`, with the events of its timeline, as a sync saves it.
fn holding_pr_and_its_events(harness: &Harness, node: &Value) {
    let (pull_requests, events) = read_pr_and_events(node);
    (harness.db.store)
        .save_fetched_pull_requests(&cpython(), pull_requests, events)
        .unwrap();
}

/// Has the store hold PR 3, a stranger's from long before, titled with `issue`'s number under CPython's
/// Conventions.
fn holding_a_strangers_pr_naming(harness: &Harness, issue: u32) {
    let mut theirs = strangers_pr(3, LONG_BEFORE);
    theirs["title"] = json!(format!("gh-{issue}: Fix it"));
    holding_pr(harness, &theirs);
}

#[test]
fn a_refresh_fetches_the_issue_a_merged_pr_on_the_workbench_names_and_tsugiki_lacks() {
    let harness = after_a_sync(
        "lacking-issue",
        vec![quiet_poll(), issues(&[40], RATE_LIMIT)],
    );
    // A stranger's PR the user reviewed, whose Bot backport failed after a sync saw the merge: a new event
    // that makes it Need attention.
    let mut node = merged_pr("some-author", AFTER_IT, &[backport_conflict()]);
    node["title"] = json!("gh-40: Fix it");
    node["latestReviews"] =
        json!({ "nodes": [{ "author": { "login": "octocat" }, "state": "APPROVED" }] });
    holding_pr_and_its_events(&harness, &node);
    let settings = filtering(&[]);
    let pr = ItemKey::PullRequest { number: 3 };
    let standalone = listed(&harness, &settings, &WorkbenchView::Whole, pr).is_some();

    drop(run(&mut refreshing(settings.clone()), &harness));

    assert!(standalone, "it Needs attention, with no Issue to sit under");
    assert_eq!(harness.operations(), ["RefreshRepository", "FetchIssues"]);
    assert_eq!(harness.issue_numbers(), [40]);
    let issue = ItemKey::Issue { number: 40 };
    assert!(listed(&harness, &settings, &WorkbenchView::Whole, issue).is_some());
    assert!(
        listed(&harness, &settings, &WorkbenchView::Whole, pr).is_none(),
        "it shows under its Issue"
    );
    harness.finish();
}

#[test]
fn a_pr_number_isnt_asked_for_again_and_one_github_doesnt_find_is_at_the_next_refresh() {
    let answer = |repository: Value| {
        let path = json!(["repository", "i40"]);
        not_found(json!({ "repository": repository }), path)
    };
    let first = answer(json!({ "i40": null, "i41": { "__typename": "PullRequest" } }));
    let second = answer(json!({ "i40": null }));
    let replies = vec![quiet_poll(), first, quiet_poll(), second];
    let harness = after_a_sync("lacking-issueless", replies);
    let mut theirs = strangers_pr(3, LONG_BEFORE);
    theirs["title"] = json!("gh-40: Fix it");
    // A closing reference counts under every Conventions.
    theirs["closingIssuesReferences"] = json!({ "nodes": [
        { "number": 41, "repository": { "nameWithOwner": "python/cpython" } },
    ] });
    holding_pr(&harness, &theirs);

    drop(run(&mut refreshing(filtering(&[])), &harness));
    drop(run(&mut refreshing(filtering(&[])), &harness));

    assert_eq!(
        harness.operations(),
        [
            "RefreshRepository",
            "FetchIssues",
            "RefreshRepository",
            "FetchIssues"
        ]
    );
    let lacking = harness
        .db
        .store
        .named_issues_lacking(&cpython(), Conventions::Cpython);
    assert_eq!(
        lacking.unwrap(),
        BTreeSet::from([40]),
        "41 is a PR's number"
    );
    assert!(harness.issue_numbers().is_empty());
    harness.finish();
}

#[test]
fn a_batch_of_issues_github_refuses_waits_for_the_next_refresh_and_this_one_finishes() {
    let refused = reply_ok(r#"{"data":null,"errors":[{"type":"FORBIDDEN","message":"No"}]}"#);
    let replies = vec![
        quiet_poll(),
        refused,
        quiet_poll(),
        issues(&[40], RATE_LIMIT),
    ];
    let harness = after_a_sync("lacking-batch-refused", replies);
    holding_a_strangers_pr_naming(&harness, 40);

    drop(run(&mut refreshing(filtering(&[])), &harness));
    let state = harness.db.store.sync_state(&cpython()).unwrap().unwrap();
    let held_after_the_first = harness.issue_numbers();
    drop(run(&mut refreshing(filtering(&[])), &harness));

    assert!(!state.refresh_under_way, "the refresh finished");
    assert!(held_after_the_first.is_empty());
    assert_eq!(
        harness.operations(),
        [
            "RefreshRepository",
            "FetchIssues",
            "RefreshRepository",
            "FetchIssues"
        ]
    );
    assert_eq!(harness.issue_numbers(), [40]);
    harness.finish();
}

#[test]
fn a_number_github_refuses_alone_waits_for_the_next_refresh_and_the_rest_of_its_batch_is_saved() {
    let fetched = crate::github::testing::issue_node;
    let repository = json!({ "i40": fetched(40), "i41": null, "i42": fetched(42) });
    let refused_41 = erring(
        json!({ "repository": repository }),
        ("FORBIDDEN", "No"),
        json!(["repository", "i41"]),
    );
    let replies = vec![
        quiet_poll(),
        refused_41,
        quiet_poll(),
        issues(&[41], RATE_LIMIT),
    ];
    let harness = after_a_sync("lacking-refused", replies);
    let mut theirs = strangers_pr(3, LONG_BEFORE);
    theirs["title"] = json!("gh-40: Fix it");
    theirs["closingIssuesReferences"] = json!({ "nodes": [
        { "number": 41, "repository": { "nameWithOwner": "python/cpython" } },
        { "number": 42, "repository": { "nameWithOwner": "python/cpython" } },
    ] });
    holding_pr(&harness, &theirs);

    drop(run(&mut refreshing(filtering(&[])), &harness));
    let state = harness.db.store.sync_state(&cpython()).unwrap().unwrap();
    let held_after_the_first = harness.issue_numbers();
    drop(run(&mut refreshing(filtering(&[])), &harness));

    assert!(!state.refresh_under_way, "the refresh finished");
    assert!(state.finished_at > moment(LAST_SYNC));
    assert_eq!(held_after_the_first, [40, 42]);
    assert_eq!(
        harness.operations(),
        [
            "RefreshRepository",
            "FetchIssues",
            "RefreshRepository",
            "FetchIssues"
        ]
    );
    assert_eq!(harness.issue_numbers(), [40, 41, 42]);
    harness.finish();
}

#[test]
fn a_refresh_that_fetches_the_issues_its_prs_name_recovers_from_a_failure_at_each_point() {
    let scenario = Scenario {
        name: "recovery-lacking-issues",
        settings: filtering(&[]),
        prepare: |harness| {
            synced(harness, ChangeCheck::Skipped);
            holding_a_strangers_pr_naming(harness, 40);
        },
        posts: vec![quiet_poll(), issues(&[40], RATE_LIMIT)],
        checks: Vec::new(),
    };

    let operations = recovers_from_each_failure(&scenario);

    assert_eq!(operations, ["RefreshRepository", "FetchIssues"]);
}

#[test]
fn a_refresh_leaves_unfetched_a_number_only_another_conventions_reads_in_a_title() {
    let harness = after_a_sync("lacking-conventions", vec![quiet_poll()]);
    holding_a_strangers_pr_naming(&harness, 12);
    let mut settings = filtering(&[]);
    settings.conventions = Conventions::Github;

    drop(run(&mut refreshing(settings), &harness));

    assert_eq!(harness.operations(), ["RefreshRepository"]);
    harness.finish();
}

#[test]
fn a_refresh_fetches_the_issue_a_strangers_pr_names_and_the_inbox_lists_it() {
    let harness = after_a_sync(
        "lacking-outside",
        vec![quiet_poll(), issues(&[40], RATE_LIMIT)],
    );
    holding_a_strangers_pr_naming(&harness, 40);
    let settings = filtering(&[]);
    let before = inbox(&harness, &settings);

    drop(run(&mut refreshing(settings.clone()), &harness));

    assert_eq!(
        before,
        [vec![3]],
        "a Standalone PR, as Tsugiki lacks its Issue"
    );
    assert_eq!(harness.operations(), ["RefreshRepository", "FetchIssues"]);
    assert_eq!(harness.issue_numbers(), [40]);
    assert_eq!(inbox(&harness, &settings), [vec![40, 3]]);
    harness.finish();
}

#[test]
fn a_refresh_probes_the_ci_of_an_open_bot_backport_of_what_the_user_wrote() {
    let backport = |number: u32, origin: u32| {
        pr_node(
            number,
            &json!({
                "title": format!("[3.14] gh-1: Fix it (GH-{origin})"),
                "baseRefName": "3.14",
                "author": { "login": "miss-islington" },
                "updatedAt": LONG_BEFORE,
                "commits": { "totalCount": 1, "nodes": [{ "commit": {
                    "message": "Fix it",
                    "parents": { "totalCount": 1 },
                    "committer": { "user": { "login": "miss-islington" } },
                } }] },
            }),
        )
    };
    let merged = |number: u32, author: &str| {
        pr_node(
            number,
            &json!({
                "author": { "login": author },
                "state": "MERGED",
                "mergeCommit": { "oid": "0".repeat(40) },
                "updatedAt": LONG_BEFORE,
            }),
        )
    };
    let replies = vec![quiet_poll(), probed(&[(10, "FAILURE")]), approved()];
    let harness = after_a_sync("probe-backport", replies);
    holding_pr(&harness, &merged(1, "octocat"));
    holding_pr(&harness, &backport(10, 1));
    holding_pr(&harness, &merged(2, "some-author"));
    holding_pr(&harness, &backport(20, 2));
    let mut syncs = refreshing(filtering(&[]));

    drop(run(&mut syncs, &harness));

    let probe_asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    let probe = probe_asked
        .iter()
        .find(|(operation, _)| operation == "ProbePullRequests");
    assert!(probe.is_some());
    let held = harness.db.store.pull_requests(&cpython()).unwrap();
    let ci = |number: u32| held.iter().find(|pr| pr.number == number).unwrap().ci;
    assert_eq!(ci(10), Some(CiState::Failure));
    assert_eq!(ci(20), None);
    harness.finish();
}

#[test]
fn a_moved_issue_keeps_who_commented_and_one_tsugiki_doesnt_hold_keeps_nothing() {
    let commented = |number: u32| {
        let mut node = issue_node(number, AFTER_IT);
        node["timelineItems"] = json!({ "nodes": [{
            "__typename": "IssueComment",
            "createdAt": AFTER_IT,
            "author": { "__typename": "User", "login": "some-author" },
        }] });
        node
    };
    let issues_list = list(&[commented(1), commented(2)], None);
    let replies = vec![
        polled(&issues_list, &list(&[], None), &[]),
        cross_references(&[], None, RATE_LIMIT),
    ];
    let harness = after_a_sync("issue-activity", replies);
    holding_issue(&harness, 1, LONG_BEFORE);

    drop(run(&mut refreshing(filtering(&[])), &harness));

    let events = harness.db.store.activity(&cpython(), 1).unwrap();
    assert_eq!(events, [comment_after_it("some-author")]);
    assert_eq!(harness.db.store.activity(&cpython(), 2).unwrap(), []);
    harness.finish();
}

#[test]
fn a_probe_keeps_a_review_that_didnt_move_the_pr() {
    let review = json!({
        "__typename": "PullRequestReview",
        "submittedAt": AFTER_IT,
        "author": { "__typename": "User", "login": "some-reviewer" },
    });
    let fields = json!({ "p1": {
        "number": 1,
        "reviewRequests": { "nodes": [] },
        "timelineItems": { "nodes": [review] },
        "latestReviews": { "nodes": [] },
        "lastCommit": { "nodes": [] },
    } });
    let probed = answer(json!({ "repository": fields }), RATE_LIMIT);
    let harness = after_a_sync("probe-activity", vec![quiet_poll(), probed]);
    holding_pr(
        &harness,
        &own_open_pr(1, &json!({ "updatedAt": LONG_BEFORE })),
    );

    drop(run(&mut refreshing(filtering(&[])), &harness));

    assert_eq!(
        harness.db.store.activity(&cpython(), 1).unwrap(),
        [Activity {
            kind: ActivityKind::Review,
            who: Login::new("some-reviewer"),
            at: moment(AFTER_IT),
        }]
    );
    harness.finish();
}

const FIVE_MINUTES: time::Duration = time::Duration::minutes(5);

/// A harness after a sync, whose first reply sets GitHub's clock to `github_time`, which the local clock is
/// off from.
fn on_githubs_clock(
    name: &str,
    github_time: OffsetDateTime,
    replies: Vec<Result<Reply, Unreachable>>,
) -> Harness {
    let mut all = vec![dated(signed_in_as("reviewer"), github_time)];
    all.extend(replies);
    let harness = after_a_sync(name, all);
    drop(harness.github.viewer().unwrap());
    harness
}

#[test]
fn a_refresh_asks_since_the_last_start_by_githubs_clock_whatever_the_local_clock_says() {
    for local_is_off_by in [FIVE_MINUTES, -FIVE_MINUTES] {
        let github_time = OffsetDateTime::now_utc() - local_is_off_by;
        let harness = on_githubs_clock(
            "github-clock",
            github_time,
            vec![quiet_poll(), quiet_poll()],
        );

        drop(run(&mut refreshing(filtering(&[])), &harness));
        drop(run(&mut refreshing(filtering(&[])), &harness));

        let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
        let since = asked[2].1["since"].as_str().unwrap();
        let since = OffsetDateTime::parse(since, &Rfc3339).unwrap();
        let expected = github_time - OVERLAP;
        assert!(
            since >= expected - time::Duration::seconds(1),
            "{since} against {expected}"
        );
        assert!(
            since <= expected + time::Duration::seconds(10),
            "{since} against {expected}"
        );
        harness.finish();
    }
}

#[test]
fn a_change_under_two_minutes_old_by_githubs_clock_keeps_its_item_in_the_filter() {
    // The local clock is 5 minutes fast, so by it this change is 6 minutes old and settled.
    let github_time = OffsetDateTime::now_utc() - FIVE_MINUTES;
    let changed = (github_time - time::Duration::minutes(1))
        .format(&Rfc3339)
        .unwrap();
    let replies = vec![
        polled(
            &list(&[issue_node(1, &changed)], None),
            &list(&[changed_pr(2, &changed)], None),
            &[search(&[], None)],
        ),
        fetched_nodes(&[strangers_pr(2, &changed)], RATE_LIMIT),
        approved(),
    ];
    let harness = on_githubs_clock("github-clock-settled", github_time, replies);
    stored_filter(&harness, "a", &[1, 2]);

    drop(run(&mut refreshing(filtering(&["a"])), &harness));

    let results = &facts(&harness).filters[&query("a")];
    assert_eq!(results.numbers, BTreeSet::from([1, 2]));
    harness.finish();
}

/// Issue `number` as the poll lists it, updated at `updated_at`, in `state`.
fn issue_in(number: u32, updated_at: &str, state: &str) -> Value {
    let mut node = issue_node(number, updated_at);
    node["state"] = json!(state);
    node
}

/// The keys of the items `harness` holds Archived.
fn archived(harness: &Harness) -> Vec<ItemKey> {
    facts(harness).activity.archived.into_keys().collect()
}

#[test]
fn a_refresh_archives_an_issue_and_a_standalone_pr_it_sees_close_or_merge() {
    let merged = json!({
        "title": "Tidy up",
        "state": "MERGED",
        "mergeCommit": { "oid": "0".repeat(40) },
        "updatedAt": AFTER_IT,
    });
    let closed = json!({
        "title": "A change",
        "author": { "login": "some-author" },
        "state": "CLOSED",
        "updatedAt": AFTER_IT,
    });
    let replies = vec![
        polled(
            &list(&[issue_in(1, AFTER_IT, "CLOSED")], None),
            &list(&[changed_pr(3, AFTER_IT), changed_pr(4, AFTER_IT)], None),
            &[search(&[], None)],
        ),
        fetched_nodes(&[pr_node(3, &closed), pr_node(4, &merged)], RATE_LIMIT),
        no_cross_references(RATE_LIMIT),
    ];
    let harness = after_a_sync("archive-closing", replies);
    holding_issue(&harness, 1, LONG_BEFORE);
    stored_filter(&harness, "a", &[1]);
    holding_pr(&harness, &strangers_pr(3, LONG_BEFORE));
    let tidy = json!({ "title": "Tidy up", "updatedAt": LONG_BEFORE });
    holding_pr(&harness, &own_open_pr(4, &tidy));

    drop(run(&mut refreshing(filtering(&["a"])), &harness));

    // Filter "a" put Issue 1 on the Workbench, and PR 4 was the user's. Nothing put a stranger's PR 3
    // there.
    let keys = [
        ItemKey::Issue { number: 1 },
        ItemKey::PullRequest { number: 4 },
    ];
    assert_eq!(archived(&harness), keys);
    assert_eq!(
        harness.db.store.closings(&cpython()).unwrap(),
        BTreeSet::new()
    );
    harness.finish();
}

#[test]
fn a_refresh_archives_a_closed_issue_whose_backport_is_still_open() {
    let replies = vec![
        polled(
            &list(&[issue_in(1, AFTER_IT, "CLOSED")], None),
            &list(&[], None),
            &[],
        ),
        probed(&[(10, "PENDING")]),
        no_cross_references(RATE_LIMIT),
        // An Archived item stays in Reach, so the refresh reads ahead the Backport's Conversation.
        approved(),
    ];
    let harness = after_a_sync("archive-backport-open", replies);
    holding_issue(&harness, 1, LONG_BEFORE);
    let backport = json!({
        "title": "[3.14] gh-1: Fix it (GH-9)",
        "baseRefName": "3.14",
        "updatedAt": LONG_BEFORE,
    });
    holding_pr(&harness, &own_open_pr(10, &backport));

    drop(run(&mut refreshing(filtering(&[])), &harness));

    assert_eq!(archived(&harness), [ItemKey::Issue { number: 1 }]);
    let backport = facts(&harness).pull_requests;
    assert_eq!(backport[0].state, PrState::Open);
    assert_eq!(
        reviews_held(&harness, 10),
        Some(vec![ReviewState::Approved])
    );
    harness.finish();
}

#[test]
fn a_refresh_keeps_an_item_unarchived_by_hand_out_and_unarchives_one_that_reopens() {
    let replies = vec![
        polled(
            &list(
                &[
                    issue_in(1, AFTER_IT, "CLOSED"),
                    issue_in(2, AFTER_IT, "OPEN"),
                ],
                None,
            ),
            &list(&[], None),
            &[search(&[issue_hit(1), issue_hit(2)], None)],
        ),
        no_cross_references(RATE_LIMIT),
        no_cross_references(RATE_LIMIT),
    ];
    let harness = after_a_sync("archive-reopens", replies);
    let closed = |number| IssueFacts {
        state: Some(IssueState::Closed),
        updated_at: moment(LONG_BEFORE),
        ..issue_facts(number)
    };
    let store = &harness.db.store;
    store
        .save_issues(&cpython(), vec![closed(1), closed(2)])
        .unwrap();
    stored_filter(&harness, "a", &[1, 2]);
    let keys = [ItemKey::Issue { number: 1 }, ItemKey::Issue { number: 2 }];
    archived_on_closing(&harness, &keys);
    store
        .unarchive(&cpython(), ItemKey::Issue { number: 1 })
        .unwrap();

    drop(run(&mut refreshing(filtering(&["a"])), &harness));

    // Issue 1 changed while closed, and Issue 2 reopened.
    assert_eq!(archived(&harness), []);
    harness.finish();
}

#[test]
fn a_refresh_leaves_a_pinned_issue_that_closes_on_the_workbench_not_archived() {
    let replies = vec![
        polled(
            &list(
                &[
                    issue_in(1, AFTER_IT, "CLOSED"),
                    issue_in(2, AFTER_IT, "CLOSED"),
                ],
                None,
            ),
            &list(&[], None),
            &[search(&[], None)],
        ),
        no_cross_references(RATE_LIMIT),
        no_cross_references(RATE_LIMIT),
    ];
    let harness = after_a_sync("archive-pinned", replies);
    holding_issue(&harness, 1, LONG_BEFORE);
    holding_issue(&harness, 2, LONG_BEFORE);
    stored_filter(&harness, "a", &[2]);
    (harness.db.store)
        .pin(&cpython(), 1, moment(LONG_BEFORE))
        .unwrap();
    let settings = filtering(&["a"]);

    drop(run(&mut refreshing(settings.clone()), &harness));

    // Filter "a" put Issue 2 on the Workbench, and it archives as it closes. The pin holds Issue 1 there.
    assert_eq!(archived(&harness), [ItemKey::Issue { number: 2 }]);
    let pinned = listed(
        &harness,
        &settings,
        &WorkbenchView::Whole,
        ItemKey::Issue { number: 1 },
    );
    assert_eq!(pinned.map(|listed| listed.hidden), Some(None));
    harness.finish();
}

#[test]
fn a_refresh_reads_the_tree_of_each_issue_pinned_since_the_last_one_and_fetches_one_it_lacks() {
    let merged = json!({
        "title": "gh-1: Fix it",
        "author": { "login": "some-author" },
        "state": "MERGED",
        "mergeCommit": { "oid": "0".repeat(40) },
        "updatedAt": LONG_BEFORE,
    });
    let replies = vec![
        polled(&list(&[], None), &list(&[], None), &[]),
        issues(&[3], RATE_LIMIT),
        cross_references(&[30], None, RATE_LIMIT),
        no_cross_references(RATE_LIMIT),
        fetched_nodes(&[pr_node(30, &merged)], RATE_LIMIT),
    ];
    let harness = after_a_sync("pinned-trees", replies);
    holding_issue(&harness, 1, LONG_BEFORE);
    holding_issue(&harness, 2, LONG_BEFORE);
    let store = &harness.db.store;
    store.pin(&cpython(), 1, moment(LAST_SYNC)).unwrap();
    store.pin(&cpython(), 2, moment(LONG_BEFORE)).unwrap();
    store.pin(&cpython(), 3, moment(LONG_BEFORE)).unwrap();

    drop(run(&mut refreshing(filtering(&[])), &harness));

    // Issue 2 was pinned before the last refresh, which read its tree, and nothing changed on it since.
    assert_eq!(
        harness.operations(),
        [
            "RefreshRepository",
            "FetchIssues",
            "FetchCrossReferences",
            "FetchCrossReferences",
            "FetchTreePullRequests"
        ]
    );
    assert_eq!(harness.numbers(), [30]);
    assert_eq!(harness.issue_numbers(), [1, 2, 3]);
    harness.finish();
}

/// Archives the items `keys` name as a sync does when it sees them close.
fn archived_on_closing(harness: &Harness, keys: &[ItemKey]) {
    crate::store::testing::archived_on_closing(&harness.db.store, keys, moment(LAST_SYNC));
}

/// Item `key` as `view` lists it, read the way the window reads it, with `settings`.
fn listed(
    harness: &Harness,
    settings: &RepositorySettings,
    view: &WorkbenchView,
    key: ItemKey,
) -> Option<ListedItem> {
    let page = Workbench::read(&harness.db.reader, settings, view, ListOrder::Attention).unwrap();
    page.items.into_iter().find(|listed| listed.key == key)
}

/// Why item `key` Needs attention where the Workbench lists it. None, if the Workbench leaves it out or
/// the Archived view lists it.
fn why_back(harness: &Harness, settings: &RepositorySettings, key: ItemKey) -> Option<Vec<Why>> {
    let back = listed(harness, settings, &WorkbenchView::Whole, key)?;
    let archived = listed(harness, settings, &WorkbenchView::Archived, key);
    archived.is_none().then(|| {
        back.attention
            .map_or_else(Vec::new, |attention| attention.why)
    })
}

#[test]
fn a_refresh_brings_back_a_closed_archived_issue_when_new_activity_on_it_needs_attention() {
    let comment = json!({
        "__typename": "IssueComment",
        "createdAt": AFTER_IT,
        "author": { "__typename": "User", "login": "some-author" },
    });
    let mut node = issue_in(1, AFTER_IT, "CLOSED");
    node["author"] = json!({ "login": "octocat" });
    node["timelineItems"] = json!({ "nodes": [comment] });
    let replies = vec![
        polled(
            &list(&[node], None),
            &list(&[], None),
            &[search(&[issue_hit(1)], None)],
        ),
        no_cross_references(RATE_LIMIT),
    ];
    let harness = after_a_sync("archived-issue-back", replies);
    let own_closed = IssueFacts {
        author: Some(me()),
        state: Some(IssueState::Closed),
        updated_at: moment(LONG_BEFORE),
        ..issue_facts(1)
    };
    (harness.db.store)
        .save_issues(&cpython(), vec![own_closed])
        .unwrap();
    stored_filter(&harness, "a", &[1]);
    let key = ItemKey::Issue { number: 1 };
    archived_on_closing(&harness, &[key]);
    let settings = filtering(&["a"]);
    let put_away = listed(&harness, &settings, &WorkbenchView::Archived, key).is_some();

    drop(run(&mut refreshing(settings.clone()), &harness));

    // The user opened Issue 1, and a colleague commented on it after it closed.
    assert!(put_away);
    let by = Login::new("some-author");
    assert_eq!(
        why_back(&harness, &settings, key),
        Some(vec![Why::Comment { by }])
    );
    harness.finish();
}

#[test]
fn a_refresh_brings_back_an_archived_merged_pr_when_the_bot_says_its_backport_failed() {
    let after = merged_pr("octocat", AFTER_IT, &[backport_conflict()]);
    let replies = vec![
        fetched_nodes(&[after], RATE_LIMIT),
        // An Archived item stays in Reach, so the refresh reads ahead the PR's Conversation.
        quiet_conversation(),
    ];
    let harness = holding_a_merged_pr_that_moves("archived-pr-back", "octocat", replies);
    let key = ItemKey::PullRequest { number: 3 };
    archived_on_closing(&harness, &[key]);
    let settings = filtering(&[]);
    let put_away = listed(&harness, &settings, &WorkbenchView::Archived, key).is_some();

    drop(run(&mut refreshing(settings.clone()), &harness));

    assert!(put_away);
    let target = "3.14".to_owned();
    assert_eq!(
        why_back(&harness, &settings, key),
        Some(vec![Why::BackportFailed { target }])
    );
    harness.finish();
}

#[test]
fn a_refresh_leaves_a_muted_pr_hidden_on_new_activity_and_shows_it_on_a_review_request() {
    const LATER: &str = "2026-11-04T11:00:00Z";
    // A colleague's PR 3 that the user reviewed, with the Review requests and the timeline given.
    let reviewed = |updated_at: &str, requests: &[Value], timeline: &[Value]| {
        let changes = json!({
            "author": { "login": "some-author" },
            "title": "A change",
            "updatedAt": updated_at,
            "latestReviews": { "nodes": [{ "author": { "login": "octocat" }, "state": "COMMENTED" }] },
            "reviewRequests": { "nodes": requests },
            "timelineItems": { "nodes": timeline },
        });
        pr_node(3, &changes)
    };
    let comment = json!({
        "__typename": "IssueComment",
        "createdAt": AFTER_IT,
        "author": { "__typename": "User", "login": "some-author" },
        "bodyText": "One more thing.",
    });
    let commented = reviewed(AFTER_IT, &[], std::slice::from_ref(&comment));
    let asked = json!({
        "__typename": "ReviewRequestedEvent",
        "createdAt": LATER,
        "requestedReviewer": { "login": "octocat" },
    });
    let request = json!({ "requestedReviewer": { "login": "octocat" } });
    let requested = reviewed(LATER, &[request], &[comment, asked]);
    let replies = vec![
        polled(
            &list(&[], None),
            &list(&[changed_pr(3, AFTER_IT)], None),
            &[],
        ),
        fetched_nodes(&[commented], RATE_LIMIT),
        polled(&list(&[], None), &list(&[changed_pr(3, LATER)], None), &[]),
        fetched_nodes(&[requested], RATE_LIMIT),
        // The Review request puts the tree back in Reach, so the refresh reads ahead the Conversation.
        quiet_conversation(),
    ];
    let harness = after_a_sync("muted-pr", replies);
    holding_pr(&harness, &reviewed(LONG_BEFORE, &[], &[]));
    let key = ItemKey::PullRequest { number: 3 };
    let muting = Muting {
        key,
        numbers: vec![3],
        up_to: Some(moment(LAST_SYNC)),
        findings: vec![],
    };
    (harness.db.store)
        .mute(&cpython(), muting, moment(LAST_SYNC))
        .unwrap();
    let settings = filtering(&[]);

    drop(run(&mut refreshing(settings.clone()), &harness));
    let on_comment = listed(&harness, &settings, &WorkbenchView::Whole, key);
    let saved = harness.db.store.activity(&cpython(), 3).unwrap();
    // The refresh started on the computer's clock, and the poll drops each PR updated before the last
    // start, so the next poll starts from `LAST_SYNC` again.
    synced(&harness, ChangeCheck::Skipped);
    drop(run(&mut refreshing(settings.clone()), &harness));
    let on_request = listed(&harness, &settings, &WorkbenchView::Whole, key).unwrap();

    // The user reviewed PR 3, so a colleague's comment would need attention on an item not Muted.
    assert_eq!(saved, [comment_after_it("some-author")]);
    assert_eq!(on_comment.map(|listed| listed.key), None);
    assert_eq!(on_request.hidden, Some(Hidden::Muted));
    let why = on_request.attention.map(|attention| attention.why);
    assert_eq!(why, Some(vec![Why::ReviewRequested]));
    harness.finish();
}

/// A harness that holds the user's Issue 1, closed and Archived on closing, whose next poll finds a
/// colleague's new PR 5 that fixes it.
fn a_pr_opens_on_an_archived_issue(name: &str) -> Harness {
    let follow_up = json!({
        "title": "gh-1: Follow up",
        "author": { "login": "some-author" },
        "updatedAt": AFTER_IT,
    });
    let replies = vec![
        polled(
            &list(&[], None),
            &list(&[changed_pr(5, AFTER_IT)], None),
            &[],
        ),
        fetched_nodes(&[pr_node(5, &follow_up)], RATE_LIMIT),
    ];
    let harness = after_a_sync(name, replies);
    let own_closed = IssueFacts {
        author: Some(me()),
        state: Some(IssueState::Closed),
        updated_at: moment(LONG_BEFORE),
        ..issue_facts(1)
    };
    (harness.db.store)
        .save_issues(&cpython(), vec![own_closed])
        .unwrap();
    archived_on_closing(&harness, &[ItemKey::Issue { number: 1 }]);
    harness
}

/// The items the Inbox of `settings` lists, the latest update first.
fn inbox_items(harness: &Harness, settings: &RepositorySettings) -> Vec<ListedItem> {
    let read = Inbox::read(
        &harness.db.reader,
        settings,
        InboxPage::All,
        moment(AFTER_IT),
    );
    read.unwrap().items
}

/// The numbers of the items the Inbox of `settings` lists, each with the PRs under it.
fn inbox(harness: &Harness, settings: &RepositorySettings) -> Vec<Vec<u32>> {
    let items = inbox_items(harness, settings);
    items.iter().map(|listed| listed.item.numbers()).collect()
}

#[test]
fn a_refresh_brings_an_archived_issue_back_to_the_inbox_when_a_pr_opens_on_it() {
    let harness = a_pr_opens_on_an_archived_issue("pr-opens-on-archived");
    let settings = filtering(&[]);
    let key = ItemKey::Issue { number: 1 };
    let before = inbox(&harness, &settings);

    drop(run(&mut refreshing(settings.clone()), &harness));

    assert_eq!(before, Vec::<Vec<u32>>::new());
    assert_eq!(archived(&harness), []);
    assert_eq!(inbox(&harness, &settings), [vec![1, 5]]);
    // The user opened Issue 1, and still nothing on it Needs attention, so no Filter puts it on the
    // Workbench.
    let attention = inbox_items(&harness, &settings).remove(0).attention;
    assert_eq!(attention, None);
    let on_workbench = listed(&harness, &settings, &WorkbenchView::Whole, key);
    assert_eq!(on_workbench.map(|listed| listed.key), None);
    harness.finish();
}

#[test]
fn an_issue_a_pr_brought_back_stays_out_of_archived_on_mark_seen_and_leaves_the_inbox_on_archive() {
    let harness = a_pr_opens_on_an_archived_issue("brought-back-then-archived");
    let settings = filtering(&[]);
    drop(run(&mut refreshing(settings.clone()), &harness));
    let store = &harness.db.store;

    store
        .mark_seen(&cpython(), seeing(vec![1, 5], Some(moment(AFTER_IT))))
        .unwrap();
    let after_seen = (archived(&harness), inbox(&harness, &settings));
    let archiving = Archiving {
        key: ItemKey::Issue { number: 1 },
        numbers: vec![1, 5],
        up_to: Some(moment(AFTER_IT)),
        put_away: PutAway::Now,
        findings: vec![],
    };
    store
        .archive(&cpython(), archiving, moment(AFTER_IT))
        .unwrap();
    let after_archive = inbox(&harness, &settings);
    store
        .unarchive(&cpython(), ItemKey::Issue { number: 1 })
        .unwrap();

    assert_eq!(after_seen, (vec![], vec![vec![1, 5]]));
    assert_eq!(after_archive, Vec::<Vec<u32>>::new());
    // Unarchive puts a closed Issue back where its state puts it, which isn't the Inbox.
    assert_eq!(inbox(&harness, &settings), Vec::<Vec<u32>>::new());
    harness.finish();
}
