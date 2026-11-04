use super::*;
use crate::github::testing::{Canned, Scripted, answered, reply_ok, signed_in_gh};
use crate::github::{Login, Reply, Request, Transport, Unreachable};
use crate::status::Change;
use crate::status::testing::{announced, changes_board, quiet_board, showing, watched_board};
use crate::store::{ChangeCheck, SyncState};
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
        intervals: Intervals {
            status: Duration::ZERO,
            repository: AN_HOUR,
        },
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
        intervals: Intervals {
            status: Duration::ZERO,
            repository: AN_HOUR,
        },
        incoming,
    };

    watch(&mut board, &empty_store(), access, &refreshes);

    let offline_as = Status::OfflineAs {
        last_login: Login::new("octocat"),
    };
    assert_eq!(announced(&seen), [octocat(), offline_as, octocat()]);
}

const AN_HOUR: Duration = Duration::from_secs(3600);
const HOURLY: Intervals = Intervals {
    status: AN_HOUR,
    repository: AN_HOUR,
};
const PLENTY: Duration = Duration::from_secs(10);

const GCMON: &str = r#"{"data":{"repository":{"nameWithOwner":"sergey-miryanov/gcmon",
        "issues":{"totalCount":10},"pullRequests":{"totalCount":3}},
        "updated":{"issueCount":43},"closed":{"issueCount":35}}}"#;

fn gcmon() -> RepositoryName {
    RepositoryName::parse("sergey-miryanov/gcmon").unwrap()
}

/// A Repository with no Filters, so a sync asks for what these tests script.
fn plain_gcmon() -> RepositorySettings {
    RepositorySettings {
        filters: Vec::new(),
        ..RepositorySettings::new(gcmon())
    }
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
    let session = Session::start(quiet_board(), Ok(folders.clone()), access, HOURLY);

    let check = session.requests().check(gcmon()).unwrap();
    session.stop(PLENTY);
    fs::remove_dir_all(folders.data).unwrap();

    assert_eq!((check.open_issues, check.open_prs), (10, 3));
}

#[test]
fn a_check_github_cant_answer_says_why() {
    let folders = temp_folders("check-offline");
    let session = Session::start(quiet_board(), Ok(folders.clone()), offline(), HOURLY);

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
    let session = Session::start(quiet_board(), Ok(folders.clone()), access, HOURLY);

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
        intervals: Intervals {
            status: Duration::from_millis(100),
            repository: AN_HOUR,
        },
        incoming,
    };
    let github = GitHub::new(
        Box::new(answered(200, GCMON)),
        crate::github::testing::token(),
    );
    let started = Instant::now();
    let due = Due::starting_at(started, refreshes.intervals);
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
        assert_eq!(refreshes.next_turn(&github, &mut syncs, due), Turn::Refresh);
        refreshed.store(true, Ordering::Relaxed);
        // Answers the checks still on their way, until the message that stops it.
        while refreshes.next_turn(&github, &mut syncs, due) != Turn::Stop {}
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
        "lastCommit": { "nodes": [] },
        "mergeCommit": null,
        "commits": { "totalCount": 0, "nodes": [] },
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
    let (board, changes) = changes_board();
    let session = Session::start(board, Ok(folders.clone()), access, HOURLY);
    let reader = session.reader();

    session.requests().sync(plain_gcmon());
    let changed = changes.recv().unwrap();
    let workbench = Workbench::read(&reader, &plain_gcmon(), None).unwrap();
    session.stop(PLENTY);
    fs::remove_dir_all(folders.data).unwrap();

    assert_eq!(
        changed,
        Change {
            repository: gcmon(),
            items: vec![7],
        }
    );
    assert!(
        matches!(&workbench.items[..], [WorkbenchItem::StandalonePr(tree)] if tree.row.number == 7),
        "{workbench:?}"
    );
}

