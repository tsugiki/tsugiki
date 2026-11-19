use time::Duration;

use super::*;
use crate::facts::issues::{IssueFacts, IssueState};
use crate::facts::pull_requests::{PrState, PullRequestFacts};
use crate::store::Store;
use crate::store::migrate::MIGRATIONS;
use crate::store::testing::{database_before, temp_database};
use crate::testing::{cpython, issue_facts, pr_facts};

#[test]
fn a_file_from_before_new_activity_counts_from_its_last_sync() {
    let (path, _folder) = temp_database("activity-migrates");
    let repository = cpython();
    let synced = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap() - Duration::days(1);
    crate::store::open_with(&path, MIGRATIONS.split_at(9).0)
        .unwrap()
        .close();
    // Our functions can't write a file of an older version.
    let raw = Connection::open(&path).unwrap();
    let inserted = raw
        .execute(
            "INSERT INTO sync_state
                     (repository, started_at, finished_at, refresh_under_way, small, etag)
                 VALUES (:repository, :at, :at, 0, 0, NULL)",
            named_params! { ":repository": repository, ":at": synced.unix_timestamp() },
        )
        .unwrap();
    assert_eq!(inserted, 1);
    drop(raw);

    Store::open(&path).unwrap().close();
    let reopened = Connection::open(&path).unwrap();
    let started = first_sync(&reopened, &repository).unwrap();
    drop(reopened);

    assert_eq!(started, Some(synced));
}

#[test]
fn marked_unseen_survives_a_restart_until_mark_seen_clears_it() {
    let (path, _folder) = temp_database("activity-marked-unseen");
    let repository = cpython();
    let flagged = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap();
    let store = Store::open(&path).unwrap();
    store
        .mark_unseen(&repository, Flagging::on(7), flagged)
        .unwrap();
    store
        .mark_unseen(&repository, Flagging::on(8), flagged)
        .unwrap();
    store.close();

    let store = Store::open(&path).unwrap();
    let restarted = store
        .work_facts(&repository)
        .unwrap()
        .activity
        .marked_unseen;
    store.mark_seen(&repository, vec![7], None).unwrap();
    let after = store.work_facts(&repository).unwrap().activity;

    store.close();
    assert_eq!(restarted, BTreeMap::from([(7, flagged), (8, flagged)]));
    assert_eq!(after.marked_unseen, BTreeMap::from([(8, flagged)]));
    assert_eq!(after.last_seen, BTreeMap::new(), "nothing new to see");
}

#[test]
fn a_last_seen_from_before_acknowledgements_acknowledges_nothing_until_mark_seen_moves_both() {
    let seen = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap() - Duration::days(2);
    let rows = format!(
        "INSERT INTO last_seen (repository, number, at) VALUES ('python/cpython', 7, {})",
        seen.unix_timestamp()
    );
    let (path, _folder) = database_before("activity-acknowledged", ACKNOWLEDGED, &rows);
    let repository = cpython();
    let store = Store::open(&path).unwrap();
    let migrated = store.work_facts(&repository).unwrap().activity;
    let later = seen + Duration::days(1);
    store
        .mark_seen(&repository, vec![7, 8], Some(later))
        .unwrap();
    store.mark_seen(&repository, vec![8], Some(seen)).unwrap();
    let after = store.work_facts(&repository).unwrap().activity;

    store.close();
    assert_eq!(migrated.last_seen, BTreeMap::from([(7, seen)]));
    assert_eq!(migrated.acknowledged, BTreeMap::new());
    let both = BTreeMap::from([(7, later), (8, later)]);
    assert_eq!(after.last_seen, both);
    assert_eq!(after.acknowledged, both, "neither moves back");
}

