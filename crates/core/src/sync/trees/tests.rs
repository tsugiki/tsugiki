use crate::sync::Syncs;
use crate::sync::testing::{
    FEW, Harness, LOTS, cpython, cross_references, fetched_nodes, issue_references,
    issue_references_with_more, issues, merged_node, plain, pr_node, run_to_the_end,
    searches_finding, up_to_the_first_issue,
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

#[test]
fn a_sync_reads_each_shown_issues_cross_references_and_fetches_the_prs_they_list() {
    let mut replies = up_to_the_first_issue();
    replies.push(issue_references(&[(1, &[2, 3])], LOTS));
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
            "FetchIssueReferences",
            "FetchTreePullRequests",
        ]
    );
    assert_eq!(harness.numbers(), [1, 2, 3]);
    harness.finish();
}

#[test]
fn a_sync_follows_an_issues_cross_references_page_by_page() {
    let mut replies = up_to_the_first_issue();
    replies.push(issue_references_with_more(1, &[3], "older", LOTS));
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
    assert_eq!(befores, [json!("older")]);
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
    replies.push(issue_references(&[(1, &[3, 1]), (2, &[3])], LOTS));
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
    replies.push(issue_references(&[(1, &[2, 3])], LOTS));
    replies.push(fetched_nodes(
        &[merged_node(2), pr_node(3, &json!({}))],
        LOTS,
    ));
    replies.extend(up_to_the_first_issue());
    replies.push(issue_references(&[(1, &[2, 3])], LOTS));
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
fn a_tree_read_below_the_floor_holds_the_next_step_back() {
    let mut replies = up_to_the_first_issue();
    replies.push(issue_references_with_more(1, &[], "older", FEW));
    let harness = Harness::new("tree-floor", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    let before = Instant::now();

    while !harness
        .asked
        .try_iter()
        .any(|(name, _)| name == "FetchIssueReferences")
    {
        drop(syncs.step(&harness.github, &harness.store).unwrap());
    }

    let ready = syncs.ready_at().unwrap();
    assert!(ready > before + Duration::from_secs(3000), "{ready:?}");
    harness.finish();
}
