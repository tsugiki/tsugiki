//! The PR view's last read of a PR's Reviews and their thread lines, beside its description and comments in
//! `issue_content`. Each read replaces the last, as there.

use std::collections::BTreeMap;

use rusqlite::{Connection, Row, Transaction, named_params};

use super::columns::UnixTime;
use super::issue_content;
use crate::github::CommitSha;
use crate::github::issue_content::IssueComment;
use crate::github::pull_request_content::{
    DiffHunk, Opening, PullRequestContent, Review, Side, Subject, ThreadComment, ThreadId,
    ThreadLine,
};
use crate::github::pull_requests::ReviewState;
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

/// Each thread's id, its place in the diff and its first 100 comments, which Files shows under the code
/// (#184), and the head their lines count in. A thread with no `side` is on the whole file. A PR read before
/// this has no Reviews until the next read, as the threads it kept can't gain their comments.
pub(super) const THREAD_COMMENTS: &str = "
    ALTER TABLE issue_content ADD COLUMN head_sha TEXT;
    DROP TABLE review_threads;
    DELETE FROM pr_reviews;
    CREATE TABLE review_threads (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        review INTEGER NOT NULL,
        position INTEGER NOT NULL,
        node_id TEXT NOT NULL,
        path TEXT NOT NULL,
        side TEXT,
        line INTEGER,
        original_line INTEGER,
        words TEXT,
        hidden TEXT,
        replies INTEGER NOT NULL,
        resolution TEXT NOT NULL,
        anchor TEXT NOT NULL,
        PRIMARY KEY (repository, number, review, position),
        FOREIGN KEY (repository, number, review) REFERENCES pr_reviews (repository, number, position)
    ) WITHOUT ROWID;
    CREATE TABLE review_thread_comments (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        review INTEGER NOT NULL,
        thread INTEGER NOT NULL,
        position INTEGER NOT NULL,
        author TEXT,
        created_at INTEGER NOT NULL,
        body_html TEXT NOT NULL,
        hidden TEXT,
        PRIMARY KEY (repository, number, review, thread, position),
        FOREIGN KEY (repository, number, review, thread)
            REFERENCES review_threads (repository, number, review, position)
    ) WITHOUT ROWID;
";

/// The hunk GitHub stored with each thread, and the user's Pending review (#185): a Review row in state
/// `PENDING`, after the others, holding the threads it started. A draft reply in another thread is a
/// comment there. A PR read before this shows its Outdated threads without their code until the next read.
pub(super) const PENDING_REVIEW: &str = "
    ALTER TABLE review_threads ADD COLUMN diff_hunk TEXT;
    ALTER TABLE review_thread_comments ADD COLUMN submission TEXT NOT NULL DEFAULT 'submitted';
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
        "DELETE FROM review_thread_comments WHERE repository = :repository AND number = :number",
        asked,
    )?;
    let _ = transaction.execute(
        "DELETE FROM review_threads WHERE repository = :repository AND number = :number",
        asked,
    )?;
    let _ = transaction.execute(
        "DELETE FROM pr_reviews WHERE repository = :repository AND number = :number",
        asked,
    )?;
    let _ = transaction.execute(
        "UPDATE issue_content SET earlier_reviews = :earlier, head_sha = :head
         WHERE repository = :repository AND number = :number",
        named_params! {
            ":repository": repository,
            ":number": number,
            ":earlier": content.earlier_reviews,
            ":head": content.head,
        },
    )?;
    let mut position = 0_u32;
    for review in &content.reviews {
        save_review(&transaction, repository, (number, position), review)?;
        position += 1;
    }
    if !content.pending.is_empty() {
        let _ = transaction.execute(
            "INSERT INTO pr_reviews (repository, number, position, state, submitted_at, body_html)
             VALUES (:repository, :number, :position, :state, 0, '')",
            named_params! {
                ":repository": repository,
                ":number": number,
                ":position": position,
                ":state": ReviewState::Pending,
            },
        )?;
        let pending = (number, position);
        save_threads(&transaction, repository, pending, &content.pending)?;
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
    save_threads(transaction, repository, (number, position), &review.threads)
}

/// Keeps `threads`, which Review `review` started.
fn save_threads(
    transaction: &Transaction<'_>,
    repository: &RepositoryName,
    (number, review): (u32, u32),
    threads: &[ThreadLine],
) -> rusqlite::Result<()> {
    for (at, thread) in (0_u32..).zip(threads) {
        save_thread(transaction, repository, (number, review, at), thread)?;
    }
    Ok(())
}

