//! What Tsugiki keeps about a PR, which `github` parses from a fetch and the store reads back.

use serde::{Deserialize, Serialize};
use specta::Type;
use time::OffsetDateTime;

use crate::facts::CommitSha;
use crate::facts::labels::Label;
use crate::login::Login;

#[cfg(test)]
mod tests;

/// GitHub's ID for an item, which a fetch asks for it by.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(String);

impl NodeId {
    /// The ID as GitHub spells it, which only `github` sends back.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
impl NodeId {
    /// The ID GitHub spells `id`.
    pub(crate) fn new(id: String) -> Self {
        Self(id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PrState {
    Open,
    Closed,
    Merged,
}

/// A commit's CI, as GitHub rolls its checks up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CiState {
    Expected,
    Error,
    Failure,
    Pending,
    Success,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewState {
    Pending,
    Commented,
    Approved,
    ChangesRequested,
    Dismissed,
}

/// Someone's latest Review of a PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestReview {
    pub author: Login,
    pub state: ReviewState,
}

/// The facts Tsugiki keeps about a PR, as GitHub reports them (ADR 0010).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestFacts {
    pub number: u32,
    pub title: String,
    pub state: PrState,
    pub is_draft: bool,
    /// Nobody, when the account no longer exists.
    pub author: Option<Login>,
    pub target_branch: String,
    /// Nothing, when the last commit has no checks.
    pub ci: Option<CiState>,
    pub updated_at: OffsetDateTime,
    /// Empty when the store reads back a merged or closed PR: those requests ask nothing of anyone.
    pub review_requests: Vec<Login>,
    pub latest_reviews: Vec<LatestReview>,
    pub assignees: Vec<Login>,
    /// In name order, ignoring case. Empty for a PR stored before Tsugiki kept labels, until GitHub sends
    /// it again.
    pub labels: Vec<Label>,
    /// The Issues in this Repository that GitHub says this PR closes.
    pub closing_issues: Vec<u32>,
    /// The commit a merged PR became. GitHub gives one only for a merged PR.
    pub merge_commit: Option<CommitSha>,
    /// The commit of the Target branch GitHub compares the PR with, which stays put once the PR merges.
    /// Nothing for a PR stored before Tsugiki read it.
    pub base: Option<CommitSha>,
    /// The commit the PR's branch is at, which the Repository's mirror fetches. Nothing for a PR stored
    /// before Tsugiki read heads.
    pub head: Option<CommitSha>,
    /// The facts the link rules read on a PR that doesn't target the default branch (ADR 0011).
    pub off_default: Option<OffDefault>,
}

/// The commit a PR's branch is at, which the Repository's mirror fetches, with the branch its diff starts
/// from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrHead {
    pub number: u32,
    pub commit: CommitSha,
    pub target_branch: String,
}

/// What a Repository's Mirror holds of a PR's head, as a look in the Mirror found it. Opening the PR asks the
/// Mirror worker for what this says is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadInMirror {
    /// Tsugiki doesn't know the PR's head yet: the sync reads it the next time it reads the PR.
    Unknown,
    /// The Mirror lacks the head.
    Missing(PrHead),
    /// The Mirror holds the head.
    Held,
}

impl HeadInMirror {
    /// What the Mirror holds of `pr`'s head, where `holds` says whether the Mirror holds a commit, as a look
    /// in it found. It asks only of a head Tsugiki knows.
    #[must_use]
    pub fn of(pr: &PullRequestFacts, holds: impl FnOnce(&CommitSha) -> bool) -> Self {
        match &pr.head {
            None => Self::Unknown,
            Some(commit) if holds(commit) => Self::Held,
            Some(commit) => Self::Missing(PrHead {
                number: pr.number,
                commit: commit.clone(),
                target_branch: pr.target_branch.clone(),
            }),
        }
    }
}

/// The commit the user's Review of a PR covered, which the Repository's mirror keeps under a ref of its own
/// (#382), so a Stale review check finds it there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewedCommit {
    pub number: u32,
    pub commit: CommitSha,
}

/// The facts only a PR off the default branch needs, for the link rules (ADR 0011).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffDefault {
    /// GitHub ignores closing keywords here, so Tsugiki reads them itself.
    pub description: String,
    /// The commits its description or its commits name in a `(cherry picked from commit <sha>)` line.
    pub cherry_picked_from: Vec<CommitSha>,
    /// The committer of each commit that isn't a merge. `None` for one with no GitHub account or a
    /// commit past the ones Tsugiki reads.
    pub committers: Vec<Option<Login>>,
}

impl PullRequestFacts {
    /// True when a sync needn't fetch this PR again, because it has merged or closed (ADR 0011). A merged PR
    /// with no merge commit came from a Tsugiki that kept none, so a fetch still has facts to bring.
    pub fn is_frozen(&self) -> bool {
        match self.state {
            PrState::Open => false,
            PrState::Closed => true,
            PrState::Merged => self.merge_commit.is_some(),
        }
    }

    /// True when `login` wrote this PR or has a Review on it. A review they haven't submitted isn't one yet.
    pub fn is_written_or_reviewed_by(&self, login: &Login) -> bool {
        self.author.as_ref() == Some(login) || self.is_reviewed_by(login)
    }

    /// True when `login` has a Review on this PR, as GitHub lists the latest of each. A review they haven't
    /// submitted isn't one yet.
    pub fn is_reviewed_by(&self, login: &Login) -> bool {
        self.latest_reviews
            .iter()
            .any(|review| review.author == *login && review.state != ReviewState::Pending)
    }

    /// True when a newer `updatedAt` says nothing a Filter or a tree reads has changed, as a merged PR stays
    /// merged. A closed PR can reopen.
    pub fn stays_as_it_is(&self) -> bool {
        self.state == PrState::Merged && self.is_frozen()
    }
}

#[cfg(test)]
impl OffDefault {
    /// Facts off the default branch with `description` and nothing else.
    pub(crate) fn described(description: &str) -> Self {
        Self {
            description: description.to_owned(),
            cherry_picked_from: vec![],
            committers: vec![],
        }
    }
}
