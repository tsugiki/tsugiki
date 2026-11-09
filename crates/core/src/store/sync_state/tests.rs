use super::*;
use crate::error::ErrorKind;
use crate::store::Store;
use crate::store::testing::temp_database;
use std::fs;

fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

fn at(seconds: i64, check: ChangeCheck) -> SyncState {
    let started_at = OffsetDateTime::from_unix_timestamp(seconds).unwrap();
    SyncState {
        started_at,
        finished_at: started_at + time::Duration::minutes(40),
        check,
        refresh_under_way: false,
    }
}

#[test]
fn a_repository_that_never_finished_a_sync_has_no_state() {
    let (path, folder) = temp_database("sync-state-none");
    let store = Store::open(&path).unwrap();

    let state = store.sync_state(&cpython()).unwrap();
    store.close();
    fs::remove_dir_all(folder).unwrap();

    assert_eq!(state, None);
}

#[test]
fn the_newest_state_survives_a_restart_whatever_the_case_of_the_name() {
    let (path, folder) = temp_database("sync-state-restart");
    let store = Store::open(&path).unwrap();
    store
        .save_sync_state(&cpython(), at(100, ChangeCheck::Skipped))
        .unwrap();
    let newest = SyncState {
        refresh_under_way: true,
        ..at(200, ChangeCheck::Tagged(Etag::new("W/\"abc\"")))
    };
    store.save_sync_state(&cpython(), newest.clone()).unwrap();
    store.close();

    let store = Store::open(&path).unwrap();
    let shouted = RepositoryName::parse("Python/CPython").unwrap();
    let state = store.sync_state(&shouted).unwrap();
    store.close();
    fs::remove_dir_all(folder).unwrap();

    assert_eq!(state, Some(newest));
}

#[test]
fn each_kind_of_check_reads_back_as_it_was_saved() {
    let (path, folder) = temp_database("sync-state-checks");
    let store = Store::open(&path).unwrap();
    let mut read = Vec::new();
    let checks = [
        ChangeCheck::Skipped,
        ChangeCheck::Untagged,
        ChangeCheck::Tagged(Etag::new("\"tag\"")),
    ];
    for check in checks.clone() {
        store.save_sync_state(&cpython(), at(1, check)).unwrap();
        read.push(store.sync_state(&cpython()).unwrap().unwrap().check);
    }
    store.close();
    fs::remove_dir_all(folder).unwrap();

    assert_eq!(read, checks);
}

#[test]
fn a_stopped_store_says_so() {
    let store = Store::stopped();

    let reading = store.sync_state(&cpython()).unwrap_err();
    let saving = store
        .save_sync_state(&cpython(), at(1, ChangeCheck::Skipped))
        .unwrap_err();

    assert_eq!(reading.kind, ErrorKind::Internal);
    assert_eq!(saving.kind, ErrorKind::Internal);
}
