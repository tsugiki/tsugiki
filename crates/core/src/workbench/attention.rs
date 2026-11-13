//! Needs attention: which items wait on the user, since when, and why. Its words are the glossary's, and the
//! window turns each `Why` into the words an Alert would use.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use specta::Type;
use time::OffsetDateTime;

use super::review_state::{SharedReviews, YourReview, your_reviews};
use super::rows::reasons;
use super::{IssueItem, WorkReason, WorkbenchItem};
use crate::activity::{NewActivity, News, WhatHappened, has_failed};
use crate::conventions::Conventions;
use crate::github::activity::{Activity, ActivityKind};
use crate::github::issues::IssueFacts;
use crate::github::pull_requests::{CiState, LatestReview, PrState, PullRequestFacts, ReviewState};
use crate::login::Login;
use crate::store::{ActivityFacts, WorkFacts};

/// One reason an item Needs attention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Why {
    ReviewRequested,
    /// The PR's diff changed since the user's Review. The row names the files.
    StaleReview,
    /// Empty `checks` means Tsugiki hasn't heard which ones failed, or GitHub names none.
    CiFailed {
        checks: Vec<String>,
    },
    CiPassed,
    ChangesRequested {
        by: Login,
    },
    Approved {
        by: Login,
    },
    Reviewed {
        by: Login,
    },
    Comment {
        by: Login,
    },
    Commits {
        by: Login,
    },
    ReadyForReview {
        by: Login,
    },
    Mentioned,
    BackportFailed {
        target: String,
    },
    /// The user marked the item unseen.
    MarkedUnseen,
}

/// The numbers Mark Seen marks and the moment it marks them up to, as the window drew them. The window sends
/// it back as it got it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SeenMark {
    /// The item, and for an Issue every PR in its tree.
    pub numbers: Vec<u32>,
    /// The moment Last seen moves on to: the newest the window drew of GitHub's last update, CI reaching a
    /// result and each event on them. Whatever a later sync brings stays new. Nothing for numbers the facts
    /// don't hold, and Last seen then stays.
    #[serde(with = "time::serde::rfc3339::option")]
    #[specta(type = Option<String>)]
    pub up_to: Option<OffsetDateTime>,
}

/// An item's part in Mark Seen and Mark Unseen: what Mark Seen sends, and which of the two its button offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Seen {
    pub mark: SeenMark,
    pub state: SeenState,
}

/// Whether the user has seen an item as it now is. The header offers Mark Seen unless they have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SeenState {
    /// The item, or a PR in an Issue's tree, is Marked unseen, whatever its activity.
    MarkedUnseen,
    /// The item, or a PR in an Issue's tree, has New activity, whoever it waits on.
    NewActivity,
    Seen,
}

/// The reasons an item Needs attention, and since when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Attention {
    /// The start of its oldest reason, which places the item in a list.
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub since: OffsetDateTime,
    /// The newest first, each once.
    pub why: Vec<Why>,
}

#[derive(Debug, Clone)]
struct Cause {
    at: OffsetDateTime,
    why: Why,
}

/// The rules that make each of a Repository's Issues and PRs Need attention.
#[derive(Debug, Default)]
pub(super) struct Causes {
    by_number: BTreeMap<u32, Vec<Cause>>,
    /// The PRs that carry a Review request.
    pub(super) review_requests: BTreeSet<u32>,
    /// The PRs whose Stale review Needs attention, which the Stale reviews view holds.
    pub(super) stale_reviews: BTreeSet<u32>,
    /// The Issues and PRs with New activity, whoever it waits on.
    with_news: BTreeSet<u32>,
    /// The newest moment the facts hold on each Issue and PR: GitHub's last update, CI reaching a result, or
    /// an event.
    known_until: BTreeMap<u32, OffsetDateTime>,
    /// The Issues and PRs the user marked unseen.
    marked_unseen: BTreeSet<u32>,
    /// Where the user's Review of each PR stands. A PR missing here is Unreviewed.
    reviews: BTreeMap<u32, YourReview>,
}

