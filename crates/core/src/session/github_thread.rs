//! The GitHub thread: the one owner of GitHub access. It holds the token, the client, GitHub's clock, the
//! hour's points and the rate limit's gate, and sends one request at a time (ADR 0015). It runs the syncs,
//! the reads of what's on screen and the user's demands on GitHub, which the session passes it, and writes
//! the status, the sync's activity, the hour's points and the last syncs. The session reads the status, the
//! gate's hold and the user's refreshes done as copies, so it answers at once what needs no request while a
//! request is out.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use time::OffsetDateTime;

use crate::error::{Error, ErrorKind};
use crate::facts::issue_content::IssueContent;
use crate::facts::pull_request_content::PullRequestContent;
use crate::fetching::{
    Fetched, Fetching, Headless, Item, ItemFetch, ItemUnfetched, NoHead, ScreenMove,
};
use crate::github::in_flight::{self, AnnounceInFlight, Asking};
use crate::github::{GitHub, GitHubAccess};
use crate::latest::{self, LatestReader, LatestWriter};
use crate::repository::RepositoryName;
use crate::repository_settings::RepositoryCheck;
use crate::settings::SettingsView;
use crate::status::{
    Account, Change, FailedRead, ItemUnread, LastSync, ReadFailure, Status, StatusBoard,
};
use crate::store::{ConversationSaved, HeadKept, Store};
use crate::sync::{Progress, Syncs};
use crate::token::TokenReads;

use super::on_screen::OnScreen;
use super::read_only::{ThreadParts, WriteJudge};
use super::{Intervals, stopped_checking, unstored};

/// What the GitHub thread is asked. The session passes it each demand on GitHub, and the Mirror worker asks
/// it a Repository's size. It takes each between two sync steps, so the user's demands go ahead of the sync's
/// tiers.
pub(super) enum GitHubMessage {
    Stop,
    /// The session's thread stopped for good, for this reason, so the GitHub thread says Tsugiki asks GitHub
    /// nothing more, and stops too (#1070).
    Stopped(Error),
    /// Add repository's check.
    Check {
        name: RepositoryName,
        reply: Sender<Result<RepositoryCheck, Error>>,
    },
    /// The user opened this Repository, whose settings the syncs read from the settings owner's view.
    Sync {
        repository: RepositoryName,
    },
    /// The user asked to refresh the open Repository now.
    Refresh,
    /// A read of what an item says. A demand that comes while the same read waits or runs shares its outcome.
    Fetch(Fetched),
    /// What the user did with an item on screen, or with the window.
    Screen(ScreenMove),
    /// A fetch of an Issue or a PR the database lacks, which answers what it found (#981).
    FetchUnstored {
        item: Item,
        reply: Sender<ItemFetch>,
    },
    /// The user typed `text` in the Inbox's search box, which GitHub answers (#465).
    Search {
        repository: RepositoryName,
        text: String,
    },
    /// The size of a Repository on GitHub, which the Mirror worker asks before a clone.
    Size {
        repository: RepositoryName,
        reply: Sender<Result<u64, Error>>,
    },
    /// The store couldn't keep what the session saved, which the status bar says.
    Problem(Error),
}

/// What brings on a read of the token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TokenRead {
    /// The GitHub thread starts.
    Start,
    /// The status check's interval passed since a read that found none.
    Due,
    /// The user clicked Refresh after a read that found none.
    Clicked,
}

/// GitHub access with no token yet, which reads the token again when asked, what hears of each read while
/// it is out, until the client takes it, the queue whose clicks a read covers, and the reads so far, so
/// the log says once why each minute's read finds none.
pub(super) struct Unconnected<'a> {
    access: GitHubAccess,
    announce: RefCell<AnnounceInFlight>,
    refreshes: &'a Refreshes,
    reads: TokenReads,
}

/// Connects at the thread's start, as [`Unconnected::connect`] says.
#[must_use = "without the client, nothing asks GitHub"]
pub(super) fn connect<'a>(
    board: &mut StatusBoard,
    store: &Store,
    access: GitHubAccess,
    refreshes: &'a Refreshes,
) -> Result<GitHub, Unconnected<'a>> {
    let announce = board.in_flight().unwrap_or_else(|| Box::new(|_| {}));
    let unconnected = Unconnected {
        access,
        announce: RefCell::new(announce),
        refreshes,
        reads: TokenReads::default(),
    };
    unconnected.connect(board, store, TokenRead::Start)
}

