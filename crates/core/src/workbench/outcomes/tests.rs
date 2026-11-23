use std::collections::BTreeMap;

use time::{Duration, OffsetDateTime};

use super::*;
use crate::conventions::CPYTHON_BACKPORT_BOT;
use crate::facts::activity::{ActivityKind, ItemKey, PutAway};
use crate::facts::filters::FilterResults;
use crate::facts::issues::{IssueFacts, IssueState};
use crate::facts::pull_requests::{CiState, OffDefault, PrState, PullRequestFacts};
use crate::filters::FilterQuery;
use crate::login::Login;
use crate::store::testing::archived_on_closing;
use crate::store::{Archiving, Flagging, Muting};
use crate::testing::{cpython, cpython_settings, hours_ago, me};
use crate::workbench::WorkbenchView;
use crate::workbench::testing::{Doing, ViewWorld, fixing, issue, mine, outcomes_done, stranger};

/// Issue 1, which only a flag holds on the Workbench; the user's PR 10; Issue 2, which only a pin holds;
/// Issue 3, pinned and Archived; and PR 20, a stranger's that the user muted, which a Review request made
/// after the mute brings back (#492).
fn build(world: &ViewWorld) {
    let requested = PullRequestFacts {
        number: 20,
        title: "PR 20".to_owned(),
        review_requests: vec![me()],
        ..stranger()
    };
    world.has(
        vec![mine(10), requested],
        vec![issue(1), issue(2), issue(3)],
    );
    let store = &world.db.store;
    let flagging = Flagging::on(1);
    store
        .mark_unseen(&cpython(), flagging, hours_ago(2))
        .unwrap();
    for number in [2, 3] {
        store.pin(&cpython(), number).unwrap();
    }
    let archiving = Archiving {
        key: ItemKey::Issue { number: 3 },
        numbers: vec![3],
        up_to: None,
        put_away: PutAway::Now,
        findings: vec![],
    };
    store.archive(&cpython(), archiving, hours_ago(3)).unwrap();
    let muting = Muting {
        key: ItemKey::PullRequest { number: 20 },
        numbers: vec![20],
        up_to: Some(hours_ago(10)),
        findings: vec![],
    };
    store.mute(&cpython(), muting, hours_ago(10)).unwrap();
    world.happened(20, &[(ActivityKind::ReviewRequested, hours_ago(5))]);
}

/// The outcome `view` gives each action on each item it lists, each held against what the view lists once
/// the store holds what the action writes.
fn judged(name: &str, view: &WorkbenchView) -> BTreeMap<(u32, Doing), Outcome> {
    let read = |world: &ViewWorld| world.placed_in(view);
    let judged = outcomes_done(name, &build, &read).into_iter();
    judged
        .map(|(number, doing, outcome)| ((number, doing), outcome))
        .collect()
}

#[test]
fn each_outcome_on_the_workbench_is_what_the_workbench_lists_once_the_store_holds_the_action() {
    let judged = judged("outcomes-whole", &WorkbenchView::Whole);

    use Doing::*;
    use Outcome::*;
    let expected = BTreeMap::from([
        // Only the flag holds Issue 1, which Mark Seen clears. The pin holds it as well as the flag does.
        ((1, MarkSeen), Leaves),
        ((1, Archive), Leaves),
        ((1, MarkUnseen), Stays),
        ((1, Mute), Leaves),
        ((1, MuteAuthor), Leaves),
        ((1, Pin), Stays),
        // Only the pin holds Issue 2.
        ((2, MarkSeen), Stays),
        ((2, Archive), Leaves),
        ((2, MarkUnseen), Stays),
        ((2, Mute), Leaves),
        ((2, MuteAuthor), Leaves),
        ((2, Unpin), Leaves),
        // The user's own PR, whose author Mute author leaves alone.
        ((10, MarkSeen), Stays),
        ((10, Archive), Leaves),
        ((10, MarkUnseen), Stays),
        ((10, Mute), Leaves),
        // Mark Seen acknowledges the request that brought the Muted PR back, which then leaves. Archive
        // and Mute do nothing to it.
        ((20, MarkSeen), Leaves),
        ((20, MarkUnseen), Stays),
        ((20, MuteAuthor), Leaves),
    ]);
    assert_eq!(judged, expected);
}

