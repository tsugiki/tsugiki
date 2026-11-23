//! One run of Tsugiki: opens the database, then keeps the status following GitHub until the app ends.
//!
//! Two threads share the work, each the one writer of what it owns. The GitHub thread owns GitHub access
//! and the status board, and sends one request at a time (`github_thread.rs`). The session thread takes
//! every message, does at once what needs no GitHub, such as the user's marks, a change of the settings
//! and the patch-ids the Mirror worker reads, and passes each demand on GitHub to the GitHub thread's queue.
//! So nothing that needs no request waits for one that hangs.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, VecDeque};
use std::ops::ControlFlow;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use time::OffsetDateTime;

use crate::error::{Error, ErrorKind};
use crate::facts::activity::ItemKey;
use crate::facts::pull_requests::PrHead;
use crate::facts::{CommitSha, WorkFacts};
use crate::fetching::{
    Fetching, Headless, Item, ItemFetch, ItemUnfetched, Marked, MirrorWant, SessionMessage,
    answered,
};
use crate::folders::Folders;
use crate::github::GitHubAccess;
use crate::latest::LatestReader;
use crate::login::Login;
use crate::mirror::{MirrorRequests, SizeOf};
use crate::patch_ids::PatchReads;
use crate::repository::RepositoryName;
use crate::repository_settings::{RepositorySettings, SavedBy, SettingsSave};
use crate::settings::SettingsView;
use crate::status::{Change, Changes, Shown, StatusBoard};
use crate::store::{
    self, Archiving, AuthorMuting, Flagging, Muting, OpenReader, ReaderToOpen, Seeing, Store,
    StoreReader,
};
use crate::work::{MirrorWork, ReachFrom, SeenMark, checks_to_keep, findings_to_keep, moved};
use crate::workbench::{Authored, authored_by};
use github_thread::{GitHubMessage, Refreshes};
use refill::LinkRefill;

mod github_thread;
mod on_screen;
mod read_only;
mod refill;
mod unstored;
pub mod write;

// `run` catches a panic in its threads, to keep the window up and say that checking stopped. Tests unwind
// whatever the release profile says, so only this line can catch the setting (docs/build-costs.md).
#[cfg(panic = "abort")]
compile_error!("`session::run` catches panics, so Tsugiki can't build with `panic = \"abort\"`");

/// What the status bar says once a panic stopped a thread of the session.
fn stopped_checking() -> Error {
    Error::new(
        ErrorKind::Internal,
        "Tsugiki stopped checking GitHub. Restart Tsugiki to check again.",
    )
}

/// A run of Tsugiki on threads of its own, so the window opens without waiting on the disk, the token or the
/// network (ADR 0017). It shows its own failures on the board.
#[must_use = "dropping the session stops it"]
pub struct Session {
    /// The handle the app asks through, which also tells this session to stop.
    fetching: Fetching,
    finished: Receiver<()>,
    reader: StoreReader,
    /// The status board's copy of whether GitHub can be asked now.
    shown: LatestReader<Shown>,
}