impl Causes {
    /// No viewer means nobody has signed in yet, so nothing waits on anyone.
    pub(super) fn of(facts: &WorkFacts, conventions: Conventions) -> Self {
        let Some(viewer) = facts.viewer.as_ref() else {
            return Self::default();
        };
        let shared = SharedReviews::of(&facts.pull_requests, conventions);
        let reviews = your_reviews(facts, viewer, &shared);
        let stale_reviews = stale_reviews(facts, viewer, &reviews, &shared);
        let rules = Rules {
            news: News::new(&facts.activity, &facts.pull_requests, viewer, conventions),
            facts: &facts.activity,
            viewer,
            stale_reviews: &stale_reviews,
        };
        let on_pull_requests = facts.pull_requests.iter().map(|pr| {
            let news = rules.news.on_pull_request(pr);
            (pr.number, rules.on_pull_request(pr, &news), news)
        });
        let on_issues = facts.issues.iter().map(|issue| {
            let news = rules.news.on_issue(issue.number);
            (issue.number, rules.on_issue(issue, &news), news)
        });
        let mut with_news = BTreeSet::new();
        let mut by_number = BTreeMap::new();
        for (number, causes, news) in on_pull_requests.chain(on_issues) {
            with_news.extend((!news.is_empty()).then_some(number));
            if !causes.is_empty() {
                by_number.extend([(number, causes)]);
            }
        }
        let requested = facts
            .pull_requests
            .iter()
            .filter(|pr| reasons(pr, viewer).contains(&WorkReason::ReviewRequest));
        Self {
            by_number,
            review_requests: requested.map(|pr| pr.number).collect(),
            stale_reviews,
            with_news,
            known_until: known_until(facts),
            marked_unseen: facts.activity.marked_unseen.keys().copied().collect(),
            reviews,
        }
    }

    /// Where the user's Review of PR `number` stands.
    pub(super) fn review_of(&self, number: u32) -> YourReview {
        let review = self.reviews.get(&number).cloned();
        review.unwrap_or(YourReview::Unreviewed)
    }

    /// True when Issue or PR `number` itself Needs attention.
    pub(super) fn needs_attention(&self, number: u32) -> bool {
        self.by_number.contains_key(&number)
    }

    /// The part in Mark Seen of the item whose numbers are `numbers`, the item's own first.
    pub(super) fn seen_of(&self, numbers: Vec<u32>) -> Seen {
        let up_to = numbers
            .iter()
            .filter_map(|number| self.known_until.get(number))
            .max()
            .copied();
        let any_in = |set: &BTreeSet<u32>| numbers.iter().any(|number| set.contains(number));
        let state = if any_in(&self.marked_unseen) {
            SeenState::MarkedUnseen
        } else if any_in(&self.with_news) {
            SeenState::NewActivity
        } else {
            SeenState::Seen
        };
        Seen {
            mark: SeenMark { numbers, up_to },
            state,
        }
    }

    /// The sum of `item` and the rest of its tree.
    pub(super) fn attention_of(&self, item: &WorkbenchItem) -> Option<Attention> {
        let found = item
            .numbers()
            .into_iter()
            .filter_map(|number| self.by_number.get(&number));
        let mut causes: Vec<&Cause> = found.flatten().collect();
        let since = causes.iter().map(|cause| cause.at).min()?;
        causes.sort_by_key(|cause| std::cmp::Reverse(cause.at));
        let mut why: Vec<Why> = Vec::new();
        for cause in causes {
            if !why.contains(&cause.why) {
                why.push(cause.why.clone());
            }
        }
        Some(Attention { since, why })
    }
}

/// The PRs whose Stale review Needs attention: each open PR where `viewer`'s Review is a Stale review. A
/// merged or closed PR waits on nobody, whatever its last check found, and nor does a colleague's Draft PR.
/// A Bot backport's Stale review is its Origin PR's, and counts only on the Origin PR, so not at all once
/// that has merged.
fn stale_reviews(
    facts: &WorkFacts,
    viewer: &Login,
    reviews: &BTreeMap<u32, YourReview>,
    shared: &SharedReviews,
) -> BTreeSet<u32> {
    let waiting = facts.pull_requests.iter().filter(|pr| {
        let colleagues_draft = pr.is_draft && pr.author.as_ref() != Some(viewer);
        let borrowed = shared.origin_of(pr.number).is_some();
        let stale = reviews.get(&pr.number).is_some_and(YourReview::is_stale);
        pr.state == PrState::Open && !colleagues_draft && !borrowed && stale
    });
    waiting.map(|pr| pr.number).collect()
}

