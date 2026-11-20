use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, channel};

use tsugiki_test_git::pass_on;

use super::testing::{answered, offline, release};
use super::*;

/// Longer than any of these tests should wait on a working thread.
const PLENTY: Duration = Duration::from_secs(60);
/// What each check asks GitHub for.
const LATEST: &str = "/repos/tsugiki/tsugiki/releases/latest";

fn ask_once(answer: Result<Reply, Unreachable>) -> Result<Version, String> {
    let mut answer = Some(answer);
    let mut ask: AskPublic = Box::new(move |_| answer.take().unwrap());
    latest(&mut ask)
}

/// Answers each check with the next of `answers`, then as if offline, and says each time it is asked.
fn github(answers: Vec<Result<Reply, Unreachable>>) -> (AskPublic, Receiver<String>) {
    let (asked, asks) = channel();
    let mut answers = VecDeque::from(answers);
    let ask: AskPublic = Box::new(move |path| {
        pass_on(&asked, path.to_string());
        answers.pop_front().unwrap_or_else(offline)
    });
    (ask, asks)
}

fn hearing() -> (AnnounceNewVersion, Receiver<NewVersion>) {
    let (heard, news) = channel();
    let announce: AnnounceNewVersion = Box::new(move |new| pass_on(&heard, new.clone()));
    (announce, news)
}

#[test]
fn the_check_asks_for_the_latest_release() {
    let (ask, asks) = github(vec![]);
    let _check = ReleaseCheck::start(ask, PLENTY, Box::new(|_| {}));

    assert_eq!(asks.recv_timeout(PLENTY).unwrap(), LATEST);
}

#[test]
fn a_tag_reads_as_v_and_a_version() {
    assert_eq!(ask_once(release("v0.3.0")).unwrap(), Version::new(0, 3, 0));
    assert_eq!(
        ask_once(release("v1.2.3-rc.1")).unwrap(),
        Version::parse("1.2.3-rc.1").unwrap()
    );
}

#[test]
fn a_failed_check_says_why() {
    assert_eq!(ask_once(offline()).unwrap_err(), "no network");
    assert_eq!(
        ask_once(Err(Unreachable::CertificateDates("expired".to_owned()))).unwrap_err(),
        "expired"
    );
    assert_eq!(
        ask_once(answered(404, "")).unwrap_err(),
        "GitHub answered 404"
    );
    assert!(
        ask_once(answered(200, "<html>"))
            .unwrap_err()
            .starts_with("GitHub's answer doesn't read as a release: ")
    );
    assert!(
        ask_once(release("vnightly"))
            .unwrap_err()
            .starts_with("the tag vnightly isn't a version: ")
    );
    assert_eq!(
        ask_once(release("0.3.0")).unwrap_err(),
        "the tag 0.3.0 doesn't start with v"
    );
}

#[test]
fn a_newer_version_is_announced_with_its_release_page_at_start_up() {
    let (ask, _asks) = github(vec![release("v999.0.0")]);
    let (announce, news) = hearing();
    let _check = ReleaseCheck::start(ask, PLENTY, announce);

    let new = news.recv_timeout(PLENTY).unwrap();

    assert_eq!(new.version, "999.0.0");
    assert_eq!(
        new.page.as_str(),
        "https://github.com/tsugiki/tsugiki/releases/tag/v999.0.0"
    );
}

#[test]
fn each_check_announces_only_a_newer_version_it_hasnt_told() {
    let running = format!("v{}", env!("CARGO_PKG_VERSION"));
    let answers = vec![
        release(&running),
        release(&format!("{running}+win")),
        release("v0.0.1"),
        offline(),
        release("v999.0.0"),
        release("v999.0.0"),
        release("v999.0.0+win"),
        answered(500, ""),
        release("v1000.0.0"),
        // A release taken back leaves an older one latest, which the status bar doesn't go back to.
        release("v999.0.0"),
    ];
    let checks = answers.len();
    let (ask, asks) = github(answers);
    let (announce, news) = hearing();
    let check = ReleaseCheck::start(ask, Duration::from_millis(1), announce);

    // The check after the last answer has handled them all.
    for _ in 0..=checks {
        assert_eq!(asks.recv_timeout(PLENTY).unwrap(), LATEST);
    }
    drop(check);
    // The thread drops `ask` as it ends.
    for _ in asks.iter() {}
    let told: Vec<String> = news.try_iter().map(|new| new.version).collect();

    assert_eq!(told, ["999.0.0", "1000.0.0"]);
}

#[test]
fn dropping_the_check_ends_its_thread_without_waiting_for_the_next_check() {
    let (ask, asks) = github(vec![]);
    let check = ReleaseCheck::start(ask, PLENTY, Box::new(|_| {}));
    assert_eq!(asks.recv_timeout(PLENTY).unwrap(), LATEST);

    drop(check);

    // `ask` goes with the thread, well before the next check is due.
    assert_eq!(
        asks.recv_timeout(PLENTY / 2),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );
}
