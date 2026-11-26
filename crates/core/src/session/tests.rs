use super::github_thread::*;
use super::*;
use crate::conventions::Conventions;
use crate::deadline::received_until;
use crate::facts::CommitSha;
use crate::facts::activity::{ActivityFacts, ItemKey, ShowMuted};
use crate::facts::filters::FilterResults;
use crate::facts::issue_content::IssueContent;
use crate::facts::issues::{IssueFacts, IssueState};
use crate::facts::patch_ids::{CantCheck, CommitPatches, FilePatch, PatchId, PatchesOf};
use crate::facts::patch_ids::{LastCheck, ReviewFacts, UncheckedCommit};
use crate::facts::pull_request_content::{PullRequestContent, Review};
use crate::facts::pull_requests::{PullRequestFacts, ReviewState};
use crate::fetching::testing::QUEUED;
use crate::fetching::{Fetched, NoHead, ScreenMove};
use crate::filters::{Filter, FilterQuery, OnWorkbench};
use crate::github::GitHub;
use crate::github::points::HourPoints;
use crate::github::replay::Replayer;
use crate::github::testing::{
    Canned, RATE_LIMIT, SIGNED_IN, Scripted, answer, answered, dated, fetched_nodes, github,
    issue_item, item_answer, item_missing, page_info, pr_content, pr_item, pr_node,
    pull_request_answer, reply_ok, search_page, signed_in_as,
};
use crate::github::{Reply, Request, Transport, Unreachable};
use crate::login::Login;
use crate::mirror::HeardRequests;
use crate::repository::{RepositoryName, RepositorySize};
use crate::repository_settings::{RepositorySettings, SavedBy};
use crate::settings::{ListOrder, SettingsFiles, SettingsOwner};
use crate::status::testing::{
    announced, changes_board, failed_reads_board, quiet_board, showing, watched_board,
};
use crate::status::{Account, Change, FailedRead, ItemUnread, LastSync, ReadFailure, Status};
use crate::store::testing::{OpenStore, kept_fixes, store_with_old_links};
use crate::store::{ChangeCheck, SyncState};
use crate::sync::Syncs;
use crate::testing::{
    TempPath, cpython, cpython_settings, empty_conversation, gcmon, hours_ago, issue_facts, listed,
    listing, me, one_folder, pr_facts, recordings,
};
use crate::token::Token;
use crate::work::{Hiding, Unseen};
use crate::workbench::inbox::InboxPage;
use crate::workbench::{
    Answered, Asked, ListedItem, Listing, Page, Workbench, WorkbenchItem, WorkbenchView,
};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvError, Sender, TryRecvError};
use std::time::Instant;
use time::OffsetDateTime;
use tsugiki_test_git::{pass_on, wait_for_message};
use tsugiki_token::testing::NO_GH;

mod held;
mod in_flight;
mod no_token;
mod rate_limit;

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
        token_from: SIGNED_IN,
    }
}

fn answering(status: u16, body: &str) -> GitHubAccess {
    GitHubAccess {
        transport: Box::new(answered(status, body)),
        token_from: SIGNED_IN,
    }
}