/// The newest moment `facts` hold on each Issue and PR, which Mark Seen moves Last seen to. It covers what
/// isn't New activity yet, such as CI on a PR the user hasn't reviewed, so that stays seen once it would be.
fn known_until(facts: &WorkFacts) -> BTreeMap<u32, OffsetDateTime> {
    let updated = facts
        .pull_requests
        .iter()
        .map(|pr| (pr.number, pr.updated_at))
        .chain(
            facts
                .issues
                .iter()
                .map(|issue| (issue.number, issue.updated_at)),
        );
    let activity = &facts.activity;
    let events = activity
        .events
        .iter()
        .flat_map(|(number, events)| events.iter().map(|event| (*number, event.at)));
    let ci = activity
        .ci_finished
        .iter()
        .map(|(number, at)| (*number, *at));
    let mut newest = BTreeMap::new();
    for (number, at) in updated.chain(events).chain(ci) {
        let known = newest.entry(number).or_insert(at);
        *known = (*known).max(at);
    }
    newest
}

impl WorkbenchItem {
    /// The numbers of what this item shows.
    fn numbers(&self) -> Vec<u32> {
        let issue = match self {
            Self::Issue(IssueItem { number, .. }) => Some(*number),
            Self::StandalonePr(_) | Self::StandaloneBackport(_) => None,
        };
        let rows = self.rows();
        issue
            .into_iter()
            .chain(rows.iter().map(|row| row.number))
            .collect()
    }
}

struct Rules<'a> {
    news: News<'a>,
    facts: &'a ActivityFacts,
    viewer: &'a Login,
    stale_reviews: &'a BTreeSet<u32>,
}

impl Rules<'_> {
    fn events_of(&self, number: u32) -> impl Iterator<Item = &Activity> {
        self.facts.events.get(&number).into_iter().flatten()
    }

    /// True when the user has commented on item `number`.
    fn has_commented(&self, number: u32) -> bool {
        self.events_of(number)
            .any(|event| event.kind == ActivityKind::Comment && event.who == *self.viewer)
    }

    /// The moment the user was last asked to review `pull_request`. Its timeline can have dropped that off
    /// the end, and then its last update is the nearest time Tsugiki has.
    fn requested_at(&self, pull_request: &PullRequestFacts) -> OffsetDateTime {
        let asked = self.events_of(pull_request.number).filter(|event| {
            let kind = matches!(
                event.kind,
                ActivityKind::ReviewRequested | ActivityKind::Assigned
            );
            kind && event.who == *self.viewer
        });
        let last = asked.map(|event| event.at).max();
        last.unwrap_or(pull_request.updated_at)
    }

    fn failed_checks_of(&self, number: u32) -> &[String] {
        let checks = self.facts.failed_checks.get(&number);
        checks.map_or(&[], Vec::as_slice)
    }

    /// The causes that wait on the user whatever they have seen: a Review request, and failed CI on an Own
    /// PR.
    fn standing(&self, pull_request: &PullRequestFacts) -> Vec<Cause> {
        let number = pull_request.number;
        let asked = reasons(pull_request, self.viewer).contains(&WorkReason::ReviewRequest);
        let requested = asked.then(|| Cause {
            at: self.requested_at(pull_request),
            why: Why::ReviewRequested,
        });
        let own = pull_request.author.as_ref() == Some(self.viewer);
        let open = pull_request.state == PrState::Open;
        let failed = (own && open && has_failed(pull_request.ci)).then(|| {
            let finished = self.facts.ci_finished.get(&number).copied();
            Cause {
                at: finished.unwrap_or(pull_request.updated_at),
                why: Why::CiFailed {
                    checks: self.failed_checks_of(number).to_vec(),
                },
            }
        });
        requested.into_iter().chain(failed).collect()
    }

    /// Marked unseen, from the moment of the flag, whatever the user's part in the item.
    fn flag(&self, number: u32) -> Option<Cause> {
        let at = *self.facts.marked_unseen.get(&number)?;
        Some(Cause {
            at,
            why: Why::MarkedUnseen,
        })
    }

    fn on_pull_request(&self, pull_request: &PullRequestFacts, news: &[NewActivity]) -> Vec<Cause> {
        let number = pull_request.number;
        let mut causes = self.standing(pull_request);
        causes.extend(self.flag(number));
        let own = pull_request.author.as_ref() == Some(self.viewer);
        // A colleague's Draft PR needs attention only when it carries a Review request.
        if pull_request.is_draft && !own {
            return causes;
        }
        if self.stale_reviews.contains(&number) {
            causes.push(Cause {
                at: pull_request.updated_at,
                why: Why::StaleReview,
            });
        }
        let involvement = if pull_request.is_written_or_reviewed_by(self.viewer) {
            Involvement::WroteOrReviewed
        } else if pull_request.assignees.contains(self.viewer) || self.has_commented(number) {
            Involvement::InConversation
        } else {
            Involvement::Follows
        };
        let counted = news.iter().filter(|new| involvement.counts(&new.what));
        causes.extend(counted.map(|new| Cause {
            at: new.at,
            why: why_of(
                new.what.clone(),
                &pull_request.latest_reviews,
                self.failed_checks_of(number),
            ),
        }));
        causes
    }

    fn on_issue(&self, issue: &IssueFacts, news: &[NewActivity]) -> Vec<Cause> {
        let involvement = if issue.author.as_ref() == Some(self.viewer)
            || issue.assignees.contains(self.viewer)
            || self.has_commented(issue.number)
        {
            Involvement::InConversation
        } else {
            Involvement::Follows
        };
        let counted = news.iter().filter(|new| involvement.counts(&new.what));
        let causes = counted.map(|new| Cause {
            at: new.at,
            why: why_of(new.what.clone(), &[], &[]),
        });
        causes.chain(self.flag(issue.number)).collect()
    }
}

