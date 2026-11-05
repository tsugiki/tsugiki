//! App-wide status, owned by the core (ADR 0008). For now: the connection, who is signed in, and problems.

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use crate::error::Error;
use crate::github::Login;
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
    Window,
    Trees,
    /// A refresh, or a sync that hasn't asked the store yet which it is. The status bar only mentions this
    /// when it has to wait.
    Refresh,
}

/// The work under way in the queue of syncs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct SyncActivity {
    pub repository: RepositoryName,
    pub part: SyncPart,
    /// The pages a first sync has read.
    pub pages: u32,
    /// The time GitHub's points come back, while the sync waits for them.
    #[serde(with = "time::serde::rfc3339::option")]
    #[specta(type = Option<String>)]
    pub paused_until: Option<OffsetDateTime>,
}

type AnnounceActivity = Box<dyn FnMut(Option<&SyncActivity>) + Send>;

/// Holds the status and announces each change, so the app can pass it on as an event. It also announces when
/// a Repository's stored facts change, so an open screen can ask again, and when a sync of the open
/// Repository finishes (ADR 0008). The session's thread owns it, so nothing here needs a lock.
pub struct StatusBoard {
    current: Status,
    announce: Box<dyn FnMut(&Status) + Send>,
    announce_change: Box<dyn FnMut(&Change) + Send>,
    announce_last_sync: Box<dyn FnMut(&LastSync) + Send>,
    sync_activity: Option<SyncActivity>,
    announce_sync_activity: AnnounceActivity,
}

impl StatusBoard {
    pub fn new(announce: Box<dyn FnMut(&Status) + Send>) -> Self {
        Self {
            current: Status::default(),
            announce,
            announce_change: Box::new(|_| {}),
            announce_last_sync: Box::new(|_| {}),
            sync_activity: None,
            announce_sync_activity: Box::new(|_| {}),
        }
    }

    /// Also tells `announce` when a Repository's stored facts change.
    #[must_use]
    pub fn with_changes(mut self, announce: Box<dyn FnMut(&Change) + Send>) -> Self {
        self.announce_change = announce;
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

    pub(crate) fn sync_activity(&mut self, activity: Option<SyncActivity>) {
        if self.sync_activity != activity {
            self.sync_activity = activity;
            (self.announce_sync_activity)(self.sync_activity.as_ref());
        }
    }

    pub(crate) fn changed(&mut self, change: &Change) {
        (self.announce_change)(change);
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
pub(crate) mod testing {
    use super::*;
    use std::sync::mpsc::{self, Receiver};

    /// A board that passes on changes to stored facts and drops status changes.
    pub(crate) fn changes_board() -> (StatusBoard, Receiver<Change>) {
        let (passed, seen) = mpsc::channel();
        let board =
            StatusBoard::new(Box::new(|_| {})).with_changes(Box::new(move |change: &Change| {
                passed.send(change.clone()).unwrap();
            }));
        (board, seen)
    }

    pub(crate) fn quiet_board() -> StatusBoard {
        StatusBoard::new(Box::new(|_| {}))
    }

    /// A board, and where it sends what it announces, so a test can also wait for a status from another
    /// thread.
    pub(crate) fn watched_board() -> (StatusBoard, Receiver<Status>) {
        let (passed, seen) = mpsc::channel();
        let board = StatusBoard::new(Box::new(move |status: &Status| {
            passed.send(status.clone()).unwrap();
        }));
        (board, seen)
    }

    /// What the board has announced since the last time a test looked.
    pub(crate) fn announced(seen: &Receiver<Status>) -> Vec<Status> {
        seen.try_iter().collect()
    }

    /// The last thing the board announced, which is what the app is showing.
    pub(crate) fn showing(seen: &Receiver<Status>) -> Status {
        seen.try_iter().last().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{announced, watched_board};
    use super::*;
    use crate::error::ErrorKind;

    #[test]
    fn a_change_or_a_last_sync_with_nobody_listening_isnt_a_new_status() {
        let (mut board, seen) = watched_board();
        let repository = RepositoryName::parse("python/cpython").unwrap();

        board.changed(&Change {
            repository: repository.clone(),
            items: Vec::new(),
        });
        board.last_sync(&LastSync {
            repository,
            at: None,
        });

        assert!(announced(&seen).is_empty());
    }

    #[test]
    fn a_last_sync_reaches_the_ui_as_a_time_or_as_nothing() {
        let repository = RepositoryName::parse("python/cpython").unwrap();
        let at = OffsetDateTime::from_unix_timestamp(1_000_000_000).unwrap();
        let never = LastSync {
            repository: repository.clone(),
            at: None,
        };
        let once = LastSync {
            repository,
            at: Some(at),
        };

        assert_eq!(
            serde_json::to_value(never).unwrap()["at"],
            serde_json::Value::Null
        );
        assert_eq!(
            serde_json::to_value(once).unwrap()["at"],
            "2001-09-09T01:46:40Z"
        );
    }

    #[test]
    fn nothing_is_known_before_tsugiki_connects() {
        let (mut board, seen) = watched_board();
        board.set(Status::Connecting);
        assert!(announced(&seen).is_empty(), "it was connecting already");
    }

    #[test]
    fn every_change_is_announced() {
        let (mut board, seen) = watched_board();
        board.set(Status::Offline);
        board.set(Status::Connecting);
        assert_eq!(announced(&seen), [Status::Offline, Status::Connecting]);
    }

    #[test]
    fn a_status_that_didnt_change_isnt_announced() {
        let (mut board, seen) = watched_board();
        board.set(Status::Offline);
        board.set(Status::Offline);
        assert_eq!(announced(&seen).len(), 1);
    }

    #[test]
    fn the_status_reaches_the_ui_tagged_by_its_state() {
        let hubot = || Login::new("hubot");
        let problem = Error::new(ErrorKind::Unauthorized, "Sign in.");
        let cases = [
            (Status::Offline, serde_json::json!({ "state": "offline" })),
            (
                Status::Connecting,
                serde_json::json!({ "state": "connecting" }),
            ),
            (
                Status::Online {
                    login: Login::new("octocat"),
                },
                serde_json::json!({ "state": "online", "login": "octocat" }),
            ),
            (
                Status::ConnectingAs {
                    last_login: hubot(),
                },
                serde_json::json!({ "state": "connecting_as", "last_login": "hubot" }),
            ),
            (
                Status::OfflineAs {
                    last_login: hubot(),
                },
                serde_json::json!({ "state": "offline_as", "last_login": "hubot" }),
            ),
            (
                Status::Problem { problem },
                serde_json::json!({
                    "state": "problem",
                    "problem": { "kind": "unauthorized", "message": "Sign in." },
                }),
            ),
        ];
        for (status, expected) in cases {
            assert_eq!(serde_json::to_value(status).unwrap(), expected);
        }
    }
}
