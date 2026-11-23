use time::Duration;

use super::*;
use crate::facts::activity::{ActivityKind, ItemKey, PutAway};
use crate::facts::issues::IssueFacts;
use crate::facts::pull_requests::{PrState, PullRequestFacts};
use crate::store::testing::{OpenStore, archived_on_closing, seeing};
use crate::store::{Archiving, AuthorMuting, Flagging, Muting, Opened};
use crate::testing::{cpython, cpython_settings, hours_ago, me};
use crate::work::Why;
use crate::workbench::outcomes::{Outcome, Place};
use crate::workbench::testing::{
    BOT, Doing, ViewWorld, assigned_issue, backport, fixing, issue, mine, outcomes_done,
    outline_of, own, stranger,
};

/// A moment well after the epoch, so two days before it is too.
fn now() -> OffsetDateTime {
    OffsetDateTime::UNIX_EPOCH + Duration::days(100)
}

fn ago(hours: i64) -> OffsetDateTime {
    now() - Duration::hours(hours)
}

fn issue_at(number: u32, updated_at: OffsetDateTime) -> IssueFacts {
    IssueFacts {
        updated_at,
        ..issue(number)
    }
}

/// Someone else's PR that names Issue `issue` in its title, or no Issue for `None`.
fn pr_at(number: u32, issue: Option<u32>, updated_at: OffsetDateTime) -> PullRequestFacts {
    let title = issue.map_or_else(
        || format!("PR {number}"),
        |issue| format!("gh-{issue}: Fix"),
    );
    PullRequestFacts {
        number,
        title,
        updated_at,
        ..stranger()
    }
}

/// Each item as its kind and number, with the PRs it shows after it.
fn outline(inbox: &Inbox) -> Vec<(&'static str, u32, Vec<u32>)> {
    outline_of(&inbox.items)
}

fn read(world: &ViewWorld, page: InboxPage) -> Inbox {
    Inbox::read(&world.db.reader, &world.settings(), page, now()).unwrap()
}

fn in_state(state: PrState, facts: PullRequestFacts) -> PullRequestFacts {
    PullRequestFacts { state, ..facts }
}

/// A Backport on 3.14 of PR `origin`, which names no Issue of its own.
fn backport_at(number: u32, origin: u32, updated_at: OffsetDateTime) -> PullRequestFacts {
    let title = format!("[3.14] Fix it (GH-{origin})");
    PullRequestFacts {
        updated_at,
        ..backport(number, &title, vec![])
    }
}

#[test]
fn open_issues_come_with_all_their_prs_under_them_and_standalone_prs_on_their_own_latest_first() {
    let world = ViewWorld::new("inbox-rows");
    world.has(
        vec![
            pr_at(10, Some(1), ago(5)),
            pr_at(11, Some(1), ago(3)),
            in_state(PrState::Merged, pr_at(13, Some(1), ago(1))),
            in_state(PrState::Closed, pr_at(14, Some(2), ago(7))),
            pr_at(20, None, ago(4)),
            pr_at(21, Some(99), ago(6)),
        ],
        vec![issue_at(1, ago(10)), issue_at(2, ago(2))],
    );

    let latest = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(
        outline(&latest),
        [
            ("issue", 1, vec![13, 11, 10]),
            ("issue", 2, vec![14]),
            ("standalone", 20, vec![]),
            ("standalone", 21, vec![]),
        ],
        "an Issue takes the latest update of its tree, merged PRs and all, and a PR naming an Issue the \
         cache lacks stands alone"
    );
    assert_eq!(latest.older, None);
}

#[test]
fn a_closed_issue_leaves_with_its_prs_and_so_does_a_standalone_pr_with_none_open() {
    let world = ViewWorld::new("inbox-closed");
    let closed_issue = IssueFacts {
        state: Some(IssueState::Closed),
        ..issue_at(1, ago(1))
    };
    world.has(
        vec![
            pr_at(10, Some(1), ago(2)),
            in_state(PrState::Closed, pr_at(30, None, ago(1))),
            in_state(PrState::Merged, pr_at(31, None, ago(1))),
            in_state(PrState::Merged, backport_at(32, 31, ago(1))),
        ],
        vec![closed_issue, issue_at(2, ago(3))],
    );

    let latest = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(outline(&latest), [("issue", 2, vec![])]);
}

#[test]
fn a_closed_issue_a_pr_brought_back_stands_at_its_prs_latest_update() {
    let world = ViewWorld::new("inbox-brought-back");
    let closed = |number, updated_at| IssueFacts {
        state: Some(IssueState::Closed),
        ..issue_at(number, updated_at)
    };
    world.has(
        vec![pr_at(10, Some(1), ago(80)), pr_at(20, Some(4), ago(90))],
        vec![
            closed(1, ago(60)),
            issue_at(2, ago(1)),
            issue_at(3, ago(70)),
            closed(4, ago(85)),
        ],
    );
    let store = &world.db.store;
    archived_on_closing(store, &[ItemKey::Issue { number: 1 }], ago(60));
    let opened = Opened {
        issues: vec![1],
        openings: BTreeSet::new(),
    };
    store.bring_back(&cpython(), opened).unwrap();

    let latest = read(&world, InboxPage::Latest);
    let after = latest.older.unwrap();
    let older = read(&world, InboxPage::Older { after });
    world.finish();

    assert_eq!(outline(&latest), [("issue", 2, vec![])]);
    // The walk meets the updates of open Issues and of PRs, so the comment on Issue 1 after it closed
    // doesn't move it. Nothing brought back the closed Issue 4.
    assert_eq!(
        outline(&older),
        [("issue", 3, vec![]), ("issue", 1, vec![10])]
    );
}