#[test]
fn an_archived_item_survives_a_restart_and_archive_marks_its_tree_seen() {
    let (path, _folder) = temp_database("activity-archived");
    let repository = cpython();
    let drawn = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap() - Duration::days(1);
    let now = drawn + Duration::hours(1);
    let store = Store::open(&path).unwrap();
    store
        .mark_unseen(&repository, Flagging::on(8), drawn)
        .unwrap();
    let issue = ItemKey::Issue { number: 7 };
    let key = issue;
    store
        .archive(
            &repository,
            Archiving {
                key,
                numbers: vec![7, 8],
                up_to: Some(drawn),
                put_away: PutAway::Now,
            },
            now,
        )
        .unwrap();
    let standalone = ItemKey::PullRequest { number: 9 };
    let key = standalone;
    store
        .archive(
            &repository,
            Archiving {
                key,
                numbers: vec![9],
                up_to: None,
                put_away: PutAway::Now,
            },
            now,
        )
        .unwrap();
    store.close();

    let store = Store::open(&path).unwrap();
    let restarted = store.work_facts(&repository).unwrap().activity;
    store.unarchive(&repository, standalone).unwrap();
    let after = store.work_facts(&repository).unwrap().activity;

    store.close();
    let archive = |seen_up_to| Archive {
        seen_up_to,
        archived_at: now,
    };
    assert_eq!(
        restarted.archived,
        BTreeMap::from([(issue, archive(drawn)), (standalone, archive(now))]),
        "with nothing drawn, it counts from now"
    );
    let both = BTreeMap::from([(7, drawn), (8, drawn)]);
    assert_eq!(restarted.last_seen, both);
    assert_eq!(restarted.acknowledged, both);
    assert_eq!(restarted.marked_unseen, BTreeMap::new());
    assert_eq!(after.archived, BTreeMap::from([(issue, archive(drawn))]));
    assert_eq!(after.last_seen, both, "unarchive leaves Last seen");
}

#[test]
fn archive_counts_from_the_latest_moment_its_tree_was_acknowledged_and_never_moves_back() {
    let (path, _folder) = temp_database("activity-archived-acknowledged");
    let repository = cpython();
    let drawn = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap() - Duration::days(1);
    let later = drawn + Duration::hours(1);
    let store = Store::open(&path).unwrap();
    // PR 8 also fixes another Issue, which the user marked seen up to a later sync.
    store.mark_seen(&repository, vec![8], Some(later)).unwrap();
    let key = ItemKey::Issue { number: 7 };
    let archiving = |numbers| Archiving {
        key,
        numbers,
        up_to: Some(drawn),
        put_away: PutAway::Now,
    };
    store
        .archive(&repository, archiving(vec![7, 8]), drawn)
        .unwrap();
    let first = store.work_facts(&repository).unwrap().activity.archived;
    // A mark that covers part of the tree, drawn before the PR's later moment.
    store
        .archive(&repository, archiving(vec![7]), drawn)
        .unwrap();
    let again = store.work_facts(&repository).unwrap().activity.archived;

    store.close();
    let archive = Archive {
        seen_up_to: later,
        archived_at: drawn,
    };
    assert_eq!(first, BTreeMap::from([(key, archive)]));
    assert_eq!(
        again, first,
        "archiving again from an older mark moves nothing back"
    );
}

#[test]
fn archive_puts_the_item_away_now_and_mark_seen_on_an_archived_item_keeps_when() {
    let (path, _folder) = temp_database("activity-archived-at");
    let repository = cpython();
    let first = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap() - Duration::days(1);
    let (second, third) = (first + Duration::hours(1), first + Duration::hours(2));
    let store = Store::open(&path).unwrap();
    let archive = |number, put_away, now| {
        let archiving = Archiving {
            key: ItemKey::Issue { number },
            numbers: vec![number],
            up_to: Some(first),
            put_away,
        };
        store.archive(&repository, archiving, now).unwrap();
        let archived = store.work_facts(&repository).unwrap().activity.archived;
        archived[&ItemKey::Issue { number }].archived_at
    };

    let kept = (
        archive(7, PutAway::Now, first),
        archive(7, PutAway::AsBefore, second),
    );
    let moved = archive(7, PutAway::Now, third);
    let fresh = archive(8, PutAway::AsBefore, second);

    store.close();
    assert_eq!(
        kept,
        (first, first),
        "Mark Seen on it keeps when it was put away"
    );
    assert_eq!(moved, third, "archiving it again puts it away now");
    assert_eq!(
        fresh, second,
        "an item with no archive to keep is put away now"
    );
}

