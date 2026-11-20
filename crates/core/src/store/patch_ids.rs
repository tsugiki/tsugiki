//! The per-file patch-ids of commits, which the Stale review check compares (#383). A commit can't change,
//! so they are kept by commit and Target branch, the one cache Tsugiki computes rather than reads from GitHub
//! (ADR 0010).

use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, Row, Transaction, named_params};
use time::OffsetDateTime;

use super::Connection;
use super::columns::UnixTime;
use super::pull_request_content::reviewed_commits;
use crate::facts::CommitSha;
use crate::facts::patch_ids::{
    CantCheck, CommitPatches, FilePatch, Findings, Found, LastCheck, PatchesOf, ReviewFacts,
    StaleFinding, StaleFound, UncheckedCommit,
};
use crate::facts::pull_requests::PullRequestFacts;
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

/// Each PR's last finding that the user's Review of `reviewed_commit` is stale against `head_commit`, or that
/// the check of the two can't run, first found at `found_at` (#908). `acknowledged_head` is `head_commit`
/// once a mark acknowledged that finding as the window drew it, and null before.
///
/// The window before these were kept could show a Stale review where a check of the user's Review couldn't
/// run, as `unchecked_commits` says of the last run, or had run: on the PR's head where the patch-ids of
/// both commits were read, and otherwise on the head `last_checks` says it compared. Each of those counts
/// as found in 1970 and acknowledged, so its Stale review still waits on the push alone, as it did then. A
/// Review whose check never ran showed none, so the first check after the upgrade finds it afresh. One the
/// check finds the same ends at the first check after the upgrade.
pub(super) const STALE_REVIEWS: &str = "
    CREATE TABLE stale_reviews (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        reviewed_commit TEXT NOT NULL,
        head_commit TEXT NOT NULL,
        found_at INTEGER NOT NULL,
        acknowledged_head TEXT,
        PRIMARY KEY (repository, number)
    ) WITHOUT ROWID;
    CREATE TEMP VIEW moved_on AS
        SELECT pr.repository, pr.number, pr.head, pr.target_branch, review.commit_sha AS reviewed
        FROM pull_requests AS pr
        JOIN viewer ON viewer.id = 1
        JOIN pr_reviews AS review ON review.repository = pr.repository AND review.number = pr.number
        WHERE review.position = (SELECT MAX(mine.position) FROM pr_reviews AS mine
                                 WHERE mine.repository = pr.repository AND mine.number = pr.number
                                   AND mine.author = viewer.login COLLATE NOCASE)
          AND review.commit_sha IS NOT NULL AND pr.head IS NOT NULL AND pr.head <> review.commit_sha;
    INSERT INTO stale_reviews
            (repository, number, reviewed_commit, head_commit, found_at, acknowledged_head)
        SELECT repository, number, reviewed, head, 0, head FROM moved_on
        WHERE EXISTS (SELECT 1 FROM unchecked_commits AS unchecked
                      WHERE unchecked.repository = moved_on.repository
                        AND unchecked.target_branch = moved_on.target_branch
                        AND unchecked.commit_sha IN (moved_on.reviewed, moved_on.head));
    INSERT OR IGNORE INTO stale_reviews
            (repository, number, reviewed_commit, head_commit, found_at, acknowledged_head)
        SELECT repository, number, reviewed, head, 0, head FROM moved_on
        WHERE (SELECT COUNT(*) FROM patched_commits AS patched
               WHERE patched.repository = moved_on.repository
                 AND patched.target_branch = moved_on.target_branch
                 AND patched.commit_sha IN (moved_on.reviewed, moved_on.head)) = 2;
    INSERT OR IGNORE INTO stale_reviews
            (repository, number, reviewed_commit, head_commit, found_at, acknowledged_head)
        SELECT moved_on.repository, moved_on.number, reviewed, last.head_commit, 0, last.head_commit
        FROM moved_on
        JOIN last_checks AS last ON last.repository = moved_on.repository AND last.number = moved_on.number
                                AND last.reviewed_commit = moved_on.reviewed;
    DROP VIEW moved_on;
";

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
        transaction
            .execute(
                "INSERT OR IGNORE INTO patched_commits (repository, commit_sha, target_branch)
                 VALUES (:repository, :commit, :target)",
                commit,
            )
            .map(drop)?;
        transaction
            .execute(
                "DELETE FROM file_patch_ids
                 WHERE repository = :repository AND commit_sha = :commit AND target_branch = :target",
                commit,
            )
            .map(drop)?;
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
            statement.execute(row).map(drop)?;
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
    transaction
        .execute(
            "DELETE FROM unchecked_commits WHERE repository = :repository",
            named_params! { ":repository": repository },
        )
        .map(drop)?;
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
        statement.execute(row).map(drop)?;
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
        statement.execute(row).map(drop)?;
    }
    drop(statement);
    transaction.commit()
}

