use super::*;
use crate::github::issue_content::IssueContent;
use crate::github::pull_request_content::Review;
use crate::github::pull_requests::{
    PrHead, PrState, PullRequestFacts, ReviewState, ReviewedCommit,
};
use crate::github::replay::Replayer;
use crate::github::testing::{Canned, Scripted, answered, reply_ok, signed_in_as, signed_in_gh};
use crate::github::{CommitSha, Reply, Request, Transport, Unreachable};
use crate::login::Login;
use crate::repository::{RepositoryName, RepositorySize};
use crate::repository_settings::RepositorySettings;
use crate::settings::ListOrder;
use crate::status::testing::{
    announced, changes_board, failed_reads_board, heads_board, quiet_board, showing, watched_board,
};
use crate::status::{Change, FailedRead, ReadFailure};
use crate::store::{ActivityFacts, ChangeCheck, SyncState};
use crate::token::Token;
use crate::workbench::{ListedItem, Workbench, WorkbenchItem, WorkbenchView};
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
        transport: Box::new(Canned(Err(Unreachable::Offline))),
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
        settings: folder.clone(),
        logs: folder,
    }
}

#[test]
fn a_second_session_on_the_same_folder_says_why_it_stopped() {
    let folders = temp_folders("held");
    let (first_board, first_seen) = watched_board();
    let first = Session::start(first_board, Ok(folders.clone()), offline(), HOURLY);
    // A session says nothing until its database is open.
    let first_status = first_seen.recv().unwrap();
    let (board, seen) = watched_board();

    let second = Session::start(board, Ok(folders.clone()), offline(), HOURLY);
    second.stop(PLENTY);

    first.stop(PLENTY);
    fs::remove_dir_all(folders.data).unwrap();
    assert!(problem_of(first_status).is_none());
    assert_eq!(
        problem_of(showing(&seen)),
        Some(Error::new(ErrorKind::Internal, crate::store::IN_USE))
    );
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
        transport: Box::new(Canned(Err(Unreachable::Offline))),
        run_gh: without_gh,
    };
    assert!(connect(&mut board, &empty_store(), access).is_none());
    assert_eq!(problem_kind(showing(&seen)), Some(ErrorKind::Unauthorized));
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
    signed_in_as("octocat")
}

#[test]
fn a_certificate_outside_its_dates_asks_for_the_clock_until_github_answers() {
    let access = replying(vec![Err(Unreachable::CertificateDates), signed_in()]);
    let (mut board, seen) = watched_board();
    let store = empty_store();
    let github = connect(&mut board, &store, access).unwrap();
    let words =
        "GitHub's certificate isn't valid by this computer's clock. Check the date and time.";
    assert_eq!(
        showing(&seen),
        Status::Problem {
            problem: Error::new(ErrorKind::CertificateDates, words)
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

/// Refreshes whose sender is already gone, so they stop before they begin, for a test that shouldn't get as
/// far as a second request. A session with no token ends on that too.
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
    let access = replying(vec![signed_in(), Err(Unreachable::Offline), signed_in()]);
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

/// The answer to `FetchIssueContent`.
fn issue_content_reply() -> Result<Reply, Unreachable> {
    reply_ok(
        r#"{"data":{"repository":{"issue":{"bodyHTML":"<p>It crashes</p>","createdAt":"2026-11-01T09:00:00Z",
            "comments":{"totalCount":0,"nodes":[]}}},
            "rateLimit":{"remaining":4000,"resetAt":"2999-01-01T00:00:00Z"}}}"#,
    )
}

/// A session that runs `ask` and stops, what the database then holds of Issue 7, and the activity facts of
/// its Repository.
fn after_asking(
    name: &str,
    access: GitHubAccess,
    ask: impl FnOnce(&SessionRequests),
) -> (Option<IssueContent>, ActivityFacts, Vec<Status>) {
    let folders = temp_folders(name);
    let (board, seen) = watched_board();
    let session = Session::start(board, Ok(folders.clone()), access, HOURLY);
    ask(&session.requests());
    session.stop(PLENTY);

    let path = folders.database();
    let store = Store::open(&path).unwrap();
    let (to_open, reader) = store::reader();
    let open = to_open.open(&path).unwrap();
    let (facts, content) = reader.issue_facts(&gcmon(), 7).unwrap();
    open.close();
    store.close();
    fs::remove_dir_all(folders.data).unwrap();
    (content, facts.activity, announced(&seen))
}

