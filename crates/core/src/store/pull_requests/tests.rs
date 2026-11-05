use super::super::testing::temp_database;
use super::super::*;
use crate::error::ErrorKind;
use crate::github::CommitSha;
use crate::github::pull_requests::{CiState, LatestReview, OffDefault, PrState, ReviewState};
use crate::repository::RepositoryName;
use std::fs;
use std::path::PathBuf;
use time::OffsetDateTime;

fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

fn facts(number: u32) -> PullRequestFacts {
    PullRequestFacts {
        number,
        title: format!("gh-{number}: Fix it"),
        state: PrState::Open,
        is_draft: false,
        author: Some(Login::new("some-author")),
        target_branch: "main".to_owned(),
        ci: Some(CiState::Success),
        updated_at: OffsetDateTime::from_unix_timestamp(1).unwrap(),
        review_requests: vec![Login::new("Octocat")],
        latest_reviews: vec![LatestReview {
            author: Login::new("some-reviewer"),
            state: ReviewState::Approved,
        }],
        assignees: vec![Login::new("some-assignee")],
        closing_issues: vec![7, 8],
        merge_commit: None,
        off_default: Some(OffDefault::described("Fixes #9")),
    }
}

fn at(seconds: i64, facts: PullRequestFacts) -> PullRequestFacts {
    PullRequestFacts {
        updated_at: OffsetDateTime::from_unix_timestamp(seconds).unwrap(),
        ..facts
    }
}

/// A store and a reader of the same file.
fn opened(name: &str) -> (Store, OpenReader, StoreReader, PathBuf, PathBuf) {
    let (path, folder) = temp_database(name);
    let store = Store::open(&path).unwrap();
    let (to_open, reader) = reader();
    let open = to_open.open(&path).unwrap();
    (store, open, reader, path, folder)
}

fn numbers(reader: &StoreReader, repository: &RepositoryName) -> Vec<u32> {
    let facts = reader.work_facts(repository).unwrap();
    facts.pull_requests.iter().map(|pr| pr.number).collect()
}

#[test]
fn what_the_writer_saves_the_reader_reads() {
    let (store, open, reader, _path, folder) = opened("pr-roundtrip");
    store.remember_login(&Login::new("Octocat")).unwrap();

    store
        .save_pull_requests(&cpython(), vec![at(1, facts(1))])
        .unwrap();
    let read = reader.work_facts(&cpython()).unwrap();
    open.close();
    store.close();
    fs::remove_dir_all(folder).unwrap();

    assert_eq!(read.viewer, Some(Login::new("octocat")));
    assert_eq!(read.pull_requests, [facts(1)]);
}

#[test]
fn a_pr_with_nothing_optional_reads_back_the_same() {
    let (store, open, reader, _path, folder) = opened("pr-sparse");
    let sparse = PullRequestFacts {
        author: None,
        ci: None,
        is_draft: true,
        review_requests: vec![],
        latest_reviews: vec![],
        assignees: vec![],
        closing_issues: vec![],
        merge_commit: None,
        off_default: None,
        ..facts(2)
    };

    store
        .save_pull_requests(&cpython(), vec![at(1, sparse.clone())])
        .unwrap();
    let read = reader.work_facts(&cpython()).unwrap();
    open.close();
    store.close();
    fs::remove_dir_all(folder).unwrap();

    assert_eq!(read.pull_requests, [sparse]);
}

fn sha(text: &str) -> CommitSha {
    CommitSha::parse(text).unwrap()
}

#[test]
fn a_backports_facts_read_back_the_same() {
    let (store, open, reader, _path, folder) = opened("pr-backport");
    let backport = PullRequestFacts {
        off_default: Some(OffDefault {
            description: "(cherry picked from commit 0123abc)".to_owned(),
            cherry_picked_from: vec![sha("0123abc")],
            committers: vec![Some(Login::new("miss-islington")), None],
        }),
        ..facts(5)
    };
    let origin = PullRequestFacts {
        state: PrState::Merged,
        merge_commit: Some(sha("0123abcdef")),
        off_default: None,
        ..facts(6)
    };

    store
        .save_pull_requests(
            &cpython(),
            vec![at(2, backport.clone()), at(1, origin.clone())],
        )
        .unwrap();
    let read = reader.work_facts(&cpython()).unwrap();
    open.close();
    store.close();
    fs::remove_dir_all(folder).unwrap();

    assert_eq!(read.pull_requests, [at(2, backport), at(1, origin)]);
}

