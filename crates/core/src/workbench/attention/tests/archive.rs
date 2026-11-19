//! Archive (#490) as scenarios in the glossary's words: an item put away until something new Needs attention
//! there, read the way the window reads it.

use std::collections::BTreeSet;

use super::*;
use crate::facts::activity::PutAway;
use crate::store::{Archiving, Judged};

impl World {
    /// Archive on item `number`'s line of `view`, as the window drew it.
    fn archive_in(&self, view: &WorkbenchView, number: u32) {
        self.archive_at(view, number, OffsetDateTime::now_utc());
    }

    /// Archive on item `number`'s line of `view`, as the window drew it, at `now`.
    fn archive_at(&self, view: &WorkbenchView, number: u32, now: OffsetDateTime) {
        let archiving = self.archiving_in(view, number, PutAway::Now);
        self.db.store.archive(&cpython(), archiving, now).unwrap();
    }

    /// Mark Seen at `now` on item `number`'s line of the Archived view, as the window drew it, which keeps
    /// the item Archived.
    fn keep_archived_at(&self, number: u32, now: OffsetDateTime) {
        let archiving = self.archiving_in(&WorkbenchView::Archived, number, PutAway::AsBefore);
        self.db.store.archive(&cpython(), archiving, now).unwrap();
    }

    /// What Archive on item `number`'s line of `view` writes, as the window drew it, putting the item away as
    /// `put_away` says.
    fn archiving_in(&self, view: &WorkbenchView, number: u32, put_away: PutAway) -> Archiving {
        let (key, SeenMark { numbers, up_to }) = self.drawn_in(view, number);
        Archiving {
            key,
            numbers,
            up_to,
            put_away,
        }
    }

    fn archive(&self, number: u32) {
        self.archive_in(&WorkbenchView::Whole, number);
    }

    /// Archives the item `key` names as the sync does when it sees the item close (#820).
    fn archive_closed(&self, key: ItemKey) {
        let judged = Judged {
            archived: vec![key],
            closings: BTreeSet::new(),
        };
        let now = OffsetDateTime::now_utc();
        self.db
            .store
            .archive_closed(&cpython(), judged, now)
            .unwrap();
    }
}

#[test]
fn a_closed_item_comes_back_while_it_needs_attention_and_goes_back_to_archived_after() {
    let world = World::new("archive-closed-back");
    world.has(vec![PullRequestFacts {
        state: PrState::Merged,
        ..own(1)
    }]);
    world.archive_closed(ItemKey::PullRequest { number: 1 });
    let archived = world.lists(&WorkbenchView::Archived, 1);

    world.happened(1, vec![comment(&colleague(), hours_ago(1))]);
    let back = world.on_workbench(1);
    world.mark_seen(world.seen(1));

    assert!(archived);
    assert!(back);
    assert!(!world.on_workbench(1));
    assert!(world.lists(&WorkbenchView::Archived, 1));
    world.finish();
}

#[test]
fn closing_archives_an_item_that_still_needs_attention_and_marks_nothing_seen() {
    let world = World::new("archive-closed-unseen");
    let _commented = world.has_a_flagged_comment();
    world.has(vec![PullRequestFacts {
        state: PrState::Merged,
        ..own(1)
    }]);

    world.archive_closed(ItemKey::PullRequest { number: 1 });

    assert!(world.on_workbench(1));
    assert!(!world.lists(&WorkbenchView::Archived, 1));
    let facts = world.db.reader.work_facts(&cpython()).unwrap().activity;
    assert_eq!(facts.last_seen.get(&1), None);
    assert!(facts.marked_unseen.contains_key(&1));
    world.finish();
}

#[test]
fn archive_takes_an_item_off_the_workbench_and_out_of_the_filters_and_their_counts() {
    let world = World::new("archive-leaves");
    world.has(vec![own(1), fixing(6, 5)]);
    world.has_issues(vec![issue(5)]);
    world.found(&mine(), &[1]);
    world.found(&involved(), &[5]);
    assert!(world.on_workbench(1));
    assert_eq!(world.count_of(&involved()), 1);

    world.archive(1);
    world.archive_in(&filter(involved()), 5);

    assert!(!world.on_workbench(1));
    assert!(!world.lists(&filter(mine()), 1));
    assert!(!world.lists(&filter(involved()), 5));
    assert_eq!(world.count_of(&involved()), 0);
    world.finish();
}

