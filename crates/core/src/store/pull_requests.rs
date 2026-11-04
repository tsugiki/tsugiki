//! The PRs Tsugiki keeps, as facts (ADR 0010). What a fact means for a screen is the core's to work out when
//! the screen asks.

use std::collections::BTreeMap;

use rusqlite::{Connection, ToSql, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::github::Login;
use crate::github::pull_requests::{LatestReview, PullRequest, PullRequestFacts};

pub(super) const SCHEMA: &str = "
    CREATE TABLE pull_requests (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        title TEXT NOT NULL,
        state TEXT NOT NULL,
        is_draft INTEGER NOT NULL,
        author TEXT,
        target_branch TEXT NOT NULL,
        ci TEXT,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (repository, number)
    ) WITHOUT ROWID;
    CREATE INDEX pull_requests_by_update ON pull_requests (repository, state, updated_at);
    CREATE TABLE review_requests (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        login TEXT NOT NULL COLLATE NOCASE,
        PRIMARY KEY (repository, number, login)
    ) WITHOUT ROWID;
    CREATE TABLE latest_reviews (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        login TEXT NOT NULL COLLATE NOCASE,
        state TEXT NOT NULL,
        PRIMARY KEY (repository, number, login)
    ) WITHOUT ROWID;
    CREATE TABLE assignees (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        login TEXT NOT NULL COLLATE NOCASE,
        PRIMARY KEY (repository, number, login)
    ) WITHOUT ROWID;
";

const SAVE: &str = "INSERT INTO pull_requests
                        (repository, number, title, state, is_draft, author, target_branch, ci, updated_at)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                    ON CONFLICT (repository, number) DO UPDATE SET
                        title = excluded.title,
                        state = excluded.state,
                        is_draft = excluded.is_draft,
                        author = excluded.author,
                        target_branch = excluded.target_branch,
                        ci = excluded.ci,
                        updated_at = excluded.updated_at";
const FORGET_REQUESTS: &str = "DELETE FROM review_requests WHERE repository = ?1 AND number = ?2";
const FORGET_REVIEWS: &str = "DELETE FROM latest_reviews WHERE repository = ?1 AND number = ?2";
const FORGET_ASSIGNEES: &str = "DELETE FROM assignees WHERE repository = ?1 AND number = ?2";
const SAVE_REQUEST: &str =
    "INSERT OR IGNORE INTO review_requests (repository, number, login) VALUES (?1, ?2, ?3)";
const SAVE_REVIEW: &str = "INSERT OR IGNORE INTO latest_reviews (repository, number, login, state) VALUES (?1, ?2, ?3, ?4)";
const SAVE_ASSIGNEE: &str =
    "INSERT OR IGNORE INTO assignees (repository, number, login) VALUES (?1, ?2, ?3)";

const OPEN_PULL_REQUESTS: &str = "SELECT number, title, state, is_draft, author, target_branch, ci
                                  FROM pull_requests
                                  WHERE repository = ?1 AND state = 'OPEN'
                                  ORDER BY updated_at DESC, number DESC";
const REVIEW_REQUESTS: &str = "SELECT number, login FROM review_requests WHERE repository = ?1";
const LATEST_REVIEWS: &str =
    "SELECT number, login, state FROM latest_reviews WHERE repository = ?1";
const ASSIGNEES: &str = "SELECT number, login FROM assignees WHERE repository = ?1";

/// The queries a screen waits on, for the plan check in the store's tests (ADR 0017).
#[cfg(test)]
pub(super) const VIEW_QUERIES: [&str; 4] = [
    OPEN_PULL_REQUESTS,
    REVIEW_REQUESTS,
    LATEST_REVIEWS,
    ASSIGNEES,
];

/// How a fieldless enum of GitHub's reads in a column, which is how GitHub spells it. Such an enum always
/// serializes to a string, and if one ever didn't, the column's `NOT NULL` would refuse the row.
fn text_of<T: Serialize>(value: &T) -> Option<String> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
}

fn parse_text<T: DeserializeOwned>(text: String) -> rusqlite::Result<T> {
    serde_json::from_value(Value::String(text)).map_err(|cause| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(cause))
    })
}

/// Stores `pull_requests` in one transaction, each replacing what Tsugiki knew about it (ADR 0010).
pub(super) fn save(
    connection: &mut Connection,
    repository: &str,
    pull_requests: &[PullRequest],
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    save_each(&transaction, repository, pull_requests)?;
    transaction.commit()
}

