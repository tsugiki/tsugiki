//! New activity: what happened on an Issue or a PR after the user's Last seen. It says nothing about review
//! state, and its words are the glossary's.

use time::OffsetDateTime;

use crate::conventions::Conventions;
use crate::error::Error;
use crate::github::Login;
use crate::github::activity::{Activity, ActivityKind};
use crate::github::pull_requests::{CiState, PullRequestFacts};
use crate::links::{BackportKind, MergeCommits, backport_of};
use crate::repository::RepositoryName;
use crate::store::Store;

/// One thing that makes an item show New activity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhatHappened {
    Commit(Login),
    Comment(Login),
    Review(Login),
    ReadyForReview(Login),
    /// The backport bot couldn't make the Backport to `target`, so none exists.
    BackportConflict {
        target: String,
    },
    /// CI failed on Bot backport `number`, which targets `target`.
    BackportCiFailed {
        number: u32,
        target: String,
    },
    /// CI reached this result on an Own PR or on a PR the user has a Review on.
    CiFinished(CiState),
}

/// One piece of New activity, and when it happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewActivity {
    pub what: WhatHappened,
    pub at: OffsetDateTime,
}

/// True when CI has reached a result. A push sets it back to pending, which is no news.
fn has_finished(ci: CiState) -> bool {
    matches!(ci, CiState::Success | CiState::Failure | CiState::Error)
}

fn has_failed(ci: Option<CiState>) -> bool {
    matches!(ci, Some(CiState::Failure | CiState::Error))
}

/// The moment after which something on item `number` is new: its Last seen, or the Repository's first sync
/// when that's later, because nothing that a first sync found is news. Nothing, before a first sync finishes.
fn news_starts(
    store: &Store,
    repository: &RepositoryName,
    number: u32,
) -> Result<Option<OffsetDateTime>, Error> {
    let Some(first_sync) = store.first_sync_started_at(repository)? else {
        return Ok(None);
    };
    let last_seen = store.last_seen(repository, number)?;
    Ok(Some(
        last_seen.map_or(first_sync, |seen| seen.max(first_sync)),
    ))
}

/// The word for `event`, if it counts. A person other than the user has to have done it, apart from a
/// backport failing, which has no other way to reach anyone.
fn what_happened(
    event: Activity,
    viewer: Option<&Login>,
    conventions: Conventions,
) -> Option<WhatHappened> {
    let by_someone_else = viewer != Some(&event.who) && !conventions.is_bot_account(&event.who);
    let who = event.who;
    match event.kind {
        ActivityKind::BackportFailed { target } => Some(WhatHappened::BackportConflict { target }),
        ActivityKind::Commit => by_someone_else.then_some(WhatHappened::Commit(who)),
        ActivityKind::Comment => by_someone_else.then_some(WhatHappened::Comment(who)),
        ActivityKind::Review => by_someone_else.then_some(WhatHappened::Review(who)),
        ActivityKind::ReadyForReview => {
            by_someone_else.then_some(WhatHappened::ReadyForReview(who))
        }
    }
}

/// The news on item `number`'s timeline after `since`.
fn on_the_timeline(
    store: &Store,
    repository: &RepositoryName,
    number: u32,
    since: OffsetDateTime,
) -> Result<Vec<NewActivity>, Error> {
    let viewer = store.last_login()?;
    let conventions = Conventions::for_repository(repository);
    let events = store.activity(repository, number)?;
    let later = events.into_iter().filter(|event| event.at > since);
    Ok(later
        .filter_map(|event| {
            let at = event.at;
            let what = what_happened(event, viewer.as_ref(), conventions)?;
            Some(NewActivity { what, at })
        })
        .collect())
}

/// The New activity on Issue `number`, oldest first.
pub fn on_issue(
    store: &Store,
    repository: &RepositoryName,
    number: u32,
) -> Result<Vec<NewActivity>, Error> {
    match news_starts(store, repository, number)? {
        Some(since) => on_the_timeline(store, repository, number, since),
        None => Ok(Vec::new()),
    }
}

/// `pull_request`'s CI result, if it's news to a user who wrote the PR or has a Review on it.
fn ci_news(
    store: &Store,
    repository: &RepositoryName,
    pull_request: &PullRequestFacts,
    since: OffsetDateTime,
) -> Result<Option<NewActivity>, Error> {
    let viewer = store.last_login()?;
    let follows = viewer
        .as_ref()
        .is_some_and(|viewer| pull_request.is_written_or_reviewed_by(viewer));
    let (true, Some(result)) = (follows, pull_request.ci.filter(|ci| has_finished(*ci))) else {
        return Ok(None);
    };
    let finished = store.ci_finished_at(repository, pull_request.number)?;
    Ok(finished.filter(|at| *at > since).map(|at| NewActivity {
        what: WhatHappened::CiFinished(result),
        at,
    }))
}

/// The failed CI on PR `number` if it's a Bot backport, and on each Bot backport of it.
fn failed_backports(
    store: &Store,
    repository: &RepositoryName,
    number: u32,
    (held, since): (&[PullRequestFacts], OffsetDateTime),
) -> Result<Vec<NewActivity>, Error> {
    let conventions = Conventions::for_repository(repository);
    let merges = MergeCommits::of(held);
    let is_about_this = |pr: &&PullRequestFacts| {
        backport_of(pr, conventions, &merges).is_some_and(|link| {
            link.kind == BackportKind::Bot && (link.origin == number || pr.number == number)
        })
    };
    let mut news = Vec::new();
    for pr in held
        .iter()
        .filter(is_about_this)
        .filter(|pr| has_failed(pr.ci))
    {
        let finished = store.ci_finished_at(repository, pr.number)?;
        if let Some(at) = finished.filter(|at| *at > since) {
            news.push(NewActivity {
                what: WhatHappened::BackportCiFailed {
                    number: pr.number,
                    target: pr.target_branch.clone(),
                },
                at,
            });
        }
    }
    Ok(news)
}

/// The New activity on PR `number`, oldest first.
pub fn on_pull_request(
    store: &Store,
    repository: &RepositoryName,
    number: u32,
) -> Result<Vec<NewActivity>, Error> {
    let Some(since) = news_starts(store, repository, number)? else {
        return Ok(Vec::new());
    };
    let held = store.pull_requests(repository)?;
    let Some(pull_request) = held.iter().find(|pr| pr.number == number) else {
        return Ok(Vec::new());
    };
    let mut news = on_the_timeline(store, repository, number, since)?;
    let backports = failed_backports(store, repository, number, (&held, since))?;
    let failed_here = backports.iter().any(|new| {
        matches!(new.what, WhatHappened::BackportCiFailed { number: failed, .. } if failed == number)
    });
    // A failed Bot backport already says that its own CI failed.
    if !failed_here {
        news.extend(ci_news(store, repository, pull_request, since)?);
    }
    news.extend(backports);
    news.sort_by_key(|new| new.at);
    Ok(news)
}

#[cfg(test)]
mod tests;
