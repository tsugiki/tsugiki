//! App-wide status, owned by the core (ADR 0008). For now: the connection, who is signed in, problems, the
//! syncs' work, the hour's points and the request to GitHub in flight.

use std::sync::Arc;

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use crate::error::Error;
use crate::github::in_flight::AnnounceInFlight;
use crate::github::points::HourPoints;
use crate::latest::{self, LatestReader, LatestWriter};
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
    /// Nobody is known, and GitHub, which couldn't be reached, is being asked again. A status bar the
    /// settings hide still shows it, as it showed being offline.
    Reconnecting,
    /// GitHub, which couldn't be reached, is being asked again, and this is who was signed in last time.
    ReconnectingAs { last_login: Login },
    /// Something stops Tsugiki reading, such as a refused token.
    Problem { problem: Error },
}

impl Status {
    /// Whether it says GitHub can't be reached, or hasn't answered since it couldn't be, so git stays held
    /// back while Tsugiki asks again. A rate limit is a problem of the API's alone.
    pub(crate) fn is_offline(&self) -> bool {
        match self {
            Self::Offline
            | Self::OfflineAs { .. }
            | Self::Reconnecting
            | Self::ReconnectingAs { .. } => true,
            Self::Connecting
            | Self::ConnectingAs { .. }
            | Self::Online { .. }
            | Self::Problem { .. } => false,
        }
    }

    /// What it says while GitHub is asked again after it couldn't be reached, keeping who was signed in last
    /// time. Any other status stays until the answer.
    fn asking_again(&self) -> Option<Self> {
        match self {
            Self::Offline => Some(Self::Reconnecting),
            Self::OfflineAs { last_login } => Some(Self::ReconnectingAs {
                last_login: last_login.clone(),
            }),
            Self::Connecting
            | Self::ConnectingAs { .. }
            | Self::Reconnecting
            | Self::ReconnectingAs { .. }
            | Self::Online { .. }
            | Self::Problem { .. } => None,
        }
    }
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
    /// Tsugiki has no token, so it asks GitHub nothing until a read of the token finds one, and the status
    /// bar says why.
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

/// The reason a read of what an Issue or a PR says failed for this item, which the session's next read of
/// it may mend: a `ReadFailure` less `WaitsOnGitHub`, which speaks for all of GitHub. A PR's Files tab that
/// waits for the head the Conversation names shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ItemUnread {
    /// Tsugiki has no token, so no read brings the item until a read of the token finds one.
    NoToken,
    /// GitHub has no such item, or the token can't see it.
    #[serde(rename = "not_on_github")]
    NotOnGitHub,
    /// Anything else that went wrong with this one read. The log says what it was.
    Other,
}

impl From<ItemUnread> for ReadFailure {
    fn from(why: ItemUnread) -> Self {
        match why {
            ItemUnread::NoToken => Self::NoToken,
            ItemUnread::NotOnGitHub => Self::NotOnGitHub,
            ItemUnread::Other => Self::Other,
        }
    }
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

type AnnouncePoints = Box<dyn FnMut(Option<&HourPoints>) + Send>;

/// The time GitHub's points come back, by this computer's clock, while the rate limit's gate holds every
/// request to GitHub (ADR 0015). Quick open offers the browser in place of a fetch until then.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct PointsBack {
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub at: OffsetDateTime,
}

type AnnouncePointsBack = Box<dyn FnMut(Option<&PointsBack>) + Send>;

/// Tells the window that a Repository's stored facts changed, from whichever thread saved them: the GitHub
/// thread for what GitHub answered, the session for what the user marked (ADR 0008).
#[derive(Clone)]
pub(crate) struct Changes(Arc<dyn Fn(&Change) + Send + Sync>);

impl Changes {
    pub(crate) fn changed(&self, change: &Change) {
        (self.0)(change);
    }
}

/// What the status bar shows of whether GitHub can be asked now, as a copy the board keeps for the session,
/// which answers at once what it can't ask.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Shown {
    pub(crate) status: Status,
    pub(crate) points_back: Option<PointsBack>,
}

impl Shown {
    /// Whether a request now would only wait for its failure: GitHub can't be reached, or the rate limit's
    /// gate holds every request until the reset.
    pub(crate) fn refuses_requests(&self) -> bool {
        self.status.is_offline() || self.points_back.is_some()
    }
}

/// Holds the status and announces each change, so the app can pass it on as an event. It also announces when
/// a Repository's stored facts change, so an open screen can ask again, when a sync of the open
/// Repository finishes, and when any Repository's last sync or count of open items changes (ADR 0008). The
/// GitHub thread owns it, so nothing here needs a lock, and the session tells the window of its own changes
/// through a clone of `changes`.
pub struct StatusBoard {
    current: Status,
    announce: Box<dyn FnMut(&Status) + Send>,
    announce_change: Changes,
    announce_failed_read: Box<dyn FnMut(&FailedRead) + Send>,
    announce_last_sync: Box<dyn FnMut(&LastSync) + Send>,
    announce_repository_sync: Box<dyn FnMut(&RepositoryName) + Send>,
    sync_activity: Option<SyncActivity>,
    announce_sync_activity: AnnounceActivity,
    hour_points: Option<HourPoints>,
    announce_hour_points: AnnouncePoints,
    points_back: Option<PointsBack>,
    announce_points_back: AnnouncePointsBack,
    /// The copy of the status and the gate's hold, once someone asked for it.
    shown: Option<LatestWriter<Shown>>,
    /// Hears of the request to GitHub in flight, until the GitHub client takes it.
    announce_in_flight: Option<AnnounceInFlight>,
}

