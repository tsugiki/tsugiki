//! A first sync after the user's own work, as one queue (ADR 0015). Whole trees of what the user needs come
//! first: their own Issues, then any Issue they open. After that, a page of a list and a batch of PRs take
//! turns, so rows arrive from the start and the PRs of the window fill in soon after their Issues. A PR older
//! than the window waits until the user opens its Issue. A small Repository also lists its open items that
//! last changed before the window, so the sync loads what the add dialog counted. One request runs at a time.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::{FETCH_BATCH, Leads, Link, Moved, SEARCH_PAGE, Step, pause_until};
use crate::error::Error;
use crate::github::issues::ListedPr;
use crate::github::pull_requests::{PageRequest, PrFetch};
use crate::github::references::IssueReferences;
use crate::github::refresh::ListPlace;
use crate::github::window::{ListRequest, Span};
use crate::links::{possible_numbers, possible_origin};
use crate::repository::RepositorySize;
use crate::status::LoadProgress;

/// GitHub's largest page. The list of PRs asks for little, so it can use it.
const PR_LIST_PAGE: usize = 100;

/// How soon a read's findings go: ahead of everything else, for a tree the user needs, or in turn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Urgency {
    Urgent,
    InTurn,
}

/// An urgent Issue with more cross-references than one page holds.
#[derive(Clone, Serialize, Deserialize)]
struct LongRead {
    number: u32,
    cursor: String,
}

/// One of a first sync's lists.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum List {
    Issues(Span),
    PullRequests(Span),
}

/// A list and how far it has got.
#[derive(Clone, Serialize, Deserialize)]
struct Walk {
    list: List,
    place: ListPlace,
}

/// The lists a first sync of a Repository this size walks, in order: the window, then a small Repository's
/// older open items, so the status bar moves from one part to the next once.
fn lists_for(size: RepositorySize) -> VecDeque<Walk> {
    let lists = match size {
        RepositorySize::Small => vec![
            List::Issues(Span::Window),
            List::PullRequests(Span::Window),
            List::Issues(Span::OlderOpen),
            List::PullRequests(Span::OlderOpen),
        ],
        RepositorySize::Large => vec![List::Issues(Span::Window), List::PullRequests(Span::Window)],
    };
    lists
        .into_iter()
        .map(|list| Walk {
            list,
            place: ListPlace::Next(None),
        })
        .collect()
}

/// The queue and where it has got to, which a first sync saves after each step.
#[derive(Serialize, Deserialize)]
pub(super) struct Loading {
    since: OffsetDateTime,
    /// Issues whose trees go before everything else: the user's own, and any they open.
    urgent_issues: Vec<u32>,
    /// Each Issue that has been in `urgent_issues`. A view on screen asks about once a minute, and one read
    /// of its tree is enough.
    opened: BTreeSet<u32>,
    /// The urgent Issues with more cross-references to read.
    long_reads: VecDeque<LongRead>,
    /// The lists still to read, the next first, once the sync has counted the Repository's open items the way
    /// the add dialog does. A count taken when the sync starts holds even if the Repository has crossed the
    /// line since it was added.
    lists: Option<VecDeque<Walk>>,
    /// The count of Issues the lists bring, while the sync knows it.
    issues_total: Option<u32>,
    issues_read: u32,
    /// The Issues this sync has read or asked for, so a PR's Issue is asked for once.
    issues_seen: BTreeSet<u32>,
    /// The PRs of `urgent_issues`, which go before `queue`.
    urgent: VecDeque<ListedPr>,
    queue: VecDeque<ListedPr>,
    /// Each PR that has ever been in either queue, so each joins once.
    queued: BTreeSet<u32>,
    /// True when a page of a list goes next, and false when a batch of PRs does.
    list_goes_next: bool,
    /// The Origin PRs of urgent PRs, which their trees need soon.
    urgent_leads: Leads,
    /// The leads of the other PRs. Most Origin PRs come through the lists anyway, so those wait for the end.
    leads: Leads,
}

/// The one request to make next.
enum Next {
    Count,
    LongRead(LongRead),
    UrgentIssues,
    UrgentOrigins,
    Batch,
    NamedIssues(Vec<u32>),
    Page(Walk),
    Origins,
}