#[test]
fn a_backport_whose_origin_pr_merged_sits_beneath_it_under_its_issue() {
    let world = ViewWorld::new("inbox-merged-origin");
    world.has(
        vec![
            in_state(PrState::Merged, pr_at(10, Some(1), ago(24 * 20))),
            backport_at(11, 10, ago(1)),
            in_state(PrState::Merged, pr_at(20, None, ago(24 * 20))),
            backport_at(21, 20, ago(2)),
        ],
        vec![issue_at(1, ago(24 * 30))],
    );

    let latest = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(
        outline(&latest),
        [("issue", 1, vec![10, 11]), ("standalone", 20, vec![21])],
        "the Backport's update places its tree, and an open Backport keeps a merged Standalone PR listed"
    );
}

#[test]
fn a_change_of_conventions_regroups_the_prs_with_no_sync() {
    let world = ViewWorld::new("inbox-conventions");
    world.has(
        vec![pr_at(10, Some(1), ago(1)), backport_at(11, 10, ago(2))],
        vec![issue_at(1, ago(3))],
    );
    let github = RepositorySettings {
        conventions: Conventions::Github,
        ..world.settings()
    };

    let under_cpython = read(&world, InboxPage::Latest);
    let under_github = Inbox::read(&world.db.reader, &github, InboxPage::Latest, now()).unwrap();
    world.finish();

    assert_eq!(outline(&under_cpython), [("issue", 1, vec![10, 11])]);
    assert_eq!(
        outline(&under_github),
        [
            ("standalone", 10, vec![]),
            ("standalone", 11, vec![]),
            ("issue", 1, vec![])
        ]
    );
}

#[test]
fn an_older_page_places_an_issue_by_a_merged_pr_and_a_backport_beneath_another_issue_s_pr() {
    let world = ViewWorld::new("inbox-older-trees");
    // Backport 21 names Issue 2 and sits beneath PR 20, which fixes Issue 1, so PR 20 shows under both.
    let fixing_two = PullRequestFacts {
        title: "[3.14] gh-2: Fix it (GH-20)".to_owned(),
        ..backport_at(21, 20, ago(24 * 5))
    };
    world.has(
        vec![
            in_state(PrState::Merged, pr_at(10, Some(3), ago(24 * 4))),
            pr_at(20, Some(1), ago(24 * 9)),
            fixing_two,
        ],
        vec![
            issue_at(1, ago(24 * 10)),
            issue_at(2, ago(24 * 10)),
            issue_at(3, ago(24 * 10)),
            issue_at(4, ago(24 * 6)),
        ],
    );

    let latest = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(
        outline(&latest),
        [
            ("issue", 3, vec![10]),
            ("issue", 2, vec![20, 21]),
            ("issue", 4, vec![]),
            ("issue", 1, vec![20]),
        ]
    );
    assert_eq!(latest.older, None);
}

#[test]
fn a_backport_that_moved_brings_its_own_issue_to_the_first_page_and_leaves_its_origin_s_issue_older()
 {
    let world = ViewWorld::new("inbox-backport-moved");
    // Backport 21 fixes Issue 2 and sits beneath PR 20, which fixes Issue 1. Under Issue 1, PR 20 shows
    // without it, so nothing under Issue 1 moved.
    let fixing_two = PullRequestFacts {
        title: "[3.14] gh-2: Fix it (GH-20)".to_owned(),
        ..backport_at(21, 20, ago(1))
    };
    world.has(
        vec![
            in_state(PrState::Merged, pr_at(20, Some(1), ago(24 * 10))),
            fixing_two,
        ],
        vec![issue_at(1, ago(24 * 10)), issue_at(2, ago(24 * 10))],
    );

    let latest = read(&world, InboxPage::Latest);
    let after = latest.older.unwrap();
    let older = read(&world, InboxPage::Older { after });
    world.finish();

    assert_eq!(outline(&latest), [("issue", 2, vec![20, 21])]);
    assert_eq!(outline(&older), [("issue", 1, vec![20])]);
}

#[test]
fn the_first_page_holds_the_last_two_days_and_the_next_the_older_batch() {
    let world = ViewWorld::new("inbox-pages");
    world.has(
        vec![pr_at(10, Some(3), ago(30)), pr_at(20, None, ago(50))],
        vec![
            issue_at(1, ago(1)),
            issue_at(2, ago(47)),
            issue_at(3, ago(100)),
            issue_at(4, ago(49)),
            issue_at(5, ago(24 * 30)),
        ],
    );

    let latest = read(&world, InboxPage::Latest);
    let after = latest.older.unwrap();
    let older = read(&world, InboxPage::Older { after });
    world.finish();

    assert_eq!(
        outline(&latest),
        [
            ("issue", 1, vec![]),
            ("issue", 3, vec![10]),
            ("issue", 2, vec![])
        ],
        "an old Issue whose PR moved in the last two days comes with it"
    );
    assert_eq!(after, InboxCursor::at(ago(48)));
    assert_eq!(
        outline(&older),
        [
            ("issue", 4, vec![]),
            ("standalone", 20, vec![]),
            ("issue", 5, vec![])
        ]
    );
    assert_eq!(older.older, None);
}