#[test]
fn opening_an_issue_keeps_what_it_says_and_leaves_last_seen_alone() {
    let access = replying(vec![signed_in(), issue_content_reply()]);

    let (content, activity, _) =
        after_asking("fetch-issue", access, |asked| asked.fetch_issue(gcmon(), 7));

    assert_eq!(content.unwrap().body_html, "<p>It crashes</p>");
    assert_eq!(activity.last_seen, BTreeMap::new());
}

/// A moment in whole seconds, as the database keeps it.
fn hours_ago(hours: i64) -> OffsetDateTime {
    let at = OffsetDateTime::now_utc() - time::Duration::hours(hours);
    at.replace_nanosecond(0).unwrap()
}

#[test]
fn mark_seen_moves_last_seen_to_the_moment_the_window_drew_and_asks_github_nothing() {
    // `Replies` panics if it is asked a second time.
    let access = replying(vec![signed_in()]);
    let drawn = hours_ago(2);

    let (content, activity, _) = after_asking("mark-seen", access, |asked| {
        asked.mark_unseen(gcmon(), 8);
        asked.mark_seen(
            gcmon(),
            SeenMark {
                numbers: vec![7, 8],
                up_to: Some(drawn),
            },
        );
    });

    assert_eq!(content, None);
    assert_eq!(activity.last_seen, BTreeMap::from([(7, drawn), (8, drawn)]));
    assert_eq!(activity.marked_unseen, BTreeMap::new());
}

#[test]
fn mark_unseen_flags_the_item_from_now() {
    let access = replying(vec![signed_in()]);
    let before = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap();

    let (_, activity, _) =
        after_asking("mark-unseen", access, |asked| asked.mark_unseen(gcmon(), 7));

    let flagged = activity.marked_unseen.get(&7).copied().unwrap();
    assert!(flagged >= before, "{flagged} {before}");
    assert_eq!(activity.last_seen, BTreeMap::new());
}

#[test]
fn opening_a_pr_keeps_its_recorded_conversation_and_leaves_last_seen_alone() {
    let recordings = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github");
    let access = GitHubAccess {
        transport: Box::new(Replayer::new(recordings)),
        run_gh: signed_in_gh,
    };
    let folders = temp_folders("open-pr");
    let (board, changes) = changes_board();
    let session = Session::start(board, Ok(folders.clone()), access, HOURLY);
    let cpython = RepositoryName::parse("python/cpython").unwrap();

    session
        .requests()
        .fetch_pull_request(cpython.clone(), 152_326);
    let _change = changes.recv_timeout(PLENTY).unwrap();
    let (facts, content) = session
        .reader()
        .pull_request_facts(&cpython, 152_326)
        .unwrap();
    session.stop(PLENTY);
    fs::remove_dir_all(folders.data).unwrap();

    let content = content.unwrap();
    let conversation = &content.conversation;
    assert!(
        conversation.body_html.contains("gh-152315"),
        "{}",
        conversation.body_html
    );
    assert_eq!(
        (conversation.comments.len(), conversation.earlier_comments),
        (10, 0)
    );
    let reviewers: Vec<_> = content
        .reviews
        .iter()
        .map(|review| (review.author.clone(), review.state, review.threads.len()))
        .collect();
    let some = |login| Some(Login::new(login));
    assert_eq!(
        reviewers.first(),
        Some(&(some("user-6"), ReviewState::Commented, 5)),
        "the oldest Review comes first, with its threads"
    );
    assert_eq!(
        reviewers.last(),
        Some(&(some("user-7"), ReviewState::Approved, 0))
    );
    assert_eq!((reviewers.len(), content.earlier_reviews), (8, 0));
    assert_eq!(facts.activity.last_seen, BTreeMap::new());
}

