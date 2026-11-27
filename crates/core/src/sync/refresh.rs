//! A sync after the first (ADR 0015). It asks what changed since the last one, fetches that, and probes the
//! user's open PRs for what changes without moving `updatedAt`. It then joins a first sync's later phases, so
//! each Issue it found changed or newly reached gets its tree read. Before it finishes, it fetches the
//! Issues that the PRs it holds name and it lacks (#980), then reads the Conversation of each changed
//! PR in Reach, so the PR view opens on its Reviews (#157).

use std::collections::{BTreeMap, BTreeSet};

use time::OffsetDateTime;

use super::filters::{FilterWalk, RefreshWalk, blames_the_search, can_narrow};
use super::{Ahead, Asked, FETCH_BATCH, Link, Moved, Phase, Resumable, SEARCH_PAGE, Step};
use crate::conventions::Conventions;
use crate::error::Error;
use crate::facts::activity::ItemActivity;
use crate::facts::issues::IssueFacts;
use crate::facts::pull_requests::{NodeId, PrState, PullRequestFacts};
use crate::filters::FilterQuery;
use crate::github::filters::{FilterHit, FilterPage};
use crate::github::pull_requests::PrFetch;
use crate::github::refresh::{ChangedPr, Etag, ListCheck, ListPlace, PollRequest};
use crate::links::{BackportKind, MergeCommits, backport_of};
use crate::login::Login;
use crate::repository::{OpenItems, RepositorySize};
use crate::repository_settings::RepositorySettings;
use crate::store::{ChangeCheck, NarrowedResults, OVERLAP, SyncState};
use crate::work::{conversations_to_read, reasons};

/// A refresh's poll: one request, and more only while a list has more that changed.
pub(super) struct Poll {
    since: OffsetDateTime,
    issues: ListPlace,
    pull_requests: ListPlace,
    /// The searches the next request runs. A request carries each once.
    in_request: Vec<FilterQuery>,
    /// The searches that walk on their own after this, narrowed to `since`.
    narrowed: Vec<FilterQuery>,
    /// The searches that walk on their own after this, whole.
    whole: Vec<FilterQuery>,
    /// The list's tag from before this poll, so a change GitHub makes during the poll moves the next check.
    tag: Option<Etag>,
    /// A change after this is too new for the search to have caught up.
    settled_before: OffsetDateTime,
    /// A refresh before this one stopped partway, so its saved facts may have lost the work they lead to.
    redo: bool,
    /// Each item this poll found updated, once the search can have caught up.
    changed: BTreeSet<u32>,
    answers: Vec<(FilterQuery, FilterPage)>,
    changed_prs: Vec<ChangedPr>,
    /// The Issues this poll changed, which need their trees read.
    moved_issues: BTreeSet<u32>,
    /// The items the searches merged so far found.
    hits: BTreeMap<u32, FilterHit>,
    /// The open items the last page counted, which the poll saves once it has read every page (#534).
    count: Option<OpenItems>,
}

impl Poll {
    fn has_read_everything(&self) -> bool {
        self.issues.is_finished() && self.pull_requests.is_finished()
    }
}

impl Default for Poll {
    fn default() -> Self {
        Self {
            since: OffsetDateTime::UNIX_EPOCH,
            settled_before: OffsetDateTime::UNIX_EPOCH,
            redo: false,
            issues: ListPlace::Next(None),
            pull_requests: ListPlace::Next(None),
            in_request: Vec::new(),
            narrowed: Vec::new(),
            whole: Vec::new(),
            tag: None,
            changed: BTreeSet::new(),
            answers: Vec::new(),
            changed_prs: Vec::new(),
            moved_issues: BTreeSet::new(),
            hits: BTreeMap::new(),
            count: None,
        }
    }
}

/// The PRs a poll found changed, with the walk that follows them.
pub(super) struct Changed {
    ids: Vec<NodeId>,
    walk: FilterWalk,
}

/// The user's open PRs left to probe, with the walk that follows them.
pub(super) struct Probing {
    numbers: Vec<u32>,
    walk: FilterWalk,
}