fn octocat_answers() -> GitHubAccess {
    answering(200, r#"{"data":{"viewer":{"login":"octocat"}}}"#)
}

fn replying(replies: Vec<Result<Reply, Unreachable>>) -> GitHubAccess {
    GitHubAccess {
        transport: Box::new(Replies(RefCell::new(replies.into()))),
        token_from: SIGNED_IN,
    }
}

fn octocat() -> Status {
    Status::from(Account::Online {
        login: Login::new("octocat"),
    })
}

/// Connects through `access` at the GitHub thread's start, with nothing else asking it, and gives the client
/// if it found a token.
fn connect_alone(board: &mut StatusBoard, store: &Store, access: GitHubAccess) -> Option<GitHub> {
    let (refreshes, _done) = Refreshes::new(HOURLY, mpsc::channel().1, unheard(), listed(vec![]));
    connect(board, store, access, &refreshes).ok()
}

/// Folders that are all one temp folder, and that folder. Stop the session before the folder drops.
fn temp_folders(name: &str) -> (Folders, TempPath) {
    let folder = crate::testing::temp_path(&format!("session-{name}"));
    fs::create_dir_all(&folder).unwrap();
    (one_folder(&folder), folder)
}

#[test]
fn a_second_session_on_the_same_folder_says_why_it_stopped() {
    let (folders, _folder) = temp_folders("held");
    let (first_board, first_seen) = watched_board();
    let first = Session::start(
        first_board,
        Ok(folders.clone()),
        reaching(offline()),
        HOURLY,
    );
    // A session says nothing until its database is open.
    let first_status = first_seen.recv().unwrap();
    let (board, seen) = watched_board();

    let second = Session::start(board, Ok(folders.clone()), reaching(offline()), HOURLY);
    second.stop(PLENTY);

    first.stop(PLENTY);
    assert!(problem_of(first_status).is_none());
    assert_eq!(
        stopped_by(showing(&seen)).as_deref(),
        Some(crate::store::IN_USE)
    );
}

#[test]
fn signing_in_remembers_the_login() {
    let store = store_remembering("hubot");
    assert!(connect_alone(&mut quiet_board(), &store, octocat_answers()).is_some());
    assert_eq!(store.last_login().unwrap(), Some(Login::new("octocat")));
}

#[test]
fn the_last_login_shows_before_github_answers() {
    let (mut board, seen) = watched_board();
    let store = store_remembering("hubot");
    assert!(connect_alone(&mut board, &store, octocat_answers()).is_some());
    assert_eq!(
        announced(&seen),
        [
            Status::from(Account::ConnectingAs {
                last_login: Login::new("hubot")
            }),
            octocat()
        ]
    );
}

#[test]
fn nobody_is_shown_before_the_first_answer() {
    let (mut board, seen) = watched_board();
    assert!(connect_alone(&mut board, &empty_store(), octocat_answers()).is_some());
    assert_eq!(announced(&seen), [octocat()]);
}

#[test]
fn without_the_network_or_a_last_login_the_status_is_offline() {
    let (mut board, seen) = watched_board();
    assert!(connect_alone(&mut board, &empty_store(), offline()).is_some());
    assert_eq!(showing(&seen), Account::Offline.into());
}

#[test]
fn without_the_network_the_last_login_still_shows() {
    let (mut board, seen) = watched_board();
    let store = store_remembering("hubot");
    assert!(connect_alone(&mut board, &store, offline()).is_some());
    assert_eq!(
        showing(&seen),
        Account::OfflineAs {
            last_login: Login::new("hubot")
        }
        .into()
    );
}

fn problem_of(status: Status) -> Option<Error> {
    status.problem
}

fn problem_kind(status: Status) -> Option<ErrorKind> {
    problem_of(status).map(|problem| problem.kind)
}

/// Whether `status` says a read of the token found none, with no problem beside it.
fn says_no_token(status: &Status) -> bool {
    matches!(
        status,
        Status {
            account: Account::NoToken { .. },
            problem: None,
        }
    )
}

#[test]
fn a_refused_token_is_the_accounts_and_keeps_the_last_login_stored() {
    let (mut board, seen) = watched_board();
    let store = store_remembering("hubot");
    assert!(connect_alone(&mut board, &store, answering(401, "")).is_some());
    assert!(matches!(
        showing(&seen),
        Status {
            account: Account::Refused { .. },
            problem: None,
        }
    ));
    assert_eq!(store.last_login().unwrap(), Some(Login::new("hubot")));
}

#[test]
fn without_gh_nothing_is_asked_and_the_problem_shows() {
    let (mut board, seen) = watched_board();
    let access = GitHubAccess {
        transport: Box::new(Canned(Err(Unreachable::Offline))),
        token_from: NO_GH,
    };
    assert!(connect_alone(&mut board, &empty_store(), access).is_none());
    assert!(says_no_token(&showing(&seen)));
}

#[test]
fn a_last_login_that_cant_be_read_doesnt_stop_tsugiki_connecting() {
    let (mut board, seen) = watched_board();
    assert!(connect_alone(&mut board, &Store::stopped(), octocat_answers()).is_some());
    assert_eq!(problem_kind(showing(&seen)), Some(ErrorKind::Internal));
}

#[test]
fn a_login_that_cant_be_stored_is_a_problem_beside_the_login() {
    let (mut board, seen) = watched_board();
    let github = connect_alone(&mut quiet_board(), &empty_store(), octocat_answers()).unwrap();
    refresh(&mut board, &Store::stopped(), &github);
    let shown = showing(&seen);
    assert_eq!(shown.account, octocat().account);
    assert_eq!(problem_kind(shown), Some(ErrorKind::Internal));
}

#[test]
fn a_last_login_that_cant_be_read_offline_is_a_problem() {
    let (mut board, seen) = watched_board();
    let github = connect_alone(&mut quiet_board(), &empty_store(), offline()).unwrap();
    refresh(&mut board, &Store::stopped(), &github);
    let shown = showing(&seen);
    assert_eq!(shown.account, Account::Offline);
    assert_eq!(problem_kind(shown), Some(ErrorKind::Internal));
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
    let github = connect_alone(&mut board, &store, access).unwrap();
    let words =
        "GitHub's certificate isn't valid by this computer's clock. Check the date and time.";
    assert_eq!(
        showing(&seen),
        Status {
            account: Account::Connecting,
            problem: Some(Error::new(ErrorKind::CertificateDates, words)),
        }
    );

    refresh(&mut board, &store, &github);
    assert_eq!(showing(&seen), octocat());
}

#[test]
fn a_status_check_github_answers_without_a_login_shows_the_last_login_alone() {
    let (mut board, seen) = watched_board();
    let store = store_remembering("hubot");

    assert!(connect_alone(&mut board, &store, answering(200, "{}")).is_some());

    let shown = showing(&seen);
    let hubot = Account::UnconfirmedAs {
        last_login: Login::new("hubot"),
    };
    assert_eq!(shown.account, hubot);
    assert_eq!(problem_kind(shown), Some(ErrorKind::Internal));
}

/// What the status bar says after a job's request failed with `kind`, from `account` with `earlier` beside
/// it, when octocat signed in last time.
fn after_a_job_failed(account: Account, earlier: Option<Error>, kind: ErrorKind) -> Status {
    after_a_job_failed_with(&store_remembering("octocat"), account, earlier, kind)
}

/// What the status bar says after a job's request failed with `kind`, from `account` with `earlier` beside
/// it, with what `store` holds.
fn after_a_job_failed_with(
    store: &Store,
    account: Account,
    earlier: Option<Error>,
    kind: ErrorKind,
) -> Status {
    let mut board = quiet_board();
    board.set(Status {
        account,
        problem: earlier,
    });
    tell_problem(&mut board, Error::new(kind, "It failed."), store);
    board.status().clone()
}

/// Each account, octocat's where it names a login, beside what it becomes after a failure GitHub answered
/// and after a certificate this computer's clock doesn't accept, when octocat signed in last time. An account
/// holds a login or a token message, which a test mustn't log, so the tests that read it print none.
fn accounts_after_failures() -> Vec<(Account, Account, Account)> {
    let octocat = || Login::new("octocat");
    let unconfirmed_as = || Account::UnconfirmedAs {
        last_login: octocat(),
    };
    let no_token = Account::NoToken {
        message: "Install gh.".to_owned(),
    };
    let refused = Account::Refused {
        message: "GitHub refused the token.".to_owned(),
    };
    let offline_as = Account::OfflineAs {
        last_login: octocat(),
    };
    let stays = |account: Account| (account.clone(), unconfirmed_as(), account);
    let kept = |account: Account| (account.clone(), account.clone(), account);
    vec![
        stays(Account::Connecting),
        stays(Account::ConnectingAs {
            last_login: octocat(),
        }),
        kept(Account::Online { login: octocat() }),
        stays(Account::Offline),
        stays(offline_as.clone()),
        // A click on Refresh asked again, and a certificate says nothing of the account, so it is
        // offline again, and the next click asks again too.
        (Account::Reconnecting, unconfirmed_as(), Account::Offline),
        (
            Account::ReconnectingAs {
                last_login: octocat(),
            },
            unconfirmed_as(),
            offline_as,
        ),
        stays(no_token),
        stays(refused),
        kept(Account::Unconfirmed),
        kept(Account::UnconfirmedAs {
            last_login: Login::new("hubot"),
        }),
        kept(Account::Stopped),
    ]
}

#[test]
fn a_failure_github_answered_leaves_the_last_login_unconfirmed_and_no_longer_offline() {
    let answered = [
        ErrorKind::QueryRefused,
        ErrorKind::WriteRefused,
        ErrorKind::NotFound,
        ErrorKind::Conflict,
        ErrorKind::Internal,
    ];
    for kind in answered {
        for (account, expected, _) in accounts_after_failures() {
            let after = after_a_job_failed(account, None, kind);

            assert!(after.account == expected);
            assert!(!after.account.is_offline());
            assert_eq!(problem_kind(after), Some(kind));
        }
    }
}

#[test]
fn a_failure_github_answered_with_no_last_login_leaves_nobody_unconfirmed() {
    for account in [Account::Connecting, Account::Offline, Account::Reconnecting] {
        let after = after_a_job_failed_with(&empty_store(), account, None, ErrorKind::Internal);

        assert_eq!(after.account, Account::Unconfirmed);
    }
}

#[test]
fn a_certificate_the_clock_doesnt_accept_leaves_the_account_as_it_was_before_any_click() {
    for (account, _, expected) in accounts_after_failures() {
        let after = after_a_job_failed(account, None, ErrorKind::CertificateDates);

        assert!(after.account == expected);
        assert_eq!(problem_kind(after), Some(ErrorKind::CertificateDates));
    }
}

#[test]
fn a_refresh_offline_whose_check_fails_on_the_certificate_is_offline_again_and_asks_again() {
    let (threads, requests) = two_threads(HOURLY, mirrors());
    let on_status = move |shown| match shown {
        1 => requests.refresh(),
        3 => {
            requests.refresh();
            requests.stop_session();
        }
        _ => {}
    };
    let (mut board, heard) = heard_board(on_status, |_| {});
    let access = replying(vec![
        Err(Unreachable::Offline),
        Err(Unreachable::CertificateDates),
        Err(Unreachable::Offline),
    ]);

    threads.watch(&mut board, &empty_store(), access);

    let words =
        "GitHub's certificate isn't valid by this computer's clock. Check the date and time.";
    let certificate = Status {
        account: Account::Offline,
        problem: Some(Error::new(ErrorKind::CertificateDates, words)),
    };
    assert_eq!(
        heard.try_iter().collect::<Vec<_>>(),
        [
            Heard::Status(Account::Offline.into()),
            Heard::Status(Account::Reconnecting.into()),
            Heard::Status(certificate.clone()),
            Heard::Status(Status {
                account: Account::Reconnecting,
                ..certificate
            }),
            Heard::Status(Account::Offline.into()),
        ]
    );
}

#[test]
fn a_job_failure_of_the_accounts_keeps_the_earlier_problem_and_any_other_replaces_it() {
    let earlier = || Some(database_problem());

    let offline = after_a_job_failed(octocat().account, earlier(), ErrorKind::Offline);
    let refused_now = after_a_job_failed(octocat().account, earlier(), ErrorKind::Unauthorized);
    let no_token = after_a_job_failed(octocat().account, earlier(), ErrorKind::NoToken);
    let other = after_a_job_failed(octocat().account, earlier(), ErrorKind::Internal);

    let offline_as = Account::OfflineAs {
        last_login: Login::new("octocat"),
    };
    assert_eq!((offline.account, offline.problem), (offline_as, earlier()));
    assert!(matches!(refused_now.account, Account::Refused { .. }));
    assert_eq!(refused_now.problem, earlier());
    let says_no_token = Account::NoToken {
        message: "It failed.".to_owned(),
    };
    assert_eq!(
        (no_token.account, no_token.problem),
        (says_no_token, earlier())
    );
    assert_eq!(
        other.problem,
        Some(Error::new(ErrorKind::Internal, "It failed."))
    );
}

/// The `Database` for `run`, with no reader waiting for it.
fn database(folders: Result<Folders, Error>) -> Database {
    let (to_open, _reader) = store::reader();
    Database { folders, to_open }
}

/// A read-only connection no database answers, for a session part that never reads a list back (#1052).
fn unopened_reader() -> StoreReader {
    store::reader().1
}

/// Runs a session on `database` whose thread has nothing to take, so its GitHub thread connects through
/// `access` and stops, for a test that shouldn't get as far as a second request. A GitHub thread with no
/// token ends on that too.
fn run_stopped(mut board: StatusBoard, database: Database, access: GitHubAccess) {
    let (to_github, queue) = mpsc::channel();
    let intervals = Intervals {
        status: Duration::ZERO,
        ..HOURLY
    };
    let (refreshes, done) =
        Refreshes::new(intervals, queue, unheard(), listed(vec![plain_gcmon()]));
    let telling = telling_through(&mut board, to_github, done);
    let keeper = Keeper::new(
        mpsc::channel().1,
        unheard(),
        Reads {
            settings: listed(vec![plain_gcmon()]),
            reader: unopened_reader(),
        },
        telling,
    );
    let threads = Threads {
        board,
        access,
        refreshes,
    };
    run(database, threads, &keeper);
}

/// Requests for a Mirror worker that isn't there, which go nowhere.
fn mirrors() -> MirrorRequests {
    HeardRequests::new().0
}

/// GitHub through `access`, a Mirror worker that isn't there, and settings that list gcmon with no Filters.
fn reaching(access: GitHubAccess) -> Connections {
    Connections::new(access, mirrors(), listed(vec![plain_gcmon()]))
}

/// A handle that reaches neither a session nor a Mirror worker, for refreshes a test drives itself.
fn unheard() -> Fetching {
    sending(mpsc::channel().0, mirrors())
}

/// A handle that sends the session's messages through `messages` and the Mirror worker's to `mirrors`, and
/// hears of no PR whose Conversation named no head.
fn sending(messages: Sender<SessionMessage>, mirrors: MirrorRequests) -> Fetching {
    Fetching::new(messages, mirrors, Headless::default())
}

/// The GitHub thread's refreshes, whose queue is empty and whose handle reaches nothing, for the fields a
/// test doesn't set.
fn unheard_refreshes() -> Refreshes {
    Refreshes::new(
        HOURLY,
        mpsc::channel().1,
        unheard(),
        listed(vec![plain_gcmon()]),
    )
    .0
}

#[test]
fn a_query_typed_after_a_search_makes_the_older_answers_out_of_date() {
    let (to_github, incoming) = mpsc::channel();
    let (refreshes, _done) =
        Refreshes::new(HOURLY, incoming, unheard(), listed(vec![plain_gcmon()]));
    to_github
        .send(GitHubMessage::Search {
            repository: cpython(),
            text: "newer".to_owned(),
        })
        .unwrap();

    assert!(
        refreshes.newer_search(&cpython(), "older"),
        "a search for another text waits, so the older answer changes nothing"
    );
    assert!(
        !refreshes.newer_search(&cpython(), "newer"),
        "the answer to the text that waits is the one that counts"
    );
    assert!(
        !refreshes.newer_search(&RepositoryName::parse("a/b").unwrap(), "older"),
        "a search in another Repository says nothing of this one"
    );
}

#[test]
fn watching_follows_github_until_told_to_stop() {
    // `Replies` panics if it is asked a fourth time, so the third status has to stop the watching.
    let access = replying(vec![signed_in(), Err(Unreachable::Offline), signed_in()]);

    let heard = watched_through(access, Duration::ZERO, |shown| match shown {
        3 => vec![GitHubMessage::Stop],
        _ => vec![],
    });

    let offline_as = Status::from(Account::OfflineAs {
        last_login: Login::new("octocat"),
    });
    assert_eq!(heard, [octocat(), offline_as, octocat()]);
}

/// A store write that failed, which the session tells the GitHub thread to show.
fn database_problem() -> Error {
    Error::new(ErrorKind::Internal, "Tsugiki can't write to its database.")
}

/// Watches GitHub through `access`, with a status check each `status`, and tells the GitHub thread the
/// messages `after` gives for each status the board announces, by its count from 1. Gives what the board
/// announced.
fn watched_through(
    access: GitHubAccess,
    status: Duration,
    after: fn(usize) -> Vec<GitHubMessage>,
) -> Vec<Status> {
    let (to_github, incoming) = mpsc::channel();
    let (passed, seen) = mpsc::channel();
    let mut shown = 0;
    let mut board = StatusBoard::new(Box::new(move |status: &Status| {
        passed.send(status.clone()).unwrap();
        shown += 1;
        for message in after(shown) {
            to_github.send(message).unwrap();
        }
    }));
    let refreshes = Refreshes {
        intervals: Intervals { status, ..HOURLY },
        incoming,
        ..unheard_refreshes()
    };

    watch(&mut board, &empty_store(), access, &refreshes);

    announced(&seen)
}

#[test]
fn with_a_token_a_problem_leaves_the_login_and_the_next_status_check_takes_it_away() {
    let heard = watched_through(octocat_answers(), Duration::ZERO, |shown| match shown {
        1 => vec![GitHubMessage::Problem(database_problem())],
        3 => vec![GitHubMessage::Stop],
        _ => vec![],
    });

    let beside = Status {
        account: octocat().account,
        problem: Some(database_problem()),
    };
    assert_eq!(heard, [octocat(), beside, octocat()]);
}

#[test]
fn a_refused_token_stays_in_the_account_when_a_problem_comes() {
    // The Stop waits behind the problem, so the watching ends whatever the board makes of it.
    let heard = watched_through(answering(401, ""), HOURLY.status, |shown| match shown {
        1 => vec![
            GitHubMessage::Problem(database_problem()),
            GitHubMessage::Stop,
        ],
        _ => vec![],
    });

    // A status holds a login or a token message, which a test mustn't log, so a failure gives the count.
    let [refused, troubled] = &heard[..] else {
        panic!("{} statuses", heard.len());
    };
    assert!(matches!(refused.account, Account::Refused { .. }));
    assert_eq!(refused.problem, None);
    assert_eq!(troubled.account, refused.account);
    assert_eq!(troubled.problem, Some(database_problem()));
}

/// The answer to `FetchIssueContent`.
fn issue_content_reply() -> Result<Reply, Unreachable> {
    let issue = json!({
        "id": "I_1",
        "bodyHTML": "<p>It crashes</p>",
        "createdAt": "2026-11-01T09:00:00Z",
        "comments": { "totalCount": 0, "nodes": [] },
    });
    answer(json!({ "repository": { "issue": issue } }), RATE_LIMIT)
}

/// The answer to `FetchPullRequestContent` for a PR with a body and nothing else.
fn pr_content_reply() -> Result<Reply, Unreachable> {
    pull_request_answer(pr_content(&json!({})), RATE_LIMIT)
}

/// A session that runs `ask` and stops, what the database then holds of Issue 7, and the activity facts of
/// its Repository.
fn after_asking(
    name: &str,
    access: GitHubAccess,
    ask: impl FnOnce(&Fetching),
) -> (Option<IssueContent>, ActivityFacts, Vec<Status>) {
    let (folders, folder) = temp_folders(name);
    let (board, seen) = watched_board();
    let session = Session::start(board, Ok(folders.clone()), reaching(access), HOURLY);
    ask(&session.fetching());
    session.stop(PLENTY);

    let db = OpenStore::of(folders.database(), folder);
    let content = db.reader.issue_content(&gcmon(), 7).unwrap();
    let facts = db.reader.work_facts(&gcmon()).unwrap();
    db.close();
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

#[test]
fn mark_seen_moves_last_seen_to_the_moment_the_window_drew_and_asks_github_nothing() {
    // `Replies` panics if it is asked a second time.
    let access = replying(vec![signed_in()]);
    let drawn = hours_ago(2);

    let (content, activity, _) = after_asking("mark-seen", access, |asked| {
        asked.mark_unseen(
            gcmon(),
            Unseen {
                number: 8,
                archived: None,
            },
            None,
        );
        asked.mark_seen(
            gcmon(),
            SeenMark {
                numbers: vec![7, 8],
                up_to: Some(drawn),
                findings: vec![],
            },
            // The session answers an action it kept, and this one has nowhere to pass the answer on.
            Some(Asked {
                id: 1,
                listing: Listing::Workbench {
                    view: WorkbenchView::Whole,
                },
            }),
        );
    });

    assert_eq!(content, None);
    assert_eq!(activity.last_seen, BTreeMap::from([(7, drawn), (8, drawn)]));
    assert_eq!(activity.marked_unseen, BTreeMap::new());
}

#[test]
fn archive_marks_the_item_seen_and_keeps_it_archived_until_unarchive() {
    let access = replying(vec![signed_in()]);
    let drawn = hours_ago(2);
    let issue = ItemKey::Issue { number: 7 };
    let standalone = ItemKey::PullRequest { number: 9 };

    let (_, activity, _) = after_asking("archive", access, |asked| {
        let mark = |numbers| SeenMark {
            numbers,
            up_to: Some(drawn),
            findings: vec![],
        };
        asked.archive(
            gcmon(),
            Hiding {
                key: issue,
                mark: mark(vec![7, 8]),
            },
            None,
        );
        asked.keep_archived(
            gcmon(),
            Hiding {
                key: issue,
                mark: mark(vec![7]),
            },
            None,
        );
        asked.archive(
            gcmon(),
            Hiding {
                key: standalone,
                mark: mark(vec![9]),
            },
            None,
        );
        asked.unarchive(gcmon(), standalone, None);
    });

    let archived = activity.archived.iter();
    let seen_up_to = archived.map(|(key, archive)| (*key, archive.seen_up_to));
    assert_eq!(
        BTreeMap::from_iter(seen_up_to),
        BTreeMap::from([(issue, drawn)])
    );
    let seen = BTreeMap::from([(7, drawn), (8, drawn), (9, drawn)]);
    assert_eq!(activity.last_seen, seen);
}

#[test]
fn mute_marks_the_item_seen_and_keeps_it_muted_until_unmute() {
    let access = replying(vec![signed_in()]);
    let drawn = hours_ago(2);
    let issue = ItemKey::Issue { number: 7 };
    let standalone = ItemKey::PullRequest { number: 9 };

    let (_, activity, _) = after_asking("mute", access, |asked| {
        let mark = |numbers| SeenMark {
            numbers,
            up_to: Some(drawn),
            findings: vec![],
        };
        asked.mute(
            gcmon(),
            Hiding {
                key: issue,
                mark: mark(vec![7, 8]),
            },
            None,
        );
        asked.mute(
            gcmon(),
            Hiding {
                key: standalone,
                mark: mark(vec![9]),
            },
            None,
        );
        asked.unmute(gcmon(), standalone);
    });

    assert_eq!(activity.muted, BTreeMap::from([(issue, drawn)]));
    let seen = BTreeMap::from([(7, drawn), (8, drawn), (9, drawn)]);
    assert_eq!(activity.last_seen, seen);
}

#[test]
fn mark_unseen_flags_the_item_from_now() {
    let access = replying(vec![signed_in()]);
    let before = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap();

    let (_, activity, _) = after_asking("mark-unseen", access, |asked| {
        asked.mark_unseen(
            gcmon(),
            Unseen {
                number: 7,
                archived: None,
            },
            None,
        )
    });

    let flagged = activity.marked_unseen.get(&7).copied().unwrap();
    assert!(flagged >= before, "{flagged} {before}");
    assert_eq!(activity.last_seen, BTreeMap::new());
}

#[test]
fn opening_a_pr_keeps_its_recorded_conversation_and_leaves_last_seen_alone() {
    let access = GitHubAccess {
        transport: Box::new(Replayer::new(recordings())),
        token_from: SIGNED_IN,
    };
    let (folders, _folder) = temp_folders("open-pr");
    let (board, changes) = changes_board();
    let session = Session::start(board, Ok(folders.clone()), reaching(access), HOURLY);

    session.fetching().fetch_pull_request(cpython(), 152_326);
    wait_for_message(&changes, PLENTY);
    let content = (session.reader())
        .pull_request_content(&cpython(), 152_326)
        .unwrap();
    let facts = session.reader().work_facts(&cpython()).unwrap();
    session.stop(PLENTY);

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
    let access = replying(vec![signed_in(), pr_content_reply()]);

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
        token_from: NO_GH,
    };
    let drawn = hours_ago(1);

    let (content, activity, _) = after_asking("no-token-marks", access, |asked| {
        asked.mark_seen(
            gcmon(),
            SeenMark {
                numbers: vec![7],
                up_to: Some(drawn),
                findings: vec![],
            },
            None,
        );
        asked.mark_unseen(
            gcmon(),
            Unseen {
                number: 8,
                archived: None,
            },
            None,
        );
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
    let (folders, _folder) = temp_folders("open-issue-changes");
    let access = replying(vec![signed_in(), issue_content_reply()]);
    let session = Session::start(board, Ok(folders.clone()), reaching(access), HOURLY);

    let requests = session.fetching();
    requests.mark_seen(
        gcmon(),
        SeenMark {
            numbers: vec![7, 8],
            up_to: None,
            findings: vec![],
        },
        None,
    );
    requests.fetch_issue(gcmon(), 7);
    session.stop(PLENTY);

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
fn a_mark_answers_with_the_list_it_left() {
    let (folders, _folder) = temp_folders("answered-list");
    let store = Store::open(&folders.database()).unwrap();
    stores_issue_5_with_its_prs(&store);
    store.close();
    let (sent, answered) = mpsc::channel();
    let board = quiet_board();
    let connections = Connections::new(offline(), mirrors(), listed(vec![cpython_gcmon()]))
        .with_acted(Box::new(move |answer: &Answered| {
            sent.send(answer.clone()).unwrap();
        }));
    let session = Session::start(board, Ok(folders.clone()), connections, HOURLY);
    let issue = ItemKey::Issue { number: 5 };

    let hiding = Hiding {
        key: issue,
        mark: SeenMark {
            numbers: vec![5, 8, 9],
            up_to: Some(hours_ago(1)),
            findings: vec![],
        },
    };
    session.fetching().mute(
        gcmon(),
        hiding,
        Some(Asked {
            id: 42,
            listing: Listing::Workbench {
                view: WorkbenchView::Whole,
            },
        }),
    );
    let answer = answered.recv_timeout(PLENTY).unwrap();
    session.stop(PLENTY);

    assert_eq!((answer.id, answer.repository), (42, gcmon()));
    let Page::Workbench { workbench } = answer.page else {
        panic!("the answer reads the Workbench the action asked about");
    };
    assert!(
        !workbench.items.iter().any(|listed| listed.key == issue),
        "the Muted item left the Workbench"
    );
}

#[test]
fn a_mark_in_the_inbox_answers_with_the_page_it_left() {
    let (folders, _folder) = temp_folders("answered-inbox");
    let store = Store::open(&folders.database()).unwrap();
    stores_issue_5_with_its_prs(&store);
    store.close();
    let (sent, answered) = mpsc::channel();
    let board = quiet_board();
    let connections = Connections::new(offline(), mirrors(), listed(vec![cpython_gcmon()]))
        .with_acted(Box::new(move |answer: &Answered| {
            sent.send(answer.clone()).unwrap();
        }));
    let session = Session::start(board, Ok(folders.clone()), connections, HOURLY);
    let issue = ItemKey::Issue { number: 5 };

    let hiding = Hiding {
        key: issue,
        mark: SeenMark {
            numbers: vec![5, 8, 9],
            up_to: Some(hours_ago(1)),
            findings: vec![],
        },
    };
    session.fetching().mute(
        gcmon(),
        hiding,
        Some(Asked {
            id: 7,
            listing: Listing::Inbox {
                page: InboxPage::Latest,
                search: String::new(),
            },
        }),
    );
    let answer = answered.recv_timeout(PLENTY).unwrap();
    session.stop(PLENTY);

    assert_eq!((answer.id, answer.repository), (7, gcmon()));
    let Page::Inbox { inbox } = answer.page else {
        panic!("the answer reads the Inbox page the action asked about");
    };
    assert!(
        !inbox.items.iter().any(|listed| listed.key == issue),
        "the Muted item left the Inbox"
    );
}

#[test]
fn an_issue_github_has_lost_troubles_only_the_log() {
    let gone = answer(json!({ "repository": { "issue": null } }), RATE_LIMIT);
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

fn unseen_7() -> Marked {
    Marked::Unseen {
        repository: gcmon(),
        number: 7,
        archived: None,
        at: OffsetDateTime::UNIX_EPOCH,
    }
}

/// Where a test's session tells: the window through `board`, and the GitHub thread through what this
/// returns. The copies say GitHub can be asked, and that the thread has done none of the user's refreshes.
fn telling(board: &mut StatusBoard) -> (Telling, mpsc::Receiver<GitHubMessage>) {
    let (github, heard) = mpsc::channel();
    let telling = telling_through(board, github, crate::latest::channel(0).1);
    (telling, heard)
}

/// Where a test's session tells the window through `board`, and the GitHub thread through `github`, whose
/// copy of how many of the user's refreshes it has done is `refreshes_done`.
fn telling_through(
    board: &mut StatusBoard,
    github: Sender<GitHubMessage>,
    refreshes_done: LatestReader<u64>,
) -> Telling {
    Telling {
        changes: board.changes(),
        acted: Box::new(|_| {}),
        github,
        shown: board.shown(),
        refreshes_done,
    }
}

/// A session's two threads as `Session::start` connects them, for a test that runs them itself: the GitHub
/// thread's refreshes, and the session's own thread, which passes it the demands on GitHub.
struct TwoThreads {
    refreshes: Refreshes,
    keeper: Keeper,
    /// Stops the session's thread once the GitHub thread has stopped.
    requests: Fetching,
}

/// A session's two threads, with the GitHub thread's refreshes at `intervals` and settings that list gcmon,
/// and the handle that sends the session's thread its messages. Both hand the Mirror worker their asks
/// through `mirrors`.
fn two_threads(intervals: Intervals, mirrors: MirrorRequests) -> (TwoThreads, Fetching) {
    let (messages, incoming) = mpsc::channel();
    let requests = sending(messages, mirrors);
    let (to_github, github_queue) = mpsc::channel();
    let settings = listed(vec![plain_gcmon()]);
    let (refreshes, done) =
        Refreshes::new(intervals, github_queue, requests.clone(), settings.clone());
    let telling = telling_through(&mut quiet_board(), to_github, done);
    let keeper = Keeper::new(
        incoming,
        requests.clone(),
        Reads {
            settings,
            reader: unopened_reader(),
        },
        telling,
    );
    let threads = TwoThreads {
        refreshes,
        keeper,
        requests: requests.clone(),
    };
    (threads, requests)
}

impl TwoThreads {
    /// The GitHub thread's queue, for a message that has to reach it before its next turn, as a message the
    /// session's thread passes on reaches it a moment later.
    fn github(&self) -> Sender<GitHubMessage> {
        self.keeper.telling.github.clone()
    }

    /// Has the GitHub thread watch GitHub through `access` with `board` on this thread, and the session's
    /// thread take its messages on another, both on `store`, until the session takes a stop or the GitHub
    /// thread does.
    fn watch(self, board: &mut StatusBoard, store: &Store, access: GitHubAccess) {
        let Self {
            refreshes,
            keeper,
            requests,
        } = self;
        thread::scope(|scope| {
            #[expect(
                unused_results,
                reason = "the scope joins it, and the stop it takes ends it"
            )]
            scope.spawn(move || {
                keeper.run(store);
                keeper.telling.to_github(GitHubMessage::Stop);
            });
            watch(board, store, access, &refreshes);
            requests.stop_session();
        });
    }
}

/// The session's queue, reading a view that lists gcmon as `listed` says and counts `saves` handed to the
/// session, which `messages` wait in, and telling as `telling` says.
fn keeper_told(
    saves: usize,
    listed: RepositorySettings,
    messages: Vec<SessionMessage>,
    telling: Telling,
) -> Keeper {
    let (sent, incoming) = mpsc::channel();
    for message in messages {
        sent.send(message).unwrap();
    }
    let view = SettingsView {
        saves_told: saves,
        ..listing(vec![listed])
    };
    Keeper::new(
        incoming,
        unheard(),
        Reads {
            settings: crate::latest::channel(view).1,
            reader: unopened_reader(),
        },
        telling,
    )
}

/// A handle whose demands on GitHub reach `github` as the session's thread passes them on, from a thread of
/// its own that a stop ends, as it ends a session, and passes on to `github` last.
fn passed_on(github: Sender<GitHubMessage>) -> Fetching {
    let (messages, incoming) = mpsc::channel();
    let told = telling_through(&mut quiet_board(), github, crate::latest::channel(0).1);
    let keeper = Keeper::new(
        incoming,
        unheard(),
        Reads {
            settings: listed(vec![plain_gcmon()]),
            reader: unopened_reader(),
        },
        told,
    );
    #[expect(
        unused_results,
        reason = "it ends on the stop the handle sends, or once the handle goes"
    )]
    thread::spawn(move || {
        // A store that opens no file, so the demands pass on at once. They need none.
        keeper.run(&Store::stopped());
        keeper.telling.to_github(GitHubMessage::Stop);
    });
    sending(messages, mirrors())
}

/// The session's queue as `keeper_told` says, telling a quiet board and a GitHub thread that isn't there.
fn quiet_keeper(saves: usize, listed: RepositorySettings, messages: Vec<SessionMessage>) -> Keeper {
    keeper_told(saves, listed, messages, telling(&mut quiet_board()).0)
}

/// The messages left in `keeper`'s queue, as the test names them.
fn left_in(keeper: &Keeper) -> Vec<String> {
    let mut left = Vec::new();
    while let Ok(message) = keeper.next_message(Duration::ZERO) {
        left.push(crate::fetching::testing::named(message));
    }
    left
}

fn mute_5() -> Marked {
    Marked::Muted {
        repository: gcmon(),
        key: ItemKey::Issue { number: 5 },
        mark: issue_5_seen(),
        at: OffsetDateTime::now_utc(),
    }
}

fn unmute_5() -> Marked {
    Marked::Unmuted {
        repository: gcmon(),
        key: ItemKey::Issue { number: 5 },
    }
}

/// The hand edit that gave gcmon CPython's Conventions, under which PRs 8 and 9 fix Issue 5.
fn cpython_saved() -> SessionMessage {
    let save = SettingsSave::new(cpython_gcmon(), &[plain_gcmon()], SavedBy::HandEdit);
    SessionMessage::SettingsChanged(save)
}

/// Whether the store holds Issue 5 muted.
fn issue_5_muted(store: &Store) -> bool {
    let activity = store.work_facts(&gcmon()).unwrap().activity;
    activity.muted.contains_key(&ItemKey::Issue { number: 5 })
}

#[test]
fn a_mute_and_an_unmute_while_a_save_is_owed_keep_their_order() {
    let store = empty_store();
    stores_issue_5_with_its_prs(&store);
    let messages = vec![
        SessionMessage::Mark {
            marked: mute_5(),
            asked: None,
        },
        SessionMessage::Mark {
            marked: unmute_5(),
            asked: None,
        },
        cpython_saved(),
    ];
    let keeper = quiet_keeper(1, cpython_gcmon(), messages);

    keeper.run(&store);

    assert!(!issue_5_muted(&store));
}

#[test]
fn a_mark_before_a_stop_is_kept_while_a_save_is_owed() {
    let store = empty_store();
    stores_issue_5_with_its_prs(&store);
    let messages = vec![
        SessionMessage::Mark {
            marked: mute_5(),
            asked: None,
        },
        SessionMessage::Stop,
        cpython_saved(),
    ];
    let keeper = quiet_keeper(1, cpython_gcmon(), messages);

    keeper.run(&store);

    assert!(issue_5_muted(&store));
}

/// gcmon under CPython's Conventions, with the Bugs Filter switched `on` the Workbench.
fn cpython_bugs(on: OnWorkbench) -> RepositorySettings {
    RepositorySettings {
        conventions: Conventions::Cpython,
        ..bugs_switched(on)
    }
}

#[test]
fn an_unmute_before_a_save_it_hasnt_followed_moves_reach_from_where_the_save_ends() {
    // Issue 5 is muted. PR 9 fixes it and Issue 6, which the Bugs Filter finds, and a hand edit switches the
    // Filter on.
    let store = empty_store();
    stores_issue_5_with_its_prs(&store);
    store.save_issues(&gcmon(), vec![issue_facts(6)]).unwrap();
    let fixing_both = PullRequestFacts {
        title: "gh-5, gh-6: Fix both".to_owned(),
        ..stored_pr(9, "some-author", "89abcde")
    };
    store
        .save_pull_requests(&gcmon(), vec![fixing_both])
        .unwrap();
    let muting = Muting {
        key: ItemKey::Issue { number: 5 },
        numbers: vec![5, 8, 9],
        up_to: None,
        findings: vec![],
    };
    store
        .mute(&gcmon(), muting, OffsetDateTime::now_utc())
        .unwrap();
    let found = FilterResults {
        numbers: [6].into(),
        problem: None,
    };
    store
        .save_filter_results(&gcmon(), &bugs_query(), found)
        .unwrap();
    let switched = SettingsSave::new(
        cpython_bugs(OnWorkbench::On),
        &[cpython_bugs(OnWorkbench::Off)],
        SavedBy::HandEdit,
    );
    let messages = vec![
        SessionMessage::Mark {
            marked: unmute_5(),
            asked: None,
        },
        SessionMessage::SettingsChanged(switched),
    ];
    let keeper = quiet_keeper(1, cpython_bugs(OnWorkbench::On), messages);

    keeper.run(&store);

    // The save puts PR 9 in Reach through Issue 6, and the Unmute PR 8 through Issue 5. Taken the other way,
    // PR 9 is in Reach before and after each move: through Issue 6 for the Unmute's, and Issue 5 for the
    // save's.
    assert_eq!(store.read_ahead_owed(&gcmon()).unwrap(), [8, 9].into());
}

#[test]
fn a_mark_follows_only_the_saves_its_settings_hold() {
    let store = empty_store();
    stores_issue_5_with_its_prs(&store);
    // The second save came after the view the mark read its settings from.
    let switch = SettingsSave::new(cpython_gcmon(), &[cpython_gcmon()], SavedBy::Window);
    let messages = vec![cpython_saved(), SessionMessage::SettingsChanged(switch)];
    let keeper = quiet_keeper(1, cpython_gcmon(), messages);

    keeper.mark(&store, unmute_5(), None);

    assert_eq!(left_in(&keeper), ["switch"]);
}

#[test]
fn a_mark_the_queue_holds_no_owed_save_for_is_kept_at_once() {
    let store = empty_store();
    stores_issue_5_with_its_prs(&store);
    let keeper = quiet_keeper(1, cpython_gcmon(), vec![SessionMessage::Refresh]);

    keeper.mark(&store, mute_5(), None);

    assert!(issue_5_muted(&store));
    assert_eq!(left_in(&keeper), ["refresh"]);
}

#[test]
fn what_the_github_thread_saved_has_the_session_hand_over_the_wants_then_the_heads_saved() {
    let store = empty_store();
    stores_issue_5_with_its_prs(&store);
    let (mirrors, heard) = HeardRequests::new();
    let keeper = Keeper::new(
        mpsc::channel().1,
        sending(mpsc::channel().0, mirrors),
        Reads {
            settings: listed(vec![plain_gcmon()]),
            reader: unopened_reader(),
        },
        telling(&mut quiet_board()).0,
    );
    let saved = |items: Vec<u32>| {
        SessionMessage::Saved(Change {
            repository: gcmon(),
            items,
        })
    };

    // A Repository the window hasn't opened doesn't sync, so it hands over nothing.
    assert!(keeper.take(&store, saved(vec![8])).is_continue());
    let unopened = heard.asked();
    let open = SessionMessage::Sync {
        repository: gcmon(),
    };
    assert!(keeper.take(&store, open).is_continue());
    assert!(keeper.take(&store, saved(vec![8])).is_continue());
    let handed = heard.asked();
    // A save that names no PR with a head hands over the wants alone.
    assert!(keeper.take(&store, saved(Vec::new())).is_continue());
    let wants_alone = heard.asked();

    assert_eq!(unopened, Vec::<String>::new());
    assert_eq!(handed.first().map(String::as_str), Some(HEADS_OF_8));
    let saved_heads = "some-maintainer/gcmon saved heads #8 at 4567def on main";
    assert_eq!(handed.last().map(String::as_str), Some(saved_heads));
    assert_eq!(wants_alone.first().map(String::as_str), Some(HEADS_OF_8));
    assert!(
        !wants_alone.iter().any(|line| line.contains("saved heads")),
        "{wants_alone:?}"
    );
}

#[test]
fn a_mark_that_moves_no_reach_follows_no_save() {
    let keeper = quiet_keeper(1, cpython_gcmon(), vec![cpython_saved()]);

    keeper.mark(&Store::stopped(), unseen_7(), None);

    assert_eq!(left_in(&keeper), [format!("settings of {}", gcmon())]);
}

#[test]
fn a_hand_edit_a_mute_follows_ahead_of_the_open_of_its_repository_tells_the_window() {
    let store = empty_store();
    stores_issue_5_with_its_prs(&store);
    // The window opens gcmon, and a hand edit gives it new Conventions, both after the Mute.
    let messages = vec![
        SessionMessage::Sync {
            repository: gcmon(),
        },
        cpython_saved(),
    ];
    let (mut board, changes) = board_hearing_settings();
    let keeper = keeper_told(1, cpython_gcmon(), messages, telling(&mut board).0);

    keeper.mark(&store, mute_5(), None);

    let told: Vec<Change> = changes.try_iter().collect();
    assert_eq!(
        told,
        [Change {
            repository: gcmon(),
            items: vec![]
        }]
    );
    assert_eq!(left_in(&keeper), ["other"]);
}

#[test]
fn a_save_missing_from_the_queue_leaves_later_marks_to_the_saves_their_view_counts() {
    let store = empty_store();
    stores_issue_5_with_its_prs(&store);
    let (sent, incoming) = mpsc::channel();
    let view = SettingsView {
        saves_told: 1,
        ..listing(vec![cpython_gcmon()])
    };
    let quiet = telling(&mut quiet_board()).0;
    let keeper = Keeper::new(
        incoming,
        unheard(),
        Reads {
            settings: crate::latest::channel(view).1,
            reader: unopened_reader(),
        },
        quiet,
    );
    keeper.mark(&store, mute_5(), None);
    // A save that only a view after this one counts.
    let switch = SettingsSave::new(cpython_gcmon(), &[cpython_gcmon()], SavedBy::Window);
    sent.send(SessionMessage::SettingsChanged(switch)).unwrap();

    keeper.mark(&store, unmute_5(), None);

    assert_eq!(left_in(&keeper), ["switch"]);
}

#[test]
fn a_mark_that_cant_be_stored_is_a_problem() {
    let seen_mark = Marked::Seen {
        repository: gcmon(),
        mark: SeenMark {
            numbers: vec![7],
            up_to: None,
            findings: vec![],
        },
    };
    let unseen = Marked::Unseen {
        repository: gcmon(),
        number: 7,
        archived: None,
        at: OffsetDateTime::UNIX_EPOCH,
    };
    for marked in [seen_mark, unseen] {
        let (told, github) = telling(&mut quiet_board());
        let keeper = keeper_told(0, plain_gcmon(), vec![], told);

        keeper.mark(&Store::stopped(), marked, None);

        assert_eq!(problems_for_the_status_bar(&github), [ErrorKind::Internal]);
    }
}

#[test]
fn a_mark_whose_list_cant_be_read_is_a_problem() {
    let store = empty_store();
    stores_issue_5_with_its_prs(&store);
    let (told, github) = telling(&mut quiet_board());
    let keeper = keeper_told(0, plain_gcmon(), vec![], told);

    keeper.mark(
        &store,
        mute_5(),
        Some(Asked {
            id: 3,
            listing: Listing::Workbench {
                view: WorkbenchView::Whole,
            },
        }),
    );

    assert_eq!(problems_for_the_status_bar(&github), [ErrorKind::Internal]);
}

/// The kinds of the problems the session asked the GitHub thread to show on the status bar.
fn problems_for_the_status_bar(github: &mpsc::Receiver<GitHubMessage>) -> Vec<ErrorKind> {
    let problem = |message| {
        let GitHubMessage::Problem(problem) = message else {
            return None;
        };
        Some(problem.kind)
    };
    github.try_iter().filter_map(problem).collect()
}

#[test]
fn what_an_issue_says_that_cant_be_stored_is_a_problem() {
    let (mut board, seen) = watched_board();
    let access = replying(vec![signed_in(), issue_content_reply()]);
    let github = connect_alone(&mut quiet_board(), &empty_store(), access).unwrap();

    let read = Reading {
        github: &github,
        queue: &unheard_refreshes(),
        syncs: &Syncs::default(),
    };

    let outcome = read_conversation(
        &mut board,
        &Store::stopped(),
        &read,
        &Fetched::Issue(issue_7()),
    );

    assert_eq!(outcome, Ok(()), "the trouble is this item's alone");
    assert_eq!(problem_kind(showing(&seen)), Some(ErrorKind::Internal));
}

#[test]
fn two_demands_for_a_conversation_before_its_read_finishes_ask_github_once() {
    let (script, asked) = Scripted::new(vec![signed_in(), pr_content_reply()]);
    let access = GitHubAccess {
        transport: Box::new(script),
        token_from: SIGNED_IN,
    };
    let (folders, _folder) = temp_folders("one-read");
    let (board, changes) = changes_board();
    let session = Session::start(board, Ok(folders), reaching(access), HOURLY);

    session.fetching().fetch_pull_request(gcmon(), 7);
    session.fetching().fetch_pull_request(gcmon(), 7);
    wait_for_message(&changes, PLENTY);
    session.stop(PLENTY);

    let operations: Vec<String> = asked.try_iter().map(|(operation, _)| operation).collect();
    assert_eq!(
        operations.len(),
        2,
        "the sign-in and one read: {operations:?}"
    );
}

#[test]
fn a_failed_read_answers_the_demands_that_came_while_it_ran_with_its_failure() {
    let (script, asked) = Scripted::new(vec![signed_in(), Err(Unreachable::Offline)]);
    let access = GitHubAccess {
        transport: Box::new(script),
        token_from: SIGNED_IN,
    };

    let failed = failed_reads("one-failed-read", access, |demands| {
        demands.fetch_pull_request(gcmon(), 7);
        demands.fetch_pull_request(gcmon(), 7);
    });

    assert_eq!(failed.len(), 1, "{failed:?}");
    assert_eq!(asked.try_iter().count(), 2, "the sign-in and one read");
}

#[test]
fn a_demand_after_a_read_ended_reads_again() {
    let replies = vec![signed_in(), pr_content_reply(), pr_content_reply()];
    let (script, asked) = Scripted::new(replies);
    let access = GitHubAccess {
        transport: Box::new(script),
        token_from: SIGNED_IN,
    };
    let (folders, _folder) = temp_folders("read-again");
    let (board, changes) = changes_board();
    let session = Session::start(board, Ok(folders), reaching(access), HOURLY);

    session.fetching().fetch_pull_request(gcmon(), 7);
    wait_for_message(&changes, PLENTY);
    session.fetching().fetch_pull_request(gcmon(), 7);
    wait_for_message(&changes, PLENTY);
    session.stop(PLENTY);

    assert_eq!(asked.try_iter().count(), 3, "the sign-in and two reads");
}

/// PR 7 of gcmon kept with `head`, a GitHub that answers its Conversation with `conversations`, and the
/// session's queue and syncs, with settings that list gcmon and gcmon opened, and a Mirror worker that only
/// listens.
struct OneRead {
    _folder: TempPath,
    store: Store,
    github: GitHub,
    demands: Fetching,
    /// The session's thread, which passes the demands on.
    passing: Keeper,
    queue: Refreshes,
    syncs: Syncs,
    heard: HeardRequests,
}

/// `message` as a test names the session's message it passes on, or "other".
fn named_for_github(message: GitHubMessage) -> String {
    let passed = match message {
        GitHubMessage::Stop => SessionMessage::Stop,
        GitHubMessage::Check { name, reply } => SessionMessage::Check { name, reply },
        GitHubMessage::Sync { repository } => SessionMessage::Sync { repository },
        GitHubMessage::Refresh => SessionMessage::Refresh,
        GitHubMessage::Fetch(fetched) => SessionMessage::Fetch(fetched),
        GitHubMessage::Screen(moved) => SessionMessage::Screen(moved),
        GitHubMessage::FetchUnstored { item, reply } => {
            SessionMessage::FetchUnstored { item, reply }
        }
        GitHubMessage::Search { repository, text } => SessionMessage::Search { repository, text },
        GitHubMessage::Size { .. } | GitHubMessage::Problem(_) | GitHubMessage::Stopped(_) => {
            return "other".to_owned();
        }
    };
    crate::fetching::testing::named(passed)
}

impl OneRead {
    fn new(name: &str, head: Option<&str>, conversations: Vec<Value>) -> Self {
        let (folders, folder) = temp_folders(name);
        let store = Store::open(&folders.database()).unwrap();
        let kept = PullRequestFacts {
            head: head.and_then(CommitSha::parse),
            ..stored_pr(7, "some-author", "0123abc")
        };
        store.save_pull_requests(&gcmon(), vec![kept]).unwrap();
        let mut replies = vec![signed_in()];
        replies.extend(
            (conversations.into_iter())
                .map(|conversation| pull_request_answer(conversation, RATE_LIMIT)),
        );
        let github = connect_alone(&mut quiet_board(), &store, replying(replies)).unwrap();
        let (demands, incoming, heard) = crate::fetching::testing::handle();
        let listing = listed(vec![plain_gcmon()]);
        let mut syncs = Syncs::new(listing.clone());
        syncs.add(gcmon());
        let (to_github, github_queue) = mpsc::channel();
        let (queue, done) = Refreshes::new(HOURLY, github_queue, demands.clone(), listing.clone());
        let told = telling_through(&mut quiet_board(), to_github, done);
        let passing = Keeper::new(
            incoming,
            demands.clone(),
            Reads {
                settings: listing,
                reader: unopened_reader(),
            },
            told,
        );
        // The session's thread took the Sync the syncs above added gcmon by.
        passing.opened.borrow_mut().push(gcmon());
        Self {
            _folder: folder,
            store,
            github,
            demands,
            passing,
            queue,
            syncs,
            heard,
        }
    }

    /// Has the session's thread pass the demands it took to the GitHub thread's queue.
    fn pass_on(&self) {
        while let Ok(message) = self.passing.next_message(Duration::ZERO) {
            assert!(self.passing.take(&self.store, message).is_continue());
        }
    }

    /// Reads `fetched`, as the GitHub thread does when its turn comes.
    fn read(&self, fetched: &Fetched) {
        self.pass_on();
        let read = Reading {
            github: &self.github,
            queue: &self.queue,
            syncs: &self.syncs,
        };
        read_conversation(&mut quiet_board(), &self.store, &read, fetched).unwrap();
        // The session's thread hands the Mirror worker what the read changed of the rules' wants.
        self.pass_on();
    }

    /// The messages left in the GitHub thread's queue, as the test names them.
    fn queued(&self) -> Vec<String> {
        self.pass_on();
        let mut queued = Vec::new();
        while let Ok(message) = self.queue.next_message(Duration::ZERO) {
            queued.push(named_for_github(message));
        }
        queued
    }

    fn finish(self) {
        self.store.close();
    }
}

fn pr_7() -> Fetched {
    Fetched::PullRequest(Item {
        repository: gcmon(),
        number: 7,
    })
}

#[test]
fn a_conversation_that_fills_the_head_of_a_pr_answers_the_await_of_a_demand_it_merged() {
    let one = OneRead::new(
        "conversation-fills-head",
        None,
        vec![pr_content(&json!({}))],
    );
    // The Files tab found no head while the read waited, and awaits it.
    assert_eq!(one.demands.find_head(gcmon(), 7), QUEUED);

    one.read(&pr_7());

    let handed = one.heard.asked();
    assert_eq!(
        handed.first().map(String::as_str),
        Some("some-maintainer/gcmon awaits #7")
    );
    assert!(
        handed.iter().any(|asked| asked.contains("wants heads")),
        "the filled head changes what the rules want: {handed:?}"
    );
    assert_eq!(
        handed.last().map(String::as_str),
        Some("some-maintainer/gcmon saved heads #7 at 0123abc on main"),
    );
    assert!(one.queued().is_empty(), "the demand shared the read");
    assert_eq!(one.demands.why_no_head(&gcmon(), 7), None);
    one.finish();
}

#[test]
fn a_conversation_of_a_pr_that_had_its_head_hands_only_the_head_to_the_mirror() {
    let one = OneRead::new(
        "conversation-had-head",
        Some("0123abc"),
        vec![pr_content(&json!({}))],
    );

    one.read(&pr_7());

    assert_eq!(
        one.heard.asked(),
        ["some-maintainer/gcmon saved heads #7 at 0123abc on main"]
    );
    one.finish();
}

#[test]
fn a_conversation_that_names_no_head_says_so_until_a_read_names_one() {
    let unnamed = pr_content(&json!({ "headRefOid": "" }));
    let one = OneRead::new(
        "conversation-no-head",
        None,
        vec![unnamed, pr_content(&json!({}))],
    );

    assert_eq!(one.demands.find_head(gcmon(), 7), QUEUED);
    one.read(&pr_7());
    let after_unnamed = one.demands.why_no_head(&gcmon(), 7);
    let other_pr = one.demands.why_no_head(&gcmon(), 8);
    let unawaited = one.heard.asked();
    one.read(&pr_7());

    assert_eq!(after_unnamed, Some(NoHead::Unnamed));
    assert_eq!(other_pr, None);
    assert_eq!(
        unawaited,
        [
            "some-maintainer/gcmon awaits #7",
            "some-maintainer/gcmon awaits no head of #7"
        ],
        "no read will answer the await"
    );
    assert_eq!(one.demands.why_no_head(&gcmon(), 7), None);
    assert_eq!(
        one.heard.asked().last().map(String::as_str),
        Some("some-maintainer/gcmon saved heads #7 at 0123abc on main")
    );
    one.finish();
}

#[test]
fn a_failed_conversation_read_says_why_the_pr_has_no_head_until_a_read_names_one() {
    // GitHub first answers that it has no PR 7.
    let one = OneRead::new(
        "conversation-unread",
        None,
        vec![Value::Null, pr_content(&json!({}))],
    );

    assert_eq!(one.demands.find_head(gcmon(), 7), QUEUED);
    one.read(&pr_7());
    let after_failure = one.demands.why_no_head(&gcmon(), 7);
    let unawaited = one.heard.asked();
    one.read(&pr_7());

    assert_eq!(after_failure, Some(NoHead::Unread(ItemUnread::NotOnGitHub)));
    assert_eq!(
        unawaited,
        [
            "some-maintainer/gcmon awaits #7",
            "some-maintainer/gcmon awaits no head of #7"
        ],
        "no read will answer the await"
    );
    assert_eq!(one.demands.why_no_head(&gcmon(), 7), None);
    assert_eq!(
        one.heard.asked().last().map(String::as_str),
        Some("some-maintainer/gcmon saved heads #7 at 0123abc on main")
    );
    one.finish();
}

#[test]
fn a_new_demand_for_a_conversation_that_named_no_head_waits_for_its_read() {
    let unnamed = pr_content(&json!({ "headRefOid": "" }));
    let one = OneRead::new("reopened-no-head", None, vec![unnamed]);
    one.read(&pr_7());

    one.demands.fetch_pull_request(gcmon(), 7);

    assert_eq!(one.demands.why_no_head(&gcmon(), 7), None);
    one.finish();
}

#[test]
fn a_conversation_no_newer_than_the_facts_hands_over_the_facts_head() {
    // As old as the PR list's read of PR 7.
    let as_old = pr_content(&json!({ "updatedAt": "1970-01-01T00:00:01Z" }));
    let one = OneRead::new("conversation-other-head", Some("4567def"), vec![as_old]);

    one.read(&pr_7());

    assert_eq!(
        one.heard.asked(),
        ["some-maintainer/gcmon saved heads #7 at 4567def on main"],
        "the head the Files tab asks for"
    );
    one.finish();
}

#[test]
fn a_conversation_newer_than_the_facts_hands_over_its_head_and_the_rules_wants_again() {
    let one = OneRead::new(
        "conversation-newer-head",
        Some("4567def"),
        vec![pr_content(&json!({}))],
    );

    one.read(&pr_7());

    let handed = one.heard.asked();
    assert!(
        handed.iter().any(|asked| asked.contains("wants heads")),
        "the moved head changes what the rules want: {handed:?}"
    );
    assert_eq!(
        handed.last().map(String::as_str),
        Some("some-maintainer/gcmon saved heads #7 at 0123abc on main"),
        "pushed to since the PR list's read"
    );
    one.finish();
}

#[test]
fn a_conversation_with_a_new_review_of_the_users_hands_over_the_rules_wants_again() {
    let review = json!({
        "id": "R1",
        "author": { "login": "octocat" },
        "state": "APPROVED",
        "submittedAt": "2026-11-04T10:00:00Z",
        "bodyHTML": "",
        "isMinimized": false,
        "minimizedReason": null,
        "commit": { "oid": "0123abc" },
    });
    let reviewed = pr_content(&json!({ "reviews": { "totalCount": 1, "nodes": [review] } }));
    let one = OneRead::new("conversation-reviewed", Some("0123abc"), vec![reviewed]);
    one.store.remember_login(&Login::new("octocat")).unwrap();

    one.read(&pr_7());

    let handed = one.heard.asked();
    assert!(
        handed.contains(&"some-maintainer/gcmon wants reviewed #7 at 0123abc".to_owned()),
        "{handed:?}"
    );
    assert_eq!(
        handed.last().map(String::as_str),
        Some("some-maintainer/gcmon saved heads #7 at 0123abc on main")
    );
    one.finish();
}

#[test]
fn a_read_merges_only_the_demands_for_the_same_read_and_keeps_the_rest_in_order() {
    let one = OneRead::new("merge", Some("0123abc"), vec![pr_content(&json!({}))]);
    one.demands.fetch_issue(gcmon(), 7);
    one.demands.fetch_pull_request(gcmon(), 7);
    one.demands.refresh();
    one.demands.fetch_pull_request(gcmon(), 8);

    one.read(&pr_7());
    one.demands.fetch_pull_request(gcmon(), 7);

    assert_eq!(one.queued(), ["Issue #7", "refresh", "PR #8", "PR #7"]);
    one.finish();
}

#[test]
fn losing_the_network_while_reading_an_issue_shows_on_the_status_bar() {
    let access = replying(vec![signed_in(), Err(Unreachable::Offline)]);

    let (content, _, shown) = after_asking("offline-issue", access, |asked| {
        asked.fetch_issue(gcmon(), 7)
    });

    assert_eq!(content, None);
    let offline_as = Status::from(Account::OfflineAs {
        last_login: Login::new("octocat"),
    });
    assert_eq!(shown.last(), Some(&offline_as));
}

/// The reads that failed while a session with `access` ran `ask`.
fn failed_reads(name: &str, access: GitHubAccess, ask: fn(&Fetching)) -> Vec<FailedRead> {
    let (folders, _folder) = temp_folders(name);
    let (board, failed) = failed_reads_board();
    let session = Session::start(board, Ok(folders.clone()), reaching(access), HOURLY);
    ask(&session.fetching());
    session.stop(PLENTY);
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
    let gone = answer(json!({ "repository": { "issue": null } }), RATE_LIMIT);
    let access = replying(vec![signed_in(), gone]);

    let failed = failed_reads_of_issue_7("gone-issue-read", access);

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
fn a_pr_read_with_no_token_says_why_the_pr_has_no_head_before_the_window_hears() {
    let access = GitHubAccess {
        transport: Box::new(Canned(Err(Unreachable::Offline))),
        token_from: NO_GH,
    };
    let (folders, _folder) = temp_folders("no-token-head");
    let (board, failed) = failed_reads_board();
    let session = Session::start(board, Ok(folders), reaching(access), HOURLY);

    assert_eq!(session.fetching().find_head(gcmon(), 7), QUEUED);
    let heard = failed.recv_timeout(PLENTY).unwrap();
    let why = session.fetching().why_no_head(&gcmon(), 7);
    session.stop(PLENTY);

    assert_eq!(heard, item_7_failed(ReadFailure::NoToken));
    assert_eq!(why, Some(NoHead::Unread(ItemUnread::NoToken)));
}

#[test]
fn a_pr_read_with_no_token_says_so() {
    let access = GitHubAccess {
        transport: Box::new(Canned(Err(Unreachable::Offline))),
        token_from: NO_GH,
    };

    let failed = failed_reads_of_pr_7("no-token-pr", access);

    assert_eq!(failed, [item_7_failed(ReadFailure::NoToken)]);
}

#[test]
fn a_pr_github_doesnt_have_says_so() {
    let gone = pull_request_answer(Value::Null, RATE_LIMIT);
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
    let access = replying(vec![signed_in(), pr_content_reply()]);

    let failed = failed_reads_of_pr_7("landed-pr", access);

    assert_eq!(failed, []);
}

const AN_HOUR: Duration = Duration::from_secs(3600);
const HOURLY: Intervals = Intervals {
    status: AN_HOUR,
    repository: AN_HOUR,
    on_screen: AN_HOUR,
};
const PLENTY: Duration = Duration::from_secs(10);

/// The answer to a fetch of an item the database lacks while the status says GitHub can't be reached.
const UNFETCHED_OFFLINE: ItemFetch = ItemFetch::Unfetched {
    why: ItemUnfetched::Offline,
};

const GCMON: &str = r#"{"data":{"repository":{"nameWithOwner":"some-maintainer/gcmon",
        "issues":{"totalCount":10},"pullRequests":{"totalCount":3}},
        "updated":{"issueCount":43},"closed":{"issueCount":35}}}"#;

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
    let access = replying(vec![signed_in(), reply_ok(GCMON)]);
    let (folders, _folder) = temp_folders("check");
    let session = Session::start(quiet_board(), Ok(folders.clone()), reaching(access), HOURLY);

    let check = session.fetching().check(gcmon()).unwrap();
    session.stop(PLENTY);

    assert_eq!(check.repository.name, gcmon());
    assert_eq!((check.open_issues, check.open_prs), (10, 3));
    assert_eq!(check.size, RepositorySize::Small);
    // Its 13 open items and the 35 that closed in the window.
    assert_eq!(check.first_sync_items, 48);
}

