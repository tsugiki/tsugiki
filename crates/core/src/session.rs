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
use crate::store::{self, OpenReader, ReaderToOpen, Store, StoreReader};
use crate::sync::Syncs;
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
    reader: StoreReader,
}

/// What others ask of the session's thread, which alone holds the token and the client.
enum Message {
    Stop,
    Check {
        name: RepositoryName,
        reply: Sender<Result<RepositoryCheck, Error>>,
    },
    Sync {
        name: RepositoryName,
    },
}

/// Asks the session about Repositories, from any thread. A check waits for what the session is doing first,
/// which at the start can be `gh` or a slow request, so don't call it from the thread that draws the window.
#[derive(Clone)]
pub struct SessionRequests {
    messages: Sender<Message>,
}

impl SessionRequests {
    /// Asks the session to load the user's work in `name`, and returns at once. A session with no token or no
    /// database has already shown why on the status bar, so this reports no failure.
    pub fn sync(&self, name: RepositoryName) {
        drop(self.messages.send(Message::Sync { name }));
    }

    /// Asks GitHub what adding `name` would load. A session with no token or no database answers with an
    /// error, and has already shown why on the status bar.
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
        let (to_open, reader) = store::reader();
        let database = Database { folders, to_open };
        let refreshes = Refreshes {
            every: interval,
            incoming,
        };
        let _ = thread::spawn(move || {
            // `stop` learns the session is over when this goes, however the thread ends.
            let _finishing = finishing;
            run(&mut board, database, access, &refreshes);
        });
        Self {
            messages,
            finished,
            reader,
        }
    }

    /// A way to ask the session about Repositories. While one exists, only `stop` ends the session.
    pub fn requests(&self) -> SessionRequests {
        SessionRequests {
            messages: self.messages.clone(),
        }
    }

    /// The read-only connection, which doesn't wait for anything this session is doing (ADR 0018).
    pub fn reader(&self) -> StoreReader {
        self.reader.clone()
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

/// The session thread's next move.
#[derive(Debug, PartialEq, Eq)]
enum Turn {
    Refresh,
    SyncStep,
    Stop,
}

impl Refreshes {
    /// Waits for a stop message, for a session with nothing to ask GitHub with. A check dropped here tells
    /// the one asking that the session isn't connected.
    fn wait_for_stop(&self) {
        while let Ok(message) = self.incoming.recv() {
            match message {
                Message::Stop => return,
                Message::Check { .. } | Message::Sync { .. } => {}
            }
        }
    }

    /// Waits for the next refresh or the next step of a sync, answering what's asked meanwhile, so a step
    /// doesn't keep anyone waiting for longer than one request. A dropped `Session` stops it too.
    fn next_turn(&self, github: &GitHub, syncs: &mut Syncs, refresh_at: Instant) -> Turn {
        loop {
            let wake = syncs.ready_at().map_or(refresh_at, |at| at.min(refresh_at));
            match self
                .incoming
                .recv_timeout(wake.saturating_duration_since(Instant::now()))
            {
                Ok(Message::Sync { name }) => syncs.add(name),
                Ok(Message::Check { name, reply }) => {
                    let today = OffsetDateTime::now_utc().date();
                    #[expect(
                        clippy::let_underscore_must_use,
                        reason = "this fails only if the caller has gone, and it waits for this answer"
                    )]
                    let _ = reply.send(github.check_repository(&name, today));
                }
                Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return Turn::Stop,
                Err(RecvTimeoutError::Timeout) if Instant::now() >= refresh_at => {
                    return Turn::Refresh;
                }
                Err(RecvTimeoutError::Timeout) => return Turn::SyncStep,
            }
        }
    }
}

/// The database a session opens when it starts.
struct Database {
    folders: Result<Folders, Error>,
    to_open: ReaderToOpen,
}

impl Database {
    /// The writer first, since it moves the file to the current schema, and then the read-only connection.
    fn open(self) -> Result<(Store, OpenReader), Error> {
        let path = self.folders?.database();
        let store = Store::open(&path)?;
        let reader = self.to_open.open(&path)?;
        Ok((store, reader))
    }
}

/// Opens the database and watches GitHub. Without a database Tsugiki doesn't ask GitHub anything: `board`
/// shows why, and a database from a newer Tsugiki stays as it is (ADR 0010).
///
/// Nothing else would show a panic in here: the last status would stay up and look current, so this replaces
/// it with a problem. It relies on panics unwinding, so `panic = "abort"` would end the app instead (#57).
fn run(board: &mut StatusBoard, database: Database, access: GitHubAccess, refreshes: &Refreshes) {
    let started = catch_unwind(AssertUnwindSafe(|| match database.open() {
        Ok((store, reader)) => {
            watch(board, &store, access, refreshes);
            reader.close();
            store.close();
        }
        Err(problem) => board.set(Status::Problem { problem }),
    }));
    if started.is_err() {
        board.set(Status::Problem {
            problem: Error::new(ErrorKind::Internal, STOPPED_CHECKING),
        });
    }
}

