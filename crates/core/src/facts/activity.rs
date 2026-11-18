//! The events on an Issue or a PR, which `github` parses from GitHub's timeline, and the store's bundle of
//! them that New activity and Needs attention read.

use std::collections::BTreeMap;

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

/// The facts New activity and Needs attention read about a Repository's items, all in one go, so ranking a
/// list costs no query for each item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivityFacts {
    /// The moment the Repository's first sync began, once a first sync has finished.
    pub first_sync: Option<OffsetDateTime>,
    pub last_seen: BTreeMap<u32, OffsetDateTime>,
    /// The moment the user last acknowledged each item: a Review request, a Stale review or failed CI that
    /// started before it no longer Needs attention. Mark Seen moves it with Last seen, and a write to GitHub
    /// doesn't.
    pub acknowledged: BTreeMap<u32, OffsetDateTime>,
    /// The moment the user marked each item unseen, for the items still flagged.
    pub marked_unseen: BTreeMap<u32, OffsetDateTime>,
    /// Each event that names the user, and what happened after the first sync began on the items they wrote,
    /// reviewed, are assigned to or commented on, each at the time New activity goes by, oldest first. Only
    /// those can make an item Need attention, and a Repository holds too many others to read with each list.
    pub events: BTreeMap<u32, Vec<Activity>>,
    /// The moment Tsugiki saw each PR's CI reach a result.
    pub ci_finished: BTreeMap<u32, OffsetDateTime>,
    /// The names of the failed checks of each PR Tsugiki has asked about.
    pub failed_checks: BTreeMap<u32, Vec<String>>,
}
