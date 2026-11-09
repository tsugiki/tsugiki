//! The Issues that PRs name, as facts (ADR 0010). The link from a PR to an Issue isn't stored: the core works
//! it out from a PR's facts when a screen asks (ADR 0011).

use std::collections::BTreeMap;

use rusqlite::named_params;

use super::Connection;
use super::columns::UnixTime;
use super::{activity, pull_requests};
use crate::github::activity::ItemActivity;
use crate::github::issues::IssueFacts;
use crate::login::Login;
use crate::repository::RepositoryName;

pub(super) const SCHEMA: &str = "
    CREATE TABLE issues (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        title TEXT NOT NULL,
        author TEXT,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (repository, number)
    ) WITHOUT ROWID;
    CREATE TABLE issue_assignees (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        login TEXT NOT NULL COLLATE NOCASE,
        PRIMARY KEY (repository, number, login),
        FOREIGN KEY (repository, number) REFERENCES issues (repository, number)
    ) WITHOUT ROWID;
";

/// Whether each Issue is open, and how it closed (#188).
pub(super) const STATES: &str = "ALTER TABLE issues ADD COLUMN state TEXT;";

/// Stores `issues` in one transaction, each replacing what Tsugiki knew about it (ADR 0010).
pub(super) fn save(
    connection: &mut Connection,
    repository: &RepositoryName,
    issues: &[IssueFacts],
    events: &[ItemActivity],
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    for issue in issues {
        save_issue(&transaction, repository, issue)?;
        forget_assignees(&transaction, repository, issue.number)?;
        for login in &issue.assignees {
            save_assignee(&transaction, repository, issue.number, login)?;
        }
    }
    activity::save(&transaction, repository, events)?;
    transaction.commit()
}

fn save_issue(
    connection: &Connection,
    repository: &RepositoryName,
    issue: &IssueFacts,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT INTO issues (repository, number, title, author, updated_at, state)
             VALUES (:repository, :number, :title, :author, :updated_at, :state)
             ON CONFLICT (repository, number) DO UPDATE SET
                 title = excluded.title,
                 author = excluded.author,
                 updated_at = excluded.updated_at,
                 state = excluded.state",
            named_params! {
                ":repository": repository,
                ":number": issue.number,
                ":title": issue.title,
                ":author": issue.author,
                ":updated_at": issue.updated_at.unix_timestamp(),
                ":state": issue.state,
            },
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
            "DELETE FROM issue_assignees WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number },
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
            "INSERT OR IGNORE INTO issue_assignees (repository, number, login)
             VALUES (:repository, :number, :login)",
            named_params! { ":repository": repository, ":number": number, ":login": login },
        )
        .map(drop)
}

fn assignees(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<u32, Vec<Login>>> {
    let query = "SELECT number, login FROM issue_assignees WHERE repository = :repository";
    pull_requests::values_by_number(
        connection.prepare(query)?,
        named_params! { ":repository": repository },
        "login",
    )
}

/// The numbers of the Issues of `repository` saved with no state, which the migration that added states
/// left (#231).
pub(super) fn numbers_without_state(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<Vec<u32>> {
    let query = "SELECT number FROM issues
                 WHERE repository = :repository AND state IS NULL
                 ORDER BY number";
    let mut statement = connection.prepare(query)?;
    let numbers = statement.query_map(named_params! { ":repository": repository }, |row| {
        row.get("number")
    });
    numbers?.collect()
}

/// The Issues of `repository` that Tsugiki keeps.
pub(super) fn read_all(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<Vec<IssueFacts>> {
    let query = "SELECT number, title, author, updated_at, state FROM issues
                 WHERE repository = :repository
                 ORDER BY number";
    let mut statement = connection.prepare(query)?;
    let mut rows = statement.query(named_params! { ":repository": repository })?;
    let mut assignees = assignees(connection, repository)?;
    let mut found = Vec::new();
    while let Some(row) = rows.next()? {
        let UnixTime(updated_at) = row.get("updated_at")?;
        let number = row.get("number")?;
        found.push(IssueFacts {
            number,
            title: row.get("title")?,
            author: row.get("author")?,
            assignees: assignees.remove(&number).unwrap_or_default(),
            updated_at,
            state: row.get("state")?,
        });
    }
    Ok(found)
}

#[cfg(test)]
mod tests;
