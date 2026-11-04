//! A first sync: the user's own work and each Filter, found by search and fetched in batches, then the whole
//! tree of each Issue they reach (ADR 0011, ADR 0015). A Repository that has finished one refreshes instead,
//! which `refresh` handles. One `step` is one request, so the session can answer what else it's asked between
//! them. `github` does the asking, and this decides what to ask for, how much at a time and when to pause.

mod filters;
mod refresh;

use std::collections::{BTreeSet, VecDeque};
use std::time::{Duration, Instant};

use time::OffsetDateTime;

use crate::error::{Error, ErrorKind};
use crate::filters::FilterQuery;
use crate::github::GitHub;
use crate::github::issues::ListedPr;
use crate::github::pull_requests::{
    NodeId, PageRequest, PrFetch, PrSearch, PullRequestFacts, RateLimit,
};
use crate::links::{possible_numbers, possible_origin};
use crate::repository::{RepositoryName, RepositorySettings};
use crate::status::{Change, LastSync};
use crate::store::{ChangeCheck, Store, SyncState};
use filters::{FilterWalk, walk_filters};
use refresh::{Changed, Poll, Probing};

/// Below this many points left, a sync waits for the reset, because the hourly 5,000 are also the user's
/// other tools'. We haven't measured what those tools need, and GitHub's `remaining` is approximate.
const RATE_LIMIT_FLOOR: u32 = 500;
/// The number of IDs one search page asks for (ADR 0015).
pub const SEARCH_PAGE: usize = 50;
/// The number of PRs one fetch asks for: fewer than a search's, because CI rollups are slow (ADR 0015).
pub const FETCH_BATCH: usize = 25;

/// The user's own items in the order a first sync loads them (ADR 0015). Assigned Issues come with Issues
/// themselves, so these all find PRs. A team's Review request doesn't name the user, so it doesn't count.
pub const WORK_SEARCHES: [PrSearch; 4] = [
    PrSearch {
        operation: "SearchReviewRequests",
        qualifiers: "is:pr is:open user-review-requested:@me",
    },
    PrSearch {
        operation: "SearchReviewedPrs",
        qualifiers: "is:pr is:open reviewed-by:@me",
    },
    PrSearch {
        operation: "SearchOwnPrs",
        qualifiers: "is:pr is:open author:@me",
    },
    PrSearch {
        operation: "SearchAssignedPrs",
        qualifiers: "is:pr is:open assignee:@me",
    },
];

/// The rate limit resets every hour, so a longer pause can only come from a clock that's wrong.
const LONGEST_PAUSE: Duration = Duration::from_secs(3600);

/// A step can fail this many times in a row for any reason but the network or the rate limit, with one try
/// at each refresh. GitHub's own outages give the same errors as a Repository that has gone, so this lets one
/// pass before the sync gives way to the next. Asking for that Repository again, or restarting, tries again.
/// The timer also tries a Repository that has finished a sync, since a refresh costs a few points. A first
/// sync costs thousands, so it waits for the user.
const TRIES: u8 = 10;

/// The progress of one Repository's sync.
struct UserWorkSync {
    repository: RepositoryName,
    /// The search of each Filter, each once.
    filters: Vec<FilterQuery>,
    phase: Phase,
    /// The PRs this run has asked GitHub for, so it asks for each once.
    fetched: BTreeSet<u32>,
    /// When this sync began: what changes after this waits for the next one.
    started_at: OffsetDateTime,
    /// How the next refresh can learn that nothing changed, as far as this one knows.
    check: ChangeCheck,
}

enum Phase {
    /// The store hasn't said yet whether this Repository has finished a sync.
    Starting,
    /// A refresh asks whether anything moved.
    CheckingList(Poll),
    /// A refresh lists what moved.
    Polling(Poll),
    FetchingChanged(Changed),
    Probing(Probing),
    Searching {
        searches: &'static [PrSearch],
        cursor: Option<String>,
        found: Vec<NodeId>,
    },
    SearchingFilters(FilterWalk),
    Fetching(Batches),
    /// The Origin PRs named by the PRs just fetched. `leads.origins` holds only those still to ask for.
    FetchingOrigins {
        why: PrFetch,
        leads: Leads,
    },
    /// A PR shows as a Standalone PR until the batch with its Issue is saved.
    FetchingIssues {
        numbers: Vec<u32>,
        shown: Vec<u32>,
    },
    ReadingCrossReferences(Reading),
}

/// The PRs left to fetch by ID.
struct Batches {
    ids: Vec<NodeId>,
    why: PrFetch,
    leads: Leads,
}

