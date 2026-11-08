use serde_json::Value;

use super::*;
use crate::github::pull_requests::{PrState, PullRequestFacts};
use crate::store::ActivityFacts;
use crate::workbench::testing::{
    at, cpython, first_issue, fixing, issue, mine, outline, reasons_for, stranger, workbench,
};

#[test]
fn an_issue_with_no_pr_of_the_users_isnt_listed() {
    let other = PullRequestFacts {
        title: "gh-1: Fix it".to_owned(),
        ..stranger()
    };
    let found = workbench(vec![other], vec![issue(1), issue(2)]);
    assert_eq!(outline(&found), []);
}

#[test]
fn a_pr_of_someone_elses_with_no_issue_isnt_listed() {
    let found = workbench(vec![stranger()], vec![]);
    assert_eq!(outline(&found), []);
}

#[test]
fn items_come_latest_first_and_a_tie_goes_to_the_larger_number() {
    let latest = PullRequestFacts {
        updated_at: at(9),
        ..mine(30)
    };
    let tied = PullRequestFacts {
        updated_at: at(5),
        ..mine(20)
    };
    let under = PullRequestFacts {
        updated_at: at(5),
        ..fixing(10, 1)
    };
    let found = workbench(vec![tied, latest, under], vec![issue(1)]);
    assert_eq!(
        outline(&found),
        [
            ("standalone", 30, vec![]),
            ("standalone", 20, vec![]),
            ("issue", 1, vec![10]),
        ]
    );
    assert_eq!(first_issue(&found).number, 1);
}

#[test]
fn the_window_gets_the_kind_of_each_item_and_its_times_as_text() {
    let found = workbench(vec![mine(3), fixing(4, 1)], vec![issue(1)]);
    let sent = serde_json::to_value(&found).unwrap();
    assert_eq!(sent["items"][0]["kind"], "standalone_pr");
    assert_eq!(sent["items"][0]["number"], 3);
    assert_eq!(sent["items"][0]["updated_at"], "1970-01-01T00:00:01Z");
    assert_eq!(sent["items"][0]["attention"], Value::Null);
    assert_eq!(sent["quiet"][0]["kind"], "issue");
    assert_eq!(sent["quiet"][0]["updated_at"], "1970-01-01T00:00:01Z");
    assert_eq!(sent["quiet"][0]["pull_requests"][0]["kind"], "pr");
    assert_eq!(sent["quiet"][0]["pull_requests"][0]["number"], 4);
}

#[test]
fn a_merged_pr_of_the_users_keeps_no_issue_on_the_workbench() {
    let merged = PullRequestFacts {
        state: PrState::Merged,
        ..fixing(10, 1)
    };
    assert_eq!(reasons_for(&merged), []);
    assert_eq!(outline(&workbench(vec![merged], vec![issue(1)])), []);
}

#[test]
fn nothing_is_the_users_before_anyone_signs_in() {
    let found = Workbench::from_facts(
        WorkFacts {
            activity: ActivityFacts::default(),
            viewer: None,
            pull_requests: vec![mine(1)],
            issues: vec![issue(1)],
            filters: BTreeMap::new(),
        },
        &cpython(),
        &WorkbenchView::Whole,
    );
    assert_eq!(found.items, []);
}
