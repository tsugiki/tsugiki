//! One run of Tsugiki: opens the database, then keeps the status following GitHub until the app ends.

use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use crate::error::{Error, ErrorKind};
use crate::facts::issue_content::IssueContent;
use crate::facts::pull_request_content::PullRequestContent;
use crate::facts::pull_requests::PrHead;
use crate::facts::{CommitSha, WorkFacts};
use crate::fetching::{Fetched, Fetching, Item, Marked, MirrorWant, SessionMessage};
use crate::folders::Folders;
use crate::github::{GitHub, GitHubAccess};
use crate::mirror::MirrorRequests;
use crate::patch_ids::PatchReads;
use crate::repository::RepositoryName;
use crate::repository_settings::{RepositoryCheck, RepositorySettings};
use crate::status::{Change, FailedRead, LastSync, ReadFailure, Status, StatusBoard};
use crate::store::{self, OpenReader, ReaderToOpen, Store, StoreReader};
use crate::sync::{Progress, Syncs};
use crate::token::read_token;
use crate::work::{SeenMark, checks_to_keep, mirror_work, reach};

// `run` catches a panic in its thread, to keep the window up and say that checking stopped. Tests unwind
// whatever the release profile says, so only this line can catch the setting (docs/build-costs.md).
#[cfg(panic = "abort")]
compile_error!("`session::run` catches panics, so Tsugiki can't build with `panic = \"abort\"`");

const STOPPED_CHECKING: &str = "Tsugiki stopped checking GitHub. Restart Tsugiki to check again.";

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
    /// The handle the app asks through, which also tells this session to stop.
    fetching: Fetching,
    finished: Receiver<()>,
    reader: StoreReader,
}

/// The news for the window that a read of what `item` says failed, and `why`.
fn failed(item: &Item, why: ReadFailure) -> FailedRead {
    FailedRead {
        repository: item.repository.clone(),
        number: item.number,
        why,
    }
}

/// What a session reaches beyond its thread: GitHub with `gh`'s token, and the Mirror worker, which it hands
/// the rules' wants and which hands it the patch-ids it reads.
pub struct Connections {
    pub github: GitHubAccess,
    pub mirrors: MirrorRequests,
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
        connections: Connections,
        intervals: Intervals,
    ) -> Self {
        let Connections {
            github: access,
            mirrors,
        } = connections;
        let (messages, incoming) = mpsc::channel();
        let fetching = Fetching::new(messages, mirrors);
        fetching.hand_patch_ids_to_session();
        let (finishing, finished) = mpsc::channel::<()>();
        let (to_open, reader) = store::reader();
        let database = Database { folders, to_open };
        let refreshes = Refreshes {
            intervals,
            incoming,
            fetching: fetching.clone(),
        };
        let _ = thread::spawn(move || {
            // `stop` learns the session is over when this goes, however the thread ends.
            let _finishing = finishing;
            run(&mut board, database, access, &refreshes);
        });
        Self {
            fetching,
            finished,
            reader,
        }
    }

    /// The way to ask GitHub and the Mirrors for data. Dropping the session stops it, whatever handles are
    /// left.
    pub fn fetching(&self) -> Fetching {
        self.fetching.clone()
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
        self.fetching.stop_session();
        #[expect(
            clippy::let_underscore_must_use,
            reason = "running out of patience is the one failure here, and the doc above says why it is safe"
        )]
        let _ = self.finished.recv_timeout(patience);
    }
}

impl Drop for Session {
    /// The session's thread holds a handle of its own, so its queue stays open and the thread needs telling.
    fn drop(&mut self) {
        self.fetching.stop_session();
    }
}

