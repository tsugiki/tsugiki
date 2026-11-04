//! The PRs Tsugiki keeps, as facts (ADR 0010). What a fact means for a screen is the core's to work out when
//! the screen asks.

use std::collections::BTreeMap;

use rusqlite::{Connection, Statement, named_params};

use crate::github::Login;
use crate::github::pull_requests::{LatestReview, PrState, PullRequestFacts};
use crate::repository::RepositoryName;

use super::columns::UnixTime;

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

/// The facts the link rules read (ADR 0011). A PR stored before this has neither until the next sync fetches
/// it.
pub(super) const LINK_FACTS: &str = "
    ALTER TABLE pull_requests ADD COLUMN off_default_description TEXT;
    CREATE TABLE closing_references (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        issue INTEGER NOT NULL,
        PRIMARY KEY (repository, number, issue)
    ) WITHOUT ROWID;
";

/// Stores `pull_requests` in one transaction, each replacing what Tsugiki knew about it (ADR 0010).
pub(super) fn save(
    connection: &mut Connection,
    repository: &RepositoryName,
    pull_requests: &[PullRequestFacts],
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    for pull_request in pull_requests {
        replace(&transaction, repository, pull_request)?;
    }
    transaction.commit()
}

fn replace(
    connection: &Connection,
    repository: &RepositoryName,
    facts: &PullRequestFacts,
) -> rusqlite::Result<()> {
    let number = facts.number;
    save_pull_request(connection, repository, facts)?;
    forget_review_requests(connection, repository, number)?;
    forget_latest_reviews(connection, repository, number)?;
    forget_assignees(connection, repository, number)?;
    forget_closing_references(connection, repository, number)?;
    for login in &facts.review_requests {
        save_review_request(connection, repository, number, login)?;
    }
    for review in &facts.latest_reviews {
        save_latest_review(connection, repository, number, review)?;
    }
    for login in &facts.assignees {
        save_assignee(connection, repository, number, login)?;
    }
    for issue in &facts.closing_issues {
        save_closing_reference(connection, repository, number, *issue)?;
    }
    Ok(())
}

fn save_pull_request(
    connection: &Connection,
    repository: &RepositoryName,
    facts: &PullRequestFacts,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT INTO pull_requests
                 (repository, number, title, state, is_draft, author, target_branch, ci, updated_at,
                  off_default_description)
             VALUES (:repository, :number, :title, :state, :is_draft, :author, :target_branch, :ci,
                     :updated_at, :off_default_description)
             ON CONFLICT (repository, number) DO UPDATE SET
                 title = excluded.title,
                 state = excluded.state,
                 is_draft = excluded.is_draft,
                 author = excluded.author,
                 target_branch = excluded.target_branch,
                 ci = excluded.ci,
                 updated_at = excluded.updated_at,
                 off_default_description = excluded.off_default_description",
            named_params! {
                ":repository": repository,
                ":number": facts.number,
                ":title": facts.title,
                ":state": facts.state,
                ":is_draft": facts.is_draft,
                ":author": facts.author,
                ":target_branch": facts.target_branch,
                ":ci": facts.ci,
                ":updated_at": facts.updated_at.unix_timestamp(),
                ":off_default_description": facts.off_default_description,
            },
        )
        .map(drop)
}

fn forget_review_requests(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "DELETE FROM review_requests WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number },
        )
        .map(drop)
}

fn forget_latest_reviews(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "DELETE FROM latest_reviews WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number },
        )
        .map(drop)
}

fn forget_assignees(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "DELETE FROM assignees WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number },
        )
        .map(drop)
}

fn forget_closing_references(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "DELETE FROM closing_references WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number },
        )
        .map(drop)
}

fn save_review_request(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    login: &Login,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT OR IGNORE INTO review_requests (repository, number, login)
             VALUES (:repository, :number, :login)",
            named_params! { ":repository": repository, ":number": number, ":login": login },
        )
        .map(drop)
}

fn save_latest_review(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    review: &LatestReview,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT OR IGNORE INTO latest_reviews (repository, number, login, state)
             VALUES (:repository, :number, :login, :state)",
            named_params! {
                ":repository": repository,
                ":number": number,
                ":login": review.author,
                ":state": review.state,
            },
        )
        .map(drop)
}

