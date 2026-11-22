//! The one way a write reaches GitHub (#914). It goes to a Repository no reason holds Read-only, the user's
//! choice or the token's mark, which it works out from the settings and the store as the write goes out, and
//! it names the item by the ID the store keeps for that number in that Repository.

use serde::de::DeserializeOwned;

use crate::error::{Error, ErrorKind};
use crate::facts::pull_requests::NodeId;
use crate::github::{GitHub, Selection, Write};
use crate::read_only::{ReadOnly, WritableRepository};
use crate::repository::RepositoryName;
use crate::repository_settings::RepositorySettings;
use crate::store::Store;

/// A write to an Issue or a PR, short of the item's ID, which [`Write`] names it by and the store gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToItem {
    /// A comment on an Issue, or on a PR's Conversation.
    AddComment { body: String },
}

impl ToItem {
    /// The write that does this to the item GitHub knows as `subject`.
    fn on(self, subject: NodeId) -> Write {
        match self {
            Self::AddComment { body } => Write::AddComment { subject, body },
        }
    }
}

/// A write to item `number` of `repository`, whose payload asks for `selection`.
pub struct ItemWrite<'a> {
    pub repository: &'a RepositorySettings,
    pub number: u32,
    pub write: ToItem,
    pub selection: Selection<'a>,
}

/// Sends `item_write` through `github`, and reads its answer as a read's. A Repository the user chose
/// Read-only for, or one the store marks as one the token can't write to, refuses it before anything goes,
/// as GitHub refuses a write, so the text stays in the composer with the reason. So does an item whose ID
/// the store doesn't hold yet.
pub fn send<D: DeserializeOwned>(
    github: &GitHub,
    store: &Store,
    item_write: ItemWrite<'_>,
) -> Result<D, Error> {
    let ItemWrite {
        repository,
        number,
        write,
        selection,
    } = item_write;
    let name = &repository.name;
    let token = store.token_cant_write(name)?.then_some(ReadOnly::Token);
    let reasons = [repository.chosen_read_only(), token].into_iter().flatten();
    let writable = WritableRepository::new(name, reasons).map_err(|why| read_only(why, name))?;
    let subject = store
        .node_id(name, number)?
        .ok_or_else(|| unread(name, number))?;
    github.write(&writable, &write.on(subject), selection)
}

/// The error of a write refused for `reason`.
fn read_only(reason: ReadOnly, repository: &RepositoryName) -> Error {
    let why = match reason {
        ReadOnly::Chosen => format!("you chose Read-only in {repository}'s settings"),
        ReadOnly::Token => format!("your token can't write to {repository}"),
    };
    Error::new(
        ErrorKind::WriteRefused,
        &format!("Tsugiki sent nothing, as {why}."),
    )
}

/// The error of a write to item `number` of `repository`, whose ID the store doesn't hold.
fn unread(repository: &RepositoryName, number: u32) -> Error {
    Error::new(
        ErrorKind::NotFound,
        &format!(
            "Tsugiki sent nothing, as it hasn't read #{number} of {repository} from GitHub yet. Open it \
             again, and send once it has."
        ),
    )
}

#[cfg(test)]
mod tests;
