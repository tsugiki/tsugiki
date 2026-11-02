//! The process's one logger, through the same paths the app uses. This file is its own process, so starting
//! the logger here can't affect any other test.

use std::fs;
use std::io;
use std::time::Duration;

use tsugiki_core::error::{Error, ErrorKind};
use tsugiki_core::github::{GitHub, GitHubAccess, Reply, Request, Transport, Unreachable};
use tsugiki_core::logfile;
use tsugiki_core::session::Session;
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
            body: "Blocked by your company's proxy".to_owned(),
        })
    }
}

#[test]
fn the_log_keeps_causes_and_hides_the_token_in_use() {
    let folder = std::env::temp_dir().join(format!("tsugiki-logfile-{}", std::process::id()));
    let path = folder.join("tsugiki.log");
    logfile::start(path.clone());
    logfile::start(folder.join("a second start changes nothing.log"));
    let read_after_flush = || {
        log::logger().flush();
        fs::read_to_string(&path).unwrap()
    };

    assert!(read_after_flush().contains("started"));

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
        Duration::from_secs(3600),
    );
    session.stop(Duration::from_secs(10));
    let file = read_after_flush();
    assert!(file.contains("the folders are lost"), "{file}");

    assert_eq!(
        fs::read_dir(&folder).unwrap().count(),
        2,
        "the log and the folder only"
    );
    fs::remove_dir_all(folder).unwrap();
}
