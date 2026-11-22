//! The counts of open Issues and PRs GitHub last gave for each Repository, which a sync learns on its way
//! and the Repository's page in Settings shows (#534).

use rusqlite::{OptionalExtension, named_params};

use super::Connection;
use crate::repository::{OpenItems, RepositoryName};

#[cfg(test)]
mod tests;

/// One row per Repository a sync has counted.
pub(super) const SCHEMA: &str = "
    CREATE TABLE open_items (
        repository TEXT NOT NULL COLLATE NOCASE PRIMARY KEY,
        issues INTEGER NOT NULL,
        prs INTEGER NOT NULL
    ) WITHOUT ROWID;
";

/// Keeps `open` as what GitHub last counted in `repository`, in place of what it counted before.
pub(super) fn save(
    connection: &Connection,
    repository: &RepositoryName,
    open: OpenItems,
) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT OR REPLACE INTO open_items (repository, issues, prs)
             VALUES (:repository, :issues, :prs)",
            named_params! { ":repository": repository, ":issues": open.issues, ":prs": open.prs },
        )
        .map(drop)
}

/// The open items GitHub last counted in `repository`, or nothing if no sync has counted them.
pub(super) fn read(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<Option<OpenItems>> {
    connection
        .query_row(
            "SELECT issues, prs FROM open_items WHERE repository = :repository",
            named_params! { ":repository": repository },
            |row| {
                Ok(OpenItems {
                    issues: row.get("issues")?,
                    prs: row.get("prs")?,
                })
            },
        )
        .optional()
}
