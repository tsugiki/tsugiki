//! A sync's walk through the Filters' searches (ADR 0015). It reads each search whole, or through ranges of
//! days when GitHub won't list it whole, and saves what the search found. It then asks for what Tsugiki
//! lacks. A refresh narrows a search to what changed, and merges the answer.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};

use super::{Batches, Leads, Link, Moved, Phase, Resumable, SEARCH_PAGE, Step, pause_until};
use crate::error::{Error, ErrorKind};
use crate::filters::FilterQuery;
use crate::github::filters::{CreatedRange, FilterHit, FilterHitKind, FilterSearch};
use crate::github::pull_requests::{NodeId, PageRequest, PrFetch, PullRequestFacts};
use crate::repository::RepositoryName;
use crate::store::{FilterProblem, FilterResults, NarrowedResults};

/// The most results GitHub lists for one search (ADR 0015).
const SEARCH_CAP: u32 = 1000;

/// One search of a walk.
#[derive(Clone, Serialize, Deserialize)]
struct WalkedSearch {
    query: FilterQuery,
    /// Keeps only what changed since this time. Without it, the search replaces the Filter's results.
    since: Option<OffsetDateTime>,
}

/// The Filters' searches, one at a time, with what they've found.
#[derive(Default, Serialize, Deserialize)]
pub(super) struct FilterWalk {
    /// The searches still to finish. The first is the one under way.
    left: VecDeque<WalkedSearch>,
    /// How far that search has got.
    place: Place,
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

/// How far the search under way has got.
#[derive(Serialize, Deserialize)]
enum Place {
    Reading(Pages),
    /// It has no more to read, and its results wait to be saved.
    Read {
        pause_until: Option<OffsetDateTime>,
    },
}

impl Default for Place {
    fn default() -> Self {
        Self::Reading(Pages::default())
    }
}

/// The pages of a search still to read.
#[derive(Default, Serialize, Deserialize)]
struct Pages {
    /// The part under way.
    slice: Slice,
    /// The parts to read after it, the next one last.
    older: Vec<Slice>,
    cursor: Option<String>,
}

/// A part of a search GitHub can list whole.
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
enum Slice {
    #[default]
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
    let Some(search) = walk.left.front().cloned() else {
        return Ok((Step::Moved, Some(fetch_what_is_missing(link, walk)?)));
    };
    let pause_until = match read_page(link, walk, &search)? {
        Page::Stays(step) => return Ok((step, None)),
        Page::Last { pause_until } => pause_until,
    };
    let query = &search.query;
    let numbers = walk.current.clone();
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

/// The meaning of a page for the search under way.
enum Page {
    /// This step ends here, with the search still under way or done with.
    Stays(Step),
    /// The search has no more to read, so its results are ready to save.
    Last { pause_until: Option<OffsetDateTime> },
}

/// Reads the next page of the search under way. A search read to its end stays that way, so a save that
/// failed goes again with no new request.
fn read_page(link: &Link<'_>, walk: &mut FilterWalk, search: &WalkedSearch) -> Result<Page, Error> {
    let pages = match &mut walk.place {
        Place::Read { pause_until } => {
            return Ok(Page::Last {
                pause_until: *pause_until,
            });
        }
        Place::Reading(pages) => pages,
    };
    let query = &search.query;
    let asked = FilterSearch {
        query,
        created: pages.slice.created(),
        since: search.since,
        page: PageRequest {
            size: SEARCH_PAGE,
            after: pages.cursor.as_deref(),
        },
    };
    let page = match link.github.search_filter(link.repository, &asked) {
        Ok(page) => page,
        Err(problem) if blames_the_search(&problem) => {
            log::warn!("GitHub wouldn't search {query:?}: {}", problem.message);
            link.store
                .save_filter_problem(link.repository, query, &problem.message)?;
            walk.start_the_next_search();
            return Ok(Page::Stays(Step::Saved {
                touched: Vec::new(),
                pause_until: None,
            }));
        }
        Err(problem) => return Err(problem),
    };
    let pause_until = pause_until(page.rate_limit);
    let more = Page::Stays(Step::Searched { pause_until });
    if pages.cursor.is_none() && page.total > SEARCH_CAP {
        let today = link.github.clock().now().date();
        if let Some((older, newer)) = pages.slice.halves(today) {
            pages.older.push(older);
            pages.slice = newer;
            return Ok(more);
        }
        walk.cut_off = true;
    }
    for hit in page.hits {
        walk.current.extend([hit.number]);
        drop(walk.hits.insert(hit.number, hit));
    }
    if page.next.is_some() {
        pages.cursor = page.next;
        return Ok(more);
    }
    if let Some(slice) = pages.older.pop() {
        pages.slice = slice;
        pages.cursor = None;
        return Ok(more);
    }
    walk.place = Place::Read { pause_until };
    Ok(Page::Last { pause_until })
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
        self.place = Place::default();
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
    Ok(Resumable::Fetching(Batches {
        ids,
        why: PrFetch::Work,
        leads,
    })
    .into())
}

#[cfg(test)]
mod tests;
