//! The per-file patch-ids of commits, which the Stale review check compares (#383). A commit can't change,
//! so they are kept by commit and Target branch, the one cache Tsugiki computes rather than reads from GitHub
//! (ADR 0010).

use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, Row, Transaction, named_params};

use super::Connection;
use super::pull_request_content::reviewed_commits;
use crate::github::CommitSha;
use crate::github::pull_requests::PullRequestFacts;
use crate::patch_ids::{CommitPatches, FilePatch, PatchesOf};
use crate::repository::RepositoryName;

/// A row for each commit whose patch-ids were read, which says so even of a commit with no files to diff,
/// and a row for each file of it.
pub(super) const SCHEMA: &str = "
    CREATE TABLE patched_commits (
        repository TEXT NOT NULL COLLATE NOCASE,
        commit_sha TEXT NOT NULL,
        target_branch TEXT NOT NULL,
        PRIMARY KEY (repository, commit_sha, target_branch)
    ) WITHOUT ROWID;
    CREATE TABLE file_patch_ids (
        repository TEXT NOT NULL COLLATE NOCASE,
        commit_sha TEXT NOT NULL,
        target_branch TEXT NOT NULL,
        position INTEGER NOT NULL,
        path TEXT NOT NULL,
        verbatim TEXT NOT NULL,
        loose TEXT NOT NULL,
        generated INTEGER NOT NULL,
        PRIMARY KEY (repository, commit_sha, target_branch, position),
        FOREIGN KEY (repository, commit_sha, target_branch)
            REFERENCES patched_commits (repository, commit_sha, target_branch)
    ) WITHOUT ROWID;
";

/// What the review state of each PR is worked out from: the commit the user's Review covered, and the
/// patch-ids the store holds of that commit and of the head.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReviewFacts {
    /// The commit of the user's Review of each PR whose Conversation Tsugiki has read.
    pub reviewed: BTreeMap<u32, CommitSha>,
    /// The per-file patch-ids of each reviewed commit and head a check compares, once they have been read.
    pub patches: BTreeMap<PatchesOf, Vec<FilePatch>>,
}

/// Keeps each of `patches`, in place of what was kept of the same commit before.
pub(super) fn save(
    connection: &mut Connection,
    repository: &RepositoryName,
    patches: &[CommitPatches],
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    for CommitPatches { of, files } in patches {
        let commit = named_params! {
            ":repository": repository,
            ":commit": of.commit,
            ":target": of.target_branch,
        };
        let _ = transaction.execute(
            "INSERT OR IGNORE INTO patched_commits (repository, commit_sha, target_branch)
             VALUES (:repository, :commit, :target)",
            commit,
        )?;
        let _ = transaction.execute(
            "DELETE FROM file_patch_ids
             WHERE repository = :repository AND commit_sha = :commit AND target_branch = :target",
            commit,
        )?;
        let mut statement = transaction.prepare(
            "INSERT INTO file_patch_ids
                 (repository, commit_sha, target_branch, position, path, verbatim, loose, generated)
             VALUES (:repository, :commit, :target, :position, :path, :verbatim, :loose, :generated)",
        )?;
        for (position, file) in (0_u32..).zip(files) {
            let row = named_params! {
                ":repository": repository,
                ":commit": of.commit,
                ":target": of.target_branch,
                ":position": position,
                ":path": file.path,
                ":verbatim": file.verbatim,
                ":loose": file.loose,
                ":generated": file.generated,
            };
            let _ = statement.execute(row)?;
        }
    }
    transaction.commit()
}

/// The review facts of `pull_requests`: the reviewed commit of each, and, where the head moved on from it,
/// the patch-ids held of both commits against its Target branch.
pub(super) fn read_facts(
    transaction: &Transaction<'_>,
    repository: &RepositoryName,
    pull_requests: &[PullRequestFacts],
) -> rusqlite::Result<ReviewFacts> {
    let reviewed: BTreeMap<u32, CommitSha> = reviewed_commits(transaction, repository)?
        .into_iter()
        .map(|reviewed| (reviewed.number, reviewed.commit))
        .collect();
    // Only a head that moved on from the reviewed commit needs a check.
    let compared = pull_requests.iter().filter_map(|pr| {
        let reviewed = reviewed.get(&pr.number)?;
        let head = pr.head.as_ref().filter(|head| *head != reviewed)?;
        let of = |commit: &CommitSha| PatchesOf {
            commit: commit.clone(),
            target_branch: pr.target_branch.clone(),
        };
        Some([of(reviewed), of(head)])
    });
    let mut patches = BTreeMap::new();
    for of in compared.flatten().collect::<Vec<_>>() {
        if !patches.contains_key(&of)
            && let Some(files) = read(transaction, repository, &of)?
        {
            drop(patches.insert(of, files));
        }
    }
    Ok(ReviewFacts { reviewed, patches })
}

/// The patch-ids held of `of`, if its were read.
fn read(
    transaction: &Transaction<'_>,
    repository: &RepositoryName,
    of: &PatchesOf,
) -> rusqlite::Result<Option<Vec<FilePatch>>> {
    let asked = named_params! {
        ":repository": repository,
        ":commit": of.commit,
        ":target": of.target_branch,
    };
    let held = transaction
        .prepare(
            "SELECT 1 FROM patched_commits
             WHERE repository = :repository AND commit_sha = :commit AND target_branch = :target",
        )?
        .query_row(asked, |_| Ok(()))
        .optional()?;
    if held.is_none() {
        return Ok(None);
    }
    let mut statement = transaction.prepare(
        "SELECT path, verbatim, loose, generated FROM file_patch_ids
         WHERE repository = :repository AND commit_sha = :commit AND target_branch = :target
         ORDER BY position",
    )?;
    let files = statement.query_map(asked, file_in)?;
    files.collect::<rusqlite::Result<_>>().map(Some)
}

fn file_in(row: &Row<'_>) -> rusqlite::Result<FilePatch> {
    Ok(FilePatch {
        path: row.get("path")?,
        verbatim: row.get("verbatim")?,
        loose: row.get("loose")?,
        generated: row.get("generated")?,
    })
}

#[cfg(test)]
mod tests;
