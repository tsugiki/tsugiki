use time::Duration;

use super::*;
use crate::store::Store;
use crate::store::migrate::MIGRATIONS;
use crate::store::testing::temp_database;

#[test]
fn a_file_from_before_new_activity_counts_from_its_last_sync() {
    let (path, folder) = temp_database("activity-migrates");
    let repository = RepositoryName::parse("python/cpython").unwrap();
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

    std::fs::remove_dir_all(folder).unwrap();
    assert_eq!(started, Some(synced));
}

#[test]
fn marked_unseen_survives_a_restart_until_mark_seen_clears_it() {
    let (path, folder) = temp_database("activity-marked-unseen");
    let repository = RepositoryName::parse("python/cpython").unwrap();
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
    std::fs::remove_dir_all(folder).unwrap();
    assert_eq!(restarted, BTreeMap::from([(7, flagged), (8, flagged)]));
    assert_eq!(after.marked_unseen, BTreeMap::from([(8, flagged)]));
    assert_eq!(after.last_seen, BTreeMap::new(), "nothing new to see");
}

#[test]
fn a_kind_from_a_later_version_is_left_alone() {
    let (path, folder) = temp_database("activity-later-kind");
    let repository = RepositoryName::parse("python/cpython").unwrap();
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
    let listed = listed_events(&raw, &repository, None, Some(&author)).unwrap();
    drop(raw);

    let read = store.activity(&repository, 1).unwrap();

    store.close();
    std::fs::remove_dir_all(folder).unwrap();
    assert_eq!(read, []);
    assert_eq!(listed, BTreeMap::new());
}