/// A later sync's first phase. A Repository with no finished sync gets `None`, and so a whole sync.
pub(super) fn first_phase(
    link: &Link<'_>,
    check: &mut ChangeCheck,
) -> Result<Option<Phase>, Error> {
    let Some(state) = link.store.sync_state(link.repository)? else {
        return Ok(None);
    };
    link.store
        .keep_only_filters(link.repository, link.keep_filters.clone())?;
    let stored = link.store.filter_results(link.repository)?;
    let (in_request, whole): (Vec<FilterQuery>, Vec<FilterQuery>) = link
        .filters
        .iter()
        .cloned()
        .partition(|query| can_narrow(stored.get(query)));
    let poll = Poll {
        since: state.changes_since(),
        settled_before: link.github.clock().now() - OVERLAP,
        redo: state.refresh_under_way,
        in_request,
        whole,
        ..Poll::default()
    };
    let next = match &state.check {
        ChangeCheck::Skipped => Phase::Polling(poll),
        ChangeCheck::Untagged | ChangeCheck::Tagged(_) => Phase::CheckingList(poll),
    };
    check.clone_from(&state.check);
    let under_way = SyncState {
        refresh_under_way: true,
        ..state
    };
    link.store.save_sync_state(link.repository, under_way)?;
    Ok(Some(next))
}

/// Asks whether the list moved since its tag. After a 304 only the probe and any new search are left.
pub(super) fn check_list(
    link: &Link<'_>,
    poll: &mut Poll,
    check: &ChangeCheck,
) -> Result<Moved, Error> {
    let stored_tag = match check {
        ChangeCheck::Tagged(tag) => Some(tag),
        ChangeCheck::Skipped | ChangeCheck::Untagged => None,
    };
    let next = match link.github.check_list(link.repository, stored_tag)? {
        ListCheck::Same => {
            // Nothing to narrow, but the walk still reads the tree of each Pinned issue that no sync has
            // read since the pin (#488).
            let walk = FilterWalk::of_refresh(RefreshWalk {
                whole: std::mem::take(&mut poll.whole),
                ..RefreshWalk::default()
            });
            Phase::FetchingChanged(Changed {
                ids: Vec::new(),
                walk,
            })
        }
        ListCheck::Moved(tag) => {
            poll.tag = tag;
            Phase::Polling(std::mem::take(poll))
        }
    };
    Ok((Step::Moved, Some(next)))
}

/// Reads the next page of what changed, then saves the open items the last page counted, then merges each
/// search that answered, and moves on after the last. Each step saves one thing and tells the window of it,
/// so a save that fails holds back no news. A failed step keeps its place in `poll`, so the next try picks
/// up after the part that worked.
pub(super) fn poll(
    link: &Link<'_>,
    poll: &mut Poll,
    check: &mut ChangeCheck,
) -> Result<Moved, Error> {
    if !poll.has_read_everything() {
        return Ok((read_page(link, poll, check)?, None));
    }
    if let Some(open) = poll.count {
        let recounted = (link.store).save_open_items(link.repository, open)?;
        poll.count = None;
        let step = if recounted {
            Step::Recounted
        } else {
            Step::Moved
        };
        return Ok((step, None));
    }
    if let Some(merged) = merge_next_answer(link, poll)? {
        return Ok((Step::saved_if_any(merged), None));
    }
    let next = Phase::FetchingChanged(plan_the_rest(link, poll)?);
    Ok((Step::Moved, Some(next)))
}

