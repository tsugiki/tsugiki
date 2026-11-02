//! App-wide status, owned by the core (ADR 0008). For now: the connection, who is signed in, and problems.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::Serialize;
use specta::Type;

use crate::error::{Error, ErrorKind};
use crate::folders::Folders;
use crate::github::{GitHub, Login, Transport};
use crate::store::Store;
use crate::token::{RunGh, Token};

const STOPPED_CHECKING: &str = "Tsugiki stopped checking GitHub. Restart Tsugiki to check again.";

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

    fn set(&self, status: Status) {
        let mut current = self.lock();
        if *current != status {
            current.clone_from(&status);
            drop(current);
            (self.announce)(&status);
        }
    }
}

/// How to reach GitHub: the network and gh in the app, stand-ins in tests.
pub struct GitHubAccess {
    pub transport: Box<dyn Transport>,
    pub run_gh: RunGh,
}

/// Shows who was signed in last time, reads gh's token, asks GitHub who it belongs to, and shows the result
/// on `board`. Returns the client when there is a token, even offline, since GitHub may come back.
#[must_use = "without the client, nothing asks GitHub again"]
pub fn connect(board: &StatusBoard, store: &Store, access: GitHubAccess) -> Option<GitHub> {
    // The last login only fills the status bar for a moment, so a failed read doesn't stop Tsugiki
    // connecting. `refresh` uses the store next, and shows the problem if the store is still failing.
    if let Ok(Some(last_login)) = store.last_login() {
        board.set(Status::ConnectingAs { last_login });
    }
    let token = match Token::from_gh(access.run_gh) {
        Ok(token) => token,
        Err(problem) => {
            board.set(Status::Problem { problem });
            return None;
        }
    };
    let github = GitHub::new(access.transport, token);
    refresh(board, store, &github);
    Some(github)
}

/// Opens the database and watches GitHub. Without a database Tsugiki doesn't ask GitHub anything: `board`
/// shows why, and a database from a newer Tsugiki stays as it is (ADR 0010).
///
/// Nothing else would show a panic in here: the last status would stay up and look current, so this replaces
/// it with a problem. It relies on panics unwinding, so `panic = "abort"` would end the app instead (#57).
pub fn start(
    board: &StatusBoard,
    folders: Result<Folders, Error>,
    access: GitHubAccess,
    interval: Duration,
) {
    let started = catch_unwind(AssertUnwindSafe(|| {
        match folders.and_then(|folders| Store::open(&folders.database())) {
            Ok(store) => {
                watch(board, &store, access, interval);
                store.close();
            }
            Err(problem) => board.set(Status::Problem { problem }),
        }
    }));
    if started.is_err() {
        board.set(Status::Problem {
            problem: Error::new(ErrorKind::Internal, STOPPED_CHECKING),
        });
    }
}

/// Connects, then asks GitHub every `interval` for as long as the app runs. Returns only when there is no
/// token, which `board` then shows.
fn watch(board: &StatusBoard, store: &Store, access: GitHubAccess, interval: Duration) {
    let Some(github) = connect(board, store, access) else {
        return;
    };
    loop {
        std::thread::sleep(interval);
        refresh(board, store, &github);
    }
}