#[test]
fn reading_a_pr_on_screen_leaves_last_seen_alone() {
    let reply = reply_ok(
        r#"{"data":{"repository":{"pullRequest":{"bodyHTML":"<p>Fixes the crash</p>","createdAt":"2026-11-01T09:00:00Z",
            "comments":{"totalCount":0,"nodes":[]},"reviews":{"totalCount":0,"nodes":[]},
            "reviewThreads":{"nodes":[]},"headRefOid":"0123abc"}},
            "rateLimit":{"remaining":4000,"resetAt":"2999-01-01T00:00:00Z"}}}"#,
    );
    let access = replying(vec![signed_in(), reply]);

    let (content, activity, _) = after_asking("fetch-pr", access, |asked| {
        asked.fetch_pull_request(gcmon(), 7)
    });

    assert_eq!(content.unwrap().body_html, "<p>Fixes the crash</p>");
    assert_eq!(activity.last_seen, BTreeMap::new());
}

#[test]
fn mark_seen_and_mark_unseen_work_with_no_token() {
    let access = GitHubAccess {
        transport: Box::new(Canned(Err(Unreachable::Offline))),
        run_gh: without_gh,
    };
    let drawn = hours_ago(1);

    let (content, activity, _) = after_asking("no-token-marks", access, |asked| {
        asked.mark_seen(
            gcmon(),
            SeenMark {
                numbers: vec![7],
                up_to: Some(drawn),
            },
        );
        asked.mark_unseen(gcmon(), 8);
    });

    assert_eq!(content, None);
    assert_eq!(activity.last_seen, BTreeMap::from([(7, drawn)]));
    assert!(activity.marked_unseen.contains_key(&8));
}

#[test]
fn marking_an_issue_seen_and_reading_it_each_tell_the_window() {
    let (changes_sent, changes) = mpsc::channel();
    let board = quiet_board().with_changes(Box::new(move |change: &Change| {
        changes_sent.send(change.clone()).unwrap();
    }));
    let folders = temp_folders("open-issue-changes");
    let access = replying(vec![signed_in(), issue_content_reply()]);
    let session = Session::start(board, Ok(folders.clone()), access, HOURLY);

    let requests = session.requests();
    requests.mark_seen(
        gcmon(),
        SeenMark {
            numbers: vec![7, 8],
            up_to: None,
        },
    );
    requests.fetch_issue(gcmon(), 7);
    session.stop(PLENTY);
    fs::remove_dir_all(folders.data).unwrap();

    let change = |items| Change {
        repository: gcmon(),
        items,
    };
    assert_eq!(
        changes.try_iter().collect::<Vec<_>>(),
        [change(vec![7, 8]), change(vec![7])]
    );
}

#[test]
fn an_issue_github_has_lost_troubles_only_the_log() {
    let gone = reply_ok(
        r#"{"data":{"repository":{"issue":null},
            "rateLimit":{"remaining":4000,"resetAt":"2999-01-01T00:00:00Z"}}}"#,
    );
    let access = replying(vec![signed_in(), gone]);

    let (content, _, shown) =
        after_asking("gone-issue", access, |asked| asked.fetch_issue(gcmon(), 7));

    assert_eq!(content, None);
    assert_eq!(shown.last(), Some(&octocat()));
}

#[test]
fn an_answer_about_one_issue_that_cant_be_read_troubles_only_the_log() {
    let access = replying(vec![signed_in(), reply_ok("not json")]);

    let (content, _, shown) = after_asking("garbled-issue", access, |asked| {
        asked.fetch_issue(gcmon(), 7);
    });

    assert_eq!(content, None);
    assert_eq!(shown.last(), Some(&octocat()));
}

fn issue_7() -> Item {
    Item {
        repository: gcmon(),
        number: 7,
    }
}

