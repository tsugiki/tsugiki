//! Needs attention: which items wait on the user, since when, and why. Its words are the glossary's, and the
//! window turns each `Why` into the words an Alert would use.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use super::rows::reasons;
use super::{IssueItem, WorkReason, WorkbenchItem};
use crate::activity::{News, WhatHappened, has_failed};
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
}

impl Causes {
    /// No viewer means nobody has signed in yet, so nothing waits on anyone.
    pub(super) fn of(facts: &WorkFacts, conventions: Conventions) -> Self {
        let Some(viewer) = facts.viewer.as_ref() else {
            return Self::default();
        };
        let rules = Rules {
            news: News::new(&facts.activity, &facts.pull_requests, viewer, conventions),
            facts: &facts.activity,
            viewer,
        };
        let on_pull_requests = facts
            .pull_requests
            .iter()
            .map(|pr| (pr.number, rules.on_pull_request(pr)));
        let on_issues = facts
            .issues
            .iter()
            .map(|issue| (issue.number, rules.on_issue(issue)));
        let by_number = on_pull_requests
            .chain(on_issues)
            .filter(|(_, causes)| !causes.is_empty())
            .collect();
        let requested = facts
            .pull_requests
            .iter()
            .filter(|pr| reasons(pr, viewer).contains(&WorkReason::ReviewRequest));
        Self {
            by_number,
            review_requests: requested.map(|pr| pr.number).collect(),
        }
    }

    /// True when Issue or PR `number` Needs attention by itself, whatever the rest of its tree does.
    pub(super) fn needs_attention(&self, number: u32) -> bool {
        self.by_number.contains_key(&number)
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

    fn on_pull_request(&self, pull_request: &PullRequestFacts) -> Vec<Cause> {
        let mut causes = self.standing(pull_request);
        let number = pull_request.number;
        let own = pull_request.author.as_ref() == Some(self.viewer);
        // A colleague's Draft PR needs attention only when it carries a Review request.
        if pull_request.is_draft && !own {
            return causes;
        }
        let involvement = if pull_request.is_written_or_reviewed_by(self.viewer) {
            Involvement::WroteOrReviewed
        } else if pull_request.assignees.contains(self.viewer) || self.has_commented(number) {
            Involvement::InConversation
        } else {
            Involvement::Follows
        };
        let news = self.news.on_pull_request(pull_request).into_iter();
        let counted = news.filter(|new| involvement.counts(&new.what));
        causes.extend(counted.map(|new| Cause {
            at: new.at,
            why: why_of(
                new.what,
                &pull_request.latest_reviews,
                self.failed_checks_of(number),
            ),
        }));
        causes
    }

    fn on_issue(&self, issue: &IssueFacts) -> Vec<Cause> {
        let involvement = if issue.author.as_ref() == Some(self.viewer)
            || issue.assignees.contains(self.viewer)
            || self.has_commented(issue.number)
        {
            Involvement::InConversation
        } else {
            Involvement::Follows
        };
        let news = self.news.on_issue(issue.number).into_iter();
        let counted = news.filter(|new| involvement.counts(&new.what));
        counted
            .map(|new| Cause {
                at: new.at,
                why: why_of(new.what, &[], &[]),
            })
            .collect()
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
