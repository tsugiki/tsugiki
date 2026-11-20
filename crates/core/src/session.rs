//! One run of Tsugiki: opens the database, then keeps the status following GitHub until the app ends.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use time::OffsetDateTime;

use crate::error::{Error, ErrorKind};
use crate::facts::activity::ItemKey;
use crate::facts::issue_content::IssueContent;
use crate::facts::pull_request_content::PullRequestContent;
use crate::facts::pull_requests::PrHead;
use crate::facts::{CommitSha, WorkFacts};
use crate::fetching::{
    Fetched, Fetching, Headless, Item, Marked, MirrorWant, NoHead, SessionMessage,
};
use crate::folders::Folders;
use crate::github::{GitHub, GitHubAccess};
use crate::latest::LatestReader;
use crate::login::Login;
use crate::mirror::MirrorRequests;
use crate::patch_ids::PatchReads;
use crate::repository::RepositoryName;
use crate::repository_settings::{RepositoryCheck, RepositorySettings, SavedBy, SettingsSave};
use crate::settings::SettingsView;
use crate::status::{Change, FailedRead, ItemUnread, LastSync, ReadFailure, Status, StatusBoard};
use crate::store::{
    self, Archiving, AuthorMuting, ConversationSaved, Flagging, HeadKept, Muting, OpenReader,
    ReaderToOpen, Seeing, Store, StoreReader,
};
use crate::sync::{Progress, Syncs};
use crate::token::read_token;
use crate::work::{MirrorWork, ReachFrom, SeenMark, checks_to_keep, findings_to_keep, moved};
use crate::workbench::{Authored, authored_by};
use on_screen::OnScreen;

mod on_screen;

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

/// What a session reaches beyond its thread: GitHub with `gh`'s token, the Mirror worker, which it hands
/// the rules' wants and which hands it the patch-ids it reads, and the settings owner's view, which it reads
/// each Repository's settings from.
pub struct Connections {
    pub github: GitHubAccess,
    pub mirrors: MirrorRequests,
    pub settings: LatestReader<SettingsView>,
}

