use super::*;
use crate::error::ErrorKind;
use crate::filters::Filter;
use crate::github::activity::{Activity, ActivityKind};
use crate::github::issues::IssueState;
use crate::github::pull_requests::{CiState, ReviewState};
use crate::github::testing::{dated, reply_ok, signed_in_as};
use crate::github::{ConditionalGet, ConditionalReply, Reply, Request, Transport, Unreachable};
use crate::repository::RepositoryName;
use crate::repository_settings::RepositorySettings;
use crate::status::Change;
use crate::store::{FilterProblem, Store, WorkFacts};
use crate::sync::recovery::{Scenario, recovers_from_each_failure};
use crate::sync::testing::{
    FEW, Harness, LOTS, cpython, cross_references, fetched_nodes, issues, no_cross_references,
    pr_node,
};
use crate::sync::{Progress, Syncs};
use crate::token::Token;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::mpsc::{self, Sender};
use time::format_description::well_known::Rfc3339;

const LAST_SYNC: &str = "2026-11-04T10:00:00Z";
const AFTER_IT: &str = "2026-11-04T10:30:00Z";
const LONG_BEFORE: &str = "2026-11-01T10:00:00Z";

fn at(text: &str) -> OffsetDateTime {
    OffsetDateTime::parse(text, &Rfc3339).unwrap()
}

fn filtering(queries: &[&str]) -> RepositorySettings {
    RepositorySettings {
        filters: queries
            .iter()
            .map(|text| Filter {
                name: format!("Named {text}").try_into().unwrap(),
                query: query(text),
            })
            .collect(),
        ..RepositorySettings::new(cpython())
    }
}

fn query(text: &str) -> FilterQuery {
    text.to_owned().try_into().unwrap()
}

fn me() -> Login {
    Login::new("octocat")
}

/// A harness whose Repository finished a sync at `LAST_SYNC`, and whose user is `octocat`.
fn after_a_sync(name: &str, replies: Vec<Result<Reply, Unreachable>>) -> Harness {
    let harness = Harness::new(name, replies);
    synced(&harness, ChangeCheck::Skipped);
    harness.store.remember_login(&me()).unwrap();
    harness
}

fn synced(harness: &Harness, check: ChangeCheck) {
    let state = SyncState {
        started_at: at(LAST_SYNC),
        finished_at: at(LAST_SYNC),
        check,
        refresh_under_way: false,
    };
    harness.store.save_sync_state(&cpython(), state).unwrap();
}

fn holding_issue(harness: &Harness, number: u32, updated_at: &str) {
    let issue = IssueFacts {
        number,
        title: format!("Issue {number}"),
        author: None,
        assignees: vec![],
        updated_at: at(updated_at),
        state: Some(IssueState::Open),
    };
    harness.store.save_issues(&cpython(), vec![issue]).unwrap();
}

/// PR `number` as a fetch would read it from `node`.
fn read_pr(node: &Value) -> Vec<PullRequestFacts> {
    let script = fetched_nodes(std::slice::from_ref(node), LOTS);
    let (script, _asked) = crate::github::testing::Scripted::new(vec![script]);
    let github = crate::github::GitHub::new(Box::new(script), crate::github::testing::token());
    let id: NodeId = serde_json::from_value(json!("id")).unwrap();
    let batch = github
        .fetch_pull_requests(PrFetch::Work, &[id], 25)
        .unwrap();
    batch.pull_requests
}

/// Saves the PR a fetch would read from `node`.
fn holding_pr(harness: &Harness, node: &Value) {
    harness
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
        .store
        .save_changed_pull_requests(&cpython(), (pull_requests, vec![]), changed)
        .unwrap();
    let state = SyncState {
        started_at: at(LAST_SYNC),
        finished_at: at(LAST_SYNC),
        check: ChangeCheck::Skipped,
        refresh_under_way: true,
    };
    harness.store.save_sync_state(&cpython(), state).unwrap();
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
        "timelineItems": { "nodes": [] },
        "author": null,
    })
}

