//! A PR's review state: Unreviewed, Reviewed or Stale review (#364). A Review is stale once the PR's diff
//! outside Generated files changed since the commit it covered, which the per-file patch-ids of both commits
//! say (`patch_ids`), or while the check can't run. A Bot backport shares its Origin PR's (ADR 0011).

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use specta::Type;

use super::origins;
use crate::conventions::Conventions;
use crate::facts::patch_ids::{
    CantCheck, Findings, Found, LastCheck, PatchesOf, ReviewFacts, StaleFound,
};
use crate::facts::pull_requests::{PrHead, PrState, PullRequestFacts};
use crate::facts::{CommitSha, WorkFacts};
use crate::links::{BackportKind, BackportLink, MergeCommits, backport_of};
use crate::login::Login;
use crate::patch_ids::{ChangedFile, changed_files};

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
/// those that sit under their Origin PR in the list (`work::origins`). A Bot backport whose Origin PR
/// Tsugiki lacks, or whose Origin PR is a Backport too, sits on its own and keeps a review state of its own.
/// So does a Manual backport.
#[derive(Debug)]
pub(crate) struct SharedReviews(BTreeMap<u32, u32>);

impl SharedReviews {
    pub(crate) fn of(pull_requests: &[PullRequestFacts], conventions: Conventions) -> Self {
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
    pub(crate) fn origin_of(&self, number: u32) -> Option<u32> {
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
    checked(pull_request, reviews).unwrap_or(YourReview::Reviewed)
}

/// The review state the checks give the user's Review of `pull_request`, once its head moved on from the
/// reviewed commit: the verdict of the check of both, why that check can't run, or the verdict of its last
/// check of the same Review. None while it waits with none of these.
fn checked(pull_request: &PullRequestFacts, reviews: &ReviewFacts) -> Option<YourReview> {
    let (reviewed, head) = compared(pull_request, reviews)?;
    let of = |commit: &CommitSha| patches_of(pull_request, commit);
    let (reviewed_of, head_of) = (of(reviewed), of(head));
    if let Some(verdict) = verdict(reviews, &reviewed_of, &head_of) {
        return Some(verdict);
    }
    let unread = [&reviewed_of, &head_of]
        .into_iter()
        .filter(|of| !reviews.patches.contains_key(of));
    if let Some(reason) = unread.into_iter().find_map(|of| reviews.unchecked.get(of)) {
        return Some(YourReview::CantCheck {
            reviewed: reviewed.clone(),
            reason: reason.clone(),
        });
    }
    let last =
        (reviews.last_checks.get(&pull_request.number)).filter(|last| last.reviewed == *reviewed);
    last.and_then(|last| verdict(reviews, &reviewed_of, &of(&last.head)))
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

/// The reviewed commit and the head of each PR whose patch-ids the store holds of both, for the store to keep
/// as its last check, where the store's last check of it compared others. Their verdict stands while a newer
/// check waits.
pub fn checks_to_keep(facts: &WorkFacts) -> Vec<(u32, LastCheck)> {
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

/// Whether a Stale review on `pull_request` can wait on `viewer`: the PR is open, and it is no colleague's
/// Draft PR. Needs attention counts a Stale review only there, and only there is a new one found (#908).
pub(super) fn can_wait_on(pull_request: &PullRequestFacts, viewer: &Login) -> bool {
    let colleagues_draft = pull_request.is_draft && pull_request.author.as_ref() != Some(viewer);
    pull_request.state == PrState::Open && !colleagues_draft
}

/// The finding the store keeps of `review`, a Stale review of `pull_request`, if it keeps one: of the
/// reviewed commit and the head the Stale review was found against (#908). A verdict that the last check
/// gave on an earlier head, while the check of the PR's head waits, says nothing new of that head, so the
/// finding kept of the same Review stands for it, as after a restart forgets why the check couldn't run.
pub(super) fn kept_finding<'a>(
    review: &YourReview,
    pull_request: &PullRequestFacts,
    kept: &'a BTreeMap<u32, StaleFound>,
) -> Option<&'a StaleFound> {
    let (reviewed, head) = found_against(review, pull_request)?;
    let kept = kept.get(&pull_request.number)?;
    let of_an_earlier_head = pull_request.head.as_ref() != Some(&head);
    (kept.reviewed == reviewed && (kept.head == head || of_an_earlier_head)).then_some(kept)
}

/// The reviewed commit and the head that `review`, a Stale review of `pull_request`, was found against: the
/// head its check compared, or the PR's head where the check can't run. None for a review state that isn't
/// stale.
fn found_against(
    review: &YourReview,
    pull_request: &PullRequestFacts,
) -> Option<(CommitSha, CommitSha)> {
    match review {
        YourReview::StaleReview { reviewed, head, .. } => Some((reviewed.clone(), head.clone())),
        YourReview::CantCheck { reviewed, .. } => {
            Some((reviewed.clone(), pull_request.head.clone()?))
        }
        YourReview::Unreviewed | YourReview::Reviewed => None,
    }
}

/// The changes to the findings the store keeps that the user's Review of a PR is stale (#908). A Review a
/// check finds stale, or can't check, against a head with no finding kept gets a new finding in place of
/// the PR's last: a new head or a new Review makes a new finding. The last ends once a check finds the diff
/// the same, the head goes back to the reviewed commit, or the user reviews again. A check that waits leaves
/// it as it is, and so does a restart that forgets why a check couldn't run.
///
/// Only a Stale review that can wait on the user gets a new finding, so a colleague's Draft PR gets one once
/// it is ready for review. A closed PR keeps its last, so a reopen finds it as it was. The user's own PRs and
/// PRs the facts no longer hold keep none. A Bot backport that shares its Origin PR's review state never gets
/// one, as the check reads none of its commits.
pub fn findings_to_keep(facts: &WorkFacts) -> Findings {
    let Some(viewer) = facts.viewer.as_ref() else {
        return Findings::default();
    };
    let reviews = &facts.reviews;
    let held: BTreeSet<u32> = facts.pull_requests.iter().map(|pr| pr.number).collect();
    let gone = (reviews.stale_found.keys()).filter(|number| !held.contains(number));
    let mut findings = Findings {
        found: vec![],
        ended: gone.copied().collect(),
    };
    for pr in &facts.pull_requests {
        let number = pr.number;
        let kept = reviews.stale_found.get(&number);
        match standing(pr, reviews, viewer) {
            Standing::Stale { reviewed, head } if can_wait_on(pr, viewer) => {
                findings.found.push(Found {
                    number,
                    reviewed,
                    head,
                });
            }
            Standing::NotStale if kept.is_some() => findings.ended.push(number),
            Standing::Waits(reviewed) if kept.is_some_and(|kept| kept.reviewed != *reviewed) => {
                findings.ended.push(number);
            }
            Standing::Stale { .. } | Standing::Kept | Standing::NotStale | Standing::Waits(_) => {}
        }
    }
    findings.ended.sort_unstable();
    findings
}

/// The standing of the user's Review of a PR, by which `findings_to_keep` keeps its findings.
enum Standing<'a> {
    /// A check finds the user's Review of commit `reviewed` stale against `head`, or can't check the two,
    /// and the store keeps no finding of that.
    Stale {
        reviewed: CommitSha,
        head: CommitSha,
    },
    /// The user's Review is stale, and the store keeps the finding of it.
    Kept,
    /// No Review of the user's there is stale: they have none, they wrote the PR, its head is the reviewed
    /// commit, or a check finds the diff the same.
    NotStale,
    /// The check of the user's Review of this commit waits, with no verdict.
    Waits(&'a CommitSha),
}

/// The standing of `viewer`'s Review of `pull_request`.
fn standing<'a>(
    pull_request: &PullRequestFacts,
    reviews: &'a ReviewFacts,
    viewer: &Login,
) -> Standing<'a> {
    let Some(reviewed) = reviews.reviewed.get(&pull_request.number) else {
        return Standing::NotStale;
    };
    let own = pull_request.author.as_ref() == Some(viewer);
    if own || pull_request.head.as_ref() == Some(reviewed) {
        return Standing::NotStale;
    }
    let Some(review) = checked(pull_request, reviews) else {
        return Standing::Waits(reviewed);
    };
    if kept_finding(&review, pull_request, &reviews.stale_found).is_some() {
        return Standing::Kept;
    }
    match found_against(&review, pull_request) {
        Some((reviewed, head)) => Standing::Stale { reviewed, head },
        None => Standing::NotStale,
    }
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
/// its head as one in Reach, in `heads`. Each comes once. A merged or closed PR's head stays put, so once
/// the store holds both its sides, its verdict is frozen and nothing of it is read again (#385). One no
/// longer in Reach shows on no screen, and the mirror may lack its final head.
pub(super) fn patches_to_read(unread: Vec<Unread>, heads: &[PrHead]) -> Vec<PatchesOf> {
    let fetched: BTreeSet<u32> = heads.iter().map(|head| head.number).collect();
    let wanted = (unread.into_iter()).filter(|side| side.open || fetched.contains(&side.number));
    let wanted: BTreeSet<PatchesOf> = wanted.map(|side| side.of).collect();
    wanted.into_iter().collect()
}

#[cfg(test)]
mod tests;
