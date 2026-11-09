//! Moves an open database to the current schema, after a backup (ADR 0010).

use std::cmp::Reverse;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::named_params;

use super::Connection;
use super::{CANT_UPDATE, NEWER, saying};
use crate::error::{Error, ErrorKind};

/// How many earlier backups stay next to the database after a migration, beside the one it just wrote, so
/// three in all (ADR 0010).
const KEPT_OTHER_BACKUPS: usize = 2;

/// One numbered step of the schema. A released step never changes, since a user's file has already run it.
/// Until 0.1 nothing is released, so a step can change in place, and a file from before the change has to be
/// deleted by hand.
pub(super) struct Migration {
    pub(super) version: u32,
    pub(super) sql: &'static str,
}

pub(super) const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: "CREATE TABLE viewer (id INTEGER PRIMARY KEY CHECK (id = 1), login TEXT NOT NULL);",
    },
    Migration {
        version: 2,
        sql: super::pull_requests::SCHEMA,
    },
    Migration {
        version: 3,
        sql: super::pull_requests::LINK_FACTS,
    },
    Migration {
        version: 4,
        sql: super::issues::SCHEMA,
    },
    Migration {
        version: 5,
        sql: super::pull_requests::BACKPORT_FACTS,
    },
    Migration {
        version: 6,
        sql: super::filters::SCHEMA,
    },
    Migration {
        version: 7,
        sql: super::sync_state::SCHEMA,
    },
    Migration {
        version: 8,
        sql: super::filters::SEARCHES,
    },
    Migration {
        version: 9,
        sql: super::sync_state::FIRST_SYNC_PLACE,
    },
    Migration {
        version: 10,
        sql: super::activity::SCHEMA,
    },
    Migration {
        version: 11,
        sql: super::failed_checks::SCHEMA,
    },
    Migration {
        version: 12,
        sql: super::issue_content::SCHEMA,
    },
    Migration {
        version: 13,
        sql: super::pull_requests::HEADS,
    },
    Migration {
        version: 14,
        sql: super::pull_request_content::SCHEMA,
    },
    Migration {
        version: 15,
        sql: super::pull_request_content::THREAD_COMMENTS,
    },
    Migration {
        version: 16,
        sql: super::pull_request_content::PENDING_REVIEW,
    },
    Migration {
        version: 17,
        sql: super::issues::STATES,
    },
    Migration {
        version: 18,
        sql: super::pull_requests::BASES,
    },
    Migration {
        version: 19,
        sql: super::sync_state::READ_AHEAD,
    },
];

/// The version a file has after all of `migrations`.
pub(super) fn latest(migrations: &[Migration]) -> u32 {
    migrations.last().map_or(0, |step| step.version)
}

fn user_version(connection: &Connection) -> rusqlite::Result<u32> {
    connection.pragma_query_value(None, "user_version", |row| row.get("user_version"))
}

/// Runs the steps this file hasn't had yet, in one transaction, after a backup if it holds anything. The
/// store's lock keeps a second Tsugiki out of here.
pub(super) fn run(
    connection: &mut Connection,
    path: &Path,
    migrations: &[Migration],
) -> Result<(), Error> {
    let version = user_version(connection).map_err(saying(CANT_UPDATE))?;
    if version > latest(migrations) {
        return Err(Error::new(ErrorKind::Internal, NEWER));
    }
    let pending: Vec<&Migration> = migrations
        .iter()
        .filter(|step| step.version > version)
        .collect();
    if pending.is_empty() {
        return Ok(());
    }
    let holds_something: bool = connection
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master) AS holds_something",
            [],
            |row| row.get("holds_something"),
        )
        .map_err(saying(CANT_UPDATE))?;
    if holds_something {
        back_up(connection, path, version)?;
    }
    connection
        .pragma_update(None, "foreign_keys", false)
        .map_err(saying(CANT_UPDATE))?;
    let ran = run_checked(connection, &pending);
    let enforced = connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(saying(CANT_UPDATE));
    ran.and(enforced)
}