/// When to ask GitHub again, and the queue the session takes its messages from.
struct Refreshes {
    intervals: Intervals,
    incoming: Receiver<SessionMessage>,
    /// The handle the session hands the rules' wants to the Mirror worker through.
    fetching: Fetching,
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
    /// The user saved these settings of a Repository.
    SettingsChanged(RepositorySettings),
    /// The user marked an item seen or unseen.
    Mark(Marked),
    /// An Issue or a PR on screen needs its conversation read from GitHub.
    Fetch(Fetched),
    /// The Mirror worker read patch-ids, or found commits it can't read, for the store to keep.
    Patched(RepositoryName, PatchReads),
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
                SessionMessage::Stop => return,
                SessionMessage::Mark(marked) => mark(board, store, marked),
                SessionMessage::Patched(repository, patches) => {
                    keep_patches(board, store, &repository, patches);
                }
                SessionMessage::Fetch(fetched) => {
                    board.failed_read(&failed(fetched.item(), ReadFailure::NoToken));
                }
                SessionMessage::Check { .. }
                | SessionMessage::Sync { .. }
                | SessionMessage::Refresh
                | SessionMessage::SettingsChanged(_) => {}
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
                Ok(SessionMessage::Sync { repository }) => {
                    let opened = repository.name.clone();
                    syncs.add(repository);
                    return Turn::Opened(opened);
                }
                Ok(SessionMessage::Refresh) => return Turn::RefreshNow,
                Ok(SessionMessage::SettingsChanged(settings)) => {
                    return Turn::SettingsChanged(settings);
                }
                Ok(SessionMessage::Mark(marked)) => return Turn::Mark(marked),
                Ok(SessionMessage::Fetch(fetched)) => return Turn::Fetch(fetched),
                Ok(SessionMessage::Patched(repository, patches)) => {
                    return Turn::Patched(repository, patches);
                }
                Ok(SessionMessage::Check { name, reply }) => {
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
                Ok(SessionMessage::Stop) | Err(RecvTimeoutError::Disconnected) => {
                    return Turn::Stop;
                }
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
        // They said how the mirrors stood in the last run, and the Mirror worker says again for this one.
        store.forget_unchecked()?;
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
            Turn::SettingsChanged(settings) => {
                let _handed = settings_changed(&refreshes.fetching, store, &mut syncs, settings);
            }
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
                        let _handed = hand_over_heads(&refreshes.fetching, store, &syncs, &change);
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

/// Hands the Mirror worker what the rules want of the Repository a step just saved facts of: the heads of
/// the PRs in its Reach, the commit each of the user's Reviews there covered, and the commits whose patch-ids
/// the Stale review check needs and the store lacks. Then the heads the step saved, so a PR the user opened
/// before its head was known gets its head first. It keeps the last check of each PR whose sides are both
/// read first. The mirror fetches only the heads that moved, and the reviewed commits it doesn't keep yet. A
/// store that can't answer just saved, and the next step that saves says if it is in trouble, and hands the
/// heads. The heads go though the reviewed commits can't be read, and those wait for a step whose read works.
/// Says whether it handed anything over.
fn hand_over_heads(
    fetching: &Fetching,
    store: &Store,
    syncs: &Syncs,
    saved: &Change,
) -> Option<()> {
    let repository = &saved.repository;
    let settings = syncs.settings(repository)?;
    let facts = store.work_facts(repository).ok()?;
    let _reached = hand_over_wants(fetching, store, settings, &facts);
    let saved_heads = (facts.pull_requests.iter())
        .filter(|pr| saved.items.contains(&pr.number))
        .filter_map(|pr| {
            Some(PrHead {
                number: pr.number,
                commit: pr.head.clone()?,
                target_branch: pr.target_branch.clone(),
            })
        });
    fetching.heads_saved(repository, saved_heads.collect());
    Some(())
}

/// Hands the Mirror worker what the rules want of the Repository of `settings`, whose stored facts are
/// `facts`, as `hand_over_heads` says. Answers with its Reach.
fn hand_over_wants(
    fetching: &Fetching,
    store: &Store,
    settings: &RepositorySettings,
    facts: &WorkFacts,
) -> BTreeSet<u32> {
    let repository = &settings.name;
    let reviewed = store.reviewed_commits(repository).ok();
    // A head can move to a commit the store has read, which makes no new read to keep a check with. A store
    // that can't keep it says so at the next step that saves.
    let _kept = keep_last_checks(store, repository, facts);
    let reached = reach(facts, settings);
    let (heads, patches) = mirror_work(facts, settings, &reached);
    let want = MirrorWant {
        heads,
        reviewed,
        patches,
    };
    fetching.want(repository, want);
    reached
}

/// The user saved `settings`, which the syncs read from now on, if the session has opened their Repository
/// since launch. A Filter switched on puts PRs in Reach that no change on GitHub would have the Mirror fetch
/// the head of or a refresh read ahead (#819). So the Mirror worker hears the new Reach at once, and the
/// store owes a read of each PR newly in Reach whose Conversation it doesn't hold at the PR's head, which
/// the next refresh reads ahead while the PR is still in Reach. One switched off takes PRs out, and the
/// Mirror worker stops wanting their heads.
///
/// A store that can't answer leaves the heads to the next step that saves, and the Conversations to the PR
/// view, which reads one when it opens. Says whether it handed anything over.
fn settings_changed(
    fetching: &Fetching,
    store: &Store,
    syncs: &mut Syncs,
    settings: RepositorySettings,
) -> Option<()> {
    let repository = settings.name.clone();
    let before = syncs.replace_settings(settings)?;
    let facts = store.work_facts(&repository).ok()?;
    let now = syncs.settings(&repository)?;
    let reached = hand_over_wants(fetching, store, now, &facts);
    let reached_before = reach(&facts, &before);
    let newly = (facts.pull_requests.iter())
        .filter(|pr| reached.contains(&pr.number) && !reached_before.contains(&pr.number))
        .map(|pr| (pr.number, pr.head.clone()))
        .collect();
    store.owe_read_ahead_at(&repository, newly).ok()
}

/// Keeps what the Mirror worker handed over: the patch-ids it read, with the last check of each PR whose
/// sides are both read now, or the commits it can't read. Then tells the window of the PRs whose reviewed
/// commit or head they are, whose review state can change with them.
fn keep_patches(
    board: &mut StatusBoard,
    store: &Store,
    repository: &RepositoryName,
    reads: PatchReads,
) {
    let (commits, saved): (BTreeSet<CommitSha>, _) = match reads {
        PatchReads::Read(patches) => {
            let read = patches.iter().map(|read| read.of.commit.clone()).collect();
            (read, store.save_patch_ids(repository, patches))
        }
        PatchReads::Unchecked(unchecked) => {
            // A commit gone from the list counts as well as one new to it.
            let now = unchecked.iter().map(|commit| commit.of.commit.clone());
            let mut commits: BTreeSet<CommitSha> = now.collect();
            let before = store.work_facts(repository);
            let saved = before.and_then(|before| {
                commits.extend(before.reviews.unchecked.into_keys().map(|of| of.commit));
                store.save_unchecked(repository, unchecked)
            });
            (commits, saved)
        }
    };
    let facts = saved.and_then(|()| store.work_facts(repository));
    let kept = facts.and_then(|facts| {
        keep_last_checks(store, repository, &facts)?;
        Ok(facts)
    });
    match kept {
        Ok(facts) => {
            let reviewed = &facts.reviews.reviewed;
            let checked = facts.pull_requests.iter().filter(|pr| {
                let sides = pr.head.iter().chain(reviewed.get(&pr.number));
                sides.into_iter().any(|commit| commits.contains(commit))
            });
            board.changed(&Change {
                repository: repository.clone(),
                items: checked.map(|pr| pr.number).collect(),
            });
        }
        Err(problem) => board.set(Status::Problem { problem }),
    }
}

/// Keeps as its last check the two commits of each PR whose patch-ids `facts` hold of both, where the store
/// kept others, so their verdict stands while a newer check waits.
fn keep_last_checks(
    store: &Store,
    repository: &RepositoryName,
    facts: &WorkFacts,
) -> Result<(), Error> {
    let checks = checks_to_keep(facts);
    if checks.is_empty() {
        return Ok(());
    }
    store.save_last_checks(repository, checks)
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
            board.failed_read(&failed(&item, ReadFailure::WaitsOnGitHub));
            return Err(problem);
        }
        Err(problem) => {
            let why = if matches!(problem.kind, ErrorKind::NotFound) {
                ReadFailure::NotOnGitHub
            } else {
                ReadFailure::Other
            };
            board.failed_read(&failed(&item, why));
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
