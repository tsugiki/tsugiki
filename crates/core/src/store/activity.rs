//! The store's part of New activity: Last seen, Marked unseen, the timeline GitHub reported, and when a
//! first sync began. Issues and PRs share their numbers in a Repository, so a number says which item a row is
//! about.

use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, Row, Transaction, named_params};
use time::OffsetDateTime;

use super::Connection;
use super::columns::{Items, UnixTime};
use super::failed_checks;
use crate::facts::activity::{Activity, ActivityFacts, ActivityKind, ItemActivity, ItemKey};
use crate::login::Login;
use crate::repository::RepositoryName;

/// A timeline's events stay after GitHub's answer stops listing them, because an answer only gives the last
/// few. `target` is empty except on a failed Bot backport. `first_syncs` starts from each Repository's last
/// sync, because nothing older is new to someone who already has it.
///
/// `noticed_at` is the time New activity goes by. It's `at`, apart from a commit that turns up on a PR
/// Tsugiki already holds: a commit's date comes from its author's machine and says nothing about when it was
/// pushed, so one made before Last seen and pushed after it would otherwise never count.
pub(super) const SCHEMA: &str = "
    CREATE TABLE last_seen (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        at INTEGER NOT NULL,
        PRIMARY KEY (repository, number)
    ) WITHOUT ROWID;
    CREATE TABLE activity (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        kind TEXT NOT NULL,
        target TEXT NOT NULL,
        who TEXT NOT NULL,
        at INTEGER NOT NULL,
        noticed_at INTEGER NOT NULL,
        PRIMARY KEY (repository, number, kind, target, who, at)
    ) WITHOUT ROWID;
    CREATE TABLE first_syncs (
        repository TEXT NOT NULL COLLATE NOCASE PRIMARY KEY,
        started_at INTEGER NOT NULL
    ) WITHOUT ROWID;
    INSERT INTO first_syncs (repository, started_at) SELECT repository, started_at FROM sync_state;
    ALTER TABLE pull_requests ADD COLUMN ci_finished_at INTEGER;
";

/// The items the user marked unseen, each with the moment of the flag, which Needs attention counts from.
/// Like Last seen, it is the user's own state, kept in Tsugiki only.
pub(super) const MARKED_UNSEEN: &str = "
    CREATE TABLE marked_unseen (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        at INTEGER NOT NULL,
        PRIMARY KEY (repository, number)
    ) WITHOUT ROWID;
";

/// The moment the user last acknowledged what stands on each item: a Review request, a Stale review, failed
/// CI on an Own PR (#865). Mark Seen moves it with Last seen, and the write to GitHub that #395 builds is to
/// move Last seen alone. Null until the user marks the item seen, so what stood before this column still
/// Needs attention.
pub(super) const ACKNOWLEDGED: &str = "ALTER TABLE last_seen ADD COLUMN acknowledged_at INTEGER;";

/// The items the user archived (#490), each under the key that names it, with the moment Archive marked it
/// seen up to. Like Last seen, it is the user's own state, kept in Tsugiki only.
pub(super) const ARCHIVED: &str = "
    CREATE TABLE archived (
        repository TEXT NOT NULL COLLATE NOCASE,
        kind TEXT NOT NULL,
        number INTEGER NOT NULL,
        at INTEGER NOT NULL,
        PRIMARY KEY (repository, kind, number)
    ) WITHOUT ROWID;
";

const ISSUE: &str = "issue";
const PULL_REQUEST: &str = "pull_request";

const COMMIT: &str = "commit";
const COMMENT: &str = "comment";
const REVIEW: &str = "review";
const READY_FOR_REVIEW: &str = "ready_for_review";
const BACKPORT_FAILED: &str = "backport_failed";
const REVIEW_REQUESTED: &str = "review_requested";
const ASSIGNED: &str = "assigned";
const MENTIONED: &str = "mentioned";

/// `kind` as its two columns.
fn columns_of(kind: &ActivityKind) -> (&'static str, &str) {
    match kind {
        ActivityKind::Commit => (COMMIT, ""),
        ActivityKind::Comment => (COMMENT, ""),
        ActivityKind::Review => (REVIEW, ""),
        ActivityKind::ReadyForReview => (READY_FOR_REVIEW, ""),
        ActivityKind::BackportFailed { target } => (BACKPORT_FAILED, target),
        ActivityKind::ReviewRequested => (REVIEW_REQUESTED, ""),
        ActivityKind::Assigned => (ASSIGNED, ""),
        ActivityKind::Mentioned => (MENTIONED, ""),
    }
}

