//! The names of the checks that failed on the user's PRs, read apart from the PRs because each PR runs too
//! many checks to ask about them all with every fetch.

use std::collections::BTreeMap;

use rusqlite::named_params;
use time::OffsetDateTime;

use super::Connection;
use crate::login::Login;
use crate::repository::RepositoryName;

pub(super) const SCHEMA: &str = "
    CREATE TABLE failed_checks (
        repository TEXT NOT NULL COLLATE NOCASE,
        number INTEGER NOT NULL,
        name TEXT NOT NULL,
        PRIMARY KEY (repository, number, name)
    ) WITHOUT ROWID;
    ALTER TABLE pull_requests ADD COLUMN checks_read_at INTEGER;
    CREATE INDEX activity_by_person ON activity (repository, who);
";

const SELECT_UNREAD: &str = "SELECT number FROM pull_requests
     WHERE repository = :repository AND author = :viewer AND state = 'OPEN'
         AND ci IN ('FAILURE', 'ERROR')
         AND (checks_read_at IS NULL OR ci_finished_at >= checks_read_at)
     ORDER BY number";

const SELECT_ALL: &str =
    "SELECT number, name FROM failed_checks WHERE repository = :repository ORDER BY number, name";

/// The numbers of `viewer`'s open PRs whose CI failed after Tsugiki last asked which checks failed. A failure
/// in the second Tsugiki asked counts, since asking twice costs little and a stale name misleads. Saving a PR
/// that moved forgets the last asking too, because a push can fail again with no pending state in between.
pub(super) fn unread(
    connection: &Connection,
    repository: &RepositoryName,
    viewer: &Login,
) -> rusqlite::Result<Vec<u32>> {
    let mut statement = connection.prepare(SELECT_UNREAD)?;
    let asked = named_params! { ":repository": repository, ":viewer": viewer };
    let rows = statement.query_map(asked, |row| row.get("number"))?;
    rows.collect()
}

fn save_one(
    connection: &Connection,
    repository: &RepositoryName,
    number: u32,
    names: &[String],
) -> rusqlite::Result<()> {
    let pull_request = named_params! { ":repository": repository, ":number": number };
    connection
        .execute(
            "DELETE FROM failed_checks WHERE repository = :repository AND number = :number",
            pull_request,
        )
        .map(drop)?;
    for name in names {
        connection
            .execute(
                "INSERT INTO failed_checks (repository, number, name)
                 VALUES (:repository, :number, :name)",
                named_params! { ":repository": repository, ":number": number, ":name": name },
            )
            .map(drop)?;
    }
    let now = OffsetDateTime::now_utc().unix_timestamp();
    connection
        .execute(
            "UPDATE pull_requests SET checks_read_at = :now
             WHERE repository = :repository AND number = :number",
            named_params! { ":repository": repository, ":number": number, ":now": now },
        )
        .map(drop)
}

/// Replaces what the store knows about each PR's failed checks.
pub(super) fn save(
    connection: &mut Connection,
    repository: &RepositoryName,
    read: &[(u32, Vec<String>)],
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    for (number, names) in read {
        save_one(&transaction, repository, *number, names)?;
    }
    transaction.commit()
}

/// Each PR's failed checks, by name.
pub(super) fn read_all(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<BTreeMap<u32, Vec<String>>> {
    let mut statement = connection.prepare(SELECT_ALL)?;
    let mut rows = statement.query(named_params! { ":repository": repository })?;
    let mut found: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let name = row.get("name")?;
        found.entry(row.get("number")?).or_default().push(name);
    }
    Ok(found)
}

#[cfg(test)]
mod tests;
