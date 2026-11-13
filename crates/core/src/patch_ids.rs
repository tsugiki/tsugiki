//! The per-file patch-ids of a commit, which the Stale review check compares (#364). Each file's change from
//! the commit's merge base with the Target branch gets one from `git patch-id --verbatim`, where every
//! whitespace change counts, and one from `git patch-id --stable`, which ignores whitespace. A commit can't
//! change, so the store keeps them by commit (ADR 0010), and checking the same commits again runs no git.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use specta::Type;

use crate::github::CommitSha;

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

/// The per-file patch-ids of a commit, as the Mirror worker read them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitPatches {
    pub of: PatchesOf,
    pub files: Vec<FilePatch>,
}

/// A file whose change differs from the one the user's Review covered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ChangedFile {
    pub path: String,
    /// The two changes differ only in whitespace. That still makes the Review stale, as indentation matters
    /// in Python.
    pub whitespace_only: bool,
}

/// The files whose change at the head differs from the reviewed commit's, by path. A file either side marks
/// generated doesn't count. A path a diff names twice, as when a file is deleted and another renamed into its
/// place, compares both of its changes.
pub fn changed_files(reviewed: &[FilePatch], head: &[FilePatch]) -> Vec<ChangedFile> {
    let generated: BTreeSet<&str> = (reviewed.iter().chain(head))
        .filter(|file| file.generated)
        .map(|file| file.path.as_str())
        .collect();
    let (reviewed, head) = (by_path(reviewed, &generated), by_path(head, &generated));
    let paths: BTreeSet<&str> = reviewed.keys().chain(head.keys()).copied().collect();
    paths
        .into_iter()
        .filter_map(|path| {
            let (before, now) = (reviewed.get(path), head.get(path));
            (ids(before, verbatim) != ids(now, verbatim)).then(|| ChangedFile {
                path: path.to_owned(),
                whitespace_only: ids(before, loose) == ids(now, loose),
            })
        })
        .collect()
}

/// Each of `files` outside `generated`, by path.
fn by_path<'a>(
    files: &'a [FilePatch],
    generated: &BTreeSet<&str>,
) -> BTreeMap<&'a str, Vec<&'a FilePatch>> {
    let mut paths: BTreeMap<&str, Vec<&FilePatch>> = BTreeMap::new();
    for file in files
        .iter()
        .filter(|file| !generated.contains(file.path.as_str()))
    {
        paths.entry(file.path.as_str()).or_default().push(file);
    }
    paths
}

fn verbatim(file: &FilePatch) -> &PatchId {
    &file.verbatim
}

fn loose(file: &FilePatch) -> &PatchId {
    &file.loose
}

/// The patch-ids `id` picks from each of `files`, sorted, so the order a diff lists them in doesn't count.
fn ids<'a>(files: Option<&Vec<&'a FilePatch>>, id: fn(&FilePatch) -> &PatchId) -> Vec<&'a PatchId> {
    let mut ids: Vec<&PatchId> = files.into_iter().flatten().map(|file| id(file)).collect();
    ids.sort_unstable();
    ids
}

#[cfg(test)]
mod tests;