/// Connects, then refreshes and runs the syncs the app asks for, until the app stops it. With no token
/// `board` shows why, and the database stays open until then, so the window can read what's stored.
fn watch(board: &mut StatusBoard, store: &Store, access: GitHubAccess, refreshes: &Refreshes) {
    let Some(github) = connect(board, store, access) else {
        refreshes.wait_for_stop();
        return;
    };
    let mut syncs = Syncs::default();
    let mut refresh_at = Instant::now() + refreshes.every;
    loop {
        match refreshes.next_turn(&github, &mut syncs, refresh_at) {
            Turn::Stop => return,
            Turn::Refresh => {
                refresh(board, store, &github);
                refresh_at = Instant::now() + refreshes.every;
            }
            // A Workbench lists nothing until Tsugiki knows who the user is, and no event tells the window
            // when it learns that. So a sync waits for it, and its first save then shows the rows.
            Turn::SyncStep if !matches!(store.last_login(), Ok(Some(_))) => {
                syncs.wait_until(refresh_at);
            }
            Turn::SyncStep => match syncs.step(&github, store) {
                Ok(Some(repository)) => board.workbench_changed(&repository),
                Ok(None) => {}
                // The next refresh shows whether GitHub is back, and the sync carries on from there.
                Err(problem) => {
                    syncs.after_failure(&problem, refresh_at);
                    board.set(status_after(problem, store));
                }
            },
        }
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
        Err(problem) => status_after(problem, store),
    });
}

