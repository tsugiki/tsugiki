use time::Duration;

use super::*;
use crate::facts::activity::{Activity, ActivityKind, ItemActivity, ItemKey, PutAway};
use crate::facts::inbox_search::{InboxSearch, RowKind, SearchHit};
use crate::facts::issues::IssueFacts;
use crate::facts::labels::Label;
use crate::facts::pull_requests::{PrState, PullRequestFacts};
use crate::store::testing::{OpenStore, archived_on_closing, seeing};
use crate::store::{Archiving, AuthorMuting, Flagging, Muting, Opened};
use crate::testing::{cpython, cpython_settings, me};
use crate::work::Why;
use crate::workbench::testing::{
    BOT, ViewWorld, assigned_issue, backport, fixing, issue, mine, outline_of, own, stranger,
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
    Inbox::read(
        &world.db.reader,
        &world.settings(),
        InboxAsk { page, search: "" },
        now(),
    )
    .unwrap()
}

/// The Inbox `text` narrows the whole cache to, as the search box reads it (#464).
fn searched(world: &ViewWorld, text: &str) -> Inbox {
    Inbox::read(
        &world.db.reader,
        &world.settings(),
        InboxAsk {
            page: InboxPage::Latest,
            search: text,
        },
        now(),
    )
    .unwrap()
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
    let under_github = Inbox::read(
        &world.db.reader,
        &github,
        InboxAsk {
            page: InboxPage::Latest,
            search: "",
        },
        now(),
    )
    .unwrap();
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

    /// Each event, one that names the user as its kind says, happened on Issue or PR `number` at its moment.
    fn happened(&self, number: u32, events: &[(ActivityKind, OffsetDateTime)]) {
        let named = |(kind, at): &(ActivityKind, OffsetDateTime)| Activity {
            kind: kind.clone(),
            who: me(),
            at: *at,
        };
        let events = events.iter().map(named).collect();
        let activity = vec![ItemActivity { number, events }];
        let store = &self.db.store;
        store
            .save_fetched_pull_requests(&cpython(), vec![], activity)
            .unwrap();
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
        InboxAsk {
            page: InboxPage::Latest,
            search: "",
        },
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

    let read = Inbox::read(
        &db.reader,
        &cpython_settings(),
        InboxAsk {
            page: InboxPage::Latest,
            search: "",
        },
        now(),
    );

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
        needs_github: false,
        truncated: None,
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

#[test]
fn a_search_narrows_the_whole_cache_by_every_word_of_a_title() {
    let world = ViewWorld::new("inbox-search-title");
    let old_issue = IssueFacts {
        title: "Parser crash on import".to_owned(),
        ..issue_at(1, ago(24 * 100))
    };
    let old_pr = PullRequestFacts {
        title: "gh-1: Fix the parser".to_owned(),
        ..pr_at(10, Some(1), ago(24 * 100))
    };
    world.has(
        vec![old_pr, pr_at(20, None, ago(1))],
        vec![old_issue, issue_at(2, ago(1))],
    );

    let found = searched(&world, "parser");
    world.finish();

    assert_eq!(
        outline(&found),
        [("issue", 1, vec![10])],
        "the item is older than the first page's two days, and one word of the title finds it"
    );
    assert_eq!(found.older, None);
}

#[test]
fn a_matching_pr_stays_in_its_issue_s_fold_and_a_matching_issue_with_none_shows_alone() {
    let world = ViewWorld::new("inbox-search-fold");
    let mut matched_pr = pr_at(10, Some(1), ago(4));
    matched_pr.title = "gh-1: Fix the parser".to_owned();
    let mut other_pr = pr_at(11, Some(1), ago(6));
    other_pr.title = "gh-1: Something else".to_owned();
    let mut matched_issue = issue_at(2, ago(3));
    matched_issue.title = "Parser speed".to_owned();
    let mut other = pr_at(20, Some(2), ago(2));
    other.title = "gh-2: Unrelated".to_owned();
    world.has(
        vec![matched_pr, other_pr, other],
        vec![issue_at(1, ago(5)), matched_issue],
    );

    let found = searched(&world, "parser");
    world.finish();

    assert_eq!(
        outline(&found),
        [("issue", 2, vec![]), ("issue", 1, vec![10])],
        "an Issue whose own title doesn't match keeps the header of the PR that does, and one that \
         matches shows alone"
    );
}

#[test]
fn is_closed_lists_the_cached_closed_rows() {
    let world = ViewWorld::new("inbox-search-closed");
    let mut open = pr_at(10, Some(1), ago(5));
    open.title = "gh-1: Fix it".to_owned();
    let mut merged = in_state(PrState::Merged, pr_at(11, Some(1), ago(1)));
    merged.title = "gh-1: Fix it for 3.14".to_owned();
    let mut standalone = in_state(PrState::Closed, pr_at(30, None, ago(3)));
    standalone.title = "Fix it again".to_owned();
    world.has(vec![open, merged, standalone], vec![issue_at(1, ago(6))]);

    let found = searched(&world, "is:closed fix");
    world.finish();

    assert_eq!(
        outline(&found),
        [("issue", 1, vec![11]), ("standalone", 30, vec![])],
        "the merged PR under an open Issue and the closed Standalone PR, and neither open row"
    );
}

#[test]
fn a_label_and_an_author_narrow_the_rows_together() {
    let world = ViewWorld::new("inbox-search-label");
    let labelled = IssueFacts {
        labels: vec![Label::new("bug")],
        ..issue_at(1, ago(2))
    };
    let other = IssueFacts {
        labels: vec![Label::new("docs")],
        ..issue_at(2, ago(1))
    };
    world.has(vec![], vec![labelled, other]);

    let found = searched(&world, "label:BUG author:issue-author");
    world.finish();

    assert_eq!(outline(&found), [("issue", 1, vec![])]);
}

#[test]
fn a_search_keeps_an_orphan_backport_under_its_issue_and_a_standalone_backport_on_its_own() {
    let world = ViewWorld::new("inbox-search-backports");
    let orphan = PullRequestFacts {
        title: "[3.14] gh-1: Fix it (GH-999)".to_owned(),
        ..backport_at(10, 999, ago(2))
    };
    let beneath = PullRequestFacts {
        title: "[3.14] Fix it (GH-20)".to_owned(),
        ..backport_at(21, 20, ago(2))
    };
    let standalone = PullRequestFacts {
        title: "[3.13] Fix it (GH-998)".to_owned(),
        ..backport_at(30, 998, ago(1))
    };
    world.has(
        vec![pr_at(20, None, ago(4)), orphan, beneath, standalone],
        vec![issue_at(1, ago(3))],
    );

    let found = searched(&world, "fix");
    world.finish();

    assert_eq!(
        outline(&found),
        [
            ("standalone backport", 30, vec![]),
            ("standalone", 20, vec![21]),
            ("issue", 1, vec![10])
        ],
        "a Backport whose Origin the cache lacks stays under its Issue, a matching Backport keeps \
         its Standalone PR's header, and an unmatched Origin leaves a Standalone Backport"
    );
}

#[test]
fn a_search_keeps_a_pr_on_its_matching_backport_and_drops_a_backport_it_does_not_find() {
    let world = ViewWorld::new("inbox-search-backports-beneath-a-pr");
    let mut top = pr_at(10, Some(1), ago(6));
    top.title = "gh-1: docs for it".to_owned();
    let kept = PullRequestFacts {
        title: "[3.14] docs for it (GH-10)".to_owned(),
        ..backport_at(11, 10, ago(4))
    };
    let dropped = PullRequestFacts {
        title: "[3.13] notes (GH-10)".to_owned(),
        ..backport_at(12, 10, ago(3))
    };
    // PR 20's own title doesn't say "docs"; only its Backport does, which keeps the tree.
    let quiet = pr_at(20, Some(1), ago(7));
    let beneath_quiet = PullRequestFacts {
        title: "[3.14] docs again (GH-20)".to_owned(),
        ..backport_at(21, 20, ago(5))
    };
    world.has(
        vec![top, kept, dropped, quiet, beneath_quiet],
        vec![issue_at(1, ago(8))],
    );

    let found = searched(&world, "docs");
    world.finish();

    assert_eq!(
        outline(&found),
        [("issue", 1, vec![10, 11, 20, 21])],
        "a matching top keeps its matching Backport and drops the other, and a matching Backport \
         keeps a top that doesn't match"
    );
}

#[test]
fn a_brought_back_closed_issue_is_open_to_a_search_as_it_is_to_the_inbox() {
    let world = ViewWorld::new("inbox-search-brought-back");
    let mut closed = issue_at(1, ago(60));
    closed.state = Some(IssueState::Closed);
    closed.title = "Fix the parser".to_owned();
    world.has(vec![pr_at(10, Some(1), ago(50))], vec![closed]);
    let store = &world.db.store;
    archived_on_closing(store, &[ItemKey::Issue { number: 1 }], ago(60));
    store
        .bring_back(
            &cpython(),
            Opened {
                issues: vec![1],
                openings: BTreeSet::new(),
            },
        )
        .unwrap();

    let found = searched(&world, "is:issue fix");
    world.finish();

    assert_eq!(
        outline(&found),
        [("issue", 1, vec![])],
        "the Inbox lists a closed Issue a PR brought back, so is:open finds it too"
    );
}

#[test]
fn a_search_keeps_the_state_the_inbox_judged_of_the_whole_tree() {
    let world = ViewWorld::new("inbox-search-archived");
    let mut archived = issue_at(1, ago(5));
    archived.title = "Parser crash".to_owned();
    let mut requested = pr_at(10, Some(1), ago(4));
    requested.title = "gh-1: Unrelated change".to_owned();
    requested.review_requests = vec![me()];
    world.has(vec![requested], vec![archived]);
    let store = &world.db.store;
    let archiving = Archiving {
        key: ItemKey::Issue { number: 1 },
        numbers: vec![1, 10],
        up_to: Some(ago(6)),
        put_away: PutAway::Now,
        findings: vec![],
    };
    store.archive(&cpython(), archiving, now()).unwrap();
    world.happened(10, &[(ActivityKind::ReviewRequested, ago(3))]);

    let found = searched(&world, "parser");
    world.finish();

    assert_eq!(outline(&found), [("issue", 1, vec![])]);
    assert_eq!(
        found.items[0].hidden, None,
        "the dropped PR still brings the archived Issue back, as the Inbox judged it"
    );
}

#[test]
fn a_search_keeps_show_muted_off_s_attention_over_the_rows_left() {
    let world = ViewWorld::new("inbox-search-muted-attention");
    let muted = IssueFacts {
        author: Some(Login::new("muted-author")),
        title: "Parser header".to_owned(),
        ..issue_at(1, ago(5))
    };
    let mut asked = by("some-author", pr_at(10, Some(1), ago(2)));
    asked.title = "gh-1: Fix parser".to_owned();
    asked.review_requests = vec![me()];
    world.has(vec![asked], vec![muted]);
    world.mute_author_at("muted-author", ago(10));

    let found = searched(&world, "parser");
    world.finish();

    assert_eq!(outline(&found), [("issue", 1, vec![10])]);
    assert!(
        found.items[0].attention.is_some(),
        "the colleague's PR still Needs attention, as Show muted off judges it"
    );
}

#[test]
fn a_search_orders_a_standalone_pr_by_its_whole_tree_s_newest_row() {
    let world = ViewWorld::new("inbox-search-whole-tree-place");
    // PR 20's newest row is a Backport the search drops, and its matching Backport is older.
    let dropped = PullRequestFacts {
        title: "[3.14] Unrelated (GH-20)".to_owned(),
        ..backport_at(21, 20, ago(2))
    };
    let matched = PullRequestFacts {
        title: "[3.14] Fix it (GH-20)".to_owned(),
        ..backport_at(22, 20, ago(4))
    };
    let standalone = PullRequestFacts {
        title: "Fix it alone".to_owned(),
        ..pr_at(30, None, ago(3))
    };
    world.has(
        vec![pr_at(20, None, ago(5)), dropped, matched, standalone],
        vec![],
    );

    let found = searched(&world, "fix");
    world.finish();

    assert_eq!(
        outline(&found),
        [("standalone", 20, vec![22]), ("standalone", 30, vec![])],
        "PR 20 keeps the place its newest row gives it, a dropped Backport, above PR 30"
    );
}

#[test]
fn a_search_places_a_brought_back_closed_issue_at_its_prs_latest_update() {
    let world = ViewWorld::new("inbox-search-brought-back-place");
    let mut closed = issue_at(1, ago(1));
    closed.state = Some(IssueState::Closed);
    closed.title = "Fix the parser".to_owned();
    let mut unrelated = pr_at(10, Some(1), ago(5));
    unrelated.title = "gh-1: Unrelated change".to_owned();
    let standalone = PullRequestFacts {
        title: "Fix it alone".to_owned(),
        ..pr_at(20, None, ago(3))
    };
    world.has(vec![unrelated, standalone], vec![closed]);
    let store = &world.db.store;
    archived_on_closing(store, &[ItemKey::Issue { number: 1 }], ago(2));
    store
        .bring_back(
            &cpython(),
            Opened {
                issues: vec![1],
                openings: BTreeSet::new(),
            },
        )
        .unwrap();

    let found = searched(&world, "fix");
    world.finish();

    assert_eq!(
        outline(&found),
        [("standalone", 20, vec![]), ("issue", 1, vec![])],
        "the closed Issue stands at its PR's update, not its own newer one, as the Inbox places it"
    );
}

#[test]
fn a_search_with_show_muted_on_lists_a_muted_authors_matching_row() {
    let world = ViewWorld::new("inbox-search-show-muted-on");
    let mut other = pr_at(10, Some(1), ago(2));
    other.title = "gh-1: Other change".to_owned();
    let mut matched = by("muted-author", pr_at(11, Some(1), ago(1)));
    matched.title = "gh-1: Fix it".to_owned();
    world.has(vec![other, matched], vec![issue_at(1, ago(3))]);
    world.mute_author("muted-author");
    world.show_muted(ShowMuted::On);

    let found = searched(&world, "fix");
    world.finish();

    assert_eq!(outline(&found), [("issue", 1, vec![11])]);
}

#[test]
fn a_search_drops_an_item_whose_rows_show_muted_hides() {
    let world = ViewWorld::new("inbox-search-narrowed-away");
    let muted = IssueFacts {
        author: Some(Login::new("muted-one")),
        title: "Fix header".to_owned(),
        ..issue_at(1, ago(5))
    };
    let mut other = by("some-author", pr_at(11, Some(1), ago(3)));
    other.title = "gh-1: Other change".to_owned();
    world.has(vec![other], vec![muted]);
    world.mute_author("muted-one");

    let found = searched(&world, "fix");
    world.finish();

    assert_eq!(
        outline(&found),
        [],
        "the Issue's row is the only match, and Show muted hides it"
    );
}

#[test]
fn a_qualifier_the_cache_does_not_read_lists_nothing_and_says_github() {
    let world = ViewWorld::new("inbox-search-github");
    world.has(vec![pr_at(10, Some(1), ago(1))], vec![issue_at(1, ago(2))]);

    let found = searched(&world, "review:approved");
    let local = searched(&world, "fix");
    world.finish();

    assert_eq!(outline(&found), []);
    assert!(found.needs_github);
    assert_eq!(found.older, None);
    assert!(!local.needs_github);
}

#[test]
fn a_search_leaves_pinned_archived_and_muted_items_out() {
    let world = ViewWorld::new("inbox-search-hidden");
    world.has(
        vec![],
        vec![
            issue_at(1, ago(1)),
            issue_at(2, ago(1)),
            issue_at(3, ago(1)),
        ],
    );
    let store = &world.db.store;
    store.pin(&cpython(), 1).unwrap();
    let archiving = Archiving {
        key: ItemKey::Issue { number: 2 },
        numbers: vec![2],
        up_to: Some(ago(0)),
        put_away: PutAway::Now,
        findings: vec![],
    };
    store.archive(&cpython(), archiving, now()).unwrap();
    world.mute_issue(3, vec![3]);

    let found = searched(&world, "Issue");
    world.finish();

    assert_eq!(
        outline(&found),
        [],
        "a search leaves out what the Inbox does"
    );
}

#[test]
fn an_empty_box_reads_the_pages_as_the_inbox_does() {
    let world = ViewWorld::new("inbox-search-empty");
    world.has(
        vec![pr_at(10, Some(1), ago(60))],
        vec![issue_at(1, ago(70)), issue_at(2, ago(1))],
    );

    let normal = read(&world, InboxPage::Latest);
    let found = searched(&world, "  ");
    world.finish();

    assert_eq!(outline(&found), outline(&normal));
    assert!(!found.needs_github);
    assert_eq!(found.older, normal.older);
}

/// A result GitHub returned.
fn found(number: u32, kind: RowKind, updated_at: OffsetDateTime) -> SearchHit {
    SearchHit {
        number,
        kind,
        updated_at,
    }
}

/// Keeps GitHub's `answer` to the box's `text` for the world's Repository.
fn keep_answer(world: &ViewWorld, text: &str, answer: InboxSearch) {
    world
        .db
        .store
        .save_inbox_search(&cpython(), text, answer)
        .unwrap();
}

#[test]
fn an_answer_lists_the_cached_rows_it_returned_in_github_s_order_and_drops_the_others() {
    let world = ViewWorld::new("inbox-answer-order");
    world.has(
        vec![pr_at(10, Some(1), ago(2))],
        vec![
            issue_at(1, ago(5)),
            issue_at(2, ago(4)),
            issue_at(3, ago(3)),
        ],
    );
    // GitHub put Issue 3 first, then Issue 1 with PR 10 in its tree, and left Issue 2 out.
    keep_answer(
        &world,
        "review:approved",
        InboxSearch {
            hits: vec![
                found(3, RowKind::Issue, ago(3)),
                found(1, RowKind::Issue, ago(5)),
                found(10, RowKind::PullRequest, ago(2)),
            ],
            total: 3,
            cut_off: false,
        },
    );

    let listed = searched(&world, "review:approved");
    world.finish();

    assert_eq!(
        outline(&listed),
        [("issue", 3, vec![]), ("issue", 1, vec![10])],
        "GitHub's order decides, a matching PR stays under its Issue, and a cached row GitHub left \
         out is dropped"
    );
    assert!(!listed.needs_github);
    assert_eq!(listed.truncated, None);
}

#[test]
fn an_answer_puts_a_placeholder_where_the_cache_lacks_a_result() {
    let world = ViewWorld::new("inbox-answer-placeholder");
    world.has(vec![], vec![issue_at(1, ago(5))]);
    keep_answer(
        &world,
        "review:approved",
        InboxSearch {
            hits: vec![
                found(99, RowKind::PullRequest, ago(2)),
                found(1, RowKind::Issue, ago(5)),
            ],
            total: 2,
            cut_off: false,
        },
    );

    let listed = searched(&world, "review:approved");
    world.finish();

    assert_eq!(
        outline(&listed),
        [("placeholder", 99, vec![]), ("issue", 1, vec![])]
    );
    let WorkbenchItem::Placeholder(placeholder) = &listed.items[0].item else {
        panic!("the result the cache lacks shows as a placeholder");
    };
    assert_eq!(placeholder.item_kind, RowKind::PullRequest);
    assert_eq!(placeholder.updated_at, ago(2));
}

#[test]
fn an_answer_past_github_s_cap_says_how_many_it_showed_of_how_many() {
    let world = ViewWorld::new("inbox-answer-cut-off");
    world.has(vec![], vec![issue_at(1, ago(5))]);
    keep_answer(
        &world,
        "review:approved",
        InboxSearch {
            hits: vec![found(1, RowKind::Issue, ago(5))],
            total: 1712,
            cut_off: true,
        },
    );

    let listed = searched(&world, "review:approved");
    world.finish();

    assert_eq!(
        listed.truncated,
        Some(Truncated {
            shown: 1,
            total: 1712,
        })
    );
}

#[test]
fn a_query_github_has_answered_lists_its_rows_even_when_the_cache_cant_read_it() {
    let world = ViewWorld::new("inbox-answer-github-only");
    world.has(vec![], vec![issue_at(1, ago(5)), issue_at(2, ago(1))]);
    keep_answer(
        &world,
        "review:approved",
        InboxSearch {
            hits: vec![found(1, RowKind::Issue, ago(5))],
            total: 1,
            cut_off: false,
        },
    );

    let listed = searched(&world, "review:approved");
    world.finish();

    assert_eq!(outline(&listed), [("issue", 1, vec![])]);
    assert!(!listed.needs_github);
}

#[test]
fn the_answer_to_another_query_lists_nothing_of_it() {
    let world = ViewWorld::new("inbox-answer-stale");
    let mut fixes = issue_at(1, ago(5));
    fixes.title = "Fix the parser".to_owned();
    let mut other = issue_at(2, ago(1));
    other.title = "Something else".to_owned();
    world.has(vec![], vec![fixes, other]);
    keep_answer(
        &world,
        "review:approved",
        InboxSearch {
            hits: vec![found(2, RowKind::Issue, ago(1))],
            total: 1,
            cut_off: false,
        },
    );

    let listed = searched(&world, "fix");
    world.finish();

    assert_eq!(
        outline(&listed),
        [("issue", 1, vec![])],
        "the answer kept for another query is not the answer to this one, so the cache matches it"
    );
    assert!(!listed.needs_github);
}