impl Unconnected<'_> {
    /// Reads the token (from gh only when `GH_TOKEN` is unset), asks GitHub who it belongs to, and shows the
    /// result on `board`. Returns the client when there is a token, even offline, since GitHub may come
    /// back, and else itself, to read again later, with the problem on `board`. At the start and on a click,
    /// the status bar says "Connecting…" meanwhile, with who was signed in last time. A read that finds no
    /// token covers each click that came while it was out.
    fn connect(
        mut self,
        board: &mut StatusBoard,
        store: &Store,
        read: TokenRead,
    ) -> Result<GitHub, Self> {
        let refreshes = self.refreshes;
        match read {
            TokenRead::Start | TokenRead::Clicked => board.account(connecting(store)),
            // The status bar keeps saying why there is no token, so it doesn't change each minute.
            TokenRead::Due => {}
        }
        let from = self.access.token_from;
        let reads = &mut self.reads;
        let token = in_flight::announced(&self.announce, Asking::Token, || reads.read(from));
        // Before the board hears the answer, so a click once the status bar says it asks again (#987). A
        // click that came while a read that found a token was out asks GitHub, as it would have before.
        if token.is_err() {
            refreshes.clicks_covered();
        }
        if read == TokenRead::Clicked {
            refreshes.refresh_done();
        }
        match token {
            Ok(token) => {
                let mut github = GitHub::new(self.access.transport, token);
                github.announce_in_flight(self.announce.into_inner());
                refresh(board, store, &github);
                Ok(github)
            }
            // A finished read, so it takes away the problem beside the account, as a status check does.
            Err(problem) => {
                board.set(Status::from(Account::NoToken {
                    message: problem.message,
                }));
                Err(self)
            }
        }
    }
}

/// What the account says while Tsugiki connects: who was signed in last time, if the store says. The last
/// login only fills the status bar for a moment, so a failed read doesn't stop Tsugiki connecting. `refresh`
/// uses the store next, and shows the problem if the store is still failing.
fn connecting(store: &Store) -> Account {
    match store.last_login() {
        Ok(Some(last_login)) => Account::ConnectingAs { last_login },
        Ok(None) | Err(_) => Account::Connecting,
    }
}

/// The news for the window that a read of what `item` says failed, and `why`.
fn failed(item: &Item, why: ReadFailure) -> FailedRead {
    FailedRead {
        repository: item.repository.clone(),
        number: item.number,
        why,
    }
}

/// When to ask GitHub again, the queue the GitHub thread takes its messages from, and how many of the user's
/// refreshes it has done, which the session reads as a copy.
pub(super) struct Refreshes {
    pub(super) intervals: Intervals,
    pub(super) incoming: Receiver<GitHubMessage>,
    /// The messages taken off `incoming` while merging demands, which go first, in the order they came.
    pub(super) backlog: RefCell<VecDeque<GitHubMessage>>,
    /// The handle the thread hands the rules' wants to the Mirror worker through.
    pub(super) fetching: Fetching,
    /// The PRs whose Conversation named no head or failed to read, which `fetching` shares.
    pub(super) headless: Headless,
    /// The Issues and PRs on screen, each read again after its last read.
    pub(super) on_screen: RefCell<OnScreen>,
    /// The settings owner's view, which the syncs read each Repository's settings from.
    pub(super) settings: LatestReader<SettingsView>,
    /// How many of the user's refreshes it has done, and the copy the session counts those it passed on
    /// against.
    pub(super) refreshes_done: Cell<u64>,
    pub(super) done_copy: RefCell<LatestWriter<u64>>,
}

/// When the next of each kind of question is due.
#[derive(Debug, Clone, Copy)]
pub(super) struct Due {
    pub(super) status: Instant,
    pub(super) repository: Instant,
}

impl Due {
    pub(super) fn starting_at(now: Instant, intervals: Intervals) -> Self {
        Self {
            status: now + intervals.status,
            repository: now + intervals.repository,
        }
    }
}

/// The GitHub thread's next move.
#[derive(Debug)]
pub(super) enum Turn {
    /// Time to ask who is signed in.
    Refresh,
    /// Time to refresh the open Repository.
    RefreshRepository,
    /// The user asked to refresh the open Repository.
    RefreshNow,
    /// The window opened this Repository, so it needs to know when its last sync was.
    Opened(RepositoryName),
    /// An Issue or a PR on screen needs its conversation read from GitHub.
    Fetch(Fetched),
    /// The user asked for an Issue or a PR the database lacks, and waits for what the fetch found.
    FetchUnstored(Item, Sender<ItemFetch>),
    /// The user typed a search in the Inbox's box, which GitHub answers (#465).
    Search {
        repository: RepositoryName,
        text: String,
    },
    /// The session couldn't keep what it saved.
    Problem(Error),
    /// Time to see whether the hour's points have reset, so the window stops showing the old hour's count.
    CheckHour,
    /// Add repository's check or the Mirror worker's size check went to GitHub, so the window hears the
    /// hour's points it cost, and the time they come back if the rate limit refused it. The turn sets no
    /// timer back, so a check puts off nothing.
    Asked,
    SyncStep,
    Stop,
    /// The session's thread stopped for good, for this reason, so the thread stops too.
    Stopped(Error),
}

