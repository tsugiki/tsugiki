use time::Duration;

use super::*;
use crate::store::Store;
use crate::store::migrate::MIGRATIONS;
use crate::store::testing::{database_before, temp_database};
use crate::testing::cpython;

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
    store.mark_unseen(&repository, 7, flagged).unwrap();
    store.mark_unseen(&repository, 8, flagged).unwrap();
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
    store.mark_unseen(&repository, 8, drawn).unwrap();
    let issue = ItemKey::Issue { number: 7 };
    let key = issue;
    store
        .archive(
            &repository,
            Archiving {
                key,
                numbers: vec![7, 8],
                up_to: Some(drawn),
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
    assert_eq!(
        restarted.archived,
        BTreeMap::from([(issue, drawn), (standalone, now)]),
        "with nothing drawn, it counts from now"
    );
    let both = BTreeMap::from([(7, drawn), (8, drawn)]);
    assert_eq!(restarted.last_seen, both);
    assert_eq!(restarted.acknowledged, both);
    assert_eq!(restarted.marked_unseen, BTreeMap::new());
    assert_eq!(after.archived, BTreeMap::from([(issue, drawn)]));
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
    assert_eq!(first, BTreeMap::from([(key, later)]));
    assert_eq!(
        again, first,
        "archiving again from an older mark moves nothing back"
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
            },
            now,
        )
        .unwrap();
    store.mark_unseen(&repository, 7, now).unwrap();
    let after = store.work_facts(&repository).unwrap().activity;

    store.close();
    assert_eq!(after.archived, BTreeMap::new());
    assert_eq!(after.marked_unseen, BTreeMap::from([(7, now)]));
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
