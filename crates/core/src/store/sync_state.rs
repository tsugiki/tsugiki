//! What a Repository's refresh needs from the last sync that finished (ADR 0015). A Repository with no row
//! here hasn't finished its first sync, so its next sync is a whole one.

use rusqlite::{Connection, OptionalExtension, Row, named_params};
use time::OffsetDateTime;

use super::columns::UnixTime;
use crate::github::refresh::Etag;
use crate::repository::RepositoryName;

pub(super) const SCHEMA: &str = "
    CREATE TABLE sync_state (
        repository TEXT PRIMARY KEY COLLATE NOCASE,
        started_at INTEGER NOT NULL,
        finished_at INTEGER NOT NULL,
        refresh_under_way INTEGER NOT NULL,
        small INTEGER NOT NULL,
        etag TEXT
    ) WITHOUT ROWID;
";

/// The place of a first sync that hasn't finished. A finished sync's row in `sync_state` replaces it.
pub(super) const FIRST_SYNC_PLACE: &str = "
    CREATE TABLE first_sync_place (
        repository TEXT PRIMARY KEY COLLATE NOCASE,
        place TEXT NOT NULL
    ) WITHOUT ROWID;
";

/// A first sync's place. Only the sync can read this, and the store only keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstSyncPlace(String);

impl FirstSyncPlace {
    pub fn new(place: String) -> Self {
        Self(place)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub(super) fn read_place(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<Option<FirstSyncPlace>> {
    connection
        .query_row(
            "SELECT place FROM first_sync_place WHERE repository = :repository",
            named_params! { ":repository": repository },
            |row| row.get("place").map(FirstSyncPlace),
        )
        .optional()
}

pub(super) fn save_place(
    connection: &Connection,
    repository: &RepositoryName,
    place: &FirstSyncPlace,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT INTO first_sync_place (repository, place) VALUES (:repository, :place)
             ON CONFLICT (repository) DO UPDATE SET place = excluded.place",
            named_params! { ":repository": repository, ":place": place.as_str() },
        )
        .map(drop)
}

/// A sync that has finished has no place to come back to.
pub(super) fn forget_place(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "DELETE FROM first_sync_place WHERE repository = :repository",
            named_params! { ":repository": repository },
        )
        .map(drop)
}

/// How a refresh can learn cheaply that nothing changed (ADR 0015).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeCheck {
    /// A large Repository changes too often for a check to save anything.
    Skipped,
    /// A small Repository whose next refresh asks for a tag.
    Untagged,
    /// A small Repository, and the tag of its list when Tsugiki last read it.
    Tagged(Etag),
}

/// A Repository's last finished sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncState {
    /// The moment that sync started. It saw each change up to here, and later ones wait for the next.
    pub started_at: OffsetDateTime,
    /// The moment it finished, which the status bar shows.
    pub finished_at: OffsetDateTime,
    pub check: ChangeCheck,
    /// A later refresh began and hasn't finished. Its saved facts may have lost the work they lead to.
    pub refresh_under_way: bool,
}

