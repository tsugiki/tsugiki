//! One run of Tsugiki: opens the database, then keeps the status following GitHub until the app ends.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use crate::error::{Error, ErrorKind};
use crate::folders::Folders;
use crate::github::{GitHub, GitHubAccess};
use crate::status::{Status, StatusBoard};
use crate::store::Store;
use crate::token::Token;

const STOPPED_CHECKING: &str = "Tsugiki stopped checking GitHub. Restart Tsugiki to check again.";

/// Shows who was signed in last time, reads gh's token, asks GitHub who it belongs to, and shows the result
/// on `board`. Returns the client when there is a token, even offline, since GitHub may come back.
#[must_use = "without the client, nothing asks GitHub again"]
fn connect(board: &mut StatusBoard, store: &Store, access: GitHubAccess) -> Option<GitHub> {
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

/// A run of Tsugiki on its own thread, so the window opens without waiting on the disk, gh or the network
/// (ADR 0017). It shows its own failures on the board.
#[must_use = "dropping the session stops it"]
pub struct Session {
    stop: Sender<()>,
    finished: Receiver<()>,
}

impl Session {
    /// Opens the database and asks GitHub who is signed in, then again every `interval`.
    pub fn start(
        mut board: StatusBoard,
        folders: Result<Folders, Error>,
        access: GitHubAccess,
        interval: Duration,
    ) -> Self {
        let (stop, stopping) = mpsc::channel();
        let (finishing, finished) = mpsc::channel::<()>();
        let refreshes = Refreshes {
            every: interval,
            stopping,
        };
        let _ = thread::spawn(move || {
            // `stop` learns the session is over when this goes, however the thread ends.
            let _finishing = finishing;
            run(&mut board, folders, access, &refreshes);
        });
        Self { stop, finished }
    }

    /// Tells the session to finish, and gives it up to `patience` to close the database.
    ///
    /// Between refreshes it closes at once. A request to GitHub, a call to `gh` or a migration can outlast
    /// `patience`, and then the process ends in the middle of it. SQLite recovers at the next start, as it
    /// does after a crash.
    pub fn stop(&self, patience: Duration) {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "this fails only if the session has already finished, which is what this asks for"
        )]
        let _ = self.stop.send(());
        #[expect(
            clippy::let_underscore_must_use,
            reason = "running out of patience is the one failure here, and the doc above says why it is safe"
        )]
        let _ = self.finished.recv_timeout(patience);
    }
}

/// When to ask GitHub again.
struct Refreshes {
    every: Duration,
    stopping: Receiver<()>,
}

impl Refreshes {
    /// Waits out the interval, and says whether to refresh. A dropped `Session` stops it too.
    fn wait_for_next(&self) -> bool {
        matches!(
            self.stopping.recv_timeout(self.every),
            Err(RecvTimeoutError::Timeout)
        )
    }
}