/// How often a session asks GitHub again.
#[derive(Debug, Clone, Copy)]
pub struct Intervals {
    /// Asking who is signed in, so the status follows the network going and coming back.
    pub status: Duration,
    /// Refreshing the open Repository (ADR 0015).
    pub repository: Duration,
    /// Reading an Issue or a PR the user can see again, after its last read.
    pub on_screen: Duration,
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
            settings,
        } = connections;
        let (messages, incoming) = mpsc::channel();
        let fetching = Fetching::new(messages, mirrors, Headless::default());
        fetching.hand_patch_ids_to_session();
        let (finishing, finished) = mpsc::channel::<()>();
        let (to_open, reader) = store::reader();
        let database = Database { folders, to_open };
        let refreshes = Refreshes::new(intervals, incoming, fetching.clone(), settings);
        #[expect(
            unused_results,
            reason = "`stop` waits for `finishing` to go, with a patience a join can't take"
        )]
        thread::spawn(move || {
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
    pub(crate) fn fetching(&self) -> Fetching {
        self.fetching.clone()
    }

    /// The read-only connection, which doesn't wait for anything this session is doing (ADR 0018).
    pub(crate) fn reader(&self) -> StoreReader {
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

/// When to ask GitHub again, the queue the session takes its messages from, and the settings owner's view,
/// which each Repository's settings come from.
struct Refreshes {
    intervals: Intervals,
    incoming: Receiver<SessionMessage>,
    /// The messages taken off `incoming` while merging demands, which go first, in the order they came.
    backlog: RefCell<VecDeque<SessionMessage>>,
    /// The handle the session hands the rules' wants to the Mirror worker through.
    fetching: Fetching,
    /// The PRs whose Conversation named no head or failed to read, which `fetching` shares.
    headless: Headless,
    /// The Issues and PRs on screen, each read again after its last read.
    on_screen: RefCell<OnScreen>,
    /// The settings owner's view. The owner publishes each save there and hands it to the session in one
    /// step, so a read finds settings at least as new as every save the session has heard of.
    settings: LatestReader<SettingsView>,
    /// How many saves of a Repository's settings the session has followed, to tell from the view's count
    /// those in its queue.
    saves_taken: Cell<usize>,
}

/// Where the session tells the window of a mark or a hand edit to the settings, with the syncs that say
/// which Repositories' lists the window shows: those opened since launch. A session with no syncs tells of
/// a hand edit to any.
struct Telling<'a> {
    board: &'a mut StatusBoard,
    syncs: Option<&'a Syncs>,
}

impl Telling<'_> {
    fn shows(&self, repository: &RepositoryName) -> bool {
        (self.syncs).is_none_or(|syncs| syncs.was_opened(repository))
    }
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
    /// The settings owner saved these settings of a Repository.
    SettingsChanged(SettingsSave),
    /// The user marked an item seen or unseen.
    Mark(Marked),
    /// An Issue or a PR on screen needs its conversation read from GitHub.
    Fetch(Fetched),
    /// The Mirror worker read patch-ids, or found commits it can't read, for the store to keep.
    Patched(RepositoryName, PatchReads),
    /// Time to see whether the hour's points have reset, so the window stops showing the old hour's count.
    CheckHour,
    SyncStep,
    Stop,
}

impl Refreshes {
    fn new(
        intervals: Intervals,
        incoming: Receiver<SessionMessage>,
        fetching: Fetching,
        settings: LatestReader<SettingsView>,
    ) -> Self {
        Self {
            intervals,
            incoming,
            backlog: RefCell::new(VecDeque::new()),
            headless: fetching.headless(),
            fetching,
            on_screen: RefCell::new(OnScreen::new(intervals.on_screen)),
            settings,
            saves_taken: Cell::new(0),
        }
    }

    /// The next message, from the backlog first, waiting as long as it takes: none once every sender has
    /// gone.
    fn wait_for_message(&self) -> Option<SessionMessage> {
        let first = self.backlog.borrow_mut().pop_front();
        first.or_else(|| self.incoming.recv().ok())
    }

    /// Has the Mirror worker follow the move of Reach `save` makes, counts it as followed, and tells the
    /// window of a hand edit to a Repository whose lists it shows.
    fn follow_settings(&self, telling: &mut Telling<'_>, store: &Store, save: &SettingsSave) {
        self.saves_taken.set(self.saves_taken.get() + 1);
        follow_save(&self.fetching, store, save);
        let repository = &save.saved().name;
        if telling.shows(repository) {
            announce_settings(telling.board, repository, save.by());
        }
    }

    /// Follows each save among the first `told` the owner handed over that the session hasn't, taking it
    /// from the queue now. The owner counts a save in the view in one step with handing it over, so each
    /// is in the queue. The messages it passes keep their order, in the backlog.
    ///
    /// It tells the window of each hand edit it follows, in a Repository opened since launch or not: a Sync
    /// it passes may open the Repository, whose lists the window read before the edit. A save missing from
    /// the queue is logged once, and the count goes on from `told`, so a later mark takes no save that only
    /// a later view counts.
    fn follow_saves_told(&self, board: &mut StatusBoard, store: &Store, told: usize) {
        let owed = told.saturating_sub(self.saves_taken.get());
        if owed == 0 {
            return;
        }
        let saves = {
            let mut backlog = self.backlog.borrow_mut();
            backlog.extend(self.incoming.try_iter());
            let mut saves = Vec::new();
            backlog.retain(|message| {
                let SessionMessage::SettingsChanged(save) = message else {
                    return true;
                };
                let owed_still = saves.len() < owed;
                if owed_still {
                    saves.push(save.clone());
                }
                !owed_still
            });
            saves
        };
        let mut every = Telling { board, syncs: None };
        for save in &saves {
            self.follow_settings(&mut every, store, save);
        }
        // `log::error!` evaluates a call among its arguments only while a logger listens.
        let found = saves.len();
        if found < owed {
            log::error!(
                "The settings owner counts {owed} saves the session hasn't followed, and the session's queue \
                 holds {found}"
            );
            self.saves_taken.set(told);
        }
    }

    /// The next message, from the backlog first, waiting up to `wait` for one.
    fn next_message(&self, wait: Duration) -> Result<SessionMessage, RecvTimeoutError> {
        let first = self.backlog.borrow_mut().pop_front();
        first.map_or_else(|| self.incoming.recv_timeout(wait), Ok)
    }

    /// The read `fetched` ended, so each demand for the same read that came while it waited or ran shares
    /// its outcome and leaves the queue. A demand that comes after this starts a read of its own.
    fn merge(&self, fetched: &Fetched) {
        let mut backlog = self.backlog.borrow_mut();
        backlog.extend(self.incoming.try_iter());
        backlog.retain(
            |message| !matches!(message, SessionMessage::Fetch(other) if other.is(fetched)),
        );
    }

    /// Tells the window that the read of `fetched` failed for `why`. For a PR, it first tells the Files tab,
    /// which asks again once the window hears and finds why, and ends the Mirror worker's await of the head,
    /// which no read answers until the next.
    fn read_failed(&self, board: &mut StatusBoard, fetched: &Fetched, why: ItemUnread) {
        let item = fetched.item();
        if let Fetched::PullRequest(_) = fetched {
            self.headless.note(item, NoHead::Unread(why));
            (self.fetching).no_head(item.repository.clone(), item.number);
        }
        board.failed_read(&failed(item, why.into()));
    }

    /// Waits for a stop message, for a session with nothing to ask GitHub with. A check dropped here tells
    /// the one asking that the session isn't connected. The marks need only the database, so they still
    /// work, and so does telling the window of a hand edit to the settings: with no syncs to say which
    /// Repositories were opened, of each one. Reach follows a change of the settings, and a Mute or an
    /// Unmute, as it does with a token, which needs only the database and the Mirror (#946).
    fn wait_for_stop(&self, board: &mut StatusBoard, store: &Store) {
        let mut telling = Telling { board, syncs: None };
        while let Some(message) = self.wait_for_message() {
            match message {
                SessionMessage::Stop => return,
                SessionMessage::Mark(marked) => self.mark(&mut telling, store, marked),
                SessionMessage::SettingsChanged(save) => {
                    self.follow_settings(&mut telling, store, &save);
                }
                SessionMessage::Patched(repository, patches) => {
                    keep_patches(telling.board, store, &repository, patches);
                }
                SessionMessage::Fetch(fetched) => {
                    self.read_failed(telling.board, &fetched, ItemUnread::NoToken);
                }
                SessionMessage::Check { .. }
                | SessionMessage::Sync { .. }
                | SessionMessage::Refresh
                | SessionMessage::Screen(_) => {}
            }
        }
    }

    /// Keeps what the user marked, has the Mirror worker follow the move of Reach the mark makes, under the
    /// Repository's settings now, if the store kept it, and tells the window, whose lists go by the mark.
    ///
    /// A mark that moves Reach first follows each save counted in the view it reads the settings from, as
    /// those settings hold them. The saves' moves of Reach then end where the mark's starts, and the two
    /// chain. Without, a save that comes after the mark moves Reach on the facts after it, and a PR that only
    /// the pair puts in Reach is owed by neither.
    fn mark(&self, telling: &mut Telling<'_>, store: &Store, marked: Marked) {
        let repository = marked.repository().clone();
        let (told, settings) =
            (self.settings).read(|view| (view.saves_told, view.repository(repository)));
        if marked.moves_reach() {
            self.follow_saves_told(telling.board, store, told);
        }
        let moved = ReachMove::of(&marked, store, &settings);
        let (saved, change) = keep_mark(store, &settings, marked);
        if saved.is_ok()
            && let Some(moved) = moved
        {
            moved.follow(&self.fetching, store);
        }
        tell_marked(telling.board, saved, &change);
    }

    /// Waits for the next refresh or the next step of a sync, answering what's asked meanwhile, so a step
    /// doesn't keep anyone waiting for longer than one request. A dropped `Session` stops it too.
    fn next_turn(&self, github: &GitHub, syncs: &mut Syncs, due: Due) -> Turn {
        let next_refresh = due.status.min(due.repository);
        loop {
            let hour_check = hour_check(github);
            let reread = self.on_screen.borrow().wake();
            let wake = [syncs.ready_at(), hour_check, reread]
                .into_iter()
                .flatten()
                .fold(next_refresh, Ord::min);
            match self.next_message(wake.saturating_duration_since(Instant::now())) {
                Ok(SessionMessage::Sync { repository }) => {
                    syncs.add(repository.clone());
                    return Turn::Opened(repository);
                }
                Ok(SessionMessage::Refresh) => return Turn::RefreshNow,
                Ok(SessionMessage::SettingsChanged(save)) => return Turn::SettingsChanged(save),
                Ok(SessionMessage::Mark(marked)) => return Turn::Mark(marked),
                Ok(SessionMessage::Fetch(fetched)) => return Turn::Fetch(fetched),
                Ok(SessionMessage::Screen(moved)) => {
                    self.on_screen.borrow_mut().moved(moved, Instant::now());
                }
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
                Err(RecvTimeoutError::Timeout) => return self.timed_turn(due, hour_check),
            }
        }
    }

    /// The turn whose time came: asking who is signed in first, as only that brings the session back online,
    /// then reading again an item on screen whose minute came round, refreshing the open Repository,
    /// checking the hour's points, and else the next sync step.
    fn timed_turn(&self, due: Due, hour_check: Option<Instant>) -> Turn {
        let now = Instant::now();
        if now >= due.status {
            return Turn::Refresh;
        }
        let reread = self.on_screen.borrow().due(now);
        if let Some(read) = reread {
            // Whether the PR has a head waits for this read, as it does for a demand's.
            if let Fetched::PullRequest(item) = &read {
                self.headless.forget(item);
            }
            return Turn::Fetch(read);
        }
        if now >= due.repository {
            return Turn::RefreshRepository;
        }
        if hour_check.is_some_and(|at| now >= at) {
            return Turn::CheckHour;
        }
        Turn::SyncStep
    }
}

/// The longest the session waits before it reads GitHub's clock again while the window shows the hour's
/// points. A Mac's `Instant` stops while it sleeps and the wall clock doesn't, so a wait for the reset alone
/// could leave the old hour's count up long after it.
const HOUR_CHECK: Duration = Duration::from_secs(60);

/// The instant to see whether the hour's points have reset by GitHub's clock, while the window shows them:
/// at the reset, or [`HOUR_CHECK`] from now if that's sooner. The hour's count then ends, and the next
/// hour's waits for GitHub's next answer.
fn hour_check(github: &GitHub) -> Option<Instant> {
    let left = github.hour_left()?;
    Some(Instant::now() + left.min(HOUR_CHECK))
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
    board.hour_points(github.hour_points());
    let mut syncs = Syncs::new(refreshes.settings.clone());
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
            Turn::SettingsChanged(save) => {
                let mut telling = Telling {
                    board,
                    syncs: Some(&syncs),
                };
                refreshes.follow_settings(&mut telling, store, &save);
            }
            Turn::Mark(marked) => {
                let mut telling = Telling {
                    board,
                    syncs: Some(&syncs),
                };
                refreshes.mark(&mut telling, store, marked);
            }
            Turn::Patched(repository, patches) => keep_patches(board, store, &repository, patches),
            // The board hears of it below, as after each turn.
            Turn::CheckHour => {}
            Turn::Fetch(fetched) => {
                if let Fetched::Issue(item) = &fetched {
                    syncs.open_issue(&item.repository, item.number);
                }
                let read = Reading {
                    github: &github,
                    queue: refreshes,
                    syncs: &syncs,
                };
                if let Err(problem) = read_conversation(board, store, &read, &fetched) {
                    syncs.after_fetch_failure(&problem, due.status, github.clock());
                    board.set(status_after(problem, store));
                }
                // A failed read waits its minute too, or the session would ask GitHub again at once.
                (refreshes.on_screen.borrow_mut()).read(fetched.item(), Instant::now());
            }
            // A Workbench lists nothing until Tsugiki knows who the user is, and no event tells the window
            // when it learns that. So a sync waits for it, and its first save then shows the rows.
            Turn::SyncStep if !matches!(store.last_login(), Ok(Some(_))) => {
                syncs.wait_until(due.status);
            }
            Turn::SyncStep => match syncs.step(&github, store) {
                Ok(Some(Progress::Changed(change))) => {
                    if !change.items.is_empty()
                        && let Some(synced) = synced_facts(store, &syncs, &change.repository)
                    {
                        hand_over_heads(&refreshes.fetching, store, &synced, &change);
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
        // Whichever turn asked GitHub, a sync, a read of what's on screen or the status, counts.
        board.hour_points(github.hour_points());
    }
}

/// Hands the Mirror worker what the rules want of the `synced` Repository a step just saved facts of: the
/// heads of the PRs in its Reach, the commit each of the user's Reviews there covered, and the commits whose
/// patch-ids the Stale review check needs and the store lacks. Then the heads the step saved, so a PR the
/// user opened before its head was known gets its head first. It keeps the last check of each PR whose sides
/// are both read first. The mirror fetches only the heads that moved, and the reviewed commits it doesn't
/// keep yet. A store that can't answer just saved, and the next step that saves says if it is in trouble,
/// and hands the heads. The heads go though the reviewed commits can't be read, and those wait for a step
/// whose read works.
fn hand_over_heads(fetching: &Fetching, store: &Store, synced: &Synced<'_>, saved: &Change) {
    hand_over_rules_wants(fetching, store, synced);
    let saved_heads = (synced.facts.pull_requests.iter())
        .filter(|pr| saved.items.contains(&pr.number))
        .filter_map(|pr| {
            Some(PrHead {
                number: pr.number,
                commit: pr.head.clone()?,
                target_branch: pr.target_branch.clone(),
            })
        });
    fetching.heads_saved(synced.repository, saved_heads.collect());
}

/// A Repository that syncs, with its stored facts and its settings in the settings owner's view.
struct Synced<'a> {
    repository: &'a RepositoryName,
    facts: WorkFacts,
    settings: RepositorySettings,
}

/// `repository` with its facts and its settings in the settings owner's view, if it was opened since launch
/// and the store can read its facts. Only those Repositories sync, so only theirs hand the Mirror worker a
/// new set of wants.
fn synced_facts<'a>(
    store: &Store,
    syncs: &Syncs,
    repository: &'a RepositoryName,
) -> Option<Synced<'a>> {
    let settings = syncs.opened_settings(repository)?;
    let facts = store.work_facts(repository).ok()?;
    Some(Synced {
        repository,
        facts,
        settings,
    })
}

