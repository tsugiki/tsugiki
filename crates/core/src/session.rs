//! One run of Tsugiki: opens the database, then keeps the status following GitHub until the app ends.

use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use time::OffsetDateTime;

use crate::error::{Error, ErrorKind};
use crate::folders::Folders;
use crate::github::issue_content::IssueContent;
use crate::github::pull_request_content::PullRequestContent;
use crate::github::{CommitSha, GitHub, GitHubAccess};
use crate::patch_ids::CommitPatches;
use crate::repository::RepositoryName;
use crate::repository_settings::{RepositoryCheck, RepositorySettings};
use crate::status::{Change, FailedRead, LastSync, MirrorWork, ReadFailure, Status, StatusBoard};
use crate::store::{self, OpenReader, ReaderToOpen, Store, StoreReader};
use crate::sync::{Progress, Syncs};
use crate::token::read_token;
use crate::workbench::{SeenMark, Workbench};

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
    let token = match read_token(access.run_gh) {
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
    Mark(Marked),
    Fetch(Fetched),
    /// The per-file patch-ids the Mirror worker read, for the store to keep.
    Patched(RepositoryName, Vec<CommitPatches>),
}

/// A mark the user made on an item: seen or unseen.
#[derive(Debug, PartialEq, Eq)]
enum Marked {
    Seen {
        repository: RepositoryName,
        mark: SeenMark,
    },
    /// The flag starts at `at`.
    Unseen { item: Item, at: OffsetDateTime },
}

/// An Issue or a PR on screen, whose conversation the session reads from GitHub.
#[derive(Debug, PartialEq, Eq)]
enum Fetched {
    Issue(Item),
    PullRequest(Item),
}

impl Fetched {
    fn item(&self) -> &Item {
        let (Self::Issue(item) | Self::PullRequest(item)) = self;
        item
    }
}

impl Item {
    /// The news for the window that a read of what this item says failed, and `why`.
    fn failed(&self, why: ReadFailure) -> FailedRead {
        FailedRead {
            repository: self.repository.clone(),
            number: self.number,
            why,
        }
    }
}

/// Keeps the per-file patch-ids read of commits in a Repository, as `SessionRequests::patch_ids_keeper`
/// gives it.
pub type KeepPatchIds = Box<dyn FnMut(&RepositoryName, Vec<CommitPatches>) + Send>;

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

    /// Asks the session to read what an Issue on screen says now, and returns at once. Only Mark Seen moves
    /// Last seen, so opening the Issue or reading it again leaves it alone.
    pub fn fetch_issue(&self, repository: RepositoryName, number: u32) {
        let item = Item { repository, number };
        drop(self.messages.send(Message::Fetch(Fetched::Issue(item))));
    }

    /// Asks the session to read a PR's Conversation on screen now, and returns at once. Like `fetch_issue`,
    /// this leaves Last seen alone.
    pub fn fetch_pull_request(&self, repository: RepositoryName, number: u32) {
        let item = Item { repository, number };
        drop(
            self.messages
                .send(Message::Fetch(Fetched::PullRequest(item))),
        );
    }

    /// Mark Seen on what `mark` names, as the window drew it, and returns at once. A session that can't ask
    /// GitHub still records it.
    pub fn mark_seen(&self, repository: RepositoryName, mark: SeenMark) {
        drop(
            self.messages
                .send(Message::Mark(Marked::Seen { repository, mark })),
        );
    }

    /// Mark Unseen on item `number`, from now, and returns at once.
    pub fn mark_unseen(&self, repository: RepositoryName, number: u32) {
        let item = Item { repository, number };
        let at = OffsetDateTime::now_utc();
        drop(
            self.messages
                .send(Message::Mark(Marked::Unseen { item, at })),
        );
    }

    /// What keeps the per-file patch-ids the Mirror worker read of commits in a Repository, for
    /// `MirrorRequests::hand_patches_to`. Each call returns at once, and the window then hears that the PRs
    /// whose reviewed commit or head they are changed. It needs only the database, so a session that can't
    /// reach GitHub keeps them too.
    pub fn patch_ids_keeper(self) -> KeepPatchIds {
        Box::new(move |repository, patches| {
            let patched = Message::Patched(repository.clone(), patches);
            drop(self.messages.send(patched));
        })
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
    /// The user marked an item seen or unseen.
    Mark(Marked),
    /// An Issue or a PR on screen needs its conversation read from GitHub.
    Fetch(Fetched),
    /// The Mirror worker read patch-ids for the store to keep.
    Patched(RepositoryName, Vec<CommitPatches>),
    SyncStep,
    Stop,
}