#[test]
fn a_check_github_cant_answer_says_why() {
    let (folders, _folder) = temp_folders("check-offline");
    let session = Session::start(
        quiet_board(),
        Ok(folders.clone()),
        reaching(offline()),
        HOURLY,
    );

    let problem = session.fetching().check(gcmon()).unwrap_err();
    session.stop(PLENTY);

    assert_eq!(problem.kind, ErrorKind::Offline);
    assert_eq!(problem.message, "Tsugiki can't reach GitHub.");
}

#[test]
fn a_session_that_has_ended_says_so_to_a_check() {
    let access = GitHubAccess {
        transport: Box::new(answered(200, GCMON)),
        token_from: NO_GH,
    };
    let (folders, _folder) = temp_folders("check-ended");
    let session = Session::start(quiet_board(), Ok(folders.clone()), reaching(access), HOURLY);

    let problem = session.fetching().check(gcmon()).unwrap_err();
    session.stop(PLENTY);

    assert_eq!(problem.kind, ErrorKind::Offline);
    assert_eq!(problem.message, crate::fetching::NOT_CONNECTED);
}

#[test]
fn checks_dont_put_off_the_next_refresh() {
    let (messages, incoming) = mpsc::channel();
    let refreshes = Refreshes {
        intervals: Intervals {
            status: Duration::from_millis(100),
            ..HOURLY
        },
        incoming,
        ..unheard_refreshes()
    };
    let github = github(answered(200, GCMON));
    let started = Instant::now();
    let due = Due::starting_at(started, refreshes.intervals);
    let mut syncs = Syncs::default();
    let refreshed = AtomicBool::new(false);

    thread::scope(|scope| {
        #[expect(
            unused_results,
            reason = "the scope joins it, and it stops by itself, so a timer that starts over with each \
                      check fails rather than hangs"
        )]
        scope.spawn(|| {
            while !refreshed.load(Ordering::Relaxed) && started.elapsed() < PLENTY {
                let (reply, answer) = mpsc::channel();
                let check = GitHubMessage::Check {
                    name: gcmon(),
                    reply,
                };
                messages.send(check).unwrap();
                drop(answer.recv());
            }
            messages.send(GitHubMessage::Stop).unwrap();
        });
        // Each check takes a turn of its own, which leaves the refresh due when it was.
        let turn = loop {
            let turn = refreshes.next_turn(&github, &mut syncs, due);
            if !matches!(turn, Turn::Asked) {
                break turn;
            }
        };
        assert!(matches!(turn, Turn::Refresh), "{turn:?}");
        refreshed.store(true, Ordering::Relaxed);
        // Answers the checks still on their way, until the message that stops it.
        while !matches!(refreshes.next_turn(&github, &mut syncs, due), Turn::Stop) {}
    });

    assert!(started.elapsed() < PLENTY);
}