/// Hands the Mirror worker what the rules want of the `synced` Repository, from its stored facts.
fn hand_over_rules_wants(fetching: &Fetching, store: &Store, synced: &Synced<'_>) {
    let stated = Stated {
        repository: synced.repository,
        facts: &synced.facts,
        work: MirrorWork::of(&synced.facts, &synced.settings),
    };
    hand_over_wants(fetching, store, stated);
}

/// The Mirror's work in a Repository, with the stored facts the rules worked it out from.
struct Stated<'a> {
    repository: &'a RepositoryName,
    facts: &'a WorkFacts,
    work: MirrorWork,
}

/// Hands the Mirror worker what the rules want of it, as `hand_over_heads` says.
fn hand_over_wants(fetching: &Fetching, store: &Store, stated: Stated<'_>) {
    let Stated {
        repository,
        facts,
        work,
    } = stated;
    let reviewed = store.reviewed_commits(repository).ok();
    // A head can move to a commit the store has read, which makes no new read to keep a check with. A store
    // that can't keep it says so in the log, and in the status bar at the next step that saves.
    if let Err(problem) = keep_checks(store, repository, facts, OffsetDateTime::now_utc()) {
        log::warn!(
            "Tsugiki couldn't keep the Stale review checks in {repository}: {}",
            problem.message
        );
    }
    let want = MirrorWant {
        heads: work.heads,
        reviewed,
        patches: work.patches,
    };
    fetching.want(repository, want);
}