#[test]
fn saving_a_backport_again_replaces_its_commits() {
    let (store, open, reader, _path, folder) = opened("pr-backport-again");
    let first = PullRequestFacts {
        off_default: Some(OffDefault {
            description: String::new(),
            cherry_picked_from: vec![sha("0123abc")],
            committers: vec![Some(Login::new("miss-islington")), None],
        }),
        ..facts(5)
    };
    let again = PullRequestFacts {
        off_default: Some(OffDefault {
            description: String::new(),
            cherry_picked_from: vec![],
            committers: vec![Some(Login::new("some-author"))],
        }),
        ..first.clone()
    };

    store.save_pull_requests(&cpython(), vec![first]).unwrap();
    store
        .save_pull_requests(&cpython(), vec![again.clone()])
        .unwrap();
    let read = reader.work_facts(&cpython()).unwrap();
    open.close();
    store.close();
    fs::remove_dir_all(folder).unwrap();

    assert_eq!(read.pull_requests, [again]);
}

#[test]
fn merged_and_closed_prs_are_frozen_and_open_ones_are_not() {
    let (store, open, _reader, _path, folder) = opened("pr-frozen");
    let in_state = |number, state| PullRequestFacts {
        state,
        ..facts(number)
    };
    let merged = |number| PullRequestFacts {
        merge_commit: Some(sha("0123abc")),
        ..in_state(number, PrState::Merged)
    };
    store
        .save_pull_requests(
            &cpython(),
            vec![
                in_state(1, PrState::Open),
                merged(2),
                in_state(3, PrState::Closed),
                // An earlier Tsugiki kept no merge commit, so this one has more to bring.
                in_state(5, PrState::Merged),
            ],
        )
        .unwrap();
    let elsewhere = RepositoryName::parse("sergey-miryanov/gcmon").unwrap();
    store
        .save_pull_requests(&elsewhere, vec![merged(4)])
        .unwrap();

    let frozen = store.frozen_pull_requests(&cpython()).unwrap();
    open.close();
    store.close();
    fs::remove_dir_all(folder).unwrap();

    let mut numbers: Vec<u32> = frozen.iter().map(|pr| pr.number).collect();
    numbers.sort_unstable();
    assert_eq!(numbers, [2, 3]);
}

#[test]
fn saving_a_pr_again_replaces_what_was_known() {
    let (store, open, reader, _path, folder) = opened("pr-replace");
    store
        .save_pull_requests(&cpython(), vec![at(1, facts(1))])
        .unwrap();
    let changed = PullRequestFacts {
        title: "gh-1: Fix it better".to_owned(),
        review_requests: vec![],
        latest_reviews: vec![],
        assignees: vec![],
        closing_issues: vec![],
        merge_commit: None,
        off_default: None,
        ..at(2, facts(1))
    };

    store
        .save_pull_requests(&cpython(), vec![changed.clone()])
        .unwrap();
    let read = reader.work_facts(&cpython()).unwrap();
    open.close();
    store.close();
    fs::remove_dir_all(folder).unwrap();

    assert_eq!(read.pull_requests, [changed]);
}

#[test]
fn every_pr_is_read_whatever_its_state_and_the_latest_update_comes_first() {
    let (store, open, reader, _path, folder) = opened("pr-order");
    let merged = PullRequestFacts {
        state: PrState::Merged,
        ..facts(3)
    };
    store
        .save_pull_requests(
            &cpython(),
            vec![
                at(10, facts(1)),
                at(30, facts(2)),
                at(99, merged),
                at(20, facts(4)),
            ],
        )
        .unwrap();

    let found = numbers(&reader, &cpython());
    open.close();
    store.close();
    fs::remove_dir_all(folder).unwrap();

    assert_eq!(found, [3, 2, 4, 1]);
}

#[test]
fn repositories_keep_apart_and_their_names_ignore_case() {
    let (store, open, reader, _path, folder) = opened("pr-repositories");
    let peps = RepositoryName::parse("python/peps").unwrap();
    let shouting = RepositoryName::parse("Python/CPython").unwrap();
    store
        .save_pull_requests(&cpython(), vec![at(1, facts(1))])
        .unwrap();
    store
        .save_pull_requests(&peps, vec![at(1, facts(2))])
        .unwrap();

    let found = numbers(&reader, &shouting);
    open.close();
    store.close();
    fs::remove_dir_all(folder).unwrap();

    assert_eq!(found, [1]);
}

