//! The PRs Tsugiki keeps, as facts (ADR 0010). What a fact means for a screen is the core's to work out when
//! the screen asks.

use std::collections::BTreeMap;

use rusqlite::{Connection, Statement, named_params};

use crate::github::pull_requests::{LatestReview, OffDefault, PullRequestFacts};
use crate::github::{CommitSha, Login};
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

/// The facts the Backport rules read (ADR 0011). A PR stored before this has none, so it isn't a Bot
/// backport until the next sync fetches it. A merged one isn't frozen until then either, because it has no
/// merge commit.
pub(super) const BACKPORT_FACTS: &str = "
    ALTER TABLE pull_requests ADD COLUMN merge_commit TEXT;
    CREATE TABLE cherry_picks (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        sha TEXT NOT NULL,
        PRIMARY KEY (repository, number, sha)
    ) WITHOUT ROWID;
    CREATE TABLE committers (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        position INTEGER NOT NULL,
        login TEXT,
        PRIMARY KEY (repository, number, position)
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
    forget_cherry_picks(connection, repository, number)?;
    forget_committers(connection, repository, number)?;
    if let Some(off_default) = &facts.off_default {
        save_off_default(connection, repository, number, off_default)?;
    }
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
                  off_default_description, merge_commit)
             VALUES (:repository, :number, :title, :state, :is_draft, :author, :target_branch, :ci,
                     :updated_at, :off_default_description, :merge_commit)
             ON CONFLICT (repository, number) DO UPDATE SET
                 title = excluded.title,
                 state = excluded.state,
                 is_draft = excluded.is_draft,
                 author = excluded.author,
                 target_branch = excluded.target_branch,
                 ci = excluded.ci,
                 updated_at = excluded.updated_at,
                 off_default_description = excluded.off_default_description,
                 merge_commit = excluded.merge_commit",
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
                ":off_default_description": facts.off_default.as_ref().map(|off| &off.description),
                ":merge_commit": facts.merge_commit,
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

fn forget_cherry_picks(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "DELETE FROM cherry_picks WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number },
        )
        .map(drop)
}

fn forget_committers(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "DELETE FROM committers WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number },
        )
        .map(drop)
}

fn save_off_default(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    off_default: &OffDefault,
) -> rusqlite::Result<()> {
    for sha in &off_default.cherry_picked_from {
        save_cherry_pick(connection, repository, number, sha)?;
    }
    for (position, login) in (0_u32..).zip(&off_default.committers) {
        let query = "INSERT INTO committers (repository, number, position, login)
                     VALUES (:repository, :number, :position, :login)";
        let values = named_params! {
            ":repository": repository,
            ":number": number,
            ":position": position,
            ":login": login,
        };
        connection.execute(query, values).map(drop)?;
    }
    Ok(())
}

fn save_cherry_pick(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    sha: &CommitSha,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT OR IGNORE INTO cherry_picks (repository, number, sha)
             VALUES (:repository, :number, :sha)",
            named_params! { ":repository": repository, ":number": number, ":sha": sha },
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

/// Each PR's cherry-picked commits.
fn cherry_picks(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<u32, Vec<CommitSha>>> {
    let mut found: BTreeMap<u32, Vec<CommitSha>> = BTreeMap::new();
    let query = "SELECT number, sha FROM cherry_picks WHERE repository = :repository";
    let mut statement = connection.prepare(query)?;
    let mut rows = statement.query(named_params! { ":repository": repository })?;
    while let Some(row) = rows.next()? {
        let sha = row.get("sha")?;
        found.entry(row.get("number")?).or_default().push(sha);
    }
    Ok(found)
}

/// Each PR's committers, in the order of its commits.
fn committers(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<u32, Vec<Option<Login>>>> {
    let mut found: BTreeMap<u32, Vec<Option<Login>>> = BTreeMap::new();
    let query = "SELECT number, login FROM committers
                 WHERE repository = :repository
                 ORDER BY number, position";
    let mut statement = connection.prepare(query)?;
    let mut rows = statement.query(named_params! { ":repository": repository })?;
    while let Some(row) = rows.next()? {
        let login = row.get("login")?;
        found.entry(row.get("number")?).or_default().push(login);
    }
    Ok(found)
}

/// Every PR of `repository`, whatever its state, the latest update first.
pub(super) fn read_all(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<Vec<PullRequestFacts>> {
    let mut requests = review_requests(connection, repository)?;
    let mut reviews = latest_reviews(connection, repository)?;
    let mut assigned = assignees(connection, repository)?;
    let mut closing = closing_references(connection, repository)?;
    let mut picks = cherry_picks(connection, repository)?;
    let mut committed = committers(connection, repository)?;
    let query = "SELECT number, title, state, is_draft, author, target_branch, ci, updated_at,
                        off_default_description, merge_commit
                 FROM pull_requests
                 WHERE repository = :repository
                 ORDER BY updated_at DESC, number DESC";
    let mut statement = connection.prepare(query)?;
    let mut rows = statement.query(named_params! { ":repository": repository })?;
    let mut found = Vec::new();
    while let Some(row) = rows.next()? {
        let number: u32 = row.get("number")?;
        let UnixTime(updated_at) = row.get("updated_at")?;
        let description: Option<String> = row.get("off_default_description")?;
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
            merge_commit: row.get("merge_commit")?,
            off_default: description.map(|description| OffDefault {
                description,
                cherry_picked_from: picks.remove(&number).unwrap_or_default(),
                committers: committed.remove(&number).unwrap_or_default(),
            }),
        });
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
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