/// The stored PRs a sync doesn't fetch again, and when each last changed.
type Frozen = BTreeMap<u32, OffsetDateTime>;

fn frozen(link: &Link<'_>) -> Result<Frozen, Error> {
    let stored = link.store.frozen_pull_requests(link.repository)?;
    Ok(stored
        .into_iter()
        .map(|pr| (pr.number, pr.updated_at))
        .collect())
}

/// The test for a cross-reference that needn't queue: a PR this sync has, or one that's frozen.
fn held<'a>(fetched: &'a BTreeSet<u32>, frozen: &'a Frozen) -> impl Fn(u32) -> bool + 'a {
    |pr| fetched.contains(&pr) || frozen.contains_key(&pr)
}

impl Loading {
    pub(super) fn new(since: OffsetDateTime, own_issues: Vec<u32>) -> Self {
        Self {
            since,
            issues_seen: own_issues.iter().copied().collect(),
            opened: own_issues.iter().copied().collect(),
            urgent_issues: own_issues,
            long_reads: VecDeque::new(),
            lists: None,
            issues_total: None,
            issues_read: 0,
            urgent: VecDeque::new(),
            queue: VecDeque::new(),
            queued: BTreeSet::new(),
            list_goes_next: true,
            urgent_leads: Leads::default(),
            leads: Leads::default(),
        }
    }

    /// Puts Issue `number`'s tree before everything else, the first time it's opened. The read takes its PRs
    /// out of the queue, even if a page has already listed them.
    pub(super) fn open_issue(&mut self, number: u32) {
        if self.opened.insert(number) {
            self.urgent_issues.insert(0, number);
        }
    }

    /// True while the user's own or opened trees are still loading.
    pub(super) fn trees_come_first(&self) -> bool {
        self.next_urgent().is_some()
    }

    pub(super) fn progress(&self, fetched: &BTreeSet<u32>) -> LoadProgress {
        LoadProgress {
            issues_read: self.issues_read,
            issues_total: self.issues_total,
            prs_fetched: fetched.len().try_into().unwrap_or(u32::MAX),
            prs_known: fetched
                .len()
                .saturating_add(self.urgent.len())
                .saturating_add(self.queue.len())
                .try_into()
                .unwrap_or(u32::MAX),
        }
    }

    /// True once the window's lists have ended and a small Repository's older open items are left.
    pub(super) fn reads_older_open_items(&self) -> bool {
        self.walks().next().is_some_and(|walk| {
            matches!(
                walk.list,
                List::Issues(Span::OlderOpen) | List::PullRequests(Span::OlderOpen)
            )
        })
    }

    /// The lists still to read, none before the count.
    fn walks(&self) -> impl Iterator<Item = &Walk> {
        self.lists.iter().flatten()
    }

    fn next(&self) -> Next {
        if self.lists.is_none() {
            return Next::Count;
        }
        self.next_urgent().unwrap_or_else(|| self.next_in_turn())
    }

    /// The next step of the user's own and opened trees, if they need one.
    fn next_urgent(&self) -> Option<Next> {
        if !self.urgent_issues.is_empty() {
            return Some(Next::UrgentIssues);
        }
        if let Some(read) = self.long_reads.front() {
            return Some(Next::LongRead(read.clone()));
        }
        if !self.urgent.is_empty() {
            return Some(Next::Batch);
        }
        (!self.urgent_leads.origins.is_empty()).then_some(Next::UrgentOrigins)
    }

    fn next_in_turn(&self) -> Next {
        // Until the lists of Issues end, a PR's Issue may still turn up on a later page.
        if !self.issues_left() {
            let named = self.named_issues_left();
            if !named.is_empty() {
                return Next::NamedIssues(named);
            }
        }
        if let Some(walk) = self.walks().next()
            && (self.list_goes_next || self.queue.is_empty())
        {
            return Next::Page(walk.clone());
        }
        if self.queue.is_empty() {
            Next::Origins
        } else {
            Next::Batch
        }
    }

    fn issues_left(&self) -> bool {
        self.walks()
            .any(|walk| matches!(walk.list, List::Issues(_)))
    }