impl Refreshes {
    /// The thread's state, and the copy of how many of the user's refreshes it has done, which the session
    /// reads.
    pub(super) fn new(
        intervals: Intervals,
        incoming: Receiver<GitHubMessage>,
        fetching: Fetching,
        settings: LatestReader<SettingsView>,
    ) -> (Self, LatestReader<u64>) {
        let (done_copy, copy) = latest::channel(0);
        let refreshes = Self {
            intervals,
            incoming,
            backlog: RefCell::new(VecDeque::new()),
            headless: fetching.headless(),
            fetching,
            on_screen: RefCell::new(OnScreen::new(intervals.on_screen)),
            settings,
            refreshes_done: Cell::new(0),
            done_copy: RefCell::new(done_copy),
        };
        (refreshes, copy)
    }

    /// The Repositories the settings list, in their order.
    fn listed(&self) -> Vec<RepositoryName> {
        self.settings.read(|view| {
            let repositories = &view.loaded.settings.repositories;
            repositories
                .iter()
                .map(|repository| repository.name.clone())
                .collect()
        })
    }

    /// A refresh the user asked for is done, so the session passes the next one on.
    fn refresh_done(&self) {
        let done = self.refreshes_done.get() + 1;
        self.refreshes_done.set(done);
        self.done_copy.borrow_mut().set(&done);
    }

    /// The next message, from the backlog first, waiting up to `wait` for one.
    pub(super) fn next_message(&self, wait: Duration) -> Result<GitHubMessage, RecvTimeoutError> {
        let first = self.backlog.borrow_mut().pop_front();
        first.map_or_else(|| self.incoming.recv_timeout(wait), Ok)
    }

    /// The read `fetched` ended, so each demand for the same read that came while it waited or ran shares
    /// its outcome and leaves the queue. A demand that comes after this starts a read of its own.
    pub(super) fn merge(&self, fetched: &Fetched) {
        let mut backlog = self.backlog.borrow_mut();
        backlog.extend(self.incoming.try_iter());
        backlog
            .retain(|message| !matches!(message, GitHubMessage::Fetch(other) if other.is(fetched)));
    }

    /// Whether a search for `repository` that isn't `text` waits to run, so the answer to `text` is out of
    /// date and changes nothing (#465).
    pub(super) fn newer_search(&self, repository: &RepositoryName, text: &str) -> bool {
        let mut backlog = self.backlog.borrow_mut();
        backlog.extend(self.incoming.try_iter());
        backlog.iter().any(|message| {
            let GitHubMessage::Search {
                repository: waiting,
                text: asked,
            } = message
            else {
                return false;
            };
            waiting.is_same(repository) && asked != text
        })
    }

    /// Tells the window that the read of `fetched` failed for `why`. For a PR, it first tells the Files tab,
    /// which asks again once the window hears and finds why, and ends the Mirror worker's await of the head,
    /// which no read answers until the next.
    pub(super) fn read_failed(&self, board: &mut StatusBoard, fetched: &Fetched, why: ItemUnread) {
        let item = fetched.item();
        if let Fetched::PullRequest(_) = fetched {
            self.headless.note(item, NoHead::Unread(why));
            (self.fetching).no_head(item.repository.clone(), item.number);
        }
        board.failed_read(&failed(item, why.into()));
    }

    /// A read of the token found none, so each click that came while it was out is done with it, as a read
    /// at once would only find none again, or wait on `gh` as long. The session passes a click on while none
    /// it passed is left, so one can come during a read at the start or the status check's.
    fn clicks_covered(&self) {
        let mut backlog = self.backlog.borrow_mut();
        backlog.extend(self.incoming.try_iter());
        backlog.retain(|message| {
            let click = matches!(message, GitHubMessage::Refresh);
            if click {
                self.refresh_done();
            }
            !click
        });
    }

