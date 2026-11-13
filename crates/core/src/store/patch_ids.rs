//! The per-file patch-ids of commits, which the Stale review check compares (#383). A commit can't change,
//! so they are kept by commit and Target branch, the one cache Tsugiki computes rather than reads from GitHub
//! (ADR 0010).

use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, Row, Transaction, named_params};

use super::Connection;
use super::pull_request_content::reviewed_commits;
use crate::github::CommitSha;
use crate::github::pull_requests::PullRequestFacts;
use crate::patch_ids::{CantCheck, CommitPatches, FilePatch, PatchesOf, UncheckedCommit};
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

/// Why the Mirror worker can't read a commit's patch-ids, as it last said in this run, and the two commits
/// each PR's last finished check compared (#384). `git_version` and `git_needed` go with an `old_git`
/// reason only.
pub(super) const CHECKS: &str = "
    CREATE TABLE unchecked_commits (
        repository TEXT NOT NULL COLLATE NOCASE,
        commit_sha TEXT NOT NULL,
        target_branch TEXT NOT NULL,
        reason TEXT NOT NULL,
        git_version TEXT,
        git_needed TEXT,
        PRIMARY KEY (repository, commit_sha, target_branch)
    ) WITHOUT ROWID;
    CREATE TABLE last_checks (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        reviewed_commit TEXT NOT NULL,
        head_commit TEXT NOT NULL,
        PRIMARY KEY (repository, number)
    ) WITHOUT ROWID;
";

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
}

/// The two commits a PR's check compared once the patch-ids of both were read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastCheck {
    pub reviewed: CommitSha,
    pub head: CommitSha,
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

/// Keeps `unchecked` as the commits of `repository` whose patch-ids the Mirror worker can't read, in place of
/// those it kept before.
pub(super) fn save_unchecked(
    connection: &mut Connection,
    repository: &RepositoryName,
    unchecked: &[UncheckedCommit],
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    let _ = transaction.execute(
        "DELETE FROM unchecked_commits WHERE repository = :repository",
        named_params! { ":repository": repository },
    )?;
    let mut statement = transaction.prepare(
        "INSERT OR REPLACE INTO unchecked_commits
             (repository, commit_sha, target_branch, reason, git_version, git_needed)
         VALUES (:repository, :commit, :target, :reason, :version, :needed)",
    )?;
    for UncheckedCommit { of, reason } in unchecked {
        let (reason, version, needed) = match reason {
            CantCheck::Cloning => ("cloning", None, None),
            CantCheck::Broken => ("broken", None, None),
            CantCheck::NoGit => ("no_git", None, None),
            CantCheck::OldGit { version, needed } => ("old_git", Some(version), Some(needed)),
            CantCheck::Refused => ("refused", None, None),
        };
        let row = named_params! {
            ":repository": repository,
            ":commit": of.commit,
            ":target": of.target_branch,
            ":reason": reason,
            ":version": version,
            ":needed": needed,
        };
        let _ = statement.execute(row)?;
    }
    drop(statement);
    transaction.commit()
}

/// Forgets why the Mirror worker couldn't read patch-ids, which says how the mirrors stood in a run that
/// ended.
pub(super) fn forget_unchecked(connection: &Connection) -> rusqlite::Result<()> {
    connection
        .execute("DELETE FROM unchecked_commits", [])
        .map(drop)
}

/// Keeps each of `checks` as the last check of its PR, in place of the one kept before.
pub(super) fn save_last_checks(
    connection: &mut Connection,
    repository: &RepositoryName,
    checks: &[(u32, LastCheck)],
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    let mut statement = transaction.prepare(
        "INSERT INTO last_checks (repository, number, reviewed_commit, head_commit)
         VALUES (:repository, :number, :reviewed, :head)
         ON CONFLICT (repository, number) DO UPDATE SET
             reviewed_commit = excluded.reviewed_commit, head_commit = excluded.head_commit",
    )?;
    for (number, check) in checks {
        let row = named_params! {
            ":repository": repository,
            ":number": number,
            ":reviewed": check.reviewed,
            ":head": check.head,
        };
        let _ = statement.execute(row)?;
    }
    drop(statement);
    transaction.commit()
}