/// Asks for the next page and saves it. One Filter GitHub refuses shouldn't cost the rest, so a refused
/// request goes again without the searches, and each of them walks alone.
fn read_page(link: &Link<'_>, poll: &mut Poll, check: &mut ChangeCheck) -> Result<Step, Error> {
    let asked = PollRequest {
        since: poll.since,
        issues: &poll.issues,
        pull_requests: &poll.pull_requests,
        filters: &poll.in_request,
        size: SEARCH_PAGE,
    };
    let answer = match link.github.poll_repository(link.repository, &asked) {
        Ok(answer) => answer,
        Err(problem) if blames_the_search(&problem) && !poll.in_request.is_empty() => {
            log::warn!(
                "GitHub refused a poll with its searches: {}",
                problem.message
            );
            poll.narrowed.append(&mut poll.in_request);
            return Ok(Step::Moved);
        }
        Err(problem) => return Err(problem),
    };
    let touched = save_moved_issues(link, (answer.issues, answer.issue_activity), poll)?;
    poll.count = Some(answer.open);
    poll.in_request.clear();
    poll.answers.extend(answer.filters);
    let settled = answer
        .pull_requests
        .iter()
        .filter(|pr| pr.updated_at <= poll.settled_before);
    poll.changed.extend(settled.map(|pr| pr.number));
    poll.changed_prs.extend(answer.pull_requests);
    poll.issues = answer.next_issues;
    poll.pull_requests = answer.next_pull_requests;
    *check = match (RepositorySize::of(answer.open.total()), poll.tag.clone()) {
        (RepositorySize::Large, _) => ChangeCheck::Skipped,
        (RepositorySize::Small, None) => ChangeCheck::Untagged,
        (RepositorySize::Small, Some(tag)) => ChangeCheck::Tagged(tag),
    };
    Ok(Step::saved_if_any(touched))
}

/// Saves each Issue Tsugiki holds that this answer changes. An Issue it doesn't hold isn't on any screen, and
/// a Filter that reaches one brings it later. Answers with the Issues it saved.
fn save_moved_issues(
    link: &Link<'_>,
    (issues, activity): (Vec<IssueFacts>, Vec<ItemActivity>),
    poll: &mut Poll,
) -> Result<Vec<u32>, Error> {
    let held = link.store.issues(link.repository)?;
    let held: BTreeMap<u32, IssueFacts> = held
        .into_iter()
        .map(|issue| (issue.number, issue))
        .collect();
    let settled: Vec<u32> = issues
        .iter()
        .filter(|issue| issue.updated_at <= poll.settled_before)
        .map(|issue| issue.number)
        .collect();
    let in_the_window: Vec<u32> = issues
        .iter()
        .map(|issue| issue.number)
        .filter(|number| held.contains_key(number))
        .collect();
    let moved: Vec<IssueFacts> = issues
        .into_iter()
        .filter(|issue| held.get(&issue.number).is_some_and(|old| old != issue))
        .collect();
    let touched: Vec<u32> = moved.iter().map(|issue| issue.number).collect();
    let activity = activity
        .into_iter()
        .filter(|item| touched.contains(&item.number))
        .collect();
    link.store
        .save_fetched_issues(link.repository, moved, activity)?;
    poll.changed.extend(settled);
    if poll.redo {
        poll.moved_issues.extend(in_the_window);
    }
    poll.moved_issues.extend(touched.iter().copied());
    Ok(touched)
}

/// Merges the next search that answered whole, and answers with the items that moved, or `None` with no
/// search left. A search with more pages walks on its own instead.
fn merge_next_answer(link: &Link<'_>, poll: &mut Poll) -> Result<Option<Vec<u32>>, Error> {
    while let Some((query, page)) = poll.answers.first() {
        if page.next.is_none() {
            let results = NarrowedResults {
                changed: poll.changed.clone(),
                found: page.hits.iter().map(|hit| hit.number).collect(),
                cut_off: false,
            };
            let moved = link
                .store
                .merge_filter_results(link.repository, query, results)?;
            let (_, page) = poll.answers.remove(0);
            poll.hits
                .extend(page.hits.into_iter().map(|hit| (hit.number, hit)));
            return Ok(Some(moved.into_iter().collect()));
        }
        let (query, _) = poll.answers.remove(0);
        poll.narrowed.push(query);
    }
    Ok(None)
}

