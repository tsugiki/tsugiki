//! Mute (#492) as scenarios in the glossary's words: an item hidden until the user unmutes it, where only
//! someone asking the user by name gets through, read the way the window reads it.

use super::*;
use crate::facts::activity::PutAway;
use crate::store::{Archiving, Muting};
use crate::workbench::Hidden;
use crate::workbench::testing::assigned_issue;

impl World {
    /// Mute on item `number`'s line of `view`, as the window drew it.
    fn mute_in(&self, view: &WorkbenchView, number: u32) {
        let (key, SeenMark { numbers, up_to }) = self.drawn_in(view, number);
        let muting = Muting {
            key,
            numbers,
            up_to,
        };
        let now = OffsetDateTime::now_utc();
        self.db.store.mute(&cpython(), muting, now).unwrap();
    }

    fn mute(&self, number: u32) {
        self.mute_in(&WorkbenchView::Whole, number);
    }

    /// Whether `view` lists item `number` marked Muted, with nothing that Needs attention.
    fn lists_muted(&self, view: &WorkbenchView, number: u32) -> bool {
        let listed = self.listed_in(view, number);
        listed.hidden == Some(Hidden::Muted) && listed.attention.is_none()
    }
}

fn assigned(at: OffsetDateTime) -> Activity {
    Activity {
        kind: ActivityKind::Assigned,
        who: me(),
        at,
    }
}

#[test]
fn a_muted_item_stays_hidden_through_comments_ci_and_reviews() {
    let world = World::new("mute-stays");
    let pending = own_ci_pending(1);
    world.has(vec![pending.clone(), own(2)]);
    world.found(&mine(), &[1, 2]);
    world.marked_unseen(2, hours_ago(3));

    world.mute(1);
    world.happened(1, vec![comment(&colleague(), hours_ago(1))]);
    world.ci_fails(pending.clone());
    world.has(vec![PullRequestFacts {
        latest_reviews: vec![LatestReview {
            author: reviewer(),
            state: ReviewState::ChangesRequested,
        }],
        ..pending
    }]);

    assert!(!world.on_workbench(1));
    assert!(!world.lists(&filter(mine()), 1));
    let workbench = world.workbench();
    assert_eq!(numbers(&workbench.items), [2]);
    assert_eq!(workbench.count, 1, "only the item marked unseen");
    world.finish();
}

#[test]
fn mute_moves_last_seen_and_clears_the_flag() {
    let world = World::new("mute-seen");
    let commented = world.has_a_flagged_comment();

    world.mute(1);

    world.assert_seen_up_to(commented);
    world.finish();
}

#[test]
fn a_review_request_that_stood_at_the_mute_lists_the_pr_muted_in_review_requests_and_its_filter() {
    let world = World::new("mute-request-stands");
    let pull_request = world.has_a_requested_pr_involved_finds();
    let requests = WorkbenchView::ReviewRequests;

    world.mute_in(&requests, 1);

    assert!(!world.on_workbench(1));
    assert!(world.lists_muted(&requests, 1));
    assert!(world.lists_muted(&filter(involved()), 1));
    assert_eq!(world.workbench().review_requests, 1);
    assert_eq!(world.workbench().count, 0);

    world.request_ends(pull_request);
    world.finish();
}

#[test]
fn an_assignment_that_stood_at_the_mute_lists_the_issue_muted_in_its_filter_only() {
    let world = World::new("mute-assignment-stands");
    world.has_issues(vec![assigned_issue(5)]);
    world.happened(5, vec![assigned(hours_ago(5))]);
    world.found(&involved(), &[5]);

    world.mute_in(&filter(involved()), 5);

    assert!(!world.on_workbench(5));
    assert!(world.lists_muted(&filter(involved()), 5));
    assert!(!world.lists(&WorkbenchView::ReviewRequests, 5));

    assert_eq!(world.count_of(&involved()), 0);

    world.has_issues(vec![issue(5)]);
    assert!(!world.lists(&filter(involved()), 5));
    assert_eq!(world.folded_in(&filter(involved())), [5]);
    world.finish();
}

#[test]
fn a_filters_view_folds_the_muted_items_nobody_asks_the_user_about_at_its_end() {
    let world = World::new("mute-filter-folds");
    world.has(vec![own(1), own(2), own(3)]);
    world.found(&mine(), &[1, 2, 3]);
    world.found(&involved(), &[1, 2, 3]);

    world.mute(1);
    world.mute(3);

    assert_eq!(numbers(&world.view(&filter(mine())).items), [2]);
    for view in [filter(mine()), filter(involved())] {
        assert_eq!(world.folded_in(&view), [3, 1], "by last update in {view:?}");
        let workbench = world.view(&view);
        let muted = workbench.muted.iter();
        assert!(
            muted
                .clone()
                .all(|listed| listed.hidden == Some(Hidden::Muted))
        );
        assert!(muted.clone().all(|listed| listed.attention.is_none()));
    }
    assert_eq!(world.count_of(&involved()), 1);
    assert!(!world.on_workbench(1));
    assert_eq!(world.folded_in(&WorkbenchView::Whole), Vec::<u32>::new());
    assert_eq!(
        world.folded_in(&WorkbenchView::ReviewRequests),
        Vec::<u32>::new()
    );
    world.finish();
}

