//! A sync's walk through the Filters' searches (ADR 0015). It reads each search whole, or through ranges of
//! days when GitHub won't list it whole, and saves what the search found. It then asks for what Tsugiki
//! lacks. A refresh narrows a search to what changed, and merges the answer.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use time::{Date, OffsetDateTime};

use super::{Batches, Leads, Link, Moved, NO_PAUSE, Phase, SEARCH_PAGE, Step, pause_until};
use crate::error::{Error, ErrorKind};
use crate::filters::FilterQuery;
use crate::github::filters::{CreatedRange, FilterHit, FilterHitKind, FilterSearch};
use crate::github::pull_requests::{NodeId, PageRequest, PrFetch, PullRequestFacts};
use crate::repository::RepositoryName;
use crate::store::{FilterProblem, FilterResults, NarrowedResults};

/// The most results GitHub lists for one search (ADR 0015).
const SEARCH_CAP: u32 = 1000;

/// One search of a walk.
#[derive(Clone)]
struct WalkedSearch {
    query: FilterQuery,
    /// Keeps only what changed since this time. Without it, the search replaces the Filter's results.
    since: Option<OffsetDateTime>,
}

/// The Filters' searches, one at a time, with what they've found.
#[derive(Default)]
pub(super) struct FilterWalk {
    /// The searches still to finish. The first is the one under way.
    left: VecDeque<WalkedSearch>,
    /// The parts of that search still to read. The last is the one under way.
    slices: Vec<Slice>,
    cursor: Option<String>,
    /// That search's results so far.
    current: BTreeSet<u32>,
    cut_off: bool,
    /// The items all the searches have found.
    hits: BTreeMap<u32, FilterHit>,
    /// The PRs the user's own searches found, which a sync always fetches.
    work: Vec<NodeId>,
    /// What a refresh found before this walk began.
    leads: Leads,
    /// Each item a refresh found updated, which a narrowed search can say has stopped matching.
    changed: BTreeSet<u32>,
}

/// What a refresh hands to its walk.
#[derive(Default)]
pub(super) struct RefreshWalk {
    /// The searches to run whole.
    pub(super) whole: Vec<FilterQuery>,
    /// The searches to narrow to `since`.
    pub(super) narrowed: Vec<FilterQuery>,
    pub(super) since: Option<OffsetDateTime>,
    pub(super) changed: BTreeSet<u32>,
    /// What the refresh's poll found for the searches that don't walk.
    pub(super) hits: BTreeMap<u32, FilterHit>,
}

/// True when a refresh can narrow this search: it has finished once, and GitHub didn't refuse it last time.
/// A search GitHub cuts off stays narrowed, because a whole read would hit the same limit.
pub(super) fn can_narrow(stored: Option<&FilterResults>) -> bool {
    stored.is_some_and(|results| !matches!(results.problem, Some(FilterProblem::Refused(_))))
}

/// A part of a search GitHub can list whole.
#[derive(Clone, Copy)]
enum Slice {
    Whole,
    Created(CreatedRange),
}

impl Slice {
    fn created(self) -> Option<CreatedRange> {
        match self {
            Self::Whole => None,
            Self::Created(range) => Some(range),
        }
    }

    /// This part in two, the older first. The search has to end somewhere, and a single day can't be cut.
    fn halves(self, today: Date) -> Option<(Self, Self)> {
        let range = self.created().unwrap_or_else(|| CreatedRange::up_to(today));
        let (older, newer) = range.halves()?;
        Some((Self::Created(older), Self::Created(newer)))
    }
}

impl FilterWalk {
    pub(super) fn new(queries: &[FilterQuery], work: Vec<NodeId>) -> Self {
        Self {
            left: queries
                .iter()
                .cloned()
                .map(|query| WalkedSearch { query, since: None })
                .collect(),
            slices: vec![Slice::Whole],
            work,
            ..Self::default()
        }
    }

    /// The walk at the end of a refresh. The searches to run whole go first, as they have nothing to show.
    pub(super) fn of_refresh(plan: RefreshWalk) -> Self {
        let RefreshWalk {
            whole,
            narrowed,
            since,
            changed,
            hits,
        } = plan;
        let whole = whole
            .into_iter()
            .map(|query| WalkedSearch { query, since: None });
        let narrowed = narrowed
            .into_iter()
            .map(|query| WalkedSearch { query, since });
        Self {
            left: whole.chain(narrowed).collect(),
            slices: vec![Slice::Whole],
            hits,
            changed,
            ..Self::default()
        }
    }

    /// The Issues a refresh changed, whose trees this sync reads.
    pub(super) fn hold_issues(&mut self, issues: BTreeSet<u32>) {
        self.leads.held_issues.extend(issues);
    }

    /// Follows what a refresh's fetch of the user's own PR points at.
    pub(super) fn learn_from(
        &mut self,
        pull_request: &PullRequestFacts,
        repository: &RepositoryName,
    ) {
        self.leads.learn_from(pull_request, repository);
    }
}

