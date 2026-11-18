//! Archive (#490) as scenarios in the glossary's words: an item put away until something new Needs attention
//! there, read the way the window reads it.

use super::*;
use crate::facts::activity::ItemKey;
use crate::facts::filters::FilterResults;
use crate::filters::FilterQuery;
use crate::store::Archiving;

/// The starter Filters: "Mine", on the Workbench, and "Involved", off it.
fn mine() -> FilterQuery {
    cpython_settings().filters[0].query.clone()
}

fn involved() -> FilterQuery {
    cpython_settings().filters[1].query.clone()
}

impl World {
    /// The Filter that searches `query` found `numbers`.
    fn found(&self, query: &FilterQuery, numbers: &[u32]) {
        let results = FilterResults {
            numbers: numbers.iter().copied().collect(),
            problem: None,
        };
        (self.db.store)
            .save_filter_results(&cpython(), query, results)
            .unwrap();
    }

    /// Item `number` as `view` lists it.
    fn listed_in(&self, view: &WorkbenchView, number: u32) -> ListedItem {
        listed_at(&self.view(view), number)
    }

    /// Archive on item `number`'s line of `view`, as the window drew it.
    fn archive_in(&self, view: &WorkbenchView, number: u32) {
        let listed = self.listed_in(view, number);
        let seen = match &listed.item {
            WorkbenchItem::Issue(issue) => issue.seen.clone(),
            WorkbenchItem::StandalonePr(tree) => tree.row.seen.clone(),
            WorkbenchItem::StandaloneBackport(backport) => backport.row.seen.clone(),
        };
        let SeenMark { numbers, up_to } = seen.mark;
        let key = listed.key;
        let archiving = Archiving {
            key,
            numbers,
            up_to,
        };
        let now = OffsetDateTime::now_utc();
        self.db.store.archive(&cpython(), archiving, now).unwrap();
    }

    fn archive(&self, number: u32) {
        self.archive_in(&WorkbenchView::Whole, number);
    }

    /// The Sidebar's count beside the Filter that searches `query`.
    fn count_of(&self, query: &FilterQuery) -> u32 {
        let lines = self.workbench().filters;
        let line = lines.iter().find(|line| line.query == *query).unwrap();
        line.count
    }
}

fn filter(query: FilterQuery) -> WorkbenchView {
    WorkbenchView::Filter { query }
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
    world.has(vec![own(1)]);
    world.happened(1, vec![comment(&colleague(), hours_ago(2))]);
    world.marked_unseen(1, hours_ago(1));
    assert!(world.workbench().items[0].attention.is_some());

    world.archive(1);

    assert!(!world.on_workbench(1));
    let facts = world.db.reader.work_facts(&cpython()).unwrap().activity;
    assert_eq!(facts.marked_unseen, BTreeMap::new());
    assert_eq!(facts.last_seen.get(&1), Some(&hours_ago(2)));
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

    assert!(!back.archived);
    assert_eq!(world.why(1), []);
    assert!(
        !world.listed_in(&WorkbenchView::Whole, 1).archived,
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
    let pull_request = requested(1);
    world.has(vec![pull_request.clone()]);
    world.happened(1, vec![review_requested(hours_ago(5))]);
    world.found(&involved(), &[1]);
    let requests = WorkbenchView::ReviewRequests;

    world.archive_in(&requests, 1);

    assert!(!world.on_workbench(1));
    for view in [&requests, &filter(involved())] {
        let listed = world.listed_in(view, 1);
        assert!(listed.archived, "marked Archived in {view:?}");
        assert_eq!(listed.attention, None, "Archive acknowledged the request");
    }
    assert_eq!(world.workbench().review_requests, 1);
    assert_eq!(world.count_of(&involved()), 1);

    world.has(vec![PullRequestFacts {
        review_requests: vec![],
        updated_at: hours_ago(4),
        ..pull_request
    }]);
    assert!(!world.lists(&requests, 1));
    assert!(!world.lists(&filter(involved()), 1));
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
    assert!(!world.listed_in(&WorkbenchView::Whole, 1).archived);
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