/// The settings owner saved `repository`'s settings otherwise than they were, as `by` did. New Conventions
/// regroup the Repository's PRs with no change to the store (ADR 0011), and a Filter's new search or switch
/// changes what the Workbench and the Sidebar list. So a hand edit reaches the window as a change to the
/// Repository that names no item, and its views read their items again (#822). The window asks again by
/// itself after a save of its own.
fn announce_settings(board: &mut StatusBoard, repository: &RepositoryName, by: SavedBy) {
    if by == SavedBy::HandEdit {
        board.changed(&Change {
            repository: repository.clone(),
            items: Vec::new(),
        });
    }
}

/// Reach follows the move `save` made from the settings it replaced, whether or not the session has opened
/// the Repository since launch: a hand edit can switch a Filter on in any Repository, and once the user
/// opens it, its refreshes read ahead only what the store owes (#901). A Repository the save added has no
/// move to follow.
fn follow_save(fetching: &Fetching, store: &Store, save: &SettingsSave) {
    if let Some(replaced) = save.replaced() {
        follow_reach(fetching, store, save.saved(), replaced);
    }
}

/// A Repository's settings say `now` in place of `before`. A Filter switched on puts PRs in Reach that no
/// change on GitHub would have the Mirror fetch the head of or a refresh read ahead (#819). So the Mirror
/// worker hears the new Reach at once, and the store owes a read of each Conversation the rules say the
/// switch owes. A refresh that finishes reads those still in Reach ahead. One switched off takes PRs out, and
/// the Mirror worker stops wanting their heads.
///
/// A store that can't read the facts leaves the heads to the next step that saves. The reads the switch owes
/// are lost, as no later step works them out, and the log says so. The PR view reads a lost Conversation
/// when it opens.
fn follow_reach(
    fetching: &Fetching,
    store: &Store,
    now: &RepositorySettings,
    before: &RepositorySettings,
) {
    let Some(facts) = facts_for_move(store, &now.name) else {
        return;
    };
    let from = |settings| ReachFrom {
        facts: &facts,
        settings,
    };
    follow_move(fetching, store, from(before), from(now));
}

