//! Mute author (#494) as scenarios in the glossary's words: a Muted author's Issues and Standalone PRs count
//! as Muted, those they open later too, read the way the window reads it.

use super::*;
use crate::store::{AuthorMuting, Muting};
use crate::workbench::testing::assigned_issue;
use crate::workbench::{Authored, Hidden, MuteAuthor, authored_by};

impl World {
    /// Mute author on `author` at `at`, as the session makes it.
    fn mute_author_at(&self, author: &Login, at: OffsetDateTime) {
        let facts = self.db.reader.work_facts(&cpython()).unwrap();
        let Authored { marks, .. } = authored_by(facts, &cpython_settings(), author);
        let author = author.clone();
        let muting = AuthorMuting { author, marks };
        self.db.store.mute_author(&cpython(), muting, at).unwrap();
    }

    fn mute_author(&self, author: &Login) {
        self.mute_author_at(author, OffsetDateTime::now_utc());
    }

    fn unmute_author(&self, author: &Login) {
        self.db.store.unmute_author(&cpython(), author).unwrap();
    }

    /// Mute on item `number`'s line of the Workbench, as the window drew it.
    fn mute_on_its_own(&self, number: u32) {
        let (key, SeenMark { numbers, up_to }) = self.drawn_in(&WorkbenchView::Whole, number);
        let muting = Muting {
            key,
            numbers,
            up_to,
        };
        let now = OffsetDateTime::now_utc();
        self.db.store.mute(&cpython(), muting, now).unwrap();
    }

    /// What `view` says hides item `number`, whether it lists the item in place or in its fold.
    fn hidden_in(&self, view: &WorkbenchView, number: u32) -> Option<Hidden> {
        self.folded_or_listed_in(view, number).hidden
    }

    /// Item `number` as `view` lists it, in place or in its fold.
    fn folded_or_listed_in(&self, view: &WorkbenchView, number: u32) -> ListedItem {
        let read = self.view(view);
        let mut listed = read.items.into_iter().chain(read.muted);
        listed.find(|listed| number_of(listed) == number).unwrap()
    }
}

/// Issue `number`, opened by the colleague.
fn colleagues_issue(number: u32) -> IssueFacts {
    IssueFacts {
        author: Some(colleague()),
        ..issue(number)
    }
}

/// The colleague's PR `number`, which the user reviewed, with a comment from them since.
fn commented_on(world: &World, number: u32) {
    world.has(vec![reviewed_by_me(number)]);
    world.happened(number, vec![comment(&colleague(), hours_ago(2))]);
}

/// What Mute author offers on the line of `item` that heads it.
fn mute_author_of(item: &WorkbenchItem) -> MuteAuthor {
    match item {
        WorkbenchItem::Issue(issue) => issue.mute_author.clone(),
        WorkbenchItem::StandalonePr(tree) => tree.row.mute_author.clone(),
        WorkbenchItem::StandaloneBackport(backport) => backport.row.mute_author.clone(),
    }
}

#[test]
fn a_muted_authors_items_leave_the_workbench_and_the_counts_and_fold_in_each_filter() {
    let world = World::new("mute-author-hides");
    commented_on(&world, 1);
    world.has(vec![own(2)]);
    world.has_issues(vec![colleagues_issue(5)]);
    world.happened(5, vec![comment(&me(), hours_ago(9))]);
    world.happened(5, vec![comment(&colleague(), hours_ago(1))]);
    world.found(&involved(), &[1, 5]);
    assert!(world.on_workbench(1));
    assert!(world.on_workbench(5));

    world.mute_author(&colleague());

    let workbench = world.workbench();
    assert_eq!(numbers(&workbench.items), [2]);
    assert_eq!(workbench.count, 0);
    assert_eq!(world.count_of(&involved()), 0);
    assert_eq!(world.folded_in(&filter(involved())), [5, 1]);
    for number in [1, 5] {
        let hidden = world.hidden_in(&filter(involved()), number);
        assert_eq!(hidden, Some(Hidden::MutedAuthor));
    }
    world.finish();
}