/// Plans what's left after the poll. It asks the store first, so a failure leaves `poll` as it was.
fn plan_the_rest(link: &Link<'_>, poll: &mut Poll) -> Result<Changed, Error> {
    let stored = stored_pull_requests(link)?;
    let viewer = if poll.redo {
        link.store.last_login()?
    } else {
        None
    };
    // A merged PR stays merged, but its timeline goes on: the bot's comment that a Bot backport failed can
    // land after a sync has fetched the merge.
    let is_behind = |pr: &ChangedPr| {
        stored
            .get(&pr.number)
            .is_none_or(|held| held.updated_at < pr.updated_at)
    };
    let ids = poll.changed_prs.iter().filter(|pr| is_behind(pr));
    let ids: Vec<NodeId> = ids.map(|pr| pr.id.clone()).collect();
    let mut walk = FilterWalk::of_refresh(RefreshWalk {
        whole: std::mem::take(&mut poll.whole),
        narrowed: std::mem::take(&mut poll.narrowed),
        since: Some(poll.since),
        changed: std::mem::take(&mut poll.changed),
        hits: std::mem::take(&mut poll.hits),
    });
    walk.hold_issues(std::mem::take(&mut poll.moved_issues));
    if poll.redo {
        // The refresh that stopped may have saved these, so the fetch can't tell they changed. It owed
        // their Conversations in the store as it saved them.
        let held = poll
            .changed_prs
            .iter()
            .filter_map(|pr| stored.get(&pr.number));
        for pull_request in held.filter(|pr| is_work(pr, viewer.as_ref())) {
            walk.learn_from(pull_request, link.repository);
        }
    }
    Ok(Changed { ids, walk })
}

/// True when `pull_request` is on one of the user's own lists.
fn is_work(pull_request: &PullRequestFacts, viewer: Option<&Login>) -> bool {
    viewer.is_some_and(|viewer| !reasons(pull_request, viewer).is_empty())
}

fn stored_pull_requests(link: &Link<'_>) -> Result<BTreeMap<u32, PullRequestFacts>, Error> {
    let stored = link.store.pull_requests(link.repository)?;
    Ok(stored.into_iter().map(|pr| (pr.number, pr)).collect())
}

/// Fetches the next batch of what changed. A stranger's PR only needs saving here: the refresh fetches the
/// Issue it names before it finishes. The user's own work also needs what it points at, as a first sync
/// fetches it.
pub(super) fn fetch_changed(
    link: &Link<'_>,
    changed: &mut Changed,
    fetched: &mut BTreeSet<u32>,
) -> Result<Moved, Error> {
    if changed.ids.is_empty() {
        let next = Probing {
            numbers: tracked_pull_requests(link, fetched)?,
            walk: std::mem::take(&mut changed.walk),
        };
        return Ok((Step::Moved, Some(Phase::Probing(next))));
    }
    let batch = link
        .github
        .fetch_pull_requests(PrFetch::Changed, &changed.ids, FETCH_BATCH)?;
    let stored = stored_pull_requests(link)?;
    let viewer = link.store.last_login()?;
    // The store keeps a read older than its own, from a replica of GitHub's that lags, out.
    let moved: Vec<&PullRequestFacts> = (batch.pull_requests.iter())
        .filter(|pr| {
            stored
                .get(&pr.number)
                .is_none_or(|kept| kept != *pr && kept.updated_at <= pr.updated_at)
        })
        .collect();
    let touched: Vec<u32> = moved.iter().map(|pr| pr.number).collect();
    link.store.save_changed_pull_requests(
        link.repository,
        (batch.pull_requests.clone(), batch.activity),
        touched.clone(),
    )?;
    fetched.extend(batch.pull_requests.iter().map(|pr| pr.number));
    for pull_request in moved.into_iter().filter(|pr| is_work(pr, viewer.as_ref())) {
        changed.walk.learn_from(pull_request, link.repository);
    }
    drop(changed.ids.drain(..batch.asked_for));
    Ok((Step::saved_if_any(touched), None))
}

