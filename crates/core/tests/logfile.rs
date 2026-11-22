//! The process's one logger, through the same paths the app uses. This file is its own process, so starting
//! the logger here can't affect any other test.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use tsugiki_core::error::{Error, ErrorKind};
use tsugiki_core::facts::pull_requests::NodeId;
use tsugiki_core::folders::Folders;
use tsugiki_core::github::replay::Replayer;
use tsugiki_core::github::{
    GitHub, GitHubAccess, Grant, Reply, Request, Selection, Transport, Unreachable, Write,
};
use tsugiki_core::mirror::{Announce, Mirrors, Remotes};
use tsugiki_core::read_only::WritableRepository;
use tsugiki_core::repository::RepositoryName;
use tsugiki_core::session::{Connections, Intervals, Session};
use tsugiki_core::settings::SettingsView;
use tsugiki_core::status::StatusBoard;
use tsugiki_core::store::Store;
use tsugiki_core::token::{GhOutput, Token, TokenFrom, TokenReads, read_token};
use tsugiki_core::version::Version;
use tsugiki_core::{latest, logfile};
use tsugiki_token::testing::{NO_GH, SLOW_GH, gh_alone, gh_prints, gh_token};

/// A token no shape gives away, so only hiding it by its value keeps it out of the log.
const NO_KNOWN_SHAPE: &str = "0123456789abcdef";
/// Another such token, in `GH_TOKEN`.
const IN_GH_TOKEN: &str = "fedcba9876543210";

fn gh_prints_a_token_of_no_known_shape() -> io::Result<GhOutput> {
    gh_prints(&format!("{NO_KNOWN_SHAPE}\n"))
}

fn gh_token_holds_a_token_of_no_known_shape() -> Option<String> {
    Some(IN_GH_TOKEN.to_owned())
}

/// Answers the way a corporate proxy might.
struct Proxy;

impl Transport for Proxy {
    fn post(&self, _: &Token, _: &Request<'_>) -> Result<Reply, Unreachable> {
        Ok(Reply {
            status: 403,
            reset: None,
            date: None,
            body: "Blocked by your company's proxy".to_owned(),
            grant: Grant::default(),
        })
    }
}

/// A second `tsg` starts the logger and exits, so it must leave the running Tsugiki's log alone.
#[expect(
    clippy::unwrap_used,
    reason = "a test fails by panicking, and this helper sits outside any #[test] function"
)]
fn a_run_says_it_started_only_when_told_to(path: &Path) {
    log::logger().flush();
    assert!(!path.exists());

    logfile::say_started(&Version::new(Some(
        "3a6dc89f0e1d2c3b4a5968778695a4b3c2d1e0f9",
    )));
    log::logger().flush();
    let started = format!("Tsugiki {} (3a6dc89) started", env!("CARGO_PKG_VERSION"));
    assert!(fs::read_to_string(path).unwrap().contains(&started));
}

/// Tsugiki takes a token from `GH_TOKEN` without asking gh, and the log hides it as it hides gh's.
#[expect(
    clippy::unwrap_used,
    reason = "a test fails by panicking, and this helper sits outside any #[test] function"
)]
fn a_token_from_gh_token_is_hidden_too(path: &Path) {
    let _token = read_token(gh_token(gh_token_holds_a_token_of_no_known_shape)).unwrap();
    log::warn!("GitHub refused {IN_GH_TOKEN} too");
    log::logger().flush();
    let file = fs::read_to_string(path).unwrap();
    assert!(!file.contains(IN_GH_TOKEN), "{file}");
    assert!(file.contains("GitHub refused [token] too"), "{file}");
}

/// The log says Tsugiki stopped a `gh` that didn't answer in time, while the status bar keeps its plain
/// words.
#[expect(
    clippy::unwrap_used,
    reason = "a test fails by panicking, and this helper sits outside any #[test] function"
)]
fn a_gh_that_doesnt_answer_in_time_is_logged_as_stopped(path: &Path) {
    let problem = read_token(SLOW_GH).unwrap_err();
    assert!(
        problem.message.contains("didn't answer"),
        "{}",
        problem.message
    );
    log::logger().flush();
    let file = fs::read_to_string(path).unwrap();
    assert!(
        file.contains("gh didn't answer within 30 s, so Tsugiki stopped it"),
        "{file}"
    );
}