#[test]
fn mark_seen_puts_a_closed_issue_back_in_its_archive_once_nothing_in_its_tree_waits() {
    // The closing archived Issue 4, and the flag on the user's PR 40 under it brings it back, as a flag on
    // the Issue itself would unarchive it.
    let build = |world: &ViewWorld| {
        let closed = IssueFacts {
            state: Some(IssueState::Closed),
            ..issue(4)
        };
        world.has(vec![fixing(40, 4)], vec![closed]);
        let store = &world.db.store;
        archived_on_closing(store, &[ItemKey::Issue { number: 4 }], hours_ago(4));
        store
            .mark_unseen(&cpython(), Flagging::on(40), hours_ago(2))
            .unwrap();
    };
    let read = |world: &ViewWorld| world.placed_in(&WorkbenchView::Whole);

    let judged = outcomes_done("outcomes-closed", &build, &read);

    assert!(
        judged.contains(&(4, Doing::MarkSeen, Outcome::Leaves)),
        "{judged:?}"
    );
}

#[test]
fn the_review_requests_keep_a_muted_pr_whose_request_stands_once_marked_seen() {
    let judged = judged("outcomes-requests", &WorkbenchView::ReviewRequests);

    assert_eq!(judged.get(&(20, Doing::MarkSeen)), Some(&Outcome::Stays));
    assert_eq!(judged.get(&(20, Doing::MuteAuthor)), Some(&Outcome::Stays));
}

#[test]
fn the_pinned_issues_keep_a_pin_through_archive_and_let_it_go_at_the_unpin() {
    let judged = judged("outcomes-pinned", &WorkbenchView::Pinned);

    use Doing::*;
    use Outcome::*;
    // Archive keeps Issue 2 there drawn Archived, and Mark Seen and Archive keep the Archived Issue 3
    // Archived.
    let expected = BTreeMap::from([
        ((2, MarkSeen), Stays),
        ((2, Archive), Stays),
        ((2, MarkUnseen), Stays),
        ((2, Mute), Leaves),
        ((2, MuteAuthor), Leaves),
        ((2, Unpin), Leaves),
        ((3, MarkSeen), Stays),
        ((3, Archive), Stays),
        ((3, Unarchive), Stays),
        ((3, MarkUnseen), Stays),
        ((3, Mute), Leaves),
        ((3, MuteAuthor), Leaves),
        ((3, Unpin), Leaves),
    ]);
    assert_eq!(judged, expected);
}

/// A minute after anything a world has made happen so far, as when a check finds a Review stale after
/// GitHub's last word on the PR.
fn a_minute_on() -> OffsetDateTime {
    hours_ago(0) + Duration::minutes(1)
}

/// The Starter Filter "Mine".
fn mine_query() -> FilterQuery {
    cpython_settings().filters[0].query.clone()
}

/// `build`'s world, with PR 30, a stranger's, whose Stale review the check found after GitHub's last word on
/// it; Issue 5, pinned and then closed, which the pin keeps from its archive; Issue 6, Archived; and the
/// closed Issue 7, archived as it closed, which the Stale review of PR 30's twin PR 31 under it brings back.
/// "Mine" found Issues 1, 5, 6 and 7 and PRs 20 and 30.
fn wider(world: &ViewWorld) {
    build(world);
    let closed = |number| IssueFacts {
        state: Some(IssueState::Closed),
        ..issue(number)
    };
    world.has(vec![], vec![issue(5), issue(6), closed(7)]);
    let store = &world.db.store;
    store.pin(&cpython(), 5).unwrap();
    world.has(vec![], vec![closed(5)]);
    let archiving = Archiving {
        key: ItemKey::Issue { number: 6 },
        numbers: vec![6],
        up_to: None,
        put_away: PutAway::Now,
        findings: vec![],
    };
    store.archive(&cpython(), archiving, hours_ago(3)).unwrap();
    archived_on_closing(store, &[ItemKey::Issue { number: 7 }], hours_ago(4));
    let stranger_numbered = |number, title: &str| PullRequestFacts {
        number,
        title: title.to_owned(),
        ..stranger()
    };
    world.reviewed_and_pushed(stranger_numbered(30, "PR 30"));
    world.reviewed_and_pushed(stranger_numbered(31, "gh-7: Fix it"));
    world.db.checks_kept(a_minute_on());
    let results = FilterResults {
        numbers: BTreeSet::from([1, 5, 6, 7, 20, 30]),
        problem: None,
    };
    store
        .save_filter_results(&cpython(), &mine_query(), results)
        .unwrap();
}

