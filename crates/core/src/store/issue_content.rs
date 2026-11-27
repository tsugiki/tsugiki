//! The Issue view's last read of an Issue, so it opens at once, and with GitHub down (ADR 0015). Each read
//! replaces the last. These rows hold long HTML, so unlike the other tables they keep a rowid: SQLite advises
//! against `WITHOUT ROWID` for large rows.

use rusqlite::{OptionalExtension, Row, Transaction, named_params};
use time::OffsetDateTime;

use super::Connection;
use super::columns::{EventColumn, NodeColumn, UnixTime};
use crate::facts::issue_content::{Comment, IssueComment, IssueContent, IssueEvent};
use crate::facts::pull_requests::NodeId;
use crate::repository::RepositoryName;

pub(super) const SCHEMA: &str = "
    CREATE TABLE issue_content (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        body_html TEXT NOT NULL,
        earlier_comments INTEGER NOT NULL,
        PRIMARY KEY (repository, number)
    );
    CREATE TABLE issue_comments (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        position INTEGER NOT NULL,
        author TEXT,
        created_at INTEGER NOT NULL,
        body_html TEXT NOT NULL,
        PRIMARY KEY (repository, number, position),
        FOREIGN KEY (repository, number) REFERENCES issue_content (repository, number)
    );
";

/// Adds the time each Issue and PR was opened. A read kept from before this step lacks it until the next
/// read (#595).
pub(super) const OPENED: &str = "ALTER TABLE issue_content ADD COLUMN opened_at INTEGER;";

/// Adds GitHub's ID of each Issue and PR, which a write names it by. A read kept from before this step lacks
/// it until the next read (#914).
pub(super) const NODE_IDS: &str = "ALTER TABLE issue_content ADD COLUMN node_id TEXT;";

/// Adds the events the Issue view shows and the count of older ones GitHub holds (#1067). A read kept from
/// before this step shows none until the next read.
pub(super) const EVENTS: &str = "
    ALTER TABLE issue_content ADD COLUMN earlier_events INTEGER NOT NULL DEFAULT 0;
    CREATE TABLE issue_events (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        position INTEGER NOT NULL,
        event TEXT NOT NULL,
        PRIMARY KEY (repository, number, position),
        FOREIGN KEY (repository, number) REFERENCES issue_content (repository, number)
    );
";

/// Replaces what's kept of Issue `number` with `content`, in one transaction.
pub(super) fn save(
    connection: &mut Connection,
    repository: &RepositoryName,
    number: u32,
    content: &IssueContent,
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    save_in(&transaction, repository, number, content)?;
    transaction.commit()
}

/// Replaces what's kept of item `number` with `content`, in `transaction`.
pub(super) fn save_in(
    transaction: &Transaction<'_>,
    repository: &RepositoryName,
    number: u32,
    content: &IssueContent,
) -> rusqlite::Result<()> {
    save_body(transaction, repository, number, content)?;
    forget_comments(transaction, repository, number)?;
    forget_events(transaction, repository, number)?;
    for (position, comment) in (0_u32..).zip(&content.comments) {
        save_comment(transaction, repository, (number, position), comment)?;
    }
    for (position, event) in (0_u32..).zip(&content.events) {
        save_event(transaction, repository, (number, position), event)?;
    }
    Ok(())
}

fn save_body(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    content: &IssueContent,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT INTO issue_content
                 (repository, number, node_id, body_html, earlier_comments, earlier_events, opened_at)
             VALUES (:repository, :number, :node_id, :body_html, :earlier_comments, :earlier_events,
                     :opened_at)
             ON CONFLICT (repository, number) DO UPDATE SET
                 node_id = excluded.node_id,
                 body_html = excluded.body_html,
                 earlier_comments = excluded.earlier_comments,
                 earlier_events = excluded.earlier_events,
                 opened_at = excluded.opened_at",
            named_params! {
                ":repository": repository,
                ":number": number,
                ":node_id": content.id.clone().map(NodeColumn),
                ":body_html": content.body_html,
                ":earlier_comments": content.earlier_comments,
                ":earlier_events": content.earlier_events,
                ":opened_at": content.opened_at.map(OffsetDateTime::unix_timestamp),
            },
        )
        .map(drop)
}

