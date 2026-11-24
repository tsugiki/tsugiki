//! The window's ask about an action it made on a list, and the core's answer once it has kept the action
//! (#1052): the list read again by the code the window asks with.

use serde::{Deserialize, Serialize};
use specta::Type;

use super::inbox::{Inbox, InboxPage};
use super::{Workbench, WorkbenchView};
use crate::error::Error;
use crate::repository::RepositoryName;
use crate::repository_settings::RepositorySettings;
use crate::settings::ListOrder;
use crate::store::StoreReader;

/// The list the window showed when the user acted, as the window asked for it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Listing {
    /// The Workbench, or one of its views.
    Workbench { view: WorkbenchView },
    /// A page of the Inbox.
    Inbox { page: InboxPage },
}

/// The window's ask about an action it made (#1052): the number it gives the action, and the list it showed,
/// so the core can say what the action did to that list.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Type)]
pub struct Asked {
    /// The number the window gives the action, which the answer names.
    pub id: u32,
    /// The list the window showed when the user acted.
    pub listing: Listing,
}

/// The list an action left, read as the window asked for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Page {
    /// The Workbench view, or one of them, as it now stands.
    Workbench { workbench: Workbench },
    /// A page of the Inbox, as it now stands.
    Inbox { inbox: Inbox },
}

impl Page {
    /// Reads the list `listing` names in `repository`, its Workbench items in `order`.
    pub(crate) fn read(
        reader: &StoreReader,
        repository: &RepositorySettings,
        listing: &Listing,
        order: ListOrder,
    ) -> Result<Self, Error> {
        Ok(match listing {
            Listing::Workbench { view } => Self::Workbench {
                workbench: Workbench::read(reader, repository, view, order)?,
            },
            Listing::Inbox { page } => Self::Inbox {
                inbox: Inbox::read_now(reader, repository, *page)?,
            },
        })
    }
}

/// The core's answer to an action it kept (#1052): the number the window gave the action, the Repository it
/// acted in, and the list it showed as it now stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Answered {
    /// The number the window gave the action.
    pub id: u32,
    /// The Repository the action was made in.
    pub repository: RepositoryName,
    /// The list the window showed, as the store holds it now.
    pub page: Page,
}