fn work_search(ids: &[&str]) -> Result<Reply, Unreachable> {
    search_page(ids, None, RATE_LIMIT)
}

/// The fetch of the user's PR 7 in `gcmon`.
fn work_fetch() -> Result<Reply, Unreachable> {
    let node = pr_node(
        7,
        &json!({
            "title": "gh-7: Fix it",
            "repository": {
                "nameWithOwner": "some-maintainer/gcmon",
                "defaultBranchRef": { "name": "main" },
            },
        }),
    );
    fetched_nodes(&[node], RATE_LIMIT)
}

/// The recorded `python/cpython` work as the Workbench lists it: the items in order, with each item's
/// reasons.
fn inbox_of(workbench: &Workbench) -> String {
    let line = |listed: &ListedItem| {
        let number = listed.item.updated_at_and_number().1;
        let why = listed.attention.as_ref().map(|attention| &attention.why);
        format!("#{number} {why:?}")
    };
    let items = workbench.items.iter().map(line).collect::<Vec<_>>();
    format!(
        "count {}, review requests {}\n\n{}\n",
        workbench.count,
        workbench.review_requests,
        items.join("\n"),
    )
}

#[test]
fn the_recorded_cpython_work_reads_as_an_inbox() {
    let access = GitHubAccess {
        transport: Box::new(Replayer::new(recordings())),
        token_from: SIGNED_IN,
    };
    let (folders, _folder) = temp_folders("recorded-inbox");
    let (board, changes) = changes_board();
    let session = Session::start(board, Ok(folders.clone()), reaching(access), HOURLY);
    let reader = session.reader();
    // The settings list gcmon alone, so CPython syncs under the defaults.
    let cpython = cpython_settings();

    session.fetching().sync(cpython.name.clone());
    // Two Filters, the PRs they found, their Issues, one Issue's tree and an Origin PR.
    for _ in 0..6 {
        wait_for_message(&changes, PLENTY);
    }
    let workbench = Workbench::read(
        &reader,
        &cpython,
        &WorkbenchView::Whole,
        ListOrder::Attention,
    )
    .unwrap();
    session.stop(PLENTY);

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
        token_from: SIGNED_IN,
    };
    let (folders, _folder) = temp_folders("sync");
    let (board, changes) = changes_board();
    let session = Session::start(board, Ok(folders.clone()), reaching(access), HOURLY);
    let reader = session.reader();

    session.fetching().sync(gcmon());
    let changed = changes.recv().unwrap();
    let workbench = Workbench::read(
        &reader,
        &plain_gcmon(),
        &WorkbenchView::Whole,
        ListOrder::Attention,
    )
    .unwrap();
    session.stop(PLENTY);

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

#[test]
fn a_read_of_a_pr_on_screen_and_a_sync_both_add_to_the_hours_points() {
    // The same hour as `RATE_LIMIT`, and a cost that tells this read apart from the sync's answers.
    let pr_read = pull_request_answer(
        pr_content(&json!({})),
        r#""rateLimit":{"cost":3,"limit":5000,"used":990,"remaining":4010,"resetAt":"2999-01-01T00:00:00Z"}"#,
    );
    let (script, _asked) = Scripted::new(vec![
        signed_in(),
        pr_read,
        work_search(&["a"]),
        work_search(&[]),
        work_search(&[]),
        work_search(&[]),
        work_fetch(),
    ]);
    let access = GitHubAccess {
        transport: Box::new(script),
        token_from: SIGNED_IN,
    };
    let (passed, seen) = mpsc::channel();
    let board = quiet_board().with_hour_points(Box::new(move |points: Option<&HourPoints>| {
        passed.send(*points.unwrap()).unwrap();
    }));
    let (folders, _folder) = temp_folders("hour-points");
    let session = Session::start(board, Ok(folders.clone()), reaching(access), HOURLY);

    session.fetching().fetch_pull_request(gcmon(), 7);
    session.fetching().sync(gcmon());
    let mut announced = vec![seen.recv_timeout(PLENTY).unwrap()];
    // Each of the sync's five answers cost a point.
    while announced.last().unwrap().spent < 8 {
        announced.push(seen.recv_timeout(PLENTY).unwrap());
    }
    session.stop(PLENTY);

    let first = announced[0];
    assert_eq!(first.spent, 3, "the read of the PR counts first");
    // GitHub's first answer of the run came after 987 points of the hour were spent.
    assert!(first.since.is_some());
    let last = announced.last().unwrap();
    assert_eq!((last.spent, last.used, last.since), (8, 1000, first.since));
}

#[test]
fn the_window_stops_showing_the_hours_points_at_its_reset_by_githubs_clock() {
    // This computer's clock is an hour slow, and the hour resets two seconds after GitHub answered.
    let github_now = OffsetDateTime::now_utc() + time::Duration::hours(1);
    let reset = (github_now + time::Duration::seconds(2))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    let viewer = reply_ok(&format!(
        r#"{{"data":{{"viewer":{{"login":"octocat"}},"rateLimit":{{"cost":1,"limit":5000,"used":1,
            "remaining":4999,"resetAt":"{reset}"}}}}}}"#
    ));
    let (script, _asked) = Scripted::new(vec![dated(viewer, github_now)]);
    let access = GitHubAccess {
        transport: Box::new(script),
        token_from: SIGNED_IN,
    };
    let (passed, seen) = mpsc::channel();
    let board = quiet_board().with_hour_points(Box::new(move |points: Option<&HourPoints>| {
        passed.send(points.map(|points| points.spent)).unwrap();
    }));
    let (folders, _folder) = temp_folders("hour-reset");
    let session = Session::start(board, Ok(folders), reaching(access), HOURLY);

    let shown = seen.recv_timeout(PLENTY).unwrap();
    // Long before the next refresh, an hour away.
    let reset = seen.recv_timeout(PLENTY).unwrap();
    session.stop(PLENTY);

    assert_eq!((shown, reset), (Some(1), None));
}

#[test]
fn a_reset_far_off_is_looked_at_again_within_a_minute_as_a_sleeping_mac_stops_its_instant() {
    let viewer = answer(json!({ "viewer": { "login": "octocat" } }), RATE_LIMIT);
    let github = github(Scripted::new(vec![viewer]).0);
    assert_eq!(hour_check(&github), None, "no points, nothing to look at");

    drop(github.viewer().unwrap());

    let at = hour_check(&github).unwrap();
    assert!(at <= Instant::now() + HOUR_CHECK);
}

/// A PR of `author` in `gcmon`, stored with its branch at `head`.
fn stored_pr(number: u32, author: &str, head: &str) -> PullRequestFacts {
    PullRequestFacts {
        title: format!("gh-{number}: Fix it"),
        author: Some(Login::new(author)),
        updated_at: OffsetDateTime::from_unix_timestamp(1).unwrap(),
        head: CommitSha::parse(head),
        ..pr_facts(number)
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
        token_from: SIGNED_IN,
    };
    let (folders, _folder) = temp_folders("reached-heads");
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
        .save_pull_request_content(
            &gcmon(),
            8,
            (
                reviewed_by("octocat", &reviewed),
                OffsetDateTime::UNIX_EPOCH,
            ),
        )
        .unwrap();
    store.close();
    let (mirrors, heard) = HeardRequests::new();
    let session = Session::start(
        quiet_board(),
        Ok(folders.clone()),
        Connections::new(access, mirrors, listed(vec![plain_gcmon()])),
        HOURLY,
    );

    session.fetching().sync(gcmon());
    let handed = heard.asked_through("saved heads", PLENTY);
    session.stop(PLENTY);

    assert_eq!(
        handed,
        [
            "some-maintainer/gcmon wants heads #7 at 89abcde on main",
            "some-maintainer/gcmon wants reviewed #8 at 0123abc",
            // Both sides of the Review the head moved on from, for the Stale review check.
            "some-maintainer/gcmon wants patch-ids of 0123abc on main, 4567def on main",
            // The step saved PR 7, so had the user opened it before its head was known, it would go first.
            "some-maintainer/gcmon saved heads #7 at 89abcde on main",
        ]
    );
}

/// gcmon with one Filter, which searches `label:bug`, switched on the Workbench or off it.
fn bugs_switched(on: OnWorkbench) -> RepositorySettings {
    let bugs = Filter {
        name: "Bugs".to_owned().try_into().unwrap(),
        query: bugs_query(),
        on_workbench: on,
    };
    RepositorySettings {
        filters: vec![bugs],
        ..RepositorySettings::new(gcmon())
    }
}

fn bugs_query() -> FilterQuery {
    "label:bug".to_owned().try_into().unwrap()
}

/// The save that switched gcmon's Bugs Filter `on`, from the other way, as `by` made it.
fn bugs_saved(on: OnWorkbench, by: SavedBy) -> SettingsSave {
    let before = match on {
        OnWorkbench::On => OnWorkbench::Off,
        OnWorkbench::Off => OnWorkbench::On,
    };
    SettingsSave::new(bugs_switched(on), &[bugs_switched(before)], by)
}

