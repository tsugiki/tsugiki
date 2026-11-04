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
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use crate::github::testing::{Scripted, reply_ok, token};
    use crate::github::{Reply, Transport, Unreachable};
    use crate::store::{self, OpenReader, StoreReader};
    use serde_json::{Value, json};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::mpsc::Receiver;

    pub(super) const LOTS: &str =
        r#""rateLimit":{"remaining":4000,"resetAt":"2026-11-04T12:00:00Z"}"#;
    pub(super) const FEW: &str = r#""rateLimit":{"remaining":10,"resetAt":"2999-01-01T00:00:00Z"}"#;

    pub(super) fn cpython() -> RepositoryName {
        RepositoryName::parse("python/cpython").unwrap()
    }

    /// A Repository with no Filters, for the sync's other phases.
    fn plain(name: RepositoryName) -> RepositorySettings {
        RepositorySettings {
            filters: Vec::new(),
            ..RepositorySettings::new(name)
        }
    }

    fn found(ids: &[&str], next: Option<&str>, rate: &str) -> Result<Reply, Unreachable> {
        let nodes: Vec<Value> = ids.iter().map(|id| json!({ "id": id })).collect();
        let info = json!({ "hasNextPage": next.is_some(), "endCursor": next });
        reply_ok(&format!(
            r#"{{"data":{{"search":{{"pageInfo":{info},"nodes":{}}},{rate}}}}}"#,
            json!(nodes)
        ))
    }

    pub(super) fn fetched(numbers: &[u32], rate: &str) -> Result<Reply, Unreachable> {
        fetched_titled(numbers, rate, |number| format!("PR {number}"))
    }

    /// An answer to a fetch of PRs, each of whose titles names the Issue with the same number.
    fn fetched_naming_issues(numbers: &[u32], rate: &str) -> Result<Reply, Unreachable> {
        fetched_titled(numbers, rate, |number| format!("gh-{number}: Fix it"))
    }

    fn fetched_titled(
        numbers: &[u32],
        rate: &str,
        title: impl Fn(u32) -> String,
    ) -> Result<Reply, Unreachable> {
        let nodes: Vec<Value> = numbers
            .iter()
            .map(|number| {
                json!({
                    "number": number,
                    "title": title(*number),
                    "state": "OPEN",
                    "isDraft": false,
                    "updatedAt": "2026-11-04T10:00:00Z",
                    "baseRefName": "main",
                    "body": "",
                    "repository": {
                        "nameWithOwner": "python/cpython",
                        "defaultBranchRef": { "name": "main" },
                    },
                    "closingIssuesReferences": { "nodes": [] },
                    "author": { "login": "octocat" },
                    "assignees": { "nodes": [] },
                    "reviewRequests": { "nodes": [] },
                    "latestReviews": { "nodes": [] },
                    "lastCommit": { "nodes": [] },
                    "mergeCommit": null,
                    "commits": { "totalCount": 0, "nodes": [] },
                })
            })
            .collect();
        reply_ok(&format!(
            r#"{{"data":{{"nodes":{},{rate}}}}}"#,
            json!(nodes)
        ))
    }

    pub(super) struct Harness {
        pub(super) github: GitHub,
        pub(super) store: Store,
        pub(super) open: OpenReader,
        pub(super) reader: StoreReader,
        pub(super) asked: Receiver<(String, Value)>,
        pub(super) folder: PathBuf,
    }

    impl Harness {
        pub(super) fn new(name: &str, replies: Vec<Result<Reply, Unreachable>>) -> Self {
            let folder = crate::testing::temp_path(&format!("sync-{name}"));
            fs::create_dir_all(&folder).unwrap();
            let path = folder.join("tsugiki.db");
            let store = Store::open(&path).unwrap();
            let (to_open, reader) = store::reader();
            let open = to_open.open(&path).unwrap();
            let (script, asked) = Scripted::new(replies);
            let github = GitHub::new(Box::new(script), token());
            Self {
                github,
                store,
                open,
                reader,
                asked,
                folder,
            }
        }

        /// A harness whose GitHub is `transport`, which keeps its own record of what's asked.
        pub(super) fn answering(name: &str, transport: Box<dyn Transport>) -> Self {
            let mut harness = Self::new(name, Vec::new());
            harness.github = GitHub::new(transport, token());
            harness
        }

        pub(super) fn numbers(&self) -> Vec<u32> {
            let facts = self.reader.work_facts(&cpython()).unwrap();
            let mut numbers: Vec<u32> = facts.pull_requests.iter().map(|pr| pr.number).collect();
            numbers.sort_unstable();
            numbers
        }

        pub(super) fn operations(&self) -> Vec<String> {
            self.asked
                .try_iter()
                .map(|(operation, _)| operation)
                .collect()
        }

        /// Makes the next sync a whole one, as a first sync that was interrupted leaves it.
        pub(super) fn forget_the_last_sync(&self) {
            self.store.forget_sync_state(&cpython()).unwrap();
        }

        pub(super) fn finish(self) {
            self.open.close();
            self.store.close();
            fs::remove_dir_all(self.folder).unwrap();
        }
    }

    /// Runs every step that's left, and says which Repositories it announced.
    pub(super) fn run_to_the_end(syncs: &mut Syncs, harness: &Harness) -> Vec<RepositoryName> {
        let mut changed = Vec::new();
        while syncs.ready_at().is_some() {
            changed.extend(announced(
                syncs.step(&harness.github, &harness.store).unwrap(),
            ));
        }
        changed
    }

    /// The Repository a step says changed, if it says one did.
    pub(super) fn announced(progress: Option<Progress>) -> Option<RepositoryName> {
        match progress? {
            Progress::Changed(change) => Some(change.repository),
            Progress::Finished(_) => None,
        }
    }

    pub(super) fn nothing(rate: &str) -> Result<Reply, Unreachable> {
        found(&[], None, rate)
    }

    pub(super) fn issues(numbers: &[u32], rate: &str) -> Result<Reply, Unreachable> {
        let fields: serde_json::Map<String, Value> = numbers
            .iter()
            .map(|number| {
                let issue = json!({
                    "__typename": "Issue",
                    "number": number,
                    "title": format!("Issue {number}"),
                    "updatedAt": "2026-11-04T10:00:00Z",
                    "author": { "login": "some-author" },
                });
                (format!("i{number}"), issue)
            })
            .collect();
        reply_ok(&format!(
            r#"{{"data":{{"repository":{},{rate}}}}}"#,
            Value::Object(fields)
        ))
    }

    pub(super) fn no_cross_references(rate: &str) -> Result<Reply, Unreachable> {
        cross_references(&[], None, rate)
    }

    /// A page of an Issue's cross-references that lists these PRs as `id<number>`.
    pub(super) fn cross_references(
        numbers: &[u32],
        older: Option<&str>,
        rate: &str,
    ) -> Result<Reply, Unreachable> {
        let nodes: Vec<Value> = numbers
            .iter()
            .map(|number| {
                json!({ "source": {
                    "__typename": "PullRequest",
                    "id": format!("id{number}"),
                    "number": number,
                    "repository": { "nameWithOwner": "python/cpython" },
                } })
            })
            .collect();
        let issue = json!({ "timelineItems": {
            "pageInfo": { "hasPreviousPage": older.is_some(), "startCursor": older },
            "nodes": nodes,
        } });
        reply_ok(&format!(
            r#"{{"data":{{"repository":{{"issue":{issue}}},{rate}}}}}"#
        ))
    }

    /// PR `number` as a fetch answers with it, with `changes` on top.
    pub(super) fn pr_node(number: u32, changes: &Value) -> Value {
        let mut node = json!({
            "number": number,
            "title": format!("gh-1: Fix it {number}"),
            "state": "OPEN",
            "isDraft": false,
            "updatedAt": "2026-11-04T10:00:00Z",
            "baseRefName": "main",
            "body": "",
            "repository": {
                "nameWithOwner": "python/cpython",
                "defaultBranchRef": { "name": "main" },
            },
            "closingIssuesReferences": { "nodes": [] },
            "author": { "login": "octocat" },
            "assignees": { "nodes": [] },
            "reviewRequests": { "nodes": [] },
            "latestReviews": { "nodes": [] },
            "lastCommit": { "nodes": [] },
            "mergeCommit": null,
            "commits": { "totalCount": 0, "nodes": [] },
        });
        for (name, value) in changes.as_object().unwrap() {
            node[name] = value.clone();
        }
        node
    }

    pub(super) fn fetched_nodes(nodes: &[Value], rate: &str) -> Result<Reply, Unreachable> {
        reply_ok(&format!(
            r#"{{"data":{{"nodes":{},{rate}}}}}"#,
            json!(nodes)
        ))
    }

    fn origin_answer(node: &Value, rate: &str) -> Result<Reply, Unreachable> {
        reply_ok(&format!(
            r#"{{"data":{{"repository":{{"pullRequest":{node}}},{rate}}}}}"#
        ))
    }

    /// PR `number`, a Backport of `origin` on 3.14.
    fn backport_node(number: u32, origin: u32) -> Value {
        pr_node(
            number,
            &json!({
                "title": format!("[3.14] gh-1: Fix it (GH-{origin})"),
                "baseRefName": "3.14",
            }),
        )
    }

    /// PR `number`, a merged PR that fixes Issue 1.
    fn merged_node(number: u32) -> Value {
        pr_node(
            number,
            &json!({ "state": "MERGED", "mergeCommit": { "oid": format!("{number:040x}") } }),
        )
    }

    /// The replies that bring the user's one PR, #1, and its one Issue, #1.
    fn up_to_the_first_issue() -> Vec<Result<Reply, Unreachable>> {
        let mut replies = searches_finding(&["a"]);
        replies.push(fetched_nodes(&[pr_node(1, &json!({}))], LOTS));
        replies.push(issues(&[1], LOTS));
        replies
    }

    #[test]
    fn a_sync_reads_each_shown_issues_cross_references_and_fetches_the_prs_they_list() {
        let mut replies = up_to_the_first_issue();
        replies.push(cross_references(&[2, 3], None, LOTS));
        replies.push(fetched_nodes(&[merged_node(2), merged_node(3)], LOTS));
        let harness = Harness::new("tree", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(
            harness.operations()[4..],
            [
                "FetchPullRequests",
                "FetchIssues",
                "FetchCrossReferences",
                "FetchTreePullRequests",
            ]
        );
        assert_eq!(harness.numbers(), [1, 2, 3]);
        harness.finish();
    }

    #[test]
    fn a_sync_follows_an_issues_cross_references_page_by_page() {
        let mut replies = up_to_the_first_issue();
        replies.push(cross_references(&[3], Some("older"), LOTS));
        replies.push(cross_references(&[2], None, LOTS));
        replies.push(fetched_nodes(&[merged_node(3), merged_node(2)], LOTS));
        let harness = Harness::new("tree-pages", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        let befores: Vec<Value> = harness
            .asked
            .try_iter()
            .filter(|(operation, _)| operation == "FetchCrossReferences")
            .map(|(_, sent)| sent["before"].clone())
            .collect();
        assert_eq!(befores, [Value::Null, json!("older")]);
        assert_eq!(harness.numbers(), [1, 2, 3]);
        harness.finish();
    }

    #[test]
    fn a_sync_reads_the_cross_references_of_each_issue_and_fetches_a_shared_pr_once() {
        let mut replies = searches_finding(&["a"]);
        replies.push(fetched_nodes(
            &[pr_node(1, &json!({ "title": "gh-1, gh-2: Fix both" }))],
            LOTS,
        ));
        replies.push(issues(&[1, 2], LOTS));
        replies.push(cross_references(&[3, 1], None, LOTS));
        replies.push(cross_references(&[3], None, LOTS));
        replies.push(fetched_nodes(&[merged_node(3)], LOTS));
        let harness = Harness::new("tree-two", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        let tree_ids: Vec<Value> = harness
            .asked
            .try_iter()
            .filter(|(operation, _)| operation == "FetchTreePullRequests")
            .map(|(_, sent)| sent["ids"].clone())
            .collect();
        assert_eq!(tree_ids, [json!(["id3"])]);
        harness.finish();
    }

    #[test]
    fn a_merged_pr_is_fetched_once_across_syncs_and_an_open_one_again() {
        let mut replies = up_to_the_first_issue();
        replies.push(cross_references(&[2, 3], None, LOTS));
        replies.push(fetched_nodes(
            &[merged_node(2), pr_node(3, &json!({}))],
            LOTS,
        ));
        replies.extend(up_to_the_first_issue());
        replies.push(cross_references(&[2, 3], None, LOTS));
        replies.push(fetched_nodes(&[pr_node(3, &json!({}))], LOTS));
        let harness = Harness::new("frozen", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        drop(run_to_the_end(&mut syncs, &harness));
        drop(harness.operations());

        harness.forget_the_last_sync();
        syncs.add(plain(cpython()));
        drop(run_to_the_end(&mut syncs, &harness));

        let tree_ids: Vec<Value> = harness
            .asked
            .try_iter()
            .filter(|(operation, _)| operation == "FetchTreePullRequests")
            .map(|(_, sent)| sent["ids"].clone())
            .collect();
        assert_eq!(tree_ids, [json!(["id3"])]);
        harness.finish();
    }

    #[test]
    fn a_sync_fetches_a_users_backports_missing_origin_pr_by_number() {
        let mut replies = searches_finding(&["a"]);
        replies.push(fetched_nodes(&[backport_node(2, 9)], LOTS));
        replies.push(origin_answer(&merged_node(9), LOTS));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::new("origin", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        let changed = run_to_the_end(&mut syncs, &harness);

        let (numbers, asked): (Vec<String>, Vec<Value>) = harness.asked.try_iter().unzip();
        assert_eq!(
            numbers[4..],
            [
                "FetchPullRequests",
                "FetchOriginPullRequest",
                "FetchIssues",
                "FetchCrossReferences",
            ]
        );
        assert_eq!(asked[5]["number"], 9);
        assert_eq!(harness.numbers(), [2, 9]);
        assert_eq!(changed.len(), 3);
        harness.finish();
    }

    #[test]
    fn a_sync_fetches_the_issues_a_users_backports_origin_pr_names() {
        let mut replies = searches_finding(&["a"]);
        let untitled = pr_node(
            2,
            &json!({ "title": "[3.14] Fix it (GH-9)", "baseRefName": "3.14" }),
        );
        replies.push(fetched_nodes(&[untitled], LOTS));
        replies.push(origin_answer(&merged_node(9), LOTS));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::new("origin-issue", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        let facts = harness.reader.work_facts(&cpython()).unwrap();
        let stored: Vec<u32> = facts.issues.iter().map(|issue| issue.number).collect();
        assert_eq!(stored, [1]);
        harness.finish();
    }

    #[test]
    fn a_sync_fetches_a_tree_backports_missing_origin_pr_after_the_tree() {
        let mut replies = up_to_the_first_issue();
        replies.push(cross_references(&[2], None, LOTS));
        replies.push(fetched_nodes(&[backport_node(2, 9)], LOTS));
        replies.push(origin_answer(&merged_node(9), LOTS));
        let harness = Harness::new("tree-origin", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(harness.numbers(), [1, 2, 9]);
        harness.finish();
    }

    #[test]
    fn a_sync_doesnt_ask_for_an_origin_pr_it_has_fetched_or_frozen() {
        let mut replies = searches_finding(&["a"]);
        replies.push(fetched_nodes(
            &[pr_node(9, &json!({})), backport_node(2, 9)],
            LOTS,
        ));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        // The next sync finds only the Backport, and its Origin PR has merged by then.
        replies.extend(searches_finding(&["a"]));
        replies.push(fetched_nodes(&[backport_node(2, 8)], LOTS));
        replies.push(origin_answer(&merged_node(8), LOTS));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        replies.extend(searches_finding(&["a"]));
        replies.push(fetched_nodes(&[backport_node(2, 8)], LOTS));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::new("origin-known", replies);
        let mut syncs = Syncs::default();

        let origins_in_each_sync: Vec<usize> = (0..3)
            .map(|_| {
                harness.forget_the_last_sync();
                syncs.add(plain(cpython()));
                drop(run_to_the_end(&mut syncs, &harness));
                harness
                    .operations()
                    .iter()
                    .filter(|operation| *operation == "FetchOriginPullRequest")
                    .count()
            })
            .collect();

        assert_eq!(origins_in_each_sync, [0, 1, 0]);
        harness.finish();
    }

    #[test]
    fn a_frozen_origin_pr_still_leads_a_new_backport_to_its_issue() {
        let untitled = |number: u32| {
            pr_node(
                number,
                &json!({ "title": "[3.14] Fix it (GH-9)", "baseRefName": "3.14" }),
            )
        };
        let mut replies = searches_finding(&["a"]);
        replies.push(fetched_nodes(&[untitled(2)], LOTS));
        replies.push(origin_answer(&merged_node(9), LOTS));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        replies.extend(searches_finding(&["b"]));
        replies.push(fetched_nodes(&[untitled(3)], LOTS));
        replies.push(issues(&[1], LOTS));
        replies.push(cross_references(&[], None, LOTS));
        let harness = Harness::new("origin-frozen", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        drop(run_to_the_end(&mut syncs, &harness));
        drop(harness.operations());

        harness.forget_the_last_sync();
        syncs.add(plain(cpython()));
        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(
            harness.operations()[4..],
            ["FetchPullRequests", "FetchIssues", "FetchCrossReferences"]
        );
        harness.finish();
    }

    #[test]
    fn an_origin_number_that_names_no_pr_is_asked_for_once() {
        let mut replies = searches_finding(&["a"]);
        replies.push(fetched_nodes(&[backport_node(2, 9)], LOTS));
        replies.push(reply_ok(&format!(
            r#"{{"data":{{"repository":{{"pullRequest":null}},{LOTS}}},"errors":[{{"type":"NOT_FOUND",
                "message":"gone","path":["repository","pullRequest"]}}]}}"#
        )));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::new("origin-gone", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(harness.numbers(), [2]);
        harness.finish();
    }

    #[test]
    fn a_tree_read_below_the_floor_holds_the_next_step_back() {
        let mut replies = up_to_the_first_issue();
        replies.push(cross_references(&[], Some("older"), FEW));
        let harness = Harness::new("tree-floor", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        let before = Instant::now();

        // Four searches, two moves, the PR, two moves, the Issue, a move and the first page.
        for _ in 0..12 {
            drop(syncs.step(&harness.github, &harness.store).unwrap());
        }

        let ready = syncs.ready_at().unwrap();
        assert!(ready > before + Duration::from_secs(3000), "{ready:?}");
        harness.finish();
    }

    #[test]
    fn an_origin_fetch_below_the_floor_holds_the_next_step_back() {
        let mut replies = searches_finding(&["a"]);
        replies.push(fetched_nodes(&[backport_node(2, 9)], LOTS));
        replies.push(origin_answer(&merged_node(9), FEW));
        let harness = Harness::new("origin-floor", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        let before = Instant::now();

        // Four searches, two moves, the PR, a move and the Origin PR.
        for _ in 0..9 {
            drop(syncs.step(&harness.github, &harness.store).unwrap());
        }

        let ready = syncs.ready_at().unwrap();
        assert!(ready > before + Duration::from_secs(3000), "{ready:?}");
        harness.finish();
    }

    /// The replies up to and including the search that finds `ids`.
    pub(super) fn searches_finding(ids: &[&str]) -> Vec<Result<Reply, Unreachable>> {
        vec![
            found(ids, None, LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
        ]
    }

    #[test]
    fn a_sync_fetches_the_issues_its_prs_name_after_the_prs() {
        let mut replies = searches_finding(&["a", "b"]);
        replies.push(fetched_naming_issues(&[1, 2], LOTS));
        replies.push(issues(&[1, 2], LOTS));
        replies.push(no_cross_references(LOTS));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::new("issues", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        let changed = run_to_the_end(&mut syncs, &harness);

        let asked: Vec<String> = harness.operations();
        assert_eq!(
            asked[4..],
            [
                "FetchPullRequests",
                "FetchIssues",
                "FetchCrossReferences",
                "FetchCrossReferences",
            ]
        );
        let facts = harness.reader.work_facts(&cpython()).unwrap();
        let stored: Vec<u32> = facts.issues.iter().map(|issue| issue.number).collect();
        assert_eq!(stored, [1, 2]);
        assert_eq!(changed, [cpython(), cpython()]);
        harness.finish();
    }

    #[test]
    fn a_sync_with_no_named_issues_asks_for_none() {
        let mut replies = searches_finding(&["a"]);
        replies.push(fetched(&[1], LOTS));
        let harness = Harness::new("no-issues", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(harness.operations().len(), 5);
        harness.finish();
    }

    #[test]
    fn a_sync_fetches_a_titles_issue_whatever_the_conventions_of_its_repository() {
        let mut replies = searches_finding(&["a"]);
        replies.push(fetched_naming_issues(&[1], LOTS));
        replies.push(issues(&[1], LOTS));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::new("conventions", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(RepositoryName::parse("python/peps").unwrap()));

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(
            harness.operations()[4..],
            ["FetchPullRequests", "FetchIssues", "FetchCrossReferences"]
        );
        harness.finish();
    }

    #[test]
    fn a_fetch_of_issues_goes_batch_by_batch_and_pauses_below_the_floor() {
        let ids: Vec<u32> = (1..=30).collect();
        let mut replies = searches_finding(&["a"]);
        replies.push(fetched_naming_issues(&ids, LOTS));
        replies.push(issues(&[1], FEW));
        replies.push(issues(&[2], LOTS));
        replies.push(no_cross_references(LOTS));
        replies.push(no_cross_references(LOTS));
        let harness = Harness::new("issue-batches", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        let before = Instant::now();

        // Four searches, two moves, the PRs, two moves and the first batch of Issues.
        for _ in 0..10 {
            drop(syncs.step(&harness.github, &harness.store).unwrap());
        }
        let ready = syncs.ready_at().unwrap();
        assert!(ready > before + Duration::from_secs(3000), "{ready:?}");
        // The two batches take all 30 numbers.
        drop(run_to_the_end(&mut syncs, &harness));
        assert_eq!(syncs.ready_at(), None);

        let facts = harness.reader.work_facts(&cpython()).unwrap();
        assert_eq!(facts.issues.len(), 2);
        harness.finish();
    }

    #[test]
    fn a_sync_searches_in_order_then_stores_what_it_found() {
        let harness = Harness::new(
            "order",
            vec![
                found(&["a"], None, LOTS),
                found(&["b"], None, LOTS),
                found(&["c"], None, LOTS),
                found(&["d"], None, LOTS),
                fetched(&[1, 2, 3, 4], LOTS),
            ],
        );
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        let changed = run_to_the_end(&mut syncs, &harness);

        assert_eq!(
            harness.operations(),
            [
                "SearchReviewRequests",
                "SearchReviewedPrs",
                "SearchOwnPrs",
                "SearchAssignedPrs",
                "FetchPullRequests",
            ]
        );
        assert_eq!(harness.numbers(), [1, 2, 3, 4]);
        assert_eq!(changed, [cpython()]);
        harness.finish();
    }

    #[test]
    fn a_search_follows_its_pages_before_the_next_search() {
        let harness = Harness::new(
            "pages",
            vec![
                found(&["a"], Some("next"), LOTS),
                found(&["b"], None, LOTS),
                nothing(LOTS),
                nothing(LOTS),
                nothing(LOTS),
                fetched(&[1, 2], LOTS),
            ],
        );
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
        assert_eq!(asked[0].1["after"], Value::Null);
        assert_eq!(asked[1].0, "SearchReviewRequests");
        assert_eq!(asked[1].1["after"], "next");
        assert_eq!(asked[2].0, "SearchReviewedPrs");
        assert_eq!(asked[2].1["after"], Value::Null);
        assert_eq!(asked[5].1["ids"], json!(["a", "b"]));
        harness.finish();
    }

    #[test]
    fn a_pr_two_searches_found_is_fetched_once() {
        let harness = Harness::new(
            "twice",
            vec![
                found(&["a"], None, LOTS),
                found(&["a"], None, LOTS),
                found(&["a", "b"], None, LOTS),
                nothing(LOTS),
                fetched(&[1, 2], LOTS),
            ],
        );
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
        assert_eq!(asked[4].1["ids"], json!(["a", "b"]));
        harness.finish();
    }

    #[test]
    fn a_retry_at_half_size_leaves_the_rest_for_the_next_batch() {
        let ids: Vec<String> = (0..4).map(|n| format!("id{n}")).collect();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        let harness = Harness::new(
            "half",
            vec![
                found(&ids, None, LOTS),
                nothing(LOTS),
                nothing(LOTS),
                nothing(LOTS),
                Err(Unreachable),
                fetched(&[1, 2], LOTS),
                fetched(&[3, 4], LOTS),
            ],
        );
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        let changed = run_to_the_end(&mut syncs, &harness);

        let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
        assert_eq!(asked[5].1["ids"], json!(["id0", "id1"]));
        assert_eq!(asked[6].1["ids"], json!(["id2", "id3"]));
        assert_eq!(harness.numbers(), [1, 2, 3, 4]);
        assert_eq!(changed, [cpython(), cpython()]);
        harness.finish();
    }

    #[test]
    fn a_failed_step_keeps_its_place_and_runs_again() {
        let harness = Harness::new(
            "failure",
            vec![
                found(&["a"], Some("next"), LOTS),
                Err(Unreachable),
                Err(Unreachable),
                found(&["b"], None, LOTS),
            ],
        );
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        assert_eq!(syncs.step(&harness.github, &harness.store).unwrap(), None);

        let failed = syncs.step(&harness.github, &harness.store).unwrap_err();
        drop(syncs.step(&harness.github, &harness.store).unwrap());

        assert_eq!(failed.kind, ErrorKind::Offline);
        let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
        assert_eq!(asked[3].1["after"], "next");
        harness.finish();
    }

    #[test]
    fn a_search_below_the_floor_holds_the_next_step_back() {
        let harness = Harness::new("floor-search", vec![nothing(FEW)]);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        let before = Instant::now();

        drop(syncs.step(&harness.github, &harness.store).unwrap());

        let ready = syncs.ready_at().unwrap();
        assert!(ready > before + Duration::from_secs(3000), "{ready:?}");
        harness.finish();
    }

    #[test]
    fn a_fetch_below_the_floor_holds_the_next_step_back() {
        let harness = Harness::new(
            "floor-fetch",
            vec![
                found(&["a"], None, LOTS),
                nothing(LOTS),
                nothing(LOTS),
                nothing(LOTS),
                fetched(&[1], FEW),
            ],
        );
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        let before = Instant::now();

        for _ in 0..7 {
            drop(syncs.step(&harness.github, &harness.store).unwrap());
        }

        let ready = syncs.ready_at().unwrap();
        assert!(ready > before + Duration::from_secs(3000), "{ready:?}");
        assert_eq!(harness.numbers(), [1]);
        harness.finish();
    }

    #[test]
    fn a_held_back_step_says_when() {
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        let later = Instant::now() + Duration::from_secs(60);

        syncs.wait_until(later);

        assert_eq!(syncs.ready_at(), Some(later));
    }

    #[test]
    fn nothing_queued_means_nothing_to_do() {
        let harness = Harness::new("empty", vec![]);
        let mut syncs = Syncs::default();

        assert_eq!(syncs.ready_at(), None);
        assert_eq!(syncs.step(&harness.github, &harness.store).unwrap(), None);
        assert_eq!(harness.operations(), [""; 0]);
        harness.finish();
    }

    #[test]
    fn a_repository_already_queued_isnt_queued_twice() {
        let harness = Harness::new(
            "dedupe",
            vec![nothing(LOTS), nothing(LOTS), nothing(LOTS), nothing(LOTS)],
        );
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        syncs.add(plain(RepositoryName::parse("Python/CPython").unwrap()));

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(harness.operations().len(), 4);
        harness.finish();
    }

    #[test]
    fn a_sync_pauses_only_below_the_floor() {
        let reset_at = OffsetDateTime::UNIX_EPOCH;
        let at_the_floor = RateLimit {
            remaining: RATE_LIMIT_FLOOR,
            reset_at,
        };
        let below = RateLimit {
            remaining: RATE_LIMIT_FLOOR - 1,
            reset_at,
        };
        assert_eq!(pause_until(at_the_floor), None);
        assert_eq!(pause_until(below), Some(reset_at));
    }

    #[test]
    fn every_search_finds_open_prs() {
        for search in WORK_SEARCHES {
            assert!(
                search.qualifiers.starts_with("is:pr is:open "),
                "{search:?}"
            );
        }
    }

    #[test]
    fn a_sync_asks_for_pages_of_50_and_batches_of_25() {
        let names: Vec<String> = (0..30).map(|n| format!("id{n}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let harness = Harness::new(
            "sizes",
            vec![
                found(&names, None, LOTS),
                nothing(LOTS),
                nothing(LOTS),
                nothing(LOTS),
                fetched(&[1], LOTS),
                fetched(&[2], LOTS),
            ],
        );
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
        assert_eq!(asked[0].1["first"], 50);
        assert_eq!(
            asked[0].1["query"],
            "repo:python/cpython is:pr is:open user-review-requested:@me"
        );
        assert_eq!(asked[4].1["ids"].as_array().unwrap().len(), 25);
        assert_eq!(asked[5].1["ids"].as_array().unwrap().len(), 5);
        harness.finish();
    }

    #[test]
    fn the_repository_asked_for_last_runs_first() {
        let harness = Harness::new(
            "queue",
            vec![
                nothing(LOTS),
                nothing(LOTS),
                nothing(LOTS),
                nothing(LOTS),
                nothing(LOTS),
                nothing(LOTS),
                nothing(LOTS),
                nothing(LOTS),
            ],
        );
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        syncs.add(plain(RepositoryName::parse("python/peps").unwrap()));

        drop(run_to_the_end(&mut syncs, &harness));

        let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
        let query = |at: usize| asked[at].1["query"].as_str().unwrap().to_owned();
        assert!(query(3).starts_with("repo:python/peps "), "{}", query(3));
        assert!(query(4).starts_with("repo:python/cpython "), "{}", query(4));
        harness.finish();
    }

    #[test]
    fn a_sync_keeps_its_progress_behind_a_newer_one() {
        let harness = Harness::new(
            "behind",
            vec![
                found(&["a"], Some("next"), LOTS),
                nothing(LOTS),
                nothing(LOTS),
                nothing(LOTS),
                nothing(LOTS),
                found(&[], None, LOTS),
            ],
        );
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        assert_eq!(syncs.step(&harness.github, &harness.store).unwrap(), None);
        syncs.add(plain(RepositoryName::parse("python/peps").unwrap()));

        // The newer sync's four searches, its seven moves from phase to phase and its end, then the older
        // sync's next page.
        for _ in 0..13 {
            let progress = syncs.step(&harness.github, &harness.store).unwrap();
            assert_eq!(announced(progress), None);
        }

        let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
        assert!(
            asked[5].1["query"]
                .as_str()
                .unwrap()
                .starts_with("repo:python/cpython ")
        );
        assert_eq!(asked[5].1["after"], "next");
        harness.finish();
    }

    fn soon() -> Instant {
        Instant::now() + Duration::from_secs(60)
    }

    #[test]
    fn with_nothing_open_a_refresh_queues_nothing() {
        let harness = Harness::new("refresh-nothing", Vec::new());
        let mut syncs = Syncs::default();
        syncs.refresh_open(&harness.store);
        syncs.refresh_now();
        assert_eq!(syncs.ready_at(), None);
        harness.finish();
    }

    #[test]
    fn the_timer_leaves_a_first_sync_that_gave_up_and_a_click_starts_it_again() {
        let harness = Harness::new("gave-up", Vec::new());
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        for _ in 0..TRIES {
            syncs.after_failure(&broken(), soon());
        }
        assert_eq!(syncs.ready_at(), None);

        syncs.refresh_open(&harness.store);
        assert_eq!(syncs.ready_at(), None);

        syncs.refresh_now();
        assert_eq!(syncs.queue.len(), 1);
        harness.finish();
    }

    #[test]
    fn a_click_ends_a_wait_for_the_network_and_keeps_a_wait_for_points() {
        let later = Instant::now() + Duration::from_secs(3600);
        let offline = Error::new(ErrorKind::Offline, "No network.");
        let spent = Error::new(ErrorKind::RateLimited { resets_at: None }, "No points.");
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        syncs.after_failure(&offline, later);
        syncs.refresh_now();
        assert!(syncs.ready_at().unwrap() < later);

        syncs.after_failure(&spent, later);
        syncs.refresh_now();
        assert_eq!(syncs.ready_at(), Some(later));
    }

    #[test]
    fn the_last_sync_says_when_it_finished_and_the_next_asks_from_when_it_started() {
        let harness = Harness::new("finish-time", searches_finding(&[]));
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        let began = OffsetDateTime::now_utc() - time::Duration::hours(1);
        syncs.queue.front_mut().unwrap().started_at = began;

        let mut finished = None;
        while syncs.ready_at().is_some() {
            if let Some(Progress::Finished(last_sync)) =
                syncs.step(&harness.github, &harness.store).unwrap()
            {
                finished = last_sync.at;
            }
        }

        let state = harness.store.sync_state(&cpython()).unwrap().unwrap();
        assert_eq!(finished, Some(state.finished_at));
        assert_eq!(state.started_at.unix_timestamp(), began.unix_timestamp());
        assert!(state.finished_at > began + time::Duration::minutes(59));
        harness.finish();
    }

    #[test]
    fn a_refresh_of_the_open_repository_joins_the_queue_once() {
        let harness = Harness::new("refresh-open", searches_finding(&[]));
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        drop(run_to_the_end(&mut syncs, &harness));
        let operations = harness.operations().len();
        assert_eq!(operations, 4, "one sync's four searches");

        syncs.refresh_open(&harness.store);
        syncs.refresh_open(&harness.store);

        assert_eq!(syncs.queue.len(), 1);
        harness.finish();
    }

    #[test]
    fn a_refresh_goes_behind_a_sync_of_another_repository() {
        let peps = RepositoryName::parse("python/peps").unwrap();
        let mut replies = searches_finding(&[]);
        replies.extend(searches_finding(&[]));
        let harness = Harness::new("refresh-behind", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        drop(run_to_the_end(&mut syncs, &harness));
        syncs.add(plain(peps.clone()));
        syncs.add(plain(cpython()));
        syncs.queue.retain(|sync| sync.repository == peps);
        drop(syncs.step(&harness.github, &harness.store).unwrap());

        syncs.refresh_open(&harness.store);

        assert_eq!(syncs.queue.len(), 2);
        assert_eq!(syncs.queue.front().unwrap().repository, peps);
        harness.finish();
    }

    #[test]
    fn only_the_open_repository_says_its_sync_finished() {
        let peps = RepositoryName::parse("python/peps").unwrap();
        let mut replies = searches_finding(&[]);
        replies.extend(searches_finding(&[]));
        let harness = Harness::new("finished", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(peps));
        syncs.add(plain(cpython()));

        let mut finished = Vec::new();
        while syncs.ready_at().is_some() {
            let progress = syncs.step(&harness.github, &harness.store).unwrap();
            if let Some(Progress::Finished(last_sync)) = progress {
                finished.push(last_sync.repository);
            }
        }

        assert_eq!(finished, [cpython()]);
        harness.finish();
    }

    fn broken() -> Error {
        Error::new(ErrorKind::Internal, "GitHub answered with status 500.")
    }

    /// Whether a sync is still queued after `tries` failures for `problem`.
    fn queued_after(tries: u8, problem: &Error) -> bool {
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        for _ in 0..tries {
            syncs.after_failure(problem, soon());
        }
        syncs.ready_at().is_some()
    }

    #[test]
    fn a_step_that_keeps_failing_gives_way_to_the_next_sync() {
        let mut syncs = Syncs::default();
        syncs.add(plain(RepositoryName::parse("python/peps").unwrap()));
        syncs.add(plain(cpython()));

        for _ in 0..TRIES {
            syncs.after_failure(&broken(), soon());
        }

        assert_eq!(syncs.queue.len(), 1);
        assert_eq!(
            syncs.queue.front().unwrap().repository,
            RepositoryName::parse("python/peps").unwrap()
        );
        assert_eq!(syncs.failures, 0);
    }

    #[test]
    fn failures_with_nothing_queued_change_nothing() {
        let mut syncs = Syncs::default();
        for _ in 0..TRIES {
            syncs.after_failure(&broken(), soon());
        }
        assert_eq!(syncs.ready_at(), None);
    }

    #[test]
    fn an_outage_shorter_than_the_tries_doesnt_end_a_sync() {
        assert!(queued_after(TRIES - 1, &broken()));
        assert!(!queued_after(TRIES, &broken()));
    }

    #[test]
    fn a_step_that_works_starts_the_count_again() {
        let harness = Harness::new("recovers", vec![nothing(LOTS)]);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        for _ in 0..TRIES - 1 {
            syncs.after_failure(&broken(), soon());
        }

        drop(syncs.step(&harness.github, &harness.store).unwrap());
        syncs.after_failure(&broken(), soon());

        assert_eq!(syncs.queue.len(), 1);
        harness.finish();
    }

    #[test]
    fn asking_again_starts_the_count_again() {
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        for _ in 0..TRIES - 1 {
            syncs.after_failure(&broken(), soon());
        }

        syncs.add(plain(cpython()));
        syncs.after_failure(&broken(), soon());

        assert_eq!(syncs.queue.len(), 1);
    }

    #[test]
    fn no_network_never_ends_a_sync() {
        let offline = Error::new(ErrorKind::Offline, "no network");
        assert!(queued_after(u8::MAX, &offline));
    }

    #[test]
    fn a_rate_limit_never_ends_a_sync() {
        let limited = Error::new(ErrorKind::RateLimited { resets_at: None }, "limited");
        assert!(queued_after(u8::MAX, &limited));
    }

    #[test]
    fn a_failure_waits_for_the_next_refresh() {
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        let next_refresh = soon();

        syncs.after_failure(&broken(), next_refresh);

        assert_eq!(syncs.ready_at(), Some(next_refresh));
    }

    fn limited_until(from_now: i64) -> Error {
        let resets_at = OffsetDateTime::now_utc().unix_timestamp() + from_now;
        let resets_at = Some(u32::try_from(resets_at).unwrap());
        Error::new(ErrorKind::RateLimited { resets_at }, "limited")
    }

    #[test]
    fn a_rate_limit_waits_for_its_reset() {
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        syncs.after_failure(&limited_until(3000), soon());

        let wait = syncs.ready_at().unwrap() - Instant::now();
        assert!(wait > Duration::from_secs(2900), "{wait:?}");
        assert!(wait <= Duration::from_secs(3001), "{wait:?}");
    }

    #[test]
    fn a_rate_limit_whose_reset_has_passed_still_waits_for_the_next_refresh() {
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        let next_refresh = soon();

        syncs.after_failure(&limited_until(-10), next_refresh);

        assert_eq!(syncs.ready_at(), Some(next_refresh));
    }

    #[test]
    fn a_save_that_fails_is_the_steps_failure() {
        let harness = Harness::new(
            "save-fails",
            vec![
                found(&["a"], None, LOTS),
                nothing(LOTS),
                nothing(LOTS),
                nothing(LOTS),
                fetched(&[1], LOTS),
            ],
        );
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        for _ in 0..5 {
            drop(syncs.step(&harness.github, &harness.store).unwrap());
        }
        let stopped = Store::stopped();

        let failed = syncs.step(&harness.github, &stopped).unwrap_err();
        stopped.close();

        assert_eq!(failed.kind, ErrorKind::Internal);
        harness.finish();
    }

    #[test]
    fn a_pause_is_as_long_as_the_reset_is_away_within_reason() {
        let now = OffsetDateTime::UNIX_EPOCH;
        let minute = time::Duration::minutes(1);

        assert_eq!(pause_for(now + minute, now), Duration::from_secs(60));
        assert_eq!(pause_for(now - minute, now), Duration::ZERO);
        assert_eq!(pause_for(now + minute * 600, now), LONGEST_PAUSE);
    }
}
