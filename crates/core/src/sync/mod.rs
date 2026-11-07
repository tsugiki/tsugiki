//! A first sync: the user's own work and each Filter, found by search and fetched in batches, then the trees
//! of the Issues they reach, then the rest of the window as one queue (ADR 0011, ADR 0015). It saves its
//! place after each step, so it comes back to the next page. A Repository that has finished one refreshes
//! instead, which `refresh` handles. One `step` is one request, so the session can answer what else it's
//! asked between them. `github` does the asking, and this decides what to ask for, how much at a time and
//! when to pause.

mod batches;
mod filters;
mod issues;
mod loading;
mod origins;
mod queue;
mod refresh;
mod searches;
mod trees;

pub use queue::{Progress, Syncs};
pub use searches::WORK_SEARCHES;

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};

use crate::error::{Error, ErrorKind};
use crate::filters::FilterQuery;
use crate::github::issues::ListedPr;
use crate::github::pull_requests::{NodeId, PrFetch, PullRequestFacts, RateLimit};
use crate::github::{GitHub, Login};
use crate::links::{possible_numbers, possible_origin};
use crate::repository::{RepositoryName, RepositorySettings, WINDOW_DAYS};
use crate::status::{LastSync, LoadProgress, SyncPart};
use crate::store::{ChangeCheck, FirstSyncPlace, Store, SyncState};
use batches::fetch;
use filters::{FilterWalk, walk_filters};
use issues::{fetch_issues, fetch_named_issues};
use loading::{Loading, load};
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
    /// The Repository's settings as the sync started, which say which PRs the user's work reaches.
    settings: RepositorySettings,
    /// The search of each Filter, each once.
    filters: Vec<FilterQuery>,
    phase: Phase,
    /// The PRs this run has asked GitHub for, so it asks for each once.
    fetched: BTreeSet<u32>,
    /// The time this sync began by GitHub's clock: what changes after this waits for the next one. Its first
    /// step sets it, or a saved first sync's place.
    started_at: OffsetDateTime,
    /// How the next refresh can learn that nothing changed, as far as this one knows.
    check: ChangeCheck,
    /// A first sync's own state. A refresh has none, because it keeps no place to come back to.
    first_sync: Option<FirstSync>,
    /// A step that worked and whose place couldn't be saved, which the next try reports once it can.
    unsaved: Option<Step>,
    /// The PRs a refresh found changed, whose Conversations it reads before it finishes if the user's work
    /// reaches them. A first sync finds none.
    changed_prs: BTreeSet<u32>,
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
    Resumable(Resumable),
    /// A refresh that would finish reads the Conversations of the changed PRs the user's work reaches,
    /// lowest number first (#157).
    ReadingAhead(BTreeSet<u32>),
}

/// The phases a first sync goes through, and so the ones it can stop in and come back to (ADR 0015). A
/// refresh passes through some of them too, but it redoes itself from the store instead.
#[derive(Serialize, Deserialize)]
enum Resumable {
    Searching {
        /// The first of `WORK_SEARCHES` still to read.
        next_search: usize,
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
    /// A first sync's window and trees, as one queue.
    Loading(Loading),
    ReadingCrossReferences(Reading),
    /// The Issues a tree's PRs name that Tsugiki lacks.
    FetchingNamedIssues(Vec<u32>),
}

impl From<Resumable> for Phase {
    fn from(place: Resumable) -> Self {
        Self::Resumable(place)
    }
}

/// The facts a first sync's place depends on. If one has changed the sync starts over: `@me` finds another
/// user's work, a Filter's searches have moved on, or another version may read the same place differently.
#[derive(PartialEq, Eq, Serialize, Deserialize)]
struct PlaceOwner {
    version: String,
    login: Option<Login>,
    filters: Vec<FilterQuery>,
}

impl PlaceOwner {
    fn now(store: &Store, filters: &[FilterQuery]) -> Result<Self, Error> {
        Ok(Self {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            login: store.last_login()?,
            filters: filters.to_vec(),
        })
    }
}

/// A first sync's own state, which a refresh doesn't have.
struct FirstSync {
    owner: PlaceOwner,
    /// The pages it has read.
    pages: u32,
}

/// A first sync's place, as the store keeps it.
#[derive(Serialize)]
struct PlaceToSave<'a> {
    owner: &'a PlaceOwner,
    place: &'a Resumable,
    fetched: &'a BTreeSet<u32>,
    started_at: OffsetDateTime,
    pages: u32,
}

/// `PlaceToSave` as it reads back.
#[derive(Deserialize)]
struct SavedPlace {
    owner: PlaceOwner,
    place: Resumable,
    fetched: BTreeSet<u32>,
    started_at: OffsetDateTime,
    pages: u32,
}