/// Reads the next page of the search under way, or moves on from the last of them. One Filter that GitHub
/// won't answer keeps what it last found and says why, and stops nothing else (ADR 0015).
pub(super) fn walk_filters(link: &Link<'_>, walk: &mut FilterWalk) -> Result<Moved, Error> {
    let (Some(search), Some(slice)) = (walk.left.front().cloned(), walk.slices.last().copied())
    else {
        return Ok((NO_PAUSE, Some(fetch_what_is_missing(link, walk)?)));
    };
    let query = &search.query;
    let asked = FilterSearch {
        query,
        created: slice.created(),
        since: search.since,
        page: PageRequest {
            size: SEARCH_PAGE,
            after: walk.cursor.as_deref(),
        },
    };
    let page = match link.github.search_filter(link.repository, &asked) {
        Ok(page) => page,
        Err(problem) if blames_the_search(&problem) => {
            log::warn!("GitHub wouldn't search {query:?}: {}", problem.message);
            link.store
                .save_filter_problem(link.repository, query, &problem.message)?;
            walk.start_the_next_search();
            let saved = Step::Saved {
                touched: Vec::new(),
                pause_until: None,
            };
            return Ok((saved, None));
        }
        Err(problem) => return Err(problem),
    };
    let pause_until = pause_until(page.rate_limit);
    if walk.cursor.is_none() && page.total > SEARCH_CAP {
        let today = OffsetDateTime::now_utc().date();
        if let Some((older, newer)) = slice.halves(today) {
            let _halved = walk.slices.pop();
            walk.slices.extend([older, newer]);
            return Ok((Step::Searched { pause_until }, None));
        }
        walk.cut_off = true;
    }
    for hit in page.hits {
        walk.current.extend([hit.number]);
        drop(walk.hits.insert(hit.number, hit));
    }
    walk.cursor = page.next;
    if walk.cursor.is_none() {
        let _read = walk.slices.pop();
    }
    if !walk.slices.is_empty() {
        return Ok((Step::Searched { pause_until }, None));
    }
    let numbers = std::mem::take(&mut walk.current);
    let step = if search.since.is_some() {
        let narrowed = NarrowedResults {
            changed: walk.changed.clone(),
            found: numbers,
            cut_off: walk.cut_off,
        };
        let moved = link
            .store
            .merge_filter_results(link.repository, query, narrowed)?;
        let touched: Vec<u32> = moved.into_iter().collect();
        // The Sidebar shows the problem, so a cut-off search counts as a change even with no number in it.
        if walk.cut_off {
            Step::Saved {
                touched,
                pause_until,
            }
        } else {
            Step::saved_if_any(touched, pause_until)
        }
    } else {
        let touched = numbers.iter().copied().collect();
        let problem = walk.cut_off.then_some(FilterProblem::CutOff);
        let results = FilterResults { numbers, problem };
        link.store
            .save_filter_results(link.repository, query, results)?;
        Step::Saved {
            touched,
            pause_until,
        }
    };
    walk.start_the_next_search();
    Ok((step, None))
}

/// True when GitHub refused this search, which the next Filter's search wouldn't run into. An outage, no
/// network, a refused token or no points left would stop that search too, so the sync tries this one again.
pub(super) fn blames_the_search(problem: &Error) -> bool {
    match problem.kind {
        ErrorKind::NotFound | ErrorKind::QueryRefused => true,
        ErrorKind::Offline
        | ErrorKind::Unauthorized
        | ErrorKind::WriteRefused
        | ErrorKind::RateLimited { .. }
        | ErrorKind::Conflict
        | ErrorKind::Internal => false,
    }
}

impl FilterWalk {
    fn start_the_next_search(&mut self) {
        drop(self.left.pop_front());
        self.slices = vec![Slice::Whole];
        self.cursor = None;
        self.current.clear();
        self.cut_off = false;
    }
}

/// The user's own PRs, and each item a Filter found that Tsugiki lacks or has an older copy of. An item it
/// has as it is now only passes on what it points at, and so does a merged PR (ADR 0011). An open PR left out
/// here still comes with its Issue's tree, which keeps its CI current: CI finishing doesn't change
/// `updatedAt`.
fn fetch_what_is_missing(link: &Link<'_>, walk: &mut FilterWalk) -> Result<Phase, Error> {
    let stored = link.store.pull_requests(link.repository)?;
    let stored: BTreeMap<u32, PullRequestFacts> =
        stored.into_iter().map(|pr| (pr.number, pr)).collect();
    let issues = link.store.issues(link.repository)?;
    let issues: BTreeMap<u32, OffsetDateTime> = issues
        .iter()
        .map(|issue| (issue.number, issue.updated_at))
        .collect();
    let mut ids = std::mem::take(&mut walk.work);
    let mut asking: BTreeSet<NodeId> = ids.iter().cloned().collect();
    let mut leads = std::mem::take(&mut walk.leads);
    for hit in std::mem::take(&mut walk.hits).into_values() {
        match hit.kind {
            FilterHitKind::Issue => {
                if issues
                    .get(&hit.number)
                    .is_some_and(|at| *at >= hit.updated_at)
                {
                    leads.held_issues.extend([hit.number]);
                } else {
                    leads.named.extend([hit.number]);
                }
            }
            FilterHitKind::PullRequest(id) => match stored.get(&hit.number) {
                Some(pr) if pr.stays_as_it_is() || pr.updated_at >= hit.updated_at => {
                    leads.learn_from(pr, link.repository);
                }
                Some(_) | None => {
                    if asking.insert(id.clone()) {
                        ids.push(id);
                    }
                }
            },
        }
    }
    Ok(Phase::Fetching(Batches {
        ids,
        why: PrFetch::Work,
        leads,
    }))
}

#[cfg(test)]
mod tests;
