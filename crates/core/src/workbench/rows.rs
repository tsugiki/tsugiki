//! A PR's row, and the reasons it's the user's work.

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use super::Seen;
use crate::github::pull_requests::{CiState, PrState, PullRequestFacts, ReviewState};
use crate::login::Login;

/// The reason a PR is on the user's Workbench.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WorkReason {
    /// The user is a requested reviewer, or an assignee of someone else's PR who hasn't submitted a Review.
    ReviewRequest,
    OwnPr,
    /// The user has submitted a Review.
    Reviewed,
}

/// A PR's line of the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct WorkbenchRow {
    pub number: u32,
    pub title: String,
    pub author: Option<Login>,
    pub state: PrState,
    pub is_draft: bool,
    pub target_branch: String,
    pub ci: Option<CiState>,
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub updated_at: OffsetDateTime,
    /// Empty for a PR that another PR put on the Workbench, and for one no longer open.
    pub reasons: Vec<WorkReason>,
    /// The user's latest submitted Review. A pending Review is a draft, so it isn't one yet.
    pub your_review: Option<ReviewState>,
    /// Its part in Mark Seen, which covers this PR alone.
    pub seen: Seen,
}

/// The PR's line of the list, whatever it has to do with `viewer`.
pub(super) fn row(facts: PullRequestFacts, viewer: &Login, seen: Seen) -> WorkbenchRow {
    let reasons = reasons(&facts, viewer);
    let your_review = facts
        .latest_reviews
        .iter()
        .find(|review| review.author == *viewer && review.state != ReviewState::Pending)
        .map(|review| review.state);
    WorkbenchRow {
        number: facts.number,
        title: facts.title,
        author: facts.author,
        state: facts.state,
        is_draft: facts.is_draft,
        target_branch: facts.target_branch,
        ci: facts.ci,
        updated_at: facts.updated_at,
        reasons,
        your_review,
        seen,
    }
}

/// A PR that has merged or closed waits on nobody, so it keeps no Issue on the Workbench.
pub(crate) fn reasons(facts: &PullRequestFacts, viewer: &Login) -> Vec<WorkReason> {
    if facts.state != PrState::Open {
        return Vec::new();
    }
    let own = facts.author.as_ref() == Some(viewer);
    // A pending Review is a draft only its author can see, so it isn't one yet.
    let reviewed = facts
        .latest_reviews
        .iter()
        .any(|review| review.author == *viewer && review.state != ReviewState::Pending);
    // GitHub takes a reviewer off the requested list when they submit, and leaves an assignee on theirs. An
    // author assigns themselves to say who's on it, which asks nothing of them.
    let assigned = facts.assignees.contains(viewer) && !reviewed && !own;
    let asked = facts.review_requests.contains(viewer) || assigned;
    [
        (asked, WorkReason::ReviewRequest),
        (own, WorkReason::OwnPr),
        (reviewed, WorkReason::Reviewed),
    ]
    .into_iter()
    .filter_map(|(applies, reason)| applies.then_some(reason))
    .collect()
}

#[cfg(test)]
mod tests;