    fn named_issues_left(&self) -> Vec<u32> {
        let left = self.leads.named.difference(&self.issues_seen);
        left.take(FETCH_BATCH).copied().collect()
    }

    /// Moves the list just read on to `next`, or drops it once it ends.
    fn move_list_on(&mut self, next: ListPlace) {
        let lists = self.lists.get_or_insert_with(VecDeque::new);
        let going_on = lists.pop_front().filter(|_| !next.is_finished());
        if let Some(walk) = going_on {
            lists.push_front(Walk {
                place: next,
                ..walk
            });
        }
        self.list_goes_next = false;
    }

    /// Counts a page of Issues. GitHub gives the window's total, but can't count the older open Issues, so
    /// while those are left there's no total until the last Issue list ends.
    fn count_issues(&mut self, read: usize, window_total: Option<u32>) {
        self.issues_read = self
            .issues_read
            .saturating_add(read.try_into().unwrap_or(u32::MAX));
        let older_left = self
            .walks()
            .any(|walk| walk.list == List::Issues(Span::OlderOpen));
        self.issues_total = if !self.issues_left() {
            Some(self.issues_read)
        } else if older_left {
            None
        } else {
            window_total
        };
    }

    /// Queues `listed`, unless this sync has it or it's frozen. An urgent PR already queued moves up.
    fn join(&mut self, listed: Vec<ListedPr>, urgency: Urgency, have: &dyn Fn(u32) -> bool) {
        for pr in listed {
            if have(pr.number) {
                continue;
            }
            if self.queued.insert(pr.number) {
                match urgency {
                    Urgency::Urgent => self.urgent.push_back(pr),
                    Urgency::InTurn => self.queue.push_back(pr),
                }
            } else if urgency == Urgency::Urgent {
                let at = self
                    .queue
                    .iter()
                    .position(|waiting| waiting.number == pr.number);
                self.urgent.extend(at.and_then(|at| self.queue.remove(at)));
            }
        }
    }

    /// Queues the whole of the trees the user needs, and goes on to read any older cross-references.
    fn join_urgent_references(
        &mut self,
        references: Vec<IssueReferences>,
        have: &dyn Fn(u32) -> bool,
    ) {
        for read in references {
            if let Some(cursor) = read.next {
                self.long_reads.push_front(LongRead {
                    number: read.number,
                    cursor,
                });
            }
            self.join(read.listed, Urgency::Urgent, have);
        }
    }

    /// Queues what a page's Issues list inside the window, so those PRs arrive soon after their Issues. The
    /// list of PRs would bring the same PRs later. An older PR waits until the user needs its tree, and older
    /// cross-references could only add more of those.
    fn join_window_references(
        &mut self,
        references: Vec<IssueReferences>,
        have: &dyn Fn(u32) -> bool,
    ) {
        for read in references {
            let since = self.since;
            let recent = read.listed.into_iter().filter(|pr| pr.updated_at >= since);
            self.join(recent.collect(), Urgency::InTurn, have);
        }
    }
}

/// Makes the next request of `loading`, or says the sync is finished.
pub(super) fn load(
    link: &Link<'_>,
    loading: &mut Loading,
    fetched: &mut BTreeSet<u32>,
) -> Result<Moved, Error> {
    let step = match loading.next() {
        Next::Count => count(link, loading)?,
        Next::LongRead(read) => read_long(link, loading, read, fetched)?,
        Next::UrgentIssues => read_urgent_issues(link, loading, fetched)?,
        Next::UrgentOrigins => load_urgent_origin(link, loading, fetched)?,
        Next::Batch => fetch_batch(link, loading, fetched)?,
        Next::NamedIssues(numbers) => fetch_named_issues(link, loading, &numbers)?,
        Next::Page(walk) => match walk.list {
            List::Issues(span) => read_issue_page(link, loading, fetched, (span, &walk.place))?,
            List::PullRequests(span) => {
                read_pull_request_page(link, loading, fetched, (span, &walk.place))?
            }
        },
        Next::Origins => load_origins(link, loading, fetched)?,
    };
    Ok((step, None))
}

/// Counts the Repository's open items with the add dialog's own query, which decides the lists to read.
fn count(link: &Link<'_>, loading: &mut Loading) -> Result<Step, Error> {
    let today = OffsetDateTime::now_utc().date();
    let check = link.github.check_repository(link.repository, today)?;
    loading.lists = Some(lists_for(check.size));
    Ok(Step::Searched { pause_until: None })
}