/// The rows of a query that counts them as `found`, on the writer's connection.
fn count(store: &Store, query: &'static str) -> u32 {
    store
        .call(move |connection| connection.query_row(query, [], |row| row.get("found")))
        .unwrap()
        .unwrap()
}

#[test]
fn a_saved_pr_holds_the_same_text_in_the_same_columns() {
    let (store, open, _reader, _path, folder) = opened("pr-columns");

    store
        .save_pull_requests(&cpython(), vec![at(7, facts(1))])
        .unwrap();
    let found = [
            count(
                &store,
                "SELECT count(*) AS found FROM pull_requests
                 WHERE repository = 'python/cpython' COLLATE BINARY AND number = 1
                   AND title = 'gh-1: Fix it' AND state = 'OPEN' AND is_draft = 0
                   AND author = 'some-author' AND target_branch = 'main' AND ci = 'SUCCESS'
                   AND updated_at = 7 AND off_default_description = 'Fixes #9'",
            ),
            count(
                &store,
                "SELECT count(*) AS found FROM review_requests
                 WHERE repository = 'python/cpython' COLLATE BINARY AND number = 1
                   AND login = 'Octocat' COLLATE BINARY",
            ),
            count(
                &store,
                "SELECT count(*) AS found FROM latest_reviews
                 WHERE repository = 'python/cpython' COLLATE BINARY AND number = 1
                   AND login = 'some-reviewer' COLLATE BINARY AND state = 'APPROVED'",
            ),
            count(
                &store,
                "SELECT count(*) AS found FROM assignees
                 WHERE repository = 'python/cpython' COLLATE BINARY AND number = 1
                   AND login = 'some-assignee' COLLATE BINARY",
            ),
            count(
                &store,
                "SELECT count(*) AS found FROM closing_references
                 WHERE repository = 'python/cpython' COLLATE BINARY AND number = 1 AND issue IN (7, 8)",
            ),
        ];
    open.close();
    store.close();
    fs::remove_dir_all(folder).unwrap();

    assert_eq!(found, [1, 1, 1, 1, 2]);
}

#[test]
fn a_row_an_earlier_tsugiki_wrote_reads_back() {
    let (store, open, reader, _path, folder) = opened("pr-earlier");
    store
            .call(|connection| {
                connection.execute_batch(
                    "INSERT INTO pull_requests
                         (repository, number, title, state, is_draft, author, target_branch, ci, updated_at)
                     VALUES ('python/cpython', 1, 'gh-1: Fix it', 'OPEN', 0, 'some-author', 'main', 'SUCCESS',
                             7);
                     INSERT INTO review_requests VALUES ('python/cpython', 1, 'Octocat');
                     INSERT INTO latest_reviews VALUES ('python/cpython', 1, 'some-reviewer', 'APPROVED');
                     INSERT INTO assignees VALUES ('python/cpython', 1, 'some-assignee');",
                )
            })
            .unwrap()
            .unwrap();

    let read = reader.work_facts(&cpython()).unwrap();
    open.close();
    store.close();
    fs::remove_dir_all(folder).unwrap();

    let earlier = PullRequestFacts {
        closing_issues: vec![],
        merge_commit: None,
        off_default: None,
        ..at(7, facts(1))
    };
    assert_eq!(read.pull_requests, [earlier]);
}

#[test]
fn logins_keep_githubs_casing_and_the_database_ignores_case() {
    let (store, open, reader, _path, folder) = opened("pr-casing");
    store.remember_login(&Login::new("OctoCat")).unwrap();
    let twice = PullRequestFacts {
        review_requests: vec![Login::new("Some-Reviewer"), Login::new("some-reviewer")],
        ..facts(1)
    };

    store
        .save_pull_requests(&cpython(), vec![at(1, twice)])
        .unwrap();
    let read = reader.work_facts(&cpython()).unwrap();
    let viewer = store.last_login().unwrap().unwrap();
    open.close();
    store.close();
    fs::remove_dir_all(folder).unwrap();

    let requested: Vec<_> = read
        .pull_requests
        .iter()
        .flat_map(|pr| &pr.review_requests)
        .map(Login::as_str)
        .collect();
    assert_eq!(requested, ["Some-Reviewer"]);
    assert_eq!(viewer.as_str(), "OctoCat");
}

