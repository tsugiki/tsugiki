//! Tsugiki keeps what it learns in one SQLite file (ADR 0010). `Store` is the one writer, and `StoreReader`
//! is a read-only connection for screens, so a screen never waits on a sync (ADR 0018). Each connection has
//! a thread that runs its jobs in turn, so a caller waits only for the jobs ahead of its own on that
//! connection.

mod activity;
mod columns;
mod failed_checks;
mod file;
mod filters;
mod issue_content;
mod issues;
mod migrate;
mod patch_ids;
mod pull_request_content;
mod pull_requests;
/// Draws the schema in `docs/schema.md` and keeps it current. Only its tests use it.
#[cfg(test)]
mod schema;
mod sync_state;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rusqlite::{OpenFlags, OptionalExtension, Transaction, named_params};
use time::OffsetDateTime;

use crate::error::{Error, ErrorKind};
use crate::filters::FilterQuery;
use crate::github::activity::ItemActivity;
use crate::github::issue_content::IssueContent;
use crate::github::issues::IssueFacts;
use crate::github::pull_request_content::PullRequestContent;
use crate::github::pull_requests::{PullRequestFacts, ReviewedCommit};
use crate::login::Login;
use crate::patch_ids::CommitPatches;
use crate::repository::RepositoryName;
pub use activity::ActivityFacts;
pub use filters::{FilterProblem, FilterResults, NarrowedResults};
use migrate::{MIGRATIONS, Migration};
pub use patch_ids::ReviewFacts;
pub use sync_state::{ChangeCheck, FirstSyncPlace, SyncState};

/// The database connection, which only `store` may hold. Code here names it through this alias, so
/// Clippy's ban on `rusqlite::Connection` holds everywhere else, and its other bans hold here too.
#[expect(
    clippy::disallowed_types,
    reason = "store is the one module that holds a database connection, so its tables stay its own"
)]
type Connection = rusqlite::Connection;

const CANT_OPEN: &str = "Tsugiki can't open its database";
const CANT_READ: &str = "Tsugiki can't read its database";
const CANT_WRITE: &str = "Tsugiki can't write to its database";
const CANT_UPDATE: &str = "Tsugiki can't update its database to this version";
const NEWER: &str = "A newer version of Tsugiki wrote this database, so this version won't open it. \
                     Update Tsugiki to read it.";
