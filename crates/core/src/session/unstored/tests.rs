use std::path::Path;

use serde_json::json;

use super::*;
use crate::github::testing::{
    Scripted, github, issue_item, item_answer, item_missing, item_refused, not_found, pr_item,
    reply_ok,
};
use crate::github::{Reply, Unreachable};
use crate::status::testing::quiet_board;
use crate::testing::gcmon;

fn item_7() -> Item {
    Item {
        repository: gcmon(),
        number: 7,
    }
}

/// What a fetch of #7 answered with `reply` came to, and the store it saved into.
fn fetched(
    reply: Result<Reply, Unreachable>,
) -> (Result<Result<Saved, ItemUnfetched>, Error>, Store) {
    let store = Store::open(Path::new(":memory:")).unwrap();
    let (script, _asked) = Scripted::new(vec![reply]);
    let fetched = fetch(&mut quiet_board(), &store, &github(script), &item_7());
    (fetched, store)
}

#[test]
fn an_issue_is_saved_and_said_to_be_one() {
    let (fetched, store) = fetched(item_answer(issue_item(7, &gcmon())));

    let saved = fetched.unwrap().unwrap();
    let change = Change {
        repository: gcmon(),
        items: vec![7],
    };
    assert_eq!(
        saved,
        Saved {
            kind: ItemFetch::Issue,
            change
        }
    );
    let issues = store.issues(&gcmon()).unwrap();
    assert_eq!(
        issues.iter().map(|issue| issue.number).collect::<Vec<_>>(),
        [7]
    );
}

#[test]
fn a_pr_is_saved_and_said_to_be_one() {
    let (fetched, store) = fetched(item_answer(pr_item(7)));

    assert_eq!(fetched.unwrap().unwrap().kind, ItemFetch::PullRequest);
    let facts = store.work_facts(&gcmon()).unwrap();
    let numbers: Vec<u32> = facts.pull_requests.iter().map(|pr| pr.number).collect();
    assert_eq!(numbers, [7]);
}

#[test]
fn a_number_that_names_nothing_is_missing() {
    let (fetched, store) = fetched(item_missing());

    assert_eq!(fetched.unwrap(), Err(ItemUnfetched::Missing));
    assert_eq!(store.issues(&gcmon()).unwrap(), []);
}

#[test]
fn an_item_github_refuses_is_hidden() {
    assert_eq!(
        fetched(item_refused()).0.unwrap(),
        Err(ItemUnfetched::Hidden)
    );
}

#[test]
fn a_repository_the_token_cant_see_hides_the_item() {
    let gone = not_found(json!({ "repository": null }), json!(["repository"]));
    assert_eq!(fetched(gone).0.unwrap(), Err(ItemUnfetched::Hidden));
}

#[test]
fn an_answer_that_cant_be_read_is_another_failure() {
    assert_eq!(
        fetched(reply_ok("not json")).0.unwrap(),
        Err(ItemUnfetched::Other)
    );
}

#[test]
fn a_failure_that_speaks_for_all_of_github_comes_back_for_the_status_bar() {
    let (fetched, _store) = fetched(Err(Unreachable::Offline));
    assert_eq!(fetched.unwrap_err().kind, ErrorKind::Offline);
}
