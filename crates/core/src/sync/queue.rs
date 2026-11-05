//! The queue of syncs: which runs next, when it can run and when it gives up (ADR 0015).

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use time::OffsetDateTime;

use super::{Step, UserWorkSync};
use crate::error::{Error, ErrorKind};
use crate::github::GitHub;
use crate::repository::RepositorySettings;
use crate::status::{Change, LastSync, SyncActivity};
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

/// The syncs still to run, one at a time. The one asked for last goes first, because the window has that
/// Repository open (ADR 0015), and the others keep their progress behind it.
#[derive(Default)]
pub struct Syncs {
    queue: VecDeque<UserWorkSync>,
    not_before: Option<Hold>,
    failures: u8,
    /// The Repository the window has open, which alone keeps refreshing.
    open: Option<RepositorySettings>,
}

impl Syncs {
    /// Puts a sync of `repository` first, carrying on from where it got if one is already queued.
    pub fn add(&mut self, repository: RepositorySettings) {
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

    /// The work of the sync at the front, for the status bar.
    pub fn activity(&self) -> Option<SyncActivity> {
        let sync = self.queue.front()?;
        let paused_until = match self.not_before {
            Some(Hold::Points(_, until)) => Some(until),
            Some(Hold::Retry(_)) | None => None,
        };
        Some(SyncActivity {
            repository: sync.repository.clone(),
            part: sync.part(),
            pages: sync
                .first_sync
                .as_ref()
                .map_or(0, |first_sync| first_sync.pages),
            paused_until,
        })
    }

    /// The time the next step can run, if there is one.
    pub fn ready_at(&self) -> Option<Instant> {
        self.queue
            .front()
            .map(|_| self.not_before.map_or_else(Instant::now, Hold::until))
    }

    /// Holds the next step back until `at`.
    pub fn wait_until(&mut self, at: Instant) {
        self.not_before = Some(Hold::Retry(at));
    }

    /// Holds the next step back after `problem`, at least until `next_refresh`, and stops a sync that keeps
    /// failing from blocking the ones behind it.
    pub fn after_failure(&mut self, problem: &Error, next_refresh: Instant) {
        self.not_before = Some(match problem.kind {
            ErrorKind::RateLimited {
                resets_at: Some(resets_at),
            } => points_hold(
                OffsetDateTime::from_unix_timestamp(i64::from(resets_at))
                    .map_or(next_refresh, |at| until_instant(at).max(next_refresh)),
            ),
            ErrorKind::RateLimited { resets_at: None } => points_hold(next_refresh),
            ErrorKind::Offline => Hold::Retry(next_refresh),
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
            if let Some(sync) = self.queue.pop_front() {
                log::error!(
                    "Tsugiki gave up syncing {}: {}",
                    sync.repository,
                    problem.message
                );
            }
        }
    }

    /// Runs one step of the first sync in the queue. Answers with what changed for the rest of the app, if
    /// anything did.
    pub fn step(&mut self, github: &GitHub, store: &Store) -> Result<Option<Progress>, Error> {
        let Some(sync) = self.queue.front_mut() else {
            return Ok(None);
        };
        self.not_before = None;
        let step = sync.step(github, store)?;
        self.failures = 0;
        match step {
            Step::Moved => Ok(None),
            Step::Searched { pause_until } => {
                self.not_before = pause_until.map(until_instant).map(points_hold);
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
                self.not_before = pause_until.map(until_instant).map(points_hold);
                Ok(Some(Progress::Changed(change)))
            }
            Step::Finished => {
                let finished = sync.finish(store)?;
                let is_open = self
                    .open
                    .as_ref()
                    .is_some_and(|open| open.name.is_same(&finished.repository));
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

fn until_instant(until: OffsetDateTime) -> Instant {
    Instant::now() + pause_for(until, OffsetDateTime::now_utc())
}

/// The wait from `now` to `until`, which comes from GitHub's clock.
fn pause_for(until: OffsetDateTime, now: OffsetDateTime) -> Duration {
    Duration::try_from(until - now)
        .unwrap_or(Duration::ZERO)
        .min(LONGEST_PAUSE)
}

#[cfg(test)]
mod tests;