fn forget_comments(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "DELETE FROM issue_comments WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number },
        )
        .map(drop)
}

fn forget_events(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "DELETE FROM issue_events WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number },
        )
        .map(drop)
}

fn save_event(
    connection: &Connection,
    repository: &RepositoryName,
    (number, position): (u32, u32),
    event: &IssueEvent,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT INTO issue_events (repository, number, position, event)
             VALUES (:repository, :number, :position, :event)",
            named_params! {
                ":repository": repository,
                ":number": number,
                ":position": position,
                ":event": EventColumn(event.clone()),
            },
        )
        .map(drop)
}

fn save_comment(
    connection: &Connection,
    repository: &RepositoryName,
    (number, position): (u32, u32),
    IssueComment { id, comment }: &IssueComment,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT INTO issue_comments
                 (repository, number, position, database_id, author, created_at, body_html, hidden)
             VALUES (:repository, :number, :position, :database_id, :author, :created_at, :body_html,
                     :hidden)",
            named_params! {
                ":repository": repository,
                ":number": number,
                ":position": position,
                ":database_id": id,
                ":author": comment.author,
                ":created_at": comment.created_at.unix_timestamp(),
                ":body_html": comment.body_html,
                ":hidden": comment.hidden,
            },
        )
        .map(drop)
}

fn comment_in(row: &Row<'_>) -> rusqlite::Result<IssueComment> {
    let UnixTime(created_at) = row.get("created_at")?;
    Ok(IssueComment {
        id: row.get("database_id")?,
        comment: Comment {
            author: row.get("author")?,
            created_at,
            body_html: row.get("body_html")?,
            hidden: row.get("hidden")?,
        },
    })
}

fn event_in(row: &Row<'_>) -> rusqlite::Result<IssueEvent> {
    Ok(row.get::<_, EventColumn>("event")?.0)
}

const SELECT_BODY: &str =
    "SELECT node_id, body_html, earlier_comments, earlier_events, opened_at FROM issue_content
     WHERE repository = :repository AND number = :number";
const SELECT_COMMENTS: &str =
    "SELECT database_id, author, created_at, body_html, hidden FROM issue_comments
     WHERE repository = :repository AND number = :number ORDER BY position";
const SELECT_EVENTS: &str = "SELECT event FROM issue_events
                             WHERE repository = :repository AND number = :number ORDER BY position";

/// The content kept for Issue `number`. Nothing, until Tsugiki has read it once.
pub(super) fn read(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<Option<IssueContent>> {
    let asked = named_params! { ":repository": repository, ":number": number };
    let mut statement = connection.prepare(SELECT_BODY)?;
    let mut found = statement.query(asked)?;
    let Some(row) = found.next()? else {
        return Ok(None);
    };
    let id: Option<NodeColumn> = row.get("node_id")?;
    let body_html = row.get("body_html")?;
    let earlier_comments = row.get("earlier_comments")?;
    let earlier_events = row.get("earlier_events")?;
    let opened_at: Option<UnixTime> = row.get("opened_at")?;
    let mut statement = connection.prepare(SELECT_COMMENTS)?;
    let comments = statement.query_map(asked, comment_in)?;
    let mut statement = connection.prepare(SELECT_EVENTS)?;
    let events = statement.query_map(asked, event_in)?;
    Ok(Some(IssueContent {
        id: id.map(|NodeColumn(id)| id),
        body_html,
        comments: comments.collect::<rusqlite::Result<_>>()?,
        earlier_comments,
        events: events.collect::<rusqlite::Result<_>>()?,
        earlier_events,
        opened_at: opened_at.map(|UnixTime(at)| at),
    }))
}

/// GitHub's ID of Issue or PR `number` of `repository`, from its last read. Nothing, before Tsugiki has read
/// it, or for a read kept from before Tsugiki asked.
pub(super) fn node_id(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<Option<NodeId>> {
    let query =
        "SELECT node_id FROM issue_content WHERE repository = :repository AND number = :number";
    let asked = named_params! { ":repository": repository, ":number": number };
    let found: Option<Option<NodeColumn>> = connection
        .query_row(query, asked, |row| row.get(0))
        .optional()?;
    Ok(found.flatten().map(|NodeColumn(id)| id))
}

#[cfg(test)]
mod tests;
