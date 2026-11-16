//! The Issues that PRs name, as facts (ADR 0010). The link from a PR to an Issue isn't stored: the core works
//! it out from a PR's facts when a screen asks (ADR 0011).

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::named_params;
use time::OffsetDateTime;

use super::columns::{Numbers, UnixTime};
use super::{Connection, Detail, TitledItem, TitledKind};
use super::{activity, pull_requests};
use crate::facts::activity::ItemActivity;
use crate::facts::issues::IssueFacts;
use crate::facts::labels::Label;
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

/// The labels of each Issue, which a search of the cache matches (#463). An Issue stored before this has
/// none until a sync reads it again.
pub(super) const LABELS: &str = "
    CREATE TABLE issue_labels (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        name TEXT NOT NULL COLLATE NOCASE,
        PRIMARY KEY (repository, number, name),
        FOREIGN KEY (repository, number) REFERENCES issues (repository, number)
    ) WITHOUT ROWID;
";

/// The open Issues by their last update, which the Inbox reads a page at a time (#827).
pub(super) const BY_UPDATE: &str =
    "CREATE INDEX issues_by_update ON issues (repository, state, updated_at);";

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
        forget_labels(&transaction, repository, issue.number)?;
        for label in &issue.labels {
            save_label(&transaction, repository, issue.number, label)?;
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

fn forget_labels(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "DELETE FROM issue_labels WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number },
        )
        .map(drop)
}

fn save_label(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    label: &Label,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT OR IGNORE INTO issue_labels (repository, number, name)
             VALUES (:repository, :number, :name)",
            named_params! { ":repository": repository, ":number": number, ":name": label },
        )
        .map(drop)
}