#[test]
fn archive_takes_an_item_out_and_a_page_of_archived_items_gives_way_to_the_next() {
    let world = ViewWorld::new("inbox-archived");
    world.has(
        vec![pr_at(20, None, ago(1)), pr_at(21, None, ago(50))],
        vec![issue_at(1, ago(2)), issue_at(2, ago(60))],
    );
    let archive = |key, number| {
        let archiving = Archiving {
            key,
            numbers: vec![number],
            up_to: Some(ago(0)),
            put_away: PutAway::Now,
            findings: vec![],
        };
        world
            .db
            .store
            .archive(&cpython(), archiving, now())
            .unwrap();
    };
    archive(ItemKey::PullRequest { number: 20 }, 20);
    archive(ItemKey::Issue { number: 1 }, 1);
    archive(ItemKey::Issue { number: 2 }, 2);

    let latest = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(outline(&latest), [("standalone", 21, vec![])]);
    assert_eq!(latest.older, None);
}

#[test]
fn mute_takes_an_item_out() {
    let world = ViewWorld::new("inbox-muted");
    world.has(
        vec![pr_at(20, None, ago(1)), pr_at(21, None, ago(50))],
        vec![issue_at(1, ago(2))],
    );
    let mute = |key, number| {
        let muting = Muting {
            key,
            numbers: vec![number],
            up_to: Some(ago(0)),
            findings: vec![],
        };
        world.db.store.mute(&cpython(), muting, now()).unwrap();
    };
    mute(ItemKey::PullRequest { number: 20 }, 20);
    mute(ItemKey::Issue { number: 1 }, 1);

    let latest = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(outline(&latest), [("standalone", 21, vec![])]);
}

#[test]
fn a_pin_takes_an_issue_out_until_unpinned() {
    let world = ViewWorld::new("inbox-pinned");
    world.has(
        vec![pr_at(10, Some(1), ago(1)), pr_at(20, None, ago(3))],
        vec![issue_at(1, ago(2)), issue_at(2, ago(4))],
    );
    let store = &world.db.store;
    store.pin(&cpython(), 1).unwrap();
    // Only an Issue takes a pin, so a pin on a Standalone PR's number pins nothing in the Inbox.
    store.pin(&cpython(), 20).unwrap();

    let pinned = read(&world, InboxPage::Latest);
    crate::store::testing::unpin(store, &cpython(), 1);
    let unpinned = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(
        outline(&pinned),
        [("standalone", 20, vec![]), ("issue", 2, vec![])]
    );
    assert_eq!(
        outline(&unpinned),
        [
            ("issue", 1, vec![10]),
            ("standalone", 20, vec![]),
            ("issue", 2, vec![])
        ]
    );
}

/// `facts`, written by `login`.
fn by(login: &str, facts: PullRequestFacts) -> PullRequestFacts {
    PullRequestFacts {
        author: Some(Login::new(login)),
        ..facts
    }
}

impl ViewWorld {
    /// Mute author on `login`, from now.
    fn mute_author(&self, login: &str) {
        self.mute_author_at(login, now());
    }

    fn mute_author_at(&self, login: &str, at: OffsetDateTime) {
        let muting = AuthorMuting {
            author: Login::new(login),
            marks: Vec::new(),
        };
        self.db.store.mute_author(&cpython(), muting, at).unwrap();
    }

    /// Mark Unseen on Issue or PR `number`, two hours ago.
    fn marked_unseen(&self, number: u32) {
        let store = &self.db.store;
        store
            .mark_unseen(&cpython(), Flagging::on(number), ago(2))
            .unwrap();
    }

    fn show_muted(&self, show: ShowMuted) {
        self.db.store.switch_show_muted(&cpython(), show).unwrap();
    }
}

#[test]
fn with_show_muted_off_no_row_by_a_muted_author_is_listed_and_on_they_come_back_in_place() {
    let world = ViewWorld::new("inbox-show-muted");
    world.has(
        vec![
            pr_at(10, Some(1), ago(1)),
            by("other-author", pr_at(11, Some(1), ago(5))),
            pr_at(20, None, ago(2)),
            by("other-author", pr_at(21, None, ago(3))),
            own(pr_at(22, None, ago(4))),
        ],
        vec![issue_at(1, ago(10))],
    );
    world.mute_author("some-author");
    // A mute of the user's own login, made under another account, hides none of their rows (#494).
    world.mute_author(me().as_str());

    let hidden = read(&world, InboxPage::Latest);
    world.show_muted(ShowMuted::On);
    let shown = read(&world, InboxPage::Latest);
    world.show_muted(ShowMuted::Off);
    let unmuted = Login::new("some-author");
    world.db.store.unmute_author(&cpython(), &unmuted).unwrap();
    let back = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(
        outline(&hidden),
        [
            ("issue", 1, vec![11]),
            ("standalone", 21, vec![]),
            ("standalone", 22, vec![])
        ],
        "a PR under someone else's Issue hides its own row alone, and the Issue keeps its place"
    );
    assert_eq!(hidden.show_muted, ShowMuted::Off);
    let all = [
        ("issue", 1, vec![10, 11]),
        ("standalone", 20, vec![]),
        ("standalone", 21, vec![]),
        ("standalone", 22, vec![]),
    ];
    assert_eq!(outline(&shown), all);
    assert_eq!(shown.show_muted, ShowMuted::On);
    assert_eq!(outline(&back), all, "unmuting brings their rows back");
}

