use serde_json::json;

use super::*;
use crate::error::ErrorKind;
use crate::github::testing::{
    Scripted, erring, github, issue_node, item_answer, not_found, pr_node,
};
use crate::github::{Reply, Unreachable};
use crate::testing::cpython;

fn fetched(reply: Result<Reply, Unreachable>) -> Numbered {
    let (script, _asked) = Scripted::new(vec![reply]);
    github(script).fetch_item(&cpython(), 12).unwrap().numbered
}

#[test]
fn a_number_that_names_an_issue_comes_back_as_the_issue() {
    let (script, asked) = Scripted::new(vec![item_answer(issue_node(12))]);

    let fetched = github(script).fetch_item(&cpython(), 12).unwrap();

    let Numbered::Issue(facts, _) = fetched.numbered else {
        panic!("{:?}", fetched.numbered);
    };
    assert_eq!(facts.number, 12);
    assert_eq!(fetched.rate_limit.remaining, 4000);
    let (operation, variables) = asked.recv().unwrap();
    assert_eq!(operation, "FetchItem");
    assert_eq!(
        variables,
        json!({ "owner": "python", "name": "cpython", "number": 12 })
    );
}

#[test]
fn a_number_that_names_a_pr_comes_back_as_the_pr() {
    let mut pr = pr_node(12, &json!({}));
    pr["__typename"] = json!("PullRequest");

    let Numbered::PullRequest(facts, _) = fetched(item_answer(pr)) else {
        panic!("not a PR");
    };
    assert_eq!(facts.number, 12);
}

#[test]
fn a_number_that_names_nothing_comes_back_missing() {
    let gone = not_found(
        json!({ "repository": { "issueOrPullRequest": null } }),
        json!(["repository", "issueOrPullRequest"]),
    );
    assert_eq!(fetched(gone), Numbered::Missing);
}

#[test]
fn an_issue_transferred_away_comes_back_missing() {
    assert_eq!(fetched(item_answer(issue_node(99))), Numbered::Missing);
}

#[test]
fn an_item_github_refuses_alone_comes_back_hidden() {
    let refused = erring(
        json!({ "repository": { "issueOrPullRequest": null } }),
        (
            "FORBIDDEN",
            "Resource not accessible by personal access token",
        ),
        json!(["repository", "issueOrPullRequest"]),
    );
    assert_eq!(fetched(refused), Numbered::Hidden);
}

#[test]
fn a_repository_the_token_cant_see_fails_as_not_found() {
    let gone = not_found(json!({ "repository": null }), json!(["repository"]));
    let (script, _asked) = Scripted::new(vec![gone]);

    let failed = github(script).fetch_item(&cpython(), 12).unwrap_err();

    assert_eq!(failed.kind, ErrorKind::NotFound);
}

#[test]
fn the_query_asks_for_either_kind_under_one_field() {
    let query = fetch_item_query();
    assert!(query.starts_with("query FetchItem("), "{query}");
    assert!(
        query.contains("issueOrPullRequest(number: $number)"),
        "{query}"
    );
    assert!(query.contains("... on Issue {"), "{query}");
    assert!(query.contains("... on PullRequest {"), "{query}");
}
