//! App-wide status, owned by the core (ADR 0008). For now: the connection, who is signed in, and problems.

use std::sync::Mutex;

use serde::Serialize;
use specta::Type;

use crate::error::{Error, ErrorKind};
use crate::github::{GitHub, Transport};
use crate::token::{RunGh, Token};

/// Whether Tsugiki can reach GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    /// Tsugiki hasn't asked yet, or has no token to ask with.
    Unknown,
    Online,
    Offline,
}

/// The app's state, shown in the status bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Status {
    pub connection: Connection,
    /// Who is signed in, once GitHub has said.
    pub login: Option<String>,
    /// What stops Tsugiki reading, such as a refused token.
    pub problem: Option<Error>,
}

/// Holds the status and announces each change, so the app can pass it on as an event.
pub struct StatusBoard {
    current: Mutex<Status>,
    announce: Box<dyn Fn(&Status) + Send + Sync>,
}

impl StatusBoard {
    pub fn new(announce: Box<dyn Fn(&Status) + Send + Sync>) -> Self {
        Self {
            current: Mutex::new(Status {
                connection: Connection::Unknown,
                login: None,
                problem: None,
            }),
            announce,
        }
    }

    pub fn get(&self) -> Status {
        self.current.lock().expect("a status lock").clone()
    }

    fn set(&self, status: Status) {
        let mut current = self.current.lock().expect("a status lock");
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
            board.set(Status {
                connection: Connection::Unknown,
                login: None,
                problem: Some(problem),
            });
            return None;
        }
    };
    let github = GitHub::new(transport, token);
    refresh(board, &github);
    Some(github)
}

/// Asks GitHub again, so `board` follows the network going and coming back.
pub fn refresh(board: &StatusBoard, github: &GitHub) {
    board.set(match github.viewer() {
        Ok(viewer) => Status {
            connection: Connection::Online,
            login: Some(viewer.login),
            problem: None,
        },
        Err(Error {
            kind: ErrorKind::Offline,
            ..
        }) => Status {
            connection: Connection::Offline,
            login: None,
            problem: None,
        },
        Err(problem) => Status {
            connection: Connection::Online,
            login: None,
            problem: Some(problem),
        },
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

    #[test]
    fn nothing_is_known_before_tsugiki_connects() {
        assert_eq!(
            quiet_board().get(),
            Status {
                connection: Connection::Unknown,
                login: None,
                problem: None
            }
        );
    }

    #[test]
    fn signing_in_shows_the_login() {
        let board = quiet_board();
        let reply = answered(200, r#"{"data":{"viewer":{"login":"octocat"}}}"#);
        assert!(connect(&board, Box::new(reply), signed_in_gh).is_some());
        assert_eq!(
            board.get(),
            Status {
                connection: Connection::Online,
                login: Some("octocat".to_owned()),
                problem: None
            }
        );
    }

    #[test]
    fn without_the_network_the_status_is_offline() {
        let board = quiet_board();
        assert!(connect(&board, offline(), signed_in_gh).is_some());
        assert_eq!(
            board.get(),
            Status {
                connection: Connection::Offline,
                login: None,
                problem: None
            }
        );
    }

    #[test]
    fn a_refused_token_is_a_problem_while_online() {
        let board = quiet_board();
        connect(&board, Box::new(answered(401, "")), signed_in_gh);
        let status = board.get();
        assert_eq!(status.connection, Connection::Online);
        assert_eq!(status.problem.unwrap().kind, ErrorKind::Unauthorized);
    }

    #[test]
    fn without_gh_nothing_is_asked_and_the_problem_shows() {
        let board = quiet_board();
        let gh_missing: RunGh = |_| Err(io::Error::from(io::ErrorKind::NotFound));
        assert!(connect(&board, offline(), gh_missing).is_none());
        let status = board.get();
        assert_eq!(status.connection, Connection::Unknown);
        assert_eq!(status.problem.unwrap().kind, ErrorKind::Unauthorized);
    }

    #[test]
    fn every_change_is_announced() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_by_board = Arc::clone(&seen);
        let board = StatusBoard::new(Box::new(move |status| {
            seen_by_board.lock().unwrap().push(status.connection);
        }));
        connect(&board, offline(), signed_in_gh);
        assert_eq!(*seen.lock().unwrap(), [Connection::Offline]);
    }

    /// Answers with each reply in turn.
    struct Replies(Mutex<VecDeque<Result<Reply, Unreachable>>>);

    impl Transport for Replies {
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
        assert_eq!(board.get().login.as_deref(), Some("octocat"));

        refresh(&board, &github);
        assert_eq!(board.get().connection, Connection::Offline);

        refresh(&board, &github);
        assert_eq!(board.get().connection, Connection::Online);
        assert_eq!(board.get().login.as_deref(), Some("octocat"));
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
    fn the_status_reaches_the_ui_in_snake_case() {
        let status = Status {
            connection: Connection::Offline,
            login: None,
            problem: None,
        };
        assert_eq!(
            serde_json::to_value(status).unwrap(),
            serde_json::json!({ "connection": "offline", "login": null, "problem": null })
        );
    }
}