#[test]
fn a_muted_authors_issue_stays_as_the_header_of_another_authors_pr_and_goes_with_the_last() {
    let world = ViewWorld::new("inbox-muted-header");
    world.has(
        vec![
            pr_at(10, Some(1), ago(1)),
            by("issue-author", pr_at(12, Some(3), ago(3))),
            in_state(PrState::Merged, pr_at(20, None, ago(24 * 20))),
            by("issue-author", backport_at(21, 20, ago(4))),
            by(
                "issue-author",
                in_state(PrState::Merged, pr_at(30, None, ago(24 * 20))),
            ),
            by(BOT, backport_at(31, 30, ago(5))),
        ],
        vec![
            issue_at(1, ago(10)),
            issue_at(2, ago(2)),
            issue_at(3, ago(10)),
        ],
    );
    world.mute_author("issue-author");

    let latest = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(
        outline(&latest),
        [
            ("issue", 1, vec![10]),
            ("standalone", 20, vec![]),
            ("standalone", 30, vec![31])
        ],
        "a row heads what stays under it: an Issue its PRs, a PR its Backports"
    );
    assert_eq!(latest.items[0].hidden, Some(Hidden::MutedAuthor));
}

#[test]
fn a_muted_authors_backport_hides_its_own_row_under_an_issue_or_on_its_own() {
    let world = ViewWorld::new("inbox-muted-backport");
    // Backport 22 names Backport 21 as its Origin PR, and Backport 21 names a PR the cache lacks, so 22 heads
    // a line and 21 sits under Issue 1 on its own.
    let backport_of_backport = PullRequestFacts {
        title: "[3.13] Fix it (GH-21)".to_owned(),
        ..backport_at(22, 21, ago(1))
    };
    let older_backport = PullRequestFacts {
        title: "[3.14] gh-1: Fix it (GH-20)".to_owned(),
        ..backport_at(21, 20, ago(2))
    };
    world.has(
        vec![older_backport, backport_of_backport],
        vec![issue_at(1, ago(3))],
    );
    world.mute_author("some-author");

    let hidden = read(&world, InboxPage::Latest);
    world.show_muted(ShowMuted::On);
    let shown = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(outline(&hidden), [("issue", 1, vec![])]);
    assert_eq!(
        outline(&shown),
        [("standalone backport", 22, vec![]), ("issue", 1, vec![21])]
    );
}

#[test]
fn an_ask_after_the_mute_shows_a_muted_authors_row_with_show_muted_off_and_one_from_before_doesnt()
{
    let world = ViewWorld::new("inbox-muted-asked");
    let requested = |facts| PullRequestFacts {
        review_requests: vec![me()],
        ..facts
    };
    let others_issue = IssueFacts {
        author: Some(Login::new("other-author")),
        ..issue_at(2, ago(9))
    };
    world.has(
        vec![
            pr_at(10, Some(1), ago(1)),
            requested(pr_at(12, Some(2), ago(4))),
            pr_at(13, Some(2), ago(5)),
            requested(pr_at(20, None, ago(2))),
            requested(pr_at(21, None, ago(3))),
        ],
        vec![assigned_issue(1), others_issue],
    );
    world.mute_author_at("some-author", ago(10));
    world.mute_author_at("issue-author", ago(10));
    world.happened(1, &[(ActivityKind::Assigned, ago(8))]);
    world.happened(12, &[(ActivityKind::ReviewRequested, ago(7))]);
    world.happened(20, &[(ActivityKind::ReviewRequested, ago(6))]);
    // This one stood at the mute, which acknowledged it.
    world.happened(21, &[(ActivityKind::ReviewRequested, ago(11))]);

    let latest = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(
        outline(&latest),
        [
            ("issue", 1, vec![10]),
            ("standalone", 20, vec![]),
            ("issue", 2, vec![12])
        ],
        "an assignment on a Muted author's Issue shows its whole tree, and a request on a PR its own row"
    );
}

#[test]
fn an_item_needs_attention_for_the_rows_show_muted_leaves() {
    let world = ViewWorld::new("inbox-muted-attention");
    let requested = PullRequestFacts {
        review_requests: vec![me()],
        ..pr_at(10, Some(1), ago(1))
    };
    world.has(
        vec![requested, by("other-author", pr_at(11, Some(1), ago(2)))],
        vec![issue_at(1, ago(3))],
    );
    world.mute_author("some-author");

    let hidden = read(&world, InboxPage::Latest);
    world.show_muted(ShowMuted::On);
    let shown = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(outline(&hidden), [("issue", 1, vec![11])]);
    assert_eq!(hidden.items[0].attention, None);
    assert!(shown.items[0].attention.is_some());
}

#[test]
fn a_muted_authors_row_an_ask_keeps_needs_attention_for_the_ask_alone() {
    let world = ViewWorld::new("inbox-muted-ask-attention");
    let requested = PullRequestFacts {
        review_requests: vec![me()],
        ..pr_at(10, Some(1), ago(1))
    };
    world.has(vec![requested], vec![issue_at(1, ago(3))]);
    world.mute_author_at("some-author", ago(10));
    world.happened(10, &[(ActivityKind::ReviewRequested, ago(5))]);
    world.marked_unseen(10);

    let latest = read(&world, InboxPage::Latest);
    world.show_muted(ShowMuted::On);
    let shown = read(&world, InboxPage::Latest);
    world.finish();

    let why = |inbox: &Inbox| {
        let attention = inbox.items[0].attention.as_ref();
        attention.map(|attention| attention.why.clone())
    };
    assert_eq!(outline(&latest), [("issue", 1, vec![10])]);
    assert_eq!(why(&latest), Some(vec![Why::ReviewRequested]));
    assert_eq!(
        why(&shown),
        Some(vec![Why::MarkedUnseen, Why::ReviewRequested]),
        "the flag counts on an item no Muted author wrote, once Show muted shows all its rows"
    );
}

