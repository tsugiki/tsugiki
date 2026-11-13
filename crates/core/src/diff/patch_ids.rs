//! Each file's patch-id in a commit's diff from its merge base with the Target branch, for the Stale review
//! check (#364). git's `patch-id` reads many patches in one go, so each file's section goes in as a patch of
//! its own, under a made-up commit line that numbers it.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::io;
use std::path::Path;

use super::{DIFF_OPTIONS, TargetIndex, git, merge_base};
use crate::github::{CommitSha, live};
use crate::patch_ids::{FilePatch, PatchId};

/// Reads the patch-id of each file `commit` changes since its merge base with the branch `target`, or with
/// the branch before the merge commit that brought `commit` in: one with every whitespace change counted,
/// and one with whitespace ignored. Both have to be in `mirror`. The diff
/// names each file's blobs in full, so a binary file's patch-id follows its content, as git's abbreviations
/// grow with the mirror.
pub fn read_patch_ids(
    mirror: &Path,
    target: &str,
    commit: &CommitSha,
) -> io::Result<Vec<FilePatch>> {
    let branch = format!("refs/heads/{target}");
    let mut base = merge_base(mirror, &branch, commit)?;
    if base == *commit {
        base = base_before_merge(mirror, &branch, commit)?;
    }
    // A commit pushed straight to the branch has no merge to look past, and its empty diff would be kept for
    // good.
    if base == *commit {
        let words = format!("{} is already on {target}", commit.as_str());
        return Err(io::Error::other(words));
    }
    let range = [base.as_str(), commit.as_str()];
    let diff = |options: &[&'static str]| {
        let command = [
            CONFIG.as_slice(),
            &["diff"],
            options,
            &DIFF_OPTIONS,
            &PINNED,
        ];
        [command.concat().as_slice(), &range].concat()
    };
    // The list names each file exactly, where the patch quotes odd names, and the two list them in the same
    // order. A renamed file goes by its new name.
    let names = git(mirror, &diff(&["--name-only", "-z"]))?;
    let paths: Vec<&str> = names.split('\0').filter(|path| !path.is_empty()).collect();
    let patch = diff(&["--full-index"]);
    let input = numbered(&git_bytes(mirror, &patch, Vec::new())?, paths.len())?;
    let verbatim = patch_ids(mirror, "--verbatim", input.clone())?;
    let loose = patch_ids(mirror, "--stable", input)?;
    let index = TargetIndex::new(mirror);
    index.make(target)?;
    let generated = index.generated(&paths)?;
    let ids = Ids { verbatim, loose };
    file_patches(&paths, generated, ids)
}

/// The merge base of `commit` with `branch` as it stood before the merge commit that brought `commit` in:
/// the first parent of the oldest commit on the branch's first-parent line that holds it. A PR merged with
/// a merge commit (#385) has its head and the commits before it on the branch, so their merge base with it
/// is themselves. Gives `commit` back where no merge brought it in.
fn base_before_merge(mirror: &Path, branch: &str, commit: &CommitSha) -> io::Result<CommitSha> {
    let range = format!("{}..{branch}", commit.as_str());
    let commits = |args: &[&str]| -> io::Result<Vec<CommitSha>> {
        let listed = git(mirror, &[["rev-list"].as_slice(), args, &[&range]].concat())?;
        Ok(listed.lines().filter_map(CommitSha::parse).collect())
    };
    let holding: BTreeSet<CommitSha> = commits(&["--ancestry-path"])?.into_iter().collect();
    let line = commits(&["--first-parent"])?;
    match line.into_iter().rfind(|on_line| holding.contains(on_line)) {
        Some(merge) => merge_base(mirror, &format!("{}^1", merge.as_str()), commit),
        None => Ok(commit.clone()),
    }
}

