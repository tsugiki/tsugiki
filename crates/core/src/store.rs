//! What Tsugiki learns, kept in one SQLite file (ADR 0010). One thread owns the connection and runs each job
//! in turn, so a caller waits for the jobs ahead of its own. Screens mustn't wait on a sync (ADR 0010), so
//! the sync has to write outside this queue when it arrives.

use std::cmp::Reverse;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use rusqlite::{Connection, OptionalExtension};

use crate::error::{Error, ErrorKind};
use crate::github::Login;

/// How many earlier backups stay next to the database after a migration, beside the one it just wrote, so
/// three in all (ADR 0010).
const KEPT_OTHER_BACKUPS: usize = 2;

/// Where the database header section of SQLite's file format document puts these.
const SQLITE_SIGNATURE: &[u8; 16] = b"SQLite format 3\0";
const USER_VERSION_OFFSET: u64 = 60;

const CANT_OPEN: &str = "Tsugiki can't open its database";
const CANT_READ: &str = "Tsugiki can't read its database";
const CANT_WRITE: &str = "Tsugiki can't write to its database";
const CANT_UPDATE: &str = "Tsugiki can't update its database to this version";
const NEWER: &str = "A newer version of Tsugiki wrote this database, so this version won't open it. \
                     Update Tsugiki to read it.";
const STOPPED: &str = "Tsugiki's database stopped working. Restart Tsugiki to open it again.";

/// One numbered step of the schema. A released step never changes, since a user's file has already run it.
struct Migration {
    version: u32,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: "CREATE TABLE viewer (id INTEGER PRIMARY KEY CHECK (id = 1), login TEXT NOT NULL);",
}];

// Add each query a screen waits on to `VIEW_QUERIES` in the tests, which check its plan (ADR 0017).
const LAST_LOGIN: &str = "SELECT login FROM viewer WHERE id = 1";
const REMEMBER_LOGIN: &str = "INSERT INTO viewer (id, login) VALUES (1, ?1)
                              ON CONFLICT (id) DO UPDATE SET login = excluded.login
                              WHERE login <> excluded.login";

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

/// The open database.
pub struct Store {
    jobs: Sender<Job>,
    thread: JoinHandle<()>,
}

impl Store {
    /// Opens the database at `path`, creating it or moving it to the current schema as needed.
    pub fn open(path: &Path) -> Result<Self, Error> {
        open_with(path, MIGRATIONS)
    }

    /// Who was signed in when Tsugiki last reached GitHub, if it ever has.
    pub fn last_login(&self) -> Result<Option<Login>, Error> {
        let found = self.call(|connection| {
            connection
                .query_row(LAST_LOGIN, [], |row| row.get::<_, String>(0))
                .optional()
        })?;
        let found = found.map_err(saying(CANT_READ))?;
        Ok(found.as_deref().map(Login::new))
    }

    pub fn remember_login(&self, login: &Login) -> Result<(), Error> {
        let login = login.as_str().to_owned();
        let written = self.call(move |connection| connection.execute(REMEMBER_LOGIN, [login]))?;
        written.map(drop).map_err(saying(CANT_WRITE))
    }

    /// Finishes what was asked for and closes the file.
    pub fn close(self) {
        drop(self.jobs);
        #[expect(
            clippy::let_underscore_must_use,
            reason = "a thread that panicked has closed the file too, and `call` has already said so"
        )]
        let _ = self.thread.join();
    }

    /// Runs `work` on the thread that owns the connection, and waits for its answer.
    fn call<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Connection) -> T + Send + 'static,
    ) -> Result<T, Error> {
        let (reply, answer) = mpsc::channel();
        let job = Box::new(move |connection: &mut Connection| {
            #[expect(
                clippy::let_underscore_must_use,
                reason = "this fails only if the caller has gone, and it waits for this answer"
            )]
            let _ = reply.send(work(connection));
        });
        self.jobs
            .send(job)
            .ok()
            .and_then(|()| answer.recv().ok())
            .ok_or_else(|| Error::new(ErrorKind::Internal, STOPPED))
    }

    /// A store whose thread has gone, as it would be after a panic there.
    #[cfg(test)]
    pub(crate) fn stopped() -> Self {
        let (jobs, _) = std::sync::mpsc::channel();
        Self {
            jobs,
            thread: std::thread::spawn(|| {}),
        }
    }
}

