//! Reach, from each Issue and PR's place on the Workbench and the links between them.

use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::facts::activity::{ActivityFacts, Archive, ItemKey};
use crate::facts::issues::IssueFacts;
use crate::facts::patch_ids::ReviewFacts;
use crate::facts::pull_requests::{PrState, PullRequestFacts};
use crate::testing::{cpython_settings, hours_ago, sha};
use crate::workbench::WorkbenchView;
use crate::workbench::testing::{
    assigned_issue, backport, bots_backport, fixing, issue, me, mine, rows_of, someone, stranger,
    view_of,
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
fn a_review_request_the_user_marked_seen_leaves_the_workbench_and_stays_in_reach() {
    let asked = PullRequestFacts {
        number: 5,
        review_requests: vec![me()],
        ..stranger()
    };
    let mut facts = facts(vec![asked], vec![]);
    let later = time::OffsetDateTime::now_utc() + time::Duration::hours(1);
    let _acknowledged = facts.activity.acknowledged.insert(5, later);
    let settings = cpython_settings();

    let causes = Causes::of(&facts, settings.conventions);

    assert_eq!(in_own_right(&facts, &settings, &causes), BTreeSet::new());
    assert_eq!(reach(&facts, &settings), BTreeSet::from([5]));
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

    let from = |settings| ReachFrom {
        facts: &facts,
        settings,
    };
    let (on, owed_on) = moved(from(&before), from(&now), &read_at);
    let (off, owed_off) = moved(from(&now), from(&before), &read_at);
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

/// `facts` with the items `keys` name Muted, as of an hour ago.
fn muting(mut facts: WorkFacts, keys: &[ItemKey]) -> WorkFacts {
    let muted = keys.iter().map(|key| (*key, hours_ago(1)));
    facts.activity.muted.extend(muted);
    facts
}

fn issue_key(number: u32) -> ItemKey {
    ItemKey::Issue { number }
}

#[test]
fn a_muted_tree_leaves_reach_and_a_pr_another_item_links_to_comes_back() {
    let pull_requests = vec![
        fixing(10, 1),
        colleagues(11, 1, PrState::Open),
        PullRequestFacts {
            number: 12,
            title: "gh-1, gh-2: Fix both".to_owned(),
            ..stranger()
        },
        colleagues(30, 1, PrState::Merged),
        bots_backport(31, "[3.14] gh-3: Fix it (GH-30)"),
        fixing(20, 2),
        fixing(40, 3),
    ];
    let issues = [1, 2, 3].map(issue).to_vec();
    let facts = muting(facts(pull_requests, issues), &[issue_key(1)]);

    let sets = Reach::of(&facts, &cpython_settings());

    // The user's PR 10 is in Issue 1's tree, so it leaves with it. PR 12 comes back as it fixes Issue 2, and
    // PR 30 as its Backport 31 fixes Issue 3.
    assert_eq!(
        sets.without_muted,
        BTreeSet::from([2, 3, 12, 20, 30, 31, 40])
    );
    assert_eq!(sets.muted_asking, BTreeSet::new());
}

#[test]
fn a_start_in_a_muted_tree_keeps_the_trees_it_shares_with_unmuted_items() {
    let pull_requests = vec![
        PullRequestFacts {
            title: "gh-1, gh-2: Fix both".to_owned(),
            ..mine(12)
        },
        colleagues(11, 1, PrState::Open),
        colleagues(21, 2, PrState::Open),
        fixing(30, 3),
        bots_backport(31, "[3.14] gh-1: Fix it (GH-30)"),
    ];
    let issues = [1, 2, 3].map(issue).to_vec();
    let facts = muting(facts(pull_requests, issues), &[issue_key(1)]);

    let reached = reach(&facts, &cpython_settings());

    // The user's PR 12 puts Issue 2 on the Workbench as well as Muted Issue 1, and their PR 30 puts Issue 3
    // there with its Backport 31, which fixes Issue 1 too.
    assert_eq!(reached, BTreeSet::from([2, 3, 12, 21, 30, 31]));
}

#[test]
fn a_muted_prs_key_names_no_tree_once_the_pr_sits_under_an_issue_or_an_origin_pr() {
    let pull_requests = vec![
        fixing(10, 1),
        colleagues(50, 1, PrState::Open),
        PullRequestFacts {
            title: "Tidy up".to_owned(),
            ..mine(40)
        },
        bots_backport(41, "[3.14] Tidy up (GH-40)"),
    ];
    let keys = [50, 41].map(|number| ItemKey::PullRequest { number });
    let facts = muting(facts(pull_requests, vec![issue(1)]), &keys);

    let reached = reach(&facts, &cpython_settings());

    // The user muted PRs 50 and 41 while they stood alone, before a title edit linked PR 50 to Issue 1 and
    // a sync fetched PR 41's Origin PR. The list draws them under items the user hasn't muted.
    assert_eq!(reached, BTreeSet::from([1, 10, 40, 41, 50]));
}

#[test]
fn a_muted_item_where_an_ask_stands_stays_in_reach_with_its_tree_until_the_ask_ends() {
    let review_requested = |facts: PullRequestFacts| PullRequestFacts {
        review_requests: vec![me()],
        ..facts
    };
    let pull_requests = vec![
        colleagues(11, 1, PrState::Open),
        colleagues(12, 1, PrState::Closed),
        colleagues(21, 2, PrState::Open),
        review_requested(PullRequestFacts {
            number: 5,
            title: "Tidy up".to_owned(),
            ..stranger()
        }),
        review_requested(colleagues(61, 6, PrState::Open)),
        colleagues(62, 6, PrState::Open),
    ];
    let issues = vec![assigned_issue(1), issue(2), issue(6)];
    let mut facts = facts(pull_requests, issues);
    // Mark Seen acknowledges an ask, which stands all the same.
    let later = time::OffsetDateTime::now_utc() + time::Duration::hours(1);
    let _acknowledged = facts.activity.acknowledged.insert(1, later);
    let standalone = ItemKey::PullRequest { number: 5 };
    let keys = [issue_key(1), issue_key(2), issue_key(6), standalone];
    let mut facts = muting(facts, &keys);
    let settings = cpython_settings();

    let asking = Reach::of(&facts, &settings);
    facts.issues[0].assignees.clear();
    let unassigned = Reach::of(&facts, &settings);

    // Muted Issue 2 asks nothing of the user.
    assert_eq!(asking.without_muted, BTreeSet::new());
    assert_eq!(
        asking.muted_asking,
        BTreeSet::from([1, 5, 6, 11, 12, 61, 62])
    );
    assert_eq!(unassigned.muted_asking, BTreeSet::from([5, 6, 61, 62]));
}

#[test]
fn reach_joins_the_items_without_the_muted_ones_and_the_muted_ones_that_ask() {
    let pull_requests = vec![
        mine(20),
        colleagues(11, 1, PrState::Open),
        colleagues(31, 3, PrState::Open),
    ];
    let issues = vec![assigned_issue(1), issue(3)];
    let facts = muting(facts(pull_requests, issues), &[issue_key(1), issue_key(3)]);
    let settings = cpython_settings();

    let sets = Reach::of(&facts, &settings);

    assert_eq!(sets.without_muted, BTreeSet::from([20]));
    assert_eq!(sets.muted_asking, BTreeSet::from([1, 11]));
    assert_eq!(reach(&facts, &settings), BTreeSet::from([1, 11, 20]));
}

#[test]
fn the_walk_stops_at_a_second_issue_that_a_pr_of_a_reached_issue_fixes() {
    let repository = cpython_settings();
    let pull_requests = vec![
        PullRequestFacts {
            number: 11,
            title: "gh-1, gh-2: Fix both".to_owned(),
            ..stranger()
        },
        colleagues(12, 2, PrState::Open),
    ];
    let facts = found_by_two_filters(
        &repository,
        (&[1], &[]),
        facts(pull_requests, vec![issue(1), issue(2)]),
    );

    // A walk that went on would chain Issue 1's tree to Issue 2's through PR 11.
    assert_eq!(reach(&facts, &repository), BTreeSet::from([1, 11]));
}

#[test]
fn an_archived_item_stays_in_reach() {
    let mut facts = facts(vec![fixing(10, 1)], vec![issue(1)]);
    let archive = Archive {
        seen_up_to: hours_ago(1),
        archived_at: hours_ago(1),
    };
    let _archived = facts.activity.archived.insert(issue_key(1), archive);

    // The Stale review check runs only on PRs in Reach, and a Stale review brings the item back.
    assert_eq!(reach(&facts, &cpython_settings()), BTreeSet::from([1, 10]));
}

/// The user's PR `facts` with head `head`.
fn at_head(head: &str, facts: PullRequestFacts) -> PullRequestFacts {
    PullRequestFacts {
        head: Some(sha(head)),
        ..facts
    }
}

#[test]
fn the_mirror_fetches_no_head_and_reads_ahead_no_conversation_of_a_muted_items_prs() {
    let pull_requests = vec![
        at_head("1010101", fixing(10, 1)),
        at_head("2020202", mine(20)),
    ];
    let facts = muting(facts(pull_requests, vec![issue(1)]), &[issue_key(1)]);
    let settings = cpython_settings();
    let owed = BTreeSet::from([10, 20]);

    let work = MirrorWork::of(&facts, &settings);
    let to_read = conversations_to_read(&facts, &owed, &settings);

    let fetched: Vec<u32> = work.heads.iter().map(|head| head.number).collect();
    assert_eq!(fetched, [20]);
    assert_eq!(to_read, BTreeSet::from([20]));
}

#[test]
fn unmute_owes_the_conversations_it_puts_back_in_reach_unless_read_at_their_heads() {
    let pull_requests = vec![
        at_head("1111111", colleagues(11, 1, PrState::Open)),
        at_head("2222222", colleagues(12, 1, PrState::Open)),
        at_head("2020202", mine(20)),
        at_head("3030303", fixing(30, 1)),
    ];
    let unmuted = facts(pull_requests.clone(), vec![issue(1)]);
    let muted = muting(facts(pull_requests, vec![issue(1)]), &[issue_key(1)]);
    let read_at = BTreeMap::from([(12, sha("2222222")), (20, sha("0000000"))]);
    let settings = cpython_settings();
    let from = |facts| ReachFrom {
        facts,
        settings: &settings,
    };

    let (work, owed) = moved(from(&muted), from(&unmuted), &read_at);

    let fetched: Vec<u32> = work.heads.iter().map(|head| head.number).collect();
    assert_eq!(fetched, [11, 12, 20, 30]);
    // PR 12's Conversation was read at its head, and PR 20 was in Reach all along.
    assert_eq!(owed, [11, 30]);
    assert_eq!(work, MirrorWork::of(&unmuted, &settings));
}