/// Reads that fail the same way log their cause once, as each minute's read does without `gh` (#1009). A
/// read that fails another way logs its own, and so does a failed read after one that found a token. Another
/// line goes before each read, as the app's other threads log meanwhile, so the logger's skip of a line
/// that repeats the one before it can't hide a repeat.
fn a_failed_read_logs_its_cause_once_until_the_cause_changes(path: &Path) {
    let count = |line: &str| lines_with(path, line);
    let mut lines = 0;
    let mut reads = TokenReads::default();
    let mut found_a_token = |from: TokenFrom| {
        lines += 1;
        log::warn!("Meanwhile, another thread's line {lines}");
        reads.read(from).is_ok()
    };
    let cant_run = "Tsugiki can't run gh: ";
    let stopped = "gh didn't answer within 30 s, so Tsugiki stopped it";
    let (cant_run_before, stopped_before) = (count(cant_run), count(stopped));

    for _ in 0..3 {
        assert!(!found_a_token(NO_GH));
    }
    assert_eq!(count(cant_run), cant_run_before + 1);

    for _ in 0..2 {
        assert!(!found_a_token(SLOW_GH));
    }
    assert_eq!(count(stopped), stopped_before + 1);
    assert!(!found_a_token(NO_GH));
    assert_eq!(count(cant_run), cant_run_before + 2);

    assert!(found_a_token(gh_alone(gh_prints_a_token_of_no_known_shape)));
    assert!(!found_a_token(NO_GH));
    assert_eq!(count(cant_run), cant_run_before + 3);
}

/// A write whose selection the token crate refuses leaves the reason in the log, and says nothing was sent.
#[expect(
    clippy::unwrap_used,
    reason = "a test fails by panicking, and this helper sits outside any #[test] function"
)]
#[expect(
    clippy::disallowed_methods,
    reason = "a test of the transport, which a write reaches once it may go; this file builds without the \
              core's `testing` helpers"
)]
fn a_refused_write_says_why_though_nothing_was_sent(path: &Path, token: &Token) {
    let subject = NodeId::new("I_1".to_owned());
    let comment = Write::AddComment {
        subject: subject.clone(),
        body: "Thanks".to_owned(),
    };
    let second_field = Selection {
        payload: "{ clientMutationId } deleteRef(input: { refId: 1 }) { clientMutationId }",
        fragments: "",
    };
    let no_recordings = path.with_file_name("no recordings");
    let cpython = RepositoryName::parse("python/cpython").unwrap();
    let writable = WritableRepository::new(cpython, Some(subject), []).unwrap();
    let refused = Replayer::new(no_recordings).write(token, &writable, (&comment, second_field));
    assert_eq!(refused.unwrap_err(), Unreachable::Refused);
    log::logger().flush();
    let file = fs::read_to_string(path).unwrap();
    let says =
        "Tsugiki refused to send AddComment to GitHub: a write's payload isn't one selection set";
    assert!(file.contains(says), "{file}");
}

#[test]
fn the_log_keeps_causes_and_hides_the_token_in_use() {
    let folder = tsugiki_test_git::temp_path("logfile");
    let path = folder.join("tsugiki.log");
    logfile::start(path.clone());
    logfile::start(folder.join("a second start changes nothing.log"));
    let read_after_flush = || {
        log::logger().flush();
        fs::read_to_string(&path).unwrap()
    };

    a_run_says_it_started_only_when_told_to(&path);

    // The real token's words reach the log with the token hidden.
    let token = read_token(gh_alone(gh_prints_a_token_of_no_known_shape)).unwrap();
    log::warn!("GitHub refused Bearer {NO_KNOWN_SHAPE}");
    let file = read_after_flush();
    assert!(!file.contains(NO_KNOWN_SHAPE), "{file}");
    assert!(file.contains("GitHub refused Bearer [token]"), "{file}");

    a_token_from_gh_token_is_hidden_too(&path);

    // A failure's cause goes to the log while the status bar keeps its plain words.
    let problem = read_token(NO_GH).unwrap_err();
    assert!(
        problem.message.contains("gh auth login"),
        "{}",
        problem.message
    );
    let file = read_after_flush();
    assert!(file.contains("Tsugiki can't run gh: "), "{file}");

    a_gh_that_doesnt_answer_in_time_is_logged_as_stopped(&path);
    a_failed_read_logs_its_cause_once_until_the_cause_changes(&path);

    // A folder where the database should be: SQLite says why, and the user sees plain words.
    let in_the_way = folder.join("database");
    fs::create_dir_all(&in_the_way).unwrap();
    let problem = Store::open(&in_the_way).err().unwrap();
    let file = read_after_flush();
    let with_its_cause = format!("{}: ", problem.message);
    assert!(file.contains(&with_its_cause), "{file}");

    a_refused_write_says_why_though_nothing_was_sent(&path, &token);

    // What a proxy said, which the status bar has no room for.
    assert!(GitHub::new(Box::new(Proxy), token).viewer().is_err());
    let file = read_after_flush();
    assert!(
        file.contains("403: Blocked by your company's proxy"),
        "{file}"
    );

    // Each problem the status bar shows, including one no earlier line explains.
    run_session(Err(lost()), NO_GH, Duration::from_secs(3600), || true);
    let file = read_after_flush();
    assert!(file.contains("the folders are lost"), "{file}");

    assert_eq!(
        fs::read_dir(&folder).unwrap().count(),
        3,
        "the log, the folder in the way and the store's lock only"
    );

    the_github_thread_logs_why_its_reads_find_no_token_once(&path);
}