    /// Waits for the next read of the token, after one that found none: once the status check's interval
    /// has passed, or at once on the user's click. Meanwhile a check dropped here tells the one asking that
    /// GitHub can't be asked, and a read says why. The Repositories the window opens wait in `syncs` for a
    /// token, and the items on screen in `on_screen`, as the status bar shows each one's last sync. The
    /// session saves the marks and the rest that needs no GitHub, as it does with a token. Nothing, once the
    /// session stops the thread.
    pub(super) fn wait_for_token(
        &self,
        board: &mut StatusBoard,
        store: &Store,
        syncs: &mut Syncs,
    ) -> Option<TokenRead> {
        let due = Instant::now() + self.intervals.status;
        loop {
            match self.next_message(due.saturating_duration_since(Instant::now())) {
                Ok(GitHubMessage::Stop) | Err(RecvTimeoutError::Disconnected) => return None,
                Ok(GitHubMessage::Stopped(problem)) => {
                    board.stopped(problem);
                    return None;
                }
                Err(RecvTimeoutError::Timeout) => return Some(TokenRead::Due),
                Ok(GitHubMessage::Refresh) => return Some(TokenRead::Clicked),
                Ok(GitHubMessage::Fetch(fetched)) => {
                    self.read_failed(board, &fetched, ItemUnread::NoToken);
                }
                // The account goes on saying there is no token, with Try again (#1062).
                Ok(GitHubMessage::Problem(problem)) => board.problem(problem),
                Ok(GitHubMessage::Sync { repository }) => {
                    syncs.add(repository.clone());
                    show_last_sync(board, store, repository);
                }
                Ok(GitHubMessage::Screen(moved)) => {
                    self.on_screen.borrow_mut().moved(moved, Instant::now());
                }
                // Each dropped reply answers its caller that GitHub can't be asked. A search while there is
                // no token waits for the next one the user types: the list keeps the rows it showed (#465).
                Ok(
                    GitHubMessage::Check { .. }
                    | GitHubMessage::FetchUnstored { .. }
                    | GitHubMessage::Size { .. }
                    | GitHubMessage::Search { .. },
                ) => {}
            }
        }
    }

    /// Waits for the next refresh or the next step of a sync, answering what's asked meanwhile, so a step
    /// doesn't keep anyone waiting for longer than one request.
    pub(super) fn next_turn(&self, github: &GitHub, syncs: &mut Syncs, due: Due) -> Turn {
        let next_refresh = due.status.min(due.repository);
        loop {
            let hour_check = hour_check(github);
            let reread = self.on_screen.borrow().wake();
            let wake = [syncs.ready_at(), hour_check, reread]
                .into_iter()
                .flatten()
                .fold(next_refresh, Ord::min);
            match self.next_message(wake.saturating_duration_since(Instant::now())) {
                Ok(GitHubMessage::Sync { repository }) => {
                    syncs.add(repository.clone());
                    return Turn::Opened(repository);
                }
                Ok(GitHubMessage::Refresh) => return Turn::RefreshNow,
                Ok(GitHubMessage::Fetch(fetched)) => return Turn::Fetch(fetched),
                Ok(GitHubMessage::FetchUnstored { item, reply }) => {
                    return Turn::FetchUnstored(item, reply);
                }
                Ok(GitHubMessage::Search { repository, text }) => {
                    return Turn::Search { repository, text };
                }
                Ok(GitHubMessage::Problem(problem)) => return Turn::Problem(problem),
                Ok(GitHubMessage::Screen(moved)) => {
                    self.on_screen.borrow_mut().moved(moved, Instant::now());
                }
                Ok(GitHubMessage::Check { name, reply }) => {
                    let today = github.clock().now().date();
                    let asked = Asking::Check {
                        repository: name.clone(),
                    };
                    let checked = github.asking(asked, || github.check_repository(&name, today));
                    answer(&reply, checked.map(RepositoryCheck::new));
                    return Turn::Asked;
                }
                Ok(GitHubMessage::Size { repository, reply }) => {
                    let asked = Asking::Size {
                        repository: repository.clone(),
                    };
                    answer(
                        &reply,
                        github.asking(asked, || github.repository_size(&repository)),
                    );
                    return Turn::Asked;
                }
                Ok(GitHubMessage::Stop) | Err(RecvTimeoutError::Disconnected) => {
                    return Turn::Stop;
                }
                Ok(GitHubMessage::Stopped(problem)) => return Turn::Stopped(problem),
                Err(RecvTimeoutError::Timeout) => return self.timed_turn(due, hour_check),
            }
        }
    }

    /// The turn whose time came: asking who is signed in first, as only that brings the thread back online,
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

/// Sends a check's answer to the one who asked.
fn answer<T>(reply: &Sender<Result<T, Error>>, answered: Result<T, Error>) {
    #[expect(
        clippy::let_underscore_must_use,
        reason = "this fails only if the caller has gone, and it waits for this answer"
    )]
    let _ = reply.send(answered);
}

/// The longest the thread waits before it reads GitHub's clock again while the window shows the hour's
/// points. A Mac's `Instant` stops while it sleeps and the wall clock doesn't, so a wait for the reset alone
/// could leave the old hour's count up long after it.
pub(super) const HOUR_CHECK: Duration = Duration::from_secs(60);