/// The user's part in an item, which decides what on it waits on them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Involvement {
    /// They wrote the PR or have a Review on it.
    WroteOrReviewed,
    /// They opened the Issue, are assigned to the item or commented on it.
    InConversation,
    /// It's only on their list. A mention still reaches them.
    Follows,
}

impl Involvement {
    /// True when `what` waits on a user who comes into the item this way.
    fn counts(self, what: &WhatHappened) -> bool {
        match what {
            WhatHappened::Mentioned => true,
            WhatHappened::Comment(_) => self != Self::Follows,
            WhatHappened::Commit(_)
            | WhatHappened::Review(_)
            | WhatHappened::ReadyForReview(_)
            | WhatHappened::BackportConflict { .. }
            | WhatHappened::BackportCiFailed { .. }
            | WhatHappened::CiFinished(_) => self == Self::WroteOrReviewed,
        }
    }
}

fn why_of(what: WhatHappened, reviews: &[LatestReview], checks: &[String]) -> Why {
    match what {
        WhatHappened::Commit(by) => Why::Commits { by },
        WhatHappened::Comment(by) => Why::Comment { by },
        WhatHappened::Review(by) => {
            let review = reviews.iter().find(|review| review.author == by);
            match review.map(|review| review.state) {
                Some(ReviewState::ChangesRequested) => Why::ChangesRequested { by },
                Some(ReviewState::Approved) => Why::Approved { by },
                Some(ReviewState::Commented | ReviewState::Dismissed | ReviewState::Pending)
                | None => Why::Reviewed { by },
            }
        }
        WhatHappened::ReadyForReview(by) => Why::ReadyForReview { by },
        WhatHappened::BackportConflict { target }
        | WhatHappened::BackportCiFailed { target, .. } => Why::BackportFailed { target },
        WhatHappened::CiFinished(CiState::Success) => Why::CiPassed,
        WhatHappened::CiFinished(
            CiState::Failure | CiState::Error | CiState::Expected | CiState::Pending,
        ) => Why::CiFailed {
            checks: checks.to_vec(),
        },
        WhatHappened::Mentioned => Why::Mentioned,
    }
}

#[cfg(test)]
mod tests;