impl Refreshes {
    /// Waits for a stop message, for a session with nothing to ask GitHub with. A check dropped here tells
    /// the one asking that the session isn't connected. Mark Seen and Mark Unseen need only the database, so
    /// they still work.
    fn wait_for_stop(&self, board: &mut StatusBoard, store: &Store) {
        while let Ok(message) = self.incoming.recv() {
            match message {
                Message::Stop => return,
                Message::Mark(marked) => mark(board, store, marked),
                Message::Patched(repository, patches) => {
                    keep_patches(board, store, &repository, patches);
                }
                Message::Fetch(fetched) => {
                    board.failed_read(&fetched.item().failed(ReadFailure::NoToken));
                }
                Message::Check { .. } | Message::Sync { .. } | Message::Refresh => {}
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
                Ok(Message::Mark(marked)) => return Turn::Mark(marked),
                Ok(Message::Fetch(fetched)) => return Turn::Fetch(fetched),
                Ok(Message::Patched(repository, patches)) => {
                    return Turn::Patched(repository, patches);
                }
                Ok(Message::Check { name, reply }) => {
                    let today = github.clock().now().date();
                    #[expect(
                        clippy::let_underscore_must_use,
                        reason = "this fails only if the caller has gone, and it waits for this answer"
                    )]
                    let _ = reply.send(
                        github
                            .check_repository(&name, today)
                            .map(RepositoryCheck::new),
                    );
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
            // Only asking who is signed in sets the status back online, so offline a click asks that first.
            // While GitHub stays away, the syncs keep waiting for the next status check.
            Turn::RefreshNow => {
                if board.is_offline() {
                    refresh(board, store, &github);
                    if !board.is_offline() {
                        due.status = Instant::now() + intervals.status;
                    }
                }
                if board.is_offline() {
                    syncs.put_open_first();
                } else {
                    syncs.refresh_now();
                }
                due.repository = Instant::now() + intervals.repository;
            }
            Turn::Opened(repository) => show_last_sync(board, store, repository),
            Turn::Mark(marked) => mark(board, store, marked),
            Turn::Patched(repository, patches) => keep_patches(board, store, &repository, patches),
            Turn::Fetch(fetched) => {
                if let Fetched::Issue(item) = &fetched {
                    syncs.open_issue(&item.repository, item.number);
                }
                if let Err(problem) = fetch_conversation(board, store, &github, fetched) {
                    syncs.after_fetch_failure(&problem, due.status, github.clock());
                    board.set(status_after(problem, store));
                }
            }
            // A Workbench lists nothing until Tsugiki knows who the user is, and no event tells the window
            // when it learns that. So a sync waits for it, and its first save then shows the rows.
            Turn::SyncStep if !matches!(store.last_login(), Ok(Some(_))) => {
                syncs.wait_until(due.status);
            }
            Turn::SyncStep => match syncs.step(&github, store) {
                Ok(Some(Progress::Changed(change))) => {
                    if !change.items.is_empty() {
                        hand_over_heads(board, store, &syncs, &change.repository);
                    }
                    board.changed(&change);
                }
                Ok(Some(Progress::Finished(last_sync))) => board.last_sync(&last_sync),
                Ok(None) => {}
                // The next refresh shows whether GitHub is back, and the sync carries on from there.
                Err(failure) => {
                    syncs.after_failure(&failure, due.status, github.clock());
                    board.set(status_after(failure.problem, store));
                }
            },
        }
        board.sync_activity(syncs.activity());
    }
}

/// Hands the mirrors the heads of the PRs the user's work reaches in `repository`, after a step saved its
/// facts, the commit each of the user's Reviews there covered, and the commits whose patch-ids the Stale
/// review check needs and the store lacks. The mirror fetches only the heads that moved, and the reviewed
/// commits it doesn't keep yet. A store that can't answer just saved, and the next
/// step that saves says if it is in trouble, and hands the heads. The heads go though the reviewed commits
/// can't be read, and those wait for a step whose read works.
fn hand_over_heads(
    board: &mut StatusBoard,
    store: &Store,
    syncs: &Syncs,
    repository: &RepositoryName,
) {
    if let (Some(settings), Ok(facts)) = (syncs.settings(repository), store.work_facts(repository))
    {
        let reviewed = store.reviewed_commits(repository).unwrap_or_default();
        let patches = Workbench::patches_to_read(&facts);
        let heads = Workbench::reached_heads(facts, settings);
        let work = MirrorWork {
            heads,
            reviewed,
            patches,
        };
        board.reached_heads(repository, work);
    }
}