#[test]
fn a_file_from_before_archived_at_puts_each_item_away_at_its_mark() {
    let drawn = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap() - Duration::days(1);
    let rows = format!(
        "INSERT INTO archived (repository, kind, number, at) VALUES ('python/cpython', 'issue', 7, {})",
        drawn.unix_timestamp()
    );
    let (path, _folder) = database_before("activity-archived-at-migrates", ARCHIVED_AT, &rows);
    let repository = cpython();

    let store = Store::open(&path).unwrap();
    let archived = store.work_facts(&repository).unwrap().activity.archived;

    store.close();
    let archive = Archive {
        seen_up_to: drawn,
        archived_at: drawn,
    };
    assert_eq!(
        archived,
        BTreeMap::from([(ItemKey::Issue { number: 7 }, archive)])
    );
}

#[test]
fn mark_unseen_unarchives_the_item() {
    let (path, _folder) = temp_database("activity-unseen-unarchives");
    let repository = cpython();
    let now = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap();
    let store = Store::open(&path).unwrap();
    let key = ItemKey::Issue { number: 7 };
    store
        .archive(
            &repository,
            Archiving {
                key,
                numbers: vec![7],
                up_to: Some(now),
                put_away: PutAway::Now,
            },
            now,
        )
        .unwrap();
    store
        .mark_unseen(&repository, Flagging::on(7), now)
        .unwrap();
    let after = store.work_facts(&repository).unwrap().activity;

    store.close();
    assert_eq!(after.archived, BTreeMap::new());
    assert_eq!(after.marked_unseen, BTreeMap::from([(7, now)]));
}

/// Issue 1 and PR 2, each in `state`, as of `updated_at`.
fn issue_and_pr(
    (issue, pr): (IssueState, PrState),
    updated_at: OffsetDateTime,
) -> (IssueFacts, PullRequestFacts) {
    let issue = IssueFacts {
        state: Some(issue),
        updated_at,
        ..issue_facts(1)
    };
    let pull_request = PullRequestFacts {
        state: pr,
        updated_at,
        ..pr_facts(2)
    };
    (issue, pull_request)
}

/// Saves `issue` and `pull_request` into `store`.
fn save_both(store: &Store, (issue, pull_request): (IssueFacts, PullRequestFacts)) {
    store.save_issues(&cpython(), vec![issue]).unwrap();
    (store.save_pull_requests(&cpython(), vec![pull_request])).unwrap();
}

#[test]
fn a_save_that_closes_an_open_item_keeps_its_closing_until_the_sync_judges_it() {
    let (path, _folder) = temp_database("activity-closings");
    let repository = cpython();
    let now = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap();
    let store = Store::open(&path).unwrap();
    // PR 3 comes closed, and was never open in the store.
    let closed = PullRequestFacts {
        state: PrState::Closed,
        ..pr_facts(3)
    };
    store.save_pull_requests(&repository, vec![closed]).unwrap();
    save_both(&store, issue_and_pr((IssueState::Open, PrState::Open), now));
    save_both(
        &store,
        issue_and_pr((IssueState::Closed, PrState::Merged), now),
    );
    let closings = store.closings(&repository).unwrap();
    let judged = Judged {
        archived: vec![ItemKey::Issue { number: 1 }],
        closings: BTreeSet::from([1, 2]),
    };
    store.archive_closed(&repository, judged, now).unwrap();
    let after = (store.work_facts(&repository).unwrap().activity).archived;
    let judged = store.closings(&repository).unwrap();

    store.close();
    assert_eq!(closings, BTreeSet::from([1, 2]));
    let archive = Archive {
        seen_up_to: now,
        archived_at: now,
    };
    assert_eq!(
        after,
        BTreeMap::from([(ItemKey::Issue { number: 1 }, archive)])
    );
    assert_eq!(judged, BTreeSet::new());
}