#[test]
fn a_check_goes_ahead_of_the_next_step_of_a_sync() {
    let (messages, incoming) = mpsc::channel();
    let checks = SessionRequests { messages };
    let refreshes = Refreshes {
        intervals: HOURLY,
        incoming,
    };
    let github = GitHub::new(
        Box::new(answered(200, GCMON)),
        crate::github::testing::token(),
    );
    let mut syncs = Syncs::default();
    syncs.add(plain_gcmon());
    let due = Due::starting_at(Instant::now(), HOURLY);

    let answer = thread::scope(|scope| {
        let asking = scope.spawn(|| checks.check(gcmon()));
        // A step can run only after the session answers the check, or this would hang.
        while !asking.is_finished() {
            let turn = refreshes.next_turn(&github, &mut syncs, due);
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
    requests.sync(plain_gcmon());
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
        intervals: Intervals {
            status: Duration::from_millis(50),
            repository: AN_HOUR,
        },
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
    fn post(&self, _: &crate::token::Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
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
    let (board, changes) = changes_board();
    let session = Session::start(
        board,
        Ok(folders.clone()),
        access,
        Intervals {
            status: Duration::from_millis(50),
            repository: AN_HOUR,
        },
    );
    let reader = session.reader();

    session.requests().sync(plain_gcmon());
    drop(changes.recv().unwrap());
    let workbench = Workbench::read(&reader, &plain_gcmon(), None).unwrap();
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
    let session = Session::start(quiet_board(), Ok(folders.clone()), access, HOURLY);
    let requests = session.requests();

    requests.sync(plain_gcmon());
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
        intervals: HOURLY,
        incoming,
    };
    drop(messages);

    refreshes.wait_for_stop();
}

#[test]
fn a_session_that_has_ended_ignores_a_sync_and_a_refresh() {
    let (messages, incoming) = mpsc::channel();
    drop(incoming);
    let requests = SessionRequests { messages };

    requests.sync(plain_gcmon());
    requests.refresh();
}

/// Answers every question with a Repository that has nothing new, and says which it was asked.
struct QuietRepository {
    asked: Sender<String>,
}

impl Transport for QuietRepository {
    fn post(&self, _: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        drop(self.asked.send(request.operation.to_owned()));
        let body = match request.operation {
            "Viewer" => r#"{"data":{"viewer":{"login":"octocat"}}}"#.to_owned(),
            _ => format!(
                r#"{{"data":{{"repository":{{
                        "openIssues":{{"totalCount":900}},"openPrs":{{"totalCount":0}},
                        "issues":{{"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[]}},
                        "pullRequests":{{"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[]}}
                    }},{WORK_RATE}}}}}"#
            ),
        };
        Ok(Reply {
            status: 200,
            reset: None,
            body,
        })
    }
}

/// A store that says `gcmon` finished a sync, in `folders`.
fn after_a_finished_sync(folders: &Folders) -> OffsetDateTime {
    let at = OffsetDateTime::from_unix_timestamp(1_000_000_000).unwrap();
    let store = Store::open(&folders.database()).unwrap();
    let state = SyncState {
        started_at: at,
        finished_at: at,
        check: ChangeCheck::Skipped,
        refresh_under_way: false,
    };
    store.save_sync_state(&gcmon(), state).unwrap();
    store.close();
    at
}

fn quiet_repository() -> (GitHubAccess, Receiver<String>) {
    let (asked, seen) = mpsc::channel();
    let access = GitHubAccess {
        transport: Box::new(QuietRepository { asked }),
        run_gh: signed_in_gh,
    };
    (access, seen)
}

/// A board that passes on what it says about last syncs.
fn last_sync_board() -> (StatusBoard, Receiver<LastSync>) {
    let (passed, seen) = mpsc::channel();
    let board = quiet_board().with_last_syncs(Box::new(move |last_sync: &LastSync| {
        passed.send(last_sync.clone()).unwrap();
    }));
    (board, seen)
}

fn wait_for(seen: &Receiver<String>, operation: &str) {
    while seen.recv_timeout(PLENTY).unwrap() != operation {}
}

#[test]
fn opening_a_repository_shows_its_last_sync_and_refreshes_it() {
    let folders = temp_folders("opened");
    let at = after_a_finished_sync(&folders);
    let (access, asked) = quiet_repository();
    let (board, last_syncs) = last_sync_board();
    let session = Session::start(board, Ok(folders.clone()), access, HOURLY);

    session.requests().sync(plain_gcmon());
    let shown = last_syncs.recv_timeout(PLENTY).unwrap();
    wait_for(&asked, "RefreshRepository");
    let after = last_syncs.recv_timeout(PLENTY).unwrap();
    session.stop(PLENTY);
    fs::remove_dir_all(folders.data).unwrap();

    assert_eq!(
        shown,
        LastSync {
            repository: gcmon(),
            at: Some(at),
        }
    );
    assert!(after.at > Some(at));
}

#[test]
fn opening_a_repository_that_never_synced_says_so() {
    let folders = temp_folders("opened-new");
    let (access, _asked) = quiet_repository();
    let (board, last_syncs) = last_sync_board();
    let session = Session::start(board, Ok(folders.clone()), access, HOURLY);

    session.requests().sync(plain_gcmon());
    let shown = last_syncs.recv_timeout(PLENTY).unwrap();
    session.stop(PLENTY);
    fs::remove_dir_all(folders.data).unwrap();

    assert_eq!(
        shown,
        LastSync {
            repository: gcmon(),
            at: None,
        }
    );
}

#[test]
fn a_store_that_cant_say_when_it_last_synced_shows_the_problem() {
    let (mut board, seen) = watched_board();

    show_last_sync(&mut board, &Store::stopped(), gcmon());

    assert!(matches!(showing(&seen), Status::Problem { .. }));
}

#[test]
fn a_click_refreshes_the_open_repository_again() {
    let folders = temp_folders("click");
    let _at = after_a_finished_sync(&folders);
    let (access, asked) = quiet_repository();
    let (board, last_syncs) = last_sync_board();
    let session = Session::start(board, Ok(folders.clone()), access, HOURLY);

    session.requests().sync(plain_gcmon());
    wait_for(&asked, "RefreshRepository");
    // Once when the window opened it, and once when that refresh finished. A click during a refresh
    // adds nothing, because that refresh is reading what the click asks for.
    let _opened = last_syncs.recv_timeout(PLENTY).unwrap();
    let _finished = last_syncs.recv_timeout(PLENTY).unwrap();
    session.requests().refresh();
    wait_for(&asked, "RefreshRepository");
    session.stop(PLENTY);
    fs::remove_dir_all(folders.data).unwrap();
}

#[test]
fn the_timer_refreshes_the_open_repository_again() {
    let folders = temp_folders("timer");
    let _at = after_a_finished_sync(&folders);
    let (access, asked) = quiet_repository();
    let every_moment = Intervals {
        status: AN_HOUR,
        repository: Duration::from_millis(10),
    };
    let session = Session::start(quiet_board(), Ok(folders.clone()), access, every_moment);

    session.requests().sync(plain_gcmon());
    wait_for(&asked, "RefreshRepository");
    wait_for(&asked, "RefreshRepository");
    session.stop(PLENTY);
    fs::remove_dir_all(folders.data).unwrap();
}

#[test]
fn a_session_with_no_token_ignores_a_refresh_until_it_stops() {
    let (messages, incoming) = mpsc::channel();
    let refreshes = Refreshes {
        intervals: HOURLY,
        incoming,
    };
    messages.send(Message::Refresh).unwrap();
    messages.send(Message::Stop).unwrap();

    refreshes.wait_for_stop();
}

#[test]
fn a_reader_answers_once_the_session_has_opened_the_database() {
    let folders = temp_folders("reader");
    let session = Session::start(quiet_board(), Ok(folders.clone()), offline(), HOURLY);

    let facts = session.reader().work_facts(&gcmon());
    session.stop(PLENTY);
    fs::remove_dir_all(folders.data).unwrap();

    assert_eq!(facts.unwrap().pull_requests, []);
}

#[test]
fn a_session_with_no_database_answers_its_readers_with_that() {
    let lost = Error::new(ErrorKind::Internal, "HOME isn't a full path.");
    let session = Session::start(quiet_board(), Err(lost), offline(), HOURLY);

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
    let session = Session::start(board, Ok(folders.clone()), octocat_answers(), HOURLY);
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
    let session = Session::start(quiet_board(), Ok(folders.clone()), access, HOURLY);
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
    let session = Session::start(quiet_board(), Err(lost), offline(), HOURLY);
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
