//! A sync after the first (ADR 0015). It asks what changed since the last one, fetches that, and probes the
//! user's open PRs for what changes without moving `updatedAt`. It then joins a first sync's later phases, so
//! each Issue it found changed or newly reached gets its tree read.

use std::collections::{BTreeMap, BTreeSet};

use time::{Duration, OffsetDateTime};

use super::filters::{FilterWalk, RefreshWalk, blames_the_search, can_narrow};
use super::{FETCH_BATCH, Link, Moved, NO_PAUSE, Phase, SEARCH_PAGE, Step, pause_until};
use crate::error::Error;
use crate::filters::FilterQuery;
use crate::github::Login;
use crate::github::filters::{FilterHit, FilterPage};
use crate::github::issues::IssueFacts;
use crate::github::pull_requests::{NodeId, PrFetch, PullRequestFacts};
use crate::github::refresh::{ChangedPr, Etag, ListCheck, ListPlace, PollRequest};
use crate::repository::SMALL_UP_TO;
use crate::store::{ChangeCheck, NarrowedResults, SyncState};
use crate::workbench::reasons;

/// GitHub's search can lag behind what it changed, so a refresh also asks about the minutes before the last
/// one (ADR 0015). The fetches skip what Tsugiki already holds. A change this new can't yet say an item left
/// a Filter, and the next refresh still sees it.
const OVERLAP: Duration = Duration::minutes(2);

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
        .keep_only_filters(link.repository, link.filters.to_vec())?;
    let stored = link.store.filter_results(link.repository)?;
    let (in_request, whole): (Vec<FilterQuery>, Vec<FilterQuery>) = link
        .filters
        .iter()
        .cloned()
        .partition(|query| can_narrow(stored.get(query)));
    let poll = Poll {
        since: state.started_at - OVERLAP,
        settled_before: OffsetDateTime::now_utc() - OVERLAP,
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
    let next = match link.github.check_list(link.repository, stored_tag) {
        ListCheck::Same => {
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
    Ok((NO_PAUSE, Some(next)))
}

/// Reads the next page of what changed, and moves on after the last. One Filter GitHub refuses mustn't stop
/// the rest, so a refused request goes again without the searches, and each of them walks alone.
pub(super) fn poll(
    link: &Link<'_>,
    poll: &mut Poll,
    check: &mut ChangeCheck,
) -> Result<Moved, Error> {
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
            return Ok((NO_PAUSE, None));
        }
        Err(problem) => return Err(problem),
    };
    let mut touched = save_moved_issues(link, answer.issues, poll)?;
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
    *check = match (answer.open_items <= SMALL_UP_TO, poll.tag.clone()) {
        (false, _) => ChangeCheck::Skipped,
        (true, None) => ChangeCheck::Untagged,
        (true, Some(tag)) => ChangeCheck::Tagged(tag),
    };
    let pause_until = pause_until(answer.rate_limit);
    let next = if poll.issues.is_finished() && poll.pull_requests.is_finished() {
        let (next, moved) = finish_poll(link, poll)?;
        touched.extend(moved);
        Some(Phase::FetchingChanged(next))
    } else {
        None
    };
    Ok((Step::saved_if_any(touched, pause_until), next))
}

/// Saves each Issue Tsugiki holds that this answer changes. An Issue it doesn't hold isn't on any screen, and
/// a Filter that reaches one brings it later.
fn save_moved_issues(
    link: &Link<'_>,
    issues: Vec<IssueFacts>,
    poll: &mut Poll,
) -> Result<Vec<u32>, Error> {
    let held = link.store.issues(link.repository)?;
    let held: BTreeMap<u32, IssueFacts> = held
        .into_iter()
        .map(|issue| (issue.number, issue))
        .collect();
    let settled = issues
        .iter()
        .filter(|issue| issue.updated_at <= poll.settled_before);
    poll.changed.extend(settled.map(|issue| issue.number));
    if poll.redo {
        let in_the_window = issues.iter().map(|issue| issue.number);
        poll.moved_issues
            .extend(in_the_window.filter(|number| held.contains_key(number)));
    }
    let moved: Vec<IssueFacts> = issues
        .into_iter()
        .filter(|issue| held.get(&issue.number).is_some_and(|old| old != issue))
        .collect();
    let touched: Vec<u32> = moved.iter().map(|issue| issue.number).collect();
    link.store.save_issues(link.repository, moved)?;
    poll.moved_issues.extend(touched.iter().copied());
    Ok(touched)
}

