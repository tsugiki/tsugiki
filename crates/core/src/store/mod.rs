//! Tsugiki keeps what it learns in one SQLite file (ADR 0010). `Store` is the one writer, and `StoreReader`
//! is a read-only connection for screens, so a screen never waits on a sync (ADR 0018). Each connection has
//! a thread that runs its jobs in turn, so a caller waits only for the jobs ahead of its own on that
//! connection.

mod columns;
mod file;
mod filters;
mod issues;
mod migrate;
mod pull_requests;
mod sync_state;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;
use std::fs;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Weak};
use std::thread::{self, JoinHandle};

use rusqlite::{Connection, OpenFlags, OptionalExtension, named_params};

use crate::error::{Error, ErrorKind};
use crate::filters::FilterQuery;
use crate::github::Login;
use crate::github::issues::IssueFacts;
use crate::github::pull_requests::PullRequestFacts;
use crate::repository::RepositoryName;
pub use filters::{FilterProblem, FilterResults, NarrowedResults};
use migrate::{MIGRATIONS, Migration};
pub use sync_state::{ChangeCheck, FirstSyncPlace, SyncState};

const CANT_OPEN: &str = "Tsugiki can't open its database";
const CANT_READ: &str = "Tsugiki can't read its database";
const CANT_WRITE: &str = "Tsugiki can't write to its database";
const CANT_UPDATE: &str = "Tsugiki can't update its database to this version";
const NEWER: &str = "A newer version of Tsugiki wrote this database, so this version won't open it. \
                     Update Tsugiki to read it.";
