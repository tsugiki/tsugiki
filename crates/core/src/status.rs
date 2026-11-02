//! App-wide status, owned by the core (ADR 0008). For now: the connection, who is signed in, and problems.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::Serialize;
use specta::Type;

use crate::error::{Error, ErrorKind};
use crate::github::{GitHub, Transport};
use crate::token::{RunGh, Token};

/// The app's state, shown in the status bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Status {
    /// Tsugiki hasn't heard from GitHub yet.
    Connecting,
    Online {
        login: String,
    },
    Offline,
    /// Something stops Tsugiki reading, such as a refused token.
    Problem {
        problem: Error,
    },
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

    fn set(&self, status: Status) {
        let mut current = self.lock();
        if *current != status {
            current.clone_from(&status);
            drop(current);
            (self.announce)(&status);
        }
    }
}

/// Reads gh's token, asks GitHub who it belongs to, and shows the result on `board`. Returns the client
/// when there is a token, even offline, since GitHub may come back.
pub fn connect(
    board: &StatusBoard,
    transport: Box<dyn Transport>,
    run_gh: RunGh,
) -> Option<GitHub> {
    let token = match Token::from_gh(run_gh) {
        Ok(token) => token,
        Err(problem) => {
            board.set(Status::Problem { problem });
            return None;
        }
    };
    let github = GitHub::new(transport, token);
    refresh(board, &github);
    Some(github)
}

/// Connects, then asks GitHub every `interval` for as long as the app runs. Returns when there is no
/// token, which `board` then shows, or after a panic. Nothing else would show a panic: the last status
/// would stay up and look current, so this replaces it with a problem. It relies on panics unwinding, so
/// `panic = "abort"` would end the app here instead (#57).
pub fn watch(
    board: &StatusBoard,
    transport: Box<dyn Transport>,
    run_gh: RunGh,
    interval: Duration,
) {
    let watched = catch_unwind(AssertUnwindSafe(|| {
        let Some(github) = connect(board, transport, run_gh) else {
            return;
        };
        loop {
            std::thread::sleep(interval);
            refresh(board, &github);
        }
    }));
    if watched.is_err() {
        let message = "Tsugiki stopped checking GitHub. Restart Tsugiki to check again.";
        board.set(Status::Problem {
            problem: Error::new(ErrorKind::Internal, message),
        });
    }
}

