use serde_json::json;

use super::*;
use crate::error::ErrorKind;
use crate::github::testing::{
    Scripted, github, issue_item, item_answer, item_missing, item_refused, items_answer,
    items_refused, not_found, pr_item,
};
use crate::github::{Reply, Unreachable};
use crate::testing::{cpython, gcmon};

/// What a fetch of #12 of python/cpython answered with `reply` came to.
fn fetched(reply: Result<Reply, Unreachable>) -> Numbered {
    let (script, _asked) = Scripted::new(vec![reply]);
    github(script).fetch_item(&cpython(), 12).unwrap().numbered
}

#[test]
fn a_number_that_names_an_issue_comes_back_as_the_issue() {
    let (script, asked) = Scripted::new(vec![item_answer(&cpython(), issue_item(12, &cpython()))]);

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
    let Numbered::PullRequest(facts, _) = fetched(item_answer(&cpython(), pr_item(12))) else {
        panic!("not a PR");
    };
    assert_eq!(facts.number, 12);
}

#[test]
fn a_number_that_names_nothing_comes_back_missing() {
    assert_eq!(fetched(item_missing()), Numbered::Missing);
}

#[test]
fn an_issue_transferred_away_comes_back_missing_under_any_number() {
    let moved = [issue_item(99, &gcmon()), issue_item(12, &gcmon())];

    let fetched = moved.map(|issue| fetched(item_answer(&cpython(), issue)));

    assert_eq!(fetched, [Numbered::Missing, Numbered::Missing]);
}

#[test]
fn a_repository_named_in_another_case_or_renamed_holds_its_issue() {
    let other_case = RepositoryName::parse("Python/CPython").unwrap();
    let renamed = RepositoryName::parse("python/cpython-renamed").unwrap();
    let held = [
        item_answer(&cpython(), issue_item(12, &other_case)),
        // GitHub finds a renamed Repository under its old name, and answers with its new one.
        item_answer(&renamed, issue_item(12, &renamed)),
    ];

    let fetched = held.map(fetched);

    assert!(
        fetched
            .iter()
            .all(|numbered| matches!(numbered, Numbered::Issue(..))),
        "{fetched:?}"
    );
}

#[test]
fn an_item_github_refuses_alone_comes_back_hidden() {
    assert_eq!(fetched(item_refused()), Numbered::Hidden);
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
        query.contains("{ nameWithOwner issueOrPullRequest(number: $number)"),
        "{query}"
    );
    assert!(
        query.contains("... on Issue { repository { nameWithOwner } "),
        "{query}"
    );
    assert!(query.contains("... on PullRequest {"), "{query}");
}

/// What a batch fetch of `numbers` of python/cpython answered with `reply` came to.
fn fetched_items(numbers: &[u32], reply: Result<Reply, Unreachable>) -> FetchedItems {
    let (script, _asked) = Scripted::new(vec![reply]);
    github(script).fetch_items(&cpython(), numbers, 50).unwrap()
}

#[test]
fn a_batch_query_names_each_number_under_an_alias() {
    let query = fetch_items_query(&[12, 13]);
    assert!(query.starts_with("query FetchItems("), "{query}");
    assert!(
        query.contains("i12: issueOrPullRequest(number: 12) { __typename"),
        "{query}"
    );
    assert!(
        query.contains("i13: issueOrPullRequest(number: 13) { __typename"),
        "{query}"
    );
    assert!(query.contains("... on Issue {"), "{query}");
    assert!(query.contains("... on PullRequest {"), "{query}");
}

#[test]
fn a_batch_comes_back_with_each_kind_and_the_numbers_github_has_nothing_under() {
    let answer = items_answer(
        &cpython(),
        &[
            (12, issue_item(12, &cpython())),
            (13, pr_item(13)),
            (14, serde_json::Value::Null),
        ],
    );

    let fetched = fetched_items(&[12, 13, 14], answer);

    assert_eq!(fetched.asked_for, 3);
    assert!(matches!(fetched.items[0], Numbered::Issue(..)));
    assert!(matches!(fetched.items[1], Numbered::PullRequest(..)));
    assert_eq!(fetched.items[2], Numbered::Missing);
    assert_eq!(fetched.rate_limit.remaining, 4000);
}

#[test]
fn a_batch_answers_a_number_github_refuses_alone_as_hidden_and_the_rest_stands() {
    let answer = items_refused(&cpython(), 12);

    let fetched = fetched_items(&[12, 13], answer);

    assert_eq!(fetched.items, [Numbered::Hidden, Numbered::Missing]);
}

#[test]
fn a_batch_leaves_out_an_issue_transferred_to_another_repository() {
    let answer = items_answer(&cpython(), &[(12, issue_item(12, &gcmon()))]);

    assert_eq!(fetched_items(&[12], answer).items, [Numbered::Missing]);
}