fn save_thread(
    transaction: &Transaction<'_>,
    repository: &RepositoryName,
    (number, review, position): (u32, u32, u32),
    thread: &ThreadLine,
) -> rusqlite::Result<()> {
    let (words, hidden) = match &thread.opening {
        Opening::Words { words } => (Some(words), None),
        Opening::Hidden { reason } => (None, Some(reason)),
    };
    let (side, line, original) = match thread.subject {
        Subject::File => (None, None, None),
        Subject::Line {
            side,
            line,
            original,
        } => (Some(side), line, original),
    };
    let _ = transaction.execute(
        "INSERT INTO review_threads (repository, number, review, position, node_id, path, side, line,
                                     original_line, words, hidden, replies, resolution, anchor, diff_hunk)
         VALUES (:repository, :number, :review, :position, :node_id, :path, :side, :line, :original,
                 :words, :hidden, :replies, :resolution, :anchor, :diff_hunk)",
        named_params! {
            ":repository": repository,
            ":number": number,
            ":review": review,
            ":position": position,
            ":node_id": thread.id.as_str(),
            ":path": thread.path,
            ":side": side,
            ":line": line,
            ":original": original,
            ":words": words,
            ":hidden": hidden,
            ":replies": thread.replies,
            ":resolution": thread.resolution,
            ":anchor": thread.anchor,
            ":diff_hunk": thread.hunk.as_ref().map(DiffHunk::as_str),
        },
    )?;
    for (
        at,
        ThreadComment {
            comment,
            submission,
        },
    ) in (0_u32..).zip(&thread.comments)
    {
        let _ = transaction.execute(
            "INSERT INTO review_thread_comments (repository, number, review, thread, position, author,
                                                 created_at, body_html, hidden, submission)
             VALUES (:repository, :number, :review, :thread, :position, :author, :created_at, :body_html,
                     :hidden, :submission)",
            named_params! {
                ":repository": repository,
                ":number": number,
                ":review": review,
                ":thread": position,
                ":position": at,
                ":author": comment.author,
                ":created_at": comment.created_at.unix_timestamp(),
                ":body_html": comment.body_html,
                ":hidden": comment.hidden,
                ":submission": submission,
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

fn thread_in(row: &Row<'_>) -> rusqlite::Result<((u32, u32), ThreadLine)> {
    let opening = match row.get("hidden")? {
        Some(reason) => Opening::Hidden { reason },
        None => Opening::Words {
            words: row.get::<_, Option<String>>("words")?.unwrap_or_default(),
        },
    };
    let subject = match row.get::<_, Option<Side>>("side")? {
        None => Subject::File,
        Some(side) => Subject::Line {
            side,
            line: row.get("line")?,
            original: row.get("original_line")?,
        },
    };
    let thread = ThreadLine {
        id: ThreadId::new(row.get_ref("node_id")?.as_str()?),
        path: row.get("path")?,
        opening,
        replies: row.get("replies")?,
        resolution: row.get("resolution")?,
        anchor: row.get("anchor")?,
        subject,
        comments: Vec::new(),
        hunk: row
            .get_ref("diff_hunk")?
            .as_str_or_null()?
            .and_then(DiffHunk::parse),
    };
    Ok(((row.get("review")?, row.get("position")?), thread))
}

fn comment_in(row: &Row<'_>) -> rusqlite::Result<((u32, u32), ThreadComment)> {
    let UnixTime(created_at) = row.get("created_at")?;
    let comment = ThreadComment {
        comment: IssueComment {
            author: row.get("author")?,
            created_at,
            body_html: row.get("body_html")?,
            hidden: row.get("hidden")?,
        },
        submission: row.get("submission")?,
    };
    Ok(((row.get("review")?, row.get("thread")?), comment))
}

fn earlier_in(row: &Row<'_>) -> rusqlite::Result<(u32, Option<CommitSha>)> {
    Ok((row.get("earlier_reviews")?, row.get("head_sha")?))
}

const SELECT_EARLIER: &str = "SELECT earlier_reviews, head_sha FROM issue_content
                              WHERE repository = :repository AND number = :number";

const SELECT_REVIEWS: &str =
    "SELECT position, author, state, commit_sha, submitted_at, body_html, hidden FROM pr_reviews
     WHERE repository = :repository AND number = :number ORDER BY position";

const SELECT_THREADS: &str =
    "SELECT review, position, node_id, path, side, line, original_line, words, hidden, replies, resolution,
            anchor, diff_hunk FROM review_threads
     WHERE repository = :repository AND number = :number ORDER BY review, position";

const SELECT_COMMENTS: &str =
    "SELECT review, thread, author, created_at, body_html, hidden, submission FROM review_thread_comments
     WHERE repository = :repository AND number = :number ORDER BY review, thread, position";

/// The content kept for PR `number`, oldest Review first, with the threads of the user's Pending review.
/// Nothing, until Tsugiki has read it once.
pub(super) fn read(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<Option<PullRequestContent>> {
    let Some(conversation) = issue_content::read(connection, repository, number)? else {
        return Ok(None);
    };
    let asked = named_params! { ":repository": repository, ":number": number };
    let (earlier_reviews, head) = connection.query_row(SELECT_EARLIER, asked, earlier_in)?;
    let mut comments: BTreeMap<(u32, u32), Vec<ThreadComment>> = BTreeMap::new();
    let mut statement = connection.prepare(SELECT_COMMENTS)?;
    for read in statement.query_map(asked, comment_in)? {
        let (thread, comment) = read?;
        comments.entry(thread).or_default().push(comment);
    }
    let mut threads: BTreeMap<u32, Vec<ThreadLine>> = BTreeMap::new();
    let mut statement = connection.prepare(SELECT_THREADS)?;
    for read in statement.query_map(asked, thread_in)? {
        let ((review, position), mut thread) = read?;
        thread.comments = comments.remove(&(review, position)).unwrap_or_default();
        threads.entry(review).or_default().push(thread);
    }
    let mut statement = connection.prepare(SELECT_REVIEWS)?;
    let mut reviews = Vec::new();
    let mut pending = Vec::new();
    for read in statement.query_map(asked, review_in)? {
        let (position, mut review) = read?;
        let started = threads.remove(&position).unwrap_or_default();
        if review.state == ReviewState::Pending {
            pending = started;
        } else {
            review.threads = started;
            reviews.push(review);
        }
    }
    Ok(Some(PullRequestContent {
        conversation,
        reviews,
        earlier_reviews,
        head,
        pending,
    }))
}