#[test]
fn mark_seen_acknowledges_the_ask_that_kept_a_muted_authors_row_which_then_goes() {
    let world = ViewWorld::new("inbox-muted-ask-seen");
    let requested = PullRequestFacts {
        review_requests: vec![me()],
        ..pr_at(10, Some(1), ago(1))
    };
    world.has(
        vec![requested, by("other-author", pr_at(11, Some(1), ago(2)))],
        vec![issue_at(1, ago(3))],
    );
    world.mute_author_at("some-author", ago(10));
    world.happened(10, &[(ActivityKind::ReviewRequested, ago(5))]);

    let asked = read(&world, InboxPage::Latest);
    let store = &world.db.store;
    let marked = seeing(vec![1, 10, 11], Some(ago(0)));
    store.mark_seen(&cpython(), marked).unwrap();
    let seen = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(outline(&asked), [("issue", 1, vec![10, 11])]);
    assert_eq!(
        outline(&seen),
        [("issue", 1, vec![11])],
        "the request still stands, and a Muted item leaves the Workbench too once marked seen"
    );
}

#[test]
fn an_archived_issue_stays_out_when_only_a_muted_authors_pr_under_it_moved() {
    let world = ViewWorld::new("inbox-muted-archived");
    world.has(
        vec![
            pr_at(10, Some(1), ago(1)),
            by("other-author", pr_at(11, Some(2), ago(1))),
        ],
        vec![issue_at(1, ago(6)), issue_at(2, ago(6))],
    );
    world.mute_author_at("some-author", ago(5));
    for (issue, pr) in [(1, 10), (2, 11)] {
        let archiving = Archiving {
            key: ItemKey::Issue { number: issue },
            numbers: vec![issue, pr],
            up_to: Some(ago(4)),
            put_away: PutAway::Now,
            findings: vec![],
        };
        let store = &world.db.store;
        store.archive(&cpython(), archiving, ago(4)).unwrap();
        world.marked_unseen(pr);
    }

    let hidden = read(&world, InboxPage::Latest);
    world.show_muted(ShowMuted::On);
    let shown = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(outline(&hidden), [("issue", 2, vec![11])]);
    assert_eq!(
        outline(&shown),
        [("issue", 2, vec![11]), ("issue", 1, vec![10])],
        "the Muted author's PR brings its Issue back once it shows"
    );
}

#[test]
fn a_page_that_show_muted_empties_gives_way_to_the_next() {
    let world = ViewWorld::new("inbox-muted-page");
    world.has(
        vec![
            pr_at(20, None, ago(1)),
            by("other-author", pr_at(21, None, ago(50))),
        ],
        vec![],
    );
    world.mute_author("some-author");

    let latest = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(outline(&latest), [("standalone", 21, vec![])]);
    assert_eq!(latest.older, None);
}

#[test]
fn a_muted_authors_items_stay_with_show_muted_on_unless_muted_or_archived_on_their_own() {
    let world = ViewWorld::new("inbox-muted-author");
    world.has(
        vec![
            pr_at(20, None, ago(1)),
            pr_at(21, None, ago(3)),
            pr_at(22, None, ago(4)),
        ],
        vec![issue_at(1, ago(2))],
    );
    let muting = Muting {
        key: ItemKey::PullRequest { number: 21 },
        numbers: vec![21],
        up_to: Some(ago(0)),
        findings: vec![],
    };
    world.db.store.mute(&cpython(), muting, now()).unwrap();
    let archiving = Archiving {
        key: ItemKey::PullRequest { number: 22 },
        numbers: vec![22],
        up_to: Some(ago(0)),
        put_away: PutAway::Now,
        findings: vec![],
    };
    world
        .db
        .store
        .archive(&cpython(), archiving, now())
        .unwrap();
    world.mute_author("some-author");
    world.show_muted(ShowMuted::On);

    let latest = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(
        outline(&latest),
        [("standalone", 20, vec![]), ("issue", 1, vec![])]
    );
    assert_eq!(latest.items[0].hidden, Some(Hidden::MutedAuthor));
}

#[test]
fn a_first_page_with_items_looks_one_step_down_for_an_older_page() {
    let world = ViewWorld::new("inbox-one-step");
    // More older Issues than the step that looks for an older page reads, so it reads only some of them.
    let count = STEP + 10;
    let older = (1..=count).map(|number| issue_at(number, ago(100 + i64::from(number))));
    world.has(
        vec![],
        std::iter::once(issue_at(100, ago(1)))
            .chain(older)
            .collect(),
    );

    let latest = read(&world, InboxPage::Latest);
    let after = latest.older.unwrap();
    let older = read(&world, InboxPage::Older { after });
    world.finish();

    assert_eq!(outline(&latest), [("issue", 100, vec![])]);
    let numbers: Vec<u32> = (outline(&older).into_iter())
        .map(|(_, number, _)| number)
        .collect();
    assert_eq!(numbers, (1..=count).collect::<Vec<u32>>());
    assert_eq!(older.older, None);
}