/// Each shown Issue's cross-references. A tree fills in only when its PRs are saved, in the next phase, so an
/// Issue's rows appear batch by batch.
struct Reading {
    issues: Vec<u32>,
    cursor: Option<String>,
    listed: Vec<ListedPr>,
}

/// The numbers the PRs fetched so far point at.
#[derive(Default)]
struct Leads {
    /// The numbers they could name as Issues.
    named: BTreeSet<u32>,
    /// The numbers they could name as Origin PRs.
    origins: BTreeSet<u32>,
    /// The Issues a Filter found that Tsugiki has as they are now. Each still needs its tree read.
    held_issues: BTreeSet<u32>,
}

/// The result of one step.
enum Step {
    Searched {
        pause_until: Option<OffsetDateTime>,
    },
    /// The store changed, and a screen showing it needs to ask again. `touched` names the Issues and PRs.
    Saved {
        touched: Vec<u32>,
        pause_until: Option<OffsetDateTime>,
    },
    Finished,
}

impl Step {
    /// A refresh often finds its answers match what Tsugiki holds, and then no screen needs to ask again.
    fn saved_if_any(touched: Vec<u32>, pause_until: Option<OffsetDateTime>) -> Self {
        if touched.is_empty() {
            Self::Searched { pause_until }
        } else {
            Self::Saved {
                touched,
                pause_until,
            }
        }
    }
}

/// What a step of the queue did that the rest of the app can see.
#[derive(Debug, PartialEq, Eq)]
pub enum Progress {
    /// Stored facts changed.
    Changed(Change),
    /// The open Repository's sync finished.
    Finished(LastSync),
}

/// The clients and the Repository a sync's phases work with.
struct Link<'a> {
    github: &'a GitHub,
    store: &'a Store,
    repository: &'a RepositoryName,
    filters: &'a [FilterQuery],
}

/// A step's result, and the phase to move to.
type Moved = (Step, Option<Phase>);

const NO_PAUSE: Step = Step::Searched { pause_until: None };

impl UserWorkSync {
    fn new(repository: RepositorySettings) -> Self {
        let mut filters: Vec<FilterQuery> = Vec::new();
        for filter in repository.filters {
            if !filters.contains(&filter.query) {
                filters.push(filter.query);
            }
        }
        Self {
            repository: repository.name,
            filters,
            phase: Phase::Starting,
            fetched: BTreeSet::new(),
            started_at: OffsetDateTime::now_utc(),
            check: ChangeCheck::Skipped,
        }
    }

    /// Makes one request, or moves on to the next phase. A failure leaves the place it had, so the same step
    /// can run again.
    fn step(&mut self, github: &GitHub, store: &Store) -> Result<Step, Error> {
        let link = Link {
            github,
            store,
            repository: &self.repository,
            filters: &self.filters,
        };
        let fetched = &mut self.fetched;
        let (step, next) = match &mut self.phase {
            // Asking the store is no request to GitHub, so this goes straight on to the first one.
            Phase::Starting => {
                self.phase =
                    refresh::first_phase(&link, &mut self.check)?.unwrap_or(Phase::Searching {
                        searches: &WORK_SEARCHES,
                        cursor: None,
                        found: Vec::new(),
                    });
                return self.step(github, store);
            }
            Phase::CheckingList(poll) => refresh::check_list(&link, poll, &self.check)?,
            Phase::Polling(poll) => refresh::poll(&link, poll, &mut self.check)?,
            Phase::FetchingChanged(changed) => refresh::fetch_changed(&link, changed, fetched)?,
            Phase::Probing(probing) => refresh::probe(&link, probing)?,
            Phase::Searching {
                searches,
                cursor,
                found,
            } => search(&link, searches, cursor, found)?,
            Phase::SearchingFilters(walk) => walk_filters(&link, walk)?,
            Phase::Fetching(batches) => fetch(&link, batches, fetched)?,
            Phase::FetchingOrigins { why, leads } => fetch_origin(&link, *why, leads, fetched)?,
            Phase::FetchingIssues { numbers, shown } => fetch_issues(&link, numbers, shown)?,
            Phase::ReadingCrossReferences(reading) => {
                read_cross_references(&link, reading, fetched)?
            }
        };
        if let Some(next) = next {
            self.phase = next;
        }
        Ok(step)
    }