#[test]
fn an_item_keys_by_its_kind_and_number() {
    let world = World::new("archive-keys");
    world.has(vec![own(1), fixing_too(own(6), 5)]);
    world.has_issues(vec![issue(5)]);

    let workbench = world.workbench();

    let keys: Vec<ItemKey> = workbench.items.iter().map(|listed| listed.key).collect();
    assert_eq!(
        keys,
        [
            ItemKey::Issue { number: 5 },
            ItemKey::PullRequest { number: 1 }
        ]
    );
    world.finish();
}

#[test]
fn archive_marks_the_item_seen_and_a_comment_from_before_it_doesnt_bring_it_back() {
    let world = World::new("archive-seen");
    let commented = world.has_a_flagged_comment();
    assert!(world.workbench().items[0].attention.is_some());

    world.archive(1);

    assert!(!world.on_workbench(1));
    world.assert_seen_up_to(commented);
    world.finish();
}

#[test]
fn a_comment_that_needs_attention_after_the_archive_brings_the_item_back_for_good() {
    let world = World::new("archive-comment-back");
    world.has(vec![own(1)]);
    world.archive(1);

    world.happened(1, vec![comment(&colleague(), hours_ago(1))]);
    let back = world.listed_in(&WorkbenchView::Whole, 1);
    world.mark_seen(world.seen(1));

    assert!(!back.is_archived());
    assert_eq!(world.why(1), []);
    assert!(
        !world.listed_in(&WorkbenchView::Whole, 1).is_archived(),
        "Mark Seen leaves it unarchived"
    );
    world.finish();
}

#[test]
fn new_activity_on_an_issue_the_user_only_follows_leaves_it_archived() {
    let world = World::new("archive-follows");
    world.has_issues(vec![issue(5)]);
    world.found(&mine(), &[5]);
    world.archive(5);

    world.happened(5, vec![comment(&colleague(), hours_ago(1))]);

    assert!(!world.on_workbench(5));
    world.finish();
}

#[test]
fn a_review_request_that_stood_at_the_archive_keeps_the_pr_in_review_requests_and_its_filter() {
    let world = World::new("archive-request-stands");
    let pull_request = world.has_a_requested_pr_involved_finds();
    let requests = WorkbenchView::ReviewRequests;

    world.archive_in(&requests, 1);

    assert!(!world.on_workbench(1));
    for view in [&requests, &filter(involved())] {
        let listed = world.listed_in(view, 1);
        assert!(listed.is_archived(), "marked Archived in {view:?}");
        assert_eq!(listed.attention, None, "Archive acknowledged the request");
    }
    assert_eq!(world.workbench().review_requests, 1);
    assert_eq!(world.count_of(&involved()), 1);

    world.request_ends(pull_request);
    assert_eq!(world.count_of(&involved()), 0);
    world.finish();
}

#[test]
fn a_review_request_after_the_archive_brings_the_pr_back() {
    let world = World::new("archive-request-back");
    world.has(vec![requested(1)]);
    world.happened(1, vec![review_requested(hours_ago(5))]);
    world.archive_in(&WorkbenchView::ReviewRequests, 1);

    world.happened(1, vec![review_requested(hours_ago(1))]);

    assert_eq!(world.why(1), [Why::ReviewRequested]);
    assert!(!world.listed_in(&WorkbenchView::Whole, 1).is_archived());
    world.finish();
}

#[test]
fn ci_that_fails_after_the_archive_brings_an_own_pr_back() {
    let world = World::new("archive-ci-back");
    let pending = own_ci_pending(1);
    world.has(vec![pending.clone()]);
    world.archive(1);
    assert!(!world.on_workbench(1));

    world.ci_fails(pending);

    assert_eq!(world.why(1), [Why::CiFailed { checks: vec![] }]);
    world.finish();
}

#[test]
fn mark_unseen_on_an_archived_item_unarchives_it() {
    let world = World::new("archive-unseen");
    world.has(vec![own(1)]);
    world.archive(1);

    world.marked_unseen(1, hours_ago(1));
    world.mark_seen(world.seen(1));

    assert!(world.on_workbench(1));
    world.finish();
}

