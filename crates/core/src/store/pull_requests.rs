//! The PRs Tsugiki keeps, as facts (ADR 0010). What a fact means for a screen is the core's to work out when
//! the screen asks.

use std::collections::BTreeMap;

use rusqlite::{Connection, Statement, named_params};
use time::OffsetDateTime;

use crate::github::activity::ItemActivity;
use crate::github::pull_requests::{LatestReview, OffDefault, PullRequestFacts};
use crate::github::{CommitSha, Login};
use crate::repository::RepositoryName;

use super::activity;
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
    events: &[ItemActivity],
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    // Before the PRs, because a commit counts differently on a PR the store holds.
    activity::save(&transaction, repository, events)?;
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
                  off_default_description, merge_commit, ci_finished_at)
             VALUES (:repository, :number, :title, :state, :is_draft, :author, :target_branch, :ci,
                     :updated_at, :off_default_description, :merge_commit, CASE
                         WHEN :ci IN ('SUCCESS', 'FAILURE', 'ERROR')
                             AND EXISTS (SELECT 1 FROM first_syncs WHERE repository = :repository)
                         THEN :now END)
             ON CONFLICT (repository, number) DO UPDATE SET
                 title = excluded.title,
                 state = excluded.state,
                 is_draft = excluded.is_draft,
                 author = excluded.author,
                 target_branch = excluded.target_branch,
                 ci = excluded.ci,
                 ci_finished_at = CASE
                     WHEN excluded.ci IS NOT pull_requests.ci
                         AND excluded.ci IN ('SUCCESS', 'FAILURE', 'ERROR')
                     THEN :now ELSE pull_requests.ci_finished_at END,
                 checks_read_at = CASE
                     WHEN excluded.updated_at > pull_requests.updated_at THEN NULL
                     ELSE pull_requests.checks_read_at END,
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
                ":now": OffsetDateTime::now_utc().unix_timestamp(),
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

pub(super) fn forget_assignees(
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

pub(super) fn save_assignee(
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

pub(super) fn assignees(
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
mod tests;