    /// Saves that this sync finished, and says when. The store keeps whole seconds, so the window hears what
    /// a later start reads back.
    fn finish(&self, store: &Store) -> Result<LastSync, Error> {
        let now = OffsetDateTime::now_utc();
        let finished_at = now.replace_nanosecond(0).unwrap_or(now);
        let state = SyncState {
            started_at: self.started_at,
            finished_at,
            check: self.check.clone(),
            refresh_under_way: false,
        };
        store.save_sync_state(&self.repository, state)?;
        Ok(LastSync {
            repository: self.repository.clone(),
            at: Some(finished_at),
        })
    }
}

fn search(
    link: &Link<'_>,
    searches: &mut &'static [PrSearch],
    cursor: &mut Option<String>,
    found: &mut Vec<NodeId>,
) -> Result<Moved, Error> {
    let Some((search, rest)) = searches.split_first() else {
        link.store
            .keep_only_filters(link.repository, link.filters.to_vec())?;
        let walk = FilterWalk::new(link.filters, std::mem::take(found));
        let next = Phase::SearchingFilters(walk);
        return Ok((NO_PAUSE, Some(next)));
    };
    let asked = PageRequest {
        size: SEARCH_PAGE,
        after: cursor.as_deref(),
    };
    let page = link
        .github
        .search_pull_requests(search, link.repository, asked)?;
    for id in page.ids {
        if !found.contains(&id) {
            found.push(id);
        }
    }
    *cursor = page.next;
    if cursor.is_none() {
        *searches = rest;
    }
    let pause_until = pause_until(page.rate_limit);
    Ok((Step::Searched { pause_until }, None))
}

fn fetch(
    link: &Link<'_>,
    batches: &mut Batches,
    fetched: &mut BTreeSet<u32>,
) -> Result<Moved, Error> {
    let Batches { ids, why, leads } = batches;
    if ids.is_empty() {
        leads.settle_origins(link, fetched)?;
        let next = Phase::FetchingOrigins {
            why: *why,
            leads: std::mem::take(leads),
        };
        return Ok((NO_PAUSE, Some(next)));
    }
    let batch = link.github.fetch_pull_requests(*why, ids, FETCH_BATCH)?;
    link.store
        .save_pull_requests(link.repository, batch.pull_requests.clone())?;
    for pull_request in &batch.pull_requests {
        leads.learn_from(pull_request, link.repository);
        fetched.extend([pull_request.number]);
    }
    let touched = batch.pull_requests.iter().map(|pr| pr.number).collect();
    drop(ids.drain(..batch.asked_for));
    let pause_until = pause_until(batch.rate_limit);
    let step = Step::Saved {
        touched,
        pause_until,
    };
    Ok((step, None))
}

impl Leads {
    fn learn_from(&mut self, pull_request: &PullRequestFacts, repository: &RepositoryName) {
        self.named
            .extend(possible_numbers(pull_request, repository));
        self.origins.extend(possible_origin(pull_request));
    }

    /// Leaves only the Origin PRs to ask for. A frozen one stays as it is stored, and the Issues it names
    /// still count, because a Backport can follow it there (ADR 0011).
    fn settle_origins(&mut self, link: &Link<'_>, fetched: &BTreeSet<u32>) -> Result<(), Error> {
        let frozen = link.store.frozen_pull_requests(link.repository)?;
        for origin in frozen.iter().filter(|pr| self.origins.contains(&pr.number)) {
            self.named.extend(possible_numbers(origin, link.repository));
        }
        let held = frozen.iter().map(|pr| pr.number);
        let held: BTreeSet<u32> = held.chain(fetched.iter().copied()).collect();
        self.origins.retain(|number| !held.contains(number));
        Ok(())
    }
}

/// The next Origin PR to ask for, one at a time.
fn fetch_origin(
    link: &Link<'_>,
    why: PrFetch,
    leads: &mut Leads,
    fetched: &mut BTreeSet<u32>,
) -> Result<Moved, Error> {
    let Some(number) = leads.origins.first().copied() else {
        let next = match why {
            PrFetch::Work => {
                let named = std::mem::take(&mut leads.named);
                let held = std::mem::take(&mut leads.held_issues);
                Some(Phase::FetchingIssues {
                    shown: held.difference(&named).copied().collect(),
                    numbers: named.into_iter().collect(),
                })
            }
            PrFetch::Tree | PrFetch::Changed => None,
        };
        let step = if next.is_some() {
            NO_PAUSE
        } else {
            Step::Finished
        };
        return Ok((step, next));
    };
    let answer = link
        .github
        .fetch_origin_pull_request(link.repository, number)?;
    let pause_until = pause_until(answer.rate_limit);
    let step = if let Some(pull_request) = answer.pull_request {
        let named = possible_numbers(&pull_request, link.repository);
        link.store
            .save_pull_requests(link.repository, vec![pull_request])?;
        leads.named.extend(named);
        Step::Saved {
            touched: vec![number],
            pause_until,
        }
    } else {
        Step::Searched { pause_until }
    };
    fetched.extend([number]);
    leads.origins.retain(|other| *other != number);
    Ok((step, None))
}