#[test]
fn a_mark_that_cant_be_stored_is_a_problem() {
    let seen_mark = Marked::Seen {
        repository: gcmon(),
        mark: SeenMark {
            numbers: vec![7],
            up_to: None,
        },
    };
    let unseen = Marked::Unseen {
        item: issue_7(),
        at: OffsetDateTime::UNIX_EPOCH,
    };
    for marked in [seen_mark, unseen] {
        let (mut board, seen) = watched_board();

        mark(&mut board, &Store::stopped(), marked);

        assert_eq!(problem_kind(showing(&seen)), Some(ErrorKind::Internal));
    }
}

#[test]
fn what_an_issue_says_that_cant_be_stored_is_a_problem() {
    let (mut board, seen) = watched_board();
    let access = replying(vec![signed_in(), issue_content_reply()]);
    let github = connect(&mut quiet_board(), &empty_store(), access).unwrap();

    fetch_conversation(
        &mut board,
        &Store::stopped(),
        &github,
        Fetched::Issue(issue_7()),
    )
    .unwrap();

    assert_eq!(problem_kind(showing(&seen)), Some(ErrorKind::Internal));
}

#[test]
fn losing_the_network_while_reading_an_issue_shows_on_the_status_bar() {
    let access = replying(vec![signed_in(), Err(Unreachable::Offline)]);

    let (content, _, shown) = after_asking("offline-issue", access, |asked| {
        asked.fetch_issue(gcmon(), 7)
    });

    assert_eq!(content, None);
    let offline_as = Status::OfflineAs {
        last_login: Login::new("octocat"),
    };
    assert_eq!(shown.last(), Some(&offline_as));
}

/// The reads that failed while a session with `access` ran `ask`.
fn failed_reads(name: &str, access: GitHubAccess, ask: fn(&SessionRequests)) -> Vec<FailedRead> {
    let folders = temp_folders(name);
    let (board, failed) = failed_reads_board();
    let session = Session::start(board, Ok(folders.clone()), access, HOURLY);
    ask(&session.requests());
    session.stop(PLENTY);
    fs::remove_dir_all(folders.data).unwrap();
    failed.try_iter().collect()
}

/// The reads that failed while a session with `access` read PR 7 of gcmon.
fn failed_reads_of_pr_7(name: &str, access: GitHubAccess) -> Vec<FailedRead> {
    failed_reads(name, access, |asked| asked.fetch_pull_request(gcmon(), 7))
}

/// The reads that failed while a session with `access` read Issue 7 of gcmon.
fn failed_reads_of_issue_7(name: &str, access: GitHubAccess) -> Vec<FailedRead> {
    failed_reads(name, access, |asked| asked.fetch_issue(gcmon(), 7))
}

#[test]
fn an_issue_read_without_the_network_waits_on_github() {
    let access = replying(vec![signed_in(), Err(Unreachable::Offline)]);

    let failed = failed_reads_of_issue_7("offline-issue-read", access);

    assert_eq!(failed, [item_7_failed(ReadFailure::WaitsOnGitHub)]);
}

#[test]
fn an_issue_github_doesnt_have_says_so() {
    let gone = reply_ok(
        r#"{"data":{"repository":{"issue":null},
            "rateLimit":{"remaining":4000,"resetAt":"2999-01-01T00:00:00Z"}}}"#,
    );
    let access = replying(vec![signed_in(), gone]);

    let failed = failed_reads_of_issue_7("gone-issue", access);

    assert_eq!(failed, [item_7_failed(ReadFailure::NotOnGitHub)]);
}

/// Item 7 of gcmon, a PR or an Issue, whose read failed for `why`.
fn item_7_failed(why: ReadFailure) -> FailedRead {
    FailedRead {
        repository: gcmon(),
        number: 7,
        why,
    }
}

#[test]
fn a_pr_read_without_the_network_waits_on_github() {
    let access = replying(vec![signed_in(), Err(Unreachable::Offline)]);

    let failed = failed_reads_of_pr_7("offline-pr", access);

    assert_eq!(failed, [item_7_failed(ReadFailure::WaitsOnGitHub)]);
}

