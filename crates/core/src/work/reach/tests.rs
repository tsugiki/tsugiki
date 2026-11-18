//! Reach, from each Issue and PR's place on the Workbench and the links between them.

use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::facts::activity::ActivityFacts;
use crate::facts::issues::IssueFacts;
use crate::facts::patch_ids::ReviewFacts;
use crate::facts::pull_requests::{PrState, PullRequestFacts};
use crate::testing::{cpython_settings, sha};
use crate::workbench::WorkbenchView;
use crate::workbench::testing::{
    backport, bots_backport, fixing, issue, me, mine, rows_of, someone, stranger, view_of,
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

    let reached = reach(
        &facts(pull_requests, vec![issue(1), issue(2)]),
        &cpython_settings(),
    );

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

    let reached = reach(&facts(pull_requests, vec![]), &cpython_settings());

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

    let reached = reach(&facts(pull_requests, vec![issue(5)]), &cpython_settings());

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

    let reached = reach(&facts(pull_requests, vec![]), &cpython_settings());

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

    let reached = reach(&facts(pull_requests, issues), &cpython_settings());

    assert_eq!(rows, BTreeSet::from([10, 11, 20, 21, 30, 60, 61, 62, 63]));
    assert!(rows.is_subset(&reached), "{rows:?} in {reached:?}");
}

#[test]
fn a_filters_find_is_reached_and_a_search_edited_away_reaches_nothing() {
    let repository = cpython_settings();
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

/// `facts`, in which the first of `repository`'s Filters, which is on, finds `on`, and the second, which is
/// off, finds `off`.
fn found_by_two_filters(
    repository: &RepositorySettings,
    (on, off): (&[u32], &[u32]),
    facts: WorkFacts,
) -> WorkFacts {
    let [first, second] = [&repository.filters[0], &repository.filters[1]];
    assert_eq!(
        [first.on_workbench, second.on_workbench],
        [OnWorkbench::On, OnWorkbench::Off]
    );
    let found = |numbers: &[u32]| FilterResults {
        numbers: numbers.iter().copied().collect(),
        problem: None,
    };
    WorkFacts {
        filters: BTreeMap::from([
            (first.query.clone(), found(on)),
            (second.query.clone(), found(off)),
        ]),
        ..facts
    }
}

#[test]
fn a_filter_switched_on_reaches_its_find_and_one_switched_off_reaches_nothing() {
    let repository = cpython_settings();
    let pull_requests = vec![
        colleagues(11, 2, PrState::Open),
        colleagues(12, 3, PrState::Open),
    ];
    let facts = found_by_two_filters(
        &repository,
        (&[2], &[3]),
        facts(pull_requests, vec![issue(2), issue(3)]),
    );

    // Issue 3 and its PR 12 stay out: Tsugiki fetches no head and reads ahead no Conversation for them.
    assert_eq!(reach(&facts, &repository), BTreeSet::from([2, 11]));
}

#[test]
fn nothing_is_reached_before_anyone_signs_in() {
    let facts = WorkFacts {
        viewer: None,
        ..facts(vec![fixing(10, 1)], vec![issue(1)])
    };

    assert_eq!(reach(&facts, &cpython_settings()), BTreeSet::new());
}

#[test]
fn the_conversations_worth_reading_ahead_are_the_owed_ones_in_reach() {
    let pull_requests = vec![
        mine(10),
        PullRequestFacts {
            number: 20,
            ..stranger()
        },
        mine(30),
    ];
    let owed = BTreeSet::from([10, 20]);

    let to_read = conversations_to_read(&facts(pull_requests, vec![]), &owed, &cpython_settings());

    // A stranger's PR waits until the user opens it, and nothing owes a read of PR 30.
    assert_eq!(to_read, BTreeSet::from([10]));
}

#[test]
fn a_filter_switched_on_owes_the_conversations_it_reaches_unless_read_at_their_heads() {
    let before = cpython_settings();
    let mut now = before.clone();
    now.filters[1].on_workbench = OnWorkbench::On;
    let strangers = |number: u32, head: Option<&str>| PullRequestFacts {
        number,
        head: head.map(sha),
        ..stranger()
    };
    let pull_requests = vec![
        strangers(11, Some("1111111")),
        strangers(12, Some("2222222")),
        strangers(13, None),
        strangers(14, Some("4444444")),
        strangers(15, Some("5555555")),
        strangers(20, Some("2020202")),
    ];
    let facts = found_by_two_filters(
        &before,
        (&[11], &[12, 13, 14, 15]),
        facts(pull_requests, vec![]),
    );
    let read_at = BTreeMap::from([(14, sha("4444444")), (15, sha("0555555"))]);

    let (on, owed_on) = switched(&facts, &read_at, &now, &before);
    let (off, owed_off) = switched(&facts, &read_at, &before, &now);
    let owed = BTreeSet::from([11, 12, 13, 15, 20]);
    let to_read_on = conversations_to_read(&facts, &owed, &now);
    let to_read_off = conversations_to_read(&facts, &owed, &before);

    // PR 14's Conversation was read at its head, PR 15's at an older one, and PR 13's head is unknown.
    assert_eq!(owed_on, [12, 13, 15]);
    let fetched: Vec<u32> = on.heads.iter().map(|head| head.number).collect();
    assert_eq!(fetched, [11, 12, 14, 15]);
    assert_eq!(owed_off, Vec::<u32>::new());
    assert_eq!(off, MirrorWork::of(&facts, &before));
    // PR 20 is owed a read, but out of Reach either way.
    assert_eq!(to_read_on, BTreeSet::from([11, 12, 13, 15]));
    assert_eq!(to_read_off, BTreeSet::from([11]));
}
