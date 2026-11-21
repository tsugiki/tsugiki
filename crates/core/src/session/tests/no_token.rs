//! A session whose read of the token found none: the GitHub thread reads it again at the status check's
//! interval and on the user's click, and connects once a read finds one (#1004).

use super::*;
use crate::github::in_flight::Asking;
use crate::github::testing::in_flight_heard;
use crate::token::GhOutput;
use std::io;
use std::sync::Mutex;
use std::sync::mpsc::Receiver;
use tsugiki_token::testing::{gh_alone, gh_prints};

/// What the stand-in `gh` answers one ask for the token with.
enum GhAnswer {
    Token,
    /// It isn't installed.
    Missing,
    /// It didn't answer in time.
    TimedOut,
    /// It doesn't answer until the test drops the other end, and then it didn't answer in time.
    Held(Receiver<()>),
}

/// A `gh` that answers each ask with the next answer the test scripted, and with [`GhAnswer::Missing`] once
/// they run out, and tells the test of each ask. A `TokenFrom` takes a plain function, so each test keeps
/// one in a static of its own and passes a function that asks it.
struct ScriptedGh(Mutex<Option<GhScript>>);

struct GhScript {
    answers: VecDeque<GhAnswer>,
    asked: Sender<()>,
}

impl ScriptedGh {
    const fn new() -> Self {
        Self(Mutex::new(None))
    }

    /// Answers `answers` from now on, and gives where each ask is told.
    fn script(&self, answers: Vec<GhAnswer>) -> Receiver<()> {
        let (asked, told) = mpsc::channel();
        let script = GhScript {
            answers: answers.into(),
            asked,
        };
        *self.0.lock().unwrap() = Some(script);
        told
    }

    /// Tells the test of an ask, and gives the next answer.
    fn next(&self) -> GhAnswer {
        let mut script = self.0.lock().unwrap();
        let script = script.as_mut().expect("a script");
        pass_on(&script.asked, ());
        script.answers.pop_front().unwrap_or(GhAnswer::Missing)
    }

    fn answer(&self) -> io::Result<GhOutput> {
        match self.next() {
            GhAnswer::Token => gh_prints("gho_scripted"),
            GhAnswer::Missing => Err(io::Error::from(io::ErrorKind::NotFound)),
            GhAnswer::TimedOut => Err(io::Error::from(io::ErrorKind::TimedOut)),
            GhAnswer::Held(let_go) => {
                let RecvError = let_go.recv().unwrap_err();
                Err(io::Error::from(io::ErrorKind::TimedOut))
            }
        }
    }
}

/// GitHub, which answers each request that octocat is signed in, with the token from `gh`.
fn through(gh: fn(&[&str]) -> io::Result<GhOutput>) -> GitHubAccess {
    GitHubAccess {
        token_from: gh_alone(gh),
        ..octocat_answers()
    }
}