/// The stored facts of `repository`, which a move of Reach goes by. A store that can't read them says so in
/// the log.
fn facts_for_move(store: &Store, repository: &RepositoryName) -> Option<WorkFacts> {
    (store.work_facts(repository))
        .inspect_err(|problem| {
            log::warn!(
                "Tsugiki couldn't read what moved Reach in {repository}: {}",
                problem.message
            );
        })
        .ok()
}

/// Hands the Mirror worker its work after Reach moved from `before` to `now`, unless the move left it as it
/// was, and has the store owe the reads ahead the move owes, as `moved` works them out. A store that can't
/// say which heads its Conversations were read at owes a read of each PR the move puts in Reach, which costs
/// a request apiece and loses nothing. One that can't keep what it owes loses them, and the log says so.
fn follow_move(fetching: &Fetching, store: &Store, before: ReachFrom<'_>, now: ReachFrom<'_>) {
    let repository = &now.settings.name;
    let read_at = store.conversation_heads(repository).unwrap_or_default();
    let (work, owed) = moved(before, now, &read_at);
    if let Some(work) = work {
        let stated = Stated {
            repository,
            facts: now.facts,
            work,
        };
        hand_over_wants(fetching, store, stated);
    }
    if let Err(problem) = store.owe_read_ahead(repository, owed) {
        log::warn!(
            "Tsugiki couldn't keep the reads ahead owed in {repository}: {}",
            problem.message
        );
    }
}