/// The open PRs on the user's own lists, and the open Bot backports of what the user wrote or reviewed, that
/// this refresh hasn't just fetched. A Bot backport's failed CI is New activity on its Origin PR, and CI
/// doesn't move `updatedAt`.
fn tracked_pull_requests(link: &Link<'_>, fetched: &BTreeSet<u32>) -> Result<Vec<u32>, Error> {
    let Some(viewer) = link.store.last_login()? else {
        return Ok(Vec::new());
    };
    let stored = link.store.pull_requests(link.repository)?;
    let conventions = Conventions::for_repository(link.repository);
    let merges = MergeCommits::of(&stored);
    let is_the_users_backport = |pr: &PullRequestFacts| {
        let link = backport_of(pr, conventions, &merges);
        let origin = link.filter(|link| link.kind == BackportKind::Bot);
        let origin = origin.and_then(|link| stored.iter().find(|held| held.number == link.origin));
        pr.state == PrState::Open
            && origin.is_some_and(|origin| origin.is_written_or_reviewed_by(&viewer))
    };
    let tracked = stored.iter().filter(|pr| {
        !fetched.contains(&pr.number)
            && (!reasons(pr, &viewer).is_empty() || is_the_users_backport(pr))
    });
    Ok(tracked.map(|pr| pr.number).collect())
}

/// Asks which checks failed on a batch of the user's open PRs whose CI failed since Tsugiki last asked. A
/// probe has read their CI. Any more wait for the next refresh. A name only fills out a preview, so
/// GitHub refusing this question leaves the rest of the refresh to run: the PR stays unread, and the next
/// refresh asks again.
fn read_failed_checks(link: &Link<'_>) -> Result<Step, Error> {
    let Some(viewer) = link.store.last_login()? else {
        return Ok(Step::Moved);
    };
    let numbers = link
        .store
        .pull_requests_with_unread_checks(link.repository, &viewer)?;
    if numbers.is_empty() {
        return Ok(Step::Moved);
    }
    let asked = link
        .github
        .failed_checks(link.repository, &numbers, FETCH_BATCH);
    let read = match asked {
        Ok(read) => read,
        Err(problem) if blames_the_search(&problem) => {
            log::warn!(
                "GitHub wouldn't say which checks failed: {}",
                problem.message
            );
            return Ok(Step::Moved);
        }
        Err(problem) => return Err(problem),
    };
    let touched = read.found.iter().map(|(number, _)| *number).collect();
    link.store.save_failed_checks(link.repository, read.found)?;
    Ok(Step::saved_if_any(touched))
}

/// Probes the next batch of the user's open PRs, for the CI and Reviews that don't move `updatedAt`.
pub(super) fn probe(link: &Link<'_>, probing: &mut Probing) -> Result<Moved, Error> {
    if probing.numbers.is_empty() {
        let read = read_failed_checks(link)?;
        let walk = std::mem::take(&mut probing.walk);
        return Ok((read, Some(Resumable::SearchingFilters(walk).into())));
    }
    let batch = link
        .github
        .probe_pull_requests(link.repository, &probing.numbers, FETCH_BATCH)?;
    let stored = stored_pull_requests(link)?;
    let updated: Vec<PullRequestFacts> = batch
        .probes
        .into_iter()
        .filter_map(|probe| {
            let held = stored.get(&probe.number)?;
            let after = probe.applied_to(held);
            (after != *held).then_some(after)
        })
        .collect();
    let touched: Vec<u32> = updated.iter().map(|pr| pr.number).collect();
    link.store.save_changed_pull_requests(
        link.repository,
        (updated, batch.activity),
        touched.clone(),
    )?;
    drop(probing.numbers.drain(..batch.asked_for));
    Ok((Step::saved_if_any(touched), None))
}

