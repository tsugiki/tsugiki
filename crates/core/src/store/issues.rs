//! The Issues that PRs name, as facts (ADR 0010). The link from a PR to an Issue isn't stored: the core works
//! it out from a PR's facts when a screen asks (ADR 0011).

use std::collections::BTreeMap;

use rusqlite::{ToSql, named_params};
use time::OffsetDateTime;

use super::columns::{Items, Numbers, UnixTime};
use super::{Connection, TitledItem, TitledKind};
use super::{activity, pull_requests};
use crate::facts::activity::ItemActivity;
use crate::facts::issues::{IssueFacts, IssuePlace, IssueState};
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
    read(connection, repository, Items::All)
}

/// The Issues of `repository` that `items` names, in number order. Each statement reads only the rows of
/// those Issues.
pub(super) fn read(
    connection: &Connection,
    repository: &RepositoryName,
    items: Items<'_>,
) -> rusqlite::Result<Vec<IssueFacts>> {
    let condition = items.condition("number");
    let query = format!(
        "SELECT number, title, author, updated_at, state FROM issues
         WHERE repository = :repository {condition}
         ORDER BY number"
    );
    let mut statement = connection.prepare(&query)?;
    let asked = items.with(vec![(":repository", repository)]);
    let mut rows = statement.query(asked.as_slice())?;
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
    // In login order, which the Issue view's author line shows them in.
    let of = (repository, items);
    let mut assignees: BTreeMap<u32, Vec<Login>> =
        values_of(connection, of, ("issue_assignees", "login"))?;
    // In name order, ignoring case, as `IssueFacts` holds them.
    let mut labels: BTreeMap<u32, Vec<Label>> =
        values_of(connection, of, ("issue_labels", "name"))?;
    for issue in &mut found {
        issue.assignees = assignees.remove(&issue.number).unwrap_or_default();
        issue.labels = labels.remove(&issue.number).unwrap_or_default();
    }
    Ok(found)
}

/// The `column` of each row of `table` that belongs to an Issue `items` names, by number, each Issue's in
/// the column's order.
fn values_of<T: rusqlite::types::FromSql>(
    connection: &Connection,
    (repository, items): (&RepositoryName, Items<'_>),
    (table, column): (&str, &str),
) -> rusqlite::Result<BTreeMap<u32, Vec<T>>> {
    let condition = items.condition("number");
    let query = format!(
        "SELECT number, {column} FROM {table} WHERE repository = :repository {condition}
         ORDER BY number, {column}"
    );
    let asked = items.with(vec![(":repository", repository)]);
    pull_requests::values_by_number(connection.prepare(&query)?, &asked, column)
}

/// The state a read of the open Issues binds as `:open`.
const OPEN: IssueState = IssueState::Open;

/// Which Issues a read of their places takes.
#[derive(Clone, Copy)]
pub(super) enum PlaceScope<'a> {
    /// The Issues with these numbers, whatever their state, in number order.
    Numbered(&'a Numbers),
    /// The open Issues updated at `since` or later, other than those `besides` lists, the latest update
    /// first.
    OpenSince {
        since: OffsetDateTime,
        besides: &'a Numbers,
    },
    /// The `limit` open Issues that follow `after`, a last update and a number, in the order of the latest
    /// update first and then the highest number first, other than those `besides` lists.
    OpenAfter {
        after: (OffsetDateTime, u32),
        limit: u32,
        besides: &'a Numbers,
    },
}

/// Where each Issue of `repository` that `scope` takes stands in a list.
pub(super) fn read_places(
    connection: &Connection,
    repository: &RepositoryName,
    scope: PlaceScope<'_>,
) -> rusqlite::Result<Vec<IssuePlace>> {
    const COLUMNS: &str =
        "SELECT number, updated_at, state FROM issues WHERE repository = :repository";
    const OTHERS: &str = "AND number NOT IN (SELECT value FROM json_each(:besides))";
    let open: &dyn ToSql = &OPEN;
    let mut asked: Vec<(&str, &dyn ToSql)> = vec![(":repository", repository)];
    let (since, updated_at, number);
    let query = match scope {
        PlaceScope::Numbered(numbers) => {
            asked.push((":numbers", numbers));
            format!(
                "{COLUMNS} AND number IN (SELECT value FROM json_each(:numbers)) ORDER BY number"
            )
        }
        PlaceScope::OpenSince {
            since: start,
            besides,
        } => {
            since = start.unix_timestamp();
            asked.extend([(":open", open), (":since", &since), (":besides", besides)]);
            format!(
                "{COLUMNS} AND state = :open AND updated_at >= :since {OTHERS}
                 ORDER BY updated_at DESC, number DESC"
            )
        }
        PlaceScope::OpenAfter {
            after,
            ref limit,
            besides,
        } => {
            (updated_at, number) = (after.0.unix_timestamp(), after.1);
            asked.extend([
                (":open", open),
                (":updated_at", &updated_at),
                (":number", &number),
                (":limit", limit),
                (":besides", besides),
            ]);
            format!(
                "{COLUMNS} AND state = :open AND (updated_at, number) < (:updated_at, :number) {OTHERS}
                 ORDER BY updated_at DESC, number DESC
                 LIMIT :limit"
            )
        }
    };
    let mut statement = connection.prepare(&query)?;
    let places = statement.query_map(asked.as_slice(), |row| {
        let UnixTime(updated_at) = row.get("updated_at")?;
        Ok(IssuePlace {
            number: row.get("number")?,
            updated_at,
            state: row.get("state")?,
        })
    });
    places?.collect()
}

#[cfg(test)]
mod tests;