fn save_each(
    connection: &Connection,
    repository: &str,
    pull_requests: &[PullRequest],
) -> rusqlite::Result<()> {
    let mut save_request = connection.prepare(SAVE_REQUEST)?;
    let mut save_review = connection.prepare(SAVE_REVIEW)?;
    let mut save_assignee = connection.prepare(SAVE_ASSIGNEE)?;
    for pull_request in pull_requests {
        let facts = &pull_request.facts;
        let number = facts.number;
        let values: [&dyn ToSql; 9] = [
            &repository,
            &number,
            &facts.title,
            &text_of(&facts.state),
            &facts.is_draft,
            &facts.author.as_ref().map(Login::as_str),
            &facts.target_branch,
            &facts.ci.as_ref().and_then(text_of),
            &pull_request.updated_at.unix_timestamp(),
        ];
        let _ = connection.prepare(SAVE)?.execute(&values[..])?;
        for forget in [FORGET_REQUESTS, FORGET_REVIEWS, FORGET_ASSIGNEES] {
            let _ = connection
                .prepare(forget)?
                .execute(params![repository, number])?;
        }
        for login in &facts.review_requests {
            let _ = save_request.execute(params![repository, number, login.as_str()])?;
        }
        for review in &facts.latest_reviews {
            let (author, state) = (review.author.as_str(), text_of(&review.state));
            let _ = save_review.execute(params![repository, number, author, state])?;
        }
        for login in &facts.assignees {
            let _ = save_assignee.execute(params![repository, number, login.as_str()])?;
        }
    }
    Ok(())
}

/// Each PR's logins from a query of `(number, login)`.
fn logins_by_number(
    connection: &Connection,
    query: &str,
    repository: &str,
) -> rusqlite::Result<BTreeMap<u32, Vec<Login>>> {
    let mut found: BTreeMap<u32, Vec<Login>> = BTreeMap::new();
    let mut statement = connection.prepare(query)?;
    let mut rows = statement.query([repository])?;
    while let Some(row) = rows.next()? {
        let login: String = row.get(1)?;
        found
            .entry(row.get(0)?)
            .or_default()
            .push(Login::new(&login));
    }
    Ok(found)
}

fn reviews_by_number(
    connection: &Connection,
    repository: &str,
) -> rusqlite::Result<BTreeMap<u32, Vec<LatestReview>>> {
    let mut found: BTreeMap<u32, Vec<LatestReview>> = BTreeMap::new();
    let mut statement = connection.prepare(LATEST_REVIEWS)?;
    let mut rows = statement.query([repository])?;
    while let Some(row) = rows.next()? {
        let login: String = row.get(1)?;
        found.entry(row.get(0)?).or_default().push(LatestReview {
            author: Login::new(&login),
            state: parse_text(row.get(2)?)?,
        });
    }
    Ok(found)
}

/// The open PRs of `repository`, the latest update first.
pub(super) fn read_open(
    connection: &Connection,
    repository: &str,
) -> rusqlite::Result<Vec<PullRequestFacts>> {
    let mut requests = logins_by_number(connection, REVIEW_REQUESTS, repository)?;
    let mut reviews = reviews_by_number(connection, repository)?;
    let mut assignees = logins_by_number(connection, ASSIGNEES, repository)?;
    let mut statement = connection.prepare(OPEN_PULL_REQUESTS)?;
    let mut rows = statement.query([repository])?;
    let mut found = Vec::new();
    while let Some(row) = rows.next()? {
        let number: u32 = row.get(0)?;
        let author: Option<String> = row.get(4)?;
        let ci: Option<String> = row.get(6)?;
        found.push(PullRequestFacts {
            number,
            title: row.get(1)?,
            state: parse_text(row.get(2)?)?,
            is_draft: row.get(3)?,
            author: author.as_deref().map(Login::new),
            target_branch: row.get(5)?,
            ci: ci.map(parse_text).transpose()?,
            review_requests: requests.remove(&number).unwrap_or_default(),
            latest_reviews: reviews.remove(&number).unwrap_or_default(),
            assignees: assignees.remove(&number).unwrap_or_default(),
        });
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::super::testing::temp_database;
    use super::super::*;
    use crate::error::ErrorKind;
    use crate::github::pull_requests::{CiState, LatestReview, PrState, ReviewState};
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
            author: Some(Login::new("vstinner")),
            target_branch: "main".to_owned(),
            ci: Some(CiState::Success),
            review_requests: vec![Login::new("Octocat")],
            latest_reviews: vec![LatestReview {
                author: Login::new("hugovk"),
                state: ReviewState::Approved,
            }],
            assignees: vec![Login::new("pablogsal")],
        }
    }

    fn at(seconds: i64, facts: PullRequestFacts) -> PullRequest {
        PullRequest {
            updated_at: OffsetDateTime::from_unix_timestamp(seconds).unwrap(),
            facts,
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
            ..facts(1)
        };

        store
            .save_pull_requests(&cpython(), vec![at(2, changed.clone())])
            .unwrap();
        let read = reader.work_facts(&cpython()).unwrap();
        open.close();
        store.close();
        fs::remove_dir_all(folder).unwrap();

        assert_eq!(read.pull_requests, [changed]);
    }

    #[test]
    fn only_open_prs_are_read_and_the_latest_update_comes_first() {
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

        assert_eq!(found, [2, 4, 1]);
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
            let _ = transaction
                .execute("UPDATE pull_requests SET title = 'unfinished'", [])
                .unwrap();
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
            .call(|connection| connection.execute("DELETE FROM viewer", []))
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
            .call(|connection| {
                connection.execute("UPDATE pull_requests SET ci = 'SOMETHING NEW'", [])
            })
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
}