#[test]
fn a_pr_read_with_no_token_says_so() {
    let access = GitHubAccess {
        transport: Box::new(Canned(Err(Unreachable::Offline))),
        run_gh: without_gh,
    };

    let failed = failed_reads_of_pr_7("no-token-pr", access);

    assert_eq!(failed, [item_7_failed(ReadFailure::NoToken)]);
}

#[test]
fn a_pr_github_doesnt_have_says_so() {
    let gone = reply_ok(
        r#"{"data":{"repository":{"pullRequest":null},
            "rateLimit":{"remaining":4000,"resetAt":"2999-01-01T00:00:00Z"}}}"#,
    );
    let access = replying(vec![signed_in(), gone]);

    let failed = failed_reads_of_pr_7("gone-pr", access);

    assert_eq!(failed, [item_7_failed(ReadFailure::NotOnGitHub)]);
}

#[test]
fn a_pr_whose_answer_cant_be_read_says_so() {
    let access = replying(vec![signed_in(), reply_ok("not json")]);

    let failed = failed_reads_of_pr_7("garbled-pr", access);

    assert_eq!(failed, [item_7_failed(ReadFailure::Other)]);
}

#[test]
fn a_pr_read_that_lands_fails_nothing() {
    let reply = reply_ok(
        r#"{"data":{"repository":{"pullRequest":{"bodyHTML":"<p>Fixes the crash</p>","createdAt":"2026-11-01T09:00:00Z",
            "comments":{"totalCount":0,"nodes":[]},"reviews":{"totalCount":0,"nodes":[]},
            "reviewThreads":{"nodes":[]},"headRefOid":"0123abc"}},
            "rateLimit":{"remaining":4000,"resetAt":"2999-01-01T00:00:00Z"}}}"#,
    );
    let access = replying(vec![signed_in(), reply]);

    let failed = failed_reads_of_pr_7("landed-pr", access);

    assert_eq!(failed, []);
}

const AN_HOUR: Duration = Duration::from_secs(3600);
const HOURLY: Intervals = Intervals {
    status: AN_HOUR,
    repository: AN_HOUR,
};
const PLENTY: Duration = Duration::from_secs(10);

const GCMON: &str = r#"{"data":{"repository":{"nameWithOwner":"some-maintainer/gcmon",
        "issues":{"totalCount":10},"pullRequests":{"totalCount":3}},
        "updated":{"issueCount":43},"closed":{"issueCount":35}}}"#;