/// The instant to see whether the hour's points have reset by GitHub's clock, while the window shows them,
/// or whether the rate limit's gate has opened, while it holds: at the reset or the gate's opening, or
/// [`HOUR_CHECK`] from now if that's sooner. The hour's count then ends, and the next hour's waits for
/// GitHub's next answer. The window hears that the gate opened.
pub(super) fn hour_check(github: &GitHub) -> Option<Instant> {
    let gate_left = github
        .points_back()
        .map(|at| Duration::try_from(at - OffsetDateTime::now_utc()).unwrap_or(Duration::ZERO));
    let left = github.hour_left().into_iter().chain(gate_left).min()?;
    Some(Instant::now() + left.min(HOUR_CHECK))
}

/// Runs the GitHub thread until the session stops it. Nothing else would show a panic in here: the last
/// status would stay up and look current, so this replaces it with a problem, and the account says Tsugiki
/// asks GitHub nothing more (#1070).
pub(super) fn run_thread(
    mut board: StatusBoard,
    store: &Store,
    access: GitHubAccess,
    refreshes: &Refreshes,
) {
    let ran = catch_unwind(AssertUnwindSafe(|| {
        watch(&mut board, store, access, refreshes)
    }));
    if ran.is_err() {
        board.stopped(stopped_checking());
    }
}

/// Connects, and while a read of the token finds none, answers what the thread is asked that GitHub can't
/// be asked, and reads the token again at the status check's interval and on the user's click. Gives the
/// client and the syncs of the Repositories the window opened meanwhile, or nothing once the session stops
/// the thread.
fn until_connected(
    board: &mut StatusBoard,
    store: &Store,
    access: GitHubAccess,
    refreshes: &Refreshes,
) -> Option<(GitHub, Syncs)> {
    let mut syncs = Syncs::new(refreshes.settings.clone());
    let mut connected = connect(board, store, access, refreshes);
    loop {
        match connected {
            Ok(github) => return Some((github, syncs)),
            Err(unconnected) => {
                let read = refreshes.wait_for_token(board, store, &mut syncs)?;
                connected = unconnected.connect(board, store, read);
                // The window still says each item on screen needs a token, so it reads them now.
                if connected.is_ok() {
                    refreshes.on_screen.borrow_mut().all_due(Instant::now());
                }
            }
        }
    }
}

