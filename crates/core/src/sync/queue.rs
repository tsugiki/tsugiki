//! The queue of syncs: which runs next, when it can run and when it gives up (ADR 0015).

use std::collections::{BTreeSet, VecDeque};
use std::time::{Duration, Instant};

use time::OffsetDateTime;

use super::{Step, UserWorkSync};
use crate::error::{Error, ErrorKind};
use crate::github::GitHub;
use crate::github::clock::GitHubClock;
use crate::repository::{RepositoryName, RepositorySettings};
use crate::status::{Change, LastSync, SyncActivity, SyncPart};
use crate::store::Store;

/// The rate limit resets every hour, so a longer pause can only come from a clock that's wrong.
const LONGEST_PAUSE: Duration = Duration::from_secs(3600);

/// A step can fail this many times in a row for any reason but the network or the rate limit, with one try
/// at each refresh. GitHub's own outages give the same errors as a Repository that has gone, so this lets one
/// pass before the sync gives way to the next. Asking for that Repository again, or restarting, tries again.
/// The timer also tries a Repository that has finished a sync, since a refresh costs a few points. A first
/// sync costs thousands, so it waits for the user.
const TRIES: u8 = 10;

/// What a step of the queue did that the rest of the app can see.
#[derive(Debug, PartialEq, Eq)]
pub enum Progress {
    /// Stored facts changed.
    Changed(Change),
    /// The open Repository's sync finished.
    Finished(LastSync),
}

/// Why the next step waits, and until when.
#[derive(Clone, Copy)]
enum Hold {
    /// Until the next look at GitHub after a failure. The user can ask sooner.
    Retry(Instant),
    /// Until GitHub's points come back, which asking sooner would only waste. The status bar shows the time.
    Points(Instant, OffsetDateTime),
}

impl Hold {
    fn until(self) -> Instant {
        match self {
            Self::Retry(at) | Self::Points(at, _) => at,
        }
    }
}

/// The read of the whole tree of an Issue the user opened.
struct OpenedTree {
    number: u32,
    sync: UserWorkSync,
}

/// The syncs still to run, one at a time. The one asked for last goes first, because the window has that
/// Repository open (ADR 0015), and the others keep their progress behind it.
#[derive(Default)]
pub struct Syncs {
    queue: VecDeque<UserWorkSync>,
    /// The trees of Issues the user opened with no first sync's queue to put them in. Each goes ahead of
    /// `queue` for its few requests, and leaves no trace of a sync behind it.
    trees: VecDeque<OpenedTree>,
    /// The Issues of `open` the user has opened since it opened. Each reads its tree once, because a view on
    /// screen asks about once a minute.
    opened: BTreeSet<u32>,
    not_before: Option<Hold>,
    failures: u8,
    /// The Repository the window has open, which alone keeps refreshing.
    open: Option<RepositorySettings>,
}

impl Syncs {
    /// Puts a sync of `repository` first, carrying on from where it got if one is already queued.
    pub fn add(&mut self, repository: RepositorySettings) {
        if !self.is_open(&repository.name) {
            self.opened.clear();
            self.trees.clear();
        }
        self.open = Some(repository.clone());
        let queued = self
            .queue
            .iter()
            .position(|sync| sync.repository.is_same(&repository.name))
            .and_then(|at| self.queue.remove(at));
        self.queue
            .push_front(queued.unwrap_or_else(|| UserWorkSync::new(repository)));
        self.failures = 0;
    }

    /// Queues the timer's refresh of the open Repository, unless a sync of it is under way or waiting. A
    /// store that can't answer queues nothing, because the sync couldn't save, and the status says why.
    pub fn refresh_open(&mut self, store: &Store) {
        let Some(open) = &self.open else {
            return;
        };
        let has_synced = matches!(store.sync_state(&open.name), Ok(Some(_)));
        let queued = self
            .queue
            .iter()
            .any(|sync| sync.repository.is_same(&open.name));
        if has_synced && !queued {
            self.queue.push_back(UserWorkSync::new(open.clone()));
        }
    }

    /// Answers the user's click: the open Repository goes first, and stops waiting for the next look at
    /// GitHub. A wait for GitHub's points stays.
    pub fn refresh_now(&mut self) {
        if let Some(open) = self.open.clone() {
            self.add(open);
        }
        if let Some(Hold::Retry(_)) = self.not_before {
            self.not_before = None;
        }
    }

    /// The work going on now, for the status bar.
    pub fn activity(&self) -> Option<SyncActivity> {
        let (sync, part) = match self.trees.front() {
            Some(tree) => (&tree.sync, SyncPart::Trees),
            None => {
                let sync = self.queue.front()?;
                (sync, sync.part())
            }
        };
        let paused_until = match self.not_before {
            Some(Hold::Points(_, until)) => Some(until),
            Some(Hold::Retry(_)) | None => None,
        };
        Some(SyncActivity {
            repository: sync.repository.clone(),
            part,
            pages: sync
                .first_sync
                .as_ref()
                .map_or(0, |first_sync| first_sync.pages),
            load: sync.load_progress(),
            paused_until,
        })
    }

    /// Tells the syncs the user opened Issue `number` of the open Repository. A first sync's queue moves its
    /// tree up. With no queue the tree gets its own read, because the PRs older than the window aren't in
    /// yet.
    pub fn open_issue(&mut self, repository: &RepositoryName, number: u32) {
        if !self.is_open(repository) || !self.opened.insert(number) {
            return;
        }
        let queued = self
            .queue
            .iter_mut()
            .find(|sync| sync.repository.is_same(repository))
            .is_some_and(|sync| sync.open_issue(number));
        if let (false, Some(open)) = (queued, &self.open) {
            self.trees.push_back(OpenedTree {
                number,
                sync: UserWorkSync::tree_of(open.clone(), number),
            });
        }
    }

