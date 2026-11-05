//! A first sync: the user's own work and each Filter, found by search and fetched in batches, then the whole
//! tree of each Issue they reach (ADR 0011, ADR 0015). A Repository that has finished one refreshes instead,
//! which `refresh` handles. One `step` is one request, so the session can answer what else it's asked between
//! them. `github` does the asking, and this decides what to ask for, how much at a time and when to pause.

mod batches;
mod filters;
mod issues;
mod origins;
mod queue;
mod refresh;
mod searches;
mod trees;

pub use queue::{Progress, Syncs};
pub use searches::WORK_SEARCHES;

use std::collections::BTreeSet;

use time::OffsetDateTime;

use crate::error::Error;
use crate::filters::FilterQuery;
use crate::github::GitHub;
use crate::github::issues::ListedPr;
use crate::github::pull_requests::{NodeId, PrFetch, PrSearch, PullRequestFacts, RateLimit};
use crate::links::{possible_numbers, possible_origin};
use crate::repository::{RepositoryName, RepositorySettings};
use crate::status::LastSync;
use crate::store::{ChangeCheck, Store, SyncState};
use batches::fetch;
use filters::{FilterWalk, walk_filters};
use issues::fetch_issues;
use origins::fetch_origin;
use refresh::{Changed, Poll, Probing};
use searches::search;
use trees::read_cross_references;

/// Below this many points left, a sync waits for the reset, because the hourly 5,000 are also the user's
/// other tools'. We haven't measured what those tools need, and GitHub's `remaining` is approximate.
const RATE_LIMIT_FLOOR: u32 = 500;
/// The number of IDs one search page asks for (ADR 0015).
pub const SEARCH_PAGE: usize = 50;
/// The number of PRs one fetch asks for: fewer than a search's, because CI rollups are slow (ADR 0015).
pub const FETCH_BATCH: usize = 25;

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

/// The time to ask again, if the points left are below the floor.
fn pause_until(limit: RateLimit) -> Option<OffsetDateTime> {
    (limit.remaining < RATE_LIMIT_FLOOR).then_some(limit.reset_at)
}

#[cfg(test)]
mod recovery;
#[cfg(test)]
mod testing;
