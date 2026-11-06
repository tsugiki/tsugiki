//! The process's one logger, through the same paths the app uses. This file is its own process, so starting
//! the logger here can't affect any other test.

use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

use tsugiki_core::error::{Error, ErrorKind};
use tsugiki_core::github::{GitHub, GitHubAccess, Reply, Request, Transport, Unreachable};
use tsugiki_core::logfile;
use tsugiki_core::session::{Intervals, Session};
use tsugiki_core::status::StatusBoard;
use tsugiki_core::store::Store;
use tsugiki_core::token::{GhOutput, Token};

fn gh_prints_a_token_of_no_known_shape(_: &[&str]) -> io::Result<GhOutput> {
    Ok(GhOutput {
        success: true,
        stdout: "0123456789abcdef\n".to_owned(),
    })
}

fn gh_is_missing(_: &[&str]) -> io::Result<GhOutput> {
    Err(io::Error::from(io::ErrorKind::NotFound))
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
        })
    }
}

/// A failed run leaves its folder behind, and a later run can get the same process id.
#[expect(
    clippy::unwrap_used,
    reason = "a test fails by panicking, and this helper sits outside any #[test] function"
)]
fn clear_after_a_failed_run(folder: &Path) {
    if folder.exists() {
        fs::remove_dir_all(folder).unwrap();
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

    logfile::say_started();
    log::logger().flush();
    assert!(fs::read_to_string(path).unwrap().contains("started"));
}

#[test]
fn the_log_keeps_causes_and_hides_the_token_in_use() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../.temp/logfile-{}", std::process::id()));
    clear_after_a_failed_run(&folder);
    let path = folder.join("tsugiki.log");
    logfile::start(path.clone());
    logfile::start(folder.join("a second start changes nothing.log"));
    let read_after_flush = || {
        log::logger().flush();
        fs::read_to_string(&path).unwrap()
    };

    a_run_says_it_started_only_when_told_to(&path);

    // The real token's words reach the log with the token hidden.
    let token = Token::from_gh(gh_prints_a_token_of_no_known_shape).unwrap();
    log::warn!("GitHub refused Bearer {}", token.expose());
    let file = read_after_flush();
    assert!(!file.contains(token.expose()), "{file}");
    assert!(file.contains("GitHub refused Bearer [token]"), "{file}");

    // A failure's cause goes to the log while the status bar keeps its plain words.
    let problem = Token::from_gh(gh_is_missing).unwrap_err();
    assert!(
        problem.message.contains("gh auth login"),
        "{}",
        problem.message
    );
    let file = read_after_flush();
    assert!(file.contains("Tsugiki can't run gh: "), "{file}");

    // A folder where the database should be: SQLite says why, and the user sees plain words.
    let in_the_way = folder.join("database");
    fs::create_dir_all(&in_the_way).unwrap();
    let problem = Store::open(&in_the_way).err().unwrap();
    let file = read_after_flush();
    let with_its_cause = format!("{}: ", problem.message);
    assert!(file.contains(&with_its_cause), "{file}");

    // What a proxy said, which the status bar has no room for.
    let token = Token::from_gh(gh_prints_a_token_of_no_known_shape).unwrap();
    assert!(GitHub::new(Box::new(Proxy), token).viewer().is_err());
    let file = read_after_flush();
    assert!(
        file.contains("403: Blocked by your company's proxy"),
        "{file}"
    );

    // Each problem the status bar shows, including one no earlier line explains.
    let session = Session::start(
        StatusBoard::new(Box::new(|_| {})),
        Err(Error::new(ErrorKind::Internal, "the folders are lost")),
        GitHubAccess {
            transport: Box::new(Proxy),
            run_gh: gh_is_missing,
        },
        Intervals {
            status: Duration::from_secs(3600),
            repository: Duration::from_secs(3600),
        },
    );
    session.stop(Duration::from_secs(10));
    let file = read_after_flush();
    assert!(file.contains("the folders are lost"), "{file}");

    assert_eq!(
        fs::read_dir(&folder).unwrap().count(),
        3,
        "the log, the folder in the way and the store's lock only"
    );
    fs::remove_dir_all(folder).unwrap();
}