pub(crate) const IN_USE: &str = "Another Tsugiki has this database open, so this one leaves it alone. \
                                 Use that one, or close it and start Tsugiki again.";
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
        self.save_fetched_pull_requests(&repository, pull_requests, Vec::new())
    }

    /// Stores `pull_requests` and what GitHub says happened on them in one transaction, so a crash can't
    /// keep a PR's new `updatedAt` and lose the activity that a later sync then has no reason to ask for.
    pub fn save_fetched_pull_requests(
        &self,
        repository: &RepositoryName,
        pull_requests: Vec<PullRequestFacts>,
        events: Vec<ItemActivity>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| {
            pull_requests::save(connection, &repository, &pull_requests, &events)
        })?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Stores PRs a refresh fetched or probed, as `save_fetched_pull_requests` does, and in the same
    /// transaction owes a read of the Conversation of each of `changed`. A refresh that stops after this has
    /// saved them can't tell they changed, so the next one reads them ahead from here (#205).
    pub fn save_changed_pull_requests(
        &self,
        repository: &RepositoryName,
        (pull_requests, events): (Vec<PullRequestFacts>, Vec<ItemActivity>),
        changed: Vec<u32>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| {
            let transaction = connection.transaction()?;
            pull_requests::save_in(&transaction, &repository, &pull_requests, &events)?;
            sync_state::owe_read_ahead(&transaction, &repository, &changed)?;
            transaction.commit()
        })?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// The PRs of `repository` whose Conversations a refresh owes a read. Saving a PR's Conversation clears
    /// its debt, and saving a finished sync clears them all.
    pub fn read_ahead_owed(&self, repository: &RepositoryName) -> Result<BTreeSet<u32>, Error> {
        let repository = repository.clone();
        let read =
            self.call(move |connection| sync_state::read_ahead_owed(connection, &repository))?;
        read.map_err(saying(CANT_READ))
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

    /// The Issues of `repository` saved before Tsugiki kept states, which a sync fetches once (#231).
    pub fn issues_without_state(&self, repository: &RepositoryName) -> Result<Vec<u32>, Error> {
        let repository = repository.clone();
        let read =
            self.call(move |connection| issues::numbers_without_state(connection, &repository))?;
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

    /// The facts behind the Workbench of `repository`, as this writer last saved them.
    pub fn work_facts(&self, repository: &RepositoryName) -> Result<WorkFacts, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| read_work_facts(connection, &repository))?;
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
        let started_at = state.started_at;
        let saved = self.call(move |connection| {
            let transaction = connection.transaction()?;
            sync_state::save(&transaction, &repository, &state)?;
            sync_state::forget_place(&transaction, &repository)?;
            activity::remember_first_sync(&transaction, &repository, started_at)?;
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
        self.save_fetched_issues(repository, issues, Vec::new())
    }

    /// Stores `issues` and what GitHub says happened on them in one transaction, as
    /// `save_fetched_pull_requests` does.
    pub fn save_fetched_issues(
        &self,
        repository: &RepositoryName,
        issues: Vec<IssueFacts>,
        events: Vec<ItemActivity>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved =
            self.call(move |connection| issues::save(connection, &repository, &issues, &events))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// The numbers of `viewer`'s open PRs whose CI failed after Tsugiki last asked which checks failed.
    pub fn pull_requests_with_unread_checks(
        &self,
        repository: &RepositoryName,
        viewer: &Login,
    ) -> Result<Vec<u32>, Error> {
        let repository = repository.clone();
        let viewer = viewer.clone();
        let read =
            self.call(move |connection| failed_checks::unread(connection, &repository, &viewer))?;
        read.map_err(saying(CANT_READ))
    }

    /// Replaces what the store knows about each PR's failed checks.
    pub fn save_failed_checks(
        &self,
        repository: &RepositoryName,
        read: Vec<(u32, Vec<String>)>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved =
            self.call(move |connection| failed_checks::save(connection, &repository, &read))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Replaces what's kept of what Issue `number` says.
    pub fn save_issue_content(
        &self,
        repository: &RepositoryName,
        number: u32,
        content: IssueContent,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| {
            issue_content::save(connection, &repository, number, &content)
        })?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Keeps what PR `number` says, its Reviews included, in place of what was kept before. A refresh no
    /// longer owes a read of it.
    pub fn save_pull_request_content(
        &self,
        repository: &RepositoryName,
        number: u32,
        content: PullRequestContent,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| {
            pull_request_content::save(connection, &repository, number, &content)
        })?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// The commit each of the user's Reviews in `repository` covered, for its mirror to keep. None before
    /// Tsugiki knows who the user is.
    pub fn reviewed_commits(
        &self,
        repository: &RepositoryName,
    ) -> Result<Vec<ReviewedCommit>, Error> {
        let repository = repository.clone();
        let reviewed = move |connection: &mut Connection| {
            pull_request_content::reviewed_commits(connection, &repository)
        };
        let read = self.call(reviewed)?;
        read.map_err(saying(CANT_READ))
    }

    /// Keeps the per-file patch-ids of each commit in `patches`, which the Stale review check compares.
    pub fn save_patch_ids(
        &self,
        repository: &RepositoryName,
        patches: Vec<CommitPatches>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved =
            self.call(move |connection| patch_ids::save(connection, &repository, &patches))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Mark Seen on items `numbers`: clears their Marked unseen, and moves their Last seen on to `up_to`, if
    /// it's given.
    pub fn mark_seen(
        &self,
        repository: &RepositoryName,
        numbers: Vec<u32>,
        up_to: Option<OffsetDateTime>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| {
            activity::mark_seen(connection, &repository, &numbers, up_to)
        })?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Flags item `number` Marked unseen from `at`.
    pub fn mark_unseen(
        &self,
        repository: &RepositoryName,
        number: u32,
        at: OffsetDateTime,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self
            .call(move |connection| activity::mark_unseen(connection, &repository, number, at))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// The events kept for item `number`, oldest first.
    #[cfg(test)]
    pub fn activity(
        &self,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<Vec<crate::github::activity::Activity>, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| activity::read(connection, &repository, number))?;
        read.map_err(saying(CANT_READ))
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
    pub activity: ActivityFacts,
    pub reviews: ReviewFacts,
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
    work_facts_in(&connection.transaction()?, repository)
}

fn work_facts_in(
    transaction: &Transaction<'_>,
    repository: &RepositoryName,
) -> rusqlite::Result<WorkFacts> {
    let viewer = read_last_login(transaction)?;
    let pull_requests = pull_requests::read_all(transaction, repository)?;
    Ok(WorkFacts {
        activity: activity::read_facts(transaction, repository, viewer.as_ref())?,
        reviews: patch_ids::read_facts(transaction, repository, &pull_requests)?,
        viewer,
        pull_requests,
        issues: issues::read_all(transaction, repository)?,
        filters: filters::read_all(transaction, repository)?,
    })
}

/// The facts behind the Issue view, from one transaction, so a view never mixes two saves (ADR 0018).
fn read_issue_facts(
    connection: &mut Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<(WorkFacts, Option<IssueContent>)> {
    let transaction = connection.transaction()?;
    let facts = work_facts_in(&transaction, repository)?;
    let content = issue_content::read(&transaction, repository, number)?;
    Ok((facts, content))
}

/// The facts behind the PR view of PR `number`, from one transaction (ADR 0018).
fn read_pull_request_facts(
    connection: &mut Connection,
    repository: &RepositoryName,
    number: u32,
) -> rusqlite::Result<(WorkFacts, Option<PullRequestContent>)> {
    let transaction = connection.transaction()?;
    let facts = work_facts_in(&transaction, repository)?;
    let content = pull_request_content::read(&transaction, repository, number)?;
    Ok((facts, content))
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

    /// The facts behind the Issue view of Issue `number`.
    pub fn issue_facts(
        &self,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<(WorkFacts, Option<IssueContent>), Error> {
        let repository = repository.clone();
        let read =
            self.call(move |connection| read_issue_facts(connection, &repository, number))?;
        read.map_err(saying(CANT_READ))
    }

    /// The facts behind the PR view of PR `number`.
    pub fn pull_request_facts(
        &self,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<(WorkFacts, Option<PullRequestContent>), Error> {
        let repository = repository.clone();
        let read =
            self.call(move |connection| read_pull_request_facts(connection, &repository, number))?;
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
    let lock = lock(path)?;
    file::check(path, migrate::latest(migrations))?;

    let mut connection = Connection::open(path).map_err(saying(CANT_OPEN))?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(saying(CANT_OPEN))?;
    // The bundled SQLite enforces foreign keys from the start, but another build might not.
    connection
        .pragma_update(None, "foreign_keys", true)
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
        drop(connection);
        drop(lock);
    });
    Ok(Store { jobs, thread })
}

/// How long a start waits for the lock before it says another Tsugiki has it. Another Tsugiki holds it for as
/// long as it runs. A program that another thread is starting holds a copy of the lock file for a moment,
/// until the system closes the files the program doesn't inherit. On macOS and Linux the lock belongs to the
/// open file and lasts while any copy is open, so a Store that has just closed can still look locked then
/// (#198). Windows starts a program with only the files it inherits.
const LOCK_PATIENCE: Duration = Duration::from_secs(1);

/// How long a start sleeps between two tries at the lock.
const LOCK_RETRY: Duration = Duration::from_millis(20);

/// Takes the lock beside the database at `path`, which one Tsugiki at a time holds while it writes (ADR
/// 0018). It belongs to the folder, so an installed Tsugiki, a development build and a portable copy each
/// hold their own. The system lets go of it when the file closes or the process ends, so a crash can't leave
/// the next start locked out. The empty file stays. A database in memory belongs to its one connection, so it
/// needs none.
fn lock(path: &Path) -> Result<Option<File>, Error> {
    if path == Path::new(":memory:") {
        return Ok(None);
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(migrate::with_suffix(path, ".lock"))
        .map_err(saying(CANT_OPEN))?;
    let deadline = Instant::now() + LOCK_PATIENCE;
    loop {
        match file.try_lock() {
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => thread::sleep(LOCK_RETRY),
            Err(TryLockError::WouldBlock) => return Err(Error::new(ErrorKind::Internal, IN_USE)),
            locked => return locked.map(|()| Some(file)).map_err(saying(CANT_OPEN)),
        }
    }
}

/// Turns a failure into words a user can read (`Error`). SQLite's or the OS's reason goes to the log.
fn saying<E: Display>(message: &'static str) -> impl FnOnce(E) -> Error {
    move |cause| {
        log::error!("{message}: {cause}");
        Error::new(ErrorKind::Internal, message)
    }
}

#[cfg(test)]
pub(super) mod testing;

#[cfg(test)]
mod tests;