fn list(nodes: &[Value], next: Option<&str>) -> Value {
    json!({
        "pageInfo": { "hasNextPage": next.is_some(), "endCursor": next },
        "nodes": nodes,
    })
}

fn search(nodes: &[Value], next: Option<&str>) -> Value {
    let mut answer = list(nodes, next);
    answer["issueCount"] = json!(nodes.len());
    answer
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
    let repository = json!({
        "openIssues": { "totalCount": open },
        "openPrs": { "totalCount": 0 },
        "issues": issues,
        "pullRequests": prs,
    });
    let fields: String = searches
        .iter()
        .enumerate()
        .map(|(position, answer)| format!(r#""f{position}":{answer},"#))
        .collect();
    reply_ok(&format!(
        r#"{{"data":{{"repository":{repository},{fields}{LOTS}}}}}"#
    ))
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
    reply_ok(&format!(
        r#"{{"data":{{"repository":{},{LOTS}}}}}"#,
        Value::Object(fields)
    ))
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
    let rate_limit = json!({ "remaining": 4000, "resetAt": "2999-01-01T00:00:00Z" });
    let data = json!({ "data": {
        "repository": { format!("p{number}"): pull_request },
        "rateLimit": rate_limit,
    } });
    reply_ok(&data.to_string())
}

/// Runs the refresh to its end, and answers with everything it announced.
fn run(syncs: &mut Syncs, harness: &Harness) -> Vec<Change> {
    let mut changes = Vec::new();
    while syncs.ready_at().is_some() {
        if let Some(Progress::Changed(change)) =
            syncs.step(&harness.github, &harness.store).unwrap()
        {
            changes.push(change);
        }
    }
    changes
}

fn refreshing(settings: RepositorySettings) -> Syncs {
    let mut syncs = Syncs::default();
    syncs.add(settings);
    syncs
}

fn facts(harness: &Harness) -> WorkFacts {
    harness.reader.work_facts(&cpython()).unwrap()
}

/// A Repository that finished a sync, holding what a poll and a probe can move.
fn holding_what_can_move(harness: &Harness) {
    synced(harness, ChangeCheck::Skipped);
    harness.store.remember_login(&me()).unwrap();
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
        fetched_nodes(&[strangers_pr(3, AFTER_IT)], LOTS),
        probed(&[(4, "FAILURE")]),
        failed_checks(4, &["Tests"]),
        reply_ok(&format!(
            r#"{{"data":{{"search":{},{LOTS}}}}}"#,
            search(&[issue_hit(2), issue_hit(5)], None)
        )),
        issues(&[2, 5, 6, 8], LOTS),
        cross_references(&[7], None, LOTS),
        no_cross_references(LOTS),
        no_cross_references(LOTS),
        no_cross_references(LOTS),
        no_cross_references(LOTS),
        fetched_nodes(&[strangers_pr(7, AFTER_IT)], LOTS),
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
        fetched_nodes(&[pr_node(4, &json!({ "updatedAt": AFTER_IT }))], LOTS),
        issues(&[1], LOTS),
        no_cross_references(LOTS),
        approved(),
    ];
    let scenario = Scenario {
        name: "recovery-redo",
        settings: filtering(&[]),
        prepare: |harness| {
            let state = SyncState {
                started_at: at(LAST_SYNC),
                finished_at: at(LAST_SYNC),
                check: ChangeCheck::Skipped,
                refresh_under_way: true,
            };
            harness.store.save_sync_state(&cpython(), state).unwrap();
            harness.store.remember_login(&me()).unwrap();
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
            harness.store.remember_login(&me()).unwrap();
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
        number,
        title: format!("Issue {number}"),
        author: None,
        assignees: vec![],
        updated_at: at(LONG_BEFORE),
        state: None,
    };
    harness.store.save_issues(&cpython(), vec![issue]).unwrap();
}

#[test]
fn a_refresh_fetches_the_issues_saved_before_tsugiki_kept_states_once() {
    let harness = after_a_sync(
        "states",
        vec![quiet_poll(), issues(&[1, 2], LOTS), quiet_poll()],
    );
    holding_issue_with_no_state(&harness, 1);
    holding_issue_with_no_state(&harness, 2);
    holding_issue(&harness, 3, LONG_BEFORE);
    let mut syncs = refreshing(filtering(&[]));

    let changes = run(&mut syncs, &harness);
    let first = harness.operations();
    syncs.add(filtering(&[]));
    drop(run(&mut syncs, &harness));

    let states: Vec<Option<IssueState>> = harness
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
        vec![quiet_poll(), issues(&[1], LOTS), quiet_poll()],
    );
    holding_issue_with_no_state(&harness, 1);
    holding_issue_with_no_state(&harness, 2);
    let mut syncs = refreshing(filtering(&[]));

    drop(run(&mut syncs, &harness));
    syncs.add(filtering(&[]));
    drop(run(&mut syncs, &harness));

    let states: Vec<Option<IssueState>> = harness
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
        syncs.step(&harness.github, &harness.store).unwrap(),
        Some(Progress::Finished(_))
    ) {}
}