/// Keeps each event in `items` that the store doesn't have yet.
pub(super) fn save(
    connection: &Connection,
    repository: &RepositoryName,
    items: &[ItemActivity],
) -> rusqlite::Result<()> {
    for item in items {
        for event in &item.events {
            save_event(connection, repository, item.number, event)?;
        }
    }
    Ok(())
}

fn save_event(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    event: &Activity,
) -> rusqlite::Result<()> {
    let (kind, target) = columns_of(&event.kind);
    connection
        .execute(
            "INSERT OR IGNORE INTO activity (repository, number, kind, target, who, at, noticed_at)
             VALUES (:repository, :number, :kind, :target, :who, :at, CASE
                 WHEN :kind = 'commit'
                     AND EXISTS (SELECT 1 FROM pull_requests
                                 WHERE repository = :repository AND number = :number)
                     AND EXISTS (SELECT 1 FROM first_syncs WHERE repository = :repository)
                 THEN :now ELSE :at END)",
            named_params! {
                ":repository": repository,
                ":number": number,
                ":kind": kind,
                ":target": target,
                ":who": event.who,
                ":at": event.at.unix_timestamp(),
                ":now": OffsetDateTime::now_utc().unix_timestamp(),
            },
        )
        .map(drop)
}

fn event_in(row: &Row<'_>) -> rusqlite::Result<Option<Activity>> {
    let UnixTime(at) = row.get("at")?;
    let kind: String = row.get("kind")?;
    let kind = match kind.as_str() {
        COMMIT => ActivityKind::Commit,
        COMMENT => ActivityKind::Comment,
        REVIEW => ActivityKind::Review,
        READY_FOR_REVIEW => ActivityKind::ReadyForReview,
        BACKPORT_FAILED => ActivityKind::BackportFailed {
            target: row.get("target")?,
        },
        REVIEW_REQUESTED => ActivityKind::ReviewRequested,
        ASSIGNED => ActivityKind::Assigned,
        MENTIONED => ActivityKind::Mentioned,
        // A later version's kind means nothing to this one, and the row stays for it.
        _ => return Ok(None),
    };
    let who: Login = row.get("who")?;
    Ok(Some(Activity { kind, who, at }))
}

#[cfg(test)]
const SELECT_EVENTS: &str = "SELECT kind, target, who, noticed_at AS at FROM activity
     WHERE repository = :repository AND number = :number ORDER BY noticed_at, kind, target, who";

/// The events kept for item `number`, each at the time New activity goes by, oldest first.
#[cfg(test)]
pub(super) fn read(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<Vec<Activity>> {
    let mut statement = connection.prepare(SELECT_EVENTS)?;
    let asked = named_params! { ":repository": repository, ":number": number };
    let rows = statement.query_map(asked, event_in)?;
    let events: Vec<Option<Activity>> = rows.collect::<rusqlite::Result<_>>()?;
    Ok(events.into_iter().flatten().collect())
}

/// Mark Seen on `numbers`, all at once: clears their Marked unseen, and moves their Last seen and the moment
/// the user acknowledged them on to `up_to`, if it's given. Neither moves back.
pub(super) fn mark_seen(
    connection: &mut Connection,
    repository: &RepositoryName,
    numbers: &[u32],
    up_to: Option<OffsetDateTime>,
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    mark_seen_in(&transaction, repository, numbers, up_to)?;
    transaction.commit()
}

/// Mark Seen on `numbers`, as `mark_seen` says, in a transaction of the caller's.
fn mark_seen_in(
    transaction: &Transaction<'_>,
    repository: &RepositoryName,
    numbers: &[u32],
    up_to: Option<OffsetDateTime>,
) -> rusqlite::Result<()> {
    for number in numbers {
        let item = named_params! { ":repository": repository, ":number": number };
        let _cleared = transaction.execute(
            "DELETE FROM marked_unseen WHERE repository = :repository AND number = :number",
            item,
        )?;
        if let Some(at) = up_to {
            let _moved = transaction.execute(
                "INSERT INTO last_seen (repository, number, at, acknowledged_at)
                 VALUES (:repository, :number, :at, :at)
                 ON CONFLICT (repository, number) DO UPDATE SET
                     at = MAX(at, excluded.at),
                     acknowledged_at = MAX(COALESCE(acknowledged_at, excluded.at), excluded.at)",
                named_params! {
                    ":repository": repository,
                    ":number": number,
                    ":at": at.unix_timestamp(),
                },
            )?;
        }
    }
    Ok(())
}

/// Flags item `number` Marked unseen from `at`, and unarchives it. Marking it again starts the flag afresh.
pub(super) fn mark_unseen(
    connection: &mut Connection,
    repository: &RepositoryName,
    number: u32,
    at: OffsetDateTime,
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    let _flagged = transaction.execute(
        "INSERT INTO marked_unseen (repository, number, at) VALUES (:repository, :number, :at)
         ON CONFLICT (repository, number) DO UPDATE SET at = excluded.at",
        named_params! {
            ":repository": repository,
            ":number": number,
            ":at": at.unix_timestamp(),
        },
    )?;
    let _unarchived = transaction.execute(
        "DELETE FROM archived WHERE repository = :repository AND number = :number",
        named_params! { ":repository": repository, ":number": number },
    )?;
    transaction.commit()
}

/// `key` as its two columns.
fn key_columns(key: ItemKey) -> (&'static str, u32) {
    match key {
        ItemKey::Issue { number } => (ISSUE, number),
        ItemKey::PullRequest { number } => (PULL_REQUEST, number),
    }
}

/// What Archive writes: the item `key` names, and Mark Seen on `numbers`, the item and its tree, up to
/// `up_to` as the window drew it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Archiving {
    pub key: ItemKey,
    pub numbers: Vec<u32>,
    pub up_to: Option<OffsetDateTime>,
}