/// Asks GitHub again, so `board` follows the network going and coming back.
fn refresh(board: &StatusBoard, store: &Store, github: &GitHub) {
    board.set(match github.viewer() {
        Ok(viewer) => match store.remember_login(&viewer.login) {
            Ok(()) => Status::Online {
                login: viewer.login,
            },
            Err(problem) => Status::Problem { problem },
        },
        Err(Error {
            kind: ErrorKind::Offline,
            ..
        }) => match store.last_login() {
            Ok(Some(last_login)) => Status::OfflineAs { last_login },
            Ok(None) => Status::Offline,
            Err(problem) => Status::Problem { problem },
        },
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
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn quiet_board() -> StatusBoard {
        StatusBoard::new(Box::new(|_| {}))
    }

    /// A board, and everything it has announced.
    fn watched_board() -> (StatusBoard, Arc<Mutex<Vec<Status>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_by_board = Arc::clone(&seen);
        let board = StatusBoard::new(Box::new(move |status: &Status| {
            seen_by_board.lock().unwrap().push(status.clone());
        }));
        (board, seen)
    }

    /// A real store that keeps nothing once it closes.
    fn empty_store() -> Store {
        Store::open(Path::new(":memory:")).unwrap()
    }

    fn store_remembering(login: &str) -> Store {
        let store = empty_store();
        store.remember_login(&Login::new(login)).unwrap();
        store
    }

    fn offline() -> GitHubAccess {
        GitHubAccess {
            transport: Box::new(Canned(Err(Unreachable))),
            run_gh: signed_in_gh,
        }
    }

    fn answering(status: u16, body: &str) -> GitHubAccess {
        GitHubAccess {
            transport: Box::new(answered(status, body)),
            run_gh: signed_in_gh,
        }
    }

    fn octocat_answers() -> GitHubAccess {
        answering(200, r#"{"data":{"viewer":{"login":"octocat"}}}"#)
    }

    fn without_gh(_: &[&str]) -> io::Result<crate::token::GhOutput> {
        Err(io::Error::from(io::ErrorKind::NotFound))
    }

    fn replying(replies: Vec<Result<Reply, Unreachable>>) -> GitHubAccess {
        GitHubAccess {
            transport: Box::new(Replies(Mutex::new(replies.into()))),
            run_gh: signed_in_gh,
        }
    }

    fn octocat() -> Status {
        Status::Online {
            login: Login::new("octocat"),
        }
    }

    fn temp_folders(name: &str) -> Folders {
        let folder: PathBuf =
            std::env::temp_dir().join(format!("tsugiki-status-{name}-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        Folders {
            data: folder.clone(),
            settings: folder,
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
        assert!(connect(&board, &empty_store(), octocat_answers()).is_some());
        assert_eq!(board.get(), octocat());
    }

    #[test]
    fn signing_in_remembers_the_login() {
        let store = store_remembering("hubot");
        assert!(connect(&quiet_board(), &store, octocat_answers()).is_some());
        assert_eq!(store.last_login().unwrap(), Some(Login::new("octocat")));
    }

    #[test]
    fn the_last_login_shows_before_github_answers() {
        let (board, seen) = watched_board();
        let store = store_remembering("hubot");
        assert!(connect(&board, &store, octocat_answers()).is_some());
        assert_eq!(
            *seen.lock().unwrap(),
            [
                Status::ConnectingAs {
                    last_login: Login::new("hubot")
                },
                octocat()
            ]
        );
    }

    #[test]
    fn nobody_is_shown_before_the_first_answer() {
        let (board, seen) = watched_board();
        assert!(connect(&board, &empty_store(), octocat_answers()).is_some());
        assert_eq!(*seen.lock().unwrap(), [octocat()]);
    }

    #[test]
    fn without_the_network_or_a_last_login_the_status_is_offline() {
        let board = quiet_board();
        assert!(connect(&board, &empty_store(), offline()).is_some());
        assert_eq!(board.get(), Status::Offline);
    }

    #[test]
    fn without_the_network_the_last_login_still_shows() {
        let board = quiet_board();
        let store = store_remembering("hubot");
        assert!(connect(&board, &store, offline()).is_some());
        assert_eq!(
            board.get(),
            Status::OfflineAs {
                last_login: Login::new("hubot")
            }
        );
    }

    fn problem_of(status: Status) -> Option<Error> {
        if let Status::Problem { problem } = status {
            Some(problem)
        } else {
            None
        }
    }

    fn problem_kind(status: Status) -> Option<ErrorKind> {
        problem_of(status).map(|problem| problem.kind)
    }

    #[test]
    fn a_refused_token_is_a_problem_and_keeps_the_last_login_stored() {
        let board = quiet_board();
        let store = store_remembering("hubot");
        assert!(connect(&board, &store, answering(401, "")).is_some());
        assert_eq!(problem_kind(board.get()), Some(ErrorKind::Unauthorized));
        assert_eq!(store.last_login().unwrap(), Some(Login::new("hubot")));
    }

    #[test]
    fn without_gh_nothing_is_asked_and_the_problem_shows() {
        let board = quiet_board();
        let access = GitHubAccess {
            transport: Box::new(Canned(Err(Unreachable))),
            run_gh: without_gh,
        };
        assert!(connect(&board, &empty_store(), access).is_none());
        assert_eq!(problem_kind(board.get()), Some(ErrorKind::Unauthorized));
    }

    #[test]
    fn a_status_without_a_problem_has_no_problem_kind() {
        assert_eq!(problem_kind(Status::Offline), None);
    }

    #[test]
    fn a_last_login_that_cant_be_read_doesnt_stop_tsugiki_connecting() {
        let board = quiet_board();
        assert!(connect(&board, &Store::stopped(), octocat_answers()).is_some());
        assert_eq!(problem_kind(board.get()), Some(ErrorKind::Internal));
    }

    #[test]
    fn a_login_that_cant_be_stored_is_a_problem() {
        let board = quiet_board();
        let github = connect(&quiet_board(), &empty_store(), octocat_answers()).unwrap();
        refresh(&board, &Store::stopped(), &github);
        assert_eq!(problem_kind(board.get()), Some(ErrorKind::Internal));
    }

    #[test]
    fn a_last_login_that_cant_be_read_offline_is_a_problem() {
        let board = quiet_board();
        let github = connect(&quiet_board(), &empty_store(), offline()).unwrap();
        refresh(&board, &Store::stopped(), &github);
        assert_eq!(problem_kind(board.get()), Some(ErrorKind::Internal));
    }

    #[test]
    fn every_change_is_announced() {
        let (board, seen) = watched_board();
        assert!(connect(&board, &empty_store(), offline()).is_some());
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
        let access = replying(vec![signed_in(), Err(Unreachable), signed_in()]);
        let board = quiet_board();
        let store = empty_store();
        let github = connect(&board, &store, access).unwrap();
        assert_eq!(board.get(), octocat());

        refresh(&board, &store, &github);
        assert_eq!(
            board.get(),
            Status::OfflineAs {
                last_login: Login::new("octocat")
            }
        );

        refresh(&board, &store, &github);
        assert_eq!(board.get(), octocat());
    }

    #[test]
    fn watching_follows_github_until_something_stops_it() {
        // `Replies` panics once it has no replies left.
        let access = replying(vec![signed_in(), Err(Unreachable), signed_in()]);
        let (board, seen) = watched_board();

        let watched = catch_unwind(AssertUnwindSafe(|| {
            watch(&board, &empty_store(), access, Duration::ZERO);
        }));

        assert!(watched.is_err());
        let offline_as = Status::OfflineAs {
            last_login: Login::new("octocat"),
        };
        assert_eq!(*seen.lock().unwrap(), [octocat(), offline_as, octocat()]);
    }

    #[test]
    fn a_panic_anywhere_in_start_says_tsugiki_stopped_checking() {
        // The first announcement panics, on the path that never reaches `watch`.
        let panicked = AtomicBool::new(false);
        let board = StatusBoard::new(Box::new(move |_: &Status| {
            assert!(
                panicked.swap(true, Ordering::Relaxed),
                "a panic while starting"
            );
        }));
        let lost = Error::new(ErrorKind::Internal, "HOME isn't a full path.");

        start(&board, Err(lost), octocat_answers(), Duration::ZERO);

        assert_eq!(problem_of(board.get()).unwrap().message, STOPPED_CHECKING);
    }

    #[test]
    fn starting_without_gh_keeps_the_problem_that_says_how_to_sign_in() {
        let folders = temp_folders("no-gh");
        let board = quiet_board();
        let access = GitHubAccess {
            transport: Box::new(Canned(Err(Unreachable))),
            run_gh: without_gh,
        };

        start(&board, Ok(folders.clone()), access, Duration::ZERO);

        assert_eq!(problem_kind(board.get()), Some(ErrorKind::Unauthorized));
        assert!(folders.database().exists());
        fs::remove_dir_all(folders.data).unwrap();
    }

    #[test]
    fn folders_that_cant_be_found_show_as_a_problem() {
        let board = quiet_board();
        let lost = Error::new(ErrorKind::Internal, "HOME isn't a full path.");

        start(&board, Err(lost), octocat_answers(), Duration::ZERO);

        assert_eq!(
            problem_of(board.get()).unwrap().message,
            "HOME isn't a full path."
        );
    }

    #[test]
    fn a_newer_database_shows_as_a_problem_and_github_isnt_asked() {
        let folders = temp_folders("newer");
        let newer = rusqlite::Connection::open(folders.database()).unwrap();
        newer.pragma_update(None, "user_version", 9999).unwrap();
        drop(newer);
        let before = fs::read(folders.database()).unwrap();
        let (board, seen) = watched_board();

        start(
            &board,
            Ok(folders.clone()),
            octocat_answers(),
            Duration::ZERO,
        );

        assert_eq!(seen.lock().unwrap().len(), 1, "octocat's answer never came");
        assert!(problem_of(board.get()).unwrap().message.contains("newer"));
        assert_eq!(fs::read(folders.database()).unwrap(), before);
        fs::remove_dir_all(folders.data).unwrap();
    }

    #[test]
    fn a_status_that_didnt_change_isnt_announced() {
        let (board, seen) = watched_board();
        let store = empty_store();
        let github = connect(&board, &store, replying(vec![signed_in(), signed_in()])).unwrap();
        refresh(&board, &store, &github);
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
                octocat(),
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