fn read_long(
    link: &Link<'_>,
    loading: &mut Loading,
    read: LongRead,
    fetched: &BTreeSet<u32>,
) -> Result<Step, Error> {
    let asked = PageRequest {
        size: SEARCH_PAGE,
        after: Some(&read.cursor),
    };
    let page = link
        .github
        .fetch_cross_references(link.repository, read.number, asked)?;
    let frozen = frozen(link)?;
    loading.join(page.listed, Urgency::Urgent, &held(fetched, &frozen));
    drop(loading.long_reads.pop_front());
    if let Some(cursor) = page.next {
        let number = read.number;
        loading.long_reads.push_front(LongRead { number, cursor });
    }
    Ok(Step::Searched {
        pause_until: pause_until(page.rate_limit),
    })
}

fn read_urgent_issues(
    link: &Link<'_>,
    loading: &mut Loading,
    fetched: &BTreeSet<u32>,
) -> Result<Step, Error> {
    let read =
        link.github
            .fetch_issue_references(link.repository, &loading.urgent_issues, FETCH_BATCH)?;
    let frozen = frozen(link)?;
    loading.join_urgent_references(read.references, &held(fetched, &frozen));
    drop(loading.urgent_issues.drain(..read.asked_for));
    Ok(Step::Searched {
        pause_until: pause_until(read.rate_limit),
    })
}

/// Fetches the next PRs, urgent ones first, and saves them.
fn fetch_batch(
    link: &Link<'_>,
    loading: &mut Loading,
    fetched: &mut BTreeSet<u32>,
) -> Result<Step, Error> {
    let waiting = loading.urgent.iter().chain(loading.queue.iter());
    let ids: Vec<_> = waiting.take(FETCH_BATCH).map(|pr| pr.id.clone()).collect();
    let batch = link
        .github
        .fetch_pull_requests(PrFetch::Tree, &ids, FETCH_BATCH)?;
    link.store.save_fetched_pull_requests(
        link.repository,
        batch.pull_requests.clone(),
        batch.activity,
    )?;
    let urgent_asked = loading.urgent.len().min(batch.asked_for);
    let urgent: BTreeSet<u32> = loading
        .urgent
        .drain(..urgent_asked)
        .map(|pr| pr.number)
        .collect();
    let queued_asked = batch.asked_for.saturating_sub(urgent_asked);
    fetched.extend(urgent.iter().copied());
    fetched.extend(loading.queue.drain(..queued_asked).map(|pr| pr.number));
    for pull_request in &batch.pull_requests {
        let origin = possible_origin(pull_request);
        loading
            .leads
            .named
            .extend(possible_numbers(pull_request, link.repository));
        if urgent.contains(&pull_request.number) {
            loading.urgent_leads.origins.extend(origin);
        } else {
            loading.leads.origins.extend(origin);
        }
    }
    loading.list_goes_next = true;
    let touched = batch.pull_requests.iter().map(|pr| pr.number).collect();
    Ok(Step::saved_if_any(touched, pause_until(batch.rate_limit)))
}

/// Fetches the Issues that PRs name and the lists haven't brought: a fresh PR on an old Issue doesn't move
/// the Issue's `updatedAt`, and with no Issue the PR would show as a Standalone PR.
fn fetch_named_issues(
    link: &Link<'_>,
    loading: &mut Loading,
    numbers: &[u32],
) -> Result<Step, Error> {
    let batch = link
        .github
        .fetch_issues(link.repository, numbers, FETCH_BATCH)?;
    let touched = batch.issues.iter().map(|issue| issue.number).collect();
    link.store
        .save_fetched_issues(link.repository, batch.issues, batch.activity)?;
    loading
        .issues_seen
        .extend(numbers.iter().take(batch.asked_for).copied());
    Ok(Step::saved_if_any(touched, pause_until(batch.rate_limit)))
}

