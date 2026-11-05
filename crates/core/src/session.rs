//! One run of Tsugiki: opens the database, then keeps the status following GitHub until the app ends.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use time::OffsetDateTime;

use crate::error::{Error, ErrorKind};
use crate::folders::Folders;
use crate::github::{GitHub, GitHubAccess};
use crate::repository::{RepositoryCheck, RepositoryName, RepositorySettings};
use crate::status::{Change, LastSync, Status, StatusBoard};
use crate::store::{self, OpenReader, ReaderToOpen, Store, StoreReader};
use crate::sync::{Progress, Syncs};
use crate::token::Token;

// `run` catches a panic in its thread, to keep the window up and say that checking stopped. Tests unwind
// whatever the release profile says, so only this line can catch the setting (docs/build-costs.md).
#[cfg(panic = "abort")]
compile_error!("`session::run` catches panics, so Tsugiki can't build with `panic = \"abort\"`");

const STOPPED_CHECKING: &str = "Tsugiki stopped checking GitHub. Restart Tsugiki to check again.";
const NOT_CONNECTED: &str = "Tsugiki isn't connected to GitHub. The status bar says why.";

/// Shows who was signed in last time, reads gh's token, asks GitHub who it belongs to, and shows the result
/// on `board`. Returns the client when there is a token, even offline, since GitHub may come back.
#[must_use = "without the client, nothing asks GitHub again"]
fn connect(board: &mut StatusBoard, store: &Store, access: GitHubAccess) -> Option<GitHub> {
    // The last login only fills the status bar for a moment, so a failed read doesn't stop Tsugiki
    // connecting. `refresh` uses the store next, and shows the problem if the store is still failing.
    if let Ok(Some(last_login)) = store.last_login() {
        board.set(Status::ConnectingAs { last_login });
    }
    let token = match Token::from_gh(access.run_gh) {
        Ok(token) => token,
        Err(problem) => {
            board.set(Status::Problem { problem });
            return None;
        }
    };
    let github = GitHub::new(access.transport, token);
    refresh(board, store, &github);
    Some(github)
}

/// A run of Tsugiki on its own thread, so the window opens without waiting on the disk, gh or the network
/// (ADR 0017). It shows its own failures on the board.
#[must_use = "dropping the session stops it"]
pub struct Session {
    messages: Sender<Message>,
    finished: Receiver<()>,
    reader: StoreReader,
}

/// An Issue or a PR of a Repository, in a message to the session's thread.
#[derive(Debug, PartialEq, Eq)]
struct Item {
    repository: RepositoryName,
    number: u32,
}

/// What others ask of the session's thread, which alone holds the token and the client.
enum Message {
    Stop,
    Check {
        name: RepositoryName,
        reply: Sender<Result<RepositoryCheck, Error>>,
    },
    Sync {
        repository: RepositorySettings,
    },
    Refresh,
    Seen {
        item: Item,
        at: OffsetDateTime,
    },
    FetchIssue(Item),
}

/// Asks the session about Repositories, from any thread. A check waits for what the session is doing first,
/// which at the start can be `gh` or a slow request, so don't call it from the thread that draws the window.
#[derive(Clone)]
pub struct SessionRequests {
    messages: Sender<Message>,
}

impl SessionRequests {
    /// Asks the session to load the user's work and the Filters of `repository`, and returns at once. A
    /// session with no token or no database has already shown why on the status bar, so this reports no
    /// failure.
    pub fn sync(&self, repository: RepositorySettings) {
        drop(self.messages.send(Message::Sync { repository }));
    }

    /// Asks the session to refresh the open Repository now, and returns at once. A sync already under way
    /// covers this.
    pub fn refresh(&self) {
        drop(self.messages.send(Message::Refresh));
    }

    /// Tells the session the user opened Issue `number`, which moves its Last seen and reads what it says
    /// from GitHub, and returns at once. A session that can't ask GitHub still records Last seen.
    pub fn open_issue(&self, repository: RepositoryName, number: u32) {
        self.seen(repository.clone(), number);
        self.fetch_issue(repository, number);
    }

    /// Asks the session to read what an Issue on screen says now, and returns at once. This leaves Last seen
    /// where it is, since a window left open doesn't mean the user read what came in.
    pub fn fetch_issue(&self, repository: RepositoryName, number: u32) {
        let item = Item { repository, number };
        drop(self.messages.send(Message::FetchIssue(item)));
    }