impl StatusBoard {
    pub fn new(announce: Box<dyn FnMut(&Status) + Send>) -> Self {
        Self {
            current: Status::default(),
            announce,
            announce_change: Changes(Arc::new(|_| {})),
            announce_failed_read: Box::new(|_| {}),
            announce_last_sync: Box::new(|_| {}),
            announce_repository_sync: Box::new(|_| {}),
            sync_activity: None,
            announce_sync_activity: Box::new(|_| {}),
            hour_points: None,
            announce_hour_points: Box::new(|_| {}),
            points_back: None,
            announce_points_back: Box::new(|_| {}),
            shown: None,
            announce_in_flight: None,
        }
    }

    /// Also tells `announce` of each request to GitHub as it goes, and nothing once it ends.
    #[must_use]
    pub fn with_in_flight(mut self, announce: AnnounceInFlight) -> Self {
        self.announce_in_flight = Some(announce);
        self
    }

    /// Hands the GitHub client what hears of the request in flight, as only the client knows when one
    /// starts and ends while the GitHub thread waits for it. Nothing, once it has.
    pub(crate) fn in_flight(&mut self) -> Option<AnnounceInFlight> {
        self.announce_in_flight.take()
    }

    /// Also tells `announce` when a Repository's stored facts change.
    #[must_use]
    pub fn with_changes(mut self, announce: Box<dyn Fn(&Change) + Send + Sync>) -> Self {
        self.announce_change = Changes(Arc::from(announce));
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

    /// Also tells `announce` of each Repository, open or not, whose last sync or count of open items
    /// changed, which its page in Settings shows (#534).
    #[must_use]
    pub fn with_repository_syncs(
        mut self,
        announce: Box<dyn FnMut(&RepositoryName) + Send>,
    ) -> Self {
        self.announce_repository_sync = announce;
        self
    }

    /// Also tells `announce` what the queue of syncs is doing, each time that changes.
    #[must_use]
    pub fn with_sync_activity(mut self, announce: AnnounceActivity) -> Self {
        self.announce_sync_activity = announce;
        self
    }

    /// Also tells `announce` the hour's points, each time an answer from GitHub changes them, and nothing
    /// once the hour resets.
    #[must_use]
    pub fn with_hour_points(mut self, announce: AnnouncePoints) -> Self {
        self.announce_hour_points = announce;
        self
    }

    /// Announces `points` if they changed, going back to nothing when the hour resets. Before GitHub's first
    /// answer that counts them, nothing is no change.
    pub(crate) fn hour_points(&mut self, points: Option<HourPoints>) {
        if self.hour_points != points {
            self.hour_points = points;
            (self.announce_hour_points)(self.hour_points.as_ref());
        }
    }

    /// Also tells `announce` the time GitHub's points come back, each time the rate limit's gate starts
    /// holding or moves that time, and nothing once it opens.
    #[must_use]
    pub fn with_points_back(mut self, announce: AnnouncePointsBack) -> Self {
        self.announce_points_back = announce;
        self
    }

    /// Announces the time GitHub's points come back, `at` by this computer's clock, if it changed, or
    /// nothing once the gate opens.
    pub(crate) fn points_back(&mut self, at: Option<OffsetDateTime>) {
        let back = at.map(|at| PointsBack { at });
        if self.points_back != back {
            self.points_back = back;
            // The copy first, so the session reads the gate no later than the window hears it.
            self.copy_shown();
            (self.announce_points_back)(self.points_back.as_ref());
        }
    }

    /// A copy of the status and the gate's hold, which the board keeps up to date from now on.
    pub(crate) fn shown(&mut self) -> LatestReader<Shown> {
        let (mut writer, reader) = latest::channel(Shown::default());
        writer.set(&self.now_shown());
        self.shown = Some(writer);
        reader
    }

    fn now_shown(&self) -> Shown {
        Shown {
            status: self.current.clone(),
            points_back: self.points_back,
        }
    }

    fn copy_shown(&mut self) {
        let now = self.now_shown();
        if let Some(shown) = &mut self.shown {
            shown.set(&now);
        }
    }

    /// The way to tell the window of a change from another thread.
    pub(crate) fn changes(&self) -> Changes {
        self.announce_change.clone()
    }

    pub(crate) fn sync_activity(&mut self, activity: Option<SyncActivity>) {
        if self.sync_activity != activity {
            self.sync_activity = activity;
            (self.announce_sync_activity)(self.sync_activity.as_ref());
        }
    }

    pub(crate) fn changed(&mut self, change: &Change) {
        self.announce_change.changed(change);
    }

    pub(crate) fn failed_read(&mut self, failed: &FailedRead) {
        (self.announce_failed_read)(failed);
    }

    pub(crate) fn last_sync(&mut self, last_sync: &LastSync) {
        (self.announce_last_sync)(last_sync);
    }

    pub(crate) fn repository_sync_changed(&mut self, repository: &RepositoryName) {
        (self.announce_repository_sync)(repository);
    }

    /// Whether the status says GitHub can't be reached, or hasn't answered since it couldn't be.
    pub(crate) fn is_offline(&self) -> bool {
        self.current.is_offline()
    }

    /// Says "Connecting…" while GitHub is asked again, if the status says it can't be reached, so a click on
    /// Refresh shows it was heard. Returns whether it did.
    #[must_use = "only a status that said offline is asked again"]
    pub(crate) fn asking_again(&mut self) -> bool {
        match self.current.asking_again() {
            Some(asking) => {
                self.set(asking);
                true
            }
            None => false,
        }
    }

    pub(crate) fn set(&mut self, status: Status) {
        if self.current != status {
            if let Status::Problem { problem } = &status {
                log::error!("The status bar says: {}", problem.message);
            }
            self.current = status;
            // The copy first, so the session reads the status no later than the window hears it.
            self.copy_shown();
            (self.announce)(&self.current);
        }
    }
}

#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
mod tests;
