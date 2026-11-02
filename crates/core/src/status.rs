//! App-wide status, owned by the core (ADR 0008). For now: the connection, who is signed in, and problems.

use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use specta::Type;

use crate::error::Error;
use crate::github::Login;

/// The app's state, shown in the status bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Status {
    /// Nobody is known yet, and GitHub hasn't answered.
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

/// Holds the status and announces each change, so the app can pass it on as an event.
pub struct StatusBoard {
    current: Mutex<Status>,
    announce: Box<dyn Fn(&Status) + Send + Sync>,
}

impl StatusBoard {
    pub fn new(announce: Box<dyn Fn(&Status) + Send + Sync>) -> Self {
        Self {
            current: Mutex::new(Status::Connecting),
            announce,
        }
    }

    pub fn get(&self) -> Status {
        self.lock().clone()
    }

    /// Only this file holds the lock, and it calls out to nothing while it does, so poisoning should never
    /// happen. If it does, the status is still whole, because `set` only ever replaces it.
    fn lock(&self) -> MutexGuard<'_, Status> {
        self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn set(&self, status: Status) {
        let mut current = self.lock();
        if *current != status {
            current.clone_from(&status);
            drop(current);
            (self.announce)(&status);
        }
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use std::sync::Arc;

    pub(crate) fn quiet_board() -> StatusBoard {
        StatusBoard::new(Box::new(|_| {}))
    }

    /// A board, and everything it has announced.
    pub(crate) fn watched_board() -> (StatusBoard, Arc<Mutex<Vec<Status>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_by_board = Arc::clone(&seen);
        let board = StatusBoard::new(Box::new(move |status: &Status| {
            seen_by_board.lock().unwrap().push(status.clone());
        }));
        (board, seen)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{quiet_board, watched_board};
    use super::*;
    use crate::error::ErrorKind;

    #[test]
    fn a_panic_while_holding_the_lock_doesnt_stop_the_status_working() {
        let board = quiet_board();
        std::thread::scope(|scope| {
            let holder = scope.spawn(|| {
                let _guard = board.current.lock();
                panic!("a panic while holding the lock");
            });
            assert!(holder.join().is_err());
        });
        assert!(board.current.is_poisoned());

        assert_eq!(board.get(), Status::Connecting);
        board.set(Status::Offline);
        assert_eq!(board.get(), Status::Offline);
    }

    #[test]
    fn nothing_is_known_before_tsugiki_connects() {
        assert_eq!(quiet_board().get(), Status::Connecting);
    }

    #[test]
    fn every_change_is_announced() {
        let (board, seen) = watched_board();
        board.set(Status::Offline);
        board.set(Status::Connecting);
        assert_eq!(*seen.lock().unwrap(), [Status::Offline, Status::Connecting]);
    }

    #[test]
    fn a_status_that_didnt_change_isnt_announced() {
        let (board, seen) = watched_board();
        board.set(Status::Offline);
        board.set(Status::Offline);
        assert_eq!(seen.lock().unwrap().len(), 1);
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