fn labels(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<u32, Vec<Label>>> {
    // In name order, ignoring case, as `IssueFacts` holds them.
    let query = "SELECT number, name FROM issue_labels WHERE repository = :repository ORDER BY number, name";
    pull_requests::values_by_number(
        connection.prepare(query)?,
        named_params! { ":repository": repository },
        "name",
    )
}

fn assignees(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<u32, Vec<Login>>> {
    // In login order, which the Issue view's author line shows them in.
    let query = "SELECT number, login FROM issue_assignees WHERE repository = :repository ORDER BY number, login";
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

/// The number, title, state and last update of each Issue of `repository`, which quick open searches.
pub(super) fn read_titled(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<Vec<TitledItem>> {
    let query = "SELECT number, title, updated_at, state FROM issues
                 WHERE repository = :repository";
    let mut statement = connection.prepare(query)?;
    let titled = statement.query_map(named_params! { ":repository": repository }, |row| {
        let UnixTime(updated_at) = row.get("updated_at")?;
        Ok(TitledItem {
            number: row.get("number")?,
            title: row.get("title")?,
            updated_at,
            kind: TitledKind::Issue {
                state: row.get("state")?,
            },
        })
    });
    titled?.collect()
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
    let rows = statement.query(named_params! { ":repository": repository })?;
    let mut found = issues_in(rows)?;
    let assignees = assignees(connection, repository)?;
    let labels = labels(connection, repository)?;
    fill(&mut found, assignees, labels);
    Ok(found)
}

/// Which of a Repository's Issues a read takes, besides all of them.
#[derive(Debug, Clone, Copy)]
pub(super) enum IssueScope<'a> {
    /// The Issues with these numbers, whatever their state, in number order.
    Numbered(&'a BTreeSet<u32>),
    /// The open Issues updated at `since` or later, the latest update first.
    OpenSince(OffsetDateTime),
    /// The `limit` open Issues that follow `updated_at` and `number` in the order of the latest update
    /// first, then the highest number first.
    OpenBefore {
        updated_at: OffsetDateTime,
        number: u32,
        limit: u32,
    },
}

/// The Issues of `repository` that `scope` takes, as much of each as `detail` says. Each statement reads only
/// the rows of those Issues.
pub(super) fn read(
    connection: &Connection,
    repository: &RepositoryName,
    scope: IssueScope<'_>,
    detail: Detail,
) -> rusqlite::Result<Vec<IssueFacts>> {
    const COLUMNS: &str = "SELECT number, title, author, updated_at, state FROM issues";
    let mut found = match scope {
        IssueScope::Numbered(numbers) => {
            let query = format!(
                "{COLUMNS} WHERE repository = :repository
                     AND number IN (SELECT value FROM json_each(:numbers))
                 ORDER BY number"
            );
            let mut statement = connection.prepare(&query)?;
            let numbers = Numbers::of(numbers.iter().copied());
            let asked = named_params! { ":repository": repository, ":numbers": numbers };
            issues_in(statement.query(asked)?)?
        }
        IssueScope::OpenSince(since) => {
            let query = format!(
                "{COLUMNS} WHERE repository = :repository AND state = 'OPEN' AND updated_at >= :since
                 ORDER BY updated_at DESC, number DESC"
            );
            let mut statement = connection.prepare(&query)?;
            let since = since.unix_timestamp();
            let asked = named_params! { ":repository": repository, ":since": since };
            issues_in(statement.query(asked)?)?
        }
        IssueScope::OpenBefore {
            updated_at,
            number,
            limit,
        } => {
            let query = format!(
                "{COLUMNS} WHERE repository = :repository AND state = 'OPEN'
                     AND (updated_at, number) < (:updated_at, :number)
                 ORDER BY updated_at DESC, number DESC
                 LIMIT :limit"
            );
            let mut statement = connection.prepare(&query)?;
            let asked = named_params! {
                ":repository": repository,
                ":updated_at": updated_at.unix_timestamp(),
                ":number": number,
                ":limit": limit,
            };
            issues_in(statement.query(asked)?)?
        }
    };
    if detail == Detail::Links {
        return Ok(found);
    }
    let numbers = Numbers::of(found.iter().map(|issue| issue.number));
    let of_found = named_params! { ":repository": repository, ":numbers": numbers };
    let assignees = values_of(connection, ("issue_assignees", "login"), of_found)?;
    let labels = values_of(connection, ("issue_labels", "name"), of_found)?;
    fill(&mut found, assignees, labels);
    Ok(found)
}

/// The Issues in `rows`, as yet with no assignees and no labels.
fn issues_in(mut rows: rusqlite::Rows<'_>) -> rusqlite::Result<Vec<IssueFacts>> {
    let mut found = Vec::new();
    while let Some(row) = rows.next()? {
        let UnixTime(updated_at) = row.get("updated_at")?;
        found.push(IssueFacts {
            number: row.get("number")?,
            title: row.get("title")?,
            author: row.get("author")?,
            assignees: Vec::new(),
            labels: Vec::new(),
            updated_at,
            state: row.get("state")?,
        });
    }
    Ok(found)
}

fn fill(
    issues: &mut [IssueFacts],
    mut assignees: BTreeMap<u32, Vec<Login>>,
    mut labels: BTreeMap<u32, Vec<Label>>,
) {
    for issue in issues {
        issue.assignees = assignees.remove(&issue.number).unwrap_or_default();
        issue.labels = labels.remove(&issue.number).unwrap_or_default();
    }
}

/// The column of each row of a table, both named in `table_column`, that belongs to an Issue of the
/// `:repository` and `:numbers` that `asked` holds, by number, in the column's order, ignoring case.
fn values_of<T: rusqlite::types::FromSql>(
    connection: &Connection,
    (table, column): (&str, &str),
    asked: &[(&str, &dyn rusqlite::ToSql)],
) -> rusqlite::Result<BTreeMap<u32, Vec<T>>> {
    let query = format!(
        "SELECT number, {column} FROM {table}
         WHERE repository = :repository AND number IN (SELECT value FROM json_each(:numbers))
         ORDER BY number, {column}"
    );
    pull_requests::values_by_number(connection.prepare(&query)?, asked, column)
}

#[cfg(test)]
mod tests;