/// Connects, then refreshes and runs the syncs the app asks for, until the session stops it. With no token
/// `board` shows why, and the thread reads it again as `until_connected` says.
pub(super) fn watch(
    board: &mut StatusBoard,
    store: &Store,
    access: GitHubAccess,
    refreshes: &Refreshes,
) {
    let Some((github, mut syncs)) = until_connected(board, store, access, refreshes) else {
        return;
    };
    board.hour_points(github.hour_points());
    board.points_back(github.points_back());
    let intervals = refreshes.intervals;
    // A visibility GitHub couldn't answer waits as long as the next status check, which shows it back.
    let mut writes = WriteJudge::new(intervals.status);
    writes.follow(
        ThreadParts {
            board,
            store,
            github: &github,
        },
        &refreshes.listed(),
    );
    let mut due = Due::starting_at(Instant::now(), intervals);
    loop {
        match refreshes.next_turn(&github, &mut syncs, due) {
            Turn::Stop => return,
            Turn::Stopped(problem) => {
                board.stopped(problem);
                return;
            }
            Turn::Refresh => {
                refresh(board, store, &github);
                due.status = Instant::now() + intervals.status;
            }
            Turn::RefreshRepository => {
                syncs.refresh_open(store);
                due.repository = Instant::now() + intervals.repository;
            }
            // Only asking who is signed in sets the status back online, so offline a click asks that first,
            // and the status bar says "Connecting…" until GitHub answers. While GitHub stays away, the syncs
            // keep waiting for the next status check. While the rate limit's gate holds, it asks nothing.
            Turn::RefreshNow => {
                let asked = (github.points_back().is_none() && board.asking_again())
                    .then(|| asked_who(store, &github, &board.status().account));
                let offline = asked
                    .as_ref()
                    .map_or_else(|| board.is_offline(), |asked| asked.account.is_offline());
                if asked.is_some() && !offline {
                    due.status = Instant::now() + intervals.status;
                }
                if offline {
                    syncs.put_open_first();
                } else {
                    syncs.refresh_now();
                }
                due.repository = Instant::now() + intervals.repository;
                // The session drops a click until this one is done, so it's done before the window hears the
                // answer: a click once the status says offline again asks again (#987).
                refreshes.refresh_done();
                if let Some(answer) = asked {
                    board.set(answer);
                }
            }
            Turn::Opened(repository) => show_last_sync(board, store, repository),
            Turn::Problem(problem) => board.problem(problem),
            // The board hears of it below, as after each turn.
            Turn::CheckHour | Turn::Asked => {}
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
                    syncs.after_fetch_failure(&problem, due.status);
                    tell_problem(board, problem, store);
                }
                // A failed read waits its minute too, or the thread would ask GitHub again at once.
                (refreshes.on_screen.borrow_mut()).read(fetched.item(), Instant::now());
            }
            Turn::FetchUnstored(item, reply) => {
                let read = Reading {
                    github: &github,
                    queue: refreshes,
                    syncs: &syncs,
                };
                let answer = fetch_unstored(board, store, &read, &item);
                if let Err(problem) = &answer {
                    syncs.after_fetch_failure(problem, due.status);
                }
                let answer = answer.unwrap_or_else(|problem| {
                    tell_problem(board, problem, store);
                    ItemFetch::Unfetched {
                        why: ItemUnfetched::Offline,
                    }
                });
                #[expect(
                    clippy::let_underscore_must_use,
                    reason = "this fails only if the caller has gone, and nobody then shows the answer"
                )]
                let _ = reply.send(answer);
            }
            // GitHub's answer to a search typed in the Inbox (#465).
            Turn::Search { repository, text } => searched(Searching {
                board: &mut *board,
                store,
                github: &github,
                queue: refreshes,
                syncs: &mut syncs,
                due: &due,
                request: SearchRequest { repository, text },
            }),
            // A Workbench lists nothing until Tsugiki knows who the user is, and no event tells the window
            // when it learns that. So a sync waits for it, and its first save then shows the rows.
            Turn::SyncStep if !matches!(store.last_login(), Ok(Some(_))) => {
                syncs.wait_until(due.status);
            }
            Turn::SyncStep => match syncs.step(&github, store) {
                Ok(progress) => tell_progress(board, &refreshes.fetching, &syncs, progress),
                // The next refresh shows whether GitHub is back, and the sync carries on from there.
                Err(failure) => {
                    let given_up = syncs.after_failure(&failure, due.status);
                    tell_progress(board, &refreshes.fetching, &syncs, given_up);
                    tell_problem(board, failure.problem, store);
                }
            },
        }
        board.sync_activity(syncs.activity());
        // Whichever turn asked GitHub, a sync, a read of what's on screen or the status, counts.
        board.hour_points(github.hour_points());
        board.points_back(github.points_back());
        writes.follow(
            ThreadParts {
                board,
                store,
                github: &github,
            },
            &refreshes.listed(),
        );
    }
}

/// A search typed in the Inbox's box, and the Repository it runs in (#465).
struct SearchRequest {
    repository: RepositoryName,
    text: String,
}

/// What an answer to a typed Inbox search needs (#465).
struct Searching<'a> {
    board: &'a mut StatusBoard,
    store: &'a Store,
    github: &'a GitHub,
    queue: &'a Refreshes,
    syncs: &'a mut Syncs,
    due: &'a Due,
    request: SearchRequest,
}

/// Runs the typed Inbox search `text` in `repository`, and tells the window of its answer or its problem. An
/// answer to a query the user has typed past changes nothing, and the newer query ran before it is saved.
fn searched(parts: Searching<'_>) {
    let Searching {
        board,
        store,
        github,
        queue,
        syncs,
        due,
        request,
    } = parts;
    match keep_search(github, store, queue, &request) {
        Ok(Some(change)) => board.changed(&change),
        Ok(None) => {}
        Err(problem) if problem.kind.speaks_for_all_of_github() => {
            syncs.after_fetch_failure(&problem, due.status);
            tell_problem(board, problem, store);
        }
        Err(problem) => log::warn!(
            "GitHub wouldn't answer the Inbox search in {}: {}",
            request.repository,
            problem.message
        ),
    }
}

/// GitHub's answer to the Inbox search `search` kept for the Inbox, and the change the window hears so it
/// reads the rows again (#465). Nothing when a newer query waits, whose answer alone counts.
fn keep_search(
    github: &GitHub,
    store: &Store,
    queue: &Refreshes,
    search: &SearchRequest,
) -> Result<Option<Change>, Error> {
    let asked = Asking::Search {
        repository: search.repository.clone(),
    };
    let answer = github.asking(asked, || {
        github.search_inbox(&search.repository, &search.text)
    })?;
    if queue.newer_search(&search.repository, &search.text) {
        return Ok(None);
    }
    store.save_inbox_search(&search.repository, &search.text, answer)?;
    Ok(Some(Change {
        repository: search.repository.clone(),
        items: Vec::new(),
    }))
}