    /// Tells the session the user opened item `number`, which moves its Last seen, and returns at once.
    pub fn seen(&self, repository: RepositoryName, number: u32) {
        let item = Item { repository, number };
        let at = OffsetDateTime::now_utc();
        drop(self.messages.send(Message::Seen { item, at }));
    }

    /// Asks GitHub what adding `name` would load. A session with no token or no database answers with an
    /// error, and has already shown why on the status bar.
    pub fn check(&self, name: RepositoryName) -> Result<RepositoryCheck, Error> {
        let (reply, answer) = mpsc::channel();
        self.messages
            .send(Message::Check { name, reply })
            .ok()
            .and_then(|()| answer.recv().ok())
            .unwrap_or_else(|| Err(Error::new(ErrorKind::Offline, NOT_CONNECTED)))
    }
}

/// How often a session asks GitHub again.
#[derive(Debug, Clone, Copy)]
pub struct Intervals {
    /// Asking who is signed in, so the status follows the network going and coming back.
    pub status: Duration,
    /// Refreshing the open Repository (ADR 0015).
    pub repository: Duration,
}

impl Session {
    /// Opens the database and asks GitHub who is signed in, then keeps asking at `intervals`.
    pub fn start(
        mut board: StatusBoard,
        folders: Result<Folders, Error>,
        access: GitHubAccess,
        intervals: Intervals,
    ) -> Self {
        let (messages, incoming) = mpsc::channel();
        let (finishing, finished) = mpsc::channel::<()>();
        let (to_open, reader) = store::reader();
        let database = Database { folders, to_open };
        let refreshes = Refreshes {
            intervals,
            incoming,
        };
        let _ = thread::spawn(move || {
            // `stop` learns the session is over when this goes, however the thread ends.
            let _finishing = finishing;
            run(&mut board, database, access, &refreshes);
        });
        Self {
            messages,
            finished,
            reader,
        }
    }

    /// A way to ask the session about Repositories. While one exists, only `stop` ends the session.
    pub fn requests(&self) -> SessionRequests {
        SessionRequests {
            messages: self.messages.clone(),
        }
    }

    /// The read-only connection, which doesn't wait for anything this session is doing (ADR 0018).
    pub fn reader(&self) -> StoreReader {
        self.reader.clone()
    }

    /// Tells the session to finish, and gives it up to `patience` to close the database.
    ///
    /// Between refreshes it closes at once. A request to GitHub, a call to `gh` or a migration can outlast
    /// `patience`, and then the process ends in the middle of it. SQLite recovers at the next start, as it
    /// does after a crash.
    pub fn stop(&self, patience: Duration) {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "this fails only if the session has already finished, which is what this asks for"
        )]
        let _ = self.messages.send(Message::Stop);
        #[expect(
            clippy::let_underscore_must_use,
            reason = "running out of patience is the one failure here, and the doc above says why it is safe"
        )]
        let _ = self.finished.recv_timeout(patience);
    }
}

/// When to ask GitHub again.
struct Refreshes {
    intervals: Intervals,
    incoming: Receiver<Message>,
}

/// When the next of each kind of question is due.
#[derive(Debug, Clone, Copy)]
struct Due {
    status: Instant,
    repository: Instant,
}

impl Due {
    fn starting_at(now: Instant, intervals: Intervals) -> Self {
        Self {
            status: now + intervals.status,
            repository: now + intervals.repository,
        }
    }
}

/// The session thread's next move.
#[derive(Debug, PartialEq, Eq)]
enum Turn {
    /// Time to ask who is signed in.
    Refresh,
    /// Time to refresh the open Repository.
    RefreshRepository,
    /// The user asked to refresh the open Repository.
    RefreshNow,
    /// The window opened this Repository, so it needs to know when its last sync was.
    Opened(RepositoryName),
    /// The user opened this item at this moment.
    Seen(Item, OffsetDateTime),
    /// An Issue on screen needs what it says read from GitHub.
    FetchIssue(Item),
    SyncStep,
    Stop,
}

impl Refreshes {
    /// Waits for a stop message, for a session with nothing to ask GitHub with. A check dropped here tells
    /// the one asking that the session isn't connected. Last seen needs only the database, so it still moves.
    fn wait_for_stop(&self, board: &mut StatusBoard, store: &Store) {
        while let Ok(message) = self.incoming.recv() {
            match message {
                Message::Stop => return,
                Message::Seen { item, at } => mark_seen(board, store, &item, at),
                Message::Check { .. }
                | Message::Sync { .. }
                | Message::Refresh
                | Message::FetchIssue(_) => {}
            }
        }
    }

