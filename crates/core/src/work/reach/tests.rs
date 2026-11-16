//! Reach, from each Issue and PR's place on the Workbench and the links between them.

use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::facts::activity::ActivityFacts;
use crate::facts::issues::IssueFacts;
use crate::facts::patch_ids::ReviewFacts;
use crate::facts::pull_requests::{PrState, PullRequestFacts};
use crate::workbench::WorkbenchView;
use crate::workbench::testing::{
    backport, bots_backport, cpython, fixing, issue, me, mine, rows_of, someone, stranger, view_of,
};

fn facts(pull_requests: Vec<PullRequestFacts>, issues: Vec<IssueFacts>) -> WorkFacts {
    WorkFacts {
        viewer: Some(me()),
        pull_requests,
        issues,
        filters: BTreeMap::new(),
        activity: ActivityFacts::default(),
        reviews: ReviewFacts::default(),
    }
}

/// A colleague's PR `number` that names Issue `issue`, in `state`.
fn colleagues(number: u32, issue: u32, state: PrState) -> PullRequestFacts {
    PullRequestFacts {
        number,
        title: format!("gh-{issue}: Fix it too"),
        state,
        ..stranger()
    }
}

#[test]
fn a_pr_in_a_reached_issues_tree_is_reached_open_merged_or_closed() {
    let pull_requests = vec![
        fixing(10, 1),
        colleagues(11, 1, PrState::Open),
        colleagues(12, 1, PrState::Merged),
        colleagues(13, 1, PrState::Closed),
        colleagues(14, 2, PrState::Open),
    ];

    let reached = reach(&facts(pull_requests, vec![issue(1), issue(2)]), &cpython());

    // The user's PR 10 puts Issue 1 on the Workbench. Nothing puts Issue 2 there.
    assert_eq!(reached, BTreeSet::from([1, 10, 11, 12, 13]));
}

#[test]
fn a_backport_of_a_reached_pr_is_reached_and_so_is_the_origin_pr_of_a_reached_backport() {
    let asked = PullRequestFacts {
        review_requests: vec![me()],
        ..backport(31, "[3.14] Tidy up (GH-30)", vec![Some(someone())])
    };
    let pull_requests = vec![
        mine(20),
        bots_backport(21, "[3.14] PR 20 (GH-20)"),
        PullRequestFacts {
            number: 30,
            title: "Tidy up".to_owned(),
            ..stranger()
        },
        asked,
        bots_backport(32, "[3.14] Tidy up (GH-30)"),
        PullRequestFacts {
            number: 40,
            title: "Tidy up more".to_owned(),
            ..stranger()
        },
        bots_backport(41, "[3.14] Tidy up more (GH-40)"),
    ];

    let reached = reach(&facts(pull_requests, vec![]), &cpython());

    // The user's PR 20 reaches its Backport 21. The Review request on Backport 31 reaches its Origin PR 30,
    // and through it Backport 32.
    assert_eq!(reached, BTreeSet::from([20, 21, 30, 31, 32]));
}

#[test]
fn an_issue_that_a_backport_of_the_users_pr_fixes_is_reached_with_its_tree() {
    let pull_requests = vec![
        mine(20),
        bots_backport(21, "[3.14] gh-5: Fix it (GH-20)"),
        colleagues(30, 5, PrState::Open),
    ];

    let reached = reach(&facts(pull_requests, vec![issue(5)]), &cpython());

    // The list draws PR 20 with its Backport under Issue 5, beside PR 30.
    assert_eq!(reached, BTreeSet::from([5, 20, 21, 30]));
}

#[test]
fn a_backport_of_a_backport_brings_no_family_of_its_own() {
    let asked = PullRequestFacts {
        review_requests: vec![me()],
        ..bots_backport(42, "[3.13] Tidy up (GH-41)")
    };
    let pull_requests = vec![
        PullRequestFacts {
            number: 40,
            title: "Tidy up".to_owned(),
            ..stranger()
        },
        bots_backport(41, "[3.14] Tidy up (GH-40)"),
        asked,
    ];

    let reached = reach(&facts(pull_requests, vec![]), &cpython());

    // A tree stays two levels deep, so Backport 42 stands on its own, as the list draws it.
    assert_eq!(reached, BTreeSet::from([42]));
}

/// PRs and Issues that put rows on the Workbench in each way it knows.
fn every_kind_of_tree() -> (Vec<PullRequestFacts>, Vec<IssueFacts>) {
    let asked_on_a_backport = PullRequestFacts {
        review_requests: vec![me()],
        ..backport(61, "[3.14] gh-6: Fix it (GH-60)", vec![Some(someone())])
    };
    let pull_requests = vec![
        fixing(10, 1),
        colleagues(11, 1, PrState::Merged),
        colleagues(12, 2, PrState::Closed),
        mine(20),
        bots_backport(21, "[3.14] gh-5: Fix it (GH-20)"),
        colleagues(30, 5, PrState::Open),
        colleagues(60, 6, PrState::Open),
        asked_on_a_backport,
        bots_backport(62, "[3.13] Fix it (GH-60)"),
        colleagues(63, 6, PrState::Closed),
        colleagues(70, 7, PrState::Open),
        PullRequestFacts {
            number: 80,
            title: "Tidy up".to_owned(),
            ..stranger()
        },
    ];
    let issues = [1, 2, 5, 6, 7].map(issue).to_vec();
    (pull_requests, issues)
}

#[test]
fn reach_holds_every_pr_the_whole_workbench_shows() {
    let (pull_requests, issues) = every_kind_of_tree();
    let shown = view_of(
        facts(pull_requests.clone(), issues.clone()),
        &WorkbenchView::Whole,
    );
    let rows: BTreeSet<u32> = shown
        .items
        .iter()
        .flat_map(|listed| rows_of(&listed.item))
        .map(|row| row.number)
        .collect();

    let reached = reach(&facts(pull_requests, issues), &cpython());

    assert_eq!(rows, BTreeSet::from([10, 11, 20, 21, 30, 60, 61, 62, 63]));
    assert!(rows.is_subset(&reached), "{rows:?} in {reached:?}");
}

#[test]
fn a_filters_find_is_reached_and_a_search_edited_away_reaches_nothing() {
    let repository = cpython();
    let found = |numbers: &[u32]| FilterResults {
        numbers: numbers.iter().copied().collect(),
        problem: None,
    };
    let filters = BTreeMap::from([
        (repository.filters[0].query.clone(), found(&[2])),
        (
            FilterQuery::try_from("is:pr author:nobody".to_owned()).unwrap(),
            found(&[3]),
        ),
    ]);
    let pull_requests = vec![
        colleagues(11, 2, PrState::Open),
        colleagues(12, 3, PrState::Open),
    ];
    let facts = WorkFacts {
        filters,
        ..facts(pull_requests, vec![issue(2), issue(3)])
    };

    assert_eq!(reach(&facts, &repository), BTreeSet::from([2, 11]));
}

#[test]
fn nothing_is_reached_before_anyone_signs_in() {
    let facts = WorkFacts {
        viewer: None,
        ..facts(vec![fixing(10, 1)], vec![issue(1)])
    };

    assert_eq!(reach(&facts, &cpython()), BTreeSet::new());
}