/// The two reads that find no token, and words the problem text holds for each.
fn found_none() -> [(GhAnswer, &'static str); 2] {
    [
        (GhAnswer::TimedOut, "didn't answer within 30 s"),
        (GhAnswer::Missing, "Install `gh` or run `gh auth login`"),
    ]
}

/// Whether `status` is a problem whose text holds `words` and names `GH_TOKEN` as the way past `gh`.
fn says(status: &Status, words: &str) -> bool {
    matches!(status, Status::Problem { problem }
        if problem.message.contains(words) && problem.message.contains("set `GH_TOKEN`"))
}

static ON_THE_INTERVAL: ScriptedGh = ScriptedGh::new();

fn gh_on_the_interval(_: &[&str]) -> io::Result<GhOutput> {
    ON_THE_INTERVAL.answer()
}

#[test]
fn after_a_read_that_found_no_token_the_status_check_reads_it_again_and_connects() {
    for (none, words) in found_none() {
        let asked = ON_THE_INTERVAL.script(vec![none, GhAnswer::Token]);
        let (folders, _folder) = temp_folders("no-token-interval");
        let (board, seen) = watched_board();
        let intervals = Intervals {
            status: Duration::from_millis(20),
            ..HOURLY
        };
        let access = through(gh_on_the_interval);
        let session = Session::start(board, Ok(folders), reaching(access), intervals);

        let problem = seen.recv_timeout(PLENTY).unwrap();
        let online = seen.recv_timeout(PLENTY).unwrap();
        session.stop(PLENTY);

        assert!(says(&problem, words), "{problem:?}");
        assert_eq!(online, octocat());
        assert_eq!(asked.try_iter().count(), 2);
    }
}

static ON_REFRESH: ScriptedGh = ScriptedGh::new();

fn gh_on_refresh(_: &[&str]) -> io::Result<GhOutput> {
    ON_REFRESH.answer()
}

#[test]
fn after_a_read_that_found_no_token_a_refresh_reads_it_again_and_connects() {
    for (none, words) in found_none() {
        let asked = ON_REFRESH.script(vec![none, GhAnswer::Token]);
        let (folders, _folder) = temp_folders("no-token-refresh");
        let (board, seen) = watched_board();
        let access = through(gh_on_refresh);
        let session = Session::start(board, Ok(folders), reaching(access), HOURLY);

        let problem = seen.recv_timeout(PLENTY).unwrap();
        session.fetching().refresh();
        let connecting = seen.recv_timeout(PLENTY).unwrap();
        let online = seen.recv_timeout(PLENTY).unwrap();
        session.stop(PLENTY);

        assert!(says(&problem, words), "{problem:?}");
        assert_eq!((connecting, online), (Status::Connecting, octocat()));
        assert_eq!(asked.try_iter().count(), 2);
    }
}

static HELD: ScriptedGh = ScriptedGh::new();

fn gh_held(_: &[&str]) -> io::Result<GhOutput> {
    HELD.answer()
}

/// Waits until the session has taken what the test asked before this, as it answers in order.
fn taken(asked: &Fetching, heard: &Receiver<Heard>) {
    asked.mark_unseen(gcmon(), 7, None);
    while heard.recv_timeout(PLENTY).unwrap() != Heard::Changed(vec![7]) {}
}

#[test]
fn clicks_while_a_read_of_the_token_is_out_add_no_read() {
    let (folders, _folder) = temp_folders("no-token-held");
    let (synced, last_syncs) = mpsc::channel();
    let (board, heard) = heard_board(|_| {}, |_| {});
    let board = board.with_last_syncs(Box::new(move |last: &LastSync| {
        pass_on(&synced, last.clone());
    }));
    let (let_start_go, start_held) = mpsc::channel();
    let (let_click_go, click_held) = mpsc::channel();
    let asked = HELD.script(vec![GhAnswer::Held(start_held), GhAnswer::Held(click_held)]);
    let session = Session::start(board, Ok(folders), reaching(through(gh_held)), HOURLY);
    let clicks = session.fetching();
    // The GitHub thread has taken what came before this once the window hears the last sync it shows.
    let thread_took = || {
        clicks.sync(gcmon());
        last_syncs.recv_timeout(PLENTY).unwrap()
    };

    // The session passes the first click on, which comes while the read at the start is out.
    asked.recv_timeout(PLENTY).unwrap();
    clicks.refresh();
    clicks.refresh();
    taken(&clicks, &heard);
    drop(let_start_go);
    let at_start = next_status(&heard);
    let _shown = thread_took();
    let after_start = asked.try_iter().count();

    // A click once the status bar says why reads again, and the clicks while it is out add nothing.
    clicks.refresh();
    asked.recv_timeout(PLENTY).unwrap();
    let connecting = next_status(&heard);
    clicks.refresh();
    clicks.refresh();
    taken(&clicks, &heard);
    drop(let_click_go);
    let on_click = next_status(&heard);
    let _shown = thread_took();
    session.stop(PLENTY);

    assert!(says(&at_start, "didn't answer"), "{at_start:?}");
    assert_eq!(after_start, 0);
    assert_eq!(connecting, Status::Connecting);
    assert!(says(&on_click, "didn't answer"), "{on_click:?}");
    assert_eq!(asked.try_iter().count(), 0);
}

static ANNOUNCED: ScriptedGh = ScriptedGh::new();

fn gh_announced(_: &[&str]) -> io::Result<GhOutput> {
    ANNOUNCED.answer()
}

#[test]
fn a_read_of_the_token_is_in_flight_until_gh_answers_and_github_hears_the_rest() {
    let (folders, _folder) = temp_folders("no-token-in-flight");
    let (let_go, held) = mpsc::channel();
    let asked = ANNOUNCED.script(vec![GhAnswer::Held(held), GhAnswer::Token]);
    let (announce, heard) = in_flight_heard();
    let board = quiet_board().with_in_flight(announce);
    let session = Session::start(board, Ok(folders), reaching(through(gh_announced)), HOURLY);

    asked.recv_timeout(PLENTY).unwrap();
    let out = heard.recv_timeout(PLENTY).unwrap();
    let still_out = heard.try_recv();
    drop(let_go);
    let ended = heard.recv_timeout(PLENTY).unwrap();
    session.fetching().refresh();
    let rest: Vec<_> = (0..4)
        .map(|_| heard.recv_timeout(PLENTY).unwrap())
        .collect();
    session.stop(PLENTY);

    assert_eq!(out, Some(Asking::Token));
    assert_eq!(still_out, Err(TryRecvError::Empty));
    assert_eq!(ended, None);
    assert_eq!(
        rest,
        [Some(Asking::Token), None, Some(Asking::Status), None]
    );
}

static ON_SCREEN: ScriptedGh = ScriptedGh::new();

fn gh_on_screen(_: &[&str]) -> io::Result<GhOutput> {
    ON_SCREEN.answer()
}

#[test]
fn an_item_opened_without_a_token_is_read_as_soon_as_a_read_finds_one() {
    let (folders, _folder) = temp_folders("no-token-on-screen");
    let asked = ON_SCREEN.script(vec![GhAnswer::Missing, GhAnswer::Token]);
    let (announce, heard) = in_flight_heard();
    let board = quiet_board().with_in_flight(announce);
    let session = Session::start(board, Ok(folders), reaching(through(gh_on_screen)), HOURLY);
    let window = session.fetching();
    asked.recv_timeout(PLENTY).unwrap();

    let issue = Item {
        repository: gcmon(),
        number: 7,
    };
    window.screen_moved(ScreenMove::Opened(Fetched::Issue(issue)));
    window.fetch_issue(gcmon(), 7);
    window.refresh();
    let heard: Vec<_> = (0..8)
        .map(|_| heard.recv_timeout(PLENTY).unwrap())
        .collect();
    session.stop(PLENTY);

    let read = Asking::Read {
        repository: gcmon(),
        number: 7,
    };
    let reads = [Some(Asking::Token), None];
    let connects = [Some(Asking::Status), None, Some(read), None];
    assert_eq!(heard, [&reads[..], &reads, &connects].concat());
}

static OPENED: ScriptedGh = ScriptedGh::new();

fn gh_opened(_: &[&str]) -> io::Result<GhOutput> {
    OPENED.answer()
}

#[test]
fn a_repository_opened_without_a_token_shows_its_last_sync() {
    let (folders, _folder) = temp_folders("no-token-opened");
    let _asked = OPENED.script(vec![GhAnswer::Missing]);
    let (synced, last_syncs) = mpsc::channel();
    let board = quiet_board().with_last_syncs(Box::new(move |last: &LastSync| {
        pass_on(&synced, last.clone());
    }));
    let session = Session::start(board, Ok(folders), reaching(through(gh_opened)), HOURLY);

    session.fetching().sync(gcmon());
    let last_sync = last_syncs.recv_timeout(PLENTY).unwrap();
    session.stop(PLENTY);

    let never = LastSync {
        repository: gcmon(),
        at: None,
    };
    assert_eq!(last_sync, never);
}
