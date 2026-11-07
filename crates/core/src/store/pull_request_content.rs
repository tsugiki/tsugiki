//! The PR view's last read of a PR's Reviews and their thread lines, beside its description and comments in
//! `issue_content`. Each read replaces the last, as there.

use std::collections::BTreeMap;

use rusqlite::{Connection, Row, Transaction, named_params};

use super::columns::UnixTime;
use super::issue_content;
use crate::github::pull_request_content::{Opening, PullRequestContent, Review, ThreadLine};
use crate::repository::RepositoryName;

/// The Reviews, and a line for each thread one started (#156). A comment hidden on GitHub keeps its reason.
/// A PR read before this has no Reviews until the next read.
pub(super) const SCHEMA: &str = "
    ALTER TABLE issue_comments ADD COLUMN hidden TEXT;
    ALTER TABLE issue_content ADD COLUMN earlier_reviews INTEGER NOT NULL DEFAULT 0;
    CREATE TABLE pr_reviews (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        position INTEGER NOT NULL,
        author TEXT,
        state TEXT NOT NULL,
        commit_sha TEXT,
        submitted_at INTEGER NOT NULL,
        body_html TEXT NOT NULL,
        hidden TEXT,
        PRIMARY KEY (repository, number, position),
        FOREIGN KEY (repository, number) REFERENCES issue_content (repository, number)
    );
    CREATE TABLE review_threads (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        review INTEGER NOT NULL,
        position INTEGER NOT NULL,
        path TEXT NOT NULL,
        line INTEGER,
        words TEXT,
        hidden TEXT,
        replies INTEGER NOT NULL,
        resolution TEXT NOT NULL,
        anchor TEXT NOT NULL,
        PRIMARY KEY (repository, number, review, position),
        FOREIGN KEY (repository, number, review) REFERENCES pr_reviews (repository, number, position)
    ) WITHOUT ROWID;
";

/// Replaces what's kept of PR `number` with `content`, in one transaction.
pub(super) fn save(
    connection: &mut Connection,
    repository: &RepositoryName,
    number: u32,
    content: &PullRequestContent,
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    issue_content::save_in(&transaction, repository, number, &content.conversation)?;
    let asked = named_params! { ":repository": repository, ":number": number };
    let _ = transaction.execute(
        "DELETE FROM review_threads WHERE repository = :repository AND number = :number",
        asked,
    )?;
    let _ = transaction.execute(
        "DELETE FROM pr_reviews WHERE repository = :repository AND number = :number",
        asked,
    )?;
    let _ = transaction.execute(
        "UPDATE issue_content SET earlier_reviews = :earlier
         WHERE repository = :repository AND number = :number",
        named_params! {
            ":repository": repository,
            ":number": number,
            ":earlier": content.earlier_reviews,
        },
    )?;
    for (position, review) in (0_u32..).zip(&content.reviews) {
        save_review(&transaction, repository, (number, position), review)?;
    }
    transaction.commit()
}

fn save_review(
    transaction: &Transaction<'_>,
    repository: &RepositoryName,
    (number, position): (u32, u32),
    review: &Review,
) -> rusqlite::Result<()> {
    let _ = transaction.execute(
        "INSERT INTO pr_reviews
             (repository, number, position, author, state, commit_sha, submitted_at, body_html, hidden)
         VALUES (:repository, :number, :position, :author, :state, :commit, :submitted_at, :body_html,
                 :hidden)",
        named_params! {
            ":repository": repository,
            ":number": number,
            ":position": position,
            ":author": review.author,
            ":state": review.state,
            ":commit": review.commit,
            ":submitted_at": review.submitted_at.unix_timestamp(),
            ":body_html": review.body_html,
            ":hidden": review.hidden,
        },
    )?;
    for (at, thread) in (0_u32..).zip(&review.threads) {
        let (words, hidden) = match &thread.opening {
            Opening::Words { words } => (Some(words), None),
            Opening::Hidden { reason } => (None, Some(reason)),
        };
        let _ = transaction.execute(
            "INSERT INTO review_threads (repository, number, review, position, path, line, words, hidden,
                                         replies, resolution, anchor)
             VALUES (:repository, :number, :review, :position, :path, :line, :words, :hidden, :replies,
                     :resolution, :anchor)",
            named_params! {
                ":repository": repository,
                ":number": number,
                ":review": position,
                ":position": at,
                ":path": thread.path,
                ":line": thread.line,
                ":words": words,
                ":hidden": hidden,
                ":replies": thread.replies,
                ":resolution": thread.resolution,
                ":anchor": thread.anchor,
            },
        )?;
    }
    Ok(())
}

fn review_in(row: &Row<'_>) -> rusqlite::Result<(u32, Review)> {
    let UnixTime(submitted_at) = row.get("submitted_at")?;
    let review = Review {
        author: row.get("author")?,
        state: row.get("state")?,
        commit: row.get("commit_sha")?,
        submitted_at,
        body_html: row.get("body_html")?,
        hidden: row.get("hidden")?,
        threads: Vec::new(),
    };
    Ok((row.get("position")?, review))
}

fn thread_in(row: &Row<'_>) -> rusqlite::Result<(u32, ThreadLine)> {
    let opening = match row.get("hidden")? {
        Some(reason) => Opening::Hidden { reason },
        None => Opening::Words {
            words: row.get::<_, Option<String>>("words")?.unwrap_or_default(),
        },
    };
    let thread = ThreadLine {
        path: row.get("path")?,
        line: row.get("line")?,
        opening,
        replies: row.get("replies")?,
        resolution: row.get("resolution")?,
        anchor: row.get("anchor")?,
    };
    Ok((row.get("review")?, thread))
}

const SELECT_EARLIER: &str =
    "SELECT earlier_reviews FROM issue_content WHERE repository = :repository AND number = :number";

const SELECT_REVIEWS: &str =
    "SELECT position, author, state, commit_sha, submitted_at, body_html, hidden FROM pr_reviews
     WHERE repository = :repository AND number = :number ORDER BY position";

const SELECT_THREADS: &str =
    "SELECT review, path, line, words, hidden, replies, resolution, anchor FROM review_threads
     WHERE repository = :repository AND number = :number ORDER BY review, position";

/// The content kept for PR `number`, oldest Review first. Nothing, until Tsugiki has read it once.
pub(super) fn read(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<Option<PullRequestContent>> {
    let Some(conversation) = issue_content::read(connection, repository, number)? else {
        return Ok(None);
    };
    let asked = named_params! { ":repository": repository, ":number": number };
    let earlier_reviews =
        connection.query_row(SELECT_EARLIER, asked, |row| row.get("earlier_reviews"))?;
    let mut threads: BTreeMap<u32, Vec<ThreadLine>> = BTreeMap::new();
    let mut statement = connection.prepare(SELECT_THREADS)?;
    for read in statement.query_map(asked, thread_in)? {
        let (review, thread) = read?;
        threads.entry(review).or_default().push(thread);
    }
    let mut statement = connection.prepare(SELECT_REVIEWS)?;
    let reviews = statement.query_map(asked, review_in)?.map(|read| {
        let (position, mut review) = read?;
        review.threads = threads.remove(&position).unwrap_or_default();
        Ok(review)
    });
    let reviews = reviews.collect::<rusqlite::Result<_>>()?;
    Ok(Some(PullRequestContent {
        conversation,
        reviews,
        earlier_reviews,
    }))
}