#[test]
fn a_filter_switched_on_hands_the_mirror_its_heads_and_owes_a_read_of_its_unread_prs() {
    let store = store_remembering("octocat");
    let issue = IssueFacts {
        number: 5,
        title: "A bug".to_owned(),
        author: None,
        assignees: vec![],
        labels: vec![],
        updated_at: OffsetDateTime::UNIX_EPOCH,
        state: Some(IssueState::Open),
    };
    store.save_issues(&gcmon(), vec![issue]).unwrap();
    let stored = vec![
        stored_pr(8, "some-author", "4567def"),
        stored_pr(9, "some-author", "89abcde"),
        stored_pr(10, "some-author", "0123abc"),
        stored_pr(11, "some-author", "7654fed"),
    ];
    store.save_pull_requests(&gcmon(), stored).unwrap();
    // PR 9's Conversation was read at its head, and PR 10's before its head moved.
    let read_at = |number: u32, head: &str| {
        let content = reviewed_by("some-reviewer", &CommitSha::parse(head).unwrap());
        store
            .save_pull_request_content(&gcmon(), number, (content, OffsetDateTime::UNIX_EPOCH))
            .unwrap();
    };
    read_at(9, "89abcde");
    read_at(10, "fedcba9");
    let found = FilterResults {
        numbers: [5, 8, 9, 10].into(),
        problem: None,
    };
    store
        .save_filter_results(&gcmon(), &bugs_query(), found)
        .unwrap();
    let (mirrors, heard) = HeardRequests::new();
    let (messages, _incoming) = mpsc::channel();
    let fetching = sending(messages, mirrors);
    let elsewhere = RepositorySettings::new(RepositoryName::parse("a/b").unwrap());

    follow_save(
        &fetching,
        &store,
        &SettingsSave::new(elsewhere, &[], SavedBy::HandEdit),
    );
    let added = heard.asked();
    follow_save(
        &fetching,
        &store,
        &bugs_saved(OnWorkbench::On, SavedBy::Window),
    );
    let on = heard.asked();
    let owed = store.read_ahead_owed(&gcmon()).unwrap();
    follow_save(
        &fetching,
        &store,
        &bugs_saved(OnWorkbench::Off, SavedBy::Window),
    );
    let off = heard.asked();

    assert_eq!(added, Vec::<String>::new());
    assert!(
        on.iter().any(|line| line.contains("#8 at 4567def")),
        "{on:?}"
    );
    // Issue 5 has no Conversation to read ahead as a PR, PR 9's was read at its head, and nothing reaches
    // PR 11.
    assert_eq!(owed, [8, 10].into());
    assert!(off.iter().all(|line| !line.contains("#8")), "{off:?}");
}

/// Switches the Bugs Filter on, which finds PR 8, with the store losing call `lost`, if any. Answers with
/// what the Mirror heard, how many calls the store had, and the reads it owes after.
fn switch_bugs_on_losing(lost: Option<usize>) -> (Vec<String>, usize, BTreeSet<u32>) {
    let store = store_remembering("octocat");
    let stored = vec![stored_pr(8, "some-author", "4567def")];
    store.save_pull_requests(&gcmon(), stored).unwrap();
    let found = FilterResults {
        numbers: [8].into(),
        problem: None,
    };
    store
        .save_filter_results(&gcmon(), &bugs_query(), found)
        .unwrap();
    let (lossy, calls) = store.failing_at_call(lost);
    let (mirrors, heard) = HeardRequests::new();
    let (messages, _incoming) = mpsc::channel();
    let fetching = sending(messages, mirrors);

    follow_save(
        &fetching,
        &lossy,
        &bugs_saved(OnWorkbench::On, SavedBy::Window),
    );

    let owed = store.read_ahead_owed(&gcmon()).unwrap();
    (heard.asked(), calls.try_iter().count(), owed)
}

#[test]
fn a_repository_a_hand_edit_takes_out_and_puts_back_moves_reach_from_the_defaults() {
    let (folders, _folder) = temp_folders("settings-block-back");
    let store = store_remembering("octocat");
    let stored = vec![stored_pr(8, "some-author", "4567def")];
    store.save_pull_requests(&gcmon(), stored).unwrap();
    let found = FilterResults {
        numbers: [8].into(),
        problem: None,
    };
    store
        .save_filter_results(&gcmon(), &bugs_query(), found)
        .unwrap();
    let on = gcmon_toml("on_workbench = true\n");
    fs::write(folders.settings_file(), &on).unwrap();
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders.clone())));
    let (sent, saves) = mpsc::channel();
    owner.tell_session(Box::new(move |save| sent.send(save).unwrap()));
    let (mirrors, heard) = HeardRequests::new();
    let fetching = sending(mpsc::channel().0, mirrors);

    // A hand edit takes gcmon's block out, so the session reads the defaults for it, and another puts the
    // block back.
    fs::write(folders.settings_file(), "").unwrap();
    assert!(owner.reread().is_ok());
    let out: Vec<SettingsSave> = saves.try_iter().collect();
    for save in &out {
        follow_save(&fetching, &store, save);
    }
    let wants_out = heard.asked();
    fs::write(folders.settings_file(), &on).unwrap();
    assert!(owner.reread().is_ok());
    let back: Vec<SettingsSave> = saves.try_iter().collect();
    for save in &back {
        follow_save(&fetching, &store, save);
    }

    assert_eq!(
        out.len(),
        1,
        "taking the block out moves gcmon to the defaults"
    );
    assert_eq!(back.len(), 1, "putting it back moves gcmon from them");
    assert!(
        wants_out.iter().all(|line| !line.contains("#8")),
        "{wants_out:?}"
    );
    let wants = heard.asked();
    assert!(
        wants.iter().any(|line| line.contains("#8 at 4567def")),
        "{wants:?}"
    );
    assert_eq!(store.read_ahead_owed(&gcmon()).unwrap(), [8].into());
}

/// A board that hands each change it announces that names no item to the receiver beside it.
fn board_hearing_settings() -> (StatusBoard, mpsc::Receiver<Change>) {
    let (sent, changes) = mpsc::channel();
    let board = quiet_board().with_changes(Box::new(move |change: &Change| {
        if change.items.is_empty() {
            sent.send(change.clone()).unwrap();
        }
    }));
    (board, changes)
}

#[test]
fn a_hand_edit_to_the_settings_tells_the_window_and_the_windows_own_switch_doesnt() {
    let (folders, _folder) = temp_folders("settings-announced");
    let store = Store::open(&folders.database()).unwrap();
    store.remember_login(&me()).unwrap();
    store.close();
    let (board, changes) = board_hearing_settings();
    let (mirrors, _heard) = HeardRequests::new();
    let connections = Connections::new(
        offline(),
        mirrors,
        listed(vec![bugs_switched(OnWorkbench::On)]),
    );
    let session = Session::start(board, Ok(folders.clone()), connections, HOURLY);
    // GitHub's Conventions are those every Repository but CPython starts with.
    let regrouped = RepositorySettings {
        conventions: Conventions::Cpython,
        ..bugs_switched(OnWorkbench::On)
    };
    let elsewhere = RepositoryName::parse("a/b").unwrap();

    let asked = session.fetching();
    // The window opens gcmon and not a/b, and each hears of its hand edits all the same.
    asked.sync(gcmon());
    asked.settings_changed(SettingsSave::new(
        RepositorySettings::new(elsewhere.clone()),
        &[],
        SavedBy::HandEdit,
    ));
    asked.settings_changed(bugs_saved(OnWorkbench::Off, SavedBy::HandEdit));
    asked.settings_changed(bugs_saved(OnWorkbench::On, SavedBy::Window));
    asked.settings_changed(SettingsSave::new(
        regrouped,
        &[bugs_switched(OnWorkbench::On)],
        SavedBy::HandEdit,
    ));
    session.stop(PLENTY);

    // A Repository not opened since launch hears of its hand edit too, as its page in Settings follows its
    // state (#1057). Nothing for the window's own save.
    let changed = |repository| Change {
        repository,
        items: vec![],
    };
    assert_eq!(
        changes.try_iter().collect::<Vec<_>>(),
        [changed(elsewhere), changed(gcmon()), changed(gcmon())]
    );
}

#[test]
fn a_hand_edit_to_the_settings_tells_the_window_with_no_token() {
    let (folders, _folder) = temp_folders("settings-no-token");
    let (board, changes) = board_hearing_settings();
    let access = GitHubAccess {
        transport: Box::new(Canned(Err(Unreachable::Offline))),
        token_from: NO_GH,
    };
    let session = Session::start(board, Ok(folders.clone()), reaching(access), HOURLY);

    // The window opens a Repository with no token as it does with one.
    let asked = session.fetching();
    asked.sync(gcmon());
    asked.settings_changed(bugs_saved(OnWorkbench::On, SavedBy::Window));
    asked.settings_changed(bugs_saved(OnWorkbench::Off, SavedBy::HandEdit));
    session.stop(PLENTY);

    assert_eq!(
        changes.try_iter().collect::<Vec<_>>(),
        [Change {
            repository: gcmon(),
            items: vec![]
        }]
    );
}

#[test]
fn a_switch_whose_reads_ahead_the_store_cant_keep_still_says_it_handed_the_mirror_its_heads() {
    let (_, calls, owed) = switch_bugs_on_losing(None);
    assert_eq!(owed, [8].into());

    let (asked, _, owed) = switch_bugs_on_losing(Some(calls - 1));

    assert!(
        asked.iter().any(|line| line.contains("#8 at 4567def")),
        "{asked:?}"
    );
    assert_eq!(owed, BTreeSet::new());
}

#[test]
fn a_switch_whose_facts_the_store_cant_read_hands_over_nothing_and_owes_nothing() {
    let (asked, _, owed) = switch_bugs_on_losing(Some(0));

    // The next step that saves hands the Mirror its heads.
    assert!(asked.iter().all(|line| !line.contains("#8")), "{asked:?}");
    assert_eq!(owed, BTreeSet::new());
}

/// What a session that took a save of the settings did, as `session_taking` finds it.
struct TookSave {
    /// What the Mirror worker heard.
    handed: Vec<String>,
    /// The reads ahead the store owes in gcmon once the session stopped.
    owed: BTreeSet<u32>,
    /// The changes that name no item, which the window heard.
    announced: Vec<Change>,
}

/// Starts a session that reaches GitHub through `github`, in a folder named after `name`, on a store whose
/// PR 8 the Bugs Filter's stored search found and whose Conversation it never read. The session opens the
/// Repository `opened`, if any, then takes `save`, whose settings the settings owner's view lists.
fn session_taking(
    name: &str,
    github: GitHubAccess,
    opened: Option<RepositoryName>,
    save: SettingsSave,
) -> TookSave {
    let (folders, _folder) = temp_folders(name);
    let store = Store::open(&folders.database()).unwrap();
    store.remember_login(&me()).unwrap();
    let stored = vec![stored_pr(8, "some-author", "4567def")];
    store.save_pull_requests(&gcmon(), stored).unwrap();
    let found = FilterResults {
        numbers: [8].into(),
        problem: None,
    };
    store
        .save_filter_results(&gcmon(), &bugs_query(), found)
        .unwrap();
    store.close();
    let (board, changes) = board_hearing_settings();
    let (mirrors, heard) = HeardRequests::new();
    let settings = listed(vec![save.saved().clone()]);
    let connections = Connections::new(github, mirrors, settings);
    let session = Session::start(board, Ok(folders.clone()), connections, HOURLY);

    if let Some(opened) = opened {
        session.fetching().sync(opened);
    }
    session.fetching().settings_changed(save);
    session.stop(PLENTY);

    let store = Store::open(&folders.database()).unwrap();
    let owed = store.read_ahead_owed(&gcmon()).unwrap();
    store.close();
    TookSave {
        handed: heard.asked(),
        owed,
        announced: changes.try_iter().collect(),
    }
}

const HEADS_OF_8: &str = "some-maintainer/gcmon wants heads #8 at 4567def on main";

#[test]
fn a_session_hands_the_mirror_the_heads_a_switch_puts_in_reach_while_offline() {
    let took = session_taking(
        "switched-session",
        offline(),
        Some(gcmon()),
        bugs_saved(OnWorkbench::On, SavedBy::Window),
    );

    assert!(
        took.handed.contains(&HEADS_OF_8.to_owned()),
        "{:?}",
        took.handed
    );
    assert_eq!(took.owed, [8].into());
}

#[test]
fn a_filter_a_hand_edit_switches_on_in_a_repository_not_opened_owes_reads_and_tells_the_window() {
    // As the first change since launch, which the settings owner hands over with the settings it replaced.
    let took = session_taking(
        "hand-edit-unopened",
        offline(),
        None,
        bugs_saved(OnWorkbench::On, SavedBy::HandEdit),
    );

    assert_eq!(took.owed, [8].into());
    assert!(
        took.handed.contains(&HEADS_OF_8.to_owned()),
        "{:?}",
        took.handed
    );
    // The window hasn't opened gcmon since launch, but gcmon's page in Settings follows its state (#1057).
    assert_eq!(
        took.announced,
        [Change {
            repository: gcmon(),
            items: vec![]
        }]
    );
}

#[test]
fn a_repository_a_hand_edit_adds_owes_no_reads() {
    let took = session_taking(
        "hand-edit-added",
        offline(),
        None,
        SettingsSave::new(bugs_switched(OnWorkbench::On), &[], SavedBy::HandEdit),
    );

    assert_eq!(took.owed, BTreeSet::new());
    assert!(
        took.handed.iter().all(|line| !line.contains("#8")),
        "{:?}",
        took.handed
    );
}

#[test]
fn a_save_that_leaves_reach_as_it_was_hands_the_mirror_nothing() {
    let renamed = |settings: RepositorySettings| {
        let mut renamed = settings;
        renamed.filters[0].name = "Defects".to_owned().try_into().unwrap();
        renamed
    };
    let on = bugs_switched(OnWorkbench::On);

    let took = session_taking(
        "renamed-filter",
        offline(),
        None,
        SettingsSave::new(renamed(on.clone()), &[on], SavedBy::HandEdit),
    );

    // The Mirror worker would have heard PR 8's head again, from facts that can be days old.
    assert_eq!(took.handed, Vec::<String>::new());
    assert_eq!(took.owed, BTreeSet::new());
}

#[test]
fn a_session_with_no_token_follows_the_reach_a_hand_edit_moves() {
    let access = GitHubAccess {
        transport: Box::new(Canned(Err(Unreachable::Offline))),
        token_from: NO_GH,
    };

    let took = session_taking(
        "hand-edit-no-token",
        access,
        None,
        bugs_saved(OnWorkbench::On, SavedBy::HandEdit),
    );

    assert_eq!(took.owed, [8].into());
    assert!(
        took.handed.contains(&HEADS_OF_8.to_owned()),
        "{:?}",
        took.handed
    );
    // The window hears of the hand edit to gcmon, opened or not, with a token or without.
    assert_eq!(
        took.announced,
        [Change {
            repository: gcmon(),
            items: vec![]
        }]
    );
}

/// Has `store` hold Issue 5, which the user's PR 8 and someone else's PR 9 fix, for the user to mute.
fn stores_issue_5_with_its_prs(store: &Store) {
    store.remember_login(&me()).unwrap();
    store.save_issues(&gcmon(), vec![issue_facts(5)]).unwrap();
    let fixing_5 = |number: u32, author: &str, head: &str| PullRequestFacts {
        title: "gh-5: Fix it".to_owned(),
        ..stored_pr(number, author, head)
    };
    let stored = vec![
        fixing_5(8, "octocat", "4567def"),
        fixing_5(9, "some-author", "89abcde"),
    ];
    store.save_pull_requests(&gcmon(), stored).unwrap();
}

/// gcmon under CPython's Conventions, which link a PR to the Issue its title names.
fn cpython_gcmon() -> RepositorySettings {
    RepositorySettings {
        conventions: Conventions::Cpython,
        ..plain_gcmon()
    }
}

/// The mark Mute makes on Issue 5 and its PRs, as `stores_issue_5_with_its_prs` stores them.
fn issue_5_seen() -> SeenMark {
    SeenMark {
        numbers: vec![5, 8, 9],
        up_to: None,
        findings: vec![],
    }
}

#[test]
fn mute_takes_an_issues_heads_from_the_mirror_and_unmute_hands_them_back_and_owes_their_reads() {
    let (folders, _folder) = temp_folders("muted-session");
    let store = Store::open(&folders.database()).unwrap();
    stores_issue_5_with_its_prs(&store);
    store.close();
    let (mirrors, heard) = HeardRequests::new();
    let connections = Connections::new(offline(), mirrors, listed(vec![cpython_gcmon()]));
    let session = Session::start(quiet_board(), Ok(folders.clone()), connections, HOURLY);
    let issue = ItemKey::Issue { number: 5 };

    let asked = session.fetching();
    asked.sync(gcmon());
    asked.mute(
        gcmon(),
        Hiding {
            key: issue,
            mark: issue_5_seen(),
        },
        None,
    );
    let muted = heard.asked_through("wants heads none", PLENTY);
    asked.unmute(gcmon(), issue);
    let unmuted = heard.asked_through("wants heads #8", PLENTY);
    session.stop(PLENTY);
    let store = Store::open(&folders.database()).unwrap();
    let owed = store.read_ahead_owed(&gcmon()).unwrap();

    assert!(
        muted.contains(&"some-maintainer/gcmon wants heads none".to_owned()),
        "{muted:?}"
    );
    let heads = "some-maintainer/gcmon wants heads #8 at 4567def on main, #9 at 89abcde on main";
    assert!(unmuted.contains(&heads.to_owned()), "{unmuted:?}");
    assert_eq!(owed, [8, 9].into());
}

#[test]
fn pin_hands_the_mirror_the_heads_of_an_issues_tree_and_owes_their_reads_and_unpin_takes_them_back()
{
    let (folders, _folder) = temp_folders("pinned-session");
    let store = Store::open(&folders.database()).unwrap();
    store.remember_login(&me()).unwrap();
    store.save_issues(&gcmon(), vec![issue_facts(5)]).unwrap();
    // Nobody else puts Issue 5 on the Workbench: the PRs that fix it are someone else's.
    let theirs = |number: u32, head: &str| PullRequestFacts {
        title: "gh-5: Fix it".to_owned(),
        ..stored_pr(number, "some-author", head)
    };
    let stored = vec![theirs(8, "4567def"), theirs(9, "89abcde")];
    store.save_pull_requests(&gcmon(), stored).unwrap();
    store.close();
    let (mirrors, heard) = HeardRequests::new();
    let connections = Connections::new(offline(), mirrors, listed(vec![cpython_gcmon()]));
    let (board, changes) = changes_board();
    let session = Session::start(board, Ok(folders.clone()), connections, HOURLY);

    let asked = session.fetching();
    asked.sync(gcmon());
    asked.pin(gcmon(), 5, None);
    let pinned = heard.asked_through("wants heads #8", PLENTY);
    let change = changes.recv_timeout(PLENTY).unwrap();
    asked.unpin(gcmon(), 5, None);
    let unpinned = heard.asked_through("wants heads none", PLENTY);
    session.stop(PLENTY);
    let store = Store::open(&folders.database()).unwrap();
    let owed = store.read_ahead_owed(&gcmon()).unwrap();
    let left = crate::store::testing::pinned(&store, &gcmon());

    let heads = "some-maintainer/gcmon wants heads #8 at 4567def on main, #9 at 89abcde on main";
    assert!(pinned.contains(&heads.to_owned()), "{pinned:?}");
    assert_eq!(change.items, [5]);
    assert!(
        unpinned.contains(&"some-maintainer/gcmon wants heads none".to_owned()),
        "{unpinned:?}"
    );
    assert_eq!(owed, [8, 9].into());
    assert_eq!(left, BTreeSet::new());
}

