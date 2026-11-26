//! GitHub's answer to a search typed in the Inbox (#465): the results it listed, in its order, and how many
//! the search reaches. One answer stands for a Repository at a time: a new query drops the answer to the old
//! one.

use rusqlite::{OptionalExtension, Row, named_params};

use super::Connection;
use super::columns::UnixTime;
use crate::facts::inbox_search::{InboxSearch, SearchHit};
use crate::repository::RepositoryName;

pub(super) const SCHEMA: &str = "
    CREATE TABLE inbox_searches (
        repository TEXT NOT NULL COLLATE NOCASE,
        query TEXT NOT NULL,
        total INTEGER NOT NULL,
        cut_off INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (repository, query)
    ) WITHOUT ROWID;

    CREATE TABLE inbox_search_hits (
        repository TEXT NOT NULL COLLATE NOCASE,
        query TEXT NOT NULL,
        position INTEGER NOT NULL,
        number INTEGER NOT NULL,
        kind TEXT NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (repository, query, position)
    ) WITHOUT ROWID;
";

/// Keeps `search` as `repository`'s answer to `query`, dropping the answer to every other query, in one
/// transaction (#465).
pub(super) fn save(
    connection: &mut Connection,
    repository: &RepositoryName,
    query: &str,
    search: &InboxSearch,
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    for table in ["inbox_search_hits", "inbox_searches"] {
        transaction
            .execute(
                &format!("DELETE FROM {table} WHERE repository = :repository"),
                named_params! { ":repository": repository },
            )
            .map(drop)?;
    }
    transaction
        .execute(
            "INSERT INTO inbox_searches (repository, query, total, cut_off)
             VALUES (:repository, :query, :total, :cut_off)",
            named_params! {
                ":repository": repository,
                ":query": query,
                ":total": search.total,
                ":cut_off": search.cut_off,
            },
        )
        .map(drop)?;
    for (position, hit) in search.hits.iter().enumerate() {
        let position = i64::try_from(position).unwrap_or(i64::MAX);
        transaction
            .execute(
                "INSERT INTO inbox_search_hits (repository, query, position, number, kind, updated_at)
                 VALUES (:repository, :query, :position, :number, :kind, :updated_at)",
                named_params! {
                    ":repository": repository,
                    ":query": query,
                    ":position": position,
                    ":number": hit.number,
                    ":kind": hit.kind,
                    ":updated_at": hit.updated_at.unix_timestamp(),
                },
            )
            .map(drop)?;
    }
    transaction.commit()
}

/// `repository`'s answer to `query`, if it has one (#465).
pub(super) fn read(
    connection: &Connection,
    repository: &RepositoryName,
    query: &str,
) -> rusqlite::Result<Option<InboxSearch>> {
    let state = connection
        .query_row(
            "SELECT total, cut_off FROM inbox_searches
             WHERE repository = :repository AND query = :query",
            named_params! { ":repository": repository, ":query": query },
            |row| Ok((row.get::<_, u32>("total")?, row.get::<_, bool>("cut_off")?)),
        )
        .optional()?;
    let Some((total, cut_off)) = state else {
        return Ok(None);
    };
    let sql = "SELECT number, kind, updated_at FROM inbox_search_hits
               WHERE repository = :repository AND query = :query ORDER BY position";
    let mut statement = connection.prepare(sql)?;
    let asked = named_params! { ":repository": repository, ":query": query };
    let rows = statement.query_map(asked, hit_in)?;
    let hits: Vec<SearchHit> = rows.collect::<rusqlite::Result<_>>()?;
    Ok(Some(InboxSearch {
        hits,
        total,
        cut_off,
    }))
}

fn hit_in(row: &Row<'_>) -> rusqlite::Result<SearchHit> {
    let UnixTime(updated_at) = row.get("updated_at")?;
    Ok(SearchHit {
        number: row.get("number")?,
        kind: row.get("kind")?,
        updated_at,
    })
}

#[cfg(test)]
mod tests;
