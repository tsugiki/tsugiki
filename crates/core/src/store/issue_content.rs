//! The Issue view's last read of an Issue, so it opens at once, and with GitHub down (ADR 0015). Each read
//! replaces the last. These rows hold long HTML, so unlike the other tables they keep a rowid: SQLite advises
//! against `WITHOUT ROWID` for large rows.

use rusqlite::{Row, Transaction, named_params};
use time::OffsetDateTime;

use super::Connection;
use super::columns::UnixTime;
use crate::github::issue_content::{IssueComment, IssueContent};
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
    for (position, comment) in (0_u32..).zip(&content.comments) {
        save_comment(transaction, repository, (number, position), comment)?;
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
            "INSERT INTO issue_content (repository, number, body_html, earlier_comments, opened_at)
             VALUES (:repository, :number, :body_html, :earlier_comments, :opened_at)
             ON CONFLICT (repository, number) DO UPDATE SET
                 body_html = excluded.body_html,
                 earlier_comments = excluded.earlier_comments,
                 opened_at = excluded.opened_at",
            named_params! {
                ":repository": repository,
                ":number": number,
                ":body_html": content.body_html,
                ":earlier_comments": content.earlier_comments,
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

fn save_comment(
    connection: &Connection,
    repository: &RepositoryName,
    (number, position): (u32, u32),
    comment: &IssueComment,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT INTO issue_comments
                 (repository, number, position, author, created_at, body_html, hidden)
             VALUES (:repository, :number, :position, :author, :created_at, :body_html, :hidden)",
            named_params! {
                ":repository": repository,
                ":number": number,
                ":position": position,
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
        author: row.get("author")?,
        created_at,
        body_html: row.get("body_html")?,
        hidden: row.get("hidden")?,
    })
}

const SELECT_BODY: &str = "SELECT body_html, earlier_comments, opened_at FROM issue_content
     WHERE repository = :repository AND number = :number";
const SELECT_COMMENTS: &str = "SELECT author, created_at, body_html, hidden FROM issue_comments
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
    let body_html = row.get("body_html")?;
    let earlier_comments = row.get("earlier_comments")?;
    let opened_at: Option<UnixTime> = row.get("opened_at")?;
    let mut statement = connection.prepare(SELECT_COMMENTS)?;
    let comments = statement.query_map(asked, comment_in)?;
    Ok(Some(IssueContent {
        body_html,
        comments: comments.collect::<rusqlite::Result<_>>()?,
        earlier_comments,
        opened_at: opened_at.map(|UnixTime(at)| at),
    }))
}

#[cfg(test)]
mod tests;