#[test]
fn a_reader_doesnt_wait_for_a_write_in_progress() {
    let (store, open, reader, _path, folder) = opened("pr-no-wait");
    store
        .save_pull_requests(&cpython(), vec![at(1, facts(1))])
        .unwrap();
    let (started, writing) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let job: Job = Box::new(move |connection| {
        let transaction = connection.transaction().unwrap();
        let unfinished = PullRequestFacts {
            title: "unfinished".to_owned(),
            ..facts(1)
        };
        super::replace(&transaction, &cpython(), &unfinished).unwrap();
        started.send(()).unwrap();
        released.recv().unwrap();
        transaction.commit().unwrap();
    });
    store.jobs.send(job).unwrap();
    writing.recv().unwrap();

    let read = reader.work_facts(&cpython());
    release.send(()).unwrap();
    open.close();
    store.close();
    fs::remove_dir_all(folder).unwrap();

    assert_eq!(read.unwrap().pull_requests, [facts(1)]);
}

#[test]
fn a_reader_cant_write() {
    let (store, open, reader, _path, folder) = opened("pr-read-only");

    let written = reader
        .call(|connection| write_last_login(connection, &Login::new("octocat")))
        .unwrap();
    open.close();
    store.close();
    fs::remove_dir_all(folder).unwrap();

    assert!(written.is_err());
}

#[test]
fn a_question_asked_before_the_database_opens_waits_for_it() {
    let (path, folder) = temp_database("pr-early");
    let (to_open, reader) = reader();
    let name = cpython();
    let asked = thread::spawn(move || reader.work_facts(&name));

    let store = Store::open(&path).unwrap();
    let open = to_open.open(&path).unwrap();
    let read = asked.join().unwrap();
    open.close();
    store.close();
    fs::remove_dir_all(folder).unwrap();

    assert_eq!(read.unwrap().pull_requests, []);
}

#[test]
fn a_database_that_never_opens_is_an_answer_too() {
    let (to_open, reader) = reader();

    drop(to_open);

    let refused = reader.work_facts(&cpython()).unwrap_err();
    assert_eq!(refused, Error::new(ErrorKind::Internal, STOPPED));
}

#[test]
fn a_reader_with_no_file_to_read_says_so() {
    let (path, folder) = temp_database("pr-no-file");
    let (to_open, _reader) = reader();

    let refused = to_open.open(&path).err().unwrap();
    drop(fs::remove_dir_all(folder));

    assert_eq!(refused, Error::new(ErrorKind::Internal, CANT_OPEN));
}

#[test]
fn closing_both_connections_leaves_only_the_database() {
    let (store, open, reader, path, folder) = opened("pr-close");
    store
        .save_pull_requests(&cpython(), vec![at(1, facts(1))])
        .unwrap();

    open.close();
    let refused = reader.work_facts(&cpython()).unwrap_err();
    store.close();
    let mut left: Vec<_> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    left.sort();
    fs::remove_dir_all(folder).unwrap();

    assert_eq!(refused, Error::new(ErrorKind::Internal, STOPPED));
    assert_eq!(left, ["tsugiki.db"]);
}

#[test]
fn a_row_tsugiki_cant_understand_is_a_read_error() {
    let (store, open, reader, _path, folder) = opened("pr-bad-row");
    store
        .save_pull_requests(&cpython(), vec![at(1, facts(1))])
        .unwrap();
    let _ = store
        .call(|connection| connection.execute("UPDATE pull_requests SET ci = 'SOMETHING NEW'", []))
        .unwrap()
        .unwrap();

    let refused = reader.work_facts(&cpython()).unwrap_err();
    open.close();
    store.close();
    fs::remove_dir_all(folder).unwrap();

    assert_eq!(refused, Error::new(ErrorKind::Internal, CANT_READ));
}

#[test]
fn a_save_that_fails_says_so_and_keeps_nothing_of_the_batch() {
    let (store, open, reader, _path, folder) = opened("pr-bad-save");
    store
        .call(|connection| {
            connection.execute_batch(
                "CREATE TRIGGER refuse BEFORE INSERT ON assignees
                     WHEN NEW.number = 2
                     BEGIN SELECT RAISE(ABORT, 'refused'); END;",
            )
        })
        .unwrap()
        .unwrap();

    let refused = store
        .save_pull_requests(&cpython(), vec![at(1, facts(1)), at(1, facts(2))])
        .unwrap_err();
    let found = numbers(&reader, &cpython());
    open.close();
    store.close();
    fs::remove_dir_all(folder).unwrap();

    assert_eq!(refused, Error::new(ErrorKind::Internal, CANT_WRITE));
    assert_eq!(found, [0_u32; 0]);
}