fn place_problem(problem: serde_json::Error) -> Error {
    Error::new(
        ErrorKind::Internal,
        &format!("Tsugiki can't read where its first sync got to: {problem}"),
    )
}

/// The place this Repository's first sync got to, if it stopped partway and that place can still be read.
fn saved_place(store: &Store, repository: &RepositoryName) -> Result<Option<SavedPlace>, Error> {
    let Some(place) = store.first_sync_place(repository)? else {
        return Ok(None);
    };
    match serde_json::from_str(place.as_str()).map_err(place_problem) {
        Ok(saved) => Ok(Some(saved)),
        Err(problem) => {
            log::warn!(
                "{repository} starts its first sync over: {}",
                problem.message
            );
            Ok(None)
        }
    }
}

/// The PRs left to fetch by ID.
#[derive(Serialize, Deserialize)]
struct Batches {
    ids: Vec<NodeId>,
    why: PrFetch,
    leads: Leads,
}

/// Each shown Issue's cross-references. A tree fills in only when its PRs are saved, in the next phase, so an
/// Issue's rows appear batch by batch.
#[derive(Default, Serialize, Deserialize)]
struct Reading {
    issues: Vec<u32>,
    cursor: Option<String>,
    listed: Vec<ListedPr>,
}

/// The numbers the PRs fetched so far point at.
#[derive(Default, Serialize, Deserialize)]
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
    /// It only moved on to the next phase, and asked GitHub for nothing.
    Moved,
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
    /// The start of a first sync's window. A refresh has no window.
    window_since: Option<OffsetDateTime>,
}

/// A step's result, and the phase to move to.
type Moved = (Step, Option<Phase>);

impl UserWorkSync {
    fn new(repository: RepositorySettings) -> Self {
        let mut filters: Vec<FilterQuery> = Vec::new();
        for filter in &repository.filters {
            if !filters.contains(&filter.query) {
                filters.push(filter.query.clone());
            }
        }
        Self {
            settings: repository,
            filters,
            phase: Phase::Starting,
            fetched: BTreeSet::new(),
            started_at: OffsetDateTime::now_utc(),
            check: ChangeCheck::Skipped,
            first_sync: None,
            unsaved: None,
            changed_prs: BTreeSet::new(),
        }
    }

    /// A read of the whole tree of Issue `number`, and nothing else.
    fn tree_of(repository: RepositorySettings, number: u32) -> Self {
        let mut sync = Self::new(repository);
        sync.phase = Resumable::ReadingCrossReferences(Reading {
            issues: vec![number],
            ..Reading::default()
        })
        .into();
        sync
    }

    /// Runs one step, then saves a first sync's place, so quitting, a crash or going offline comes back to
    /// the next page. A save that fails keeps the step, whose news still counts, for the next try to report.
    fn step(&mut self, github: &GitHub, store: &Store) -> Result<Step, Error> {
        let step = match self.unsaved.take() {
            Some(step) => step,
            None => self.advance(github, store)?,
        };
        match self.save_place(store) {
            Ok(()) => Ok(step),
            Err(problem) => {
                self.unsaved = Some(step);
                Err(problem)
            }
        }
    }

    fn save_place(&self, store: &Store) -> Result<(), Error> {
        let (Some(first_sync), Phase::Resumable(place)) = (&self.first_sync, &self.phase) else {
            return Ok(());
        };
        let saved = PlaceToSave {
            owner: &first_sync.owner,
            place,
            fetched: &self.fetched,
            started_at: self.started_at,
            pages: first_sync.pages,
        };
        let text = serde_json::to_string(&saved).map_err(place_problem)?;
        store.save_first_sync_place(&self.settings.name, FirstSyncPlace::new(text))
    }

    /// Takes up a first sync where it stopped, or starts one.
    fn start_first_sync(&mut self, store: &Store) -> Result<(), Error> {
        let owner = PlaceOwner::now(store, &self.filters)?;
        let saved = saved_place(store, &self.settings.name)?;
        let Some(saved) = saved.filter(|saved| saved.owner == owner) else {
            self.first_sync = Some(FirstSync { owner, pages: 0 });
            self.phase = Resumable::Searching {
                next_search: 0,
                cursor: None,
                found: Vec::new(),
            }
            .into();
            return Ok(());
        };
        self.first_sync = Some(FirstSync {
            owner,
            pages: saved.pages,
        });
        self.phase = saved.place.into();
        self.fetched = saved.fetched;
        self.started_at = saved.started_at;
        Ok(())
    }

