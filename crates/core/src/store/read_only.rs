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

/// What the token's scopes said of the Repositories the settings list.
pub enum TokenJudged {
    /// The scopes said nothing, as a fine-grained token's answers name none. The marks of this token stay.
    Silent,
    /// The scopes reach every listed Repository but these.
    CantWrite(Vec<RepositoryName>),
}

/// Keeps what the scopes of `token` said, as `judged`, and gives the Repositories of `listed` whose mark
/// changed. The marks of any other token go first: a token of another kind or expiry is another token.
pub(super) fn judge(
    connection: &mut Connection,
    token: TokenSeen,
    listed: &[RepositoryName],
    judged: &TokenJudged,
) -> rusqlite::Result<Vec<RepositoryName>> {
    let transaction = connection.transaction()?;
    let before = listed
        .iter()
        .map(|repository| marked(&transaction, repository))
        .collect::<rusqlite::Result<Vec<bool>>>()?;
    let (kind, expires) = (
        kind_column(token.kind),
        token.expires.map(|at| at.unix_timestamp()),
    );
    transaction
        .execute(
            "DELETE FROM read_only WHERE token_kind <> :kind OR token_expires_at IS NOT :expires",
            named_params! { ":kind": kind, ":expires": expires },
        )
        .map(drop)?;
    if let TokenJudged::CantWrite(repositories) = judged {
        transaction.execute("DELETE FROM read_only", []).map(drop)?;
        for repository in repositories {
            transaction
                .execute(
                    "INSERT OR IGNORE INTO read_only (repository, token_kind, token_expires_at)
                     VALUES (:repository, :kind, :expires)",
                    named_params! { ":repository": repository, ":kind": kind, ":expires": expires },
                )
                .map(drop)?;
        }
    }
    let mut changed = Vec::new();
    for (repository, was) in listed.iter().zip(before) {
        if marked(&transaction, repository)? != was {
            changed.push(repository.clone());
        }
    }
    transaction.commit()?;
    Ok(changed)
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
