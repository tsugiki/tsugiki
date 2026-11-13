//! A PR's review state: Unreviewed, Reviewed or Stale review (#364). A Review is stale once the PR's diff
//! outside Generated files changed since the commit it covered, which the per-file patch-ids of both commits
//! say (`patch_ids`), or while the check can't run. A Bot backport shares its Origin PR's (ADR 0011).

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use specta::Type;

use super::backports::origins;
use crate::conventions::Conventions;
use crate::github::CommitSha;
use crate::github::pull_requests::{PrHead, PrState, PullRequestFacts};
use crate::links::{BackportKind, BackportLink, MergeCommits, backport_of};
use crate::login::Login;
use crate::patch_ids::{CantCheck, ChangedFile, PatchesOf, changed_files};
use crate::store::{LastCheck, ReviewFacts, WorkFacts};

/// Where the user's Review of a PR stands, which its row shows in place of GitHub's review state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum YourReview {
    Unreviewed,
    /// The PR's diff is the same as when the user reviewed it, or no check has said otherwise yet.
    Reviewed,
    /// The PR's diff changed since the user's Review of commit `reviewed`, in `files`, as the check of
    /// commit `head` found. While a newer head waits for its check, `head` is the one the last check read.
    StaleReview {
        #[specta(type = String)]
        reviewed: CommitSha,
        #[specta(type = String)]
        head: CommitSha,
        files: Vec<ChangedFile>,
    },
    /// A Stale review of commit `reviewed`, as the check can't run for `reason` (#384). It runs once the
    /// reason goes away.
    CantCheck {
        #[specta(type = String)]
        reviewed: CommitSha,
        reason: CantCheck,
    },
}

impl YourReview {
    /// Whether the PR shows a Stale review: its diff changed since the user's Review, or the check can't
    /// say it didn't.
    pub fn is_stale(&self) -> bool {
        match self {
            Self::StaleReview { .. } | Self::CantCheck { .. } => true,
            Self::Unreviewed | Self::Reviewed => false,
        }
    }
}

/// The Bot backports that share their Origin PR's review state, each with its Origin PR's number (ADR 0011):
/// those that sit under their Origin PR in the list (`backports::origins`). A Bot backport whose Origin PR
/// Tsugiki lacks, or whose Origin PR is a Backport too, sits on its own and keeps a review state of its own.
/// So does a Manual backport.
#[derive(Debug)]
pub(super) struct SharedReviews(BTreeMap<u32, u32>);

impl SharedReviews {
    pub(super) fn of(pull_requests: &[PullRequestFacts], conventions: Conventions) -> Self {
        let merges = MergeCommits::of(pull_requests);
        let links: Vec<(u32, Option<BackportLink>)> = (pull_requests.iter())
            .map(|pr| (pr.number, backport_of(pr, conventions, &merges)))
            .collect();
        let origins = origins(links.iter().map(|(number, link)| (*number, link.as_ref())));
        let backports = (links.iter()).filter_map(|(number, link)| Some((number, link.as_ref()?)));
        let shared = backports
            .filter(|(_, link)| link.kind == BackportKind::Bot && origins.contains(&link.origin));
        Self(
            shared
                .map(|(number, link)| (*number, link.origin))
                .collect(),
        )
    }

    /// The Origin PR whose review state PR `number` shares, if PR `number` is a Bot backport that does.
    pub(super) fn origin_of(&self, number: u32) -> Option<u32> {
        self.0.get(&number).copied()
    }
}

/// The review state of each of `facts`' PRs for `viewer`. A Bot backport in `shared` takes its Origin PR's.
pub(super) fn your_reviews(
    facts: &WorkFacts,
    viewer: &Login,
    shared: &SharedReviews,
) -> BTreeMap<u32, YourReview> {
    let own: BTreeMap<u32, YourReview> = (facts.pull_requests.iter())
        .map(|pr| (pr.number, your_review(pr, viewer, &facts.reviews)))
        .collect();
    let review_of = |(number, review): (&u32, &YourReview)| {
        let origin = shared
            .origin_of(*number)
            .and_then(|origin| own.get(&origin));
        (*number, origin.unwrap_or(review).clone())
    };
    own.iter().map(review_of).collect()
}