    /// The part of a sync this is, for the status bar.
    fn part(&self) -> SyncPart {
        match (&self.phase, &self.first_sync) {
            (Phase::Resumable(Resumable::Loading(loading)), Some(_)) => {
                if loading.trees_come_first() {
                    SyncPart::Trees
                } else if loading.reads_older_open_items() {
                    SyncPart::OlderOpen
                } else {
                    SyncPart::Window
                }
            }
            (_, Some(_)) => SyncPart::YourWork,
            (_, None) => SyncPart::Refresh,
        }
    }

    /// The progress of a first sync's queue, for the status bar.
    fn load_progress(&self) -> Option<LoadProgress> {
        match &self.phase {
            Phase::Resumable(Resumable::Loading(loading)) => Some(loading.progress(&self.fetched)),
            Phase::Starting
            | Phase::CheckingList(_)
            | Phase::Polling(_)
            | Phase::FetchingChanged(_)
            | Phase::Probing(_)
            | Phase::Resumable(_)
            | Phase::ReadingAhead(_) => None,
        }
    }

    /// Puts Issue `number`'s tree first, if this sync is at the point of loading trees, and says if it was.
    fn open_issue(&mut self, number: u32) -> bool {
        if let Phase::Resumable(Resumable::Loading(loading)) = &mut self.phase {
            loading.open_issue(number);
            true
        } else {
            false
        }
    }

    /// Makes one request, or moves on to the next phase. A failure leaves the place it had, so the same step
    /// can run again.
    fn advance(&mut self, github: &GitHub, store: &Store) -> Result<Step, Error> {
        let link = Link {
            github,
            store,
            repository: &self.settings.name,
            filters: &self.filters,
            window_since: self
                .first_sync
                .as_ref()
                .map(|_| self.started_at - Duration::days(WINDOW_DAYS.into())),
        };
        let fetched = &mut self.fetched;
        let changed_prs = &mut self.changed_prs;
        let (step, next) = match &mut self.phase {
            // Asking the store is no request to GitHub, so this goes straight on to the first one.
            Phase::Starting => {
                self.started_at = github.clock().now();
                match refresh::first_phase(&link, &mut self.check)? {
                    Some(phase) => self.phase = phase,
                    None => self.start_first_sync(store)?,
                }
                return self.advance(github, store);
            }
            Phase::CheckingList(poll) => refresh::check_list(&link, poll, &self.check)?,
            Phase::Polling(poll) => refresh::poll(&link, poll, &mut self.check)?,
            Phase::FetchingChanged(changed) => {
                refresh::fetch_changed(&link, changed, fetched, changed_prs)?
            }
            Phase::Probing(probing) => refresh::probe(&link, probing, changed_prs)?,
            Phase::ReadingAhead(numbers) => refresh::read_ahead(&link, numbers)?,
            Phase::Resumable(Resumable::Searching {
                next_search,
                cursor,
                found,
            }) => search(&link, next_search, cursor, found)?,
            Phase::Resumable(Resumable::SearchingFilters(walk)) => walk_filters(&link, walk)?,
            Phase::Resumable(Resumable::Fetching(batches)) => fetch(&link, batches, fetched)?,
            Phase::Resumable(Resumable::FetchingOrigins { why, leads }) => {
                fetch_origin(&link, *why, leads, fetched)?
            }
            Phase::Resumable(Resumable::FetchingIssues { numbers, shown }) => {
                fetch_issues(&link, numbers, shown)?
            }
            Phase::Resumable(Resumable::Loading(loading)) => load(&link, loading, fetched)?,
            Phase::Resumable(Resumable::ReadingCrossReferences(reading)) => {
                read_cross_references(&link, reading, fetched)?
            }
            Phase::Resumable(Resumable::FetchingNamedIssues(numbers)) => {
                fetch_named_issues(&link, numbers)?
            }
        };
        if let Some(next) = next {
            self.phase = next;
        }
        if matches!(step, Step::Finished) && !changed_prs.is_empty() {
            // A failure here keeps the PRs, for the retry to read.
            let numbers = refresh::to_read_ahead(&link, &self.settings, changed_prs)?;
            changed_prs.clear();
            if !numbers.is_empty() {
                self.phase = Phase::ReadingAhead(numbers);
                return Ok(Step::Moved);
            }
        }
        if let (Some(first_sync), Step::Searched { .. } | Step::Saved { .. }) =
            (self.first_sync.as_mut(), &step)
        {
            first_sync.pages += 1;
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
        store.save_sync_state(&self.settings.name, state)?;
        Ok(LastSync {
            repository: self.settings.name.clone(),
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