/// Keeps the patch-ids the Mirror worker read, and tells the window of the PRs whose reviewed commit or head
/// they are, whose review state can change with them.
fn keep_patches(
    board: &mut StatusBoard,
    store: &Store,
    repository: &RepositoryName,
    patches: Vec<CommitPatches>,
) {
    let read: BTreeSet<CommitSha> = patches.iter().map(|read| read.of.commit.clone()).collect();
    let facts = store
        .save_patch_ids(repository, patches)
        .and_then(|()| store.work_facts(repository));
    match facts {
        Ok(facts) => {
            let reviewed = &facts.reviews.reviewed;
            let checked = facts.pull_requests.iter().filter(|pr| {
                let sides = pr.head.iter().chain(reviewed.get(&pr.number));
                sides.into_iter().any(|commit| read.contains(commit))
            });
            board.changed(&Change {
                repository: repository.clone(),
                items: checked.map(|pr| pr.number).collect(),
            });
        }
        Err(problem) => board.set(Status::Problem { problem }),
    }
}

/// Keeps what the user marked, and tells the window, whose lists go by it.
fn mark(board: &mut StatusBoard, store: &Store, marked: Marked) {
    let (saved, change) = match marked {
        Marked::Seen { repository, mark } => {
            let SeenMark { numbers, up_to } = mark;
            let change = Change {
                repository,
                items: numbers.clone(),
            };
            (store.mark_seen(&change.repository, numbers, up_to), change)
        }
        Marked::Unseen {
            item: Item { repository, number },
            at,
        } => {
            let saved = store.mark_unseen(&repository, number, at);
            let items = vec![number];
            (saved, Change { repository, items })
        }
    };
    match saved {
        Ok(()) => board.changed(&change),
        Err(problem) => board.set(Status::Problem { problem }),
    }
}

/// An Issue's or a PR's content, as GitHub answered.
enum Content {
    Issue(IssueContent),
    PullRequest(PullRequestContent),
}

/// Reads what an Issue or a PR says from GitHub, keeps it, and tells the window. GitHub numbers both from
/// one sequence, so they share where they are kept. If this fails, the view goes on showing what was last
/// read, and the window hears why. An error returned here speaks for all of GitHub, so the status bar and
/// the syncs need to hear it. Any other trouble is this one item's, and goes to the log.
fn fetch_conversation(
    board: &mut StatusBoard,
    store: &Store,
    github: &GitHub,
    fetched: Fetched,
) -> Result<(), Error> {
    let (item, noun, read) = match fetched {
        Fetched::Issue(item) => {
            let read = github.issue_content(&item.repository, item.number);
            (item, "Issue", read.map(Content::Issue))
        }
        Fetched::PullRequest(item) => {
            let read = github.pull_request_content(&item.repository, item.number);
            (
                item,
                "PR",
                read.map(|read| Content::PullRequest(read.content)),
            )
        }
    };
    let content = match read {
        Ok(content) => content,
        Err(problem) if problem.kind.speaks_for_all_of_github() => {
            board.failed_read(&item.failed(ReadFailure::WaitsOnGitHub));
            return Err(problem);
        }
        Err(problem) => {
            let why = if matches!(problem.kind, ErrorKind::NotFound) {
                ReadFailure::NotOnGitHub
            } else {
                ReadFailure::Other
            };
            board.failed_read(&item.failed(why));
            log::warn!(
                "Tsugiki couldn't read {noun} #{} in {}: {}",
                item.number,
                item.repository,
                problem.message
            );
            return Ok(());
        }
    };
    let Item { repository, number } = item;
    let saved = match content {
        Content::Issue(content) => store.save_issue_content(&repository, number, content),
        Content::PullRequest(content) => {
            store.save_pull_request_content(&repository, number, content)
        }
    };
    match saved {
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
        ErrorKind::CertificateDates
        | ErrorKind::Unauthorized
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