/// The status bar's words after GitHub didn't answer as it should.
fn status_after(problem: Error, store: &Store) -> Status {
    match problem.kind {
        ErrorKind::Offline => match store.last_login() {
            Ok(Some(last_login)) => Status::OfflineAs { last_login },
            Ok(None) => Status::Offline,
            Err(problem) => Status::Problem { problem },
        },
        ErrorKind::Unauthorized
        | ErrorKind::WriteRefused
        | ErrorKind::RateLimited { .. }
        | ErrorKind::NotFound
        | ErrorKind::Conflict
        | ErrorKind::Internal => Status::Problem { problem },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::testing::{Canned, Scripted, answered, reply_ok, signed_in_gh};
    use crate::github::{Login, Reply, Request, Transport, Unreachable};
    use crate::repository::RepositorySettings;
    use crate::status::testing::{announced, quiet_board, showing, watched_board, workbench_board};
    use crate::workbench::{Workbench, WorkbenchItem};
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

    /// The `Database` for `run`, with no reader waiting for it.
    fn database(folders: Result<Folders, Error>) -> Database {
        let (to_open, _reader) = store::reader();
        Database { folders, to_open }
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

        let check = session.requests().check(gcmon()).unwrap();
        session.stop(PLENTY);
        fs::remove_dir_all(folders.data).unwrap();

        assert_eq!((check.open_issues, check.open_prs), (10, 3));
    }

    #[test]
    fn a_check_github_cant_answer_says_why() {
        let folders = temp_folders("check-offline");
        let session = Session::start(quiet_board(), Ok(folders.clone()), offline(), AN_HOUR);

        let problem = session.requests().check(gcmon()).unwrap_err();
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

        let problem = session.requests().check(gcmon()).unwrap_err();
        session.stop(PLENTY);
        fs::remove_dir_all(folders.data).unwrap();

        assert_eq!(problem.kind, ErrorKind::Offline);
        assert_eq!(problem.message, NOT_CONNECTED);
    }

    #[test]
    fn checks_dont_put_off_the_next_refresh() {
        let (messages, incoming) = mpsc::channel();
        let checks = SessionRequests { messages };
        let refreshes = Refreshes {
            every: Duration::from_millis(100),
            incoming,
        };
        let github = GitHub::new(
            Box::new(answered(200, GCMON)),
            crate::github::testing::token(),
        );
        let started = Instant::now();
        let refresh_at = started + refreshes.every;
        let mut syncs = Syncs::default();
        let refreshed = AtomicBool::new(false);

        thread::scope(|scope| {
            // Stops by itself, so a timer that starts over with each check fails rather than hangs.
            let _asking = scope.spawn(|| {
                while !refreshed.load(Ordering::Relaxed) && started.elapsed() < PLENTY {
                    drop(checks.check(gcmon()));
                }
                drop(checks.messages.send(Message::Stop));
            });
            assert_eq!(
                refreshes.next_turn(&github, &mut syncs, refresh_at),
                Turn::Refresh
            );
            refreshed.store(true, Ordering::Relaxed);
            // Answers the checks still on their way, until the message that stops it.
            while refreshes.next_turn(&github, &mut syncs, refresh_at) != Turn::Stop {}
        });

        assert!(started.elapsed() < PLENTY);
    }

    const WORK_RATE: &str = r#""rateLimit":{"remaining":4000,"resetAt":"2026-11-04T12:00:00Z"}"#;

    fn work_search(ids: &[&str]) -> Result<Reply, Unreachable> {
        let nodes: Vec<serde_json::Value> = ids
            .iter()
            .map(|id| serde_json::json!({ "id": id }))
            .collect();
        let body = format!(
            r#"{{"data":{{"search":{{"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":{}}},{WORK_RATE}}}}}"#,
            serde_json::json!(nodes)
        );
        reply_ok(&body)
    }

    fn work_fetch() -> Result<Reply, Unreachable> {
        let node = serde_json::json!({
            "number": 7,
            "title": "gh-7: Fix it",
            "state": "OPEN",
            "isDraft": false,
            "updatedAt": "2026-11-04T10:00:00Z",
            "baseRefName": "main",
            "body": "",
            "repository": {
                "nameWithOwner": "sergey-miryanov/gcmon",
                "defaultBranchRef": { "name": "main" },
            },
            "closingIssuesReferences": { "nodes": [] },
            "author": { "login": "octocat" },
            "assignees": { "nodes": [] },
            "reviewRequests": { "nodes": [] },
            "latestReviews": { "nodes": [] },
            "commits": { "nodes": [] },
        });
        reply_ok(&format!(r#"{{"data":{{"nodes":[{node}],{WORK_RATE}}}}}"#))
    }

    #[test]
    fn a_sync_fills_the_workbench_and_says_so() {
        let (script, _asked) = Scripted::new(vec![
            signed_in(),
            work_search(&["a"]),
            work_search(&[]),
            work_search(&[]),
            work_search(&[]),
            work_fetch(),
        ]);
        let access = GitHubAccess {
            transport: Box::new(script),
            run_gh: signed_in_gh,
        };
        let folders = temp_folders("sync");
        let (board, changes) = workbench_board();
        let session = Session::start(board, Ok(folders.clone()), access, AN_HOUR);
        let reader = session.reader();

        session.requests().sync(gcmon());
        let changed = changes.recv().unwrap();
        let workbench = Workbench::read(&reader, &RepositorySettings::new(gcmon())).unwrap();
        session.stop(PLENTY);
        fs::remove_dir_all(folders.data).unwrap();

        assert_eq!(changed, gcmon());
        assert!(
            matches!(&workbench.items[..], [WorkbenchItem::StandalonePr(row)] if row.number == 7),
            "{workbench:?}"
        );
    }

    #[test]
    fn a_check_goes_ahead_of_the_next_step_of_a_sync() {
        let (messages, incoming) = mpsc::channel();
        let checks = SessionRequests { messages };
        let refreshes = Refreshes {
            every: AN_HOUR,
            incoming,
        };
        let github = GitHub::new(
            Box::new(answered(200, GCMON)),
            crate::github::testing::token(),
        );
        let mut syncs = Syncs::default();
        syncs.add(gcmon());
        let refresh_at = Instant::now() + AN_HOUR;

        let answer = thread::scope(|scope| {
            let asking = scope.spawn(|| checks.check(gcmon()));
            // A step can run only after the session answers the check, or this would hang.
            while !asking.is_finished() {
                let turn = refreshes.next_turn(&github, &mut syncs, refresh_at);
                assert_eq!(turn, Turn::SyncStep);
            }
            asking.join().unwrap()
        });

        assert_eq!(answer.unwrap().open_prs, 3);
    }

    #[test]
    fn a_sync_github_cant_answer_shows_offline_and_waits_for_the_next_refresh() {
        // `Replies` panics at a fifth question, so the sync can't run ahead of the refresh.
        let access = replying(vec![
            signed_in(),
            Err(Unreachable),
            Err(Unreachable),
            signed_in(),
        ]);
        let (messages, incoming) = mpsc::channel();
        let requests = SessionRequests {
            messages: messages.clone(),
        };
        requests.sync(gcmon());
        let (passed, seen) = mpsc::channel();
        let mut shown = 0;
        let mut board = StatusBoard::new(Box::new(move |status: &Status| {
            passed.send(status.clone()).unwrap();
            shown += 1;
            if shown == 3 {
                messages.send(Message::Stop).unwrap();
            }
        }));
        let refreshes = Refreshes {
            every: Duration::from_millis(50),
            incoming,
        };

        watch(&mut board, &empty_store(), access, &refreshes);

        let offline_as = Status::OfflineAs {
            last_login: Login::new("octocat"),
        };
        assert_eq!(announced(&seen), [octocat(), offline_as, octocat()]);
    }

    /// Can't reach GitHub for its first question about the viewer. After that, answers each question by name.
    struct BlipThenWork {
        blip: AtomicBool,
    }

    impl Transport for BlipThenWork {
        fn post(
            &self,
            _: &crate::token::Token,
            request: &Request<'_>,
        ) -> Result<Reply, Unreachable> {
            match request.operation {
                "Viewer" if self.blip.swap(false, Ordering::Relaxed) => Err(Unreachable),
                "Viewer" => signed_in(),
                "FetchPullRequests" => work_fetch(),
                _ => work_search(&["a"]),
            }
        }
    }

    #[test]
    fn a_sync_waits_until_tsugiki_knows_who_the_user_is() {
        let access = GitHubAccess {
            transport: Box::new(BlipThenWork {
                blip: AtomicBool::new(true),
            }),
            run_gh: signed_in_gh,
        };
        let folders = temp_folders("late-viewer");
        let (board, changes) = workbench_board();
        let session = Session::start(
            board,
            Ok(folders.clone()),
            access,
            Duration::from_millis(50),
        );
        let reader = session.reader();

        session.requests().sync(gcmon());
        drop(changes.recv().unwrap());
        let workbench = Workbench::read(&reader, &RepositorySettings::new(gcmon())).unwrap();
        session.stop(PLENTY);
        fs::remove_dir_all(folders.data).unwrap();

        assert_eq!(workbench.items.len(), 1);
    }

    #[test]
    fn with_no_token_what_is_stored_stays_readable() {
        let access = GitHubAccess {
            transport: Box::new(Canned(Err(Unreachable))),
            run_gh: without_gh,
        };
        let folders = temp_folders("no-token");
        let session = Session::start(quiet_board(), Ok(folders.clone()), access, AN_HOUR);
        let requests = session.requests();

        requests.sync(gcmon());
        let checked = requests.check(gcmon());
        let facts = session.reader().work_facts(&gcmon());
        session.stop(PLENTY);
        fs::remove_dir_all(folders.data).unwrap();

        assert_eq!(checked.unwrap_err().kind, ErrorKind::Offline);
        assert_eq!(facts.unwrap().pull_requests, []);
    }

    #[test]
    fn a_session_with_no_token_ends_when_it_can_no_longer_be_asked_anything() {
        let (messages, incoming) = mpsc::channel();
        let refreshes = Refreshes {
            every: AN_HOUR,
            incoming,
        };
        drop(messages);

        refreshes.wait_for_stop();
    }

    #[test]
    fn a_session_that_has_ended_ignores_a_sync() {
        let (messages, incoming) = mpsc::channel();
        drop(incoming);

        SessionRequests { messages }.sync(gcmon());
    }

    #[test]
    fn a_reader_answers_once_the_session_has_opened_the_database() {
        let folders = temp_folders("reader");
        let session = Session::start(quiet_board(), Ok(folders.clone()), offline(), AN_HOUR);

        let facts = session.reader().work_facts(&gcmon());
        session.stop(PLENTY);
        fs::remove_dir_all(folders.data).unwrap();

        assert_eq!(facts.unwrap().pull_requests, []);
    }

    #[test]
    fn a_session_with_no_database_answers_its_readers_with_that() {
        let lost = Error::new(ErrorKind::Internal, "HOME isn't a full path.");
        let session = Session::start(quiet_board(), Err(lost), offline(), AN_HOUR);

        let refused = session.reader().work_facts(&gcmon());
        session.stop(PLENTY);

        assert_eq!(refused.unwrap_err().kind, ErrorKind::Internal);
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
        let lost = Error::new(ErrorKind::Internal, "HOME isn't a full path.");
        let session = Session::start(quiet_board(), Err(lost), offline(), AN_HOUR);
        assert_eq!(session.finished.recv(), Err(RecvError));

        session.stop(AN_HOUR);
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
            database(Err(lost)),
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
            database(Ok(folders.clone())),
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
            database(Err(lost)),
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
            database(Ok(folders.clone())),
            octocat_answers(),
            &stopped_refreshes(),
        );

        let [only]: [Status; 1] = announced(&seen).try_into().unwrap();
        assert!(problem_of(only).unwrap().message.contains("newer"));
        assert_eq!(fs::read(folders.database()).unwrap(), before);
        fs::remove_dir_all(folders.data).unwrap();
    }
}