/// What a session reaches beyond its threads: GitHub with `gh`'s token, the Mirror worker, which it hands
/// the rules' wants, which hands it the patch-ids it reads, and which asks it a Repository's size, and the
/// settings owner's view, which it reads each Repository's settings from.
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
        let (to_github, github_queue) = mpsc::channel();
        let shown = board.shown();
        mirrors.size_through(sizes_through(to_github.clone(), shown.clone()));
        let (messages, incoming) = mpsc::channel();
        let fetching = Fetching::new(messages, mirrors, Headless::default());
        fetching.hand_patch_ids_to_session();
        let (finishing, finished) = mpsc::channel::<()>();
        let (to_open, reader) = store::reader();
        let database = Database { folders, to_open };
        let (refreshes, refreshes_done) =
            Refreshes::new(intervals, github_queue, fetching.clone(), settings.clone());
        let keeper = Keeper::new(
            incoming,
            fetching.clone(),
            settings,
            Telling {
                changes: board.changes(),
                github: to_github,
                shown: shown.clone(),
                refreshes_done,
            },
        );
        let threads = Threads {
            board,
            access,
            refreshes,
        };
        #[expect(
            unused_results,
            reason = "`stop` waits for `finishing` to go, with a patience a join can't take"
        )]
        thread::spawn(move || {
            // `stop` learns the session is over when this goes, however the thread ends.
            let _finishing = finishing;
            run(database, threads, &keeper);
        });
        Self {
            fetching,
            finished,
            reader,
            shown,
        }
    }

    /// A copy of what the status bar shows of whether GitHub can be asked now.
    pub(crate) fn shown(&self) -> LatestReader<Shown> {
        self.shown.clone()
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
    /// Between requests it closes at once. A request to GitHub, a call to `gh` or a migration can outlast
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

/// Asks the GitHub thread for a Repository's size, for the Mirror worker before a clone. The rate limit's
/// gate holds the request as it holds every other, and its cost counts in the hour's points, so the gate's
/// answer says when the clone can go. A session that has stopped, or has no database, answers that it isn't
/// connected. While the status `shown` says GitHub can't be reached, it answers so at once, as a request
/// would only keep the worker waiting for its failure, behind the request the GitHub thread waits on.
fn sizes_through(github: Sender<GitHubMessage>, shown: LatestReader<Shown>) -> SizeOf {
    Box::new(move |repository| {
        if shown.read(|shown| shown.account.is_offline()) {
            let words =
                format!("GitHub can't be reached, so Tsugiki can't ask how big {repository} is.");
            return Err(Error::new(ErrorKind::Offline, &words));
        }
        let repository = repository.clone();
        answered(&github, |reply| GitHubMessage::Size { repository, reply })
    })
}

/// What the GitHub thread starts with, once the database is open.
struct Threads {
    board: StatusBoard,
    access: GitHubAccess,
    refreshes: Refreshes,
}

/// Where the session tells of what its own saves did, and what it reads of the GitHub thread's: the window
/// hears of each change, and the status bar, which the GitHub thread writes, of a problem. The board's copy
/// says whether GitHub can be asked now, and the thread's how many of the user's refreshes it has done.
struct Telling {
    changes: Changes,
    github: Sender<GitHubMessage>,
    shown: LatestReader<Shown>,
    refreshes_done: LatestReader<u64>,
}

impl Telling {
    fn changed(&self, change: &Change) {
        self.changes.changed(change);
    }

    /// Has the GitHub thread show `problem` on the status bar, once it has answered the request it waits on.
    fn problem(&self, problem: Error) {
        self.to_github(GitHubMessage::Problem(problem));
    }

    fn to_github(&self, message: GitHubMessage) {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "this fails only once the GitHub thread has stopped, which a stop or a panic does, and \
                      then nothing is left to hear the message"
        )]
        let _ = self.github.send(message);
    }
}

/// Which Repositories' hand edits to the settings the window hears of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandEdits {
    /// Those whose lists the window shows: the Repositories it opened since launch.
    Listed,
    /// Every Repository's.
    Every,
}

/// The session thread's queue and what it keeps: the user's marks, the settings owner's saves, the patch-ids
/// the Mirror worker reads and the refill of the links, none of which needs GitHub. It passes the rest to the
/// GitHub thread.
struct Keeper {
    incoming: Receiver<SessionMessage>,
    /// The messages taken off `incoming` while following owed saves, which go first, in the order they came.
    backlog: RefCell<VecDeque<SessionMessage>>,
    /// The handle the session hands the rules' wants to the Mirror worker through.
    fetching: Fetching,
    /// The settings owner's view. The owner publishes each save there and hands it to the session in one
    /// step, so a read finds settings at least as new as every save the session has heard of.
    settings: LatestReader<SettingsView>,
    /// How many saves of a Repository's settings the session has followed, to tell from the view's count
    /// those in its queue.
    saves_taken: Cell<usize>,
    /// The refill of the links the store kept under rules other than `LinkRule::SET`, which takes a step when
    /// no message waits.
    links: RefCell<LinkRefill>,
    /// The Repositories the window opened since launch, as their Syncs passed through this queue, which the
    /// GitHub thread's syncs take after. A hand edit to the settings of one of them reaches the window.
    opened: RefCell<Vec<RepositoryName>>,
    telling: Telling,
    /// How many of the user's refreshes the session passed to the GitHub thread.
    refreshes_passed: Cell<u64>,
}