/// Runs `pending` in one transaction, and commits only if every foreign key still holds. These are SQLite's
/// steps for changing a table, from the ALTER TABLE page of its docs: a step that rebuilds a table
/// others point at drops it first, which enforcement refuses. `PRAGMA foreign_keys` does nothing inside a
/// transaction, so `run` turns enforcement off before this and back on after it.
fn run_checked(connection: &mut Connection, pending: &[&Migration]) -> Result<(), Error> {
    let transaction = connection.transaction().map_err(saying(CANT_UPDATE))?;
    for step in pending {
        transaction
            .execute_batch(step.sql)
            .map_err(saying(CANT_UPDATE))?;
        transaction
            .pragma_update(None, "user_version", step.version)
            .map_err(saying(CANT_UPDATE))?;
    }
    let broken = broken_keys(&transaction).map_err(saying(CANT_UPDATE))?;
    if !broken.is_empty() {
        // Dropping the transaction rolls it back.
        return Err(saying(CANT_UPDATE)(format!(
            "rows break a foreign key, from table to table: {}",
            broken.join(", ")
        )));
    }
    transaction.commit().map_err(saying(CANT_UPDATE))
}

/// Each pair of tables where a row points at a row that isn't there, as `child -> parent`.
fn broken_keys(connection: &Connection) -> rusqlite::Result<Vec<String>> {
    let query = "SELECT DISTINCT \"table\" AS child, parent FROM pragma_foreign_key_check ORDER BY child, parent";
    let mut statement = connection.prepare(query)?;
    let pairs = statement.query_map([], |row| {
        let child: String = row.get("child")?;
        let parent: String = row.get("parent")?;
        Ok(format!("{child} -> {parent}"))
    });
    pairs?.collect()
}

/// Writes `tsugiki.db.v<version>.bak`, replacing one a failed migration left. The copy gets its final name
/// only once it is whole, so a failure here leaves the earlier backup as it was.
fn back_up(connection: &Connection, path: &Path, version: u32) -> Result<(), Error> {
    let backup = with_suffix(path, &format!(".v{version}.bak"));
    let partial = with_suffix(&backup, ".tmp");
    if partial.exists() {
        fs::remove_file(&partial).map_err(saying(CANT_UPDATE))?;
    }
    let target = partial.to_string_lossy();
    let _copied = connection
        .execute("VACUUM INTO :target", named_params! { ":target": target })
        .map_err(saying(CANT_UPDATE))?;
    fs::rename(&partial, &backup).map_err(saying(CANT_UPDATE))?;
    remove_old_backups(path, &backup);
    Ok(())
}

/// Keeps `written` and the highest numbered of the others. A backup this can't remove only takes up space and
/// the next migration tries again, so it doesn't stop this one.
fn remove_old_backups(path: &Path, written: &Path) {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let prefix = format!("{name}.v");
    let backup_version = |file_name: OsString| -> Option<u32> {
        let file_name = file_name.to_string_lossy().into_owned();
        file_name
            .strip_prefix(&prefix)?
            .strip_suffix(".bak")?
            .parse()
            .ok()
    };
    let mut others: Vec<(u32, PathBuf)> = fs::read_dir(path.with_file_name(""))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| Some((backup_version(entry.file_name())?, entry.path())))
        .filter(|(_, file)| file != written)
        .collect();
    others.sort_unstable_by_key(|(version, _)| Reverse(*version));
    for (_, old) in others.iter().skip(KEPT_OTHER_BACKUPS) {
        if let Err(cause) = fs::remove_file(old) {
            log::warn!("Tsugiki can't remove {}: {cause}", old.display());
        }
    }
}

pub(super) fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut longer = path.as_os_str().to_owned();
    longer.push(suffix);
    PathBuf::from(longer)
}

#[cfg(test)]
mod tests;