/// Archive on `archiving`'s item: Mark Seen on its numbers, and the item kept as archived from `up_to`, or
/// from `now` when the window drew no sync, or from a later moment one of its numbers was acknowledged at.
/// Archiving it again never moves that moment back.
pub(super) fn archive(
    connection: &mut Connection,
    repository: &RepositoryName,
    archiving: &Archiving,
    now: OffsetDateTime,
) -> rusqlite::Result<()> {
    let Archiving {
        key,
        numbers,
        up_to,
    } = archiving;
    let transaction = connection.transaction()?;
    mark_seen_in(&transaction, repository, numbers, *up_to)?;
    // A number acknowledged later than `up_to`, as by Mark Seen on another Issue its PR fixes, would
    // otherwise read as acknowledged after the archive, and so as come back.
    let mut at = up_to.unwrap_or(now).unix_timestamp();
    for number in numbers {
        let acknowledged: Option<i64> = transaction
            .query_row(
                "SELECT acknowledged_at FROM last_seen WHERE repository = :repository AND number = :number",
                named_params! { ":repository": repository, ":number": number },
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        at = at.max(acknowledged.unwrap_or(at));
    }
    let (kind, number) = key_columns(*key);
    let _archived = transaction.execute(
        "INSERT INTO archived (repository, kind, number, at) VALUES (:repository, :kind, :number, :at)
         ON CONFLICT (repository, kind, number) DO UPDATE SET at = MAX(at, excluded.at)",
        named_params! {
            ":repository": repository,
            ":kind": kind,
            ":number": number,
            ":at": at,
        },
    )?;
    transaction.commit()
}

/// Unarchives the item `key` names, and leaves its Last seen as it is.
pub(super) fn unarchive(
    connection: &Connection,
    repository: &RepositoryName,
    key: ItemKey,
) -> rusqlite::Result<()> {
    let (kind, number) = key_columns(key);
    connection
        .execute(
            "DELETE FROM archived WHERE repository = :repository AND kind = :kind AND number = :number",
            named_params! { ":repository": repository, ":kind": kind, ":number": number },
        )
        .map(drop)
}

/// The archived items of `items`, each with the moment it was archived from.
fn archived(
    connection: &Connection,
    repository: &RepositoryName,
    items: Items<'_>,
) -> rusqlite::Result<BTreeMap<ItemKey, OffsetDateTime>> {
    let condition = items.condition("number");
    let query =
        format!("SELECT kind, number, at FROM archived WHERE repository = :repository {condition}");
    let mut statement = connection.prepare(&query)?;
    let asked = items.with(vec![(":repository", repository)]);
    let mut rows = statement.query(asked.as_slice())?;
    let mut found = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let kind: String = row.get("kind")?;
        let number: u32 = row.get("number")?;
        let key = match kind.as_str() {
            ISSUE => ItemKey::Issue { number },
            PULL_REQUEST => ItemKey::PullRequest { number },
            // A later version's kind, such as Dependency updates, means nothing to this one.
            _ => continue,
        };
        let UnixTime(at) = row.get("at")?;
        let _replaced = found.insert(key, at);
    }
    Ok(found)
}

/// Remembers when this Repository's first sync began, once.
pub(super) fn remember_first_sync(
    connection: &Connection,
    repository: &RepositoryName,
    started_at: OffsetDateTime,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT OR IGNORE INTO first_syncs (repository, started_at)
             VALUES (:repository, :started_at)",
            named_params! { ":repository": repository, ":started_at": started_at.unix_timestamp() },
        )
        .map(drop)
}

