//! One run of Tsugiki: opens the database, then keeps the status following GitHub until the app ends.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use time::OffsetDateTime;

use crate::error::{Error, ErrorKind};
use crate::folders::Folders;
use crate::github::{GitHub, GitHubAccess};
use crate::repository::{RepositoryCheck, RepositoryName};
use crate::status::{Status, StatusBoard};
use crate::store::Store;
use crate::token::Token;

// `run` catches a panic in its thread, to keep the window up and say that checking stopped. Tests unwind
// whatever the release profile says, so only this line can catch the setting (docs/build-costs.md).
#[cfg(panic = "abort")]
compile_error!("`session::run` catches panics, so Tsugiki can't build with `panic = \"abort\"`");

const STOPPED_CHECKING: &str = "Tsugiki stopped checking GitHub. Restart Tsugiki to check again.";
const NOT_CONNECTED: &str = "Tsugiki isn't connected to GitHub. The status bar says why.";

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
    messages: Sender<Message>,
    finished: Receiver<()>,
}

/// What others ask of the session's thread, which alone holds the token and the client.
enum Message {
    Stop,
    Check {
        name: RepositoryName,
        reply: Sender<Result<RepositoryCheck, Error>>,
    },
}

/// Asks the session about Repositories, from any thread. It waits for what the session is doing first, which
/// at the start can be `gh` or a slow request, so don't call it from the thread that draws the window.
pub struct RepositoryChecks {
    messages: Sender<Message>,
}

impl RepositoryChecks {
    /// What adding `name` would load, or why GitHub won't say. A session that has ended has already shown why
    /// on the status bar: no token, or no database.
    pub fn check(&self, name: RepositoryName) -> Result<RepositoryCheck, Error> {
        let (reply, answer) = mpsc::channel();
        self.messages
            .send(Message::Check { name, reply })
            .ok()
            .and_then(|()| answer.recv().ok())
            .unwrap_or_else(|| Err(Error::new(ErrorKind::Offline, NOT_CONNECTED)))
    }
}

impl Session {
    /// Opens the database and asks GitHub who is signed in, then again every `interval`.
    pub fn start(
        mut board: StatusBoard,
        folders: Result<Folders, Error>,
        access: GitHubAccess,
        interval: Duration,
    ) -> Self {
        let (messages, incoming) = mpsc::channel();
        let (finishing, finished) = mpsc::channel::<()>();
        let refreshes = Refreshes {
            every: interval,
            incoming,
        };
        let _ = thread::spawn(move || {
            // `stop` learns the session is over when this goes, however the thread ends.
            let _finishing = finishing;
            run(&mut board, folders, access, &refreshes);
        });
        Self { messages, finished }
    }

