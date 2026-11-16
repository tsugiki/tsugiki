//! The reasons a PR is the user's work.

use serde::Serialize;
use specta::Type;

use crate::facts::pull_requests::{PrState, PullRequestFacts};
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

/// A PR that has merged or closed waits on nobody, so it keeps no Issue on the Workbench.
pub(crate) fn reasons(facts: &PullRequestFacts, viewer: &Login) -> Vec<WorkReason> {
    if facts.state != PrState::Open {
        return Vec::new();
    }
    let own = facts.author.as_ref() == Some(viewer);
    let reviewed = facts.is_reviewed_by(viewer);
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
