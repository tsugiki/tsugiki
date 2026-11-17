use super::*;
use crate::error::ErrorKind;
use crate::facts::issue_content::IssueContent;
use crate::facts::pull_request_content::PullRequestContent;
use crate::store::Store;
use crate::store::testing::temp_database;
use crate::testing::cpython;

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
    let (path, _folder) = temp_database("sync-state-none");
    let store = Store::open(&path).unwrap();

    let state = store.sync_state(&cpython()).unwrap();
    store.close();

    assert_eq!(state, None);
}

#[test]
fn the_newest_state_survives_a_restart_whatever_the_case_of_the_name() {
    let (path, _folder) = temp_database("sync-state-restart");
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

    assert_eq!(state, Some(newest));
}

#[test]
fn each_kind_of_check_reads_back_as_it_was_saved() {
    let (path, _folder) = temp_database("sync-state-checks");
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

    assert_eq!(read, checks);
}

#[test]
fn the_reads_a_refresh_owes_last_until_a_sync_finishes() {
    let (path, _folder) = temp_database("sync-state-read-ahead");
    let store = Store::open(&path).unwrap();
    let under_way = SyncState {
        refresh_under_way: true,
        ..at(100, ChangeCheck::Skipped)
    };
    store
        .save_sync_state(&cpython(), under_way.clone())
        .unwrap();
    store
        .save_changed_pull_requests(&cpython(), (vec![], vec![]), vec![3, 2, 1])
        .unwrap();
    let shouted = RepositoryName::parse("Python/CPython").unwrap();
    store
        .save_changed_pull_requests(&shouted, (vec![], vec![]), vec![3])
        .unwrap();
    store.save_sync_state(&cpython(), under_way).unwrap();
    store
        .save_pull_request_content(&cpython(), 2, no_conversation())
        .unwrap();
    store.close();

    let store = Store::open(&path).unwrap();
    let owed = store.read_ahead_owed(&cpython()).unwrap();
    store
        .save_sync_state(&cpython(), at(200, ChangeCheck::Skipped))
        .unwrap();
    let after = store.read_ahead_owed(&cpython()).unwrap();
    store.close();

    assert_eq!(owed, BTreeSet::from([1, 3]), "PR 2's Conversation was read");
    assert_eq!(after, BTreeSet::new(), "a finished sync owes nothing");
}

fn no_conversation() -> PullRequestContent {
    PullRequestContent {
        conversation: IssueContent {
            body_html: String::new(),
            comments: vec![],
            earlier_comments: 0,
            opened_at: None,
        },
        reviews: vec![],
        earlier_reviews: 0,
        head: None,
        pending: vec![],
    }
}

#[test]
fn a_pr_is_owed_a_read_at_its_head_unless_its_conversation_was_read_there() {
    let (path, _root) = temp_database("owed-at-head");
    let store = Store::open(&path).unwrap();
    let head = |text: &str| CommitSha::parse(text);
    let read_at = |text: &str| PullRequestContent {
        head: head(text),
        ..no_conversation()
    };
    store
        .save_pull_request_content(&cpython(), 2, read_at("0123abc"))
        .unwrap();
    store
        .save_pull_request_content(&cpython(), 3, read_at("0123abc"))
        .unwrap();
    store
        .save_pull_request_content(&cpython(), 4, no_conversation())
        .unwrap();

    let heads = vec![
        (1, head("4567def")),
        (2, head("0123abc")),
        (3, head("89abcde")),
        (4, head("0123abc")),
        (5, None),
    ];
    store.owe_read_ahead_at(&cpython(), heads.clone()).unwrap();
    store.owe_read_ahead_at(&cpython(), heads).unwrap();
    let owed = store.read_ahead_owed(&cpython()).unwrap();
    store.close();

    assert_eq!(owed, BTreeSet::from([1, 3, 4, 5]));
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
