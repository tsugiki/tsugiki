//! The marks of the Repositories the token can't write to (#398). Each keeps the kind and the expiry of the
//! token it was judged for, never the token, so a mark outlives a restart and goes with its token.

use rusqlite::{OptionalExtension, named_params};

use super::Connection;
use crate::read_only::TokenSeen;
use crate::repository::RepositoryName;
use crate::token::TokenKind;

/// One row per Repository the token can't write to, with the token's kind and expiry, in seconds since the
/// Unix epoch, or null for a token that doesn't expire.
pub(super) const SCHEMA: &str = "
    CREATE TABLE read_only (
        repository TEXT NOT NULL COLLATE NOCASE PRIMARY KEY,
        token_kind TEXT NOT NULL,
        token_expires_at INTEGER
    ) WITHOUT ROWID;
";

/// Forgets the marks of each token of another kind than `kind`, and gives the Repositories they were on.
pub(super) fn forget_other_kinds(
    connection: &Connection,
    kind: TokenKind,
) -> rusqlite::Result<Vec<RepositoryName>> {
    let mut statement = connection
        .prepare("DELETE FROM read_only WHERE token_kind <> :kind RETURNING repository")?;
    let forgot = statement.query_map(named_params! { ":kind": kind_column(kind) }, |row| {
        row.get(0)
    })?;
    forgot.collect()
}

/// Forgets the marks of each token but `token`, as a token of another kind or expiry is another token, and
/// gives the Repositories they were on.
pub(super) fn forget_other_tokens(
    connection: &Connection,
    token: TokenSeen,
) -> rusqlite::Result<Vec<RepositoryName>> {
    let mut statement = connection.prepare(
        "DELETE FROM read_only WHERE token_kind <> :kind OR token_expires_at IS NOT :expires
         RETURNING repository",
    )?;
    let (kind, expires) = token_columns(token);
    let named = named_params! { ":kind": kind, ":expires": expires };
    let forgot = statement.query_map(named, |row| row.get(0))?;
    forgot.collect()
}

/// Marks `repository` as one `token` can't write to, and gives whether it had no mark before.
pub(super) fn mark(
    connection: &mut Connection,
    token: TokenSeen,
    repository: &RepositoryName,
) -> rusqlite::Result<bool> {
    let transaction = connection.transaction()?;
    let before = marked(&transaction, repository)?;
    let (kind, expires) = token_columns(token);
    transaction
        .execute(
            "INSERT OR REPLACE INTO read_only (repository, token_kind, token_expires_at)
             VALUES (:repository, :kind, :expires)",
            named_params! { ":repository": repository, ":kind": kind, ":expires": expires },
        )
        .map(drop)?;
    transaction.commit()?;
    Ok(!before)
}

/// Clears `repository`'s mark, and gives whether it had one.
pub(super) fn clear(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<bool> {
    let cleared = connection.execute(
        "DELETE FROM read_only WHERE repository = :repository",
        named_params! { ":repository": repository },
    )?;
    Ok(cleared > 0)
}

/// Whether the store marks `repository` as one the token can't write to.
pub(super) fn marked(
    connection: &Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<bool> {
    let query = "SELECT 1 FROM read_only WHERE repository = :repository";
    let asked = named_params! { ":repository": repository };
    let found = connection.query_row(query, asked, |_| Ok(())).optional()?;
    Ok(found.is_some())
}

/// What the `token_kind` and `token_expires_at` columns hold of `token`.
fn token_columns(token: TokenSeen) -> (&'static str, Option<i64>) {
    (
        kind_column(token.kind),
        token.expires.map(|at| at.unix_timestamp()),
    )
}

/// How the `token_kind` column spells `kind`.
fn kind_column(kind: TokenKind) -> &'static str {
    match kind {
        TokenKind::OAuth => "oauth",
        TokenKind::Classic => "classic",
        TokenKind::FineGrained => "fine_grained",
        TokenKind::Other => "other",
    }
}

#[cfg(test)]
mod tests;