fn open_with(path: &Path, migrations: &[Migration]) -> Result<Store, Error> {
    fs::create_dir_all(path.with_file_name("")).map_err(saying(CANT_OPEN))?;
    let latest = migrations.last().map_or(0, |step| step.version);
    if version_in_header(path).is_some_and(|version| version > latest) {
        return Err(Error::new(ErrorKind::Internal, NEWER));
    }

    let mut connection = Connection::open(path).map_err(saying(CANT_OPEN))?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(saying(CANT_OPEN))?;
    migrate(&mut connection, path, migrations)?;
    // Tsugiki normally quits without closing the store, which would leave a new version in the `-wal` file,
    // where `version_in_header` can't see it. If another connection blocks this, SQLite says so in the row
    // and not as an error: `migrate` still catches the version, and the next start tries again.
    connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .map_err(saying(CANT_UPDATE))?;

    let (jobs, queue) = mpsc::channel::<Job>();
    let thread = thread::spawn(move || {
        for job in queue {
            job(&mut connection);
        }
    });
    Ok(Store { jobs, thread })
}

/// The `user_version` in a SQLite file's header. Reading the bytes leaves no trace, where even a read-only
/// connection to a WAL database creates `-wal` and `-shm` files. Any other file has no version to read.
///
/// A crash between a migration's commit and its checkpoint leaves a newer version in the WAL only. `migrate`
/// catches that once the file is open, and closing that connection then moves the WAL into the file.
fn version_in_header(path: &Path) -> Option<u32> {
    let mut file = File::open(path).ok()?;
    let mut signature = [0; SQLITE_SIGNATURE.len()];
    file.read_exact(&mut signature).ok()?;
    let _position = file.seek(SeekFrom::Start(USER_VERSION_OFFSET)).ok()?;
    let mut version = [0; 4];
    file.read_exact(&mut version).ok()?;
    (signature == *SQLITE_SIGNATURE).then_some(u32::from_be_bytes(version))
}

fn user_version(connection: &Connection) -> rusqlite::Result<u32> {
    connection.pragma_query_value(None, "user_version", |row| row.get(0))
}

/// Runs the steps this file hasn't had yet, in one transaction, after a backup if it holds anything. Two
/// Tsugikis could race here until #34 makes it a single instance.
fn migrate(
    connection: &mut Connection,
    path: &Path,
    migrations: &[Migration],
) -> Result<(), Error> {
    let version = user_version(connection).map_err(saying(CANT_UPDATE))?;
    if version > migrations.last().map_or(0, |step| step.version) {
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
        .query_row("SELECT EXISTS (SELECT 1 FROM sqlite_master)", [], |row| {
            row.get(0)
        })
        .map_err(saying(CANT_UPDATE))?;
    if holds_something {
        back_up(connection, path, version)?;
    }
    let run_all = |connection: &mut Connection| -> rusqlite::Result<()> {
        let transaction = connection.transaction()?;
        for step in pending {
            transaction.execute_batch(step.sql)?;
            transaction.pragma_update(None, "user_version", step.version)?;
        }
        transaction.commit()
    };
    run_all(connection).map_err(saying(CANT_UPDATE))
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
        .execute("VACUUM INTO ?1", [target.as_ref()])
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
        #[expect(
            clippy::let_underscore_must_use,
            reason = "see above; the cause goes to the local log once #58 adds one"
        )]
        let _ = fs::remove_file(old);
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut longer = path.as_os_str().to_owned();
    longer.push(suffix);
    PathBuf::from(longer)
}

