//! The per-file patch-ids the Stale review check compares (#383), read in each mirror once it has the
//! commits. Reading them only reads the mirror, so it runs between the worker's git jobs, after the heads and
//! reviewed commits waiting, and hands what it read to the session to keep. A commit read since launch isn't
//! read again, and the session asks only for the commits the store lacks.

use std::collections::BTreeSet;
use std::mem;

use super::Worker;
use crate::diff::read_patch_ids;
use crate::patch_ids::{CommitPatches, PatchesOf};
use crate::repository::RepositoryName;

/// What the worker knows of one Repository's commits whose patch-ids the check needs.
#[derive(Debug, Default)]
pub(super) struct Patches {
    /// Handed over, and not read yet.
    waiting: BTreeSet<PatchesOf>,
    /// Their read failed, most often because the mirror lacks the commit or the Target branch yet. They are
    /// read again once a fetch brings heads or reviewed commits, or at the next hand-over.
    missing: BTreeSet<PatchesOf>,
    /// Read since launch.
    read: BTreeSet<PatchesOf>,
    /// Read, and handed over together once nothing waits.
    handing: Vec<CommitPatches>,
}

impl Patches {
    /// Reads again the commits whose read failed.
    pub(super) fn read_missing_again(&mut self) {
        self.waiting.append(&mut self.missing);
    }
}

impl Worker {
    /// Reads each of `wanted` that the worker hasn't read since launch, next time it reads.
    pub(super) fn hand_over_patches(
        &mut self,
        repository: &RepositoryName,
        wanted: Vec<PatchesOf>,
    ) {
        self.change_heads(repository, |known| {
            let patches = &mut known.patches;
            patches.missing.clear();
            let unread = wanted.into_iter().filter(|of| !patches.read.contains(of));
            patches.waiting = unread.collect();
        });
    }

    /// Whether a commit waits to be read in a Repository whose mirror is there, with somewhere to hand its
    /// patch-ids.
    pub(super) fn patches_wait(&self) -> bool {
        let Ok(folders) = &self.folders else {
            return false;
        };
        let waiting = (self.heads.iter()).filter(|known| !known.patches.waiting.is_empty());
        self.patched.is_some()
            && waiting
                .into_iter()
                .any(|known| folders.mirror(&known.repository).exists())
    }

    /// Reads the patch-ids of one commit waiting in a Repository whose mirror is there, and hands over what
    /// was read there once nothing more waits, so the loop takes its messages between reads. Says whether
    /// it read one. It reads none until it has somewhere to hand them.
    pub(super) fn read_waiting_patch(&mut self) -> bool {
        let (Ok(folders), Some(patched)) = (&self.folders, &mut self.patched) else {
            return false;
        };
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
                let _ = patches.read.insert(of.clone());
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
            patched(&known.repository, mem::take(&mut patches.handing));
        }
        true
    }
}
