//! The rate limit's gate in a running session: below the floor of points, no job's request reaches GitHub
//! until the reset, and each answers at once (#1001).

use super::*;
use crate::status::{PointsBack, SyncActivity};
use time::format_description::well_known::Rfc3339;

/// `at` as GitHub writes a time.
fn github_time(at: OffsetDateTime) -> String {
    at.format(&Rfc3339).unwrap()
}

/// GitHub's answer that octocat is signed in, with 10 points left of an hour that resets at `reset`.
fn signed_in_below_the_floor(reset: OffsetDateTime) -> Result<Reply, Unreachable> {
    let reset = github_time(reset);
    reply_ok(&format!(
        r#"{{"data":{{"viewer":{{"login":"octocat"}},"rateLimit":{{"cost":1,"limit":5000,
            "used":4990,"remaining":10,"resetAt":"{reset}"}}}}}}"#
    ))
}

/// What a board told the window while the session ran.
struct Heard {
    statuses: Receiver<Status>,
    failed_reads: Receiver<FailedRead>,
    points_back: Receiver<Option<OffsetDateTime>>,
    activity: Receiver<Option<SyncActivity>>,
    hour_points: Receiver<Option<HourPoints>>,
}

/// A board that passes on the statuses, the failed reads, the time GitHub's points come back, the sync's
/// activity and the hour's points.
fn heard_board() -> (StatusBoard, Heard) {
    let (status, statuses) = mpsc::channel();
    let (failed, failed_reads) = mpsc::channel();
    let (back, points_back) = mpsc::channel();
    let (doing, activity) = mpsc::channel();
    let (points, hour_points) = mpsc::channel();
    let board = StatusBoard::new(Box::new(move |shown: &Status| {
        status.send(shown.clone()).unwrap();
    }))
    .with_failed_reads(Box::new(move |read: &FailedRead| {
        failed.send(read.clone()).unwrap();
    }))
    .with_points_back(Box::new(move |at: Option<&PointsBack>| {
        back.send(at.map(|back| back.at)).unwrap();
    }))
    .with_sync_activity(Box::new(move |now: Option<&SyncActivity>| {
        doing.send(now.cloned()).unwrap();
    }))
    .with_hour_points(Box::new(move |hour: Option<&HourPoints>| {
        points.send(hour.copied()).unwrap();
    }));
    let heard = Heard {
        statuses,
        failed_reads,
        points_back,
        activity,
        hour_points,
    };
    (board, heard)
}

/// A session whose first answer, to who is signed in, leaves the hour's points below the floor until
/// `reset`, and then answers with `replies`, with what it asked and what its board told the window.
struct Paused {
    session: Session,
    asked: Receiver<(String, Value)>,
    heard: Heard,
    _folder: TempPath,
}

impl Paused {
    fn start(
        name: &str,
        reset: OffsetDateTime,
        replies: Vec<Result<Reply, Unreachable>>,
        intervals: Intervals,
    ) -> Self {
        let mut all = vec![signed_in_below_the_floor(reset)];
        all.extend(replies);
        let (script, asked) = Scripted::new(all);
        let access = GitHubAccess {
            transport: Box::new(script),
            token_from: SIGNED_IN,
        };
        let (folders, folder) = temp_folders(name);
        let (board, heard) = heard_board();
        let session = Session::start(board, Ok(folders), reaching(access), intervals);
        let paused = Self {
            session,
            asked,
            heard,
            _folder: folder,
        };
        assert_eq!(paused.asked_after_start(), ["Viewer"]);
        paused
    }

    /// The operations asked of GitHub since the last look, once the session has asked who is signed in.
    fn asked_after_start(&self) -> Vec<String> {
        let first = self.asked.recv_timeout(PLENTY).unwrap().0;
        let mut asked = vec![first];
        asked.extend(self.asked.try_iter().map(|(operation, _)| operation));
        asked
    }

    fn asked(&self) -> Vec<String> {
        self.asked
            .try_iter()
            .map(|(operation, _)| operation)
            .collect()
    }

    fn stop(self) -> Heard {
        self.session.stop(PLENTY);
        self.heard
    }
}

/// A reset twenty minutes away, far from any test's end.
fn later() -> OffsetDateTime {
    OffsetDateTime::now_utc() + time::Duration::minutes(20)
}

#[test]
fn below_the_floor_the_window_hears_when_the_points_come_back() {
    let reset = later();
    let paused = Paused::start("gate-told", reset, Vec::new(), HOURLY);

    let back = paused.heard.points_back.recv_timeout(PLENTY).unwrap();
    drop(paused.stop());

    let back = back.unwrap();
    assert!(
        (back - reset).abs() <= time::Duration::SECOND,
        "{back} {reset}"
    );
}

