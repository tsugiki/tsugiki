//! The request to GitHub in flight, which the status bar shows once GitHub is slow to answer it (#998).

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use crate::repository::RepositoryName;

/// What a request asks GitHub, as the status bar names it. Whoever asks says so through `GitHub::asking`,
/// as the GraphQL operation's name means nothing to a user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Asking {
    /// Who is signed in, which Tsugiki asks about once a minute.
    Status,
    /// A step of a Repository's refresh.
    Refresh { repository: RepositoryName },
    /// A step of a Repository's first sync, of the tree of an Issue the user opened, or of the fetch of
    /// Issues with no state.
    Sync { repository: RepositoryName },
    /// What an Issue or a PR says, read for the screen.
    Read {
        repository: RepositoryName,
        number: u32,
    },
    /// An Issue or a PR the database lacks, which quick open fetches by its number.
    Fetch {
        repository: RepositoryName,
        number: u32,
    },
    /// Add repository's check of a Repository.
    Check { repository: RepositoryName },
    /// The size of a Repository, which the Mirror worker asks before a clone.
    Size { repository: RepositoryName },
}

/// The request to GitHub in flight: what it asks, and when it went, by this computer's clock, from which
/// the window counts the seconds itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct InFlight {
    pub asked: Asking,
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub since: OffsetDateTime,
}

/// Hears each request to GitHub as it goes, and nothing once it ends, answered or failed.
pub type AnnounceInFlight = Box<dyn FnMut(Option<&InFlight>) + Send>;