/// The review facts of `pull_requests`: the reviewed commit of each, and, where the head moved on from it,
/// the patch-ids held of both commits against its Target branch, why the worker can't read those it can't,
/// and the PR's last check of the same Review with the patch-ids of the head it compared.
pub(super) fn read_facts(
    transaction: &Transaction<'_>,
    repository: &RepositoryName,
    pull_requests: &[PullRequestFacts],
) -> rusqlite::Result<ReviewFacts> {
    let reviewed: BTreeMap<u32, CommitSha> = reviewed_commits(transaction, repository)?
        .into_iter()
        .map(|reviewed| (reviewed.number, reviewed.commit))
        .collect();
    let last_checks = read_last_checks(transaction, repository)?;
    // Only a head that moved on from the reviewed commit needs a check.
    let compared = pull_requests.iter().filter_map(|pr| {
        let reviewed = reviewed.get(&pr.number)?;
        let head = pr.head.as_ref().filter(|head| *head != reviewed)?;
        let of = |commit: &CommitSha| PatchesOf {
            commit: commit.clone(),
            target_branch: pr.target_branch.clone(),
        };
        let last = (last_checks.get(&pr.number)).filter(|last| last.reviewed == *reviewed);
        let last = last.map(|last| of(&last.head));
        Some([Some(of(reviewed)), Some(of(head)), last])
    });
    let mut patches = BTreeMap::new();
    for of in compared.flatten().flatten().collect::<Vec<_>>() {
        if !patches.contains_key(&of)
            && let Some(files) = read(transaction, repository, &of)?
        {
            drop(patches.insert(of, files));
        }
    }
    Ok(ReviewFacts {
        reviewed,
        patches,
        unchecked: read_unchecked(transaction, repository)?,
        last_checks,
    })
}

fn read_last_checks(
    transaction: &Transaction<'_>,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<u32, LastCheck>> {
    let mut statement = transaction.prepare(
        "SELECT number, reviewed_commit, head_commit FROM last_checks WHERE repository = :repository",
    )?;
    let checks = statement.query_map(named_params! { ":repository": repository }, |row| {
        let check = LastCheck {
            reviewed: row.get("reviewed_commit")?,
            head: row.get("head_commit")?,
        };
        Ok((row.get("number")?, check))
    })?;
    checks.collect()
}

fn read_unchecked(
    transaction: &Transaction<'_>,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<PatchesOf, CantCheck>> {
    let mut statement = transaction.prepare(
        "SELECT commit_sha, target_branch, reason, git_version, git_needed FROM unchecked_commits
         WHERE repository = :repository",
    )?;
    let unchecked = statement.query_map(named_params! { ":repository": repository }, |row| {
        let of = PatchesOf {
            commit: row.get("commit_sha")?,
            target_branch: row.get("target_branch")?,
        };
        Ok((of, reason_in(row)?))
    })?;
    unchecked.collect()
}

fn reason_in(row: &Row<'_>) -> rusqlite::Result<CantCheck> {
    let reason: String = row.get("reason")?;
    let version: Option<String> = row.get("git_version")?;
    let needed: Option<String> = row.get("git_needed")?;
    match (reason.as_str(), version, needed) {
        ("cloning", None, None) => Ok(CantCheck::Cloning),
        ("broken", None, None) => Ok(CantCheck::Broken),
        ("no_git", None, None) => Ok(CantCheck::NoGit),
        ("old_git", Some(version), Some(needed)) => Ok(CantCheck::OldGit { version, needed }),
        ("refused", None, None) => Ok(CantCheck::Refused),
        _ => Err(rusqlite::Error::FromSqlConversionFailure(
            row.as_ref().column_index("reason")?,
            rusqlite::types::Type::Text,
            format!("{reason} isn't a reason the check can't run").into(),
        )),
    }
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