#[test]
fn mute_author_marks_their_open_items_seen_and_leaves_the_closed_ones() {
    let world = World::new("mute-author-seen");
    // One moment for the comment and the check, as a second can pass between two reads of the clock.
    let commented = hours_ago(2);
    world.has(vec![reviewed_by_me(1)]);
    world.happened(1, vec![comment(&colleague(), commented)]);
    world.marked_unseen(1, hours_ago(1));
    let closed = PullRequestFacts {
        state: PrState::Closed,
        ..reviewed_by_me(2)
    };
    world.has(vec![closed]);
    world.happened(2, vec![comment(&colleague(), hours_ago(3))]);
    world.has(vec![own(3)]);
    world.happened(3, vec![comment(&colleague(), hours_ago(4))]);

    world.mute_author(&colleague());

    world.assert_seen_up_to(commented);
    let last_seen = world
        .db
        .reader
        .work_facts(&cpython())
        .unwrap()
        .activity
        .last_seen;
    assert_eq!(last_seen.get(&2), None, "a closed item");
    assert_eq!(last_seen.get(&3), None, "the user's own item");
    world.finish();
}

#[test]
fn an_item_they_open_after_the_mute_is_muted_too() {
    let world = World::new("mute-author-later");
    world.has(vec![own(2)]);
    world.mute_author(&colleague());

    commented_on(&world, 1);

    assert!(!world.on_workbench(1));
    world.found(&involved(), &[1]);
    assert_eq!(world.folded_in(&filter(involved())), [1]);
    world.finish();
}

#[test]
fn a_review_request_that_stood_at_the_mute_lists_their_pr_muted_in_review_requests() {
    let world = World::new("mute-author-request-stands");
    let pull_request = world.has_a_requested_pr_involved_finds();

    world.mute_author(&colleague());

    let requests = WorkbenchView::ReviewRequests;
    assert!(!world.on_workbench(1));
    assert_eq!(world.hidden_in(&requests, 1), Some(Hidden::MutedAuthor));
    assert_eq!(world.why_in(&requests, 1), Vec::<Why>::new());
    assert_eq!(
        world.hidden_in(&filter(involved()), 1),
        Some(Hidden::MutedAuthor)
    );
    assert_eq!(world.folded_in(&filter(involved())), Vec::<u32>::new());
    world.request_ends(pull_request);
    world.finish();
}

#[test]
fn an_ask_after_the_mute_shows_their_item_on_the_workbench_for_that_alone() {
    let world = World::new("mute-author-ask-after");
    world.has(vec![pull_request(1)]);
    world.has_issues(vec![colleagues_issue(5)]);
    world.mute_author_at(&colleague(), hours_ago(3));

    let asked = hours_ago(1);
    world.has(vec![PullRequestFacts {
        updated_at: asked,
        ..requested(1)
    }]);
    world.happened(1, vec![review_requested(asked)]);
    world.has_issues(vec![IssueFacts {
        author: Some(colleague()),
        ..assigned_issue(5)
    }]);
    world.happened(5, vec![assigned(hours_ago(2))]);

    assert_eq!(world.why(1), [Why::ReviewRequested]);
    assert_eq!(world.since(1), asked);
    assert_eq!(world.why(5), [Why::Assigned]);
    let whole = WorkbenchView::Whole;
    assert_eq!(world.hidden_in(&whole, 1), Some(Hidden::MutedAuthor));
    assert_eq!(world.workbench().count, 2);
    world.finish();
}

#[test]
fn their_pr_inside_another_authors_issue_lists_and_needs_attention_as_before() {
    let world = World::new("mute-author-in-a-tree");
    world.has_issues(vec![issue(5)]);
    world.has(vec![fixing_too(reviewed_by_me(6), 5)]);
    world.happened(6, vec![comment(&colleague(), hours_ago(2))]);
    let before = world.listed_in(&WorkbenchView::Whole, 5);

    world.mute_author(&colleague());

    let after = world.listed_in(&WorkbenchView::Whole, 5);
    assert_eq!(after.hidden, None);
    assert_eq!(after.attention, before.attention);
    assert!(after.attention.is_some());
    world.finish();
}