#[test]
fn a_refresh_goes_before_the_fetch_of_the_issues_with_no_state() {
    let harness = after_a_sync(
        "states-behind",
        vec![quiet_poll(), quiet_poll(), issues(&[1], LOTS)],
    );
    holding_issue_with_no_state(&harness, 1);
    let mut syncs = refreshing(filtering(&[]));

    until_finished(&mut syncs, &harness);
    syncs.add(filtering(&[]));
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

    let failed = syncs.step(&harness.github, &harness.store).unwrap_err();
    let during = harness.store.sync_state(&cpython()).unwrap().unwrap();
    drop(run(&mut syncs, &harness));
    let after = harness.store.sync_state(&cpython()).unwrap().unwrap();

    assert_eq!(failed.problem.kind, ErrorKind::Offline);
    assert_eq!(during.started_at, at(LAST_SYNC));
    assert!(during.refresh_under_way);
    assert!(after.started_at > at(LAST_SYNC));
    assert!(!after.refresh_under_way);
    harness.finish();
}

#[test]
fn an_issue_the_poll_changed_is_saved_named_and_gets_its_tree_read() {
    let issues_list = list(&[issue_node(1, AFTER_IT)], None);
    let replies = vec![
        polled(&issues_list, &list(&[], None), &[]),
        cross_references(&[7], None, LOTS),
        fetched_nodes(&[strangers_pr(7, AFTER_IT)], LOTS),
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

#[test]
fn an_issue_nothing_shows_stays_out_and_an_unchanged_one_says_nothing() {
    let issues_list = list(&[issue_node(1, AFTER_IT), issue_node(2, LONG_BEFORE)], None);
    let harness = after_a_sync(
        "issue-ignored",
        vec![polled(&issues_list, &list(&[], None), &[])],
    );
    let same = IssueFacts {
        number: 2,
        title: "Issue 2 now".to_owned(),
        author: None,
        assignees: vec![],
        updated_at: at(LONG_BEFORE),
        state: Some(IssueState::Open),
    };
    harness.store.save_issues(&cpython(), vec![same]).unwrap();
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
            LOTS,
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
            LOTS,
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
        fetched_nodes(&[strangers_pr(1, LONG_BEFORE)], LOTS),
    ];
    let harness = after_a_sync("same-again", replies);
    holding_pr(&harness, &strangers_pr(1, LONG_BEFORE));
    let mut syncs = refreshing(filtering(&[]));

    let changes = run(&mut syncs, &harness);

    assert_eq!(changes, []);
    harness.finish();
}

#[test]
fn the_users_changed_pr_brings_its_issue_and_a_strangers_doesnt() {
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
            LOTS,
        ),
        probed(&[]),
        issues(&[40], LOTS),
        no_cross_references(LOTS),
    ];
    let harness = after_a_sync("whose", replies);
    let mut syncs = refreshing(filtering(&[]));

    drop(run(&mut syncs, &harness));

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    let issue_fetches: Vec<&(String, Value)> = asked
        .iter()
        .filter(|(name, _)| name == "FetchIssues")
        .collect();
    assert_eq!(issue_fetches.len(), 1);
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
    let read = harness.reader.work_facts(&cpython()).unwrap();
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
    let few_left = reply_ok(&format!(r#"{{"data":{{"repository":{{}},{FEW}}}}}"#));
    let replies = vec![quiet_poll(), probed(&first), few_left];
    let harness = after_a_sync("probe-batches", replies);
    for number in &many {
        holding_pr(&harness, &own_open_pr(*number, &json!({})));
    }
    let mut syncs = refreshing(filtering(&[]));

    drop(syncs.step(&harness.github, &harness.store).unwrap());
    drop(syncs.step(&harness.github, &harness.store).unwrap());
    drop(syncs.step(&harness.github, &harness.store).unwrap());
    drop(syncs.step(&harness.github, &harness.store).unwrap());
    drop(syncs.step(&harness.github, &harness.store).unwrap());
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
            LOTS,
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
    let pull_request = json!({
        "bodyHTML": "<p>A change</p>",
        "createdAt": "2026-11-01T09:00:00Z",
        "comments": { "totalCount": 0, "nodes": [] },
        "reviews": { "totalCount": 1, "nodes": [review] },
        "reviewThreads": { "nodes": [] },
        "headRefOid": "0123abc",
    });
    let rate_limit = json!({ "remaining": 4000, "resetAt": "2999-01-01T00:00:00Z" });
    let data = json!({ "repository": { "pullRequest": pull_request }, "rateLimit": rate_limit });
    reply_ok(&json!({ "data": data }).to_string())
}

/// The states of the Reviews Tsugiki holds of PR `number`, or nothing if it hasn't read its Conversation.
fn reviews_held(harness: &Harness, number: u32) -> Option<Vec<ReviewState>> {
    let (_, content) = harness
        .reader
        .pull_request_facts(&cpython(), number)
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
            LOTS,
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
            LOTS,
        ),
        // The query ran out of its 10 seconds on this PR.
        Ok(Reply {
            status: 502,
            reset: None,
            date: None,
            body: String::new(),
        }),
        approved(),
    ];
    let harness = after_a_sync("read-ahead-refused", replies);
    let mut syncs = refreshing(filtering(&[]));

    drop(run(&mut syncs, &harness));

    assert_eq!(reviews_held(&harness, 1), None);
    assert_eq!(reviews_held(&harness, 2), Some(vec![ReviewState::Approved]));
    let state = harness.store.sync_state(&cpython()).unwrap().unwrap();
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
            LOTS,
        ),
    ];
    let harness = Harness::new("nobody", replies);
    synced(&harness, ChangeCheck::Skipped);
    let mut syncs = refreshing(filtering(&[]));

    drop(run(&mut syncs, &harness));

    assert_eq!(
        harness.operations(),
        ["RefreshRepository", "FetchChangedPullRequests"]
    );
    harness.finish();
}