#[test]
fn a_backport_of_a_backport_stands_on_its_own_line() {
    let world = ViewWorld::new("inbox-backport-of-backport");
    // Backport 22 names Backport 21 as its Origin PR, and a tree stays two levels, so each heads a line.
    let backport_of_backport = PullRequestFacts {
        title: "[3.13] Fix it (GH-21)".to_owned(),
        ..backport_at(22, 21, ago(1))
    };
    let older_backport = PullRequestFacts {
        title: "[3.14] gh-1: Fix it (GH-20)".to_owned(),
        ..backport_at(21, 20, ago(24 * 10))
    };
    world.has(
        vec![
            in_state(PrState::Merged, older_backport),
            backport_of_backport,
        ],
        vec![issue_at(1, ago(24 * 10))],
    );

    let latest = read(&world, InboxPage::Latest);
    let after = latest.older.unwrap();
    let older = read(&world, InboxPage::Older { after });
    world.finish();

    assert_eq!(outline(&latest), [("standalone backport", 22, vec![])]);
    assert_eq!(outline(&older), [("issue", 1, vec![21])]);
}

#[test]
fn an_older_page_holds_a_batch_and_says_where_the_next_starts() {
    let world = ViewWorld::new("inbox-batches");
    // Enough Issues that the store's batch of those with no PR fills, and a group can fall past it.
    let count = OLDER_BATCH * 2 + 10;
    let hours = |number: u32| ago(100 + i64::from(number));
    let issues = (1..=count).map(|number| issue_at(number, hours(number)));
    // Every fifth Issue has a PR, which places the Issue from the groups and not from the store's batch.
    let prs = (1..=count)
        .filter(|number| number.is_multiple_of(5))
        .map(|number| pr_at(1000 + number, Some(number), hours(number)));
    world.has(prs.collect(), issues.collect());

    let first = read(&world, InboxPage::Latest);
    let second = read(
        &world,
        InboxPage::Older {
            after: first.older.unwrap(),
        },
    );
    let third = read(
        &world,
        InboxPage::Older {
            after: second.older.unwrap(),
        },
    );
    world.finish();

    let numbers = |inbox: &Inbox| -> Vec<u32> {
        outline(inbox)
            .into_iter()
            .map(|(_, number, _)| number)
            .collect()
    };
    let batch = usize::try_from(OLDER_BATCH).unwrap();
    let all: Vec<u32> = (1..=count).collect();
    assert_eq!(
        numbers(&first),
        all[..batch],
        "a first page with nothing from the last two days holds the first older batch"
    );
    assert_eq!(numbers(&second), all[batch..batch * 2]);
    assert_eq!(numbers(&third), all[batch * 2..]);
    assert_eq!(third.older, None);
}

#[test]
fn a_page_that_takes_the_last_items_says_nothing_is_older() {
    let world = ViewWorld::new("inbox-exact");
    let issues = (1..=OLDER_BATCH).map(|number| issue_at(number, ago(100 + i64::from(number))));
    world.has(vec![], issues.collect());

    let first = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(first.items.len(), usize::try_from(OLDER_BATCH).unwrap());
    assert_eq!(first.older, None);
}

/// The numbers of the items `inbox` lists, in its order.
fn numbers(inbox: &Inbox) -> Vec<u32> {
    (outline(inbox).into_iter())
        .map(|(_, number, _)| number)
        .collect()
}

#[test]
fn pages_read_again_through_where_they_ended_are_one_page_that_ends_there_too() {
    let world = ViewWorld::new("inbox-through");
    let count = OLDER_BATCH * 2 + 10;
    let issues = (1..=count).map(|number| issue_at(number, ago(100 + i64::from(number))));
    let recent = [issue_at(1001, ago(1)), issue_at(1002, ago(2))];
    world.has(vec![], recent.into_iter().chain(issues).collect());

    let first = read(&world, InboxPage::Latest);
    let second = read(
        &world,
        InboxPage::Older {
            after: first.older.unwrap(),
        },
    );
    let through_first = read(
        &world,
        InboxPage::Through {
            until: first.older.unwrap(),
        },
    );
    let through_second = read(
        &world,
        InboxPage::Through {
            until: second.older.unwrap(),
        },
    );
    let all = read(&world, InboxPage::All);
    world.finish();

    assert_eq!(numbers(&through_first), numbers(&first));
    assert_eq!(
        through_first.older, first.older,
        "the first page's start stays where it was, though the clock moved on"
    );
    let shown: Vec<u32> = numbers(&first)
        .into_iter()
        .chain(numbers(&second))
        .collect();
    assert_eq!(numbers(&through_second), shown);
    assert_eq!(through_second.older, second.older);
    let every: Vec<u32> = [1001, 1002].into_iter().chain(1..=count).collect();
    assert_eq!(numbers(&all), every);
    assert_eq!(all.older, None);
}

#[test]
fn a_page_through_the_last_items_says_nothing_is_older() {
    let world = ViewWorld::new("inbox-through-last");
    world.has(vec![], vec![issue_at(1, ago(1)), issue_at(2, ago(100))]);

    let through = read(
        &world,
        InboxPage::Through {
            until: InboxCursor::at(ago(100)),
        },
    );
    world.finish();

    assert_eq!(numbers(&through), [1, 2]);
    assert_eq!(through.older, None);
}

