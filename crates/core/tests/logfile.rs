//! The process's one logger, through the same paths the app uses. This file is its own process, so starting
//! the logger here can't affect any other test.

use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

use tsugiki_core::error::{Error, ErrorKind};
use tsugiki_core::facts::pull_requests::NodeId;
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
use tsugiki_core::token::{GhOutput, Token, read_token};
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
    let lost = || Err(Error::new(ErrorKind::Internal, "the folders are lost"));
    let mirrors = Mirrors::start(
        lost(),
        Remotes::under(String::new()),
        Announce::cloning_only(Box::new(|_| {})),
    );
    let session = Session::start(
        StatusBoard::new(Box::new(|_| {})),
        lost(),
        Connections {
            github: GitHubAccess {
                transport: Box::new(Proxy),
                token_from: NO_GH,
            },
            mirrors: mirrors.requests(),
            settings: latest::channel(SettingsView::default()).1,
        },
        Intervals {
            status: Duration::from_secs(3600),
            repository: Duration::from_secs(3600),
            on_screen: Duration::from_secs(3600),
        },
    );
    session.stop(Duration::from_secs(10));
    mirrors.stop(Duration::from_secs(10));
    let file = read_after_flush();
    assert!(file.contains("the folders are lost"), "{file}");

    assert_eq!(
        fs::read_dir(&folder).unwrap().count(),
        3,
        "the log, the folder in the way and the store's lock only"
    );
}