fn fetch_issues(
    link: &Link<'_>,
    numbers: &mut Vec<u32>,
    shown: &mut Vec<u32>,
) -> Result<Moved, Error> {
    if numbers.is_empty() {
        let next = Phase::ReadingCrossReferences(Reading {
            issues: std::mem::take(shown),
            cursor: None,
            listed: Vec::new(),
        });
        return Ok((NO_PAUSE, Some(next)));
    }
    let batch = link
        .github
        .fetch_issues(link.repository, numbers, FETCH_BATCH)?;
    let numbered = batch.issues.iter().map(|issue| issue.number);
    let found: Vec<u32> = numbered.collect();
    link.store.save_issues(link.repository, batch.issues)?;
    shown.extend(found.iter().copied());
    drop(numbers.drain(..batch.asked_for));
    let pause_until = pause_until(batch.rate_limit);
    let step = Step::Saved {
        touched: found,
        pause_until,
    };
    Ok((step, None))
}

fn read_cross_references(
    link: &Link<'_>,
    reading: &mut Reading,
    fetched: &BTreeSet<u32>,
) -> Result<Moved, Error> {
    let Reading {
        issues,
        cursor,
        listed,
    } = reading;
    let Some(issue) = issues.first().copied() else {
        let frozen = link.store.frozen_pull_requests(link.repository)?;
        let frozen: BTreeSet<u32> = frozen.iter().map(|pr| pr.number).collect();
        let next = Phase::Fetching(Batches {
            ids: not_yet_fetched(std::mem::take(listed), fetched, &frozen),
            why: PrFetch::Tree,
            leads: Leads::default(),
        });
        return Ok((NO_PAUSE, Some(next)));
    };
    let asked = PageRequest {
        size: SEARCH_PAGE,
        after: cursor.as_deref(),
    };
    let page = link
        .github
        .fetch_cross_references(link.repository, issue, asked)?;
    listed.extend(page.listed);
    *cursor = page.next;
    if cursor.is_none() {
        drop(issues.drain(..1));
    }
    let pause_until = pause_until(page.rate_limit);
    Ok((Step::Searched { pause_until }, None))
}

/// The IDs of the PRs in `listed` to fetch, each once. A PR Tsugiki keeps as merged or closed stays frozen,
/// and an open one is fetched again, because it may not be open any more (ADR 0011). The link rules read what
/// comes back when a screen asks, so a PR that only mentions an Issue stays out of its tree.
fn not_yet_fetched(
    listed: Vec<ListedPr>,
    fetched: &BTreeSet<u32>,
    frozen: &BTreeSet<u32>,
) -> Vec<NodeId> {
    let mut seen = BTreeSet::new();
    listed
        .into_iter()
        .filter(|pr| {
            !fetched.contains(&pr.number) && !frozen.contains(&pr.number) && seen.insert(pr.number)
        })
        .map(|pr| pr.id)
        .collect()
}

/// Why the next step waits, and until when.
#[derive(Clone, Copy)]
enum Hold {
    /// Until the next look at GitHub after a failure. The user can ask sooner.
    Retry(Instant),
    /// Until GitHub's points come back, which asking sooner would only waste.
    Points(Instant),
}

impl Hold {
    fn until(self) -> Instant {
        match self {
            Self::Retry(at) | Self::Points(at) => at,
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
            } => Hold::Points(
                OffsetDateTime::from_unix_timestamp(i64::from(resets_at))
                    .map_or(next_refresh, |at| until_instant(at).max(next_refresh)),
            ),
            ErrorKind::RateLimited { resets_at: None } => Hold::Points(next_refresh),
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
            Step::Searched { pause_until } => {
                self.not_before = pause_until.map(until_instant).map(Hold::Points);
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
                self.not_before = pause_until.map(until_instant).map(Hold::Points);
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

/// The time to ask again, if the points left are below the floor.
fn pause_until(limit: RateLimit) -> Option<OffsetDateTime> {
    (limit.remaining < RATE_LIMIT_FLOOR).then_some(limit.reset_at)
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
