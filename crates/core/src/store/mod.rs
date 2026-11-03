//! What Tsugiki learns, kept in one SQLite file (ADR 0010). One thread owns the connection and runs each job
//! in turn, so a caller waits for the jobs ahead of its own. Screens mustn't wait on a sync (ADR 0010), so
//! the sync has to write outside this queue when it arrives.

mod file;
mod migrate;

use std::fmt::Display;
use std::fs;
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use rusqlite::{Connection, OptionalExtension};

use crate::error::{Error, ErrorKind};
use crate::github::Login;
use migrate::{MIGRATIONS, Migration};

const CANT_OPEN: &str = "Tsugiki can't open its database";
const CANT_READ: &str = "Tsugiki can't read its database";
const CANT_WRITE: &str = "Tsugiki can't write to its database";
const CANT_UPDATE: &str = "Tsugiki can't update its database to this version";
const NEWER: &str = "A newer version of Tsugiki wrote this database, so this version won't open it. \
                     Update Tsugiki to read it.";
const STOPPED: &str = "Tsugiki's database stopped working. Restart Tsugiki to open it again.";

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
    file::check(path, migrate::latest(migrations))?;

    let mut connection = Connection::open(path).map_err(saying(CANT_OPEN))?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(saying(CANT_OPEN))?;
    migrate::run(&mut connection, path, migrations)?;
    // A crash, a force-kill or a quit during a slow request skips closing the store, which would leave a new
    // version in the `-wal` file, where `file::check` can't see it. If another connection blocks this, SQLite
    // says so in the row and not as an error: `migrate` still catches the version, and the next start tries
    // again.
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

/// Turns a failure into words a user can read (`Error`). SQLite's or the OS's reason goes to the log.
fn saying<E: Display>(message: &'static str) -> impl FnOnce(E) -> Error {
    move |cause| {
        log::error!("{message}: {cause}");
        Error::new(ErrorKind::Internal, message)
    }
}

#[cfg(test)]
pub(super) mod testing {
    use super::*;
    use std::path::PathBuf;

    pub(super) const V1: Migration = Migration {
        version: 1,
        sql: "CREATE TABLE notes (id INTEGER PRIMARY KEY, text TEXT NOT NULL);
              INSERT INTO notes (text) VALUES ('kept');",
    };
    pub(super) const V2: Migration = Migration {
        version: 2,
        sql: "CREATE TABLE tags (id INTEGER PRIMARY KEY);",
    };
    pub(super) const V3: Migration = Migration {
        version: 3,
        sql: "CREATE TABLE labels (id INTEGER PRIMARY KEY);",
    };
    pub(super) const V3_BROKEN: Migration = Migration {
        version: 3,
        sql: "CREATE TABLE half_done (id INTEGER PRIMARY KEY); THIS IS NOT SQL;",
    };

    /// The path of a database in a new temp folder, and that folder.
    pub(super) fn temp_database(name: &str) -> (PathBuf, PathBuf) {
        let folder = crate::testing::temp_path(&format!("store-{name}"));
        fs::create_dir_all(&folder).unwrap();
        (folder.join("tsugiki.db"), folder)
    }

    pub(super) fn user_version(path: &Path) -> u32 {
        Connection::open(path)
            .unwrap()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    }

    pub(super) fn has_table(path: &Path, table: &str) -> bool {
        Connection::open(path)
            .unwrap()
            .prepare(&format!("SELECT 1 FROM {table}"))
            .is_ok()
    }

    pub(super) fn backups(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".bak"))
            .collect();
        names.sort();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::file::{Contents, contents};
    use super::testing::{V1, V2, backups, has_table, temp_database, user_version};
    use super::*;

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
    fn a_migration_reaches_the_file_while_the_store_is_still_open() {
        let (path, folder) = temp_database("checkpoint");
        open_with(&path, &[V1]).unwrap().close();

        // A crash or a force-kill skips closing the store, so what the header says can't wait for `close`.
        let store = open_with(&path, &[V1, V2]).unwrap();
        assert_eq!(contents(&path), Contents::Database { version: 2 });

        store.close();
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn a_new_database_reaches_the_file_while_the_store_is_still_open() {
        let (path, folder) = temp_database("checkpoint-new");
        let store = open_with(&path, &[V1]).unwrap();
        assert_eq!(contents(&path), Contents::Database { version: 1 });
        store.close();
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
}