    /// A way to ask the session about Repositories. While one exists, only `stop` ends the session.
    pub fn repository_checks(&self) -> RepositoryChecks {
        RepositoryChecks {
            messages: self.messages.clone(),
        }
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
        let _ = self.messages.send(Message::Stop);
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
    incoming: Receiver<Message>,
}

impl Refreshes {
    /// Waits out the interval, answering what's asked meanwhile, and says whether to refresh. A dropped
    /// `Session` stops it too.
    fn wait_for_next(&self, github: &GitHub) -> bool {
        let next = Instant::now() + self.every;
        loop {
            match self
                .incoming
                .recv_timeout(next.saturating_duration_since(Instant::now()))
            {
                Ok(Message::Check { name, reply }) => {
                    let today = OffsetDateTime::now_utc().date();
                    #[expect(
                        clippy::let_underscore_must_use,
                        reason = "this fails only if the caller has gone, and it waits for this answer"
                    )]
                    let _ = reply.send(github.check_repository(&name, today));
                }
                Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return false,
                Err(RecvTimeoutError::Timeout) => return true,
            }
        }
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
    while refreshes.wait_for_next(&github) {
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
    use std::sync::atomic::{AtomicBool, Ordering};
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
        let folder = crate::testing::temp_path(&format!("session-{name}"));
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
            incoming: mpsc::channel().1,
        }
    }

    #[test]
    fn watching_follows_github_until_told_to_stop() {
        // `Replies` panics if it is asked a fourth time, so the third status has to stop the watching.
        let access = replying(vec![signed_in(), Err(Unreachable), signed_in()]);
        let (stop, incoming) = mpsc::channel();
        let (passed, seen) = mpsc::channel();
        let mut shown = 0;
        let mut board = StatusBoard::new(Box::new(move |status: &Status| {
            passed.send(status.clone()).unwrap();
            shown += 1;
            if shown == 3 {
                stop.send(Message::Stop).unwrap();
            }
        }));
        let refreshes = Refreshes {
            every: Duration::ZERO,
            incoming,
        };

        watch(&mut board, &empty_store(), access, &refreshes);

        let offline_as = Status::OfflineAs {
            last_login: Login::new("octocat"),
        };
        assert_eq!(announced(&seen), [octocat(), offline_as, octocat()]);
    }

    const AN_HOUR: Duration = Duration::from_secs(3600);
    const PLENTY: Duration = Duration::from_secs(10);

    const GCMON: &str = r#"{"data":{"repository":{"nameWithOwner":"sergey-miryanov/gcmon",
        "issues":{"totalCount":10},"pullRequests":{"totalCount":3}},
        "updated":{"issueCount":43},"closed":{"issueCount":35}}}"#;

    fn gcmon() -> RepositoryName {
        RepositoryName::parse("sergey-miryanov/gcmon").unwrap()
    }

    #[test]
    fn a_session_answers_a_check_between_refreshes() {
        // `Replies` panics if it is asked a third time, so the check can't bring a refresh with it.
        let gcmon_reply = Reply {
            status: 200,
            reset: None,
            body: GCMON.to_owned(),
        };
        let access = replying(vec![signed_in(), Ok(gcmon_reply)]);
        let folders = temp_folders("check");
        let session = Session::start(quiet_board(), Ok(folders.clone()), access, AN_HOUR);

        let check = session.repository_checks().check(gcmon()).unwrap();
        session.stop(PLENTY);
        fs::remove_dir_all(folders.data).unwrap();

        assert_eq!((check.open_issues, check.open_prs), (10, 3));
    }

    #[test]
    fn a_check_github_cant_answer_says_why() {
        let folders = temp_folders("check-offline");
        let session = Session::start(quiet_board(), Ok(folders.clone()), offline(), AN_HOUR);

        let problem = session.repository_checks().check(gcmon()).unwrap_err();
        session.stop(PLENTY);
        fs::remove_dir_all(folders.data).unwrap();

        assert_eq!(problem.kind, ErrorKind::Offline);
        assert_eq!(problem.message, "Tsugiki can't reach GitHub.");
    }

    #[test]
    fn a_session_that_has_ended_says_so_to_a_check() {
        let access = GitHubAccess {
            transport: Box::new(answered(200, GCMON)),
            run_gh: without_gh,
        };
        let folders = temp_folders("check-ended");
        let session = Session::start(quiet_board(), Ok(folders.clone()), access, AN_HOUR);

        let problem = session.repository_checks().check(gcmon()).unwrap_err();
        session.stop(PLENTY);
        fs::remove_dir_all(folders.data).unwrap();

        assert_eq!(problem.kind, ErrorKind::Offline);
        assert_eq!(problem.message, NOT_CONNECTED);
    }

    #[test]
    fn checks_dont_put_off_the_next_refresh() {
        let (messages, incoming) = mpsc::channel();
        let checks = RepositoryChecks { messages };
        let refreshes = Refreshes {
            every: Duration::from_millis(100),
            incoming,
        };
        let github = GitHub::new(
            Box::new(answered(200, GCMON)),
            crate::github::testing::token(),
        );
        let started = Instant::now();
        let refreshed = AtomicBool::new(false);

        thread::scope(|scope| {
            // Stops by itself, so a timer that starts over with each check fails rather than hangs.
            let _asking = scope.spawn(|| {
                while !refreshed.load(Ordering::Relaxed) && started.elapsed() < PLENTY {
                    drop(checks.check(gcmon()));
                }
                drop(checks.messages.send(Message::Stop));
            });
            assert!(refreshes.wait_for_next(&github));
            refreshed.store(true, Ordering::Relaxed);
            // Answers the checks still on their way, until the message that stops it.
            while refreshes.wait_for_next(&github) {}
        });

        assert!(started.elapsed() < PLENTY);
    }

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
