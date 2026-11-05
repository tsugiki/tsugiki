//! New activity: what happened on an Issue or a PR after the user's Last seen. It says nothing about review
//! state, and its words are the glossary's.

use time::OffsetDateTime;

use crate::conventions::Conventions;
use crate::github::Login;
use crate::github::activity::{Activity, ActivityKind};
use crate::github::pull_requests::{CiState, PrState, PullRequestFacts};
use crate::links::{BackportKind, MergeCommits, backport_of};
use crate::store::ActivityFacts;

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
    /// A post mentioned the user.
    Mentioned,
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

pub(crate) fn has_failed(ci: Option<CiState>) -> bool {
    matches!(ci, Some(CiState::Failure | CiState::Error))
}

/// The word for `event`, if it counts. A person other than the user has to have done it, apart from a
/// backport failing, which has no other way to reach anyone, and a mention, which names the user.
fn what_happened(
    event: Activity,
    viewer: &Login,
    conventions: Conventions,
) -> Option<WhatHappened> {
    let by_someone_else = *viewer != event.who && !conventions.is_bot_account(&event.who);
    let who = event.who;
    match event.kind {
        ActivityKind::BackportFailed { target } => Some(WhatHappened::BackportConflict { target }),
        ActivityKind::Commit => by_someone_else.then_some(WhatHappened::Commit(who)),
        ActivityKind::Comment => by_someone_else.then_some(WhatHappened::Comment(who)),
        ActivityKind::Review => by_someone_else.then_some(WhatHappened::Review(who)),
        ActivityKind::ReadyForReview => {
            by_someone_else.then_some(WhatHappened::ReadyForReview(who))
        }
        ActivityKind::Mentioned => (*viewer == who).then_some(WhatHappened::Mentioned),
        ActivityKind::ReviewRequested | ActivityKind::Assigned => None,
    }
}

/// The New activity of a Repository's items, as one user reads it.
pub struct News<'a> {
    facts: &'a ActivityFacts,
    viewer: &'a Login,
    conventions: Conventions,
    /// Each open Bot backport whose CI failed, with its Origin PR's number. Nobody can act on a closed one.
    failed_backports: Vec<(u32, &'a PullRequestFacts)>,
}

impl<'a> News<'a> {
    pub fn new(
        facts: &'a ActivityFacts,
        pull_requests: &'a [PullRequestFacts],
        viewer: &'a Login,
        conventions: Conventions,
    ) -> Self {
        let merges = MergeCommits::of(pull_requests);
        let open = pull_requests.iter().filter(|pr| pr.state == PrState::Open);
        let failed = open.filter(|pr| has_failed(pr.ci));
        let failed_backports = failed
            .filter_map(|pr| {
                let link = backport_of(pr, conventions, &merges)?;
                (link.kind == BackportKind::Bot).then_some((link.origin, pr))
            })
            .collect();
        Self {
            facts,
            viewer,
            conventions,
            failed_backports,
        }
    }

    /// The moment after which something on item `number` is new: its Last seen, or the Repository's first
    /// sync when that's later, because nothing that a first sync found is news. Nothing, before a first
    /// sync finishes.
    fn starts(&self, number: u32) -> Option<OffsetDateTime> {
        let first_sync = self.facts.first_sync?;
        let last_seen = self.facts.last_seen.get(&number).copied();
        Some(last_seen.map_or(first_sync, |seen| seen.max(first_sync)))
    }

    /// The news on item `number`'s timeline after `since`.
    fn on_the_timeline(&self, number: u32, since: OffsetDateTime) -> Vec<NewActivity> {
        let events = self.facts.events.get(&number).into_iter().flatten();
        let later = events.filter(|event| event.at > since).cloned();
        later
            .filter_map(|event| {
                let at = event.at;
                let what = what_happened(event, self.viewer, self.conventions)?;
                Some(NewActivity { what, at })
            })
            .collect()
    }

    /// The New activity on Issue `number`, oldest first.
    pub fn on_issue(&self, number: u32) -> Vec<NewActivity> {
        self.starts(number)
            .map_or_else(Vec::new, |since| self.on_the_timeline(number, since))
    }

    /// `pull_request`'s CI result, if it's news to a user who wrote the PR or has a Review on it. Nobody is
    /// waiting on the CI of a PR that has merged or closed.
    fn ci_news(
        &self,
        pull_request: &PullRequestFacts,
        since: OffsetDateTime,
    ) -> Option<NewActivity> {
        let result = pull_request.ci.filter(|ci| has_finished(*ci))?;
        if pull_request.state != PrState::Open
            || !pull_request.is_written_or_reviewed_by(self.viewer)
        {
            return None;
        }
        let finished = self.facts.ci_finished.get(&pull_request.number).copied();
        finished.filter(|at| *at > since).map(|at| NewActivity {
            what: WhatHappened::CiFinished(result),
            at,
        })
    }

    /// The failed CI on PR `number` if it's a Bot backport, and on each Bot backport of it.
    fn failed_backports(&self, number: u32, since: OffsetDateTime) -> Vec<NewActivity> {
        let about_this = self
            .failed_backports
            .iter()
            .filter(|(origin, pr)| *origin == number || pr.number == number);
        about_this
            .filter_map(|(_, pr)| {
                let finished = self.facts.ci_finished.get(&pr.number).copied();
                finished.filter(|at| *at > since).map(|at| NewActivity {
                    what: WhatHappened::BackportCiFailed {
                        number: pr.number,
                        target: pr.target_branch.clone(),
                    },
                    at,
                })
            })
            .collect()
    }

    /// The New activity on `pull_request`, oldest first.
    pub fn on_pull_request(&self, pull_request: &PullRequestFacts) -> Vec<NewActivity> {
        let number = pull_request.number;
        let Some(since) = self.starts(number) else {
            return Vec::new();
        };
        let mut news = self.on_the_timeline(number, since);
        let backports = self.failed_backports(number, since);
        let failed_here = backports.iter().any(|new| {
            matches!(new.what, WhatHappened::BackportCiFailed { number: failed, .. } if failed == number)
        });
        // A failed Bot backport already says that its own CI failed.
        if !failed_here {
            news.extend(self.ci_news(pull_request, since));
        }
        news.extend(backports);
        news.sort_by_key(|new| new.at);
        news
    }
}

#[cfg(test)]
mod tests;