#[test]
fn with_no_token_mute_tells_the_window_and_unmute_owes_the_read_of_a_pr_read_at_an_older_head() {
    let (folders, _folder) = temp_folders("muted-no-token");
    let store = Store::open(&folders.database()).unwrap();
    stores_issue_5_with_its_prs(&store);
    // PR 8's Conversation was read before its head moved, and PR 9's at its head.
    let read_at = |number: u32, head: &str| {
        let content = reviewed_by("some-reviewer", &CommitSha::parse(head).unwrap());
        store
            .save_pull_request_content(&gcmon(), number, (content, OffsetDateTime::UNIX_EPOCH))
            .unwrap();
    };
    read_at(8, "fedcba9");
    read_at(9, "89abcde");
    store.close();
    let (mirrors, heard) = HeardRequests::new();
    let github = GitHubAccess {
        transport: Box::new(Canned(Err(Unreachable::Offline))),
        token_from: NO_GH,
    };
    let connections = Connections::new(github, mirrors, listed(vec![cpython_gcmon()]));
    let (board, changes) = changes_board();
    let session = Session::start(board, Ok(folders.clone()), connections, HOURLY);
    let issue = ItemKey::Issue { number: 5 };

    let asked = session.fetching();
    asked.sync(gcmon());
    asked.mute(
        gcmon(),
        Hiding {
            key: issue,
            mark: issue_5_seen(),
        },
        None,
    );
    let muted = heard.asked_through("wants heads none", PLENTY);
    let change = changes.recv_timeout(PLENTY).unwrap();
    asked.unmute(gcmon(), issue);
    let unmuted = heard.asked_through("wants heads #8", PLENTY);
    session.stop(PLENTY);
    let store = Store::open(&folders.database()).unwrap();
    let owed = store.read_ahead_owed(&gcmon()).unwrap();

    assert!(
        muted.contains(&"some-maintainer/gcmon wants heads none".to_owned()),
        "{muted:?}"
    );
    assert_eq!(change.items, [5, 8, 9]);
    let heads = "some-maintainer/gcmon wants heads #8 at 4567def on main, #9 at 89abcde on main";
    assert!(unmuted.contains(&heads.to_owned()), "{unmuted:?}");
    assert_eq!(owed, [8].into());
}

#[test]
fn mute_author_marks_their_items_seen_takes_their_trees_from_the_mirror_and_unmute_hands_them_back()
{
    let (folders, _folder) = temp_folders("muted-author-session");
    let store = Store::open(&folders.database()).unwrap();
    stores_issue_5_with_its_prs(&store);
    let author = Login::new("some-author");
    let opened = IssueFacts {
        author: Some(author.clone()),
        updated_at: hours_ago(3),
        ..issue_facts(5)
    };
    store.save_issues(&gcmon(), vec![opened]).unwrap();
    store.close();
    let (mirrors, heard) = HeardRequests::new();
    let connections = Connections::new(offline(), mirrors, listed(vec![cpython_gcmon()]));
    let (board, changes) = changes_board();
    let session = Session::start(board, Ok(folders.clone()), connections, HOURLY);

    let asked = session.fetching();
    asked.sync(gcmon());
    asked.mute_author(gcmon(), author.clone(), None);
    let muted = heard.asked_through("wants heads none", PLENTY);
    let change = changes.recv_timeout(PLENTY).unwrap();
    let store = session.reader();
    let activity = store.work_facts(&gcmon()).unwrap().activity;
    asked.unmute_author(gcmon(), author);
    let unmuted = heard.asked_through("wants heads #8", PLENTY);
    session.stop(PLENTY);

    assert!(
        muted.contains(&"some-maintainer/gcmon wants heads none".to_owned()),
        "{muted:?}"
    );
    assert_eq!(change.items, [5, 8, 9], "their Issue and its tree");
    let at = activity
        .muted_authors
        .first()
        .map(|muted| muted.login.as_str());
    assert_eq!(at, Some("some-author"));
    assert_eq!(activity.last_seen.keys().collect::<Vec<_>>(), [&5, &8, &9]);
    let heads = "some-maintainer/gcmon wants heads #8 at 4567def on main, #9 at 89abcde on main";
    assert!(unmuted.contains(&heads.to_owned()), "{unmuted:?}");
}

#[test]
fn mute_author_on_facts_the_store_cant_read_keeps_nothing_and_unmute_author_still_unmutes() {
    let store = empty_store();
    let author = Login::new("some-author");
    let at = OffsetDateTime::now_utc();
    let muting = AuthorMuting {
        author: author.clone(),
        marks: Vec::new(),
    };
    store.mute_author(&gcmon(), muting, at).unwrap();
    let settings = cpython_gcmon();
    let other = Login::new("another-author");
    let muted = Marked::AuthorMuted {
        repository: gcmon(),
        author: other,
        at,
    };
    let unmuted = Marked::AuthorUnmuted {
        repository: gcmon(),
        author,
    };

    // The first call of each reads the facts, and goes missing.
    let (lossy, _calls) = store.failing_at_call(Some(0));
    let (mute_saved, mute_change) = keep_mark(&lossy, &settings, muted);
    let (lossy, _calls) = store.failing_at_call(Some(0));
    let (unmute_saved, unmute_change) = keep_mark(&lossy, &settings, unmuted);

    assert!(mute_saved.is_err());
    assert!(unmute_saved.is_ok());
    let nothing: Vec<u32> = Vec::new();
    assert_eq!(
        [mute_change.items, unmute_change.items],
        [nothing.clone(), nothing]
    );
    let activity = store.work_facts(&gcmon()).unwrap().activity;
    assert_eq!(activity.muted_authors, []);
}

#[test]
fn show_muted_answers_once_the_store_kept_the_switch_or_why_it_couldnt() {
    let db = OpenStore::new("session-show-muted");
    let (lossy, _calls) = db.store.failing_at_call(Some(0));
    let (mirrors, _heard) = HeardRequests::new();
    let (messages, incoming) = mpsc::channel();
    let keeper = Keeper::new(
        incoming,
        sending(messages, mirrors),
        Reads {
            settings: listed(vec![cpython_gcmon()]),
            reader: unopened_reader(),
        },
        telling(&mut quiet_board()).0,
    );
    let switch = |show| {
        let (reply, answer) = mpsc::channel();
        let message = SessionMessage::ShowMuted {
            repository: gcmon(),
            show,
            reply,
        };
        (message, answer)
    };

    let (on, saved) = switch(ShowMuted::On);
    let took_on = keeper.take(&db.store, on);
    let (off, lost) = switch(ShowMuted::Off);
    let took_off = keeper.take(&lossy, off);
    let read = (db.reader).read_snapshot(&gcmon(), |snapshot| snapshot.show_muted());

    // The lossy store's thread holds the store's queue open, which closing the store waits on.
    drop(lossy);
    db.close();
    assert!(took_on.is_continue() && took_off.is_continue());
    assert_eq!(saved.recv().unwrap(), Ok(()));
    assert!(lost.recv().unwrap().is_err(), "the save went missing");
    assert_eq!(read.unwrap(), ShowMuted::On);
}

#[test]
fn a_mute_whose_facts_the_store_cant_read_back_keeps_the_mute_and_hands_the_mirror_nothing() {
    let store = empty_store();
    stores_issue_5_with_its_prs(&store);
    let (lossy, calls) = store.failing_at_call(Some(2));
    let (mirrors, heard) = HeardRequests::new();
    let (messages, incoming) = mpsc::channel();
    let keeper = Keeper::new(
        incoming,
        sending(messages, mirrors),
        Reads {
            settings: listed(vec![cpython_gcmon()]),
            reader: unopened_reader(),
        },
        telling(&mut quiet_board()).0,
    );
    let issue = ItemKey::Issue { number: 5 };
    let muted = Marked::Muted {
        repository: gcmon(),
        key: issue,
        mark: issue_5_seen(),
        at: OffsetDateTime::now_utc(),
    };

    keeper.mark(&lossy, muted, None);

    // The calls go: the facts before the Mute, the Mute, then the facts after it, which went missing. The
    // next step that saves hands the Mirror its heads.
    assert_eq!(calls.try_iter().collect::<Vec<_>>(), [0, 1, 2]);
    assert_eq!(heard.asked(), Vec::<String>::new());
    let activity = store.work_facts(&gcmon()).unwrap().activity;
    assert_eq!(activity.muted.keys().collect::<Vec<_>>(), [&issue]);
}

#[test]
fn a_sync_keeps_the_last_check_of_a_pr_whose_head_moved_to_a_commit_already_read() {
    let (handed, held) = synced_with_both_sides_read("last-check-on-sync", "");

    let last = LastCheck {
        reviewed: CommitSha::parse("0123abc").unwrap(),
        head: CommitSha::parse("4567def").unwrap(),
    };
    assert!(
        handed.contains(&"some-maintainer/gcmon wants patch-ids of none".to_owned()),
        "{handed:?}"
    );
    assert_eq!(held.last_checks, BTreeMap::from([(8, last)]));
}

#[test]
fn a_last_check_the_store_cant_keep_on_a_sync_leaves_the_mirror_its_wants() {
    let refuse = "CREATE TRIGGER refuse BEFORE INSERT ON last_checks
                  BEGIN SELECT RAISE(ABORT, 'refused'); END;";

    let (handed, held) = synced_with_both_sides_read("last-check-refused", refuse);

    assert!(
        handed.contains(&"some-maintainer/gcmon wants patch-ids of none".to_owned()),
        "{handed:?}"
    );
    assert_eq!(held.last_checks, BTreeMap::new());
}

/// A sync of gcmon, whose PR 8 moved to a head whose patch-ids the store holds, as it holds those of the
/// commit the user's Review covered, with `prepared` run on the database first. Gives what the Mirror worker
/// heard up to the heads the sync saved, and the Review facts the store holds then.
#[expect(
    clippy::disallowed_types,
    reason = "a store that can't keep a last check needs a trigger the store would never write"
)]
fn synced_with_both_sides_read(name: &str, prepared: &str) -> (Vec<String>, ReviewFacts) {
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
        token_from: SIGNED_IN,
    };
    let (folders, _folder) = temp_folders(name);
    let store = Store::open(&folders.database()).unwrap();
    store
        .save_pull_requests(&gcmon(), vec![stored_pr(8, "some-author", "4567def")])
        .unwrap();
    let reviewed = CommitSha::parse("0123abc").unwrap();
    store
        .save_pull_request_content(
            &gcmon(),
            8,
            (
                reviewed_by("octocat", &reviewed),
                OffsetDateTime::UNIX_EPOCH,
            ),
        )
        .unwrap();
    let both = vec![patches_of("0123abc", 'a'), patches_of("4567def", 'a')];
    store.save_patch_ids(&gcmon(), both).unwrap();
    store.close();
    let raw = rusqlite::Connection::open(folders.database()).unwrap();
    raw.execute_batch(prepared).unwrap();
    drop(raw);
    let (mirrors, heard) = HeardRequests::new();
    let session = Session::start(
        quiet_board(),
        Ok(folders.clone()),
        Connections::new(access, mirrors, listed(vec![plain_gcmon()])),
        HOURLY,
    );
    session.fetching().sync(gcmon());
    let handed = heard.asked_through("saved heads", PLENTY);
    let held = session.reader().work_facts(&gcmon()).unwrap().reviews;
    session.stop(PLENTY);
    (handed, held)
}

/// `commit`'s patch-ids against main, with `Lib/ast.py` changed in the way `digit` names.
fn patches_of(commit: &str, digit: char) -> CommitPatches {
    CommitPatches {
        of: on_main(commit),
        files: vec![FilePatch {
            path: "Lib/ast.py".to_owned(),
            verbatim: PatchId::parse(&digit.to_string().repeat(40)).unwrap(),
            loose: PatchId::parse(&"b".repeat(40)).unwrap(),
            generated: false,
        }],
    }
}

fn on_main(commit: &str) -> PatchesOf {
    PatchesOf {
        commit: CommitSha::parse(commit).unwrap(),
        target_branch: "main".to_owned(),
    }
}

/// Stores in `folders` that the user reviewed PR 8 at commit 0123abc and its head moved on to 4567def, and
/// that the last run left a reason it couldn't read commit fedcba9.
fn store_pr_8_reviewed(folders: &Folders) {
    let store = Store::open(&folders.database()).unwrap();
    store.remember_login(&me()).unwrap();
    let stored = vec![
        stored_pr(7, "octocat", "89abcde"),
        stored_pr(8, "some-author", "4567def"),
    ];
    store.save_pull_requests(&gcmon(), stored).unwrap();
    let reviewed = CommitSha::parse("0123abc").unwrap();
    store
        .save_pull_request_content(
            &gcmon(),
            8,
            (
                reviewed_by("octocat", &reviewed),
                OffsetDateTime::UNIX_EPOCH,
            ),
        )
        .unwrap();
    let left = UncheckedCommit {
        of: on_main("fedcba9"),
        reason: CantCheck::Cloning,
    };
    store.save_unchecked(&gcmon(), vec![left]).unwrap();
    store.close();
}

/// Has a session keep each of `handed` from the Mirror worker, where `store_pr_8_reviewed` says. Gives the
/// change the window heard after each, and the review facts the store then holds.
fn keeping_patches(
    name: &str,
    access: GitHubAccess,
    handed: Vec<PatchReads>,
) -> (Vec<Change>, ReviewFacts) {
    let (folders, _folder) = temp_folders(name);
    store_pr_8_reviewed(&folders);
    let (board, changes) = changes_board();
    let session = Session::start(board, Ok(folders.clone()), reaching(access), HOURLY);

    let mut keep = session.fetching().patch_ids_keeper();
    let heard = handed.into_iter().map(|reads| {
        keep(&gcmon(), reads);
        changes.recv_timeout(PLENTY).unwrap()
    });
    let heard = heard.collect();
    let held = session.reader().work_facts(&gcmon()).unwrap().reviews;
    session.stop(PLENTY);
    (heard, held)
}

#[test]
fn the_session_keeps_the_patch_ids_the_mirror_read_and_tells_the_window_whose_they_are() {
    let head = PatchReads::Read(vec![patches_of("4567def", 'a')]);
    let (heard, held) = keeping_patches("keeps-patches", replying(vec![signed_in()]), vec![head]);

    assert_eq!(
        heard,
        [Change {
            repository: gcmon(),
            items: vec![8],
        }]
    );
    assert_eq!(held.patches.len(), 1);
    assert_eq!(held.last_checks, BTreeMap::new());
}

#[test]
fn a_session_with_no_token_keeps_patch_ids_too() {
    let access = GitHubAccess {
        transport: Box::new(Canned(Err(Unreachable::Offline))),
        token_from: NO_GH,
    };
    let head = PatchReads::Read(vec![patches_of("4567def", 'a')]);

    let (heard, held) = keeping_patches("keeps-patches-offline", access, vec![head]);

    assert_eq!(heard[0].items, [8]);
    assert_eq!(held.patches.len(), 1);
}

#[test]
fn a_new_session_forgets_why_the_last_run_couldnt_read_patch_ids() {
    let head = PatchReads::Read(vec![patches_of("4567def", 'a')]);

    let (_, held) = keeping_patches("forgets-unchecked", replying(vec![signed_in()]), vec![head]);

    assert_eq!(held.unchecked, BTreeMap::new());
}

#[test]
fn once_both_sides_are_read_the_session_keeps_them_as_the_prs_last_check() {
    let both = vec![patches_of("0123abc", 'a'), patches_of("4567def", 'c')];

    let (_, held) = keeping_patches(
        "keeps-last-check",
        replying(vec![signed_in()]),
        vec![PatchReads::Read(both)],
    );

    let last = LastCheck {
        reviewed: CommitSha::parse("0123abc").unwrap(),
        head: CommitSha::parse("4567def").unwrap(),
    };
    assert_eq!(held.last_checks, BTreeMap::from([(8, last)]));
}

#[test]
fn the_session_dates_the_first_stale_finding_of_a_review_before_the_window_hears_of_it() {
    let both = vec![patches_of("0123abc", 'a'), patches_of("4567def", 'c')];
    let before = OffsetDateTime::now_utc().replace_nanosecond(0).unwrap();

    let (_, held) = keeping_patches(
        "dates-stale-finding",
        replying(vec![signed_in()]),
        vec![PatchReads::Read(both)],
    );

    let found = &held.stale_found[&8];
    assert_eq!(found.reviewed, CommitSha::parse("0123abc").unwrap());
    assert!(before <= found.at && found.at <= OffsetDateTime::now_utc());
}

#[test]
fn the_session_keeps_the_commits_the_mirror_cant_read_and_tells_the_window_of_each_change() {
    let refused = UncheckedCommit {
        of: on_main("0123abc"),
        reason: CantCheck::Refused,
    };
    let handed = vec![
        PatchReads::Unchecked(vec![refused.clone()]),
        PatchReads::Unchecked(vec![]),
        PatchReads::Unchecked(vec![refused]),
    ];

    let (heard, held) = keeping_patches("keeps-unchecked", replying(vec![signed_in()]), handed);

    let items: Vec<Vec<u32>> = heard.into_iter().map(|change| change.items).collect();
    assert_eq!(items, [[8], [8], [8]]);
    assert_eq!(
        held.unchecked,
        BTreeMap::from([(on_main("0123abc"), CantCheck::Refused)])
    );
    // A check that can't run finds the Review stale, and the session dates it.
    assert_eq!(held.stale_found.keys().collect::<Vec<_>>(), [&8]);
}

#[test]
fn patch_ids_the_store_cant_keep_show_its_problem() {
    for reads in [PatchReads::Read(vec![]), PatchReads::Unchecked(vec![])] {
        let (told, github) = telling(&mut quiet_board());
        let store = Store::stopped();

        keep_patches(&told, &store, &gcmon(), reads);

        assert_eq!(problems_for_the_status_bar(&github), [ErrorKind::Internal]);
    }
}

#[test]
fn reads_the_store_kept_reach_the_window_though_it_cant_keep_the_finding() {
    let store = empty_store();
    store.remember_login(&me()).unwrap();
    let stored = stored_pr(8, "some-author", "4567def");
    store.save_pull_requests(&gcmon(), vec![stored]).unwrap();
    let reviewed = CommitSha::parse("0123abc").unwrap();
    let content = (
        reviewed_by("octocat", &reviewed),
        OffsetDateTime::UNIX_EPOCH,
    );
    store
        .save_pull_request_content(&gcmon(), 8, content)
        .unwrap();
    let (passed, changes) = mpsc::channel();
    let mut board = quiet_board().with_changes(Box::new(move |change: &Change| {
        passed.send(change.clone()).unwrap();
    }));
    let (told, github) = telling(&mut board);
    // The calls go: the patch-ids, the facts, the last check, then the finding, which goes missing.
    let (lossy, _calls) = store.failing_at_call(Some(3));
    let both = vec![patches_of("0123abc", 'a'), patches_of("4567def", 'c')];

    keep_patches(&told, &lossy, &gcmon(), PatchReads::Read(both));

    assert_eq!(problems_for_the_status_bar(&github), [ErrorKind::Internal]);
    let heard: Vec<Vec<u32>> = changes.try_iter().map(|change| change.items).collect();
    assert_eq!(heard, [[8]]);
    let held = store.work_facts(&gcmon()).unwrap().reviews;
    assert_eq!(held.patches.len(), 2);
    assert_eq!(held.stale_found, BTreeMap::new());
}