pub(super) fn read(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<Option<SyncState>> {
    connection
        .query_row(
            "SELECT started_at, finished_at, refresh_under_way, small, etag
             FROM sync_state WHERE repository = :repository",
            named_params! { ":repository": repository },
            state_in,
        )
        .optional()
}

fn state_in(row: &Row<'_>) -> rusqlite::Result<SyncState> {
    let UnixTime(started_at) = row.get("started_at")?;
    let UnixTime(finished_at) = row.get("finished_at")?;
    let small: bool = row.get("small")?;
    let etag: Option<Etag> = row.get("etag")?;
    let check = match (small, etag) {
        (false, _) => ChangeCheck::Skipped,
        (true, None) => ChangeCheck::Untagged,
        (true, Some(etag)) => ChangeCheck::Tagged(etag),
    };
    Ok(SyncState {
        started_at,
        finished_at,
        check,
        refresh_under_way: row.get("refresh_under_way")?,
    })
}

pub(super) fn save(
    connection: &Connection,
    repository: &RepositoryName,
    state: &SyncState,
) -> rusqlite::Result<()> {
    let (small, etag) = match &state.check {
        ChangeCheck::Skipped => (false, None),
        ChangeCheck::Untagged => (true, None),
        ChangeCheck::Tagged(etag) => (true, Some(etag)),
    };
    let params = named_params! {
        ":repository": repository,
        ":started_at": state.started_at.unix_timestamp(),
        ":finished_at": state.finished_at.unix_timestamp(),
        ":refresh_under_way": state.refresh_under_way,
        ":small": small,
        ":etag": etag,
    };
    connection
        .execute(
            "INSERT INTO sync_state
                 (repository, started_at, finished_at, refresh_under_way, small, etag)
             VALUES (:repository, :started_at, :finished_at, :refresh_under_way, :small, :etag)
             ON CONFLICT (repository) DO UPDATE SET
                 started_at = excluded.started_at,
                 finished_at = excluded.finished_at,
                 refresh_under_way = excluded.refresh_under_way,
                 small = excluded.small,
                 etag = excluded.etag",
            params,
        )
        .map(drop)
}

#[cfg(test)]
pub(super) fn forget(connection: &Connection, repository: &RepositoryName) -> rusqlite::Result<()> {
    connection
        .execute(
            "DELETE FROM sync_state WHERE repository = :repository",
            named_params! { ":repository": repository },
        )
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use crate::store::Store;
    use crate::store::testing::temp_database;
    use std::fs;

    fn cpython() -> RepositoryName {
        RepositoryName::parse("python/cpython").unwrap()
    }

    fn at(seconds: i64, check: ChangeCheck) -> SyncState {
        let started_at = OffsetDateTime::from_unix_timestamp(seconds).unwrap();
        SyncState {
            started_at,
            finished_at: started_at + time::Duration::minutes(40),
            check,
            refresh_under_way: false,
        }
    }

    #[test]
    fn a_repository_that_never_finished_a_sync_has_no_state() {
        let (path, folder) = temp_database("sync-state-none");
        let store = Store::open(&path).unwrap();

        let state = store.sync_state(&cpython()).unwrap();
        store.close();
        fs::remove_dir_all(folder).unwrap();

        assert_eq!(state, None);
    }

    #[test]
    fn the_newest_state_survives_a_restart_whatever_the_case_of_the_name() {
        let (path, folder) = temp_database("sync-state-restart");
        let store = Store::open(&path).unwrap();
        store
            .save_sync_state(&cpython(), at(100, ChangeCheck::Skipped))
            .unwrap();
        let newest = SyncState {
            refresh_under_way: true,
            ..at(200, ChangeCheck::Tagged(Etag::new("W/\"abc\"")))
        };
        store.save_sync_state(&cpython(), newest.clone()).unwrap();
        store.close();

        let store = Store::open(&path).unwrap();
        let shouted = RepositoryName::parse("Python/CPython").unwrap();
        let state = store.sync_state(&shouted).unwrap();
        store.close();
        fs::remove_dir_all(folder).unwrap();

        assert_eq!(state, Some(newest));
    }

    #[test]
    fn each_kind_of_check_reads_back_as_it_was_saved() {
        let (path, folder) = temp_database("sync-state-checks");
        let store = Store::open(&path).unwrap();
        let mut read = Vec::new();
        let checks = [
            ChangeCheck::Skipped,
            ChangeCheck::Untagged,
            ChangeCheck::Tagged(Etag::new("\"tag\"")),
        ];
        for check in checks.clone() {
            store.save_sync_state(&cpython(), at(1, check)).unwrap();
            read.push(store.sync_state(&cpython()).unwrap().unwrap().check);
        }
        store.close();
        fs::remove_dir_all(folder).unwrap();

        assert_eq!(read, checks);
    }

    #[test]
    fn a_stopped_store_says_so() {
        let store = Store::stopped();

        let reading = store.sync_state(&cpython()).unwrap_err();
        let saving = store
            .save_sync_state(&cpython(), at(1, ChangeCheck::Skipped))
            .unwrap_err();

        assert_eq!(reading.kind, ErrorKind::Internal);
        assert_eq!(saving.kind, ErrorKind::Internal);
    }
}
