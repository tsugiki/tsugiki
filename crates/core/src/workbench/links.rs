//! The Issues each PR fixes, by the link rules (ADR 0011).

use std::collections::BTreeSet;

use super::WorkbenchRow;
use crate::github::issues::IssueFacts;
use crate::github::pull_requests::PullRequestFacts;
use crate::links::{BackportLink, MergeCommits, backport_of, fixed_numbers};
use crate::repository_settings::RepositorySettings;

/// A PR with what the link rules say about it.
pub(super) struct Linked {
    pub(super) row: WorkbenchRow,
    /// The Issues it fixes. Only a number the sync fetched as an Issue counts, so a PR's number or a typo
    /// doesn't.
    pub(super) issues: BTreeSet<u32>,
    pub(super) backport: Option<BackportLink>,
}

/// Links each PR, whose line of the list `row` draws.
pub(super) fn link(
    pull_requests: Vec<PullRequestFacts>,
    issues: &[IssueFacts],
    repository: &RepositorySettings,
    row: impl Fn(PullRequestFacts) -> WorkbenchRow,
) -> Vec<Linked> {
    let merges = MergeCommits::of(&pull_requests);
    let known: BTreeSet<u32> = issues.iter().map(|issue| issue.number).collect();
    pull_requests
        .into_iter()
        .map(|facts| {
            let fixed = fixed_numbers(&facts, repository);
            Linked {
                issues: fixed.intersection(&known).copied().collect(),
                backport: backport_of(&facts, repository.conventions, &merges),
                row: row(facts),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
