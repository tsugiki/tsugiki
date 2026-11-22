//! Whether Tsugiki can write to a Repository now (#398): **Read-only** when the token can't write there,
//! and **Offline** while GitHub can't be reached. Each write control follows the Repository's state.

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use crate::repository::Visibility;
use crate::token::{TokenKind, WriteScope};

/// Why Tsugiki can't write to a Repository, whatever the connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ReadOnly {
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
