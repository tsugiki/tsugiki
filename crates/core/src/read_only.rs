//! Whether Tsugiki can write to a Repository now (#398): **Read-only** when the user chose it or the token
//! can't write there, and **Offline** while GitHub can't be reached. Each write control follows the
//! Repository's state, and a write goes out only for a [`WritableRepository`] (#914).

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;
use tsugiki_token::NodeId;

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

/// A Repository Tsugiki may write to, with the item in it that one write goes to. Only
/// [`WritableRepository::new`] makes one, as the write goes out, and only when no reason holds the
/// Repository Read-only, so a write that gets past a write control still can't reach a Read-only
/// Repository. It holds the item's ID, read for its number in that Repository, so no write can pair it with
/// another Repository's item. It isn't `Clone`, and the client's write takes it by value, so it serves one
/// write.
#[derive(Debug)]
pub struct WritableRepository {
    repository: RepositoryName,
    subject: NodeId,
}

/// The reason no [`WritableRepository`] could be made for a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unwritable {
    /// The Repository is Read-only, for this reason first, in the order [`RepositoryState`] lists them.
    ReadOnly(ReadOnly),
    /// Tsugiki holds no ID for the item: it hasn't read it, or it read it before it asked for IDs.
    Unread,
}

impl WritableRepository {
    /// `repository`, writable, with `subject`, the ID of the item the write goes to, when none of `reasons`
    /// holds it Read-only and the ID is known. A reason comes first, so a Read-only Repository says so
    /// whatever Tsugiki holds of the item.
    ///
    /// `clippy.toml` refuses a call to this outside `session::write`, which works out the reasons from the
    /// settings in force and the store on each write, so a mark that changed since a control showed Send
    /// counts. A test of what a write does once it may go calls it too, and says so.
    pub fn new(
        repository: RepositoryName,
        subject: Option<NodeId>,
        reasons: impl IntoIterator<Item = ReadOnly>,
    ) -> Result<Self, Unwritable> {
        if let Some(reason) = reasons.into_iter().min() {
            return Err(Unwritable::ReadOnly(reason));
        }
        let subject = subject.ok_or(Unwritable::Unread)?;
        Ok(Self {
            repository,
            subject,
        })
    }

    /// The Repository the write goes to.
    pub fn repository(&self) -> &RepositoryName {
        &self.repository
    }

    /// GitHub's ID of the item the write goes to.
    pub fn subject(&self) -> &NodeId {
        &self.subject
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
