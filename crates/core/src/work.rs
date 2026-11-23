//! The user's work, judged from its facts. By Issue or PR number it says each one's place on the Workbench
//! in its own right, what makes it Need attention, and where the user's Review stands. By Repository it works
//! out Reach, and from it the Mirror's work, the Conversations worth reading ahead and the checks the store
//! keeps.
//!
//! It's the functional core: it takes `facts` and returns values. It opens no connection, runs no git and
//! sends no message. `workbench` groups what it judges into items and lays them out, and `sync` and
//! `session` act on what it decides.

mod causes;
mod reach;
mod reasons;
mod review_state;

use std::collections::BTreeSet;

use crate::links::BackportLink;

pub(crate) use causes::{Cause, Causes};
pub use causes::{SeenMark, Why};
pub use reach::MirrorWork;
pub(crate) use reach::{
    ReachFrom, archived_by_closing, conversations_to_read, held_in_own_right, matched, moved,
    opened_on,
};
pub use reasons::WorkReason;
pub(crate) use reasons::reasons;
pub(crate) use review_state::SharedReviews;
pub use review_state::{YourReview, checks_to_keep, findings_to_keep};

/// The PRs a Backport can sit under, of those each with its Backport link: the ones that aren't Backports
/// too, which keeps a tree two levels deep. A Bot backport shares the review state of the one it sits under.
pub(crate) fn origins<'a, K: 'a>(
    pull_requests: impl Iterator<Item = (u32, Option<&'a BackportLink<K>>)>,
) -> BTreeSet<u32> {
    let origins = pull_requests.filter(|(_, backport)| backport.is_none());
    origins.map(|(number, _)| number).collect()
}