/// Asks GitHub again, so `board` follows the network going and coming back.
fn refresh(board: &StatusBoard, github: &GitHub) {
    board.set(match github.viewer() {
        Ok(viewer) => Status::Online {
            login: viewer.login,
        },
        Err(Error {
            kind: ErrorKind::Offline,
            ..
        }) => Status::Offline,
        Err(problem) => Status::Problem { problem },
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::testing::{Canned, answered, signed_in_gh};
    use crate::github::{Reply, Request, Unreachable};
    use crate::token::Token;
    use std::collections::VecDeque;
    use std::io;
    use std::sync::Arc;

    fn quiet_board() -> StatusBoard {
        StatusBoard::new(Box::new(|_| {}))
    }

    fn offline() -> Box<Canned> {
        Box::new(Canned(Err(Unreachable)))
    }

    fn octocat() -> Status {
        Status::Online {
            login: "octocat".to_owned(),
        }
    }

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
    fn signing_in_shows_the_login() {
        let board = quiet_board();
        let reply = answered(200, r#"{"data":{"viewer":{"login":"octocat"}}}"#);
        assert!(connect(&board, Box::new(reply), signed_in_gh).is_some());
        assert_eq!(board.get(), octocat());
    }

    #[test]
    fn without_the_network_the_status_is_offline() {
        let board = quiet_board();
        assert!(connect(&board, offline(), signed_in_gh).is_some());
        assert_eq!(board.get(), Status::Offline);
    }

    fn problem_kind(status: Status) -> Option<ErrorKind> {
        if let Status::Problem { problem } = status {
            Some(problem.kind)
        } else {
            None
        }
    }

    #[test]
    fn a_refused_token_is_a_problem() {
        let board = quiet_board();
        let _github = connect(&board, Box::new(answered(401, "")), signed_in_gh);
        assert_eq!(problem_kind(board.get()), Some(ErrorKind::Unauthorized));
    }

    #[test]
    fn without_gh_nothing_is_asked_and_the_problem_shows() {
        let board = quiet_board();
        let gh_missing: RunGh = |_| Err(io::Error::from(io::ErrorKind::NotFound));
        assert!(connect(&board, offline(), gh_missing).is_none());
        assert_eq!(problem_kind(board.get()), Some(ErrorKind::Unauthorized));
    }

    #[test]
    fn a_status_without_a_problem_has_no_problem_kind() {
        assert_eq!(problem_kind(Status::Offline), None);
    }

    #[test]
    fn every_change_is_announced() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_by_board = Arc::clone(&seen);
        let board = StatusBoard::new(Box::new(move |status: &Status| {
            seen_by_board.lock().unwrap().push(status.clone());
        }));
        let _github = connect(&board, offline(), signed_in_gh);
        assert_eq!(*seen.lock().unwrap(), [Status::Offline]);
    }

    /// Answers with each reply in turn.
    struct Replies(Mutex<VecDeque<Result<Reply, Unreachable>>>);

    impl Transport for Replies {
        #[expect(
            clippy::unwrap_in_result,
            reason = "too few replies should fail the test"
        )]
        fn post(&self, _: &Token, _: &Request<'_>) -> Result<Reply, Unreachable> {
            self.0.lock().unwrap().pop_front().expect("another reply")
        }
    }

    fn signed_in() -> Result<Reply, Unreachable> {
        Ok(Reply {
            status: 200,
            reset: None,
            body: r#"{"data":{"viewer":{"login":"octocat"}}}"#.to_owned(),
        })
    }

    #[test]
    fn the_status_follows_the_network_going_and_coming_back() {
        let replies = [signed_in(), Err(Unreachable), signed_in()];
        let transport = Box::new(Replies(Mutex::new(replies.into())));
        let board = quiet_board();
        let github = connect(&board, transport, signed_in_gh).unwrap();
        assert_eq!(board.get(), octocat());

        refresh(&board, &github);
        assert_eq!(board.get(), Status::Offline);

        refresh(&board, &github);
        assert_eq!(board.get(), octocat());
    }

    #[test]
    fn watching_follows_github_until_a_panic_and_then_says_it_stopped() {
        // `Replies` panics once it has no replies left.
        let replies = [signed_in(), Err(Unreachable), signed_in()];
        let transport = Box::new(Replies(Mutex::new(replies.into())));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_by_board = Arc::clone(&seen);
        let board = StatusBoard::new(Box::new(move |status: &Status| {
            seen_by_board.lock().unwrap().push(status.clone());
        }));

        watch(&board, transport, signed_in_gh, Duration::ZERO);

        let seen = seen.lock().unwrap();
        assert_eq!(seen[..3], [octocat(), Status::Offline, octocat()]);
        assert_eq!(seen.len(), 4);
        assert_eq!(problem_kind(board.get()), Some(ErrorKind::Internal));
    }

    #[test]
    fn watching_without_gh_keeps_the_problem_that_says_how_to_sign_in() {
        let board = quiet_board();
        let gh_missing: RunGh = |_| Err(io::Error::from(io::ErrorKind::NotFound));
        watch(&board, offline(), gh_missing, Duration::ZERO);
        assert_eq!(problem_kind(board.get()), Some(ErrorKind::Unauthorized));
    }

    #[test]
    fn a_status_that_didnt_change_isnt_announced() {
        let replies = [signed_in(), signed_in()];
        let transport = Box::new(Replies(Mutex::new(replies.into())));
        let seen = Arc::new(Mutex::new(0));
        let seen_by_board = Arc::clone(&seen);
        let board = StatusBoard::new(Box::new(move |_| *seen_by_board.lock().unwrap() += 1));
        let github = connect(&board, transport, signed_in_gh).unwrap();
        refresh(&board, &github);
        assert_eq!(*seen.lock().unwrap(), 1);
    }

    #[test]
    fn the_status_reaches_the_ui_tagged_by_its_state() {
        assert_eq!(
            serde_json::to_value(Status::Offline).unwrap(),
            serde_json::json!({ "state": "offline" })
        );
        assert_eq!(
            serde_json::to_value(octocat()).unwrap(),
            serde_json::json!({ "state": "online", "login": "octocat" })
        );
        let problem = Error::new(ErrorKind::Unauthorized, "Sign in.");
        assert_eq!(
            serde_json::to_value(Status::Problem { problem }).unwrap(),
            serde_json::json!({
                "state": "problem",
                "problem": { "kind": "unauthorized", "message": "Sign in." },
            })
        );
    }
}