fn stored_filter(harness: &Harness, text: &str, numbers: &[u32]) {
    let results = crate::store::FilterResults {
        numbers: numbers.iter().copied().collect(),
        problem: None,
    };
    harness
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
        fetched_nodes(&[strangers_pr(2, &now)], LOTS),
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
    let origin = reply_ok(&format!(
        r#"{{"data":{{"repository":{{"pullRequest":{}}},{LOTS}}}}}"#,
        pr_node(
            9,
            &json!({ "state": "MERGED", "mergeCommit": { "oid": "0".repeat(40) } })
        ),
    ));
    let replies = vec![
        polled(
            &list(&[issue_node(1, AFTER_IT)], None),
            &list(&[changed_pr(10, AFTER_IT)], None),
            &[],
        ),
        fetched_nodes(&[backport], LOTS),
        cross_references(&[10], None, LOTS),
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
    let replies = vec![moved(), moved(), no_cross_references(LOTS)];
    let harness = after_a_sync("stopped-issue", replies);
    holding_issue(&harness, 1, LONG_BEFORE);

    let mut stopped = refreshing(filtering(&[]));
    drop(stopped.step(&harness.github, &harness.store).unwrap());
    drop(stopped);
    let mut syncs = refreshing(filtering(&[]));
    let changes = run(&mut syncs, &harness);

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(asked[1].0, "RefreshRepository");
    assert_eq!(asked[1].1["since"], "2026-11-04T09:58:00Z");
    assert_eq!(asked[2].0, "FetchCrossReferences");
    assert_eq!(changes, []);
    let state = harness.store.sync_state(&cpython()).unwrap().unwrap();
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
        issues(&[40], LOTS),
        no_cross_references(LOTS),
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
        harness.store.read_ahead_owed(&cpython()).unwrap(),
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
    reply_ok(&format!(r#"{{"data":{{"repository":{fields},{LOTS}}}}}"#))
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
        drop(stopped.step(&harness.github, &harness.store).unwrap());
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
        cross_references(&[], None, LOTS),
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
            LOTS,
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
        drop(stopped.step(&harness.github, &harness.store).unwrap());
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
    let answer = format!(r#"{{"data":{{"repository":{{"p1":{probe}}},{LOTS}}}}}"#);
    let harness = after_a_sync("probe-order", vec![quiet_poll(), reply_ok(&answer)]);
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
        issues(&[2], LOTS),
        no_cross_references(LOTS),
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
        issues(&[2, 3], LOTS),
        no_cross_references(LOTS),
        no_cross_references(LOTS),
        no_cross_references(LOTS),
    ];
    let harness = after_a_sync(&format!("poll-news-{lost:?}"), replies);
    holding_issue(&harness, 1, LONG_BEFORE);
    stored_filter(&harness, "a", &[]);
    stored_filter(&harness, "b", &[]);
    let mut syncs = refreshing(filtering(&["a", "b"]));
    let (flaky, calls) = harness.store.failing_at_call(lost);

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
        reply_ok(&format!(
            r#"{{"data":{{"search":{},{LOTS}}}}}"#,
            search(&[issue_hit(2), issue_hit(3)], None)
        )),
        issues(&[2, 3], LOTS),
        no_cross_references(LOTS),
        no_cross_references(LOTS),
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
        reply_ok(&format!(
            r#"{{"data":{{"search":{},{LOTS}}}}}"#,
            search(&[issue_hit(2)], None)
        )),
        issues(&[2], LOTS),
        no_cross_references(LOTS),
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
    let replies = vec![
        quiet_poll(),
        reply_ok(&format!(
            r#"{{"data":{{"search":{},{LOTS}}}}}"#,
            search(&[], None)
        )),
    ];
    let harness = after_a_sync("filter-refused-before", replies);
    stored_filter(&harness, "a", &[1]);
    harness
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
        reply_ok(&format!(
            r#"{{"data":{{"search":{},{LOTS}}}}}"#,
            search(&[issue_hit(2)], None)
        )),
        issues(&[2], LOTS),
        no_cross_references(LOTS),
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

    let problem = syncs.step(&harness.github, &harness.store).unwrap_err();

    assert_eq!(problem.problem.kind, ErrorKind::QueryRefused);
    harness.finish();
}

#[test]
fn a_poll_that_fails_with_searches_in_it_is_a_failure_too() {
    let harness = after_a_sync("poll-offline", vec![Err(Unreachable::Offline)]);
    stored_filter(&harness, "a", &[]);
    let mut syncs = refreshing(filtering(&["a"]));

    let problem = syncs.step(&harness.github, &harness.store).unwrap_err();

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
        drop(self.tags.send(request.etag.map(str::to_owned)));
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
    harness.store.remember_login(&me()).unwrap();
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
    harness.store.sync_state(&cpython()).unwrap().unwrap().check
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
    let after = harness.store.sync_state(&cpython()).unwrap().unwrap();
    assert_eq!(after.check, ChangeCheck::Tagged(Etag::new("\"same\"")));
    assert!(after.started_at > at(LAST_SYNC));
    harness.finish();
}

#[test]
fn a_304_still_runs_a_filter_that_has_never_searched() {
    let search_answer = reply_ok(&format!(
        r#"{{"data":{{"search":{},{LOTS}}}}}"#,
        search(&[], None)
    ));
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

    let state = harness.store.sync_state(&other).unwrap();

    assert_eq!(state, None);
    harness.finish();
}

/// The comment at `AFTER_IT` that these tests read back.
fn comment_after_it(who: &str) -> Activity {
    Activity {
        kind: ActivityKind::Comment,
        who: Login::new(who),
        at: at(AFTER_IT),
    }
}

/// A merged PR by someone else that Tsugiki holds, and the poll that says it moved, with GitHub's next
/// answer.
fn a_merged_pr_moves(name: &str, replies: Vec<Result<Reply, Unreachable>>) -> Harness {
    let moved = list(&[changed_pr(3, AFTER_IT)], None);
    let mut all = vec![polled(&list(&[], None), &moved, &[])];
    all.extend(replies);
    let harness = after_a_sync(name, all);
    let held = json!({
        "author": { "login": "some-author" },
        "state": "MERGED",
        "mergeCommit": { "oid": "0".repeat(40) },
        "updatedAt": LONG_BEFORE,
    });
    holding_pr(&harness, &pr_node(3, &held));
    drop(run(&mut refreshing(filtering(&[])), &harness));
    harness
}

#[test]
fn the_bots_comment_after_a_sync_saw_the_merge_still_reaches_the_store() {
    let conflict = json!({
        "__typename": "IssueComment",
        "createdAt": AFTER_IT,
        "author": { "__typename": "Bot", "login": "miss-islington-app" },
        "bodyText": "Sorry, I could not cleanly backport this to `3.14` due to a conflict.",
    });
    let after = json!({
        "author": { "login": "some-author" },
        "state": "MERGED",
        "mergeCommit": { "oid": "0".repeat(40) },
        "updatedAt": AFTER_IT,
        "timelineItems": { "nodes": [conflict] },
    });
    let replies = vec![fetched_nodes(&[pr_node(3, &after)], LOTS)];

    let harness = a_merged_pr_moves("merged-moved", replies);

    let events = harness.store.activity(&cpython(), 3).unwrap();
    assert_eq!(
        events,
        [Activity {
            kind: ActivityKind::BackportFailed {
                target: "3.14".to_owned()
            },
            who: Login::new("miss-islington-app"),
            at: at(AFTER_IT),
        }]
    );
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
    let held = harness.store.pull_requests(&cpython()).unwrap();
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
        cross_references(&[], None, LOTS),
    ];
    let harness = after_a_sync("issue-activity", replies);
    holding_issue(&harness, 1, LONG_BEFORE);

    drop(run(&mut refreshing(filtering(&[])), &harness));

    let events = harness.store.activity(&cpython(), 1).unwrap();
    assert_eq!(events, [comment_after_it("some-author")]);
    assert_eq!(harness.store.activity(&cpython(), 2).unwrap(), []);
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
    let answer = format!(r#"{{"data":{{"repository":{fields},{LOTS}}}}}"#);
    let harness = after_a_sync("probe-activity", vec![quiet_poll(), reply_ok(&answer)]);
    holding_pr(
        &harness,
        &own_open_pr(1, &json!({ "updatedAt": LONG_BEFORE })),
    );

    drop(run(&mut refreshing(filtering(&[])), &harness));

    assert_eq!(
        harness.store.activity(&cpython(), 1).unwrap(),
        [Activity {
            kind: ActivityKind::Review,
            who: Login::new("some-reviewer"),
            at: at(AFTER_IT),
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
        fetched_nodes(&[strangers_pr(2, &changed)], LOTS),
        approved(),
    ];
    let harness = on_githubs_clock("github-clock-settled", github_time, replies);
    stored_filter(&harness, "a", &[1, 2]);

    drop(run(&mut refreshing(filtering(&["a"])), &harness));

    let results = &facts(&harness).filters[&query("a")];
    assert_eq!(results.numbers, BTreeSet::from([1, 2]));
    harness.finish();
}
