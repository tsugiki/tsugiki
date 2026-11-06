//! The store's part of New activity: Last seen, the timeline GitHub reported, and when a first sync
//! began. Issues and PRs share their numbers in a Repository, so a number says which item a row is about.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension, Row, named_params};
use time::OffsetDateTime;

use super::columns::UnixTime;
use super::failed_checks;
use crate::github::Login;
use crate::github::activity::{Activity, ActivityKind, ItemActivity};
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

/// Moves Last seen on to `at`. A clock set back doesn't move it back.
pub(super) fn mark_seen(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    at: OffsetDateTime,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT INTO last_seen (repository, number, at) VALUES (:repository, :number, :at)
             ON CONFLICT (repository, number) DO UPDATE SET at = MAX(at, excluded.at)",
            named_params! {
                ":repository": repository,
                ":number": number,
                ":at": at.unix_timestamp(),
            },
        )
        .map(drop)
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

/// The facts New activity and Needs attention read about a Repository's items, all in one go, so ranking a
/// list costs no query for each item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivityFacts {
    /// The moment the Repository's first sync began, once a first sync has finished.
    pub first_sync: Option<OffsetDateTime>,
    pub last_seen: BTreeMap<u32, OffsetDateTime>,
    /// Each event that names the user, and what happened after the first sync began on the items they wrote,
    /// reviewed, are assigned to or commented on, each at the time New activity goes by, oldest first. Only
    /// those can make an item Need attention, and a Repository holds too many others to read with each list.
    pub events: BTreeMap<u32, Vec<Activity>>,
    /// The moment Tsugiki saw each PR's CI reach a result.
    pub ci_finished: BTreeMap<u32, OffsetDateTime>,
    /// The names of the failed checks of each PR Tsugiki has asked about.
    pub failed_checks: BTreeMap<u32, Vec<String>>,
}

const SELECT_LAST_SEEN: &str = "SELECT number, at FROM last_seen WHERE repository = :repository";
const SELECT_CI_FINISHED: &str = "SELECT number, ci_finished_at AS at FROM pull_requests
     WHERE repository = :repository AND ci_finished_at IS NOT NULL";
const SELECT_LISTED_EVENTS: &str =
    "SELECT number, kind, target, who, noticed_at AS at FROM activity
     WHERE repository = :repository AND (who = :viewer OR (noticed_at > :first_sync AND number IN (
         SELECT number FROM pull_requests WHERE repository = :repository AND author = :viewer
         UNION SELECT number FROM issues WHERE repository = :repository AND author = :viewer
         UNION SELECT number FROM latest_reviews WHERE repository = :repository AND login = :viewer
         UNION SELECT number FROM assignees WHERE repository = :repository AND login = :viewer
         UNION SELECT number FROM issue_assignees WHERE repository = :repository AND login = :viewer
         UNION SELECT number FROM activity WHERE repository = :repository AND who = :viewer
     )))
     ORDER BY noticed_at, kind, target, who";

fn moment_in(row: &Row<'_>) -> rusqlite::Result<(u32, OffsetDateTime)> {
    let UnixTime(at) = row.get("at")?;
    Ok((row.get("number")?, at))
}

/// The moment in each row of `query`, which selects `number` and `at`, by number.
fn moments(
    connection: &Connection,
    query: &str,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<u32, OffsetDateTime>> {
    let mut statement = connection.prepare(query)?;
    let asked = named_params! { ":repository": repository };
    let rows = statement.query_map(asked, moment_in)?;
    rows.collect()
}

fn listed_events(
    connection: &Connection,
    repository: &RepositoryName,
    first_sync: Option<OffsetDateTime>,
    viewer: Option<&Login>,
) -> rusqlite::Result<BTreeMap<u32, Vec<Activity>>> {
    let mut statement = connection.prepare(SELECT_LISTED_EVENTS)?;
    let asked = named_params! {
        ":repository": repository,
        ":first_sync": first_sync.map(OffsetDateTime::unix_timestamp),
        ":viewer": viewer,
    };
    let mut rows = statement.query(asked)?;
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
    let first_sync = first_sync(connection, repository)?;
    Ok(ActivityFacts {
        first_sync,
        last_seen: moments(connection, SELECT_LAST_SEEN, repository)?,
        events: listed_events(connection, repository, first_sync, viewer)?,
        ci_finished: moments(connection, SELECT_CI_FINISHED, repository)?,
        failed_checks: failed_checks::read_all(connection, repository)?,
    })
}

#[cfg(test)]
mod tests {
    use time::Duration;

    use super::*;
    use crate::store::Store;
    use crate::store::migrate::MIGRATIONS;
    use crate::store::testing::temp_database;

    #[test]
    fn a_file_from_before_new_activity_counts_from_its_last_sync() {
        let (path, folder) = temp_database("activity-migrates");
        let repository = RepositoryName::parse("python/cpython").unwrap();
        let synced = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap() - Duration::days(1);
        crate::store::open_with(&path, MIGRATIONS.split_at(9).0)
            .unwrap()
            .close();
        // Our functions can't write a file of an older version.
        let raw = Connection::open(&path).unwrap();
        let inserted = raw
            .execute(
                "INSERT INTO sync_state
                     (repository, started_at, finished_at, refresh_under_way, small, etag)
                 VALUES (:repository, :at, :at, 0, 0, NULL)",
                named_params! { ":repository": repository, ":at": synced.unix_timestamp() },
            )
            .unwrap();
        assert_eq!(inserted, 1);
        drop(raw);

        Store::open(&path).unwrap().close();
        let reopened = Connection::open(&path).unwrap();
        let started = first_sync(&reopened, &repository).unwrap();
        drop(reopened);

        std::fs::remove_dir_all(folder).unwrap();
        assert_eq!(started, Some(synced));
    }

    #[test]
    fn a_kind_from_a_later_version_is_left_alone() {
        let (path, folder) = temp_database("activity-later-kind");
        let repository = RepositoryName::parse("python/cpython").unwrap();
        let store = Store::open(&path).unwrap();
        // This version has no way to write a kind it doesn't know.
        let raw = Connection::open(&path).unwrap();
        let inserted = raw
            .execute(
                "INSERT INTO activity (repository, number, kind, target, who, at, noticed_at)
                 VALUES (:repository, 1, 'later', '', 'some-author', 0, 0)",
                named_params! { ":repository": repository },
            )
            .unwrap();
        assert_eq!(inserted, 1);
        let author = Login::new("some-author");
        let listed = listed_events(&raw, &repository, None, Some(&author)).unwrap();
        drop(raw);

        let read = store.activity(&repository, 1).unwrap();

        store.close();
        std::fs::remove_dir_all(folder).unwrap();
        assert_eq!(read, []);
        assert_eq!(listed, BTreeMap::new());
    }
}
