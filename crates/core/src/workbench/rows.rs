//! A PR's row.

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use super::Seen;
use crate::facts::pull_requests::{CiState, PrState, PullRequestFacts};
use crate::login::Login;
use crate::work::{WorkReason, YourReview, reasons};

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
    /// Where the user's Review stands: Unreviewed, Reviewed or Stale review.
    pub your_review: YourReview,
    /// Its part in Mark Seen, which covers this PR alone.
    pub seen: Seen,
}

/// The PR's line of the list, whatever it has to do with `viewer`, with the review state `your_review`.
pub(super) fn row(
    facts: PullRequestFacts,
    viewer: &Login,
    your_review: YourReview,
    seen: Seen,
) -> WorkbenchRow {
    let reasons = reasons(&facts, viewer);
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

#[cfg(test)]
mod tests;