#[test]
fn a_review_request_after_the_mute_shows_the_item_on_the_workbench_with_only_that_reason() {
    let world = World::new("mute-request-after");
    let fix = fixing(6, 5);
    world.has(vec![fix.clone()]);
    world.has_issues(vec![issue(5)]);
    world.happened(5, vec![comment(&me(), hours_ago(9))]);
    world.found(&involved(), &[5]);
    world.found(&mine(), &[5]);
    world.mute_in(&filter(involved()), 5);
    assert!(!world.on_workbench(5));

    let asked = hours_ago(1);
    world.happened(5, vec![comment(&colleague(), hours_ago(2))]);
    world.has(vec![PullRequestFacts {
        review_requests: vec![me()],
        updated_at: asked,
        ..fix.clone()
    }]);
    world.happened(6, vec![review_requested(asked)]);

    let listed = world.listed_in(&WorkbenchView::Whole, 5);
    assert_eq!(listed.hidden, Some(Hidden::Muted));
    assert_eq!(world.why(5), [Why::ReviewRequested]);
    assert_eq!(world.since(5), asked);
    assert_eq!(world.workbench().count, 1);
    assert_eq!(world.count_of(&mine()), 1, "Mine is on the Workbench");
    assert_eq!(world.count_of(&involved()), 0, "Involved is off it");

    world.has(vec![PullRequestFacts {
        updated_at: hours_ago(0),
        ..fix
    }]);
    assert!(!world.on_workbench(5));
    assert!(!world.lists(&filter(involved()), 5));
    assert_eq!(world.count_of(&mine()), 0);
    world.finish();
}

#[test]
fn an_assignment_after_the_mute_shows_the_issue_on_the_workbench_until_it_ends() {
    let world = World::new("mute-assignment-after");
    world.has_issues(vec![issue(5)]);
    world.found(&involved(), &[5]);
    world.mute_in(&filter(involved()), 5);

    let at = hours_ago(1);
    world.has_issues(vec![assigned_issue(5)]);
    world.happened(5, vec![assigned(at)]);

    assert_eq!(world.why(5), [Why::Assigned]);
    assert_eq!(world.since(5), at);
    assert_eq!(
        world.listed_in(&WorkbenchView::Whole, 5).hidden,
        Some(Hidden::Muted)
    );

    world.has_issues(vec![issue(5)]);
    assert!(!world.on_workbench(5));
    world.finish();
}

#[test]
fn an_assignment_with_no_event_left_counts_as_from_before_the_mute() {
    let world = World::new("mute-assignment-no-event");
    world.has_issues(vec![issue(5)]);
    world.found(&involved(), &[5]);
    world.mute_in(&filter(involved()), 5);

    world.has_issues(vec![assigned_issue(5)]);

    assert!(!world.on_workbench(5));
    assert!(world.lists_muted(&filter(involved()), 5));
    world.finish();
}

#[test]
fn an_assignment_to_a_closed_issue_asks_nothing() {
    let world = World::new("mute-assignment-closed");
    world.has_issues(vec![issue(5)]);
    world.found(&involved(), &[5]);
    world.mute_in(&filter(involved()), 5);

    world.has_issues(vec![IssueFacts {
        state: Some(crate::facts::issues::IssueState::Closed),
        ..assigned_issue(5)
    }]);
    world.happened(5, vec![assigned(hours_ago(1))]);

    assert!(!world.on_workbench(5));
    assert!(!world.lists(&filter(involved()), 5));
    world.finish();
}

#[test]
fn a_muted_item_that_is_archived_too_leaves_the_archived_view() {
    let world = World::new("mute-archived");
    world.has(vec![own(1), own(2)]);
    let (key, SeenMark { numbers, up_to }) = world.drawn_in(&WorkbenchView::Whole, 1);
    let archiving = Archiving {
        key,
        numbers,
        up_to,
        put_away: PutAway::Now,
    };
    let now = OffsetDateTime::now_utc();
    world.db.store.archive(&cpython(), archiving, now).unwrap();
    world.mute_in(&WorkbenchView::Archived, 1);

    let read = world.view(&WorkbenchView::Archived);

    assert!(read.items.is_empty());
    assert_eq!(read.archived, 0);
    world.finish();
}

#[test]
fn unmute_puts_the_item_back_where_its_state_puts_it() {
    let world = World::new("mute-unmute");
    world.has(vec![own(1)]);
    world.mute(1);
    assert!(!world.on_workbench(1));

    let key = ItemKey::PullRequest { number: 1 };
    world.db.store.unmute(&cpython(), key).unwrap();

    assert!(world.on_workbench(1));
    assert_eq!(world.listed_in(&WorkbenchView::Whole, 1).hidden, None);
    world.finish();
}