#[test]
fn below_the_floor_a_read_on_screen_asks_nothing_and_waits_on_github_with_the_status_as_it_was() {
    let paused = Paused::start("gate-read", later(), Vec::new(), HOURLY);

    paused.session.fetching().fetch_issue(gcmon(), 7);
    paused.session.fetching().fetch_pull_request(gcmon(), 8);
    let issue = paused.heard.failed_reads.recv_timeout(PLENTY).unwrap();
    let pr = paused.heard.failed_reads.recv_timeout(PLENTY).unwrap();
    let asked = paused.asked();
    let heard = paused.stop();

    assert_eq!(issue, item_7_failed(ReadFailure::WaitsOnGitHub));
    assert_eq!((pr.number, pr.why), (8, ReadFailure::WaitsOnGitHub));
    assert_eq!(asked, Vec::<String>::new());
    assert_eq!(announced(&heard.statuses).last(), Some(&octocat()));
}

#[test]
fn below_the_floor_a_fetch_of_an_unstored_item_asks_nothing_and_answers_offline() {
    let paused = Paused::start("gate-fetch", later(), Vec::new(), HOURLY);

    let fetched = paused.session.fetching().fetch_unstored(gcmon(), 7);
    let asked = paused.asked();
    let heard = paused.stop();

    let offline = ItemFetch::Unfetched {
        why: ItemUnfetched::Offline,
    };
    assert_eq!(fetched, offline);
    assert_eq!(asked, Vec::<String>::new());
    assert_eq!(announced(&heard.statuses).last(), Some(&octocat()));
}

#[test]
fn below_the_floor_a_check_asks_nothing_and_says_when_the_points_come_back() {
    let reset = later();
    let paused = Paused::start("gate-check", reset, Vec::new(), HOURLY);

    let problem = paused.session.fetching().check(gcmon()).unwrap_err();
    let asked = paused.asked();
    drop(paused.stop());

    let ErrorKind::RateLimited {
        resets_at: Some(resets_at),
    } = problem.kind
    else {
        panic!("{problem:?}");
    };
    assert!(
        (i64::from(resets_at) - reset.unix_timestamp()).abs() <= 1,
        "{problem:?}"
    );
    assert_eq!(asked, Vec::<String>::new());
}

#[test]
fn below_the_floor_refresh_asks_nothing_and_leaves_the_status() {
    let quick = Intervals {
        status: Duration::from_millis(20),
        ..HOURLY
    };
    let paused = Paused::start("gate-refresh", later(), Vec::new(), quick);

    paused.session.fetching().refresh();
    // Several of the status's intervals.
    thread::sleep(Duration::from_millis(200));
    let asked = paused.asked();
    let heard = paused.stop();

    assert_eq!(asked, Vec::<String>::new());
    assert_eq!(announced(&heard.statuses).last(), Some(&octocat()));
}

#[test]
fn below_the_floor_a_sync_asks_nothing_and_says_when_it_goes_on() {
    let reset = later();
    let paused = Paused::start("gate-sync", reset, Vec::new(), HOURLY);

    paused.session.fetching().sync(gcmon());
    let paused_until = loop {
        let activity = paused.heard.activity.recv_timeout(PLENTY).unwrap();
        if let Some(until) = activity.and_then(|activity| activity.paused_until) {
            break until;
        }
    };
    let asked = paused.asked();
    drop(paused.stop());

    assert!(
        (paused_until - reset).abs() <= time::Duration::seconds(2),
        "{paused_until} {reset}"
    );
    assert_eq!(asked, Vec::<String>::new());
}

#[test]
fn the_first_request_after_the_reset_goes_and_says_the_new_hours_points() {
    let reset = OffsetDateTime::now_utc() + time::Duration::milliseconds(1500);
    let fetch = item_answer(&gcmon(), issue_item(7, &gcmon()));
    let paused = Paused::start("gate-reset", reset, vec![fetch], HOURLY);
    let held = paused.heard.points_back.recv_timeout(PLENTY).unwrap();

    let opened = paused.heard.points_back.recv_timeout(PLENTY).unwrap();
    let fetched = paused.session.fetching().fetch_unstored(gcmon(), 7);
    let asked = paused.asked();
    let heard = paused.stop();

    assert!(held.is_some());
    assert_eq!(opened, None, "the window hears the gate open at the reset");
    assert_eq!(fetched, ItemFetch::Issue);
    assert_eq!(asked, ["FetchItem"]);
    let last = heard.hour_points.try_iter().flatten().last().unwrap();
    assert_eq!(last.remaining, 4000, "{last:?}");
}
