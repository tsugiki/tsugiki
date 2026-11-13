//! A PR's review state: Unreviewed, Reviewed or Stale review (#364). A Review is stale once the PR's diff
//! outside Generated files changed since the commit it covered, which the per-file patch-ids of both commits
//! say (`patch_ids`).

use std::collections::BTreeSet;

use serde::Serialize;
use specta::Type;

use crate::github::CommitSha;
use crate::github::pull_requests::{PrState, PullRequestFacts};
use crate::login::Login;
use crate::patch_ids::{ChangedFile, PatchesOf, changed_files};
use crate::store::{ReviewFacts, WorkFacts};

/// Where the user's Review of a PR stands, which its row shows in place of GitHub's review state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum YourReview {
    Unreviewed,
    /// The PR's diff is the same as when the user reviewed it, or no check has said otherwise yet.
    Reviewed,
    /// The PR's diff changed since the user's Review, in `files`.
    StaleReview {
        files: Vec<ChangedFile>,
    },
}

/// The review state of `pull_request` for `viewer`. The user's comments on their own PR, which GitHub keeps
/// as Reviews, are no Review of it. A head equal to the reviewed commit is Reviewed with no check. Otherwise
/// the patch-ids of both commits decide, once the store holds them, and until then the PR stays Reviewed.
pub(super) fn your_review(
    pull_request: &PullRequestFacts,
    viewer: &Login,
    reviews: &ReviewFacts,
) -> YourReview {
    let reviewed = reviews.reviewed.get(&pull_request.number);
    let own = pull_request.author.as_ref() == Some(viewer);
    if own || (reviewed.is_none() && !pull_request.is_reviewed_by(viewer)) {
        return YourReview::Unreviewed;
    }
    let patches_of = |commit: &CommitSha| {
        let of = PatchesOf {
            commit: commit.clone(),
            target_branch: pull_request.target_branch.clone(),
        };
        reviews.patches.get(&of)
    };
    let sides = reviewed.zip(pull_request.head.as_ref());
    let compared = sides.filter(|(reviewed, head)| reviewed != head);
    let files = compared.and_then(|(reviewed, head)| {
        let (reviewed, head) = (patches_of(reviewed)?, patches_of(head)?);
        Some(changed_files(reviewed, head))
    });
    match files {
        Some(files) if !files.is_empty() => YourReview::StaleReview { files },
        Some(_) | None => YourReview::Reviewed,
    }
}

/// The commits whose patch-ids a check still needs: the reviewed commit and the head of each open PR of
/// someone else's whose head moved on from the user's Review, those the store holds left out. Each comes
/// once.
pub(super) fn patches_to_read(facts: &WorkFacts) -> Vec<PatchesOf> {
    let reviews = &facts.reviews;
    let open = facts
        .pull_requests
        .iter()
        .filter(|pr| pr.state == PrState::Open && pr.author != facts.viewer);
    let sides = open.filter_map(|pr| {
        let reviewed = reviews.reviewed.get(&pr.number)?;
        let head = pr.head.as_ref().filter(|head| *head != reviewed)?;
        let of = |commit: &CommitSha| PatchesOf {
            commit: commit.clone(),
            target_branch: pr.target_branch.clone(),
        };
        Some([of(reviewed), of(head)])
    });
    let unread = sides
        .flatten()
        .filter(|of| !reviews.patches.contains_key(of));
    unread.collect::<BTreeSet<_>>().into_iter().collect()
}

#[cfg(test)]
mod tests;