/// A PR's Conversation with one Review by `author`, of `commit`.
fn reviewed_by(author: &str, commit: &CommitSha) -> PullRequestContent {
    PullRequestContent {
        conversation: empty_conversation(),
        reviews: vec![Review {
            id: None,
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
    let checks = passed_on(messages);
    let refreshes = Refreshes {
        intervals: HOURLY,
        incoming,
        ..unheard_refreshes()
    };
    let github = github(answered(200, GCMON));
    let mut syncs = Syncs::new(refreshes.settings.clone());
    syncs.add(gcmon());
    let due = Due::starting_at(Instant::now(), HOURLY);

    let answer = thread::scope(|scope| {
        let asking = scope.spawn(|| checks.check(gcmon()));
        // A step can run only after the session answers the check, or this would hang.
        while !asking.is_finished() {
            let turn = refreshes.next_turn(&github, &mut syncs, due);
            assert!(matches!(turn, Turn::SyncStep | Turn::Asked), "{turn:?}");
        }
        asking.join().unwrap()
    });

    assert_eq!(answer.unwrap().open_prs, 3);
}

#[test]
fn a_fetch_of_an_unstored_item_goes_ahead_of_the_next_step_of_a_sync() {
    let (messages, incoming) = mpsc::channel();
    let fetching = passed_on(messages);
    let refreshes = Refreshes {
        intervals: HOURLY,
        incoming,
        ..unheard_refreshes()
    };
    let github = github(answered(200, GCMON));
    let mut syncs = Syncs::new(refreshes.settings.clone());
    syncs.add(gcmon());
    let due = Due::starting_at(Instant::now(), HOURLY);

    let (turn, answer) = thread::scope(|scope| {
        let asking = scope.spawn(|| fetching.fetch_unstored(gcmon(), 7));
        // Steps run until the message comes, and the next turn takes it.
        let turn = loop {
            let turn = refreshes.next_turn(&github, &mut syncs, due);
            if !matches!(turn, Turn::SyncStep) {
                break turn;
            }
        };
        turn_reply(&turn).send(ItemFetch::Issue).unwrap();
        (turn, asking.join().unwrap())
    });

    assert!(matches!(turn, Turn::FetchUnstored(ref item, _) if *item == issue_7()));
    assert_eq!(answer, ItemFetch::Issue);
}

/// The reply of a turn that fetches an unstored item.
fn turn_reply(turn: &Turn) -> &Sender<ItemFetch> {
    let Turn::FetchUnstored(_, reply) = turn else {
        panic!("{turn:?}");
    };
    reply
}

#[test]
fn a_fetched_unstored_item_tells_the_window_and_stays_in_the_database() {
    let (folders, folder) = temp_folders("fetch-unstored");
    let (board, changes) = changes_board();
    let access = replying(vec![
        signed_in(),
        item_answer(&gcmon(), issue_item(7, &gcmon())),
    ]);
    let session = Session::start(board, Ok(folders.clone()), reaching(access), HOURLY);

    let fetched = session.fetching().fetch_unstored(gcmon(), 7);
    session.stop(PLENTY);

    assert_eq!(fetched, ItemFetch::Issue);
    let change = Change {
        repository: gcmon(),
        items: vec![7],
    };
    assert!(changes.try_iter().any(|heard| heard == change));
    let db = OpenStore::of(folders.database(), folder);
    let items = db.reader.titled_items(&gcmon()).unwrap();
    db.close();
    assert_eq!(
        items.iter().map(|item| item.number).collect::<Vec<_>>(),
        [7]
    );
}

#[test]
fn an_unstored_item_offline_is_unfetched_shows_offline_and_asks_nothing_after() {
    let (folders, _folder) = temp_folders("fetch-unstored-offline");
    let (board, seen) = watched_board();
    let (script, asked) = Scripted::new(vec![signed_in(), Err(Unreachable::Offline)]);
    let access = GitHubAccess {
        transport: Box::new(script),
        token_from: SIGNED_IN,
    };
    let session = Session::start(board, Ok(folders), reaching(access), HOURLY);

    let fetched = session.fetching().fetch_unstored(gcmon(), 7);
    let again = session.fetching().fetch_unstored(gcmon(), 8);
    session.stop(PLENTY);

    assert_eq!([fetched, again], [UNFETCHED_OFFLINE; 2]);
    // Once the status says offline, a fetch keeps nobody waiting on a request bound to fail.
    let fetches = asked
        .try_iter()
        .filter(|(operation, _)| operation == "FetchItem");
    assert_eq!(fetches.count(), 1);
    let offline_as = Status::from(Account::OfflineAs {
        last_login: Login::new("octocat"),
    });
    assert_eq!(announced(&seen).last(), Some(&offline_as));
}

#[test]
fn a_fetched_unstored_pr_in_a_syncing_repository_hands_its_head_to_the_mirror() {
    let (folders, _folder) = temp_folders("fetch-unstored-head");
    after_a_finished_sync(&folders);
    let (access, asked) = quiet_repository();
    let (mirrors, heard) = HeardRequests::new();
    let connections = Connections::new(access, mirrors, listed(vec![plain_gcmon()]));
    let session = Session::start(quiet_board(), Ok(folders), connections, HOURLY);
    session.fetching().sync(gcmon());
    wait_for(&asked, "RefreshRepository");

    let fetched = session.fetching().fetch_unstored(gcmon(), 7);
    let handed = heard.asked_through("saved heads #7", PLENTY);
    session.stop(PLENTY);

    assert_eq!(fetched, ItemFetch::PullRequest);
    let saved = "some-maintainer/gcmon saved heads #7 at 89abcde on main";
    assert!(handed.iter().any(|line| line == saved), "{handed:?}");
}

#[test]
fn a_number_the_session_fetches_that_names_nothing_is_missing() {
    let (folders, _folder) = temp_folders("fetch-unstored-missing");
    let access = replying(vec![signed_in(), item_missing()]);
    let session = Session::start(quiet_board(), Ok(folders), reaching(access), HOURLY);

    let fetched = session.fetching().fetch_unstored(gcmon(), 7);
    session.stop(PLENTY);

    let missing = ItemFetch::Unfetched {
        why: ItemUnfetched::Missing,
    };
    assert_eq!(fetched, missing);
}

#[test]
fn a_session_with_no_token_answers_a_fetch_of_an_unstored_item_offline() {
    let (folders, _folder) = temp_folders("fetch-unstored-no-token");
    let access = GitHubAccess {
        transport: Box::new(Canned(Err(Unreachable::Offline))),
        token_from: NO_GH,
    };
    let session = Session::start(quiet_board(), Ok(folders), reaching(access), HOURLY);

    let fetched = session.fetching().fetch_unstored(gcmon(), 7);
    session.stop(PLENTY);

    assert_eq!(fetched, UNFETCHED_OFFLINE);
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
    let intervals = Intervals {
        status: Duration::from_millis(50),
        ..HOURLY
    };
    let (threads, requests) = two_threads(intervals, mirrors());
    requests.sync(gcmon());
    let github = threads.github();
    let (passed, seen) = mpsc::channel();
    let mut shown = 0;
    let mut board = StatusBoard::new(Box::new(move |status: &Status| {
        passed.send(status.clone()).unwrap();
        shown += 1;
        // Back online, the sync would step at the next turn, so the stop has to be there first.
        if shown == 3 {
            github.send(GitHubMessage::Stop).unwrap();
        }
    }));

    threads.watch(&mut board, &empty_store(), access);

    let offline_as = Status::from(Account::OfflineAs {
        last_login: Login::new("octocat"),
    });
    assert_eq!(announced(&seen), [octocat(), offline_as, octocat()]);
}

#[test]
fn a_sync_waits_until_tsugiki_knows_who_the_user_is() {
    let (access, up, _asked) = outage();
    // GitHub can't be reached for the first question about the viewer, and answers once the status says so.
    up.store(false, Ordering::Relaxed);
    let back = move |_| up.store(true, Ordering::Relaxed);
    let (board, heard) = heard_board(back, |_| {});
    let (folders, _folder) = temp_folders("late-viewer");
    let session = Session::start(
        board,
        Ok(folders.clone()),
        reaching(access),
        Intervals {
            status: Duration::from_millis(50),
            ..HOURLY
        },
    );
    let reader = session.reader();

    session.fetching().sync(gcmon());
    while !matches!(heard.recv().unwrap(), Heard::Changed(_)) {}
    let workbench = Workbench::read(
        &reader,
        &plain_gcmon(),
        &WorkbenchView::Whole,
        ListOrder::Attention,
    )
    .unwrap();
    session.stop(PLENTY);

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
        pass_on(&self.asked, request.operation.to_owned());
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
        token_from: SIGNED_IN,
    };
    (access, up, seen)
}

/// The news a board passed on to the app, in order.
#[derive(Debug, PartialEq, Eq)]
enum Heard {
    Status(Status),
    Changed(Vec<u32>),
}

/// A board that passes on its statuses and the changes, in one line. It runs `on_status` with how many
/// statuses it has shown, and `on_change` with how many changes.
fn heard_board(
    mut on_status: impl FnMut(usize) + Send + 'static,
    on_change: impl FnMut(usize) + Send + 'static,
) -> (StatusBoard, Receiver<Heard>) {
    let (heard, seen) = mpsc::channel();
    let changes = heard.clone();
    let mut shown = 0;
    // Either thread of a session can tell of a change, so the count is behind a lock.
    let changed = std::sync::Mutex::new((0, on_change));
    let board = StatusBoard::new(Box::new(move |status: &Status| {
        heard.send(Heard::Status(status.clone())).unwrap();
        shown += 1;
        on_status(shown);
    }))
    .with_changes(Box::new(move |change: &Change| {
        changes.send(Heard::Changed(change.items.clone())).unwrap();
        let mut held = changed.lock().unwrap();
        held.0 += 1;
        let count = held.0;
        (held.1)(count);
    }));
    (board, seen)
}

/// The next status the board passed on, skipping the changes.
fn next_status(heard: &Receiver<Heard>) -> Status {
    loop {
        if let Heard::Status(status) = heard.recv_timeout(PLENTY).unwrap() {
            return status;
        }
    }
}

#[test]
fn a_refresh_while_offline_comes_back_online_before_the_next_status_check() {
    let (access, up, _asked) = outage();
    let (mirrors, wanted) = HeardRequests::new();
    let (threads, requests) = two_threads(HOURLY, mirrors);
    requests.sync(gcmon());
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
    // Once the sync saves, the open Issue's read comes round. The GitHub thread tells of the save, then takes
    // the sync's next step unless a message waits, so the read and the stop go straight to its queue: by the
    // session's thread, they could come after that step.
    let github = threads.github();
    let on_change = move |changed| {
        if changed == 1 {
            let issue_9 = Fetched::Issue(Item {
                repository: gcmon(),
                number: 9,
            });
            github.send(GitHubMessage::Fetch(issue_9)).unwrap();
            github.send(GitHubMessage::Stop).unwrap();
        }
    };
    let (mut board, heard) = heard_board(on_status, on_change);

    threads.watch(&mut board, &empty_store(), access);

    let offline_as = Status::from(Account::OfflineAs {
        last_login: Login::new("octocat"),
    });
    let reconnecting_as = Status::from(Account::ReconnectingAs {
        last_login: Login::new("octocat"),
    });
    assert_eq!(
        heard.try_iter().collect::<Vec<_>>(),
        [
            Heard::Status(octocat()),
            Heard::Status(offline_as),
            Heard::Status(reconnecting_as),
            Heard::Status(octocat()),
            Heard::Changed(vec![7]),
            Heard::Changed(vec![9]),
        ]
    );
    let heads: Vec<String> = (wanted.asked().into_iter())
        .filter(|line| line.contains("wants heads"))
        .collect();
    assert_eq!(
        heads,
        ["some-maintainer/gcmon wants heads #7 at 89abcde on main"]
    );
}

#[test]
fn a_refresh_offline_from_the_start_says_connecting_until_github_fails_again() {
    let (threads, requests) = two_threads(HOURLY, mirrors());
    let on_status = move |shown| {
        if shown == 1 {
            requests.refresh();
            requests.stop_session();
        }
    };
    let (mut board, heard) = heard_board(on_status, |_| {});

    threads.watch(&mut board, &empty_store(), offline());

    assert_eq!(
        heard.try_iter().collect::<Vec<_>>(),
        [
            Heard::Status(Account::Offline.into()),
            Heard::Status(Account::Reconnecting.into()),
            Heard::Status(Account::Offline.into()),
        ]
    );
}

#[test]
fn a_refresh_while_github_is_still_away_says_connecting_then_offline_and_runs_no_step() {
    let (access, up, asked) = outage();
    let (threads, requests) = two_threads(HOURLY, mirrors());
    requests.sync(gcmon());
    let on_status = move |shown| match shown {
        1 => up.store(false, Ordering::Relaxed),
        2 => {
            requests.refresh();
            requests.stop_session();
        }
        _ => {}
    };
    let (mut board, heard) = heard_board(on_status, |_| {});

    threads.watch(&mut board, &empty_store(), access);

    let offline_as = Status::from(Account::OfflineAs {
        last_login: Login::new("octocat"),
    });
    let reconnecting_as = Status::from(Account::ReconnectingAs {
        last_login: Login::new("octocat"),
    });
    assert_eq!(
        heard.try_iter().collect::<Vec<_>>(),
        [
            Heard::Status(octocat()),
            Heard::Status(offline_as.clone()),
            Heard::Status(reconnecting_as),
            Heard::Status(offline_as),
        ]
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
        token_from: NO_GH,
    };
    let (folders, _folder) = temp_folders("no-token-readable");
    let session = Session::start(quiet_board(), Ok(folders.clone()), reaching(access), HOURLY);
    let requests = session.fetching();

    requests.sync(gcmon());
    let checked = requests.check(gcmon());
    let facts = session.reader().work_facts(&gcmon());
    session.stop(PLENTY);

    assert_eq!(checked.unwrap_err().kind, ErrorKind::Offline);
    assert_eq!(facts.unwrap().pull_requests, []);
}

#[test]
fn a_session_that_has_ended_ignores_a_sync_and_a_refresh() {
    let (messages, incoming) = mpsc::channel();
    drop(incoming);
    let requests = sending(messages, mirrors());

    requests.sync(gcmon());
    requests.refresh();
}

/// Answers every question with a Repository that has nothing new, and says which it was asked.
struct QuietRepository {
    asked: Sender<String>,
}

impl Transport for QuietRepository {
    fn post(&self, _: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        pass_on(&self.asked, request.operation.to_owned());
        if request.operation == "Viewer" {
            return reply_ok(r#"{"data":{"viewer":{"login":"octocat"}}}"#);
        }
        // GitHub has nothing new to say of any PR a probe asks about.
        if request.operation == "ProbePullRequests" {
            return answer(json!({ "repository": {} }), RATE_LIMIT);
        }
        // A number the user fetches names PR 7.
        if request.operation == "FetchItem" {
            return item_answer(&gcmon(), pr_item(7));
        }
        let none = json!({ "pageInfo": page_info(None), "nodes": [] });
        let repository = json!({
            "openIssues": { "totalCount": 900 },
            "openPrs": { "totalCount": 0 },
            "issues": none,
            "pullRequests": none,
        });
        answer(json!({ "repository": repository }), RATE_LIMIT)
    }
}

/// When the sync that [`after_a_finished_sync`] keeps started and finished.
fn finished_sync_at() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_000_000_000).unwrap()
}

/// A store that says `gcmon` finished a sync at [`finished_sync_at`], in `folders`.
fn after_a_finished_sync(folders: &Folders) {
    let at = finished_sync_at();
    let store = Store::open(&folders.database()).unwrap();
    let state = SyncState {
        started_at: at,
        finished_at: at,
        check: ChangeCheck::Skipped,
        refresh_under_way: false,
    };
    store.save_sync_state(&gcmon(), state).unwrap();
    store.close();
}

fn quiet_repository() -> (GitHubAccess, Receiver<String>) {
    let (asked, seen) = mpsc::channel();
    let access = GitHubAccess {
        transport: Box::new(QuietRepository { asked }),
        token_from: SIGNED_IN,
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
    let (folders, _folder) = temp_folders("opened");
    after_a_finished_sync(&folders);
    let at = finished_sync_at();
    let (access, asked) = quiet_repository();
    let (board, last_syncs) = last_sync_board();
    let session = Session::start(board, Ok(folders.clone()), reaching(access), HOURLY);

    session.fetching().sync(gcmon());
    let shown = last_syncs.recv_timeout(PLENTY).unwrap();
    wait_for(&asked, "RefreshRepository");
    let after = last_syncs.recv_timeout(PLENTY).unwrap();
    session.stop(PLENTY);

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
fn opening_a_repository_has_its_refresh_read_ahead_what_one_before_this_launch_owed() {
    let (folders, _folder) = temp_folders("opened-owed");
    after_a_finished_sync(&folders);
    let store = Store::open(&folders.database()).unwrap();
    store.remember_login(&Login::new("octocat")).unwrap();
    let stored = vec![stored_pr(8, "octocat", "4567def")];
    store.save_pull_requests(&gcmon(), stored).unwrap();
    store.owe_read_ahead(&gcmon(), vec![8]).unwrap();
    store.close();
    let (access, asked) = quiet_repository();
    let session = Session::start(quiet_board(), Ok(folders.clone()), reaching(access), HOURLY);

    session.fetching().sync(gcmon());
    wait_for(&asked, "RefreshRepository");
    wait_for(&asked, "FetchPullRequestContent");
    session.stop(PLENTY);
}

/// GitHub as [`QuietRepository`] answers it, which holds its first answer to `operation` back until the test
/// says go, and says when it got there.
struct HeldAt {
    quiet: QuietRepository,
    operation: &'static str,
    reached: Sender<()>,
    go: Receiver<()>,
    held: Cell<bool>,
}

impl Transport for HeldAt {
    fn post(&self, token: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
        if request.operation == self.operation && !self.held.replace(true) {
            let went = self.reached.send(()).is_ok() && self.go.recv_timeout(PLENTY).is_ok();
            // A test that never says go fails in its own wait.
            if !went {
                return Err(Unreachable::Offline);
            }
        }
        self.quiet.post(token, request)
    }
}

/// gcmon's settings, with the Bugs Filter switched on the Workbench by `switch`, which is empty or says so.
fn gcmon_toml(switch: &str) -> String {
    format!(
        "[[repositories]]\nname = \"some-maintainer/gcmon\"\n\n[[repositories.filters]]\nname = \"Bugs\"\n\
         query = \"label:bug\"\n{switch}"
    )
}

/// Starts a session on gcmon, which finished a sync, with a settings owner whose `settings.toml` lists it
/// with the Bugs Filter off. The store owes a read ahead of PR 8, a stranger's, which only that Filter
/// reaches. The window opens gcmon, and GitHub holds its first answer to `held` back until `save` has
/// switched the Filter on. Returns once the session read PR 8's Conversation ahead, which only settings
/// read after the save would have it do, and fails after [`PLENTY`] without.
fn reading_ahead_once_saved(name: &str, held: &'static str, save: fn(&SettingsOwner, &Folders)) {
    let (folders, _folder) = temp_folders(name);
    after_a_finished_sync(&folders);
    let store = Store::open(&folders.database()).unwrap();
    store.remember_login(&me()).unwrap();
    let stored = vec![stored_pr(8, "some-author", "4567def")];
    store.save_pull_requests(&gcmon(), stored).unwrap();
    let found = FilterResults {
        numbers: [8].into(),
        problem: None,
    };
    store
        .save_filter_results(&gcmon(), &bugs_query(), found)
        .unwrap();
    store.owe_read_ahead(&gcmon(), vec![8]).unwrap();
    store.close();
    fs::write(folders.settings_file(), gcmon_toml("")).unwrap();
    let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders.clone())));
    let (asked, seen) = mpsc::channel();
    let (reached, at_held) = mpsc::channel();
    let (go, gone) = mpsc::channel();
    let access = GitHubAccess {
        transport: Box::new(HeldAt {
            quiet: QuietRepository { asked },
            operation: held,
            reached,
            go: gone,
            held: Cell::new(false),
        }),
        token_from: SIGNED_IN,
    };
    let connections = Connections::new(access, mirrors(), owner.reader());
    let session = Session::start(quiet_board(), Ok(folders.clone()), connections, HOURLY);
    // As `Calls::launched` does.
    let told = session.fetching();
    owner.tell_session(Box::new(move |save| told.settings_changed(save)));

    session.fetching().sync(gcmon());
    at_held.recv_timeout(PLENTY).unwrap();
    save(&owner, &folders);
    go.send(()).unwrap();
    wait_for(&seen, "FetchPullRequestContent");
    session.stop(PLENTY);
}

