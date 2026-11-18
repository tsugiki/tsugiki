use crate::github::testing::{not_found, pull_request_answer};
use crate::sync::Syncs;
use crate::sync::recovery::{Scenario, recovers_from_each_failure, resumes_after_each_step};
use crate::sync::testing::{
    FEW, Harness, LARGE, RATE_LIMIT, comment_by, counted, cpython, cross_references, fetched_nodes,
    hours_ago_text, issue_references, issue_references_with_more, issues, merged_node,
    no_issue_references, nothing, plain, pr_node, run_to_the_end, search_page, searches_finding,
    up_to_the_first_issue, window_issue_referencing, window_page, window_pr,
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

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

#[test]
fn a_first_sync_keeps_what_happened_on_the_prs_it_fetched_and_on_their_origin_prs() {
    let timeline = |login: &str| json!({ "nodes": [comment_by(login, &hours_ago_text(1))] });
    let mut backport = backport_node(1, 9);
    backport["timelineItems"] = timeline("some-reviewer");
    let mut origin = merged_node(9);
    origin["timelineItems"] = timeline("some-author");
    let replies = vec![
        search_page(&["a"], None, RATE_LIMIT),
        nothing(RATE_LIMIT),
        nothing(RATE_LIMIT),
        nothing(RATE_LIMIT),
        fetched_nodes(&[backport], RATE_LIMIT),
        pull_request_answer(origin, RATE_LIMIT),
        issues(&[1], RATE_LIMIT),
        no_issue_references(RATE_LIMIT),
    ];
    let harness = Harness::new("origin-activity", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(harness.activity_by(1), ["some-reviewer"]);
    assert_eq!(harness.activity_by(9), ["some-author"]);
    harness.finish();
}

#[test]
fn a_first_sync_recovers_from_a_failure_at_each_point() {
    let posts = vec![
        search_page(&["a"], Some("next"), RATE_LIMIT),
        search_page(&["b"], None, RATE_LIMIT),
        nothing(RATE_LIMIT),
        nothing(RATE_LIMIT),
        nothing(RATE_LIMIT),
        fetched_nodes(&[pr_node(1, &json!({})), backport_node(2, 9)], RATE_LIMIT),
        pull_request_answer(merged_node(9), RATE_LIMIT),
        issues(&[1], RATE_LIMIT),
        counted(LARGE),
        issue_references_with_more(1, &[3], "older", RATE_LIMIT),
        cross_references(&[4, 21], None, RATE_LIMIT),
        fetched_nodes(
            &[merged_node(3), backport_node(4, 8), merged_node(21)],
            RATE_LIMIT,
        ),
        pull_request_answer(merged_node(8), RATE_LIMIT),
        window_page(
            &[window_issue_referencing(20, &hours_ago_text(1), &[22])],
            None,
            RATE_LIMIT,
        ),
        fetched_nodes(&[merged_node(22)], RATE_LIMIT),
        window_page(
            &[window_pr(23, &hours_ago_text(2))],
            Some("older"),
            RATE_LIMIT,
        ),
        fetched_nodes(&[merged_node(23)], RATE_LIMIT),
        window_page(&[window_pr(21, &hours_ago_text(3))], None, RATE_LIMIT),
    ];
    let scenario = Scenario {
        name: "recovery-first",
        settings: plain(cpython()),
        prepare: |_| {},
        posts,
        checks: Vec::new(),
    };

    let operations = recovers_from_each_failure(&scenario);
    resumes_after_each_step(&scenario);

    assert_eq!(
        operations,
        [
            "SearchReviewRequests",
            "SearchReviewRequests",
            "SearchReviewedPrs",
            "SearchOwnPrs",
            "SearchAssignedPrs",
            "FetchPullRequests",
            "FetchOriginPullRequest",
            "FetchIssues",
            "CheckRepository",
            "FetchIssueReferences",
            "FetchCrossReferences",
            "FetchTreePullRequests",
            "FetchOriginPullRequest",
            "WindowIssues",
            "FetchTreePullRequests",
            "WindowPullRequests",
            "FetchTreePullRequests",
            "WindowPullRequests",
        ]
    );
}

#[test]
fn a_sync_fetches_a_users_backports_missing_origin_pr_by_number() {
    let mut replies = searches_finding(&["a"]);
    replies.push(fetched_nodes(&[backport_node(2, 9)], RATE_LIMIT));
    replies.push(pull_request_answer(merged_node(9), RATE_LIMIT));
    replies.push(issues(&[1], RATE_LIMIT));
    replies.push(no_issue_references(RATE_LIMIT));
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
            "FetchIssueReferences",
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
    replies.push(fetched_nodes(&[untitled], RATE_LIMIT));
    replies.push(pull_request_answer(merged_node(9), RATE_LIMIT));
    replies.push(issues(&[1], RATE_LIMIT));
    replies.push(no_issue_references(RATE_LIMIT));
    let harness = Harness::new("origin-issue", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(harness.issue_numbers(), [1]);
    harness.finish();
}

#[test]
fn a_sync_fetches_a_tree_backports_missing_origin_pr_after_the_tree() {
    let mut replies = up_to_the_first_issue();
    replies.push(issue_references(&[(1, &[2])], RATE_LIMIT));
    replies.push(fetched_nodes(&[backport_node(2, 9)], RATE_LIMIT));
    replies.push(pull_request_answer(merged_node(9), RATE_LIMIT));
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
        RATE_LIMIT,
    ));
    replies.push(issues(&[1], RATE_LIMIT));
    replies.push(no_issue_references(RATE_LIMIT));
    // The next sync finds only the Backport, and its Origin PR has merged by then.
    replies.extend(searches_finding(&["a"]));
    replies.push(fetched_nodes(&[backport_node(2, 8)], RATE_LIMIT));
    replies.push(pull_request_answer(merged_node(8), RATE_LIMIT));
    replies.push(issues(&[1], RATE_LIMIT));
    replies.push(no_issue_references(RATE_LIMIT));
    replies.extend(searches_finding(&["a"]));
    replies.push(fetched_nodes(&[backport_node(2, 8)], RATE_LIMIT));
    replies.push(issues(&[1], RATE_LIMIT));
    replies.push(no_issue_references(RATE_LIMIT));
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
    replies.push(fetched_nodes(&[untitled(2)], RATE_LIMIT));
    replies.push(pull_request_answer(merged_node(9), RATE_LIMIT));
    replies.push(issues(&[1], RATE_LIMIT));
    replies.push(no_issue_references(RATE_LIMIT));
    replies.extend(searches_finding(&["b"]));
    replies.push(fetched_nodes(&[untitled(3)], RATE_LIMIT));
    replies.push(issues(&[1], RATE_LIMIT));
    replies.push(issue_references(&[(1, &[])], RATE_LIMIT));
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
        ["FetchPullRequests", "FetchIssues", "FetchIssueReferences"]
    );
    harness.finish();
}