#[test]
fn a_page_through_a_place_no_item_reaches_any_more_holds_the_next_batch() {
    let world = ViewWorld::new("inbox-through-gone");
    // The items above where the pages on show ended have all closed.
    world.has(vec![], vec![issue_at(1, ago(100)), issue_at(2, ago(101))]);

    let through = read(
        &world,
        InboxPage::Through {
            until: InboxCursor::at(ago(50)),
        },
    );
    world.finish();

    assert_eq!(numbers(&through), [1, 2]);
    assert_eq!(through.older, None);
}

#[test]
fn the_first_page_says_nothing_is_older_when_only_its_issues_are() {
    let world = ViewWorld::new("inbox-named-older");
    world.has(
        vec![pr_at(10, Some(1), ago(1))],
        vec![issue_at(1, ago(100))],
    );

    let latest = read(&world, InboxPage::Latest);
    world.finish();

    assert_eq!(outline(&latest), [("issue", 1, vec![10])]);
    assert_eq!(
        latest.older, None,
        "Issue 1 is older, but its PR put it on this page"
    );
}

#[test]
fn an_item_updated_in_the_second_the_first_page_starts_is_on_it_alone() {
    let world = ViewWorld::new("inbox-boundary");
    world.has(
        vec![pr_at(20, Some(2), ago(48))],
        vec![
            issue_at(1, ago(48)),
            issue_at(2, ago(50)),
            issue_at(3, ago(49)),
        ],
    );
    let within_a_second = now() + Duration::milliseconds(500);

    let latest = Inbox::read(
        &world.db.reader,
        &world.settings(),
        InboxPage::Latest,
        within_a_second,
    )
    .unwrap();
    let older = read(
        &world,
        InboxPage::Older {
            after: latest.older.unwrap(),
        },
    );
    world.finish();

    assert_eq!(
        outline(&latest),
        [("issue", 2, vec![20]), ("issue", 1, vec![])]
    );
    assert_eq!(outline(&older), [("issue", 3, vec![])]);
}

#[test]
fn issues_their_prs_moved_up_leave_no_gap_in_an_older_page() {
    let world = ViewWorld::new("inbox-skip");
    let count = OLDER_BATCH;
    // The store's batch holds only Issues with a PR newer than the cursor, and an older one lies past it.
    let issues = (1..=count).map(|number| issue_at(number, ago(200 + i64::from(number))));
    let prs = (1..=count).map(|number| pr_at(1000 + number, Some(number), ago(1)));
    let older_issue = issue_at(500, ago(24 * 60));
    world.has(prs.collect(), issues.chain([older_issue]).collect());

    let latest = read(&world, InboxPage::Latest);
    let older = read(
        &world,
        InboxPage::Older {
            after: latest.older.unwrap(),
        },
    );
    world.finish();

    assert_eq!(latest.items.len(), usize::try_from(count).unwrap());
    assert_eq!(outline(&older), [("issue", 500, vec![])]);
    assert_eq!(older.older, None);
}

#[test]
fn rows_carry_what_the_workbench_rows_carry() {
    let world = ViewWorld::new("inbox-attention");
    let requested = PullRequestFacts {
        review_requests: vec![crate::workbench::testing::me()],
        ..pr_at(10, Some(1), ago(1))
    };
    world.has(vec![requested, mine(20)], vec![issue_at(1, ago(2))]);

    let latest = read(&world, InboxPage::Latest);
    world.finish();

    let why = (latest.items.first())
        .and_then(|listed| listed.attention.as_ref())
        .map(|attention| attention.why.clone());
    assert_eq!(why, Some(vec![crate::work::Why::ReviewRequested]));
}

#[test]
fn nothing_is_listed_before_anyone_signs_in() {
    let db = OpenStore::new("inbox-no-viewer");
    let store = &db.store;
    store
        .save_issues(&cpython(), vec![issue_at(1, ago(1))])
        .unwrap();
    store
        .save_pull_requests(&cpython(), vec![fixing(10, 1)])
        .unwrap();

    let read = Inbox::read(&db.reader, &cpython_settings(), InboxPage::Latest, now());

    db.close();
    let read = read.unwrap();
    assert_eq!(outline(&read), []);
    assert_eq!(read.older, None);
}

#[test]
fn the_window_sends_a_page_and_gets_the_next_one_s_start_as_text() {
    let asked: InboxPage = serde_json::from_value(serde_json::json!({
        "kind": "older",
        "after": { "updated_at": "1970-04-11T00:00:00Z", "number": 7 },
    }))
    .unwrap();
    let latest: InboxPage =
        serde_json::from_value(serde_json::json!({ "kind": "latest" })).unwrap();
    let through: InboxPage = serde_json::from_value(serde_json::json!({
        "kind": "through",
        "until": { "updated_at": "1970-04-11T00:00:00Z", "number": 7 },
    }))
    .unwrap();
    let all: InboxPage = serde_json::from_value(serde_json::json!({ "kind": "all" })).unwrap();
    let inbox = Inbox {
        items: vec![],
        older: Some(InboxCursor::at(now())),
        show_muted: ShowMuted::On,
    };

    let sent = serde_json::to_value(&inbox).unwrap();

    let after = InboxCursor {
        updated_at: now(),
        number: 7,
    };
    assert_eq!(asked, InboxPage::Older { after });
    assert_eq!(latest, InboxPage::Latest);
    assert_eq!(through, InboxPage::Through { until: after });
    assert_eq!(all, InboxPage::All);
    assert_eq!(
        sent["older"],
        serde_json::json!({ "updated_at": "1970-04-11T00:00:00Z", "number": 0 })
    );
    assert_eq!(sent["show_muted"], "on");
}