/// Keeps `findings`: each found Review dated `now` and not yet acknowledged, in place of what was kept of
/// its PR, and each ended one forgotten.
pub(super) fn save_findings(
    connection: &mut Connection,
    repository: &RepositoryName,
    findings: &Findings,
    now: OffsetDateTime,
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    let mut found = transaction.prepare(
        "INSERT OR REPLACE INTO stale_reviews
             (repository, number, reviewed_commit, head_commit, found_at, acknowledged_head)
         VALUES (:repository, :number, :reviewed, :head, :now, NULL)",
    )?;
    for Found {
        number,
        reviewed,
        head,
    } in &findings.found
    {
        let row = named_params! {
            ":repository": repository,
            ":number": number,
            ":reviewed": reviewed,
            ":head": head,
            ":now": now.unix_timestamp(),
        };
        found.execute(row).map(drop)?;
    }
    drop(found);
    let mut ended = transaction
        .prepare("DELETE FROM stale_reviews WHERE repository = :repository AND number = :number")?;
    for number in &findings.ended {
        let row = named_params! { ":repository": repository, ":number": number };
        ended.execute(row).map(drop)?;
    }
    drop(ended);
    transaction.commit()
}

/// Acknowledges each of `findings`, which a mark carries as the window drew them, where the store still
/// keeps that finding, in the mark's `transaction`. One the window never drew, as a later head's, stays as
/// it is.
pub(super) fn acknowledge_in(
    transaction: &Transaction<'_>,
    repository: &RepositoryName,
    findings: &[StaleFinding],
) -> rusqlite::Result<()> {
    let mut statement = transaction.prepare(
        "UPDATE stale_reviews SET acknowledged_head = head_commit
         WHERE repository = :repository AND number = :number AND reviewed_commit = :reviewed
           AND head_commit = :head AND found_at = :found_at",
    )?;
    for finding in findings {
        let row = named_params! {
            ":repository": repository,
            ":number": finding.number,
            ":reviewed": finding.reviewed,
            ":head": finding.head,
            ":found_at": finding.found_at.unix_timestamp(),
        };
        statement.execute(row).map(drop)?;
    }
    Ok(())
}

/// The review facts of `pull_requests`: the reviewed commit of each, and, where the head moved on from it,
/// the patch-ids held of both commits against its Target branch, why the worker can't read those it can't,
/// the PR's last check of the same Review with the patch-ids of the head it compared, and its last finding
/// that the user's Review is stale.
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
            patches.extend([(of, files)]);
        }
    }
    Ok(ReviewFacts {
        reviewed,
        patches,
        unchecked: read_unchecked(transaction, repository)?,
        last_checks,
        stale_found: read_stale_found(transaction, repository)?,
    })
}

fn read_stale_found(
    transaction: &Transaction<'_>,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<u32, StaleFound>> {
    let mut statement = transaction.prepare(
        "SELECT number, reviewed_commit, head_commit, found_at, acknowledged_head FROM stale_reviews
         WHERE repository = :repository",
    )?;
    let asked = named_params! { ":repository": repository };
    let found = statement.query_map(asked, |row| {
        let UnixTime(at) = row.get("found_at")?;
        let head: CommitSha = row.get("head_commit")?;
        let acknowledged: Option<CommitSha> = row.get("acknowledged_head")?;
        let found = StaleFound {
            reviewed: row.get("reviewed_commit")?,
            acknowledged: acknowledged.as_ref() == Some(&head),
            head,
            at,
        };
        Ok((row.get("number")?, found))
    })?;
    found.collect()
}

fn read_last_checks(
    transaction: &Transaction<'_>,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<u32, LastCheck>> {
    let mut statement = transaction.prepare(
        "SELECT number, reviewed_commit, head_commit FROM last_checks WHERE repository = :repository",
    )?;
    let asked = named_params! { ":repository": repository };
    let checks = statement.query_map(asked, last_check_in)?;
    checks.collect()
}

fn last_check_in(row: &Row<'_>) -> rusqlite::Result<(u32, LastCheck)> {
    let check = LastCheck {
        reviewed: row.get("reviewed_commit")?,
        head: row.get("head_commit")?,
    };
    Ok((row.get("number")?, check))
}

fn read_unchecked(
    transaction: &Transaction<'_>,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<PatchesOf, CantCheck>> {
    let mut statement = transaction.prepare(
        "SELECT commit_sha, target_branch, reason, git_version, git_needed FROM unchecked_commits
         WHERE repository = :repository",
    )?;
    let asked = named_params! { ":repository": repository };
    let unchecked = statement.query_map(asked, unchecked_in)?;
    unchecked.collect()
}

fn unchecked_in(row: &Row<'_>) -> rusqlite::Result<(PatchesOf, CantCheck)> {
    let of = PatchesOf {
        commit: row.get("commit_sha")?,
        target_branch: row.get("target_branch")?,
    };
    Ok((of, reason_in(row)?))
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