/// The count of the GitHub thread's asks of `gh` for its token, in the step that counts them.
static GH_ASKED: AtomicUsize = AtomicUsize::new(0);

/// A `gh` that isn't installed, which logs a line of its own each time it is asked, as another thread might
/// between two reads, so the logger's skip of a line that repeats the one before it can't hide a repeat.
fn gh_missing_and_counted() -> io::Result<GhOutput> {
    let asked = GH_ASKED.fetch_add(1, Ordering::SeqCst) + 1;
    log::warn!("gh asked {asked} times");
    Err(io::Error::from(io::ErrorKind::NotFound))
}

/// The GitHub thread keeps its reads of the token while it has none, so each minute's read without `gh`
/// adds no line once the first has said why (#1009).
fn the_github_thread_logs_why_its_reads_find_no_token_once(path: &Path) {
    let cant_run = "Tsugiki can't run gh: ";
    let cant_run_before = lines_with(path, cant_run);
    let folder = tsugiki_test_git::temp_path("logfile-session");
    let folders = Folders {
        data: folder.to_path_buf(),
        settings: folder.to_path_buf(),
        logs: folder.to_path_buf(),
    };
    let asked_thrice = || GH_ASKED.load(Ordering::SeqCst) >= 3;
    run_session(
        Ok(folders),
        gh_alone(gh_missing_and_counted),
        Duration::from_millis(10),
        asked_thrice,
    );
    assert!(asked_thrice());
    assert_eq!(lines_with(path, cant_run), cant_run_before + 1);
}

/// The error of folders Tsugiki couldn't find.
fn lost() -> Error {
    Error::new(ErrorKind::Internal, "the folders are lost")
}

/// Runs a session on `folders`, whose GitHub thread takes the token `from` and reads it again each
/// `status` while it finds none, until `done` holds or 10 s are up, then stops it. Its Mirrors have lost
/// their folders.
fn run_session(
    folders: Result<Folders, Error>,
    from: TokenFrom,
    status: Duration,
    done: impl FnMut() -> bool,
) {
    let mirrors = Mirrors::start(
        Err(lost()),
        Remotes::under(String::new()),
        Announce::cloning_only(Box::new(|_| {})),
    );
    let session = Session::start(
        StatusBoard::new(Box::new(|_| {})),
        folders,
        Connections {
            github: GitHubAccess {
                transport: Box::new(Proxy),
                token_from: from,
            },
            mirrors: mirrors.requests(),
            settings: latest::channel(SettingsView::default()).1,
        },
        Intervals {
            status,
            repository: Duration::from_secs(3600),
            on_screen: Duration::from_secs(3600),
        },
    );
    tsugiki_test_git::wait_until(Duration::from_secs(10), done);
    session.stop(Duration::from_secs(10));
    mirrors.stop(Duration::from_secs(10));
}

/// The count of `line` in the log at `path`, once the logger has written what it was sent.
#[expect(
    clippy::unwrap_used,
    reason = "a test fails by panicking, and this helper sits outside any #[test] function"
)]
fn lines_with(path: &Path, line: &str) -> usize {
    log::logger().flush();
    fs::read_to_string(path).unwrap().matches(line).count()
}