/// Tells the window and the Mirror worker what the sync queue's `progress` says, if anything. A step's news
/// comes this way, and so does that of a step a given-up sync saved before its place failed to save.
fn tell_progress(
    board: &mut StatusBoard,
    fetching: &Fetching,
    syncs: &Syncs,
    progress: Option<Progress>,
) {
    match progress {
        Some(Progress::Changed(change)) => {
            if !change.items.is_empty() {
                hand_over_heads(fetching, syncs, &change);
            }
            board.changed(&change);
        }
        Some(Progress::Finished(last_sync)) => {
            board.repository_sync_changed(&last_sync.repository);
            board.last_sync(&last_sync);
        }
        Some(Progress::Recorded(repository)) => board.repository_sync_changed(&repository),
        None => {}
    }
}

/// Has the session hand the Mirror worker what the rules want of the Repository whose facts were `saved`,
/// then the heads saved, if the window opened it since launch. Only those Repositories sync.
fn hand_over_heads(fetching: &Fetching, syncs: &Syncs, saved: &Change) {
    if syncs.was_opened(&saved.repository) {
        fetching.facts_saved(saved.clone());
    }
}

/// An Issue's or a PR's content, as GitHub answered.
enum Content {
    Issue(IssueContent),
    /// A PR's, and GitHub's `updatedAt` for it at the read.
    PullRequest(PullRequestContent, OffsetDateTime),
}

/// What a read of a conversation asks: GitHub, the thread's queue, whose demands for the same read share
/// it, and the Mirror worker with the syncs, which say whose wants a PR's Conversation can change.
pub(super) struct Reading<'a> {
    pub(super) github: &'a GitHub,
    pub(super) queue: &'a Refreshes,
    pub(super) syncs: &'a Syncs,
}

/// Fetches `item`, which the database lacks, as the user asked (#981), and says what came of it: the kind it
/// saved, which it tells the window and the Mirror worker of, or why nothing came. A failure that speaks for
/// all of GitHub comes back for the caller to show. The window offers no fetch while offline or while the
/// rate limit's gate holds, and the session answers one that raced the status or the gate. One that raced
/// them here asks nothing, as a request then would only keep the user waiting for its failure.
fn fetch_unstored(
    board: &mut StatusBoard,
    store: &Store,
    read: &Reading<'_>,
    item: &Item,
) -> Result<ItemFetch, Error> {
    if board.is_offline() || read.github.points_back().is_some() {
        return Ok(ItemFetch::Unfetched {
            why: ItemUnfetched::Offline,
        });
    }
    Ok(match unstored::fetch(board, store, read.github, item)? {
        Ok(saved) => {
            hand_over_heads(&read.queue.fetching, read.syncs, &saved.change);
            board.changed(&saved.change);
            saved.kind
        }
        Err(why) => ItemFetch::Unfetched { why },
    })
}