#[test]
fn an_archived_pr_leaves_the_stale_reviews_view_and_its_count() {
    let world = World::new("archive-stale");
    world.reviewed_and_moved(reviewed_by_me(1), 'a', 'b');
    let stale = WorkbenchView::StaleReviews;

    world.archive_in(&stale, 1);

    let read = world.view(&stale);
    assert!(read.items.is_empty());
    assert_eq!(read.stale_reviews, 0);
    world.finish();
}

// The Archived view (#491).

/// The Archived view lists the items in this order, whichever order the list has.
fn archived_in_order(world: &World) -> Vec<u32> {
    let settings = cpython_settings();
    let read = |order| {
        let view = Workbench::read(&world.db.reader, &settings, &WorkbenchView::Archived, order);
        numbers(&view.unwrap().items)
    };
    let by_attention = read(ListOrder::Attention);
    assert_eq!(
        read(ListOrder::LastUpdate),
        by_attention,
        "it keeps its own order"
    );
    by_attention
}

#[test]
fn the_archived_view_lists_the_archived_items_the_last_archived_first_and_counts_items() {
    let world = World::new("archived-view");
    world.has(vec![own(1), own(2), fixing_too(own(6), 5), own(7)]);
    world.has_issues(vec![issue(5)]);
    let whole = WorkbenchView::Whole;
    world.archive_at(&whole, 2, hours_ago(3));
    world.archive_at(&whole, 5, hours_ago(2));
    world.archive_at(&whole, 1, hours_ago(1));

    let order = archived_in_order(&world);

    assert_eq!(order, [1, 5, 2]);
    let read = world.view(&WorkbenchView::Archived);
    assert!(read.items.iter().all(ListedItem::is_archived));
    assert_eq!(read.archived, 3, "an Issue with its PR counts once");
    assert_eq!(world.workbench().archived, 3);
    world.finish();
}

#[test]
fn two_archives_in_one_second_keep_their_order() {
    let world = World::new("archived-view-one-second");
    world.has(vec![own(1), own(2)]);
    let at = hours_ago(1).replace_millisecond(100).unwrap();
    world.archive_at(&WorkbenchView::Whole, 2, at);
    world.archive_at(&WorkbenchView::Whole, 1, at + Duration::milliseconds(200));

    assert_eq!(archived_in_order(&world), [1, 2]);
    world.finish();
}

#[test]
fn an_item_that_comes_back_leaves_the_archived_view() {
    let world = World::new("archived-view-back");
    world.has(vec![own(1), own(2)]);
    world.archive(1);
    world.archive(2);

    world.happened(1, vec![comment(&colleague(), hours_ago(1))]);

    assert_eq!(archived_in_order(&world), [2]);
    assert_eq!(world.workbench().archived, 1);
    world.finish();
}

#[test]
fn mark_seen_on_an_archived_item_keeps_its_place_and_archive_moves_it_first() {
    let world = World::new("archived-view-mark-seen");
    world.has(vec![own(1), own(2)]);
    let archived = WorkbenchView::Archived;
    world.archive_at(&WorkbenchView::Whole, 1, hours_ago(3));
    world.archive_at(&WorkbenchView::Whole, 2, hours_ago(2));

    world.keep_archived_at(1, hours_ago(1));
    let kept = archived_in_order(&world);
    world.archive_at(&archived, 1, hours_ago(1));

    assert_eq!(kept, [2, 1]);
    assert_eq!(archived_in_order(&world), [1, 2]);
    world.finish();
}

#[test]
fn unarchive_puts_the_item_back_where_its_state_puts_it() {
    let world = World::new("archived-view-unarchive");
    world.has(vec![own(1), own(2)]);
    world.archive(1);
    world.archive(2);

    let key = world.listed_in(&WorkbenchView::Archived, 1).key;
    world.db.store.unarchive(&cpython(), key).unwrap();

    assert_eq!(archived_in_order(&world), [2]);
    assert!(world.on_workbench(1));
    assert!(!world.listed_in(&WorkbenchView::Whole, 1).is_archived());
    world.finish();
}

#[test]
fn mark_unseen_on_an_archived_item_brings_it_back_marked_unseen() {
    let world = World::new("archived-view-unseen");
    world.has(vec![own(1)]);
    world.archive(1);

    world.marked_unseen(1, hours_ago(1));

    assert_eq!(archived_in_order(&world), Vec::<u32>::new());
    assert_eq!(world.why(1), [Why::MarkedUnseen]);
    world.finish();
}