impl Keeper {
    fn new(
        incoming: Receiver<SessionMessage>,
        fetching: Fetching,
        settings: LatestReader<SettingsView>,
        telling: Telling,
    ) -> Self {
        Self {
            incoming,
            backlog: RefCell::new(VecDeque::new()),
            fetching,
            settings,
            saves_taken: Cell::new(0),
            links: RefCell::new(LinkRefill::default()),
            opened: RefCell::new(Vec::new()),
            telling,
            refreshes_passed: Cell::new(0),
        }
    }

    /// Takes each message until a stop, and between them a step of the links' refill when one is due.
    fn run(&self, store: &Store) {
        *self.links.borrow_mut() = LinkRefill::owed_by(store);
        loop {
            // A wait too long for the clock waits for the next message, however long it takes.
            let wait = if self.links.borrow().is_due() {
                Duration::ZERO
            } else {
                Duration::MAX
            };
            let message = match self.next_message(wait) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout) => {
                    self.links.borrow_mut().step(&self.telling.changes, store);
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return,
            };
            if self.take(store, message).is_break() {
                return;
            }
        }
    }

    /// Does what `message` asks, or passes it to the GitHub thread. Breaks on a stop.
    fn take(&self, store: &Store, message: SessionMessage) -> ControlFlow<()> {
        let telling = &self.telling;
        match message {
            SessionMessage::Stop => return ControlFlow::Break(()),
            SessionMessage::Mark(marked) => self.mark(store, marked),
            SessionMessage::SettingsChanged(save) => {
                self.follow_settings(store, &save, HandEdits::Listed);
            }
            SessionMessage::Patched(repository, patches) => {
                keep_patches(telling, store, &repository, patches);
            }
            SessionMessage::Saved(saved) => self.hand_over_saved(store, &saved),
            SessionMessage::Sync { repository } => {
                self.links.borrow_mut().put_first(&repository);
                if !self.shows(&repository) {
                    self.opened.borrow_mut().push(repository.clone());
                }
                telling.to_github(GitHubMessage::Sync { repository });
            }
            // An item of another Repository, as quick open opens one, reads its links on screen too.
            SessionMessage::Fetch(fetched) => {
                (self.links.borrow_mut()).put_first(&fetched.item().repository);
                telling.to_github(GitHubMessage::Fetch(fetched));
            }
            SessionMessage::FetchUnstored { item, reply } => {
                (self.links.borrow_mut()).put_first(&item.repository);
                self.fetch_unstored(item, reply);
            }
            SessionMessage::Refresh => self.refresh(),
            SessionMessage::Check { name, reply } => {
                telling.to_github(GitHubMessage::Check { name, reply });
            }
            SessionMessage::Screen(moved) => telling.to_github(GitHubMessage::Screen(moved)),
            SessionMessage::ShowMuted {
                repository,
                show,
                reply,
            } => {
                #[expect(
                    clippy::let_underscore_must_use,
                    reason = "this fails only if the caller has gone, and nobody then shows the answer"
                )]
                let _ = reply.send(store.switch_show_muted(&repository, show));
            }
        }
        ControlFlow::Continue(())
    }

    /// Passes a fetch of an item the database lacks to the GitHub thread, or answers at once while the status
    /// says GitHub can't be reached or the rate limit's gate holds, as a request then would only keep the
    /// user waiting for its failure, behind the request the GitHub thread waits on.
    fn fetch_unstored(&self, item: Item, reply: Sender<ItemFetch>) {
        if self.telling.shown.read(Shown::refuses_requests) {
            #[expect(
                clippy::let_underscore_must_use,
                reason = "this fails only if the caller has gone, and nobody then shows the answer"
            )]
            let _ = reply.send(ItemFetch::Unfetched {
                why: ItemUnfetched::Offline,
            });
            return;
        }
        (self.telling).to_github(GitHubMessage::FetchUnstored { item, reply });
    }

    /// Passes the user's refresh to the GitHub thread, unless one it passed still waits there or runs, which
    /// covers this one: offline, a second click while the first asks who is signed in would only ask again.
    fn refresh(&self) {
        if self.refreshes_passed.get() > self.telling.refreshes_done.get() {
            return;
        }
        self.refreshes_passed.set(self.refreshes_passed.get() + 1);
        self.telling.to_github(GitHubMessage::Refresh);
    }

    /// Whether the window opened `repository` since launch, and lists its items.
    fn shows(&self, repository: &RepositoryName) -> bool {
        (self.opened.borrow().iter()).any(|opened| opened.is_same(repository))
    }

    /// Hands the Mirror worker what the rules want of the Repository whose facts the GitHub thread `saved`,
    /// from the facts the store holds now: the heads of the PRs in its Reach, the commit each of the user's
    /// Reviews there covered, and the commits whose patch-ids the Stale review check needs and the store
    /// lacks. It keeps the last check of each PR whose sides are both read first. Then the heads of the items
    /// `saved` names, so a PR the user opened before its head was known gets its head first. Only a
    /// Repository the window opened since launch syncs, so only it hands over wants. This thread alone works
    /// the wants out and keeps the checks, so a set read before a mark never lands after the mark's own. A
    /// store that can't read the facts hands nothing, and the next step that saves says if it is in trouble,
    /// and hands them again.
    fn hand_over_saved(&self, store: &Store, saved: &Change) {
        let repository = &saved.repository;
        let shown = self
            .shows(repository)
            .then(|| store.work_facts(repository).ok());
        let Some(facts) = shown.flatten() else {
            return;
        };
        let settings = (self.settings).read(|view| view.repository(repository.clone()));
        let stated = Stated {
            repository,
            facts: &facts,
            work: MirrorWork::of(&facts, &settings),
        };
        hand_over_wants(&self.fetching, store, stated);
        let saved_heads = (facts.pull_requests.iter())
            .filter(|pr| saved.items.contains(&pr.number))
            .filter_map(|pr| {
                Some(PrHead {
                    number: pr.number,
                    commit: pr.head.clone()?,
                    target_branch: pr.target_branch.clone(),
                })
            })
            .collect::<Vec<_>>();
        if !saved_heads.is_empty() {
            self.fetching.heads_saved(repository, saved_heads);
        }
    }

    /// Has the Mirror worker follow the move of Reach `save` makes, counts it as followed, and tells the
    /// window of a hand edit to a Repository whose lists it shows, or to any, as `edits` says.
    fn follow_settings(&self, store: &Store, save: &SettingsSave, edits: HandEdits) {
        self.saves_taken.set(self.saves_taken.get() + 1);
        follow_save(&self.fetching, store, save);
        let repository = &save.saved().name;
        if edits == HandEdits::Every || self.shows(repository) {
            announce_settings(&self.telling, repository, save.by());
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
    fn follow_saves_told(&self, store: &Store, told: usize) {
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
        for save in &saves {
            self.follow_settings(store, save, HandEdits::Every);
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

    /// Keeps what the user marked, has the Mirror worker follow the move of Reach the mark makes, under the
    /// Repository's settings now, if the store kept it, and tells the window, whose lists go by the mark.
    ///
    /// A mark that moves Reach first follows each save counted in the view it reads the settings from, as
    /// those settings hold them. The saves' moves of Reach then end where the mark's starts, and the two
    /// chain. Without, a save that comes after the mark moves Reach on the facts after it, and a PR that only
    /// the pair puts in Reach is owed by neither.
    fn mark(&self, store: &Store, marked: Marked) {
        let repository = marked.repository().clone();
        let (told, settings) =
            (self.settings).read(|view| (view.saves_told, view.repository(repository)));
        if marked.moves_reach() {
            self.follow_saves_told(store, told);
        }
        let moved = ReachMove::of(&marked, store, &settings);
        let (saved, change) = keep_mark(store, &settings, marked);
        if saved.is_ok()
            && let Some(moved) = moved
        {
            moved.follow(&self.fetching, store);
        }
        tell_marked(&self.telling, saved, &change);
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

/// Opens the database, then runs the GitHub thread beside the session's until the app stops them. Without a
/// database Tsugiki doesn't ask GitHub anything: the board shows why, and a database from a newer Tsugiki
/// stays as it is (ADR 0010).
///
/// Nothing else would show a panic in here: the last status would stay up and look current, so this replaces
/// it with a problem. A panic in the session's thread has the GitHub thread say so and stop. It relies on
/// panics unwinding, so `panic = "abort"` would end the app instead (#57).
fn run(database: Database, threads: Threads, keeper: &Keeper) {
    let Threads {
        mut board,
        access,
        refreshes,
    } = threads;
    let opened = catch_unwind(AssertUnwindSafe(|| match database.open() {
        Ok(opened) => Some(opened),
        Err(problem) => {
            board.problem(problem);
            None
        }
    }));
    let (store, reader) = match opened {
        Ok(Some(opened)) => opened,
        Ok(None) => return,
        Err(_) => {
            board.problem(stopped_checking());
            return;
        }
    };
    thread::scope(|scope| {
        let shared = &store;
        #[expect(
            unused_results,
            reason = "the scope joins the GitHub thread, which catches its own panics"
        )]
        scope.spawn(move || github_thread::run_thread(board, shared, access, &refreshes));
        let kept = catch_unwind(AssertUnwindSafe(|| keeper.run(&store)));
        if kept.is_err() {
            keeper.telling.problem(stopped_checking());
        }
        keeper.telling.to_github(GitHubMessage::Stop);
    });
    reader.close();
    store.close();
}

/// The Mirror's work in a Repository, with the stored facts the rules worked it out from.
struct Stated<'a> {
    repository: &'a RepositoryName,
    facts: &'a WorkFacts,
    work: MirrorWork,
}

/// Hands the Mirror worker what the rules want of it: the heads of the PRs in Reach, the commit each of the
/// user's Reviews there covered, and the commits whose patch-ids the Stale review check needs and the store
/// lacks. It keeps the last check of each PR whose sides are both read first.
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
fn announce_settings(telling: &Telling, repository: &RepositoryName, by: SavedBy) {
    if by == SavedBy::HandEdit {
        telling.changed(&Change {
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

/// A Mute or an Unmute, which moves Reach (#918), or a Pin or an Unpin (#488), with the Repository's
/// settings and the stored facts from before the store keeps it. It moves Reach with or without a token
/// (#946).
///
/// Mute takes the item's tree out of Reach, apart from what an ask or another item keeps there, so the
/// Mirror worker stops wanting their heads. Unmute puts the tree back, which no change on GitHub would have
/// the Mirror fetch the heads of or a refresh read ahead. So the Mirror worker hears the new Reach at once,
/// and the store owes the reads the rules say Unmute owes, as with a Filter switched on (`follow_reach`).
/// Pin puts the Issue's tree in Reach as Unmute does, and Unpin takes out what nothing else keeps there.
struct ReachMove {
    settings: RepositorySettings,
    before: WorkFacts,
}

impl ReachMove {
    /// The move `marked` makes, under the Repository's `settings`: none for a mark that moves no Reach.
    /// Facts the store can't read, before the mark or after it, leave the heads to the next step that saves,
    /// and lose the reads an Unmute or a Pin owes.
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
fn keep_patches(telling: &Telling, store: &Store, repository: &RepositoryName, reads: PatchReads) {
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
                telling.problem(problem);
            }
            let reviewed = &facts.reviews.reviewed;
            let checked = facts.pull_requests.iter().filter(|pr| {
                let sides = pr.head.iter().chain(reviewed.get(&pr.number));
                sides.into_iter().any(|commit| commits.contains(commit))
            });
            telling.changed(&Change {
                repository: repository.clone(),
                items: checked.map(|pr| pr.number).collect(),
            });
        }
        Err(problem) => telling.problem(problem),
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
fn tell_marked(telling: &Telling, saved: Result<(), Error>, change: &Change) {
    match saved {
        Ok(()) => telling.changed(change),
        Err(problem) => telling.problem(problem),
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
        Marked::Pinned { repository, number } => {
            let saved = store.pin(&repository, number);
            let items = vec![number];
            (saved, Change { repository, items })
        }
        Marked::Unpinned { repository, number } => {
            let saved = store.unpin(&repository, number, OffsetDateTime::now_utc());
            let items = vec![number];
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

#[cfg(test)]
mod tests;