#[test]
fn a_save_between_the_windows_open_and_the_session_taking_it_reaches_the_sync() {
    // The session asks who is signed in before it takes the open, which waits in its queue meanwhile.
    reading_ahead_once_saved("saved-before-sync", "Viewer", |owner, _| {
        let saved = owner.put_on_workbench(gcmon(), bugs_query(), OnWorkbench::On);
        assert!(saved.is_ok(), "{saved:?}");
    });
}

#[test]
fn a_hand_edit_to_an_opened_repository_reaches_its_syncs_next_step() {
    // The refresh's poll, the step before its read ahead.
    reading_ahead_once_saved(
        "hand-edit-mid-sync",
        "RefreshRepository",
        |owner, folders| {
            fs::write(folders.settings_file(), gcmon_toml("on_workbench = true\n")).unwrap();
            let reread = owner.reread();
            assert!(reread.is_ok(), "{reread:?}");
        },
    );
}

#[test]
fn opening_a_repository_that_never_synced_says_so() {
    let (folders, _folder) = temp_folders("opened-new");
    let (access, _asked) = quiet_repository();
    let (board, last_syncs) = last_sync_board();
    let session = Session::start(board, Ok(folders.clone()), reaching(access), HOURLY);

    session.fetching().sync(gcmon());
    let shown = last_syncs.recv_timeout(PLENTY).unwrap();
    session.stop(PLENTY);

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

    assert!(showing(&seen).problem.is_some());
}

#[test]
fn a_click_refreshes_the_open_repository_again() {
    let (folders, _folder) = temp_folders("click");
    after_a_finished_sync(&folders);
    let (access, asked) = quiet_repository();
    let (board, last_syncs) = last_sync_board();
    let session = Session::start(board, Ok(folders.clone()), reaching(access), HOURLY);

    session.fetching().sync(gcmon());
    wait_for(&asked, "RefreshRepository");
    // Once when the window opened it, and once when that refresh finished. A click during a refresh
    // adds nothing, because that refresh is reading what the click asks for.
    wait_for_message(&last_syncs, PLENTY);
    wait_for_message(&last_syncs, PLENTY);
    session.fetching().refresh();
    wait_for(&asked, "RefreshRepository");
    session.stop(PLENTY);
}

#[test]
fn the_timer_refreshes_the_open_repository_again() {
    let (folders, _folder) = temp_folders("timer");
    after_a_finished_sync(&folders);
    let (access, asked) = quiet_repository();
    let every_moment = Intervals {
        repository: Duration::from_millis(10),
        ..HOURLY
    };
    let session = Session::start(
        quiet_board(),
        Ok(folders.clone()),
        reaching(access),
        every_moment,
    );

    session.fetching().sync(gcmon());
    wait_for(&asked, "RefreshRepository");
    wait_for(&asked, "RefreshRepository");
    session.stop(PLENTY);
}

/// A session that reads each item the user can see again 10 ms after its last read, and the operations
/// it asks GitHub.
fn reading_on_screen_often(folders: &Folders) -> (Session, Receiver<String>) {
    let (access, asked) = quiet_repository();
    let often = Intervals {
        on_screen: Duration::from_millis(10),
        ..HOURLY
    };
    let session = Session::start(quiet_board(), Ok(folders.clone()), reaching(access), often);
    (session, asked)
}

/// What `asked` hears within `wait`.
fn asked_within(asked: &Receiver<String>, wait: Duration) -> Vec<String> {
    received_until(asked, Instant::now() + wait).collect()
}

#[test]
fn the_session_reads_a_pr_on_screen_again_after_its_minute() {
    let (folders, _folder) = temp_folders("on-screen");
    let (session, asked) = reading_on_screen_often(&folders);

    session.fetching().fetch_pull_request(gcmon(), 7);
    // GitHub numbers Issues and PRs from one sequence, so `issue_7` names PR 7 too.
    let opened = ScreenMove::Opened(Fetched::PullRequest(issue_7()));
    session.fetching().screen_moved(opened);
    wait_for(&asked, "FetchPullRequestContent");
    wait_for(&asked, "FetchPullRequestContent");
    session.stop(PLENTY);
}

#[test]
fn the_session_reads_a_hidden_issue_no_more_until_it_shows_and_then_at_once() {
    let (folders, _folder) = temp_folders("on-screen-hidden");
    let (session, asked) = reading_on_screen_often(&folders);
    let fetching = session.fetching();

    fetching.fetch_issue(gcmon(), 7);
    fetching.screen_moved(ScreenMove::Opened(Fetched::Issue(issue_7())));
    fetching.screen_moved(ScreenMove::Hid(issue_7()));
    wait_for(&asked, "FetchIssueContent");
    let while_hidden = asked_within(&asked, Duration::from_millis(100));
    fetching.screen_moved(ScreenMove::Showed(issue_7()));
    wait_for(&asked, "FetchIssueContent");
    session.stop(PLENTY);

    assert!(
        !while_hidden
            .iter()
            .any(|asked| asked == "FetchIssueContent"),
        "{while_hidden:?}"
    );
}

#[test]
fn with_no_token_a_click_brings_on_a_read_of_it_and_a_stop_ends_the_wait() {
    let (messages, incoming) = mpsc::channel();
    let refreshes = Refreshes {
        intervals: HOURLY,
        incoming,
        ..unheard_refreshes()
    };
    messages.send(GitHubMessage::Refresh).unwrap();
    messages.send(GitHubMessage::Stop).unwrap();
    let wait = || {
        let mut syncs = Syncs::new(listed(vec![]));
        refreshes.wait_for_token(&mut quiet_board(), &empty_store(), &mut syncs)
    };

    let reads = [wait(), wait()];

    assert_eq!(reads, [Some(TokenRead::Clicked), None]);
}

#[test]
fn a_reader_answers_once_the_session_has_opened_the_database() {
    let (folders, _folder) = temp_folders("reader");
    let session = Session::start(
        quiet_board(),
        Ok(folders.clone()),
        reaching(offline()),
        HOURLY,
    );

    let facts = session.reader().work_facts(&gcmon());
    session.stop(PLENTY);

    assert_eq!(facts.unwrap().pull_requests, []);
}

#[test]
fn a_session_with_no_database_answers_its_readers_with_that() {
    let lost = Error::new(ErrorKind::Internal, "HOME isn't a full path.");
    let session = Session::start(quiet_board(), Err(lost), reaching(offline()), HOURLY);

    let refused = session.reader().work_facts(&gcmon());
    session.stop(PLENTY);

    assert_eq!(refused.unwrap_err().kind, ErrorKind::Internal);
}

fn wal_of(folders: &Folders) -> PathBuf {
    folders.data.join("tsugiki.db-wal")
}

#[test]
fn stopping_between_refreshes_closes_the_store() {
    let (folders, _folder) = temp_folders("stop-between");
    let (board, statuses) = watched_board();
    let session = Session::start(
        board,
        Ok(folders.clone()),
        reaching(octocat_answers()),
        HOURLY,
    );
    assert_eq!(statuses.recv().unwrap(), octocat());
    let waiting = fs::metadata(wal_of(&folders)).unwrap().len();
    assert!(waiting > 0, "the login is still in the -wal");

    session.stop(PLENTY);

    assert!(!wal_of(&folders).exists(), "closing moved it to the file");
    let store = Store::open(&folders.database()).unwrap();
    assert_eq!(store.last_login().unwrap(), Some(Login::new("octocat")));
    store.close();
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
    let (folders, _folder) = temp_folders("stop-in-request");
    let (asked, request) = mpsc::channel();
    let (answer, answering) = mpsc::channel();
    let access = GitHubAccess {
        transport: Box::new(Held {
            asked,
            answer: answering,
        }),
        token_from: SIGNED_IN,
    };
    let session = Session::start(quiet_board(), Ok(folders.clone()), reaching(access), HOURLY);
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
}

#[test]
fn stopping_a_session_that_has_already_finished_returns_at_once() {
    let lost = Error::new(ErrorKind::Internal, "HOME isn't a full path.");
    let session = Session::start(quiet_board(), Err(lost), reaching(offline()), HOURLY);
    assert_eq!(session.finished.recv(), Err(RecvError));

    session.stop(AN_HOUR);
}

#[test]
fn a_panic_before_the_github_thread_starts_says_tsugiki_stopped_checking() {
    // The first announcement panics, on the path that never reaches the GitHub thread.
    let (board, seen) = panicking_once();
    let lost = Error::new(ErrorKind::Internal, "HOME isn't a full path.");

    run_stopped(board, database(Err(lost)), octocat_answers());

    assert_eq!(stopped_by(showing(&seen)), Some(stopped_checking().message));
}

/// Why `status` says Tsugiki asks GitHub nothing more this run, with no account part, or nothing when it
/// says anything else (#1070).
fn stopped_by(status: Status) -> Option<String> {
    match status {
        Status {
            account: Account::Stopped,
            problem: Some(problem),
        } => Some(problem.message),
        _ => None,
    }
}

/// A board whose first announcement of a status panics, and where it sends the others.
fn panicking_once() -> (StatusBoard, mpsc::Receiver<Status>) {
    let (passed, seen) = mpsc::channel();
    let mut announcements = 0;
    let board = StatusBoard::new(Box::new(move |status: &Status| {
        announcements += 1;
        assert!(announcements > 1, "a panic in the GitHub thread");
        passed.send(status.clone()).unwrap();
    }));
    (board, seen)
}

#[test]
fn a_panic_in_the_github_thread_says_tsugiki_stopped_checking() {
    let (folders, _folder) = temp_folders("github-panic");
    // The first announcement is who signed in, on the GitHub thread.
    let (board, seen) = panicking_once();

    run_stopped(board, database(Ok(folders)), octocat_answers());

    assert_eq!(stopped_by(showing(&seen)), Some(stopped_checking().message));
}

/// A session whose GitHub thread panicked at its first status and said Tsugiki stopped checking, with what
/// the session asked of the Mirror worker, which isn't there.
fn github_stopped(name: &str) -> (Session, HeardRequests, TempPath) {
    let (folders, folder) = temp_folders(name);
    let (board, seen) = panicking_once();
    let (requests, mirrors) = HeardRequests::new();
    let connections = Connections {
        mirrors: requests,
        ..reaching(octocat_answers())
    };
    let session = Session::start(board, Ok(folders), connections, HOURLY);
    let stopped = seen.recv_timeout(PLENTY).unwrap();
    // The message prints nothing of the status, which CodeQL reads as holding the login.
    assert!(
        stopped_by(stopped) == Some(stopped_checking().message),
        "the status says Tsugiki stopped checking"
    );
    (session, mirrors, folder)
}

/// The kind and the words of the failure in `answer`, or nothing when it succeeded.
fn failure_of<T>(answer: Result<T, Error>) -> Option<(ErrorKind, String)> {
    answer.err().map(|problem| (problem.kind, problem.message))
}

/// What `fetching::answered` says once the thread it asks has stopped.
fn not_connected() -> Option<(ErrorKind, String)> {
    Some((
        ErrorKind::Offline,
        crate::fetching::NOT_CONNECTED.to_owned(),
    ))
}

/// Runs `ask` on a thread of its own and waits up to `PLENTY` for its answer, so a call that would wait for
/// good fails the test in place of hanging it.
fn beside<T: Send + 'static>(
    ask: impl FnOnce() -> T + Send + 'static,
) -> Result<T, RecvTimeoutError> {
    let (answer, answered) = mpsc::channel();
    #[expect(
        unused_results,
        reason = "it ends with the answer, or never, which the wait gives up on"
    )]
    thread::spawn(move || answer.send(ask()));
    answered.recv_timeout(PLENTY)
}

/// The GitHub thread owns its queue, so the queue goes when the thread stops, and a request sent after
/// fails at once (#1076).
#[test]
fn once_the_github_thread_stopped_the_mirrors_size_check_says_at_once_it_isnt_connected() {
    let (session, mirrors, _folder) = github_stopped("github-stopped-size");
    let mut size_of = mirrors.sizes();

    let size = beside(move || size_of(&gcmon()));
    session.stop(PLENTY);

    assert_eq!(failure_of(size.unwrap()), not_connected());
}

#[test]
fn once_the_github_thread_stopped_add_repositorys_check_says_at_once_it_isnt_connected() {
    let (session, _mirrors, _folder) = github_stopped("github-stopped-check");
    let fetching = session.fetching();

    let checked = beside(move || fetching.check(gcmon()));
    session.stop(PLENTY);

    assert_eq!(failure_of(checked.unwrap()), not_connected());
}

#[test]
fn once_the_github_thread_stopped_a_fetch_of_an_unstored_item_answers_offline_at_once() {
    let (session, _mirrors, _folder) = github_stopped("github-stopped-fetch");
    let fetching = session.fetching();

    let fetched = beside(move || fetching.fetch_unstored(gcmon(), 7));
    session.stop(PLENTY);

    assert_eq!(fetched, Ok(UNFETCHED_OFFLINE));
}

#[test]
fn a_panic_in_the_sessions_thread_has_the_github_thread_say_tsugiki_stopped_checking() {
    let (folders, _folder) = temp_folders("session-panic");
    let (board, seen) = watched_board();
    let board = board.with_changes(Box::new(|_: &Change| {
        panic!("a panic in the session's thread")
    }));
    let session = Session::start(board, Ok(folders), reaching(octocat_answers()), HOURLY);
    assert_eq!(seen.recv_timeout(PLENTY), Ok(octocat()));

    session.fetching().mark_unseen(
        gcmon(),
        Unseen {
            number: 7,
            archived: None,
        },
        None,
    );
    let stopped = seen.recv_timeout(PLENTY).unwrap();
    session.stop(PLENTY);

    assert_eq!(stopped_by(stopped), Some(stopped_checking().message));
}

/// The account said no token, which Try again would read again, but nothing hears the click any more.
#[test]
fn a_panic_in_the_sessions_thread_with_no_token_still_says_tsugiki_stopped_checking() {
    let (folders, _folder) = temp_folders("session-panic-no-token");
    let (board, seen) = watched_board();
    let board = board.with_changes(Box::new(|_: &Change| {
        panic!("a panic in the session's thread")
    }));
    let access = GitHubAccess {
        transport: Box::new(Canned(Err(Unreachable::Offline))),
        token_from: NO_GH,
    };
    let session = Session::start(board, Ok(folders), reaching(access), HOURLY);
    assert!(says_no_token(&seen.recv_timeout(PLENTY).unwrap()));

    session.fetching().mark_unseen(
        gcmon(),
        Unseen {
            number: 7,
            archived: None,
        },
        None,
    );
    let stopped = seen.recv_timeout(PLENTY).unwrap();
    session.stop(PLENTY);

    assert_eq!(stopped_by(stopped), Some(stopped_checking().message));
}

#[test]
fn starting_without_gh_keeps_the_problem_that_says_how_to_sign_in() {
    let (folders, _folder) = temp_folders("no-gh");
    let (board, seen) = watched_board();
    let access = GitHubAccess {
        transport: Box::new(Canned(Err(Unreachable::Offline))),
        token_from: NO_GH,
    };

    run_stopped(board, database(Ok(folders.clone())), access);

    assert!(says_no_token(&showing(&seen)));
    assert!(folders.database().exists());
}

#[test]
fn folders_that_cant_be_found_show_as_a_problem() {
    let (board, seen) = watched_board();
    let lost = Error::new(ErrorKind::Internal, "HOME isn't a full path.");

    run_stopped(board, database(Err(lost)), octocat_answers());

    assert_eq!(
        stopped_by(showing(&seen)).as_deref(),
        Some("HOME isn't a full path.")
    );
}

#[test]
#[expect(
    clippy::disallowed_types,
    reason = "a database from a newer Tsugiki has a version the store can't write, so the test sets it"
)]
fn a_newer_database_shows_as_a_problem_and_github_isnt_asked() {
    let (folders, _folder) = temp_folders("newer");
    let newer = rusqlite::Connection::open(folders.database()).unwrap();
    newer.pragma_update(None, "user_version", 9999).unwrap();
    drop(newer);
    let before = fs::read(folders.database()).unwrap();
    let (board, seen) = watched_board();

    run_stopped(board, database(Ok(folders.clone())), octocat_answers());

    let [only]: [Status; 1] = announced(&seen).try_into().unwrap();
    assert!(stopped_by(only).unwrap().contains("newer"));
    assert_eq!(fs::read(folders.database()).unwrap(), before);
}

/// The ways the window opens something of gcmon: the Repository, and a PR of it from quick open.
const OPENING_GCMON: [fn(&Fetching); 2] = [
    |fetching| fetching.sync(gcmon()),
    |fetching| fetching.fetch_pull_request(gcmon(), 10),
];

/// Runs the session's thread with a store whose links an older set of rules kept, in CPython and gcmon,
/// once the window has opened something of gcmon through `open`. Answers with the Repository of each Change,
/// which names no item, and the store. The session stops once it has heard of both. The refill asks GitHub
/// nothing, so it runs on the session's thread, whether the GitHub thread has a token or waits on a request.
fn refilled_watching(open: fn(&Fetching)) -> (Vec<RepositoryName>, Store) {
    let store = store_with_old_links(&[cpython(), gcmon()]);
    let (stop, incoming) = mpsc::channel();
    open(&sending(stop.clone(), mirrors()));
    let (heard, changes) = mpsc::channel();
    let count = std::sync::atomic::AtomicUsize::new(0);
    let mut board = quiet_board().with_changes(Box::new(move |change: &Change| {
        assert!(change.items.is_empty(), "{change:?}");
        heard.send(change.repository.clone()).unwrap();
        if count.fetch_add(1, Ordering::SeqCst) == 1 {
            stop.send(SessionMessage::Stop).unwrap();
        }
    }));
    let (told, _github) = telling(&mut board);
    let keeper = Keeper::new(
        incoming,
        unheard(),
        Reads {
            settings: listed(vec![plain_gcmon()]),
            reader: unopened_reader(),
        },
        told,
    );

    keeper.run(&store);

    (changes.try_iter().collect(), store)
}

/// The fixes a store holds once the refill of CPython and gcmon worked out their links under
/// `LinkRule::SET`.
fn refilled_fixes() -> Vec<(String, u32, u32)> {
    let fixed = |repository: RepositoryName| (repository.to_string(), 10, 5);
    vec![fixed(cpython()), fixed(gcmon())]
}

#[test]
fn a_session_refills_stale_links_the_open_repository_first_and_tells_the_window() {
    for open in OPENING_GCMON {
        let (heard, store) = refilled_watching(open);

        assert_eq!(heard, [gcmon(), cpython()]);
        assert_eq!(store.stale_links().unwrap(), None);
        assert_eq!(kept_fixes(&store), refilled_fixes());
    }
}
