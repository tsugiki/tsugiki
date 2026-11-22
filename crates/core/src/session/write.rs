//! The one way a write reaches GitHub (#914). It goes to a Repository no reason holds Read-only, the user's
//! choice or the token's mark, which it works out from the settings in force and the store as the write goes
//! out, and it names the item by the ID the store keeps for that number in that Repository.

use serde::de::DeserializeOwned;

use crate::error::{Error, ErrorKind};
use crate::github::{GitHub, Selection, ToItem};
use crate::latest::LatestReader;
use crate::read_only::{ReadOnly, Unwritable, WritableRepository};
use crate::repository::RepositoryName;
use crate::repository_settings::RepositorySettings;
use crate::settings::SettingsView;
use crate::store::StoreReader;

/// A write to item `number` of `repository`, whose payload asks for `selection`.
pub struct ItemWrite<'a> {
    pub repository: RepositoryName,
    pub number: u32,
    pub write: ToItem,
    pub selection: Selection<'a>,
}

/// The reasons that hold `repository` Read-only, from its settings and the store's mark, in no order: what
/// its state shows, and what refuses a write to it.
pub(crate) fn read_only_reasons(
    repository: &RepositorySettings,
    reader: &StoreReader,
) -> Result<Vec<ReadOnly>, Error> {
    let token = reader
        .token_cant_write(&repository.name)?
        .then_some(ReadOnly::Token);
    Ok([repository.chosen_read_only(), token]
        .into_iter()
        .flatten()
        .collect())
}

/// Sends `item_write` through `github`, and reads its answer as a read's. It reads the Repository's settings
/// from `settings` and its mark from `reader` as it sends, so a choice or a mark made since a control showed
/// Send counts. A Read-only Repository refuses the write before anything goes, as GitHub refuses one, so the
/// text stays in the composer with the reason. So does an item whose ID the store doesn't hold yet.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the first write, #393's comment, goes through this")
)]
pub(crate) fn send<D: DeserializeOwned>(
    github: &GitHub,
    settings: &LatestReader<SettingsView>,
    reader: &StoreReader,
    item_write: ItemWrite<'_>,
) -> Result<D, Error> {
    let ItemWrite {
        repository,
        number,
        write,
        selection,
    } = item_write;
    let in_force = settings.read(|view| view.repository(repository.clone()));
    let reasons = read_only_reasons(&in_force, reader)?;
    let subject = reader.node_id(&repository, number)?;
    #[expect(
        clippy::disallowed_methods,
        reason = "the one caller clippy.toml allows, with the reasons read from the settings in force and the \
                  store just now"
    )]
    let writable = WritableRepository::new(repository.clone(), subject, reasons);
    let writable = writable.map_err(|why| refused(why, &repository, number))?;
    github.write(writable, write, selection)
}

/// The error of a write to item `number` of `repository` that Tsugiki didn't send, for `why`.
fn refused(why: Unwritable, repository: &RepositoryName, number: u32) -> Error {
    let (kind, why) = match why {
        Unwritable::ReadOnly(ReadOnly::Chosen) => (
            ErrorKind::WriteRefused,
            format!("you chose Read-only in {repository}'s settings"),
        ),
        Unwritable::ReadOnly(ReadOnly::Token) => (
            ErrorKind::WriteRefused,
            format!("your token can't write to {repository}"),
        ),
        // Tsugiki's copy is behind what the write needs, and GitHub may well have the item.
        Unwritable::Unread => (
            ErrorKind::Conflict,
            format!(
                "it hasn't read #{number} of {repository} from GitHub yet. Open it again, and send once it \
                 has"
            ),
        ),
    };
    Error::new(kind, &format!("Tsugiki sent nothing, as {why}."))
}

#[cfg(test)]
mod tests;