#[test]
fn an_older_read_closes_nothing() {
    let (path, _folder) = temp_database("activity-closings-older");
    let repository = cpython();
    let now = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap();
    let store = Store::open(&path).unwrap();
    let (_, open) = issue_and_pr((IssueState::Open, PrState::Open), now);
    let (_, older) = issue_and_pr(
        (IssueState::Open, PrState::Merged),
        now - Duration::hours(1),
    );

    store.save_pull_requests(&repository, vec![open]).unwrap();
    store.save_pull_requests(&repository, vec![older]).unwrap();
    let closings = store.closings(&repository).unwrap();

    store.close();
    assert_eq!(closings, BTreeSet::new());
}

#[test]
fn an_item_that_reopens_leaves_archived_and_its_closing_goes() {
    let (path, _folder) = temp_database("activity-reopens");
    let repository = cpython();
    let now = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap();
    let store = Store::open(&path).unwrap();
    save_both(
        &store,
        issue_and_pr((IssueState::Closed, PrState::Closed), now),
    );
    let judged = Judged {
        archived: vec![
            ItemKey::Issue { number: 1 },
            ItemKey::PullRequest { number: 2 },
        ],
        closings: BTreeSet::new(),
    };
    store.archive_closed(&repository, judged, now).unwrap();
    // A closing of Issue 1 the sync never judged.
    save_both(
        &store,
        issue_and_pr((IssueState::Open, PrState::Closed), now),
    );
    save_both(
        &store,
        issue_and_pr((IssueState::Closed, PrState::Closed), now),
    );

    save_both(&store, issue_and_pr((IssueState::Open, PrState::Open), now));
    let after = (store.work_facts(&repository).unwrap().activity).archived;
    let closings = store.closings(&repository).unwrap();

    store.close();
    assert_eq!(after, BTreeMap::new());
    assert_eq!(closings, BTreeSet::new());
}

#[test]
fn mark_unseen_on_a_pr_unarchives_the_archived_item_that_holds_it() {
    let (path, _folder) = temp_database("activity-unseen-unarchives-holder");
    let repository = cpython();
    let now = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap();
    let store = Store::open(&path).unwrap();
    let key = ItemKey::Issue { number: 7 };
    let archiving = Archiving {
        key,
        numbers: vec![7, 8],
        up_to: Some(now),
        put_away: PutAway::Now,
    };
    store.archive(&repository, archiving, now).unwrap();
    let flagging = Flagging {
        number: 8,
        archived: Some(key),
    };

    store.mark_unseen(&repository, flagging, now).unwrap();
    let after = store.work_facts(&repository).unwrap().activity;

    store.close();
    assert_eq!(after.archived, BTreeMap::new());
    assert_eq!(after.marked_unseen, BTreeMap::from([(8, now)]));
}

#[test]
fn a_kind_from_a_later_version_is_left_alone() {
    let (path, _folder) = temp_database("activity-later-kind");
    let repository = cpython();
    let store = Store::open(&path).unwrap();
    // This version has no way to write a kind it doesn't know.
    let raw = Connection::open(&path).unwrap();
    let inserted = raw
        .execute(
            "INSERT INTO activity (repository, number, kind, target, who, at, noticed_at)
                 VALUES (:repository, 1, 'later', '', 'some-author', 0, 0)",
            named_params! { ":repository": repository },
        )
        .unwrap();
    assert_eq!(inserted, 1);
    let author = Login::new("some-author");
    let listed = listed_events(&raw, &repository, (None, Some(&author)), Items::All).unwrap();
    drop(raw);

    let read = store.activity(&repository, 1).unwrap();

    store.close();
    assert_eq!(read, []);
    assert_eq!(listed, BTreeMap::new());
}