    fn is_open(&self, repository: &RepositoryName) -> bool {
        self.open
            .as_ref()
            .is_some_and(|open| open.name.is_same(repository))
    }

    /// The time the next step can run, if there is one.
    pub fn ready_at(&self) -> Option<Instant> {
        self.trees
            .front()
            .map(|tree| &tree.sync)
            .or_else(|| self.queue.front())
            .map(|_| self.not_before.map_or_else(Instant::now, Hold::until))
    }

    /// Holds the next step back until `at`.
    pub fn wait_until(&mut self, at: Instant) {
        self.not_before = Some(Hold::Retry(at));
    }

    /// Holds the next step back after `problem`, at least until `next_refresh`, and stops a sync that keeps
    /// failing from blocking the ones behind it.
    /// GitHub's `clock` turns a rate limit's reset into a wait.
    pub fn after_failure(&mut self, problem: &Error, next_refresh: Instant, clock: GitHubClock) {
        self.not_before = Some(match problem.kind {
            ErrorKind::RateLimited {
                resets_at: Some(resets_at),
            } => points_hold(
                OffsetDateTime::from_unix_timestamp(i64::from(resets_at))
                    .map_or(next_refresh, |at| {
                        until_instant(clock, at).max(next_refresh)
                    }),
            ),
            ErrorKind::RateLimited { resets_at: None } => points_hold(next_refresh),
            // Fixing the clock is up to the user, and the next try after it goes through.
            ErrorKind::Offline | ErrorKind::CertificateDates => Hold::Retry(next_refresh),
            ErrorKind::Unauthorized
            | ErrorKind::WriteRefused
            | ErrorKind::NotFound
            | ErrorKind::Conflict
            | ErrorKind::QueryRefused
            | ErrorKind::Internal => {
                self.count_failure(problem);
                Hold::Retry(next_refresh)
            }
        });
    }

    fn count_failure(&mut self, problem: &Error) {
        self.failures += 1;
        if self.failures >= TRIES {
            self.failures = 0;
            match self.trees.pop_front() {
                Some(tree) => self.give_up_on(&tree, problem),
                None => {
                    if let Some(sync) = self.queue.pop_front() {
                        log::error!(
                            "Tsugiki gave up syncing {}: {}",
                            sync.repository,
                            problem.message
                        );
                    }
                }
            }
        }
    }

    /// Forgets that `tree`'s Issue was opened, so opening it asks again. A view on screen does that about
    /// once a minute.
    fn give_up_on(&mut self, tree: &OpenedTree, problem: &Error) {
        let number = tree.number;
        self.opened.retain(|opened| *opened != number);
        log::warn!(
            "Tsugiki couldn't read the tree of Issue #{number}: {}",
            problem.message
        );
    }

    /// Runs one step of the first sync in the queue. Answers with what changed for the rest of the app, if
    /// anything did.
    pub fn step(&mut self, github: &GitHub, store: &Store) -> Result<Option<Progress>, Error> {
        let reading_a_tree = !self.trees.is_empty();
        let tree = self.trees.front_mut().map(|tree| &mut tree.sync);
        let Some(sync) = tree.or_else(|| self.queue.front_mut()) else {
            return Ok(None);
        };
        self.not_before = None;
        let step = match sync.step(github, store) {
            // One Issue's trouble mustn't stand in the way of the sync behind it.
            Err(problem) if reading_a_tree && !problem.kind.speaks_for_all_of_github() => {
                if let Some(tree) = self.trees.pop_front() {
                    self.give_up_on(&tree, &problem);
                }
                return Ok(None);
            }
            step => step?,
        };
        self.failures = 0;
        match step {
            Step::Moved => Ok(None),
            Step::Searched { pause_until } => {
                self.not_before = pause_until
                    .map(|at| until_instant(github.clock(), at))
                    .map(points_hold);
                Ok(None)
            }
            Step::Saved {
                touched,
                pause_until,
            } => {
                let change = Change {
                    repository: sync.repository.clone(),
                    items: touched,
                };
                self.not_before = pause_until
                    .map(|at| until_instant(github.clock(), at))
                    .map(points_hold);
                Ok(Some(Progress::Changed(change)))
            }
            // A tree isn't a whole sync, so it doesn't move the time of the last one.
            Step::Finished if reading_a_tree => {
                drop(self.trees.pop_front());
                Ok(None)
            }
            Step::Finished => {
                let finished = sync.finish(store)?;
                let is_open = self.is_open(&finished.repository);
                drop(self.queue.pop_front());
                Ok(is_open.then_some(Progress::Finished(finished)))
            }
        }
    }
}

/// A wait for GitHub's points until `at`.
fn points_hold(at: Instant) -> Hold {
    let wait = at.saturating_duration_since(Instant::now());
    Hold::Points(at, OffsetDateTime::now_utc() + wait)
}

/// The local instant GitHub's `clock` reaches `until`.
fn until_instant(clock: GitHubClock, until: OffsetDateTime) -> Instant {
    Instant::now() + pause_for(until, clock.now())
}

/// The wait from `now` to `until`, which comes from GitHub's clock.
fn pause_for(until: OffsetDateTime, now: OffsetDateTime) -> Duration {
    Duration::try_from(until - now)
        .unwrap_or(Duration::ZERO)
        .min(LONGEST_PAUSE)
}

#[cfg(test)]
mod tests;
