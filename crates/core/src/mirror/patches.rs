//! The per-file patch-ids the Stale review check compares (#383), read in each mirror once it has the
//! commits. Reading them only reads the mirror, so it runs between the worker's git jobs, after the heads and
//! reviewed commits waiting, and hands what it read to the session to keep. The session asks only for the
//! commits the store lacks, so a commit the store keeps isn't read again, and one whose save failed is. The
//! worker also tells the session which commits it can't read now and why (#384), each time that changes, so
//! the check says so until the reason goes away.

use std::collections::BTreeSet;
use std::mem;

use super::{CHECK_GIT, MirrorProblem, Worker};
use crate::diff::read_patch_ids;
use crate::patch_ids::{CantCheck, CommitPatches, PatchReads, PatchesOf, UncheckedCommit};
use crate::repository::RepositoryName;

/// What the worker knows of one Repository's commits whose patch-ids the check needs.
#[derive(Debug, Default)]
pub(super) struct Patches {
    /// Handed over, and not read yet.
    waiting: BTreeSet<PatchesOf>,
    /// Their read failed, most often because the mirror lacks the commit or the Target branch yet. They are
    /// read again once a fetch brings heads or reviewed commits, or at the next hand-over.
    missing: BTreeSet<PatchesOf>,
    /// Read, and handed over together once nothing waits, or at the next hand-over.
    handing: Vec<CommitPatches>,
    /// The commits the session last heard the worker can't read, and why.
    told: Vec<UncheckedCommit>,
}

impl Patches {
    /// Reads again the commits whose read failed.
    pub(super) fn read_missing_again(&mut self) {
        self.waiting.append(&mut self.missing);
    }
}

impl Worker {
    /// Hands over what was read since the last hand-over, as `wanted` takes the place of what waits. Reads
    /// each of `wanted` it hasn't just handed over, next time it reads.
    pub(super) fn hand_over_patches(
        &mut self,
        repository: &RepositoryName,
        wanted: Vec<PatchesOf>,
    ) {
        let mut read = Vec::new();
        self.change_heads(repository, |known| {
            let patches = &mut known.patches;
            patches.missing.clear();
            read = mem::take(&mut patches.handing);
            let unread = wanted
                .into_iter()
                .filter(|of| !read.iter().any(|done| done.of == *of));
            patches.waiting = unread.collect();
        });
        if !read.is_empty()
            && let Some(patched) = &mut self.patched
        {
            patched(repository, PatchReads::Read(read));
        }
    }

    /// Whether a commit waits to be read in a Repository whose mirror is there, with somewhere to hand its
    /// patch-ids and a git that can read them.
    pub(super) fn patches_wait(&self) -> bool {
        let Ok(folders) = &self.folders else {
            return false;
        };
        let waiting = (self.heads.iter()).filter(|known| !known.patches.waiting.is_empty());
        self.patched.is_some()
            && self.old_for_check.is_none()
            && waiting
                .into_iter()
                .any(|known| folders.mirror(&known.repository).exists())
    }

    /// Reads the patch-ids of one commit waiting in a Repository whose mirror is there, and hands over what
    /// was read there once nothing more waits, so the loop takes its messages between reads. Says whether
    /// it read one. It reads none until it has somewhere to hand them, nor with a git too old for them.
    pub(super) fn read_waiting_patch(&mut self) -> bool {
        let (Ok(folders), Some(patched)) = (&self.folders, &mut self.patched) else {
            return false;
        };
        if self.old_for_check.is_some() {
            return false;
        }
        let found = self.heads.iter_mut().find_map(|known| {
            let mirror = folders.mirror(&known.repository);
            let ready = !known.patches.waiting.is_empty() && mirror.exists();
            let of = ready.then(|| known.patches.waiting.pop_first())??;
            Some((known, mirror, of))
        });
        let Some((known, mirror, of)) = found else {
            return false;
        };
        let patches = &mut known.patches;
        match read_patch_ids(&mirror, &of.target_branch, &of.commit) {
            Ok(files) => {
                patches.handing.push(CommitPatches { of, files });
            }
            Err(cause) => {
                let (repository, commit) = (&known.repository, of.commit.as_str());
                log::info!(
                    "Tsugiki can't read the patch-ids of {commit} in {repository}'s mirror yet: {cause}"
                );
                let _ = patches.missing.insert(of);
            }
        }
        if patches.waiting.is_empty() && !patches.handing.is_empty() {
            let read = mem::take(&mut patches.handing);
            patched(&known.repository, PatchReads::Read(read));
        }
        true
    }

    /// Tells the session which of the commits waiting to be read in each Repository the worker can't read
    /// now, and why, where that changed since the session last heard. A commit whose read only waits for
    /// its turn, or for a fetch to bring it, has no reason.
    pub(super) fn tell_unchecked(&mut self) {
        let (Ok(folders), Some(patched)) = (&self.folders, &mut self.patched) else {
            return;
        };
        // A git too old for the mirrors is too old for the check, which needs a newer one still.
        let old_git = |version: &String| CantCheck::OldGit {
            version: version.clone(),
            needed: version_words(CHECK_GIT),
        };
        let git = match &self.problem {
            Some(MirrorProblem::NoGit) => Some(CantCheck::NoGit),
            Some(MirrorProblem::OldGit { version }) => Some(old_git(version)),
            Some(MirrorProblem::DiskShort { .. }) | None => {
                self.old_for_check.as_ref().map(old_git)
            }
        };
        for known in &mut self.heads {
            let wanted: BTreeSet<&PatchesOf> = (known.patches.waiting.iter())
                .chain(&known.patches.missing)
                .collect();
            let mirror = folders.mirror(&known.repository);
            let mirror_reason = if wanted.is_empty() || git.is_some() || mirror.exists() {
                None
            } else if (self.replaced.iter()).any(|one| one.is_same(&known.repository)) {
                Some(CantCheck::Broken)
            } else {
                Some(CantCheck::Cloning)
            };
            let repository_reason = git.clone().or(mirror_reason);
            let refused = (repository_reason.is_none() && !wanted.is_empty())
                .then(|| known.reviews.refused(&known.repository, &mirror));
            let unchecked: Vec<UncheckedCommit> = (wanted.into_iter())
                .filter_map(|of| {
                    let refused = refused.is_some_and(|refused| refused.contains(&of.commit));
                    let reason = repository_reason
                        .clone()
                        .or_else(|| refused.then_some(CantCheck::Refused))?;
                    Some(UncheckedCommit {
                        of: of.clone(),
                        reason,
                    })
                })
                .collect();
            if unchecked != known.patches.told {
                patched(&known.repository, PatchReads::Unchecked(unchecked.clone()));
                known.patches.told = unchecked;
            }
        }
    }
}

/// A git version as its release notes name it, such as 2.39.
fn version_words((major, minor): (u32, u32)) -> String {
    format!("{major}.{minor}")
}