#[test]
fn an_archived_kind_from_a_later_version_is_left_alone() {
    let (path, _folder) = temp_database("activity-later-archived");
    let repository = cpython();
    Store::open(&path).unwrap().close();
    // This version has no way to write a kind it doesn't know.
    let raw = Connection::open(&path).unwrap();
    let inserted = raw
        .execute(
            "INSERT INTO archived (repository, kind, number, at)
                 VALUES (:repository, 'dependency_updates', 0, 0)",
            named_params! { ":repository": repository },
        )
        .unwrap();
    assert_eq!(inserted, 1);
    let read = archived(&raw, &repository, Items::All).unwrap();
    drop(raw);

    assert_eq!(read, BTreeMap::new());
}

#[test]
fn a_muted_item_survives_a_restart_and_mute_marks_its_tree_seen() {
    let (path, _folder) = temp_database("activity-muted");
    let repository = cpython();
    let drawn = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap() - Duration::days(1);
    let later = drawn + Duration::hours(1);
    let now = drawn + Duration::hours(2);
    let store = Store::open(&path).unwrap();
    store
        .mark_unseen(&repository, Flagging::on(7), drawn)
        .unwrap();
    // PR 8 also fixes another Issue, which the user marked seen up to a later sync.
    store.mark_seen(&repository, vec![8], Some(later)).unwrap();
    let issue = ItemKey::Issue { number: 7 };
    let (key, numbers, up_to) = (issue, vec![7, 8], Some(drawn));
    store
        .mute(
            &repository,
            Muting {
                key,
                numbers,
                up_to,
            },
            now,
        )
        .unwrap();
    let standalone = ItemKey::PullRequest { number: 9 };
    let (key, numbers, up_to) = (standalone, vec![9], None);
    store
        .mute(
            &repository,
            Muting {
                key,
                numbers,
                up_to,
            },
            now,
        )
        .unwrap();
    store.close();

    let store = Store::open(&path).unwrap();
    let restarted = store.work_facts(&repository).unwrap().activity;
    store.unmute(&repository, standalone).unwrap();
    let after = store.work_facts(&repository).unwrap().activity;

    store.close();
    assert_eq!(
        restarted.muted,
        BTreeMap::from([(issue, later), (standalone, now)]),
        "from the latest moment its tree was acknowledged, and with nothing drawn, from now"
    );
    assert_eq!(
        restarted.last_seen,
        BTreeMap::from([(7, drawn), (8, later)])
    );
    assert_eq!(restarted.marked_unseen, BTreeMap::new());
    assert_eq!(after.muted, BTreeMap::from([(issue, later)]));
    assert_eq!(
        after.last_seen, restarted.last_seen,
        "unmute leaves Last seen"
    );
}

#[test]
fn a_muted_kind_from_a_later_version_is_left_alone() {
    let (path, _folder) = temp_database("activity-later-muted");
    let repository = cpython();
    Store::open(&path).unwrap().close();
    // This version has no way to write a kind it doesn't know.
    let raw = Connection::open(&path).unwrap();
    let inserted = raw
        .execute(
            "INSERT INTO muted (repository, kind, number, at)
                 VALUES (:repository, 'dependency_updates', 0, 0)",
            named_params! { ":repository": repository },
        )
        .unwrap();
    assert_eq!(inserted, 1);
    let read = muted(&raw, &repository, Items::All).unwrap();
    drop(raw);

    assert_eq!(read, BTreeMap::new());
}

/// Closed Issues `numbers`, Archived as they closed `now`.
fn archived_closed(store: &Store, numbers: &[u32], now: OffsetDateTime) {
    let closed = |number| IssueFacts {
        state: Some(IssueState::Closed),
        ..issue_facts(number)
    };
    let issues = numbers.iter().copied().map(closed).collect();
    store.save_issues(&cpython(), issues).unwrap();
    let judged = Judged {
        archived: numbers
            .iter()
            .map(|number| ItemKey::Issue { number: *number })
            .collect(),
        closings: BTreeSet::new(),
    };
    store.archive_closed(&cpython(), judged, now).unwrap();
}