#[test]
fn an_origin_number_that_names_no_pr_is_asked_for_once() {
    let mut replies = searches_finding(&["a"]);
    replies.push(fetched_nodes(&[backport_node(2, 9)], RATE_LIMIT));
    replies.push(not_found(
        json!({ "repository": { "pullRequest": null } }),
        json!(["repository", "pullRequest"]),
    ));
    replies.push(issues(&[1], RATE_LIMIT));
    replies.push(no_issue_references(RATE_LIMIT));
    let harness = Harness::new("origin-gone", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(harness.numbers(), [2]);
    harness.finish();
}

#[test]
fn an_origin_fetch_below_the_floor_holds_the_next_step_back() {
    let mut replies = searches_finding(&["a"]);
    replies.push(fetched_nodes(&[backport_node(2, 9)], RATE_LIMIT));
    replies.push(pull_request_answer(merged_node(9), FEW));
    let harness = Harness::new("origin-floor", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    let before = Instant::now();

    // Four searches, two moves, the PR, a move and the Origin PR.
    for _ in 0..9 {
        drop(syncs.step(&harness.github, &harness.db.store).unwrap());
    }

    let ready = syncs.ready_at().unwrap();
    assert!(ready > before + Duration::from_secs(3000), "{ready:?}");
    harness.finish();
}