    /// Waits for the next refresh or the next step of a sync, answering what's asked meanwhile, so a step
    /// doesn't keep anyone waiting for longer than one request. A dropped `Session` stops it too.
    fn next_turn(&self, github: &GitHub, syncs: &mut Syncs, due: Due) -> Turn {
        let next_refresh = due.status.min(due.repository);
        loop {
            let wake = syncs
                .ready_at()
                .map_or(next_refresh, |at| at.min(next_refresh));
            match self
                .incoming
                .recv_timeout(wake.saturating_duration_since(Instant::now()))
            {
                Ok(Message::Sync { repository }) => {
                    let opened = repository.name.clone();
                    syncs.add(repository);
                    return Turn::Opened(opened);
                }
                Ok(Message::Refresh) => return Turn::RefreshNow,
                Ok(Message::Seen { item, at }) => return Turn::Seen(item, at),
                Ok(Message::FetchIssue(item)) => return Turn::FetchIssue(item),
                Ok(Message::Check { name, reply }) => {
                    let today = OffsetDateTime::now_utc().date();
                    #[expect(
                        clippy::let_underscore_must_use,
                        reason = "this fails only if the caller has gone, and it waits for this answer"
                    )]
                    let _ = reply.send(github.check_repository(&name, today));
                }
                Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return Turn::Stop,
                Err(RecvTimeoutError::Timeout) if Instant::now() >= due.status => {
                    return Turn::Refresh;
                }
                Err(RecvTimeoutError::Timeout) if Instant::now() >= due.repository => {
                    return Turn::RefreshRepository;
                }
                Err(RecvTimeoutError::Timeout) => return Turn::SyncStep,
            }
        }
    }
}

/// The database a session opens when it starts.
struct Database {
    folders: Result<Folders, Error>,
    to_open: ReaderToOpen,
}

impl Database {
    /// The writer first, since it moves the file to the current schema, and then the read-only connection.
    fn open(self) -> Result<(Store, OpenReader), Error> {
        let path = self.folders?.database();
        let store = Store::open(&path)?;
        let reader = self.to_open.open(&path)?;
        Ok((store, reader))
    }
}

/// Opens the database and watches GitHub. Without a database Tsugiki doesn't ask GitHub anything: `board`
/// shows why, and a database from a newer Tsugiki stays as it is (ADR 0010).
///
/// Nothing else would show a panic in here: the last status would stay up and look current, so this replaces
/// it with a problem. It relies on panics unwinding, so `panic = "abort"` would end the app instead (#57).
fn run(board: &mut StatusBoard, database: Database, access: GitHubAccess, refreshes: &Refreshes) {
    let started = catch_unwind(AssertUnwindSafe(|| match database.open() {
        Ok((store, reader)) => {
            watch(board, &store, access, refreshes);
            reader.close();
            store.close();
        }
        Err(problem) => board.set(Status::Problem { problem }),
    }));
    if started.is_err() {
        board.set(Status::Problem {
            problem: Error::new(ErrorKind::Internal, STOPPED_CHECKING),
        });
    }
}

