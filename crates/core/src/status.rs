//! App-wide status, owned by the core (ADR 0008). For now: the connection, who is signed in, and problems.

use serde::Serialize;
use specta::Type;

use crate::error::Error;
use crate::github::Login;

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

/// Holds the status and announces each change, so the app can pass it on as an event. The session's thread
/// owns it, so nothing here needs a lock.
pub struct StatusBoard {
    current: Status,
    announce: Box<dyn FnMut(&Status) + Send>,
}

impl StatusBoard {
    pub fn new(announce: Box<dyn FnMut(&Status) + Send>) -> Self {
        Self {
            current: Status::default(),
            announce,
        }
    }

    pub(crate) fn set(&mut self, status: Status) {
        if self.current != status {
            self.current = status;
            (self.announce)(&self.current);
        }
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use std::sync::mpsc::{self, Receiver};

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