/// Fetches the next batch of the Issues that the PRs Tsugiki holds name under the Repository's
/// `conventions`, as their links count them, and it lacks, whatever the state of each PR, so a merged PR
/// shows under its Issue and not as a Standalone PR (#980). A refresh saves a stranger's PR alone, and a PR
/// that merged before it became the user's work doesn't change again, so this goes by what the store holds,
/// which also mends a store that lacked the Issue from before. A saved Issue is an ordinary item: the Inbox
/// lists it, and the usual rules say if it Needs attention. It asks the store again before each batch and
/// finishes once none are left. The store keeps each number that will never name an Issue, so the next
/// refresh doesn't ask for it again; one GitHub doesn't find is asked again at the next refresh.
///
/// A number GitHub refuses alone, while it answers the others, waits for the next refresh, and the rest of
/// the batch is saved. A batch GitHub refuses as a whole is logged and waits for the next refresh too, and
/// only trouble that speaks for all of GitHub stops this one, as in `read_ahead`.
pub(super) fn fetch_lacking_issues(
    link: &Link<'_>,
    asked: &mut Asked,
    conventions: Conventions,
) -> Result<Moved, Error> {
    let lacking = link
        .store
        .named_issues_lacking(link.repository, conventions)?;
    let numbers: Vec<u32> = lacking.difference(&asked.numbers).copied().collect();
    if numbers.is_empty() {
        return Ok((Step::Finished, None));
    }
    let batch = match link
        .github
        .fetch_issues(link.repository, &numbers, FETCH_BATCH)
    {
        Ok(batch) => batch,
        Err(problem) if !problem.kind.speaks_for_all_of_github() => {
            // The failure doesn't say how far a retry after a 502 shrank the request, so the whole batch
            // waits for the next refresh.
            let batch: Vec<u32> = numbers.into_iter().take(FETCH_BATCH).collect();
            log::warn!(
                "GitHub wouldn't give the Issues {batch:?}: {}",
                problem.message
            );
            asked.numbers.extend(batch);
            return Ok((Step::Searched, None));
        }
        Err(problem) => return Err(problem),
    };
    let touched: Vec<u32> = batch.issues.iter().map(|issue| issue.number).collect();
    link.store.save_issues_and_issueless(
        link.repository,
        (batch.issues, batch.activity),
        batch.issueless,
    )?;
    // Each number the answer covers is asked, the refused ones too, which `fetch_issues` has logged.
    asked
        .numbers
        .extend(numbers.into_iter().take(batch.asked_for));
    Ok((Step::saved_if_any(touched), None))
}

/// The Conversations the rules want read ahead under `repository`'s settings, from what the store holds
/// now. With no read owed, reading the facts would cost a read of the whole Repository for nothing.
fn to_read_ahead(link: &Link<'_>, repository: &RepositorySettings) -> Result<BTreeSet<u32>, Error> {
    let owed = link.store.read_ahead_owed(link.repository)?;
    if owed.is_empty() {
        return Ok(owed);
    }
    let facts = link.store.work_facts(link.repository)?;
    Ok(conversations_to_read(&facts, &owed, repository))
}

/// Reads the Conversation of the next PR that the rules want read ahead and `ahead` hasn't tried, with its
/// Reviews and their threads: about a point each. Finishes once none are left. It asks the rules before each
/// read, so a PR that leaves Reach, or whose Conversation the PR view read meanwhile, isn't read. A store
/// that can't answer fails the step, which the queue retries like any other, so the refresh doesn't finish,
/// and forget what the store owes, on a set it couldn't read. Trouble with one PR leaves the rest to read,
/// and the PR view reads that one when it opens. Only trouble that speaks for all of GitHub stops the
/// refresh.
pub(super) fn read_ahead(
    link: &Link<'_>,
    ahead: &mut Ahead,
    repository: &RepositorySettings,
) -> Result<Moved, Error> {
    let wanted = to_read_ahead(link, repository)?;
    let Some(number) = wanted.difference(&ahead.tried).next().copied() else {
        return Ok((Step::Finished, None));
    };
    let step = match link.github.pull_request_content(link.repository, number) {
        Ok(read) => {
            link.store.save_pull_request_content(
                link.repository,
                number,
                (read.content, read.updated_at),
            )?;
            Step::Saved {
                touched: vec![number],
            }
        }
        Err(problem) if !problem.kind.speaks_for_all_of_github() => {
            log::warn!(
                "GitHub wouldn't give PR {number}'s Conversation: {}",
                problem.message
            );
            Step::Searched
        }
        Err(problem) => return Err(problem),
    };
    ahead.tried.extend([number]);
    Ok((step, None))
}

#[cfg(test)]
mod tests;
