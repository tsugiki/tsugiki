//! Whether Tsugiki can write to a Repository now (#398): **Read-only** when the user chose it or the token
//! can't write there, and **Offline** while GitHub can't be reached. Each write control follows the
//! Repository's state, and a write goes out only for a [`WritableRepository`] (#914).

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use crate::repository::{RepositoryName, Visibility};
use crate::token::{TokenKind, WriteScope};

/// Why Tsugiki can't write to a Repository, whatever the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ReadOnly {
    /// The user chose it: `read_only` in the Repository's settings.
    Chosen,
    /// The token can't write there: its scopes don't reach the Repository.
    Token,
}

/// Whether GitHub can be reached now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    Online,
    /// GitHub can't be reached, or hasn't answered since it couldn't be. What waits on GitHub goes once it
    /// answers.
    Offline,
}

/// A Repository's state, which each write control reads: why Tsugiki can't write there, if it can't, and
/// whether GitHub can be reached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct RepositoryState {
    /// Each reason Tsugiki can't write to the Repository, once, in a fixed order. None, when it can.
    pub read_only: Vec<ReadOnly>,
    pub connection: Connection,
}

impl RepositoryState {
    /// The state of a Repository Tsugiki can't write to for each of `reasons`, with `connection`. A reason
    /// given twice counts once.
    pub fn new(reasons: impl IntoIterator<Item = ReadOnly>, connection: Connection) -> Self {
        let mut read_only: Vec<ReadOnly> = reasons.into_iter().collect();
        read_only.sort_unstable();
        read_only.dedup();
        Self {
            read_only,
            connection,
        }
    }
}

/// A Repository Tsugiki may write to as a write goes out. Only [`WritableRepository::new`] makes one, and
/// only when no reason holds the Repository Read-only, so a write that gets past a write control still
/// can't reach a Read-only Repository. It isn't `Clone`, so none outlives the write it was made for.
#[derive(Debug)]
pub struct WritableRepository {
    repository: RepositoryName,
}

impl WritableRepository {
    /// `repository`, writable, when none of `reasons` holds it Read-only, or else the first reason, in the
    /// order [`RepositoryState`] lists them. The caller works the reasons out for each write, from the
    /// settings and the store, so a mark that changed since a control showed Send counts.
    pub fn new(
        repository: &RepositoryName,
        reasons: impl IntoIterator<Item = ReadOnly>,
    ) -> Result<Self, ReadOnly> {
        match reasons.into_iter().min() {
            Some(reason) => Err(reason),
            None => Ok(Self {
                repository: repository.clone(),
            }),
        }
    }

    /// The Repository a write may go to.
    pub fn repository(&self) -> &RepositoryName {
        &self.repository
    }
}

/// What the store keeps of the token beside each mark, never the token itself. A token of another kind or
/// expiry is another token, and its marks go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenSeen {
    pub kind: TokenKind,
    /// Nothing, for a token that doesn't expire.
    pub expires: Option<OffsetDateTime>,
}

/// Whether a token whose scopes let it write to `scope` can't write to a Repository whose visibility is
/// `visibility`. Nothing, when only the visibility can tell and it isn't known yet.
pub fn token_cant_write(scope: WriteScope, visibility: Option<Visibility>) -> Option<bool> {
    match scope {
        WriteScope::Every => Some(false),
        WriteScope::None => Some(true),
        WriteScope::Public => visibility.map(|visibility| visibility == Visibility::Private),
    }
}

#[cfg(test)]
mod tests;