/// The outcome the first page at `at` gives each action on each item it lists in the world `build` makes,
/// each held against what the page lists once the store holds what the action writes (#1052).
fn judged(
    name: &str,
    at: OffsetDateTime,
    build: &dyn Fn(&ViewWorld),
) -> BTreeMap<(u32, Doing), Outcome> {
    let read = |world: &ViewWorld| {
        let read = Inbox::read(&world.db.reader, &world.settings(), InboxPage::Latest, at);
        let items = read.unwrap().items.into_iter();
        items.map(|listed| (listed, Place::Listed)).collect()
    };
    let judged = outcomes_done(name, build, &read).into_iter();
    judged
        .map(|(number, doing, outcome)| ((number, doing), outcome))
        .collect()
}

/// Issue 1, which the Issue's author opened, with PR 10 under it, which someone else wrote; Issue 2, which
/// the Issue's author opened too; and the Standalone PR 20. "Show muted" is as `show` says.
fn three_items(world: &ViewWorld, show: ShowMuted) {
    world.has(
        vec![pr_at(10, Some(1), ago(1)), pr_at(20, None, ago(3))],
        vec![issue_at(1, ago(10)), issue_at(2, ago(2))],
    );
    world.show_muted(show);
}

#[test]
fn each_outcome_in_the_inbox_is_what_the_inbox_lists_once_the_store_holds_the_action() {
    let off = judged("inbox-outcomes-off", now(), &|world| {
        three_items(world, ShowMuted::Off);
    });
    let on = judged("inbox-outcomes-on", now(), &|world| {
        three_items(world, ShowMuted::On);
    });

    use Doing::*;
    use Outcome::*;
    let item = |number| {
        [
            ((number, MarkSeen), Stays),
            ((number, Archive), Leaves),
            ((number, Mute), Leaves),
            ((number, MuteAuthor), Leaves),
        ]
    };
    let mut expected: BTreeMap<(u32, Doing), Outcome> =
        [item(1), item(2), item(20)].into_iter().flatten().collect();
    // A pin takes an Issue out. Mute author leaves Issue 1 as the header of PR 10, which someone else wrote,
    // and Mute author on that someone takes PR 10's row out from under it.
    expected.extend([
        ((1, Pin), Leaves),
        ((2, Pin), Leaves),
        ((1, MuteAuthor), Stays),
        ((10, MuteAuthor), Leaves),
    ]);
    assert_eq!(off, expected);
    // With "Show muted" on, a Muted author's item and row stay.
    expected.extend([
        ((2, MuteAuthor), Stays),
        ((20, MuteAuthor), Stays),
        ((10, MuteAuthor), Stays),
    ]);
    assert_eq!(on, expected);
}

#[test]
fn mute_author_with_show_muted_on_keeps_an_item_back_for_a_stale_review_its_marks_cant_acknowledge() {
    let build = |world: &ViewWorld| {
        let updated_at = hours_ago(1);
        world.reviewed_and_pushed(by("other-author", pr_at(30, None, updated_at)));
        let archiving = Archiving {
            key: ItemKey::PullRequest { number: 30 },
            numbers: vec![30],
            up_to: Some(updated_at),
            put_away: PutAway::Now,
            findings: vec![],
        };
        let store = &world.db.store;
        store.archive(&cpython(), archiving, updated_at).unwrap();
        // The check found the Stale review after the archive, which brings PR 30 back, though before
        // GitHub's last word on the PR.
        world.db.checks_kept(hours_ago(2));
        world.show_muted(ShowMuted::On);
    };

    let judged = judged("inbox-outcomes-stale-found", OffsetDateTime::now_utc(), &build);

    // Mute author's marks come from the facts, not from the page, so they leave the finding standing.
    assert_eq!(judged.get(&(30, Doing::MuteAuthor)), Some(&Outcome::Stays));
}

#[test]
fn mute_author_with_show_muted_off_keeps_an_item_where_a_fresh_ask_keeps_an_earlier_muted_authors_row()
 {
    let build = |world: &ViewWorld| {
        let closed = IssueFacts {
            state: Some(IssueState::Closed),
            ..issue_at(1, ago(9))
        };
        let requested = PullRequestFacts {
            review_requests: vec![me()],
            ..by("other-author", pr_at(10, Some(1), ago(1)))
        };
        world.has(vec![requested], vec![closed]);
        // The PR's opening brings the closed Issue back from its archive (#823).
        let store = &world.db.store;
        archived_on_closing(store, &[ItemKey::Issue { number: 1 }], ago(8));
        let opened = Opened {
            issues: vec![1],
            openings: BTreeSet::new(),
        };
        store.bring_back(&cpython(), opened).unwrap();
        world.mute_author_at("other-author", ago(10));
        world.happened(10, &[(ActivityKind::ReviewRequested, ago(5))]);
    };

    let judged = judged("inbox-outcomes-fresh-ask", now(), &build);

    // Mute author marks no closed item seen, so the ask stays fresh, and keeps PR 10 under its Issue.
    assert_eq!(judged.get(&(1, Doing::MuteAuthor)), Some(&Outcome::Stays));
}