fn read_issue_page(
    link: &Link<'_>,
    loading: &mut Loading,
    fetched: &BTreeSet<u32>,
    (span, place): (Span, &ListPlace),
) -> Result<Step, Error> {
    let asked = ListRequest {
        span,
        since: loading.since,
        place,
        size: SEARCH_PAGE,
    };
    let page = link.github.list_issues(link.repository, asked)?;
    let frozen = frozen(link)?;
    let numbers: Vec<u32> = page.items.iter().map(|issue| issue.number).collect();
    link.store
        .save_fetched_issues(link.repository, page.items, page.activity)?;
    loading.join_window_references(page.references, &held(fetched, &frozen));
    loading.issues_seen.extend(numbers.iter().copied());
    loading.move_list_on(page.next);
    loading.count_issues(numbers.len(), page.total);
    Ok(Step::saved_if_any(numbers, pause_until(page.rate_limit)))
}

/// Reads a page of PRs, and queues each one the Issues haven't brought. A frozen PR goes back in only if it
/// moved, because a merged PR's timeline goes on.
fn read_pull_request_page(
    link: &Link<'_>,
    loading: &mut Loading,
    fetched: &BTreeSet<u32>,
    (span, place): (Span, &ListPlace),
) -> Result<Step, Error> {
    let asked = ListRequest {
        span,
        since: loading.since,
        place,
        size: PR_LIST_PAGE,
    };
    let page = link.github.list_pull_requests(link.repository, asked)?;
    let frozen = frozen(link)?;
    let moved = page
        .items
        .into_iter()
        .filter(|pr| {
            frozen
                .get(&pr.number)
                .is_none_or(|held| *held < pr.updated_at)
        })
        .map(|pr| ListedPr {
            id: pr.id,
            number: pr.number,
            updated_at: pr.updated_at,
        })
        .collect();
    loading.join(moved, Urgency::InTurn, &|pr| fetched.contains(&pr));
    loading.move_list_on(page.next);
    Ok(Step::Searched {
        pause_until: pause_until(page.rate_limit),
    })
}

/// Fetches the next Origin PR an urgent PR needs. A frozen one stays as it's stored.
fn load_urgent_origin(
    link: &Link<'_>,
    loading: &mut Loading,
    fetched: &mut BTreeSet<u32>,
) -> Result<Step, Error> {
    loading.urgent_leads.settle_origins(link, fetched)?;
    let named = std::mem::take(&mut loading.urgent_leads.named);
    loading.leads.named.extend(named);
    let Some(number) = loading.urgent_leads.origins.first().copied() else {
        return Ok(Step::Moved);
    };
    load_origin(link, loading, number, fetched)
}

/// Fetches the next Origin PR no list brought, or finishes the sync with nothing left. Settling can name more
/// Issues, and those go first.
fn load_origins(
    link: &Link<'_>,
    loading: &mut Loading,
    fetched: &mut BTreeSet<u32>,
) -> Result<Step, Error> {
    loading.leads.settle_origins(link, fetched)?;
    if let Some(number) = loading.leads.origins.first().copied() {
        return load_origin(link, loading, number, fetched);
    }
    if loading.named_issues_left().is_empty() {
        Ok(Step::Finished)
    } else {
        Ok(Step::Moved)
    }
}

/// Fetches Origin PR `number` by itself, because only its number is known, and saves it.
fn load_origin(
    link: &Link<'_>,
    loading: &mut Loading,
    number: u32,
    fetched: &mut BTreeSet<u32>,
) -> Result<Step, Error> {
    let answer = link
        .github
        .fetch_origin_pull_request(link.repository, number)?;
    let pause_until = pause_until(answer.rate_limit);
    let step = if let Some((pull_request, activity)) = answer.pull_request {
        let named = possible_numbers(&pull_request, link.repository);
        link.store.save_fetched_pull_requests(
            link.repository,
            vec![pull_request],
            vec![activity],
        )?;
        loading.leads.named.extend(named);
        Step::Saved {
            touched: vec![number],
            pause_until,
        }
    } else {
        Step::Searched { pause_until }
    };
    loading
        .urgent_leads
        .origins
        .retain(|origin| *origin != number);
    loading.leads.origins.retain(|origin| *origin != number);
    loading.queue.retain(|pr| pr.number != number);
    fetched.extend([number]);
    Ok(step)
}

#[cfg(test)]
mod tests;