fn moment_in(row: &Row<'_>) -> rusqlite::Result<(u32, OffsetDateTime)> {
    let UnixTime(at) = row.get("at")?;
    Ok((row.get("number")?, at))
}

/// The moment in each row of `query`, which selects `number` and `at`, by number.
fn moments(
    connection: &Connection,
    query: &str,
    repository: &RepositoryName,
    items: Items<'_>,
) -> rusqlite::Result<BTreeMap<u32, OffsetDateTime>> {
    let mut statement = connection.prepare(query)?;
    let asked = items.with(vec![(":repository", repository)]);
    let rows = statement.query_map(asked.as_slice(), moment_in)?;
    rows.collect()
}

/// The events of `items` that `viewer` reads, given when the first sync began.
fn listed_events(
    connection: &Connection,
    repository: &RepositoryName,
    (first_sync, viewer): (Option<OffsetDateTime>, Option<&Login>),
    items: Items<'_>,
) -> rusqlite::Result<BTreeMap<u32, Vec<Activity>>> {
    let condition = items.condition("number");
    let query = format!(
        "SELECT number, kind, target, who, noticed_at AS at FROM activity
         WHERE repository = :repository {condition}
             AND (who = :viewer OR (noticed_at > :first_sync AND number IN (
                 SELECT number FROM pull_requests WHERE repository = :repository AND author = :viewer
                 UNION SELECT number FROM issues WHERE repository = :repository AND author = :viewer
                 UNION SELECT number FROM latest_reviews WHERE repository = :repository AND login = :viewer
                 UNION SELECT number FROM assignees WHERE repository = :repository AND login = :viewer
                 UNION SELECT number FROM issue_assignees WHERE repository = :repository AND login = :viewer
                 UNION SELECT number FROM activity WHERE repository = :repository AND who = :viewer
             )))
         ORDER BY noticed_at, kind, target, who"
    );
    let mut statement = connection.prepare(&query)?;
    let first_sync = first_sync.map(OffsetDateTime::unix_timestamp);
    let asked = items.with(vec![
        (":repository", repository),
        (":first_sync", &first_sync),
        (":viewer", &viewer),
    ]);
    let mut rows = statement.query(asked.as_slice())?;
    let mut found: BTreeMap<u32, Vec<Activity>> = BTreeMap::new();
    while let Some(row) = rows.next()? {
        if let Some(event) = event_in(row)? {
            found.entry(row.get("number")?).or_default().push(event);
        }
    }
    Ok(found)
}

fn first_sync(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<Option<OffsetDateTime>> {
    connection
        .query_row(
            "SELECT started_at FROM first_syncs WHERE repository = :repository",
            named_params! { ":repository": repository },
            |row| row.get("started_at").map(|UnixTime(at)| at),
        )
        .optional()
}

/// `repository`'s activity, as `viewer` reads it.
pub(super) fn read_facts(
    connection: &Connection,
    repository: &RepositoryName,
    viewer: Option<&Login>,
) -> rusqlite::Result<ActivityFacts> {
    read_facts_of(connection, repository, viewer, Items::All)
}

/// The activity of `items` of `repository`, as `viewer` reads it.
pub(super) fn read_facts_of(
    connection: &Connection,
    repository: &RepositoryName,
    viewer: Option<&Login>,
    items: Items<'_>,
) -> rusqlite::Result<ActivityFacts> {
    let first_sync = first_sync(connection, repository)?;
    let condition = items.condition("number");
    let last_seen =
        format!("SELECT number, at FROM last_seen WHERE repository = :repository {condition}");
    let acknowledged = format!(
        "SELECT number, acknowledged_at AS at FROM last_seen
         WHERE repository = :repository AND acknowledged_at IS NOT NULL {condition}"
    );
    let marked_unseen =
        format!("SELECT number, at FROM marked_unseen WHERE repository = :repository {condition}");
    let ci_finished = format!(
        "SELECT number, ci_finished_at AS at FROM pull_requests
         WHERE repository = :repository AND ci_finished_at IS NOT NULL {condition}"
    );
    Ok(ActivityFacts {
        first_sync,
        last_seen: moments(connection, &last_seen, repository, items)?,
        acknowledged: moments(connection, &acknowledged, repository, items)?,
        marked_unseen: moments(connection, &marked_unseen, repository, items)?,
        archived: archived(connection, repository, items)?,
        events: listed_events(connection, repository, (first_sync, viewer), items)?,
        ci_finished: moments(connection, &ci_finished, repository, items)?,
        failed_checks: failed_checks::read(connection, repository, items)?,
    })
}

#[cfg(test)]
mod tests;