/// The user's git config that changes what `git diff` writes and has no option to override it, set back to
/// git's defaults. A change to it would otherwise change every file's patch-id, and make each Review stale.
const CONFIG: [&str; 14] = [
    "-c",
    "core.quotePath=true",
    "-c",
    "core.attributesFile=",
    "-c",
    "diff.noprefix=false",
    "-c",
    "diff.mnemonicPrefix=false",
    "-c",
    "diff.suppressBlankEmpty=false",
    "-c",
    "diff.interHunkContext=0",
    "-c",
    "diff.renameLimit=1000",
];

/// The options beside `DIFF_OPTIONS` that override the rest of the user's diff config, for the same reason.
const PINNED: [&str; 5] = [
    "--diff-algorithm=myers",
    "--indent-heuristic",
    "--src-prefix=a/",
    "--dst-prefix=b/",
    "--ignore-submodules=none",
];

/// The patch-ids `git patch-id` gave each numbered section, both ways.
struct Ids {
    verbatim: BTreeMap<usize, PatchId>,
    loose: BTreeMap<usize, PatchId>,
}

/// `patch-id`'s input: each file's section of `patch`, after a made-up commit line that numbers it. The
/// list of files names `files` of them.
fn numbered(patch: &[u8], files: usize) -> io::Result<Vec<u8>> {
    let sections = sections(patch);
    if sections.len() != files {
        let sections = sections.len();
        let words = format!("the patch has {sections} files, and the list {files}");
        return Err(io::Error::other(words));
    }
    let numbered = (sections.iter().enumerate()).flat_map(|(at, section)| {
        [format!("commit {:040x}\n", at + 1).as_bytes(), section].concat()
    });
    Ok(numbered.collect())
}

/// Each of `paths`, with whether it is `generated` and its patch-ids from `ids`, in order.
fn file_patches(paths: &[&str], generated: Vec<bool>, mut ids: Ids) -> io::Result<Vec<FilePatch>> {
    let mut files = Vec::new();
    for (at, (path, generated)) in paths.iter().zip(generated).enumerate() {
        let (Some(verbatim), Some(loose)) = (ids.verbatim.remove(&at), ids.loose.remove(&at))
        else {
            return Err(io::Error::other(format!(
                "git patch-id gave {path} no patch-id"
            )));
        };
        files.push(FilePatch {
            path: (*path).to_owned(),
            verbatim,
            loose,
            generated,
        });
    }
    Ok(files)
}

/// The patch cut into one section per file, each from its `diff --git` line, as git wrote it. git writes a
/// file that became a symlink or a submodule, or the other way, as a deletion and an addition under the same
/// `diff --git` line, which the list names once, so those two make one section.
fn sections(patch: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut at = 0;
    let mut header: &[u8] = &[];
    for line in patch.split_inclusive(|byte| *byte == b'\n') {
        if line.starts_with(b"diff --git ") && line != header {
            starts.push(at);
            header = line;
        }
        at += line.len();
    }
    let ends = starts.iter().skip(1).copied().chain([patch.len()]);
    (starts.iter().zip(ends))
        .map(|(start, end)| patch.get(*start..end).unwrap_or_default())
        .collect()
}

/// The patch-id `git patch-id` with `mode` gives each numbered section of `input`, by number.
fn patch_ids(mirror: &Path, mode: &str, input: Vec<u8>) -> io::Result<BTreeMap<usize, PatchId>> {
    let output = git_bytes(mirror, &["patch-id", mode], input)?;
    let output = String::from_utf8_lossy(&output);
    let ids = output.lines().filter_map(|line| {
        let (id, number) = line.split_once(' ')?;
        let number = usize::from_str_radix(number, 16).ok()?.checked_sub(1)?;
        Some((number, PatchId::parse(id)?))
    });
    Ok(ids.collect())
}

fn git_bytes(mirror: &Path, args: &[&str], input: Vec<u8>) -> io::Result<Vec<u8>> {
    let mut all = vec![OsStr::new("-C"), mirror.as_os_str()];
    all.extend(args.iter().map(OsStr::new));
    live::read_git_bytes(&all, input)
}

#[cfg(test)]
mod tests;