/// Open PR `number` with `title`, as of `updated_at`.
fn titled(number: u32, title: &str, updated_at: OffsetDateTime) -> PullRequestFacts {
    PullRequestFacts {
        title: title.to_owned(),
        updated_at,
        ..pr_facts(number)
    }
}

#[test]
fn a_pr_saved_for_the_first_time_is_an_opening_when_it_names_an_archived_issue() {
    let (path, _folder) = temp_database("activity-openings");
    let repository = cpython();
    let now = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap();
    let store = Store::open(&path).unwrap();
    let held = titled(12, "gh-1: Fix it", now - Duration::hours(1));
    store.save_pull_requests(&repository, vec![held]).unwrap();
    archived_closed(&store, &[1], now);
    store
        .save_issues(&repository, vec![issue_facts(2)])
        .unwrap();

    let saved = vec![
        titled(10, "gh-2: Fix that", now),
        titled(11, "gh-1: Follow up", now),
        titled(12, "gh-1: Fix it", now),
    ];
    store.save_pull_requests(&repository, saved).unwrap();
    let openings = store.openings(&repository).unwrap();

    store.close();
    // Issue 2 isn't Archived, and the store held PR 12 before.
    assert_eq!(openings, BTreeSet::from([11]));
}

#[test]
fn an_issue_a_pr_brought_back_stays_back_until_archive_mute_or_a_state_move() {
    let (path, _folder) = temp_database("activity-brought-back");
    let repository = cpython();
    let now = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap();
    let store = Store::open(&path).unwrap();
    archived_closed(&store, &[1, 2, 3, 4], now);
    store
        .save_pull_requests(&repository, vec![titled(11, "gh-1, gh-2: Follow up", now)])
        .unwrap();
    let opened = Opened {
        issues: vec![1, 2, 3, 4],
        openings: BTreeSet::from([11]),
    };
    let read_back = |store: &Store| {
        let repository = cpython();
        let read = store.call(move |connection| brought_back(connection, &repository));
        read.unwrap().unwrap()
    };

    store.bring_back(&repository, opened).unwrap();
    let back = read_back(&store);
    let archived = (store.work_facts(&repository).unwrap().activity).archived;
    let openings = store.openings(&repository).unwrap();
    let archiving = Archiving {
        key: ItemKey::Issue { number: 1 },
        numbers: vec![1],
        up_to: Some(now),
        put_away: PutAway::Now,
    };
    store.archive(&repository, archiving, now).unwrap();
    let muting = Muting {
        key: ItemKey::Issue { number: 2 },
        numbers: vec![2],
        up_to: Some(now),
    };
    store.mute(&repository, muting, now).unwrap();
    let reopened = IssueFacts {
        state: Some(IssueState::Open),
        updated_at: now,
        ..issue_facts(3)
    };
    store.save_issues(&repository, vec![reopened]).unwrap();
    let after = read_back(&store);

    store.close();
    assert_eq!(back, BTreeSet::from([1, 2, 3, 4]));
    assert_eq!(archived, BTreeMap::new());
    assert_eq!(openings, BTreeSet::new());
    assert_eq!(after, BTreeSet::from([4]));
}

#[test]
fn an_open_issue_a_pr_brought_back_leaves_the_mark_when_it_closes() {
    let (path, _folder) = temp_database("activity-brought-back-closes");
    let repository = cpython();
    let now = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap();
    let store = Store::open(&path).unwrap();
    store
        .save_issues(&repository, vec![issue_facts(1)])
        .unwrap();
    let opened = Opened {
        issues: vec![1],
        openings: BTreeSet::new(),
    };
    store.bring_back(&repository, opened).unwrap();
    let closed = IssueFacts {
        state: Some(IssueState::Closed),
        updated_at: now,
        ..issue_facts(1)
    };

    store.save_issues(&repository, vec![closed]).unwrap();
    let read = store.call(move |connection| brought_back(connection, &cpython()));

    store.close();
    // The closing archives the Issue or leaves it out of the Inbox, as for any Issue that closes.
    assert_eq!(read.unwrap().unwrap(), BTreeSet::new());
}
