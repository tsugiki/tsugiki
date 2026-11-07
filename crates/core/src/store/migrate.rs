//! Moves an open database to the current schema, after a backup (ADR 0010).

use std::cmp::Reverse;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, named_params};

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
mod tests {
    use super::super::file::{Contents, contents};
    use super::super::open_with;
    use super::super::testing::{
        V1, V2, V3, V3_BROKEN, backups, has_table, temp_database, user_version,
    };
    use super::*;

    #[test]
    fn an_older_database_moves_forward_and_leaves_a_backup_of_how_it_was() {
        let (path, folder) = temp_database("older");
        open_with(&path, &[V1]).unwrap().close();

        open_with(&path, &[V1, V2]).unwrap().close();

        assert_eq!(user_version(&path), 2);
        assert!(has_table(&path, "tags"));
        assert_eq!(backups(&folder), ["tsugiki.db.v1.bak"]);
        let backup = folder.join("tsugiki.db.v1.bak");
        assert_eq!(user_version(&backup), 1);
        assert!(has_table(&backup, "notes"));
        assert!(!has_table(&backup, "tags"));
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_current_database_leaves_no_backup() {
        let (path, folder) = temp_database("current");
        open_with(&path, &[V1]).unwrap().close();
        open_with(&path, &[V1]).unwrap().close();
        assert_eq!(backups(&folder), [""; 0]);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_failed_migration_changes_nothing_and_can_be_tried_again() {
        let (path, folder) = temp_database("failed");
        open_with(&path, &[V1]).unwrap().close();

        let problem = open_with(&path, &[V1, V2, V3_BROKEN]).err().unwrap();

        assert_eq!(problem.kind, ErrorKind::Internal);
        assert!(problem.message.contains("update"), "{}", problem.message);
        assert_eq!(user_version(&path), 1);
        assert!(
            !has_table(&path, "tags"),
            "one transaction covers each step"
        );
        assert!(!has_table(&path, "half_done"));
        assert!(has_table(&path, "notes"));

        open_with(&path, &[V1, V2]).unwrap().close();
        assert_eq!(user_version(&path), 2);
        assert_eq!(backups(&folder), ["tsugiki.db.v1.bak"]);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn only_the_last_few_backups_are_kept() {
        let (path, folder) = temp_database("pruned");
        let steps: Vec<Migration> = (1..=6)
            .map(|version| Migration {
                version,
                sql: "CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY);",
            })
            .collect();
        for count in 1..=steps.len() {
            open_with(&path, steps.get(..count).unwrap())
                .unwrap()
                .close();
        }
        fs::write(folder.join("tsugiki.db.notes.bak"), "someone else's").unwrap();

        open_with(&path, &steps).unwrap().close();

        assert_eq!(
            backups(&folder),
            [
                "tsugiki.db.notes.bak",
                "tsugiki.db.v3.bak",
                "tsugiki.db.v4.bak",
                "tsugiki.db.v5.bak"
            ]
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn the_backup_just_written_stays_whatever_the_numbers_on_the_others() {
        let (path, folder) = temp_database("just-written");
        open_with(&path, &[V1, V2]).unwrap().close();
        for name in [
            "tsugiki.db.v3.bak",
            "tsugiki.db.v4.bak",
            "tsugiki.db.v5.bak",
        ] {
            fs::write(folder.join(name), "from a later version").unwrap();
        }

        open_with(&path, &[V1, V2, V3]).unwrap().close();

        assert_eq!(
            backups(&folder),
            [
                "tsugiki.db.v2.bak",
                "tsugiki.db.v4.bak",
                "tsugiki.db.v5.bak"
            ]
        );
        assert_eq!(user_version(&folder.join("tsugiki.db.v2.bak")), 2);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_backup_with_a_padded_number_goes_by_its_real_name() {
        let (path, folder) = temp_database("padded");
        open_with(&path, &[V1]).unwrap().close();
        for name in [
            "tsugiki.db.v01.bak",
            "tsugiki.db.v2.bak",
            "tsugiki.db.v3.bak",
            "tsugiki.db.v4.bak",
        ] {
            fs::write(folder.join(name), "old").unwrap();
        }

        open_with(&path, &[V1, V2]).unwrap().close();

        assert_eq!(
            backups(&folder),
            [
                "tsugiki.db.v1.bak",
                "tsugiki.db.v3.bak",
                "tsugiki.db.v4.bak"
            ]
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn an_old_backup_that_cant_be_removed_doesnt_stop_the_migration() {
        let (path, folder) = temp_database("stuck-backup");
        open_with(&path, &[V1]).unwrap().close();
        for version in 2..=5 {
            // Removing a file fails on a folder, on Windows and on macOS.
            fs::create_dir(folder.join(format!("tsugiki.db.v{version}.bak"))).unwrap();
        }

        open_with(&path, &[V1, V2]).unwrap().close();

        assert_eq!(user_version(&path), 2);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_backup_that_fails_keeps_the_one_before_it() {
        let (path, folder) = temp_database("failed-backup");
        open_with(&path, &[V1]).unwrap().close();
        let earlier = folder.join("tsugiki.db.v1.bak");
        fs::write(&earlier, "the earlier backup").unwrap();
        // A folder where the new backup goes first makes writing it fail.
        fs::create_dir(folder.join("tsugiki.db.v1.bak.tmp")).unwrap();

        let problem = open_with(&path, &[V1, V2]).err().unwrap();

        assert_eq!(problem.message, CANT_UPDATE);
        assert_eq!(fs::read_to_string(&earlier).unwrap(), "the earlier backup");
        assert_eq!(user_version(&path), 1);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_backup_left_half_written_doesnt_block_the_next_one() {
        let (path, folder) = temp_database("half-written");
        open_with(&path, &[V1]).unwrap().close();
        let partial = folder.join("tsugiki.db.v1.bak.tmp");
        fs::write(&partial, "half of a backup").unwrap();

        open_with(&path, &[V1, V2]).unwrap().close();

        assert!(!partial.exists());
        assert_eq!(user_version(&folder.join("tsugiki.db.v1.bak")), 1);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_file_with_tables_and_no_version_gets_a_backup() {
        let (path, folder) = temp_database("no-version");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("CREATE TABLE theirs (id INTEGER PRIMARY KEY);")
            .unwrap();
        drop(connection);

        open_with(&path, &[V1]).unwrap().close();

        assert_eq!(backups(&folder), ["tsugiki.db.v0.bak"]);
        assert!(has_table(&folder.join("tsugiki.db.v0.bak"), "theirs"));
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_newer_version_still_in_the_wal_is_refused_too() {
        let (path, folder) = temp_database("newer-in-wal");
        open_with(&path, &[V1]).unwrap().close();
        // While this stays open its change sits in the `-wal` file, and the header still says version 1.
        let writer = Connection::open(&path).unwrap();
        writer.pragma_update(None, "user_version", 2).unwrap();
        assert_eq!(contents(&path), Contents::Database { version: 1 });

        let problem = open_with(&path, &[V1]).err().unwrap();

        assert!(problem.message.contains("newer"), "{}", problem.message);
        drop(writer);
        fs::remove_dir_all(folder).unwrap();
    }

    /// A parent and a child that points at it.
    const PARENTS: Migration = Migration {
        version: 2,
        sql: "CREATE TABLE parents (id INTEGER PRIMARY KEY);
              CREATE TABLE children (id INTEGER PRIMARY KEY, parent INTEGER NOT NULL REFERENCES parents (id));
              INSERT INTO parents (id) VALUES (1);
              INSERT INTO children (parent) VALUES (1);",
    };

    /// Gives `parents` a new column the way SQLite's steps for changing a table do: a new table, a copy, a
    /// drop and a rename. Dropping a parent that has children fails while foreign keys are enforced.
    const REBUILT_PARENTS: Migration = Migration {
        version: 3,
        sql: "CREATE TABLE new_parents (id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT '');
              INSERT INTO new_parents (id) SELECT id FROM parents;
              DROP TABLE parents;
              ALTER TABLE new_parents RENAME TO parents;",
    };

    const ORPHAN: Migration = Migration {
        version: 3,
        sql: "INSERT INTO children (parent) VALUES (99);",
    };

    fn children(path: &Path) -> Vec<u32> {
        let connection = Connection::open(path).unwrap();
        let mut statement = connection
            .prepare("SELECT parent FROM children ORDER BY id")
            .unwrap();
        statement
            .query_map([], |row| row.get("parent"))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn enforces_foreign_keys(connection: &Connection) -> bool {
        connection
            .pragma_query_value(None, "foreign_keys", |row| row.get("foreign_keys"))
            .unwrap()
    }

    #[test]
    fn a_migration_can_rebuild_a_table_that_others_point_at() {
        let (path, folder) = temp_database("rebuilt-parent");
        open_with(&path, &[V1, PARENTS]).unwrap().close();

        open_with(&path, &[V1, PARENTS, REBUILT_PARENTS])
            .unwrap()
            .close();

        assert_eq!(user_version(&path), 3);
        assert_eq!(children(&path), [1]);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_migration_that_breaks_a_foreign_key_fails_and_changes_nothing() {
        let (path, folder) = temp_database("broken-key");
        open_with(&path, &[V1, PARENTS]).unwrap().close();

        let problem = open_with(&path, &[V1, PARENTS, ORPHAN]).err().unwrap();

        assert_eq!(problem.kind, ErrorKind::Internal);
        assert_eq!(problem.message, CANT_UPDATE);
        assert_eq!(user_version(&path), 2);
        assert_eq!(children(&path), [1]);
        assert_eq!(backups(&folder), ["tsugiki.db.v2.bak"]);
        assert_eq!(children(&folder.join("tsugiki.db.v2.bak")), [1]);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn foreign_keys_are_enforced_again_after_a_migration_whatever_happened() {
        let (path, folder) = temp_database("keys-back-on");
        let mut connection = Connection::open(&path).unwrap();

        run(&mut connection, &path, &[V1, PARENTS]).unwrap();
        assert!(enforces_foreign_keys(&connection));
        let _problem = run(&mut connection, &path, &[V1, PARENTS, ORPHAN]).unwrap_err();
        assert!(enforces_foreign_keys(&connection));

        drop(connection);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn the_migrations_count_up_from_one() {
        for (expected, step) in (1..).zip(MIGRATIONS) {
            assert_eq!(step.version, expected);
        }
    }
}