/// Reads what an Issue or a PR says from GitHub, keeps it, and tells the window. The demands for the same
/// read that came before GitHub answered share its answer, its failure too.
///
/// GitHub numbers Issues and PRs from one sequence, so they share where they are kept. If the read fails,
/// the view goes on showing what was last read, and the window hears why, as do the Mirror worker and the
/// Files tab of a PR, which may wait for the head the Conversation names. An error returned here speaks
/// for all of GitHub, so the status bar and the syncs need to hear it. Any other trouble is this one item's,
/// and goes to the log.
pub(super) fn read_conversation(
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
            hand_over_head(read, item, &saved);
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
/// Review of the user's that covers another commit, changes what the rules want, so the session hands the
/// worker their wants again, once it takes the message this sends it.
fn hand_over_head(read: &Reading<'_>, item: &Item, saved: &ConversationSaved) {
    let fetching = &read.queue.fetching;
    let repository = &item.repository;
    let wants_again = (matches!(saved.head, HeadKept::Moved(_)) || saved.reviewed_moved)
        && read.syncs.was_opened(repository);
    let kept_head = matches!(saved.head, HeadKept::Moved(_) | HeadKept::Had(_));
    if wants_again {
        // The session hands over the wants again, then the head the facts now keep, if there is one.
        let items = kept_head.then_some(item.number).into_iter().collect();
        fetching.facts_saved(Change {
            repository: repository.clone(),
            items,
        });
    }
    match &saved.head {
        HeadKept::Moved(head) | HeadKept::Had(head) => {
            if !wants_again {
                fetching.heads_saved(repository, vec![head.clone()]);
            }
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
pub(super) enum Kept {
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
    let asked = Asking::Read {
        repository: item.repository.clone(),
        number: item.number,
    };
    let (noun, read) = github.asking(asked, || match fetched {
        Fetched::Issue(_) => {
            let read = github.issue_content(&item.repository, item.number);
            ("Issue", read.map(Content::Issue))
        }
        Fetched::PullRequest(_) => {
            let read = github.pull_request_content(&item.repository, item.number);
            let read = read.map(|read| Content::PullRequest(read.content, read.updated_at));
            ("PR", read)
        }
    });
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
            board.problem(problem);
            None
        }
    }
}

/// Tells the window when `repository` last finished a sync, so the status bar can say.
pub(super) fn show_last_sync(board: &mut StatusBoard, store: &Store, repository: RepositoryName) {
    match store.sync_state(&repository) {
        Ok(state) => board.last_sync(&LastSync {
            repository,
            at: state.map(|state| state.finished_at),
        }),
        Err(problem) => board.problem(problem),
    }
}

/// Asks GitHub again, so `board` follows the network going and coming back. While the rate limit's gate
/// holds, it asks nothing and the status stays as it is. A finished check sets the account and replaces the
/// problem beside it, or takes it away.
pub(super) fn refresh(board: &mut StatusBoard, store: &Store, github: &GitHub) {
    if github.points_back().is_none() {
        let asked = asked_who(store, github, &board.status().account);
        board.set(asked);
    }
}

/// Asks GitHub who is signed in, and says what the status bar says after its answer, from the `account` it
/// says now.
fn asked_who(store: &Store, github: &GitHub, account: &Account) -> Status {
    match github.asking(Asking::Status, || github.viewer()) {
        Ok(viewer) => {
            let problem = store.remember_login(&viewer.login).err();
            let account = Account::Online {
                login: viewer.login,
            };
            Status { account, problem }
        }
        Err(problem) => status_after(problem, store, account),
    }
}

/// Sets the status bar's words after a job's request failed. The rate limit leaves them as they are: the
/// gate holds every request until the reset, and the status bar says when the sync goes on. Only the
/// question of who is signed in shows GitHub's refusal, as it asks nothing while the gate holds. A job isn't
/// a status check, so a failure that only changes the account keeps the problem beside it.
pub(super) fn tell_problem(board: &mut StatusBoard, problem: Error, store: &Store) {
    if !matches!(problem.kind, ErrorKind::RateLimited { .. }) {
        let Status { account, problem } = status_after(problem, store, &board.status().account);
        let problem = problem.or_else(|| board.status().problem.clone());
        board.set(Status { account, problem });
    }
}

/// The status bar's words after GitHub didn't answer as it should, from the `account` it says now. Being
/// offline and the token's problems are the account's, and any other failure is a problem beside it. A
/// failure GitHub answered, a refused query included, also leaves who is signed in unconfirmed. A
/// certificate this computer's clock doesn't accept stops the request before GitHub answers, so it says
/// nothing of the account, which stays as it was before any click on Refresh asked again.
fn status_after(problem: Error, store: &Store, account: &Account) -> Status {
    match problem.kind {
        ErrorKind::Offline => match store.last_login() {
            Ok(Some(last_login)) => Status::from(Account::OfflineAs { last_login }),
            Ok(None) => Status::from(Account::Offline),
            Err(problem) => Status {
                account: Account::Offline,
                problem: Some(problem),
            },
        },
        ErrorKind::NoToken => Status::from(Account::NoToken {
            message: problem.message,
        }),
        ErrorKind::Unauthorized => Status::from(Account::Refused {
            message: problem.message,
        }),
        ErrorKind::CertificateDates => Status {
            account: account.before_asking_again(),
            problem: Some(problem),
        },
        ErrorKind::QueryRefused
        | ErrorKind::WriteRefused
        | ErrorKind::RateLimited { .. }
        | ErrorKind::NotFound
        | ErrorKind::Conflict
        | ErrorKind::Internal => Status {
            account: unconfirmed(account, store),
            problem: Some(problem),
        },
    }
}

/// The account after GitHub answered without saying who is signed in: it isn't offline, nor did it refuse
/// the token, but who is signed in waits for the next status check, so the status bar shows the last login,
/// if the store says, without "Connecting…". A login GitHub confirmed stays, and so does an account already
/// unconfirmed and a session that stopped.
fn unconfirmed(account: &Account, store: &Store) -> Account {
    match account {
        Account::Online { .. }
        | Account::Unconfirmed
        | Account::UnconfirmedAs { .. }
        | Account::Stopped => account.clone(),
        Account::Connecting
        | Account::ConnectingAs { .. }
        | Account::Offline
        | Account::OfflineAs { .. }
        | Account::Reconnecting
        | Account::ReconnectingAs { .. }
        | Account::NoToken { .. }
        | Account::Refused { .. } => match store.last_login() {
            Ok(Some(last_login)) => Account::UnconfirmedAs { last_login },
            Ok(None) | Err(_) => Account::Unconfirmed,
        },
    }
}

#[cfg(test)]
mod tests;