fn gcmon() -> RepositoryName {
    RepositoryName::parse("some-maintainer/gcmon").unwrap()
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
        date: None,
        body: GCMON.to_owned(),
    };
    let access = replying(vec![signed_in(), Ok(gcmon_reply)]);
    let folders = temp_folders("check");
    let session = Session::start(quiet_board(), Ok(folders.clone()), access, HOURLY);

    let check = session.requests().check(gcmon()).unwrap();
    session.stop(PLENTY);
    fs::remove_dir_all(folders.data).unwrap();

    assert_eq!(check.repository.name, gcmon());
    assert_eq!((check.open_issues, check.open_prs), (10, 3));
    assert_eq!(check.size, RepositorySize::Small);
    // Its 13 open items and the 35 that closed in the window.
    assert_eq!(check.first_sync_items, 48);
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
            "nameWithOwner": "some-maintainer/gcmon",
            "defaultBranchRef": { "name": "main" },
        },
        "closingIssuesReferences": { "nodes": [] },
        "author": { "login": "octocat" },
        "assignees": { "nodes": [] },
        "labels": { "nodes": [] },
        "reviewRequests": { "nodes": [] },
        "timelineItems": { "nodes": [] },
        "latestReviews": { "nodes": [] },
        "lastCommit": { "nodes": [] },
        "mergeCommit": null,
        "baseRefOid": "0123456789abcdef0123456789abcdef01234567",
        "headRefOid": "89abcdef0123456789abcdef0123456789abcdef",
        "commits": { "totalCount": 0, "nodes": [] },
    });
    reply_ok(&format!(r#"{{"data":{{"nodes":[{node}],{WORK_RATE}}}}}"#))
}

/// The recorded `python/cpython` work as the Workbench lists it: the sections in order, with each item's
/// reasons.
fn inbox_of(workbench: &Workbench) -> String {
    let line = |listed: &ListedItem| {
        let number = listed.item.updated_at_and_number().1;
        let why = listed.attention.as_ref().map(|attention| &attention.why);
        format!("#{number} {why:?}")
    };
    let waiting = workbench.items.iter().map(line).collect::<Vec<_>>();
    let quiet = workbench.quiet.iter().map(line).collect::<Vec<_>>();
    format!(
        "count {}, review requests {}\n\n{}\n\nquiet\n{}\n",
        workbench.count,
        workbench.review_requests,
        waiting.join("\n"),
        quiet.join("\n"),
    )
}

#[test]
fn the_recorded_cpython_work_reads_as_an_inbox() {
    let recordings = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github");
    let access = GitHubAccess {
        transport: Box::new(Replayer::new(recordings)),
        run_gh: signed_in_gh,
    };
    let folders = temp_folders("recorded-inbox");
    let (board, changes) = changes_board();
    let session = Session::start(board, Ok(folders.clone()), access, HOURLY);
    let reader = session.reader();
    let cpython = RepositorySettings::new(RepositoryName::parse("python/cpython").unwrap());

    session.requests().sync(cpython.clone());
    // Two Filters, the PRs they found, their Issues, one Issue's tree and an Origin PR.
    for _ in 0..6 {
        let _change = changes.recv_timeout(PLENTY).unwrap();
    }
    let workbench = Workbench::read(
        &reader,
        &cpython,
        &WorkbenchView::Whole,
        ListOrder::Attention,
    )
    .unwrap();
    session.stop(PLENTY);
    fs::remove_dir_all(folders.data).unwrap();

    insta::assert_snapshot!(inbox_of(&workbench));
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
    let workbench = Workbench::read(
        &reader,
        &plain_gcmon(),
        &WorkbenchView::Whole,
        ListOrder::Attention,
    )
    .unwrap();
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
        matches!(&workbench.items[..], [ListedItem { item: WorkbenchItem::StandalonePr(tree), .. }] if tree.row.number == 7),
        "{workbench:?}"
    );
}

/// A PR of `author` in `gcmon`, stored with its branch at `head`.
fn stored_pr(number: u32, author: &str, head: &str) -> PullRequestFacts {
    PullRequestFacts {
        number,
        title: format!("gh-{number}: Fix it"),
        state: PrState::Open,
        is_draft: false,
        author: Some(Login::new(author)),
        target_branch: "main".to_owned(),
        ci: None,
        updated_at: OffsetDateTime::from_unix_timestamp(1).unwrap(),
        review_requests: vec![],
        latest_reviews: vec![],
        assignees: vec![],
        labels: vec![],
        closing_issues: vec![],
        merge_commit: None,
        base: None,
        head: CommitSha::parse(head),
        off_default: None,
    }
}

#[test]
fn a_sync_hands_the_mirror_the_heads_it_saved_of_the_prs_the_users_work_reaches_and_the_reviewed_commits()
 {
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
    let folders = temp_folders("reached-heads");
    let store = Store::open(&folders.database()).unwrap();
    // The user's own PR, which the sync finds moved on, and a PR of someone else's.
    let stored = vec![
        stored_pr(7, "octocat", "0123abc"),
        stored_pr(8, "some-author", "4567def"),
    ];
    store.save_pull_requests(&gcmon(), stored).unwrap();
    // The user reviewed someone else's PR at a commit it has since moved on from.
    let reviewed = CommitSha::parse("0123abc").unwrap();
    store
        .save_pull_request_content(&gcmon(), 8, reviewed_by("octocat", &reviewed))
        .unwrap();
    store.close();
    let (board, heads) = heads_board();
    let session = Session::start(board, Ok(folders.clone()), access, HOURLY);

    session.requests().sync(plain_gcmon());
    let handed = heads.recv_timeout(PLENTY).unwrap();
    session.stop(PLENTY);
    fs::remove_dir_all(folders.data).unwrap();

    let moved = CommitSha::parse("89abcdef0123456789abcdef0123456789abcdef").unwrap();
    assert_eq!(
        handed,
        (
            gcmon(),
            vec![PrHead {
                number: 7,
                commit: moved,
                target_branch: "main".to_owned(),
            }],
            vec![ReviewedCommit {
                number: 8,
                commit: reviewed,
            }]
        )
    );
}