/// The outcome `view` gives each action on each item it lists in the `wider` world.
fn judged_wider(name: &str, view: &WorkbenchView) -> BTreeMap<(u32, Doing), Outcome> {
    let read = |world: &ViewWorld| world.placed_in(view);
    let judged = outcomes_done(name, &wider, &read).into_iter();
    judged
        .map(|(number, doing, outcome)| ((number, doing), outcome))
        .collect()
}

#[test]
fn archive_and_mark_seen_acknowledge_a_stale_review_the_check_found_after_the_prs_last_update() {
    let judged = judged_wider("outcomes-stale-found", &WorkbenchView::Whole);

    // The window drew the finding in each mark, so the store acknowledges it: PR 30 goes to its archive,
    // and Issue 7 back to the archive it closed into.
    assert_eq!(judged.get(&(30, Doing::Archive)), Some(&Outcome::Leaves));
    assert_eq!(judged.get(&(7, Doing::MarkSeen)), Some(&Outcome::Leaves));
}

#[test]
fn each_view_gives_each_action_the_outcome_the_view_shows_once_the_store_holds_it() {
    let views = [
        ("whole", WorkbenchView::Whole),
        (
            "filter",
            WorkbenchView::Filter {
                query: mine_query(),
            },
        ),
        ("requests", WorkbenchView::ReviewRequests),
        ("stale", WorkbenchView::StaleReviews),
        ("archived", WorkbenchView::Archived),
        ("pinned", WorkbenchView::Pinned),
    ];
    let judged: BTreeMap<&str, BTreeMap<(u32, Doing), Outcome>> = (views.iter())
        .map(|(name, view)| (*name, judged_wider(&format!("outcomes-wider-{name}"), view)))
        .collect();

    let outcome =
        |name: &str, number: u32, doing: Doing| judged[name].get(&(number, doing)).copied();
    // The unpin of Issue 5, closed while pinned, archives it (#820).
    assert_eq!(outcome("whole", 5, Doing::Unpin), Some(Outcome::Leaves));
    // A pin unarchives the Archived Issue 6, which a Filter's view lists either way.
    assert_eq!(outcome("archived", 6, Doing::Pin), Some(Outcome::Leaves));
    assert_eq!(outcome("filter", 6, Doing::Pin), Some(Outcome::Stays));
    // A Filter's view folds an item the user mutes (#824).
    assert_eq!(outcome("filter", 1, Doing::Mute), Some(Outcome::Folds));
    assert_eq!(outcome("stale", 30, Doing::Archive), Some(Outcome::Leaves));
}

#[test]
fn mark_seen_keeps_an_issue_whose_pr_has_a_bot_backport_under_another_issue_that_failed_ci_later() {
    // The user's merged PR 1 under Issue 5, which only the failed CI of PR 1's Bot backport PR 4 holds on
    // the Workbench. PR 4 names Issue 7 alone, so it sits in Issue 7's tree and not in Issue 5's, and its
    // CI finished after anything in Issue 5's tree, which Issue 5's mark covers.
    let build = |world: &ViewWorld| {
        world.db.finishes_its_first_sync();
        let origin = PullRequestFacts {
            state: PrState::Merged,
            updated_at: hours_ago(10),
            ..fixing(1, 5)
        };
        let bot = Login::new(CPYTHON_BACKPORT_BOT);
        let backport = PullRequestFacts {
            number: 4,
            title: "[3.14] gh-7: Fix it (GH-1)".to_owned(),
            author: Some(bot.clone()),
            target_branch: "3.14".to_owned(),
            off_default: Some(OffDefault {
                committers: vec![Some(bot)],
                ..OffDefault::described("")
            }),
            updated_at: hours_ago(10),
            ..stranger()
        };
        world.has(vec![origin, backport.clone()], vec![issue(5), issue(7)]);
        let failed = PullRequestFacts {
            ci: Some(CiState::Failure),
            ..backport
        };
        world.has(vec![failed], vec![]);
    };
    let read = |world: &ViewWorld| world.placed_in(&WorkbenchView::Whole);

    let judged = outcomes_done("outcomes-backport-elsewhere", &build, &read);

    assert!(
        judged.contains(&(5, Doing::MarkSeen, Outcome::Stays)),
        "{judged:?}"
    );
}
