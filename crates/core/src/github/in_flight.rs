//! What the GitHub thread waits on, a request to GitHub or `gh` with the token, which the status bar shows
//! once it is slow to answer (#998, #1004).

use std::cell::RefCell;

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use crate::repository::RepositoryName;

/// What a request asks GitHub, as the status bar names it, or the token Tsugiki asks `gh` for. Whoever asks
/// says so through `GitHub::asking`, as the GraphQL operation's name means nothing to a user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Asking {
    /// The token, which Tsugiki asks `gh auth token` for when `GH_TOKEN` is unset. No request to GitHub goes
    /// out until it answers.
    Token,
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
    /// Whether a Repository is private, which Tsugiki asks when the token writes to public repositories
    /// alone.
    Visibility { repository: RepositoryName },
}

/// What the GitHub thread waits on: what it asks, and when it asked, by this computer's clock, from which
/// the window counts the seconds itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct InFlight {
    pub asked: Asking,
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub since: OffsetDateTime,
}

/// Hears each wait as it starts, and nothing once it ends, answered or failed. It must not panic: the end
/// can come while a panic in the wait unwinds, and a second panic then aborts the app.
pub type AnnounceInFlight = Box<dyn FnMut(Option<&InFlight>) + Send>;

/// Runs `wait`, which asks what `asked` says, and gives what it gave. `announce` hears of it as it starts,
/// and nothing once it ends, a panic included.
pub(crate) fn announced<T>(
    announce: &RefCell<AnnounceInFlight>,
    asked: Asking,
    wait: impl FnOnce() -> T,
) -> T {
    let in_flight = InFlight {
        asked,
        since: OffsetDateTime::now_utc(),
    };
    (announce.borrow_mut())(Some(&in_flight));
    let _ended = Ended(announce);
    wait()
}

/// Announces that the wait ended, as it ends, a panic included, so the window doesn't count the seconds of
/// a wait the GitHub thread no longer waits on.
struct Ended<'a>(&'a RefCell<AnnounceInFlight>);

impl Drop for Ended<'_> {
    fn drop(&mut self) {
        // A panic while unwinding would abort, so a borrow already out goes unannounced.
        if let Ok(mut announce) = self.0.try_borrow_mut() {
            announce(None);
        }
    }
}
