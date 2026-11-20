//! The per-file patch-ids of a commit, as the Mirror worker reads them and the store keeps them by commit
//! (ADR 0010), and the store's bundle of what the Stale review check compares (#364).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;
use time::OffsetDateTime;

use crate::facts::CommitSha;

/// The patch-id git gives a file's change: a whole SHA-1, in lower case.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PatchId(String);

impl PatchId {
    /// The patch-id `text` spells, if it spells one.
    pub fn parse(text: &str) -> Option<Self> {
        let is_id = text.len() == 40 && text.chars().all(|c| c.is_ascii_hexdigit());
        is_id.then(|| Self(text.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One file's change in a commit's diff from its merge base with the Target branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePatch {
    /// Where the file is at the commit. A renamed file goes by its new name.
    pub path: String,
    /// The change's patch-id, every whitespace change counted.
    pub verbatim: PatchId,
    /// Its patch-id with whitespace ignored.
    pub loose: PatchId,
    /// Whether `.gitattributes` on the Target branch marks it `linguist-generated`.
    pub generated: bool,
}

/// A commit whose per-file patch-ids the check needs, with the Target branch its diff starts from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PatchesOf {
    pub commit: CommitSha,
    pub target_branch: String,
}

/// Why the Stale review check can't read a commit's patch-ids now (#384). Each goes away with its cause, and
/// the check then runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CantCheck {
    /// The Repository's mirror isn't cloned yet.
    Cloning,
    /// git said the Repository's mirror was broken, and it isn't cloned again yet.
    Broken,
    /// git isn't on the `PATH`.
    NoGit,
    /// git is older than the `needed` version, as `git --version` says in `version`.
    OldGit { version: String, needed: String },
    /// GitHub refused to send the commit.
    Refused,
}

/// The per-file patch-ids of a commit, as the Mirror worker read them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitPatches {
    pub of: PatchesOf,
    pub files: Vec<FilePatch>,
}

/// A commit whose patch-ids the check needs and can't read now, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UncheckedCommit {
    pub of: PatchesOf,
    pub reason: CantCheck,
}

/// What the review state of each PR is worked out from: the commit the user's Review covered, and the
/// patch-ids the store holds of that commit and of the head.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReviewFacts {
    /// The commit of the user's Review of each PR whose Conversation Tsugiki has read.
    pub reviewed: BTreeMap<u32, CommitSha>,
    /// The per-file patch-ids of each reviewed commit and head a check compares, once they have been read,
    /// and of the head each PR's last check compared.
    pub patches: BTreeMap<PatchesOf, Vec<FilePatch>>,
    /// Why the Mirror worker can't read the patch-ids of the commits a check compares, for those it can't.
    pub unchecked: BTreeMap<PatchesOf, CantCheck>,
    /// The commits each PR's last finished check compared, whose verdict stands while a newer check waits.
    pub last_checks: BTreeMap<u32, LastCheck>,
    /// The last finding the store kept of each PR that the user's Review of it is stale.
    pub stale_found: BTreeMap<u32, StaleFound>,
}

/// The two commits a PR's check compared once the patch-ids of both were read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastCheck {
    pub reviewed: CommitSha,
    pub head: CommitSha,
}

/// A check's finding that the user's Review of commit `reviewed` is stale against head `head`, or that the
/// check of the two can't run, first found `at` (#908). The window can't show the Stale review before then.
/// A new head or a new Review makes a new finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleFound {
    pub reviewed: CommitSha,
    pub head: CommitSha,
    pub at: OffsetDateTime,
    /// Whether Mark Seen, Archive or Mute acknowledged this finding, as the window drew it.
    pub acknowledged: bool,
}

impl StaleFound {
    /// This finding on PR `number`, as a mark carries it.
    pub fn on(&self, number: u32) -> StaleFinding {
        StaleFinding {
            number,
            reviewed: self.reviewed.clone(),
            head: self.head.clone(),
            found_at: self.at,
        }
    }
}

/// A finding that the user's Review of PR `number` is stale, which the window drew, as Mark Seen, Archive
/// and Mute carry it back to acknowledge that finding alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StaleFinding {
    pub number: u32,
    #[specta(type = String)]
    pub reviewed: CommitSha,
    #[specta(type = String)]
    pub head: CommitSha,
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub found_at: OffsetDateTime,
}

/// A Review of commit `reviewed` on PR `number` that a check finds stale against head `head`, or can't
/// check, with no finding of it kept yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub number: u32,
    pub reviewed: CommitSha,
    pub head: CommitSha,
}

/// The changes to what the store keeps of `StaleFound`, after what the checks say now.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Findings {
    /// Each new finding, which the store dates from now in place of the PR's last.
    pub found: Vec<Found>,
    /// Each PR whose last finding no longer stands: a check finds the diff the same, the head went back to
    /// the reviewed commit, the user reviewed it again or wrote it, or the facts no longer hold the PR.
    pub ended: Vec<u32>,
}

impl Findings {
    pub fn is_empty(&self) -> bool {
        self.found.is_empty() && self.ended.is_empty()
    }
}

#[cfg(test)]
mod tests;