/// A Mute or an Unmute, which moves Reach (#918), with the Repository's settings and the stored facts from
/// before the store keeps it. It moves Reach with or without a token (#946).
///
/// Mute takes the item's tree out of Reach, apart from what an ask or another item keeps there, so the
/// Mirror worker stops wanting their heads. Unmute puts the tree back, which no change on GitHub would have
/// the Mirror fetch the heads of or a refresh read ahead. So the Mirror worker hears the new Reach at once,
/// and the store owes the reads the rules say Unmute owes, as with a Filter switched on (`follow_reach`).
struct ReachMove {
    settings: RepositorySettings,
    before: WorkFacts,
}

impl ReachMove {
    /// The move `marked` makes, under the Repository's `settings`: none for a mark other than Mute and
    /// Unmute. Facts the store can't read, before the mark or after it, leave the heads to the next step that
    /// saves, and lose the reads an Unmute owes.
    fn of(marked: &Marked, store: &Store, settings: &RepositorySettings) -> Option<Self> {
        if !marked.moves_reach() {
            return None;
        }
        let before = facts_for_move(store, marked.repository())?;
        let settings = settings.clone();
        Some(Self { settings, before })
    }

    /// Has the Mirror worker follow the move, once the store kept the mark. Reach after it goes by the facts
    /// the store kept.
    fn follow(self, fetching: &Fetching, store: &Store) {
        let Self { settings, before } = self;
        let Some(now) = facts_for_move(store, &settings.name) else {
            return;
        };
        let from = |facts| ReachFrom {
            facts,
            settings: &settings,
        };
        follow_move(fetching, store, from(&before), from(&now));
    }
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
    match facts {
        Ok(facts) => {
            // The reads are saved, so the window hears of them whether or not the checks are.
            if let Err(problem) = keep_checks(store, repository, &facts, OffsetDateTime::now_utc())
            {
                board.set(Status::Problem { problem });
            }
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
/// kept others, so their verdict stands while a newer check waits. Then dates from `now` each Review a check
/// finds stale, or can't check, for the first time, before the window hears of it (#908).
pub(crate) fn keep_checks(
    store: &Store,
    repository: &RepositoryName,
    facts: &WorkFacts,
    now: OffsetDateTime,
) -> Result<(), Error> {
    let checks = checks_to_keep(facts);
    if !checks.is_empty() {
        store.save_last_checks(repository, checks)?;
    }
    let findings = findings_to_keep(facts);
    if findings.is_empty() {
        return Ok(());
    }
    store.save_findings(repository, findings, now)
}

/// Tells the window of `change`, which a mark made, or the status bar why the store couldn't keep it.
fn tell_marked(board: &mut StatusBoard, saved: Result<(), Error>, change: &Change) {
    match saved {
        Ok(()) => board.changed(change),
        Err(problem) => board.set(Status::Problem { problem }),
    }
}

/// Keeps what the user marked, with the change to tell the window of. Mute author and Unmute author find
/// the author's items under the Repository's `settings`.
pub(crate) fn keep_mark(
    store: &Store,
    settings: &RepositorySettings,
    marked: Marked,
) -> (Result<(), Error>, Change) {
    match marked {
        Marked::Seen { repository, mark } => {
            let SeenMark {
                numbers,
                up_to,
                findings,
            } = mark;
            let change = Change {
                repository,
                items: numbers.clone(),
            };
            let seeing = Seeing {
                numbers,
                up_to,
                findings,
            };
            (store.mark_seen(&change.repository, seeing), change)
        }
        Marked::Unseen {
            repository,
            number,
            archived,
            at,
        } => {
            let saved = store.mark_unseen(&repository, Flagging { number, archived }, at);
            let unarchived = archived.map(ItemKey::number);
            let mut items = vec![number];
            items.extend(unarchived.filter(|holder| *holder != number));
            (saved, Change { repository, items })
        }
        Marked::Archived {
            repository,
            key,
            mark:
                SeenMark {
                    numbers,
                    up_to,
                    findings,
                },
            put_away,
            at,
        } => {
            let change = Change {
                repository,
                items: numbers.clone(),
            };
            let archiving = Archiving {
                key,
                numbers,
                up_to,
                put_away,
                findings,
            };
            let saved = store.archive(&change.repository, archiving, at);
            (saved, change)
        }
        Marked::Unarchived { repository, key } => {
            let saved = store.unarchive(&repository, key);
            let items = vec![key.number()];
            (saved, Change { repository, items })
        }
        Marked::Muted {
            repository,
            key,
            mark:
                SeenMark {
                    numbers,
                    up_to,
                    findings,
                },
            at,
        } => {
            let change = Change {
                repository,
                items: numbers.clone(),
            };
            let muting = Muting {
                key,
                numbers,
                up_to,
                findings,
            };
            let saved = store.mute(&change.repository, muting, at);
            (saved, change)
        }
        Marked::Unmuted { repository, key } => {
            let saved = store.unmute(&repository, key);
            let items = vec![key.number()];
            (saved, Change { repository, items })
        }
        Marked::AuthorMuted {
            repository,
            author,
            at,
        } => {
            let authored = authored_in(store, settings, &author);
            let (saved, items) = match authored {
                Ok(Authored { marks, numbers }) => {
                    let muting = AuthorMuting { author, marks };
                    (store.mute_author(&repository, muting, at), numbers)
                }
                Err(problem) => (Err(problem), Vec::new()),
            };
            (saved, Change { repository, items })
        }
        Marked::AuthorUnmuted { repository, author } => {
            // The window asks again on any change, so a failed read loses only the numbers it names.
            let items = (authored_in(store, settings, &author))
                .inspect_err(|problem| {
                    // `log::warn!` evaluates a call among its arguments only while a logger listens.
                    let login = author.as_str();
                    log::warn!(
                        "Tsugiki couldn't read what unmuting {login} brings back in {repository}: {}",
                        problem.message
                    );
                })
                .map(|authored| authored.numbers)
                .unwrap_or_default();
            let saved = store.unmute_author(&repository, &author);
            (saved, Change { repository, items })
        }
    }
}

/// What muting or unmuting `author` touches in the stored facts of the Repository `settings` name.
fn authored_in(
    store: &Store,
    settings: &RepositorySettings,
    author: &Login,
) -> Result<Authored, Error> {
    let facts = store.work_facts(&settings.name)?;
    Ok(authored_by(facts, settings, author))
}

/// An Issue's or a PR's content, as GitHub answered.
enum Content {
    Issue(IssueContent),
    /// A PR's, and GitHub's `updatedAt` for it at the read.
    PullRequest(PullRequestContent, OffsetDateTime),
}

/// What a read of a conversation asks: GitHub, the session's queue, whose demands for the same read share
/// it, and the Mirror worker with the syncs, which say whose wants a PR's Conversation can change.
struct Reading<'a> {
    github: &'a GitHub,
    queue: &'a Refreshes,
    syncs: &'a Syncs,
}

/// Reads what an Issue or a PR says from GitHub, keeps it, and tells the window. The demands for the same
/// read that came before GitHub answered share its answer, its failure too.
///
/// GitHub numbers Issues and PRs from one sequence, so they share where they are kept. If the read fails,
/// the view goes on showing what was last read, and the window hears why, as do the Mirror worker and the
/// Files tab of a PR, which may wait for the head the Conversation names. An error returned here speaks
/// for all of GitHub, so the status bar and the syncs need to hear it. Any other trouble is this one item's,
/// and goes to the log.
fn read_conversation(
    board: &mut StatusBoard,
    store: &Store,
    read: &Reading<'_>,
    fetched: &Fetched,
) -> Result<(), Error> {
    let answered = ask_github(board, read.github, fetched);
    read.queue.merge(fetched);
    let item = fetched.item();
    let content = match answered? {
        Ok(content) => content,
        Err(why) => {
            read.queue.read_failed(board, fetched, why);
            return Ok(());
        }
    };
    let change = match keep_conversation(board, store, item, content) {
        None => return Ok(()),
        Some(Kept::Issue(change)) => change,
        Some(Kept::PullRequest(change, saved)) => {
            hand_over_head(store, read, item, &saved);
            change
        }
    };
    board.changed(&change);
    Ok(())
}

/// Tells the Mirror worker and the Files tab what keeping PR `item`'s Conversation did, as `saved` says.
///
/// The Conversation names the PR's head, and the worker hears the head the facts keep is saved: the newer
/// read's, the list's or this one's. A PR the user opened before Tsugiki knew its head gets it first, and a
/// PR pushed to since the last list read gets the newer head before the next. The demands were merged
/// before this, so the worker hears each await their screens sent before the head that answers it. A
/// Conversation that names no head, or a PR the facts don't hold, ends the await. A head that moved, or a
/// Review of the user's that covers another commit, changes what the rules want, so the worker hears their
/// wants again first.
fn hand_over_head(store: &Store, read: &Reading<'_>, item: &Item, saved: &ConversationSaved) {
    let fetching = &read.queue.fetching;
    let repository = &item.repository;
    if (matches!(saved.head, HeadKept::Moved(_)) || saved.reviewed_moved)
        && let Some(synced) = synced_facts(store, read.syncs, repository)
    {
        hand_over_rules_wants(fetching, store, &synced);
    }
    match &saved.head {
        HeadKept::Moved(head) | HeadKept::Had(head) => {
            fetching.heads_saved(repository, vec![head.clone()]);
            read.queue.headless.forget(item);
        }
        HeadKept::Unnamed => {
            fetching.no_head(repository.clone(), item.number);
            read.queue.headless.note(item, NoHead::Unnamed);
        }
        HeadKept::NoFacts => {
            fetching.no_head(repository.clone(), item.number);
            read.queue.headless.forget(item);
        }
    }
}

/// What a read of a conversation kept: the change the window hears, and for a PR what keeping it did
/// beside it.
#[derive(Debug, PartialEq, Eq)]
enum Kept {
    Issue(Change),
    PullRequest(Change, ConversationSaved),
}

/// What an Issue or a PR says, as GitHub answered `fetched`, or why the read failed for this item alone, as
/// `read_conversation` says. The status bar hears a failure that speaks for all of GitHub, and this logs
/// the others, which the caller tells the window of.
fn ask_github(
    board: &mut StatusBoard,
    github: &GitHub,
    fetched: &Fetched,
) -> Result<Result<Content, ItemUnread>, Error> {
    let item = fetched.item();
    let (noun, read) = match fetched {
        Fetched::Issue(_) => {
            let read = github.issue_content(&item.repository, item.number);
            ("Issue", read.map(Content::Issue))
        }
        Fetched::PullRequest(_) => {
            let read = github.pull_request_content(&item.repository, item.number);
            let read = read.map(|read| Content::PullRequest(read.content, read.updated_at));
            ("PR", read)
        }
    };
    let content = match read {
        Ok(content) => content,
        Err(problem) if problem.kind.speaks_for_all_of_github() => {
            board.failed_read(&failed(item, ReadFailure::WaitsOnGitHub));
            return Err(problem);
        }
        Err(problem) => {
            let why = if matches!(problem.kind, ErrorKind::NotFound) {
                ItemUnread::NotOnGitHub
            } else {
                ItemUnread::Other
            };
            log::warn!(
                "Tsugiki couldn't read {noun} #{} in {}: {}",
                item.number,
                item.repository,
                problem.message
            );
            return Ok(Err(why));
        }
    };
    Ok(Ok(content))
}

/// Keeps `content`, what `item` says, and gives what that kept, or nothing when the store can't keep it,
/// which the status bar shows.
fn keep_conversation(
    board: &mut StatusBoard,
    store: &Store,
    item: &Item,
    content: Content,
) -> Option<Kept> {
    let Item { repository, number } = item;
    let change = Change {
        repository: repository.clone(),
        items: vec![*number],
    };
    let kept = match content {
        Content::Issue(content) => {
            (store.save_issue_content(repository, *number, content)).map(|()| Kept::Issue(change))
        }
        Content::PullRequest(content, updated_at) => {
            (store.save_read_of_pull_request(repository, *number, (content, updated_at)))
                .map(|saved| Kept::PullRequest(change, saved))
        }
    };
    match kept {
        Ok(kept) => Some(kept),
        Err(problem) => {
            board.set(Status::Problem { problem });
            None
        }
    }
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