/// A PR's Conversation with one Review by `author`, of `commit`.
fn reviewed_by(author: &str, commit: &CommitSha) -> PullRequestContent {
    PullRequestContent {
        conversation: IssueContent {
            body_html: String::new(),
            comments: vec![],
            earlier_comments: 0,
            opened_at: None,
        },
        reviews: vec![Review {
            author: Some(Login::new(author)),
            state: ReviewState::Approved,
            commit: Some(commit.clone()),
            submitted_at: time::OffsetDateTime::UNIX_EPOCH,
            body_html: String::new(),
            hidden: None,
            threads: vec![],
        }],
        earlier_reviews: 0,
        head: Some(commit.clone()),
        pending: vec![],
    }
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
        Err(Unreachable::Offline),
        Err(Unreachable::Offline),
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

#[test]
fn a_sync_waits_until_tsugiki_knows_who_the_user_is() {
    let (access, up, _asked) = outage();
    // GitHub can't be reached for the first question about the viewer, and answers once the status says so.
    up.store(false, Ordering::Relaxed);
    let back = move |_| up.store(true, Ordering::Relaxed);
    let (board, heard) = heard_board(back, |_| {});
    let folders = temp_folders("late-viewer");
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
    while !matches!(heard.recv().unwrap(), Heard::Changed(_)) {}
    let workbench = Workbench::read(
        &reader,
        &plain_gcmon(),
        &WorkbenchView::Whole,
        ListOrder::Attention,
    )
    .unwrap();
    session.stop(PLENTY);
    fs::remove_dir_all(folders.data).unwrap();

    assert_eq!(workbench.items.len(), 1);
}

/// Answers each question by name while the network is up, and can't reach GitHub while it is down. Says
/// which it was asked.
struct Outage {
    up: Arc<AtomicBool>,
    asked: Sender<String>,
}

impl Transport for Outage {
    fn post(&self, _: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        drop(self.asked.send(request.operation.to_owned()));
        if !self.up.load(Ordering::Relaxed) {
            return Err(Unreachable::Offline);
        }
        match request.operation {
            "Viewer" => signed_in(),
            "FetchPullRequests" => work_fetch(),
            "FetchIssueContent" => issue_content_reply(),
            _ => work_search(&["a"]),
        }
    }
}

/// An outage that starts with the network up, the switch that takes it down, and what it was asked.
fn outage() -> (GitHubAccess, Arc<AtomicBool>, Receiver<String>) {
    let up = Arc::new(AtomicBool::new(true));
    let (asked, seen) = mpsc::channel();
    let access = GitHubAccess {
        transport: Box::new(Outage {
            up: Arc::clone(&up),
            asked,
        }),
        run_gh: signed_in_gh,
    };
    (access, up, seen)
}

/// The news a board passed on to the app and the mirrors, in order.
#[derive(Debug, PartialEq, Eq)]
enum Heard {
    Status(Status),
    Heads(Vec<u32>),
    Changed(Vec<u32>),
}

/// A board that passes on its statuses, the PR heads it hands the mirrors and the changes, in one line.
/// It runs `on_status` with how many statuses it has shown, and `on_change` with how many changes.
fn heard_board(
    mut on_status: impl FnMut(usize) + Send + 'static,
    mut on_change: impl FnMut(usize) + Send + 'static,
) -> (StatusBoard, Receiver<Heard>) {
    let (heard, seen) = mpsc::channel();
    let (heads, changes) = (heard.clone(), heard.clone());
    let (mut shown, mut changed) = (0, 0);
    let board = StatusBoard::new(Box::new(move |status: &Status| {
        heard.send(Heard::Status(status.clone())).unwrap();
        shown += 1;
        on_status(shown);
    }))
    .with_heads(Box::new(
        move |_: &RepositoryName, handed: Vec<PrHead>, _| {
            let numbers = handed.iter().map(|head| head.number).collect();
            heads.send(Heard::Heads(numbers)).unwrap();
        },
    ))
    .with_changes(Box::new(move |change: &Change| {
        changes.send(Heard::Changed(change.items.clone())).unwrap();
        changed += 1;
        on_change(changed);
    }));
    (board, seen)
}

#[test]
fn a_refresh_while_offline_comes_back_online_before_the_next_status_check() {
    let (access, up, _asked) = outage();
    let (messages, incoming) = mpsc::channel();
    let requests = SessionRequests {
        messages: messages.clone(),
    };
    requests.sync(plain_gcmon());
    let clicks = requests.clone();
    // Online, the network goes; offline, it comes back and the user clicks Refresh.
    let on_status = move |shown| match shown {
        1 => up.store(false, Ordering::Relaxed),
        2 => {
            up.store(true, Ordering::Relaxed);
            clicks.refresh();
        }
        _ => {}
    };
    // Once the sync saves, the open Issue's read comes round.
    let on_change = move |changed| {
        if changed == 1 {
            requests.fetch_issue(gcmon(), 9);
            messages.send(Message::Stop).unwrap();
        }
    };
    let (mut board, heard) = heard_board(on_status, on_change);
    let refreshes = Refreshes {
        intervals: HOURLY,
        incoming,
    };

    watch(&mut board, &empty_store(), access, &refreshes);

    let offline_as = Status::OfflineAs {
        last_login: Login::new("octocat"),
    };
    assert_eq!(
        heard.try_iter().collect::<Vec<_>>(),
        [
            Heard::Status(octocat()),
            Heard::Status(offline_as),
            Heard::Status(octocat()),
            Heard::Heads(vec![7]),
            Heard::Changed(vec![7]),
            Heard::Changed(vec![9]),
        ]
    );
}

#[test]
fn a_refresh_while_github_is_still_away_stays_offline_and_runs_no_step() {
    let (access, up, asked) = outage();
    let (messages, incoming) = mpsc::channel();
    let requests = SessionRequests {
        messages: messages.clone(),
    };
    requests.sync(plain_gcmon());
    let on_status = move |shown| match shown {
        1 => up.store(false, Ordering::Relaxed),
        2 => {
            requests.refresh();
            messages.send(Message::Stop).unwrap();
        }
        _ => {}
    };
    let (mut board, heard) = heard_board(on_status, |_| {});
    let refreshes = Refreshes {
        intervals: HOURLY,
        incoming,
    };

    watch(&mut board, &empty_store(), access, &refreshes);

    let offline_as = Status::OfflineAs {
        last_login: Login::new("octocat"),
    };
    assert_eq!(
        heard.try_iter().collect::<Vec<_>>(),
        [Heard::Status(octocat()), Heard::Status(offline_as)]
    );
    let asked: Vec<String> = asked.try_iter().collect();
    // The step asks again for half as much after no answer, and the click asks only who is signed in.
    let viewers = asked
        .iter()
        .filter(|operation| *operation == "Viewer")
        .count();
    assert_eq!(
        (viewers, asked.last().map(String::as_str)),
        (2, Some("Viewer")),
        "{asked:?}"
    );
}

#[test]
fn with_no_token_what_is_stored_stays_readable() {
    let access = GitHubAccess {
        transport: Box::new(Canned(Err(Unreachable::Offline))),
        run_gh: without_gh,
    };
    let folders = temp_folders("no-token-readable");
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
            date: None,
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

    refreshes.wait_for_stop(&mut quiet_board(), &empty_store());
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
        transport: Box::new(Canned(Err(Unreachable::Offline))),
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
#[expect(
    clippy::disallowed_types,
    reason = "a database from a newer Tsugiki has a version the store can't write, so the test sets it"
)]
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