/// Opens the database and watches GitHub. Without a database Tsugiki doesn't ask GitHub anything: `board`
/// shows why, and a database from a newer Tsugiki stays as it is (ADR 0010).
///
/// Nothing else would show a panic in here: the last status would stay up and look current, so this replaces
/// it with a problem. It relies on panics unwinding, so `panic = "abort"` would end the app instead (#57).
fn run(
    board: &mut StatusBoard,
    folders: Result<Folders, Error>,
    access: GitHubAccess,
    refreshes: &Refreshes,
) {
    let started = catch_unwind(AssertUnwindSafe(|| {
        match folders.and_then(|folders| Store::open(&folders.database())) {
            Ok(store) => {
                watch(board, &store, access, refreshes);
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

/// Connects, then asks GitHub again until told to stop. With no token it returns straight away, and `board`
/// shows why.
fn watch(board: &mut StatusBoard, store: &Store, access: GitHubAccess, refreshes: &Refreshes) {
    let Some(github) = connect(board, store, access) else {
        return;
    };
    while refreshes.wait_for_next() {
        refresh(board, store, &github);
    }
}

/// Asks GitHub again, so `board` follows the network going and coming back.
fn refresh(board: &mut StatusBoard, store: &Store, github: &GitHub) {
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
    use crate::github::{Login, Reply, Request, Transport, Unreachable};
    use crate::status::testing::{announced, quiet_board, showing, watched_board};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::{RecvError, TryRecvError};

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
            transport: Box::new(Replies(RefCell::new(replies.into()))),
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
            std::env::temp_dir().join(format!("tsugiki-session-{name}-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        Folders {
            data: folder.clone(),
            settings: folder,
        }
    }

    #[test]
    fn signing_in_shows_the_login() {
        let (mut board, seen) = watched_board();
        assert!(connect(&mut board, &empty_store(), octocat_answers()).is_some());
        assert_eq!(showing(&seen), octocat());
    }

    #[test]
    fn signing_in_remembers_the_login() {
        let store = store_remembering("hubot");
        assert!(connect(&mut quiet_board(), &store, octocat_answers()).is_some());
        assert_eq!(store.last_login().unwrap(), Some(Login::new("octocat")));
    }

    #[test]
    fn the_last_login_shows_before_github_answers() {
        let (mut board, seen) = watched_board();
        let store = store_remembering("hubot");
        assert!(connect(&mut board, &store, octocat_answers()).is_some());
        assert_eq!(
            announced(&seen),
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
        let (mut board, seen) = watched_board();
        assert!(connect(&mut board, &empty_store(), octocat_answers()).is_some());
        assert_eq!(announced(&seen), [octocat()]);
    }

    #[test]
    fn without_the_network_or_a_last_login_the_status_is_offline() {
        let (mut board, seen) = watched_board();
        assert!(connect(&mut board, &empty_store(), offline()).is_some());
        assert_eq!(showing(&seen), Status::Offline);
    }

    #[test]
    fn without_the_network_the_last_login_still_shows() {
        let (mut board, seen) = watched_board();
        let store = store_remembering("hubot");
        assert!(connect(&mut board, &store, offline()).is_some());
        assert_eq!(
            showing(&seen),
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
        let (mut board, seen) = watched_board();
        let store = store_remembering("hubot");
        assert!(connect(&mut board, &store, answering(401, "")).is_some());
        assert_eq!(problem_kind(showing(&seen)), Some(ErrorKind::Unauthorized));
        assert_eq!(store.last_login().unwrap(), Some(Login::new("hubot")));
    }

    #[test]
    fn without_gh_nothing_is_asked_and_the_problem_shows() {
        let (mut board, seen) = watched_board();
        let access = GitHubAccess {
            transport: Box::new(Canned(Err(Unreachable))),
            run_gh: without_gh,
        };
        assert!(connect(&mut board, &empty_store(), access).is_none());
        assert_eq!(problem_kind(showing(&seen)), Some(ErrorKind::Unauthorized));
    }

    #[test]
    fn a_status_without_a_problem_has_no_problem_kind() {
        assert_eq!(problem_kind(Status::Offline), None);
    }

    #[test]
    fn a_last_login_that_cant_be_read_doesnt_stop_tsugiki_connecting() {
        let (mut board, seen) = watched_board();
        assert!(connect(&mut board, &Store::stopped(), octocat_answers()).is_some());
        assert_eq!(problem_kind(showing(&seen)), Some(ErrorKind::Internal));
    }

    #[test]
    fn a_login_that_cant_be_stored_is_a_problem() {
        let (mut board, seen) = watched_board();
        let github = connect(&mut quiet_board(), &empty_store(), octocat_answers()).unwrap();
        refresh(&mut board, &Store::stopped(), &github);
        assert_eq!(problem_kind(showing(&seen)), Some(ErrorKind::Internal));
    }

    #[test]
    fn a_last_login_that_cant_be_read_offline_is_a_problem() {
        let (mut board, seen) = watched_board();
        let github = connect(&mut quiet_board(), &empty_store(), offline()).unwrap();
        refresh(&mut board, &Store::stopped(), &github);
        assert_eq!(problem_kind(showing(&seen)), Some(ErrorKind::Internal));
    }

    /// Answers with each reply in turn.
    struct Replies(RefCell<VecDeque<Result<Reply, Unreachable>>>);

    impl Transport for Replies {
        fn post(&self, _: &Token, _: &Request<'_>) -> Result<Reply, Unreachable> {
            self.0.borrow_mut().pop_front().expect("another reply")
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
        let (mut board, seen) = watched_board();
        let store = empty_store();
        let github = connect(&mut board, &store, access).unwrap();
        assert_eq!(showing(&seen), octocat());

        refresh(&mut board, &store, &github);
        assert_eq!(
            showing(&seen),
            Status::OfflineAs {
                last_login: Login::new("octocat")
            }
        );

        refresh(&mut board, &store, &github);
        assert_eq!(showing(&seen), octocat());
    }

    /// Refreshes told to stop before they began, for a test that shouldn't get as far as a second request.
    fn stopped_refreshes() -> Refreshes {
        Refreshes {
            every: Duration::ZERO,
            stopping: mpsc::channel().1,
        }
    }

    #[test]
    fn watching_follows_github_until_told_to_stop() {
        // `Replies` panics if it is asked a fourth time, so the third status has to stop the watching.
        let access = replying(vec![signed_in(), Err(Unreachable), signed_in()]);
        let (stop, stopping) = mpsc::channel();
        let (passed, seen) = mpsc::channel();
        let mut shown = 0;
        let mut board = StatusBoard::new(Box::new(move |status: &Status| {
            passed.send(status.clone()).unwrap();
            shown += 1;
            if shown == 3 {
                stop.send(()).unwrap();
            }
        }));
        let refreshes = Refreshes {
            every: Duration::ZERO,
            stopping,
        };

        watch(&mut board, &empty_store(), access, &refreshes);

        let offline_as = Status::OfflineAs {
            last_login: Login::new("octocat"),
        };
        assert_eq!(announced(&seen), [octocat(), offline_as, octocat()]);
    }

    const AN_HOUR: Duration = Duration::from_secs(3600);
    const PLENTY: Duration = Duration::from_secs(10);

    fn wal_of(folders: &Folders) -> PathBuf {
        folders.data.join("tsugiki.db-wal")
    }

    #[test]
    fn stopping_between_refreshes_closes_the_store() {
        let folders = temp_folders("stop-between");
        let (board, statuses) = watched_board();
        let session = Session::start(board, Ok(folders.clone()), octocat_answers(), AN_HOUR);
        assert_eq!(statuses.recv().unwrap(), octocat());
        let waiting = fs::metadata(wal_of(&folders)).unwrap().len();
        assert!(waiting > 0, "the login is still in the -wal");

        session.stop(PLENTY);

        assert!(!wal_of(&folders).exists(), "closing moved it to the file");
        let store = Store::open(&folders.database()).unwrap();
        assert_eq!(store.last_login().unwrap(), Some(Login::new("octocat")));
        store.close();
        fs::remove_dir_all(folders.data).unwrap();
    }

    /// Holds its answer back until the test lets go.
    struct Held {
        asked: Sender<()>,
        answer: Receiver<()>,
    }

    impl Transport for Held {
        #[expect(
            clippy::unwrap_in_result,
            reason = "a test that lets go of either channel early should fail"
        )]
        fn post(&self, _: &Token, _: &Request<'_>) -> Result<Reply, Unreachable> {
            self.asked.send(()).unwrap();
            self.answer.recv().unwrap();
            signed_in()
        }
    }

    #[test]
    fn stopping_doesnt_wait_out_a_request_in_progress() {
        let folders = temp_folders("stop-in-request");
        let (asked, request) = mpsc::channel();
        let (answer, answering) = mpsc::channel();
        let access = GitHubAccess {
            transport: Box::new(Held {
                asked,
                answer: answering,
            }),
            run_gh: signed_in_gh,
        };
        let session = Session::start(quiet_board(), Ok(folders.clone()), access, AN_HOUR);
        request.recv().unwrap();

        session.stop(Duration::from_millis(10));

        assert_eq!(session.finished.try_recv(), Err(TryRecvError::Empty));
        answer.send(()).unwrap();
        let finished = session.finished.recv_timeout(PLENTY);
        assert_eq!(
            finished,
            Err(RecvTimeoutError::Disconnected),
            "it finishes after all"
        );
        fs::remove_dir_all(folders.data).unwrap();
    }

    #[test]
    fn stopping_a_session_that_has_already_finished_returns_at_once() {
        let folders = temp_folders("stop-finished");
        let access = GitHubAccess {
            transport: Box::new(Canned(Err(Unreachable))),
            run_gh: without_gh,
        };
        let session = Session::start(quiet_board(), Ok(folders.clone()), access, AN_HOUR);
        assert_eq!(session.finished.recv(), Err(RecvError));

        session.stop(AN_HOUR);

        fs::remove_dir_all(folders.data).unwrap();
    }

    #[test]
    fn a_panic_anywhere_in_a_run_says_tsugiki_stopped_checking() {
        // The first announcement panics, on the path that never reaches `watch`.
        let (passed, seen) = mpsc::channel();
        let mut announcements = 0;
        let mut board = StatusBoard::new(Box::new(move |status: &Status| {
            announcements += 1;
            assert!(announcements > 1, "a panic while starting");
            passed.send(status.clone()).unwrap();
        }));
        let lost = Error::new(ErrorKind::Internal, "HOME isn't a full path.");

        run(
            &mut board,
            Err(lost),
            octocat_answers(),
            &stopped_refreshes(),
        );

        assert_eq!(
            problem_of(showing(&seen)).unwrap().message,
            STOPPED_CHECKING
        );
    }

    #[test]
    fn starting_without_gh_keeps_the_problem_that_says_how_to_sign_in() {
        let folders = temp_folders("no-gh");
        let (mut board, seen) = watched_board();
        let access = GitHubAccess {
            transport: Box::new(Canned(Err(Unreachable))),
            run_gh: without_gh,
        };

        run(
            &mut board,
            Ok(folders.clone()),
            access,
            &stopped_refreshes(),
        );

        assert_eq!(problem_kind(showing(&seen)), Some(ErrorKind::Unauthorized));
        assert!(folders.database().exists());
        fs::remove_dir_all(folders.data).unwrap();
    }

    #[test]
    fn folders_that_cant_be_found_show_as_a_problem() {
        let (mut board, seen) = watched_board();
        let lost = Error::new(ErrorKind::Internal, "HOME isn't a full path.");

        run(
            &mut board,
            Err(lost),
            octocat_answers(),
            &stopped_refreshes(),
        );

        assert_eq!(
            problem_of(showing(&seen)).unwrap().message,
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
        let (mut board, seen) = watched_board();

        run(
            &mut board,
            Ok(folders.clone()),
            octocat_answers(),
            &stopped_refreshes(),
        );

        let [only]: [Status; 1] = announced(&seen).try_into().unwrap();
        assert!(problem_of(only).unwrap().message.contains("newer"));
        assert_eq!(fs::read(folders.database()).unwrap(), before);
        fs::remove_dir_all(folders.data).unwrap();
    }
}