/// Merges each search that answered whole, and plans what's left. Answers with the numbers that moved.
fn finish_poll(link: &Link<'_>, poll: &mut Poll) -> Result<(Changed, Vec<u32>), Error> {
    let mut moved = Vec::new();
    let mut hits: BTreeMap<u32, FilterHit> = BTreeMap::new();
    let mut narrowed = std::mem::take(&mut poll.narrowed);
    for (query, page) in std::mem::take(&mut poll.answers) {
        if page.next.is_some() {
            narrowed.push(query);
            continue;
        }
        let results = NarrowedResults {
            changed: poll.changed.clone(),
            found: page.hits.iter().map(|hit| hit.number).collect(),
            cut_off: false,
        };
        moved.extend(
            link.store
                .merge_filter_results(link.repository, &query, results)?,
        );
        hits.extend(page.hits.into_iter().map(|hit| (hit.number, hit)));
    }
    let stored = stored_pull_requests(link)?;
    let is_behind = |pr: &ChangedPr| {
        stored
            .get(&pr.number)
            .is_none_or(|held| !held.stays_as_it_is() && held.updated_at < pr.updated_at)
    };
    let ids = poll.changed_prs.iter().filter(|pr| is_behind(pr));
    let ids: Vec<NodeId> = ids.map(|pr| pr.id.clone()).collect();
    let mut walk = FilterWalk::of_refresh(RefreshWalk {
        whole: std::mem::take(&mut poll.whole),
        narrowed,
        since: Some(poll.since),
        changed: std::mem::take(&mut poll.changed),
        hits,
    });
    walk.hold_issues(std::mem::take(&mut poll.moved_issues));
    if poll.redo {
        let viewer = link.store.last_login()?;
        let held = poll
            .changed_prs
            .iter()
            .filter_map(|pr| stored.get(&pr.number));
        for pull_request in held.filter(|pr| is_work(pr, viewer.as_ref())) {
            walk.learn_from(pull_request, link.repository);
        }
    }
    Ok((Changed { ids, walk }, moved))
}

/// True when `pull_request` is on one of the user's own lists.
fn is_work(pull_request: &PullRequestFacts, viewer: Option<&Login>) -> bool {
    viewer.is_some_and(|viewer| !reasons(pull_request, viewer).is_empty())
}

fn stored_pull_requests(link: &Link<'_>) -> Result<BTreeMap<u32, PullRequestFacts>, Error> {
    let stored = link.store.pull_requests(link.repository)?;
    Ok(stored.into_iter().map(|pr| (pr.number, pr)).collect())
}

/// Fetches the next batch of what changed. A stranger's PR only needs saving, since a screen links it to an
/// Issue when it asks. The user's own work also needs what it points at, as a first sync fetches it.
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
        return Ok((NO_PAUSE, Some(Phase::Probing(next))));
    }
    let batch = link
        .github
        .fetch_pull_requests(PrFetch::Changed, &changed.ids, FETCH_BATCH)?;
    let stored = stored_pull_requests(link)?;
    let viewer = link.store.last_login()?;
    link.store
        .save_pull_requests(link.repository, batch.pull_requests.clone())?;
    let mut touched = Vec::new();
    for pull_request in &batch.pull_requests {
        fetched.extend([pull_request.number]);
        if stored.get(&pull_request.number) == Some(pull_request) {
            continue;
        }
        touched.push(pull_request.number);
        if is_work(pull_request, viewer.as_ref()) {
            changed.walk.learn_from(pull_request, link.repository);
        }
    }
    drop(changed.ids.drain(..batch.asked_for));
    let pause_until = pause_until(batch.rate_limit);
    Ok((Step::saved_if_any(touched, pause_until), None))
}

/// The open PRs on the user's own lists that this refresh hasn't just fetched.
fn tracked_pull_requests(link: &Link<'_>, fetched: &BTreeSet<u32>) -> Result<Vec<u32>, Error> {
    let Some(viewer) = link.store.last_login()? else {
        return Ok(Vec::new());
    };
    let stored = link.store.pull_requests(link.repository)?;
    let tracked = stored
        .iter()
        .filter(|pr| !fetched.contains(&pr.number) && !reasons(pr, &viewer).is_empty());
    Ok(tracked.map(|pr| pr.number).collect())
}

/// Probes the next batch of the user's open PRs, for the CI and Reviews that don't move `updatedAt`.
pub(super) fn probe(link: &Link<'_>, probing: &mut Probing) -> Result<Moved, Error> {
    if probing.numbers.is_empty() {
        let walk = std::mem::take(&mut probing.walk);
        return Ok((NO_PAUSE, Some(Phase::SearchingFilters(walk))));
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
    link.store.save_pull_requests(link.repository, updated)?;
    drop(probing.numbers.drain(..batch.asked_for));
    let pause_until = pause_until(batch.rate_limit);
    Ok((Step::saved_if_any(touched, pause_until), None))
}

#[cfg(test)]
mod tests;
