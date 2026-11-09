//! App-wide status, owned by the core (ADR 0008). For now: the connection, who is signed in, and problems.

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use crate::error::Error;
use crate::github::pull_requests::PrHead;
use crate::login::Login;
use crate::repository::RepositoryName;

/// The app's state, shown in the status bar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Status {
    /// Nobody is known yet, and GitHub hasn't answered.
    #[default]
    Connecting,
    /// GitHub hasn't answered yet, and this is who was signed in last time.
    ConnectingAs { last_login: Login },
    /// GitHub confirmed this login just now.
    Online { login: Login },
    /// Nobody is known, and GitHub can't be reached.
    Offline,
    /// GitHub can't be reached, and this is who was signed in last time.
    OfflineAs { last_login: Login },
    /// Something stops Tsugiki reading, such as a refused token.
    Problem { problem: Error },
}

/// What a sync changed in a Repository's stored facts (ADR 0008).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Change {
    pub repository: RepositoryName,
    /// The Issues and PRs it touched. A change to something else about the Repository names none.
    pub items: Vec<u32>,
}

/// The reason a read of what an Issue or a PR says didn't bring it from GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ReadFailure {
    /// Tsugiki has no token, so it asks GitHub nothing until it starts again, and the status bar says how to
    /// sign in.
    NoToken,
    /// GitHub can't be asked anything now, and the status bar says why. A read after GitHub answers again
    /// brings it.
    #[serde(rename = "waits_on_github")]
    WaitsOnGitHub,
    /// GitHub has no such item, or the token can't see it.
    #[serde(rename = "not_on_github")]
    NotOnGitHub,
    /// Anything else that went wrong with this one read, such as an answer Tsugiki couldn't parse. The log
    /// says what it was.
    Other,
}

/// A read of an Issue or a PR on screen that failed, so the window can say why it shows nothing new.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct FailedRead {
    pub repository: RepositoryName,
    pub number: u32,
    pub why: ReadFailure,
}

/// When the open Repository last finished a sync.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct LastSync {
    pub repository: RepositoryName,
    /// Nothing, until its first sync finishes.
    #[serde(with = "time::serde::rfc3339::option")]
    #[specta(type = Option<String>)]
    pub at: Option<OffsetDateTime>,
}

/// The part of a sync under way, in the order a first sync runs them (ADR 0015).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SyncPart {
    YourWork,
    /// The trees of the user's own Issues, and of any Issue they open.
    Trees,
    Window,
    /// A small Repository's open items that last changed before the window.
    OlderOpen,
    /// A refresh, or a sync that hasn't asked the store yet which it is. The status bar only mentions this
    /// when it has to wait.
    Refresh,
}

/// A first sync's progress through its queue. It learns of more PRs as it reads, so `prs_known` grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct LoadProgress {
    pub issues_read: u32,
    /// Nothing, until the first page of Issues says.
    pub issues_total: Option<u32>,
    pub prs_fetched: u32,
    pub prs_known: u32,
}

/// The work under way in the queue of syncs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct SyncActivity {
    /// The open Repository's sync, or the tree of an Issue the user opened in it, while there is one.
    pub open: Option<OpenSync>,
    /// The time GitHub's points come back, while the syncs wait for them.
    #[serde(with = "time::serde::rfc3339::option")]
    #[specta(type = Option<String>)]
    pub paused_until: Option<OffsetDateTime>,
    /// The other Repositories with a sync queued, by name. They take turns with the open one (ADR 0015).
    pub elsewhere: Vec<RepositoryName>,
}

/// How far the open Repository's sync has got.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct OpenSync {
    pub repository: RepositoryName,
    pub part: SyncPart,
    /// The pages a first sync has read.
    pub pages: u32,
    /// Its progress through the queue, once it gets there.
    pub load: Option<LoadProgress>,
}

type AnnounceActivity = Box<dyn FnMut(Option<&SyncActivity>) + Send>;

type AnnounceHeads = Box<dyn FnMut(&RepositoryName, Vec<PrHead>) + Send>;

/// Holds the status and announces each change, so the app can pass it on as an event. It also announces when
/// a Repository's stored facts change, so an open screen can ask again, and when a sync of the open
/// Repository finishes (ADR 0008). The session's thread owns it, so nothing here needs a lock.
pub struct StatusBoard {
    current: Status,
    announce: Box<dyn FnMut(&Status) + Send>,
    announce_change: Box<dyn FnMut(&Change) + Send>,
    announce_failed_read: Box<dyn FnMut(&FailedRead) + Send>,
    announce_last_sync: Box<dyn FnMut(&LastSync) + Send>,
    sync_activity: Option<SyncActivity>,
    announce_sync_activity: AnnounceActivity,
    announce_heads: AnnounceHeads,
}

impl StatusBoard {
    pub fn new(announce: Box<dyn FnMut(&Status) + Send>) -> Self {
        Self {
            current: Status::default(),
            announce,
            announce_change: Box::new(|_| {}),
            announce_failed_read: Box::new(|_| {}),
            announce_last_sync: Box::new(|_| {}),
            sync_activity: None,
            announce_sync_activity: Box::new(|_| {}),
            announce_heads: Box::new(|_, _| {}),
        }
    }

    /// Also tells `announce` when a Repository's stored facts change.
    #[must_use]
    pub fn with_changes(mut self, announce: Box<dyn FnMut(&Change) + Send>) -> Self {
        self.announce_change = announce;
        self
    }

    /// Also tells `announce` when a read of an Issue or a PR on screen fails.
    #[must_use]
    pub fn with_failed_reads(mut self, announce: Box<dyn FnMut(&FailedRead) + Send>) -> Self {
        self.announce_failed_read = announce;
        self
    }

    /// Also tells `announce` what the open Repository's last sync was.
    #[must_use]
    pub fn with_last_syncs(mut self, announce: Box<dyn FnMut(&LastSync) + Send>) -> Self {
        self.announce_last_sync = announce;
        self
    }

    /// Also tells `announce` what the queue of syncs is doing, each time that changes.
    #[must_use]
    pub fn with_sync_activity(mut self, announce: AnnounceActivity) -> Self {
        self.announce_sync_activity = announce;
        self
    }

    /// Also hands `announce` the heads of the PRs the user's work reaches in a Repository, each time a sync
    /// saves its facts, for its mirror to fetch.
    #[must_use]
    pub fn with_heads(mut self, announce: AnnounceHeads) -> Self {
        self.announce_heads = announce;
        self
    }

    pub(crate) fn reached_heads(&mut self, repository: &RepositoryName, heads: Vec<PrHead>) {
        (self.announce_heads)(repository, heads);
    }

    pub(crate) fn sync_activity(&mut self, activity: Option<SyncActivity>) {
        if self.sync_activity != activity {
            self.sync_activity = activity;
            (self.announce_sync_activity)(self.sync_activity.as_ref());
        }
    }

    pub(crate) fn changed(&mut self, change: &Change) {
        (self.announce_change)(change);
    }

    pub(crate) fn failed_read(&mut self, failed: &FailedRead) {
        (self.announce_failed_read)(failed);
    }

    pub(crate) fn last_sync(&mut self, last_sync: &LastSync) {
        (self.announce_last_sync)(last_sync);
    }

    pub(crate) fn set(&mut self, status: Status) {
        if self.current != status {
            if let Status::Problem { problem } = &status {
                log::error!("The status bar says: {}", problem.message);
            }
            self.current = status;
            (self.announce)(&self.current);
        }
    }
}

#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
mod tests;
