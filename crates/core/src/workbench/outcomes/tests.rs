use std::collections::BTreeMap;

use super::*;
use crate::facts::activity::{ActivityKind, PutAway};
use crate::facts::issues::{IssueFacts, IssueState};
use crate::facts::pull_requests::PullRequestFacts;
use crate::store::testing::archived_on_closing;
use crate::store::{Archiving, Flagging, Muting};
use crate::testing::{cpython, hours_ago, me};
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
        ((1, Mute), Leaves),
        ((1, MuteAuthor), Leaves),
        ((1, Pin), Stays),
        // Only the pin holds Issue 2.
        ((2, MarkSeen), Stays),
        ((2, Archive), Leaves),
        ((2, Mute), Leaves),
        ((2, MuteAuthor), Leaves),
        ((2, Unpin), Leaves),
        // The user's own PR, whose author Mute author leaves alone.
        ((10, MarkSeen), Stays),
        ((10, Archive), Leaves),
        ((10, Mute), Leaves),
        // Mark Seen acknowledges the request that brought the Muted PR back, which then leaves. Archive
        // and Mute do nothing to it.
        ((20, MarkSeen), Leaves),
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
    // Archive keeps Issue 2 there drawn Archived, and Mark Seen keeps the Archived Issue 3 Archived.
    let expected = BTreeMap::from([
        ((2, MarkSeen), Stays),
        ((2, Archive), Stays),
        ((2, Mute), Leaves),
        ((2, MuteAuthor), Leaves),
        ((2, Unpin), Leaves),
        ((3, MarkSeen), Stays),
        ((3, Unarchive), Stays),
        ((3, Mute), Leaves),
        ((3, MuteAuthor), Leaves),
        ((3, Unpin), Leaves),
    ]);
    assert_eq!(judged, expected);
}