/// The review state of `pull_request` for `viewer`. The user's comments on their own PR, which GitHub keeps
/// as Reviews, are no Review of it. A head equal to the reviewed commit is Reviewed with no check. Otherwise
/// the patch-ids of both commits decide, once the store holds them. A side the Mirror worker can't read
/// makes it a Stale review with the reason. While the check waits, the PR keeps the verdict of its last
/// check of the same Review, and is Reviewed without one.
fn your_review(
    pull_request: &PullRequestFacts,
    viewer: &Login,
    reviews: &ReviewFacts,
) -> YourReview {
    let reviewed = reviews.reviewed.get(&pull_request.number);
    let own = pull_request.author.as_ref() == Some(viewer);
    if own || (reviewed.is_none() && !pull_request.is_reviewed_by(viewer)) {
        return YourReview::Unreviewed;
    }
    let Some((reviewed, head)) = compared(pull_request, reviews) else {
        return YourReview::Reviewed;
    };
    let of = |commit: &CommitSha| patches_of(pull_request, commit);
    let (reviewed_of, head_of) = (of(reviewed), of(head));
    if let Some(verdict) = verdict(reviews, &reviewed_of, &head_of) {
        return verdict;
    }
    let unread = [&reviewed_of, &head_of]
        .into_iter()
        .filter(|of| !reviews.patches.contains_key(of));
    if let Some(reason) = unread.into_iter().find_map(|of| reviews.unchecked.get(of)) {
        return YourReview::CantCheck {
            reviewed: reviewed.clone(),
            reason: reason.clone(),
        };
    }
    let last =
        (reviews.last_checks.get(&pull_request.number)).filter(|last| last.reviewed == *reviewed);
    last.and_then(|last| verdict(reviews, &reviewed_of, &of(&last.head)))
        .unwrap_or(YourReview::Reviewed)
}

/// What the patch-ids of `reviewed` and `head` say, once the store holds both.
fn verdict(reviews: &ReviewFacts, reviewed: &PatchesOf, head: &PatchesOf) -> Option<YourReview> {
    let commits = (reviewed.commit.clone(), head.commit.clone());
    let (reviewed, head) = (reviews.patches.get(reviewed)?, reviews.patches.get(head)?);
    let files = changed_files(reviewed, head);
    Some(if files.is_empty() {
        YourReview::Reviewed
    } else {
        YourReview::StaleReview {
            reviewed: commits.0,
            head: commits.1,
            files,
        }
    })
}

/// The reviewed commit and the head of `pull_request`, if its head moved on from the user's Review.
fn compared<'a>(
    pull_request: &'a PullRequestFacts,
    reviews: &'a ReviewFacts,
) -> Option<(&'a CommitSha, &'a CommitSha)> {
    let reviewed = reviews.reviewed.get(&pull_request.number)?;
    let head = (pull_request.head.as_ref()).filter(|head| *head != reviewed)?;
    Some((reviewed, head))
}

/// `commit`'s patch-ids against `pull_request`'s Target branch.
fn patches_of(pull_request: &PullRequestFacts, commit: &CommitSha) -> PatchesOf {
    PatchesOf {
        commit: commit.clone(),
        target_branch: pull_request.target_branch.clone(),
    }
}

/// The reviewed commit and the head of each PR whose patch-ids the store holds of both, as its last check,
/// where the store's last check of it compared others.
pub(super) fn checks_to_keep(facts: &WorkFacts) -> Vec<(u32, LastCheck)> {
    let reviews = &facts.reviews;
    let checked = facts.pull_requests.iter().filter_map(|pr| {
        let (reviewed, head) = compared(pr, reviews)?;
        let read = |commit| reviews.patches.contains_key(&patches_of(pr, commit));
        let check = LastCheck {
            reviewed: reviewed.clone(),
            head: head.clone(),
        };
        let new = reviews.last_checks.get(&pr.number) != Some(&check);
        (read(reviewed) && read(head) && new).then_some((pr.number, check))
    });
    checked.collect()
}

/// A commit whose patch-ids a check still needs, as the reviewed commit or the head of PR `number`.
pub(super) struct Unread {
    number: u32,
    open: bool,
    of: PatchesOf,
}

/// The commits whose patch-ids a check still needs: the reviewed commit and the head of each PR of someone
/// else's whose head moved on from the user's Review, those the store holds left out. A Bot backport in
/// `shared` needs no check, as its Origin PR's review state is its own.
pub(super) fn unread_sides(facts: &WorkFacts, shared: &SharedReviews) -> Vec<Unread> {
    let others = (facts.pull_requests.iter())
        .filter(|pr| pr.author != facts.viewer && shared.origin_of(pr.number).is_none());
    let sides = others.filter_map(|pr| {
        let (reviewed, head) = compared(pr, &facts.reviews)?;
        let unread = |commit| Unread {
            number: pr.number,
            open: pr.state == PrState::Open,
            of: patches_of(pr, commit),
        };
        Some([unread(reviewed), unread(head)])
    });
    let unread = sides.flatten();
    unread
        .filter(|side| !facts.reviews.patches.contains_key(&side.of))
        .collect()
}

/// The commits of `unread` to read: each open PR's, and a merged or closed PR's while the mirror fetches
/// its head for the user's work, in `heads`. Each comes once. A merged or closed PR's head stays put, so
/// once the store holds both its sides, its verdict is frozen and nothing of it is read again (#385). One
/// the user's work no longer reaches shows on no screen, and the mirror may lack its final head.
pub(super) fn patches_to_read(unread: Vec<Unread>, heads: &[PrHead]) -> Vec<PatchesOf> {
    let fetched: BTreeSet<u32> = heads.iter().map(|head| head.number).collect();
    let wanted = (unread.into_iter()).filter(|side| side.open || fetched.contains(&side.number));
    let wanted: BTreeSet<PatchesOf> = wanted.map(|side| side.of).collect();
    wanted.into_iter().collect()
}

#[cfg(test)]
mod tests;