const STOPPED: &str = "Tsugiki's database stopped working. Restart Tsugiki to open it again.";

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

    /// The user signed in when Tsugiki last reached GitHub, if it ever has.
    pub fn last_login(&self) -> Result<Option<Login>, Error> {
        let found = self.call(|connection| read_last_login(connection))?;
        found.map_err(saying(CANT_READ))
    }

    pub fn remember_login(&self, login: &Login) -> Result<(), Error> {
        let login = login.clone();
        let written = self.call(move |connection| write_last_login(connection, &login))?;
        written.map_err(saying(CANT_WRITE))
    }

    /// Stores `pull_requests` of `repository` in one transaction (ADR 0010).
    pub fn save_pull_requests(
        &self,
        repository: &RepositoryName,
        pull_requests: Vec<PullRequestFacts>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self
            .call(move |connection| pull_requests::save(connection, &repository, &pull_requests))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Each PR Tsugiki has of `repository`, for a sync to see what it's missing.
    pub fn pull_requests(
        &self,
        repository: &RepositoryName,
    ) -> Result<Vec<PullRequestFacts>, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| pull_requests::read_all(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    /// The merged and closed PRs of `repository`, which a sync doesn't fetch again (ADR 0011).
    pub fn frozen_pull_requests(
        &self,
        repository: &RepositoryName,
    ) -> Result<Vec<PullRequestFacts>, Error> {
        let mut found = self.pull_requests(repository)?;
        found.retain(PullRequestFacts::is_frozen);
        Ok(found)
    }

    /// Each Issue Tsugiki has of `repository`, for a sync to see what it's missing.
    pub fn issues(&self, repository: &RepositoryName) -> Result<Vec<IssueFacts>, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| issues::read_all(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    /// Replaces what the search `query` found in `repository`, in one transaction (ADR 0010).
    pub fn save_filter_results(
        &self,
        repository: &RepositoryName,
        query: &FilterQuery,
        results: FilterResults,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let query = query.clone();
        let saved =
            self.call(move |connection| filters::save(connection, &repository, &query, &results))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Applies a narrowed search to what `query` found in `repository`, in one transaction (ADR 0010).
    /// Answers with the numbers that joined or left the Filter.
    pub fn merge_filter_results(
        &self,
        repository: &RepositoryName,
        query: &FilterQuery,
        narrowed: NarrowedResults,
    ) -> Result<BTreeSet<u32>, Error> {
        let repository = repository.clone();
        let query = query.clone();
        let merge = move |connection: &mut Connection| {
            filters::merge(connection, &repository, &query, &narrowed)
        };
        let merged = self.call(merge)?;
        merged.map_err(saying(CANT_WRITE))
    }

    /// Each search of `repository` that has run, with its results, for a refresh to see how to ask again.
    pub fn filter_results(
        &self,
        repository: &RepositoryName,
    ) -> Result<BTreeMap<FilterQuery, FilterResults>, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| filters::read_all(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    /// The last sync of `repository` that finished, if one has.
    pub fn sync_state(&self, repository: &RepositoryName) -> Result<Option<SyncState>, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| sync_state::read(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    pub fn save_sync_state(
        &self,
        repository: &RepositoryName,
        state: SyncState,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| {
            let transaction = connection.transaction()?;
            sync_state::save(&transaction, &repository, &state)?;
            sync_state::forget_place(&transaction, &repository)?;
            transaction.commit()
        })?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// The place this Repository's first sync got to, if one stopped partway.
    pub fn first_sync_place(
        &self,
        repository: &RepositoryName,
    ) -> Result<Option<FirstSyncPlace>, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| sync_state::read_place(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    pub fn save_first_sync_place(
        &self,
        repository: &RepositoryName,
        place: FirstSyncPlace,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved =
            self.call(move |connection| sync_state::save_place(connection, &repository, &place))?;
        saved.map_err(saying(CANT_WRITE))
    }

    #[cfg(test)]
    pub(crate) fn forget_sync_state(&self, repository: &RepositoryName) -> Result<(), Error> {
        let repository = repository.clone();
        let forgot = self.call(move |connection| sync_state::forget(connection, &repository))?;
        forgot.map_err(saying(CANT_WRITE))
    }

    /// Says what stopped the search `query`, and leaves what it last found.
    pub fn save_filter_problem(
        &self,
        repository: &RepositoryName,
        query: &FilterQuery,
        problem: &str,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let query = query.clone();
        let problem = problem.to_owned();
        let save = move |connection: &mut Connection| {
            filters::save_problem(connection, &repository, &query, &problem)
        };
        let saved = self.call(save)?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Removes the results of each search of `repository` outside `keep`, which the user has edited away.
    pub fn keep_only_filters(
        &self,
        repository: &RepositoryName,
        keep: Vec<FilterQuery>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved =
            self.call(move |connection| filters::keep_only(connection, &repository, &keep))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Stores `issues` of `repository` in one transaction (ADR 0010).
    pub fn save_issues(
        &self,
        repository: &RepositoryName,
        issues: Vec<IssueFacts>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| issues::save(connection, &repository, &issues))?;
        saved.map_err(saying(CANT_WRITE))
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
        ask(&self.jobs, work)
    }

    /// This store, except that call number `lost`, if any, goes missing as if its thread had stopped. The
    /// other calls reach the database in order, so a step can save one thing and fail on the next. The
    /// receiver hears of each call.
    #[cfg(test)]
    pub(crate) fn failing_at_call(
        &self,
        lost: Option<usize>,
    ) -> (Self, std::sync::mpsc::Receiver<usize>) {
        let (jobs, calls) = std::sync::mpsc::channel::<Job>();
        let (seen, heard) = std::sync::mpsc::channel();
        let database = self.jobs.clone();
        let thread = std::thread::spawn(move || {
            for (call, job) in calls.iter().enumerate() {
                seen.send(call).expect("the test holds the receiver");
                if lost != Some(call) {
                    drop(database.send(job));
                }
            }
        });
        (Self { jobs, thread }, heard)
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

/// The facts a screen needs to work out the Workbench, read at one moment.
#[derive(Debug, PartialEq, Eq)]
pub struct WorkFacts {
    /// The user signed in when Tsugiki last reached GitHub, if it ever has.
    pub viewer: Option<Login>,
    pub pull_requests: Vec<PullRequestFacts>,
    pub issues: Vec<IssueFacts>,
    /// Each search of this Repository, with its last results.
    pub filters: BTreeMap<FilterQuery, FilterResults>,
}

fn read_last_login(connection: &Connection) -> rusqlite::Result<Option<Login>> {
    connection
        .query_row("SELECT login FROM viewer WHERE id = 1", [], |row| {
            row.get("login")
        })
        .optional()
}

fn write_last_login(connection: &Connection, login: &Login) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT INTO viewer (id, login) VALUES (1, :login)
             ON CONFLICT (id) DO UPDATE SET login = excluded.login
             WHERE login <> excluded.login",
            named_params! { ":login": login },
        )
        .map(drop)
}

// Add each read a screen waits on to `view_statements` in the tests, which check its plans (ADR 0017).
fn read_work_facts(
    connection: &mut Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<WorkFacts> {
    let transaction = connection.transaction()?;
    Ok(WorkFacts {
        viewer: read_last_login(&transaction)?,
        pull_requests: pull_requests::read_all(&transaction, repository)?,
        issues: issues::read_all(&transaction, repository)?,
        filters: filters::read_all(&transaction, repository)?,
    })
}

/// Asks the read-only connection, from any thread. A question asked before the database opens waits for it.
#[derive(Clone)]
pub struct StoreReader {
    jobs: Weak<Sender<Job>>,
}

impl StoreReader {
    /// The facts behind the Workbench of `repository`.
    pub fn work_facts(&self, repository: &RepositoryName) -> Result<WorkFacts, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| read_work_facts(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    fn call<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Connection) -> T + Send + 'static,
    ) -> Result<T, Error> {
        let stopped = || Error::new(ErrorKind::Internal, STOPPED);
        ask(&*self.jobs.upgrade().ok_or_else(stopped)?, work)
    }
}

/// The read-only connection before the database is ready for it. Dropping this answers what's waiting.
#[must_use = "dropping this tells every reader that the database stopped"]
pub struct ReaderToOpen {
    jobs: Arc<Sender<Job>>,
    queue: Receiver<Job>,
}

/// The read-only connection, on its thread. Only this keeps the thread going, so the app's readers can't keep
/// the file open once the session ends.
#[must_use = "dropping this leaves the connection open until the process ends"]
pub struct OpenReader {
    jobs: Arc<Sender<Job>>,
    thread: JoinHandle<()>,
}

/// A way to ask the read-only connection, and what opens it when `Store::open` has made the file current.
pub fn reader() -> (ReaderToOpen, StoreReader) {
    let (jobs, queue) = mpsc::channel::<Job>();
    let jobs = Arc::new(jobs);
    let reader = StoreReader {
        jobs: Arc::downgrade(&jobs),
    };
    (ReaderToOpen { jobs, queue }, reader)
}

impl ReaderToOpen {
    /// Opens `path` read-only, on a thread of its own.
    pub fn open(self, path: &Path) -> Result<OpenReader, Error> {
        let mut connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(saying(CANT_OPEN))?;
        let queue = self.queue;
        let thread = thread::spawn(move || {
            for job in queue {
                job(&mut connection);
            }
        });
        Ok(OpenReader {
            jobs: self.jobs,
            thread,
        })
    }
}

impl OpenReader {
    /// Finishes the jobs it has and closes the connection. Close this before `Store`: the last connection
    /// to close removes the `-wal` and `-shm` files.
    pub fn close(self) {
        drop(self.jobs);
        #[expect(
            clippy::let_underscore_must_use,
            reason = "a thread that panicked has closed the file too, and `call` has already said so"
        )]
        let _ = self.thread.join();
    }
}

/// Runs `work` on the thread that owns a connection, and waits for its answer.
fn ask<T: Send + 'static>(
    jobs: &Sender<Job>,
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
    jobs.send(job)
        .ok()
        .and_then(|()| answer.recv().ok())
        .ok_or_else(|| Error::new(ErrorKind::Internal, STOPPED))
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
            .pragma_query_value(None, "user_version", |row| row.get("user_version"))
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
    use crate::github::CommitSha;
    use crate::github::pull_requests::{CiState, LatestReview, OffDefault, PrState, ReviewState};
    use rusqlite::trace::{TraceEvent, TraceEventCodes};
    use time::OffsetDateTime;

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
            .pragma_query_value(None, "journal_mode", |row| row.get("journal_mode"))
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

    /// Each statement the reads a screen waits on run, as SQLite traces them. One row in each table makes a
    /// query inside a loop over rows run too. A query behind a condition these rows don't meet stays unseen.
    fn view_statements(connection: &mut Connection) -> Vec<String> {
        let cpython = RepositoryName::parse("python/cpython").unwrap();
        let viewer = Login::new("octocat");
        let saved_at = OffsetDateTime::UNIX_EPOCH;
        let pull_request = PullRequestFacts {
            number: 1,
            title: "gh-1: Fix it".to_owned(),
            state: PrState::Open,
            is_draft: false,
            author: Some(Login::new("some-author")),
            target_branch: "main".to_owned(),
            ci: Some(CiState::Success),
            updated_at: saved_at,
            review_requests: vec![viewer.clone()],
            latest_reviews: vec![LatestReview {
                author: Login::new("some-reviewer"),
                state: ReviewState::Approved,
            }],
            assignees: vec![Login::new("some-assignee")],
            closing_issues: vec![7],
            merge_commit: CommitSha::parse("0123abc"),
            off_default: Some(OffDefault {
                description: String::new(),
                cherry_picked_from: CommitSha::parse("0123abc").into_iter().collect(),
                committers: vec![Some(viewer.clone())],
            }),
        };
        let issue = IssueFacts {
            number: 1,
            title: "It broke".to_owned(),
            author: Some(Login::new("some-author")),
            updated_at: saved_at,
        };
        write_last_login(connection, &viewer).unwrap();
        pull_requests::save(connection, &cpython, &[pull_request]).unwrap();
        issues::save(connection, &cpython, &[issue]).unwrap();
        let query: FilterQuery = "is:open".to_owned().try_into().unwrap();
        let results = FilterResults {
            numbers: [1].into(),
            problem: Some(FilterProblem::Refused("It was cut off".to_owned())),
        };
        filters::save(connection, &cpython, &query, &results).unwrap();
        let (seen, statements) = mpsc::channel();
        let trace = move |event: TraceEvent<'_>| {
            if let TraceEvent::Stmt(_, sql) = event {
                seen.send(sql.to_owned()).unwrap();
            }
        };
        let codes = TraceEventCodes::SQLITE_TRACE_STMT;
        connection.trace_v2(codes, Some(trace)).unwrap();
        let read = read_work_facts(connection, &cpython).unwrap();
        assert_eq!(read.pull_requests.len(), 1);
        let no_trace = None::<fn(TraceEvent<'_>)>;
        connection.trace_v2(codes, no_trace).unwrap();
        statements.try_iter().collect()
    }

    fn migrated() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        for step in MIGRATIONS {
            connection.execute_batch(step.sql).unwrap();
        }
        connection
    }

    #[test]
    fn no_view_query_scans_a_large_table() {
        let mut connection = migrated();
        for sql in view_statements(&mut connection) {
            assert_eq!(full_scans(&connection, &sql), [""; 0], "{sql}");
        }
    }

    #[test]
    fn the_plan_check_sees_each_table_a_view_reads() {
        let statements = view_statements(&mut migrated());
        let reads = |table: &str| {
            let from = format!("FROM {table}");
            statements.iter().any(|sql| sql.contains(&from))
        };
        let tables = [
            "viewer",
            "pull_requests",
            "review_requests",
            "latest_reviews",
            "assignees",
            "closing_references",
            "cherry_picks",
            "committers",
            "issues",
            "filter_matches",
            "filter_problems",
        ];
        assert_eq!(tables.map(reads), [true; 11]);
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

        let no_index = "SELECT title FROM issues WHERE title = :title";
        let whole_index = "SELECT author FROM issues";
        assert_eq!(full_scans(&connection, no_index), ["issues"]);
        assert_eq!(full_scans(&connection, whole_index), ["issues"]);

        let by_key = "SELECT title FROM issues WHERE id = :id";
        let by_index = "SELECT title FROM issues WHERE author = :author";
        let small = "SELECT login FROM viewer";
        assert_eq!(full_scans(&connection, by_key), [""; 0]);
        assert_eq!(full_scans(&connection, by_index), [""; 0]);
        assert_eq!(full_scans(&connection, small), [""; 0]);
    }
}