#[test]
fn unmute_author_brings_their_items_back_and_one_muted_on_its_own_stays_muted() {
    let world = World::new("mute-author-unmute");
    commented_on(&world, 1);
    world.has(vec![PullRequestFacts {
        review_requests: vec![me()],
        ..pull_request(2)
    }]);
    world.found(&involved(), &[1, 2]);
    world.mute_on_its_own(2);
    world.mute_author(&colleague());
    assert_eq!(
        world.hidden_in(&filter(involved()), 2),
        Some(Hidden::MutedAuthor)
    );

    world.unmute_author(&colleague());

    assert_eq!(world.hidden_in(&filter(involved()), 1), None);
    assert_eq!(world.hidden_in(&filter(involved()), 2), Some(Hidden::Muted));
    world.finish();
}

#[test]
fn a_mute_of_the_user_under_another_account_leaves_their_own_items_as_they_are() {
    let world = World::new("mute-author-the-user");
    world.has(vec![own(2)]);
    world.happened(2, vec![comment(&colleague(), hours_ago(1))]);
    // A mute another account made in the same data folder, before the token changed.
    world.mute_author(&me());

    assert!(world.on_workbench(2));
    assert_eq!(world.hidden_in(&WorkbenchView::Whole, 2), None);
    world.finish();
}

#[test]
fn a_muted_authors_archived_item_leaves_the_archived_view() {
    let world = World::new("mute-author-archived");
    commented_on(&world, 1);
    let (key, SeenMark { numbers, up_to }) = world.drawn_in(&WorkbenchView::Whole, 1);
    let archiving = crate::store::Archiving {
        key,
        numbers,
        up_to,
        put_away: crate::facts::activity::PutAway::Now,
    };
    let now = OffsetDateTime::now_utc();
    world.db.store.archive(&cpython(), archiving, now).unwrap();
    assert_eq!(world.view(&WorkbenchView::Archived).archived, 1);

    world.mute_author(&colleague());

    let read = world.view(&WorkbenchView::Archived);
    assert!(read.items.is_empty());
    assert_eq!(read.archived, 0);
    world.finish();
}

#[test]
fn each_line_offers_mute_author_for_its_author_but_the_users_own_and_a_gone_account() {
    let world = World::new("mute-author-offers");
    commented_on(&world, 1);
    world.has(vec![own(2)]);
    world.has(vec![PullRequestFacts {
        author: None,
        review_requests: vec![me()],
        ..pull_request(3)
    }]);
    world.has_issues(vec![issue(5)]);
    world.has(vec![fixing_too(reviewed_by_me(6), 5)]);
    world.happened(6, vec![comment(&colleague(), hours_ago(2))]);
    let mute = |login: Login| MuteAuthor::Mute { login };
    let offered = |number| mute_author_of(&world.listed_in(&WorkbenchView::Whole, number).item);
    assert_eq!(offered(1), mute(colleague()));
    assert_eq!(offered(2), MuteAuthor::Off);
    assert_eq!(offered(3), MuteAuthor::Off);
    assert_eq!(offered(5), mute(Login::new("issue-author")));

    world.mute_author(&colleague());

    let WorkbenchItem::Issue(issue) = world.listed_in(&WorkbenchView::Whole, 5).item else {
        panic!("Issue 5 is an Issue");
    };
    let TreeEntry::Pr(tree) = &issue.pull_requests[0] else {
        panic!("PR 6 sits under Issue 5");
    };
    let login = colleague();
    assert_eq!(tree.row.mute_author, MuteAuthor::Unmute { login });
    world.finish();
}

fn assigned(at: OffsetDateTime) -> Activity {
    Activity {
        kind: ActivityKind::Assigned,
        who: me(),
        at,
    }
}