/// Connects, then refreshes and runs the syncs the app asks for, until the app stops it. With no token
/// `board` shows why, and the database stays open until then, so the window can read what's stored.
fn watch(board: &mut StatusBoard, store: &Store, access: GitHubAccess, refreshes: &Refreshes) {
    let Some(github) = connect(board, store, access) else {
        refreshes.wait_for_stop(board, store);
        return;
    };
    let mut syncs = Syncs::default();
    let intervals = refreshes.intervals;
    let mut due = Due::starting_at(Instant::now(), intervals);
    loop {
        match refreshes.next_turn(&github, &mut syncs, due) {
            Turn::Stop => return,
            Turn::Refresh => {
                refresh(board, store, &github);
                due.status = Instant::now() + intervals.status;
            }
            Turn::RefreshRepository => {
                syncs.refresh_open(store);
                due.repository = Instant::now() + intervals.repository;
            }
            Turn::RefreshNow => {
                syncs.refresh_now();
                due.repository = Instant::now() + intervals.repository;
            }
            Turn::Opened(repository) => show_last_sync(board, store, repository),
            Turn::Seen(item, at) => mark_seen(board, store, &item, at),
            Turn::FetchIssue(item) => {
                if let Err(problem) = fetch_issue(board, store, &github, item) {
                    syncs.after_failure(&problem, due.status);
                    board.set(status_after(problem, store));
                }
            }
            // A Workbench lists nothing until Tsugiki knows who the user is, and no event tells the window
            // when it learns that. So a sync waits for it, and its first save then shows the rows.
            Turn::SyncStep if !matches!(store.last_login(), Ok(Some(_))) => {
                syncs.wait_until(due.status);
            }
            Turn::SyncStep => match syncs.step(&github, store) {
                Ok(Some(Progress::Changed(change))) => board.changed(&change),
                Ok(Some(Progress::Finished(last_sync))) => board.last_sync(&last_sync),
                Ok(None) => {}
                // The next refresh shows whether GitHub is back, and the sync carries on from there.
                Err(problem) => {
                    syncs.after_failure(&problem, due.status);
                    board.set(status_after(problem, store));
                }
            },
        }
        board.sync_activity(syncs.activity());
    }
}

/// Moves the Last seen of `item`, and tells the window, whose lists go by it.
fn mark_seen(board: &mut StatusBoard, store: &Store, item: &Item, at: OffsetDateTime) {
    match store.mark_seen(&item.repository, item.number, at) {
        Ok(()) => board.changed(&Change {
            repository: item.repository.clone(),
            items: vec![item.number],
        }),
        Err(problem) => board.set(Status::Problem { problem }),
    }
}

/// Reads what an Issue says from GitHub, keeps it, and tells the window. The view goes on showing what was
/// last read if this fails. An error returned here speaks for all of GitHub, so the status bar and the syncs
/// need to hear it. Any other trouble is this one Issue's, and goes to the log.
fn fetch_issue(
    board: &mut StatusBoard,
    store: &Store,
    github: &GitHub,
    item: Item,
) -> Result<(), Error> {
    let Item { repository, number } = item;
    let content = match github.issue_content(&repository, number) {
        Ok(content) => content,
        Err(problem) => {
            return match problem.kind {
                ErrorKind::Offline | ErrorKind::RateLimited { .. } | ErrorKind::Unauthorized => {
                    Err(problem)
                }
                ErrorKind::WriteRefused
                | ErrorKind::NotFound
                | ErrorKind::Conflict
                | ErrorKind::QueryRefused
                | ErrorKind::Internal => {
                    log::warn!("Tsugiki couldn't read Issue #{number}: {}", problem.message);
                    Ok(())
                }
            };
        }
    };
    match store.save_issue_content(&repository, number, content) {
        Ok(()) => board.changed(&Change {
            repository,
            items: vec![number],
        }),
        Err(problem) => board.set(Status::Problem { problem }),
    }
    Ok(())
}

/// Tells the window when `repository` last finished a sync, so the status bar can say.
fn show_last_sync(board: &mut StatusBoard, store: &Store, repository: RepositoryName) {
    match store.sync_state(&repository) {
        Ok(state) => board.last_sync(&LastSync {
            repository,
            at: state.map(|state| state.finished_at),
        }),
        Err(problem) => board.set(Status::Problem { problem }),
    }
}

/// Asks GitHub again, so `board` follows the network going and coming back.
fn refresh(board: &mut StatusBoard, store: &Store, github: &GitHub) {
    board.set(match github.viewer() {
        Ok(viewer) => match store.remember_login(&viewer.login) {
            Ok(()) => Status::Online {
                login: viewer.login,
            },
            Err(problem) => Status::Problem { problem },
        },
        Err(problem) => status_after(problem, store),
    });
}

/// The status bar's words after GitHub didn't answer as it should.
fn status_after(problem: Error, store: &Store) -> Status {
    match problem.kind {
        ErrorKind::Offline => match store.last_login() {
            Ok(Some(last_login)) => Status::OfflineAs { last_login },
            Ok(None) => Status::Offline,
            Err(problem) => Status::Problem { problem },
        },
        ErrorKind::Unauthorized
        | ErrorKind::WriteRefused
        | ErrorKind::RateLimited { .. }
        | ErrorKind::NotFound
        | ErrorKind::Conflict
        | ErrorKind::QueryRefused
        | ErrorKind::Internal => Status::Problem { problem },
    }
}

#[cfg(test)]
mod tests;