/// Turns a failure into words a user can read (`Error`). SQLite's or the OS's reason goes to the local log
/// once #58 adds one.
fn saying<E>(message: &'static str) -> impl FnOnce(E) -> Error {
    move |_cause| Error::new(ErrorKind::Internal, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    const V1: Migration = Migration {
        version: 1,
        sql: "CREATE TABLE notes (id INTEGER PRIMARY KEY, text TEXT NOT NULL);
              INSERT INTO notes (text) VALUES ('kept');",
    };
    const V2: Migration = Migration {
        version: 2,
        sql: "CREATE TABLE tags (id INTEGER PRIMARY KEY);",
    };
    const V3: Migration = Migration {
        version: 3,
        sql: "CREATE TABLE labels (id INTEGER PRIMARY KEY);",
    };
    const V3_BROKEN: Migration = Migration {
        version: 3,
        sql: "CREATE TABLE half_done (id INTEGER PRIMARY KEY); THIS IS NOT SQL;",
    };

    /// The path of a database in a new temp folder, and that folder.
    fn temp_database(name: &str) -> (PathBuf, PathBuf) {
        let folder =
            std::env::temp_dir().join(format!("tsugiki-store-{name}-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        (folder.join("tsugiki.db"), folder)
    }

    fn user_version(path: &Path) -> u32 {
        Connection::open(path)
            .unwrap()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    fn has_table(path: &Path, table: &str) -> bool {
        Connection::open(path)
            .unwrap()
            .prepare(&format!("SELECT 1 FROM {table}"))
            .is_ok()
    }

    fn backups(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".bak"))
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_new_database_gets_the_whole_schema_and_no_backup() {
        let (path, folder) = temp_database("new");
        Store::open(&path).unwrap().close();

        let latest = MIGRATIONS.last().unwrap().version;
        assert_eq!(user_version(&path), latest);
        assert!(has_table(&path, "viewer"));
        assert_eq!(backups(&folder), [""; 0]);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn the_folder_is_created_when_it_is_missing() {
        let (_, folder) = temp_database("missing-folder");
        let path = folder.join("below").join("tsugiki.db");
        Store::open(&path).unwrap().close();
        assert!(path.exists());
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn the_database_runs_in_wal_mode() {
        let (path, folder) = temp_database("wal");
        Store::open(&path).unwrap().close();

        let mode: String = Connection::open(&path)
            .unwrap()
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        fs::remove_dir_all(folder).unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn nobody_is_remembered_at_first() {
        let (path, folder) = temp_database("first");
        let store = Store::open(&path).unwrap();
        assert_eq!(store.last_login().unwrap(), None);
        store.close();
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn the_login_survives_a_restart_and_the_newest_one_wins() {
        let (path, folder) = temp_database("restart");
        let store = Store::open(&path).unwrap();
        store.remember_login(&Login::new("octocat")).unwrap();
        store.remember_login(&Login::new("hubot")).unwrap();
        store.close();

        let store = Store::open(&path).unwrap();
        assert_eq!(store.last_login().unwrap(), Some(Login::new("hubot")));
        store.close();
        fs::remove_dir_all(folder).unwrap();
    }

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
    fn a_migration_reaches_the_file_while_the_store_is_still_open() {
        let (path, folder) = temp_database("checkpoint");
        open_with(&path, &[V1]).unwrap().close();

        // Tsugiki normally quits without closing the store, so what the header says can't wait for `close`.
        let store = open_with(&path, &[V1, V2]).unwrap();
        assert_eq!(version_in_header(&path), Some(2));

        store.close();
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_new_database_reaches_the_file_while_the_store_is_still_open() {
        let (path, folder) = temp_database("checkpoint-new");
        let store = open_with(&path, &[V1]).unwrap();
        assert_eq!(version_in_header(&path), Some(1));
        store.close();
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_newer_database_is_refused_and_left_untouched() {
        let (path, folder) = temp_database("newer");
        open_with(&path, &[V1, V2]).unwrap().close();
        let before = fs::read(&path).unwrap();

        let problem = open_with(&path, &[V1]).err().unwrap();

        assert_eq!(problem.kind, ErrorKind::Internal);
        assert!(problem.message.contains("newer"), "{}", problem.message);
        assert_eq!(fs::read(&path).unwrap(), before);
        let names: Vec<_> = fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["tsugiki.db"], "no -wal or -shm files either");
        assert_eq!(backups(&folder), [""; 0]);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_file_that_isnt_a_database_is_an_error_and_left_untouched() {
        let (path, folder) = temp_database("not-a-database");
        let text = "this is not a database, and it is long enough for SQLite to look at".repeat(9);
        fs::write(&path, &text).unwrap();

        let problem = Store::open(&path).err().unwrap();

        assert_eq!(problem.kind, ErrorKind::Internal);
        assert_eq!(problem.message, CANT_OPEN, "plain words, not SQLite's");
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_file_too_short_to_be_a_database_is_not_called_newer() {
        let (path, folder) = temp_database("short");
        for length in [1, 20] {
            fs::write(&path, "x".repeat(length)).unwrap();
            let problem = Store::open(&path).err().unwrap();
            assert!(!problem.message.contains("newer"), "{}", problem.message);
        }
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_newer_version_still_in_the_wal_is_refused_too() {
        let (path, folder) = temp_database("newer-in-wal");
        open_with(&path, &[V1]).unwrap().close();
        // While this stays open its change sits in the `-wal` file, and the header still says version 1.
        let writer = Connection::open(&path).unwrap();
        writer.pragma_update(None, "user_version", 2).unwrap();
        assert_eq!(version_in_header(&path), Some(1));

        let problem = open_with(&path, &[V1]).err().unwrap();

        assert!(problem.message.contains("newer"), "{}", problem.message);
        drop(writer);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_folder_that_cant_be_created_is_an_error() {
        let (_, folder) = temp_database("blocked-folder");
        fs::write(folder.join("a-file"), "").unwrap();
        let path = folder.join("a-file").join("tsugiki.db");

        let problem = Store::open(&path).err().unwrap();

        assert_eq!(problem.kind, ErrorKind::Internal);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_panic_on_the_database_thread_comes_back_as_an_error() {
        let store = Store::open(Path::new(":memory:")).unwrap();

        let panicked = store.call(|_| panic!("a panic on the database thread"));
        let after = store.last_login();
        store.close();

        let stopped = Error::new(ErrorKind::Internal, STOPPED);
        assert_eq!(panicked.unwrap_err(), stopped);
        assert_eq!(after.unwrap_err(), stopped);
    }

    /// The queries screens wait on, which the plan check below runs.
    const VIEW_QUERIES: [&str; 1] = [LAST_LOGIN];

    /// Tables that stay a few rows long, so reading all of them is fine. Any other table counts as large,
    /// so a new one is checked until someone says otherwise.
    const SMALL_TABLES: [&str; 1] = ["viewer"];

    /// The tables that `sql` reads end to end, other than `SMALL_TABLES`.
    fn full_scans(connection: &Connection, sql: &str) -> Vec<String> {
        let mut plan = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap();
        let mut rows = plan.raw_query();
        let mut scans = Vec::new();
        while let Some(row) = rows.next().unwrap() {
            let detail: String = row.get("detail").unwrap();
            let table = detail
                .strip_prefix("SCAN ")
                .and_then(|rest| rest.split(' ').next());
            if let Some(table) = table.filter(|table| !SMALL_TABLES.contains(table)) {
                scans.push(table.to_owned());
            }
        }
        scans
    }

    #[test]
    fn no_view_query_scans_a_large_table() {
        let connection = Connection::open_in_memory().unwrap();
        for step in MIGRATIONS {
            connection.execute_batch(step.sql).unwrap();
        }
        for query in VIEW_QUERIES {
            assert_eq!(full_scans(&connection, query), [""; 0], "{query}");
        }
    }

    #[test]
    fn the_plan_check_finds_a_full_scan_of_a_large_table() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE issues (id INTEGER PRIMARY KEY, title TEXT, author TEXT);
                 CREATE INDEX issues_by_author ON issues (author);
                 CREATE TABLE viewer (id INTEGER PRIMARY KEY, login TEXT);",
            )
            .unwrap();

        let no_index = "SELECT title FROM issues WHERE title = ?1";
        let whole_index = "SELECT author FROM issues";
        assert_eq!(full_scans(&connection, no_index), ["issues"]);
        assert_eq!(full_scans(&connection, whole_index), ["issues"]);

        let by_key = "SELECT title FROM issues WHERE id = ?1";
        let by_index = "SELECT title FROM issues WHERE author = ?1";
        let small = "SELECT login FROM viewer";
        assert_eq!(full_scans(&connection, by_key), [""; 0]);
        assert_eq!(full_scans(&connection, by_index), [""; 0]);
        assert_eq!(full_scans(&connection, small), [""; 0]);
    }

    #[test]
    fn the_migrations_count_up_from_one() {
        for (expected, step) in (1..).zip(MIGRATIONS) {
            assert_eq!(step.version, expected);
        }
    }
}
