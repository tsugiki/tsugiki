use time::Duration;

use super::*;
use crate::facts::activity::{ItemKey, PutAway};
use crate::facts::issues::IssueFacts;
use crate::facts::pull_requests::{PrState, PullRequestFacts};
use crate::store::testing::OpenStore;
use crate::store::{Archiving, Muting, Opened};
use crate::testing::{cpython, cpython_settings};
use crate::workbench::testing::{ViewWorld, backport, fixing, issue, mine, outline_of, stranger};

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
    let opened = Opened {
        issues: vec![1],
        openings: BTreeSet::new(),
    };
    world.db.store.bring_back(&cpython(), opened).unwrap();

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
}