fn save_assignee(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    login: &Login,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT OR IGNORE INTO assignees (repository, number, login)
             VALUES (:repository, :number, :login)",
            named_params! { ":repository": repository, ":number": number, ":login": login },
        )
        .map(drop)
}

fn save_closing_reference(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    issue: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT OR IGNORE INTO closing_references (repository, number, issue)
             VALUES (:repository, :number, :issue)",
            named_params! { ":repository": repository, ":number": number, ":issue": issue },
        )
        .map(drop)
}

fn review_requests(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<u32, Vec<Login>>> {
    logins_by_number(
        connection
            .prepare("SELECT number, login FROM review_requests WHERE repository = :repository")?,
        repository,
    )
}

fn assignees(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<u32, Vec<Login>>> {
    logins_by_number(
        connection.prepare("SELECT number, login FROM assignees WHERE repository = :repository")?,
        repository,
    )
}

/// Each PR's logins, from a `statement` that selects `number` and `login` for `:repository`.
fn logins_by_number(
    mut statement: Statement<'_>,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<u32, Vec<Login>>> {
    let mut found: BTreeMap<u32, Vec<Login>> = BTreeMap::new();
    let mut rows = statement.query(named_params! { ":repository": repository })?;
    while let Some(row) = rows.next()? {
        let login = row.get("login")?;
        found.entry(row.get("number")?).or_default().push(login);
    }
    Ok(found)
}

fn latest_reviews(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<u32, Vec<LatestReview>>> {
    let mut found: BTreeMap<u32, Vec<LatestReview>> = BTreeMap::new();
    let query = "SELECT number, login, state FROM latest_reviews WHERE repository = :repository";
    let mut statement = connection.prepare(query)?;
    let mut rows = statement.query(named_params! { ":repository": repository })?;
    while let Some(row) = rows.next()? {
        let review = LatestReview {
            author: row.get("login")?,
            state: row.get("state")?,
        };
        found.entry(row.get("number")?).or_default().push(review);
    }
    Ok(found)
}

/// Each PR's closing references, in number order.
fn closing_references(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<u32, Vec<u32>>> {
    let mut found: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let query = "SELECT number, issue FROM closing_references
                 WHERE repository = :repository
                 ORDER BY number, issue";
    let mut statement = connection.prepare(query)?;
    let mut rows = statement.query(named_params! { ":repository": repository })?;
    while let Some(row) = rows.next()? {
        let issue = row.get("issue")?;
        found.entry(row.get("number")?).or_default().push(issue);
    }
    Ok(found)
}

/// The open PRs of `repository`, the latest update first.
pub(super) fn read_open(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<Vec<PullRequestFacts>> {
    let mut requests = review_requests(connection, repository)?;
    let mut reviews = latest_reviews(connection, repository)?;
    let mut assigned = assignees(connection, repository)?;
    let mut closing = closing_references(connection, repository)?;
    let query = "SELECT number, title, state, is_draft, author, target_branch, ci, updated_at,
                        off_default_description
                 FROM pull_requests
                 WHERE repository = :repository AND state = :state
                 ORDER BY updated_at DESC, number DESC";
    let mut statement = connection.prepare(query)?;
    let values = named_params! { ":repository": repository, ":state": PrState::Open };
    let mut rows = statement.query(values)?;
    let mut found = Vec::new();
    while let Some(row) = rows.next()? {
        let number: u32 = row.get("number")?;
        let UnixTime(updated_at) = row.get("updated_at")?;
        found.push(PullRequestFacts {
            number,
            title: row.get("title")?,
            state: row.get("state")?,
            is_draft: row.get("is_draft")?,
            author: row.get("author")?,
            target_branch: row.get("target_branch")?,
            ci: row.get("ci")?,
            updated_at,
            review_requests: requests.remove(&number).unwrap_or_default(),
            latest_reviews: reviews.remove(&number).unwrap_or_default(),
            assignees: assigned.remove(&number).unwrap_or_default(),
            closing_issues: closing.remove(&number).unwrap_or_default(),
            off_default_description: row.get("off_default_description")?,
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
            off_default_description: Some("Fixes #9".to_owned()),
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
            off_default_description: None,
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
            closing_issues: vec![],
            off_default_description: None,
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
            off_default_description: None,
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
