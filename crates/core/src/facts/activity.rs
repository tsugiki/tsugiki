//! The events on an Issue or a PR, which `github` parses from GitHub's timeline, and the store's bundle of
//! them that New activity and Needs attention read.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use specta::Type;
use time::OffsetDateTime;

use crate::login::Login;

/// The kinds of thing a person, or a backport bot's failure, adds to a timeline.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActivityKind {
    /// A pushed commit or a rewritten history.
    Commit,
    Comment,
    Review,
    /// A Draft PR became ready for review.
    ReadyForReview,
    /// The backport bot said it couldn't backport this PR to `target`.
    BackportFailed {
        target: String,
    },
    /// Someone asked `who` to review this PR. It says when a Review request began, and it's no news itself.
    ReviewRequested,
    /// Someone assigned this PR to `who`. It says when a Review request began, and it's no news itself.
    Assigned,
    /// A post mentioned `who`, the person GitHub names on this event. `who` didn't write the post.
    Mentioned,
}

/// One thing that happened on an Issue or a PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    pub kind: ActivityKind,
    pub who: Login,
    pub at: OffsetDateTime,
}

/// The activity GitHub reported on one Issue or PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemActivity {
    pub number: u32,
    pub events: Vec<Activity>,
}

/// Names a Workbench item in the user's own state that hides it: Archived, and Muted (#492). Issues and PRs
/// share their numbers in a Repository, and a Standalone PR and a Standalone Backport are both PRs.
/// Dependency updates join it as a kind of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemKey {
    Issue { number: u32 },
    PullRequest { number: u32 },
}

impl ItemKey {
    /// The number of the Issue or PR that heads the item.
    pub fn number(self) -> u32 {
        match self {
            Self::Issue { number } | Self::PullRequest { number } => number,
        }
    }
}

/// One item the user archived (#490).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Archive {
    /// The moment Archive marked the item seen up to. What the user acknowledges after it brings the item
    /// back.
    pub seen_up_to: OffsetDateTime,
    /// When the user put the item away, which orders the Archived view (#491).
    pub archived_at: OffsetDateTime,
}

/// When Archive puts its item away, which orders the Archived view (#491).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutAway {
    /// Now, as E, the palette's Archive and the Issue view's button do: the item goes to the top of the
    /// Archived view.
    Now,
    /// When it was last put away, as Mark Seen does on an item the window draws Archived: the item keeps its
    /// place in the Archived view.
    AsBefore,
}

/// The moment Archive or Mute hides an item from, once Mark Seen on its tree acknowledged it up to `up_to`:
/// that moment, or `now` when the window drew no sync, or a later moment in `acknowledged`, when the user
/// acknowledged one of the tree's Issues and PRs. One acknowledged later, as by Mark Seen on another Issue
/// its PR fixes, would otherwise read as acknowledged after the archive, and so as come back.
pub(crate) fn hidden_from(
    up_to: Option<OffsetDateTime>,
    now: OffsetDateTime,
    acknowledged: impl IntoIterator<Item = OffsetDateTime>,
) -> OffsetDateTime {
    let start = up_to.unwrap_or(now);
    acknowledged.into_iter().fold(start, OffsetDateTime::max)
}

/// A person or bot whose items the user muted in a Repository (#494).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutedAuthor {
    pub login: Login,
    /// The moment of the mute. A Review request or an assignment that starts after it shows their item.
    pub at: OffsetDateTime,
}

/// Whether a Repository's Inbox lists the rows Muted authors wrote: its "Show muted" switch (#825), which
/// starts off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ShowMuted {
    #[default]
    Off,
    On,
}

/// The facts New activity and Needs attention read about a Repository's items, all in one go, so ranking a
/// list costs no query for each item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivityFacts {
    /// The moment the Repository's first sync began, once a first sync has finished.
    pub first_sync: Option<OffsetDateTime>,
    pub last_seen: BTreeMap<u32, OffsetDateTime>,
    /// The moment the user last acknowledged each item: a Review request, a Stale review or failed CI that
    /// started before it no longer Needs attention. Mark Seen moves it with Last seen, and a write to GitHub
    /// (#395) is to leave it.
    pub acknowledged: BTreeMap<u32, OffsetDateTime>,
    /// The moment the user marked each item unseen, for the items still flagged.
    pub marked_unseen: BTreeMap<u32, OffsetDateTime>,
    /// The items the user archived. Whether each is still Archived or has come back, `workbench` judges
    /// (#490).
    pub archived: BTreeMap<ItemKey, Archive>,
    /// The items the user muted, each with the moment Mute marked it seen up to (#492).
    pub muted: BTreeMap<ItemKey, OffsetDateTime>,
    /// The Muted authors of the Repository, by login (#494). Their items count as Muted, worked out from
    /// each item's author, so the store keeps no Mute on each of them.
    pub muted_authors: Vec<MutedAuthor>,
    /// The user's Pinned issues (#488).
    pub pinned: BTreeSet<u32>,
    /// The Pinned issues that closed while pinned, whose closing waits for the unpin, which archives them.
    pub held_closings: BTreeSet<u32>,
    /// Each event that names the user, and what happened after the first sync began on the items they wrote,
    /// reviewed, are assigned to or commented on, each at the time New activity goes by, oldest first. Only
    /// those can make an item Need attention, and a Repository holds too many others to read with each list.
    pub events: BTreeMap<u32, Vec<Activity>>,
    /// The moment Tsugiki saw each PR's CI reach a result.
    pub ci_finished: BTreeMap<u32, OffsetDateTime>,
    /// The names of the failed checks of each PR Tsugiki has asked about.
    pub failed_checks: BTreeMap<u32, Vec<String>>,
}

impl ActivityFacts {
    /// Whether the item `key` names is a Pinned issue (#488). Only an Issue takes a pin.
    pub fn is_pinned(&self, key: ItemKey) -> bool {
        match key {
            ItemKey::Issue { number } => self.pinned.contains(&number),
            ItemKey::PullRequest { .. } => false,
        }
    }
}
