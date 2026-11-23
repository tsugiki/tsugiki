use super::testing::{announced, watched_board};
use super::*;
use crate::error::ErrorKind;
use crate::testing::cpython;

#[test]
fn a_change_a_last_sync_or_the_points_coming_back_with_nobody_listening_isnt_a_new_status() {
    let (mut board, seen) = watched_board();
    let repository = cpython();

    board.changed(&Change {
        repository: repository.clone(),
        items: Vec::new(),
    });
    board.last_sync(&LastSync {
        repository,
        at: None,
    });
    board.points_back(Some(OffsetDateTime::UNIX_EPOCH));

    assert!(announced(&seen).is_empty());
}

#[test]
fn the_board_hands_over_what_hears_of_the_request_in_flight_once() {
    let (announce, _) = crate::github::testing::in_flight_heard();
    let mut board = super::testing::quiet_board().with_in_flight(announce);

    assert!(board.in_flight().is_some());
    assert!(board.in_flight().is_none());
}

#[test]
fn a_request_in_flight_reaches_the_ui_with_what_it_asks_and_when_it_went() {
    let in_flight = crate::github::in_flight::InFlight {
        asked: crate::github::in_flight::Asking::Read {
            repository: cpython(),
            number: 7,
        },
        since: OffsetDateTime::from_unix_timestamp(1_000_000_000).unwrap(),
    };

    assert_eq!(
        serde_json::to_value(&in_flight).unwrap(),
        serde_json::json!({
            "asked": { "kind": "read", "repository": "python/cpython", "number": 7 },
            "since": "2001-09-09T01:46:40Z",
        })
    );
}

#[test]
fn a_last_sync_reaches_the_ui_as_a_time_or_as_nothing() {
    let repository = cpython();
    let at = OffsetDateTime::from_unix_timestamp(1_000_000_000).unwrap();
    let never = LastSync {
        repository: repository.clone(),
        at: None,
    };
    let once = LastSync {
        repository,
        at: Some(at),
    };

    assert_eq!(
        serde_json::to_value(never).unwrap()["at"],
        serde_json::Value::Null
    );
    assert_eq!(
        serde_json::to_value(once).unwrap()["at"],
        "2001-09-09T01:46:40Z"
    );
}

#[test]
fn nothing_is_known_before_tsugiki_connects() {
    let (mut board, seen) = watched_board();
    board.account(Account::Connecting);
    assert!(announced(&seen).is_empty(), "it was connecting already");
}

#[test]
fn every_change_is_announced() {
    let (mut board, seen) = watched_board();
    board.account(Account::Offline);
    board.account(Account::Connecting);
    assert_eq!(
        announced(&seen),
        [
            Status::from(Account::Offline),
            Status::from(Account::Connecting)
        ]
    );
}

fn octocat() -> Account {
    Account::Online {
        login: Login::new("octocat"),
    }
}

fn database_problem() -> Error {
    Error::new(ErrorKind::Internal, "Tsugiki can't write to its database.")
}

#[test]
fn a_problem_shows_beside_the_account_and_a_change_of_account_keeps_it() {
    let (mut board, seen) = watched_board();
    board.account(octocat());

    board.problem(database_problem());
    board.account(Account::Offline);

    let beside = |account| Status {
        account,
        problem: Some(database_problem()),
    };
    assert_eq!(
        announced(&seen),
        [
            octocat().into(),
            beside(octocat()),
            beside(Account::Offline)
        ]
    );
}

#[test]
fn a_whole_status_replaces_the_problem_or_takes_it_away() {
    let (mut board, seen) = watched_board();
    board.problem(database_problem());

    board.set(octocat().into());

    assert_eq!(announced(&seen).last(), Some(&octocat().into()));
}

#[test]
fn only_the_account_says_whether_github_can_be_reached() {
    let mut board = super::testing::quiet_board();
    board.problem(database_problem());
    assert!(!board.is_offline());

    board.account(Account::OfflineAs {
        last_login: Login::new("hubot"),
    });

    assert!(board.is_offline());
    assert!(board.asking_again());
    assert_eq!(
        board.status(),
        &Status {
            account: Account::ReconnectingAs {
                last_login: Login::new("hubot")
            },
            problem: Some(database_problem()),
        }
    );
}

#[test]
fn the_account_reaches_the_ui_tagged_by_its_state() {
    let hubot = || Login::new("hubot");
    let cases = [
        (Account::Offline, serde_json::json!({ "state": "offline" })),
        (
            Account::Connecting,
            serde_json::json!({ "state": "connecting" }),
        ),
        (
            octocat(),
            serde_json::json!({ "state": "online", "login": "octocat" }),
        ),
        (
            Account::ConnectingAs {
                last_login: hubot(),
            },
            serde_json::json!({ "state": "connecting_as", "last_login": "hubot" }),
        ),
        (
            Account::OfflineAs {
                last_login: hubot(),
            },
            serde_json::json!({ "state": "offline_as", "last_login": "hubot" }),
        ),
        (
            Account::Reconnecting,
            serde_json::json!({ "state": "reconnecting" }),
        ),
        (
            Account::ReconnectingAs {
                last_login: hubot(),
            },
            serde_json::json!({ "state": "reconnecting_as", "last_login": "hubot" }),
        ),
        (
            Account::NoToken {
                message: "Install gh.".to_owned(),
            },
            serde_json::json!({ "state": "no_token", "message": "Install gh." }),
        ),
        (
            Account::Refused {
                message: "Sign in.".to_owned(),
            },
            serde_json::json!({ "state": "refused", "message": "Sign in." }),
        ),
    ];
    for (account, expected) in cases {
        assert_eq!(serde_json::to_value(account).unwrap(), expected);
    }
}

#[test]
fn the_status_reaches_the_ui_as_the_account_and_a_problem_or_nothing() {
    let troubled = Status {
        account: Account::Offline,
        problem: Some(Error::new(ErrorKind::Internal, "Disk full.")),
    };

    assert_eq!(
        serde_json::to_value(Status::from(Account::Offline)).unwrap(),
        serde_json::json!({ "account": { "state": "offline" }, "problem": null })
    );
    assert_eq!(
        serde_json::to_value(troubled).unwrap(),
        serde_json::json!({
            "account": { "state": "offline" },
            "problem": { "kind": "internal", "message": "Disk full." },
        })
    );
}

#[test]
fn the_time_the_points_come_back_reaches_the_ui_only_when_it_changes() {
    let (passed, seen) = std::sync::mpsc::channel();
    let mut board = super::testing::quiet_board().with_points_back(Box::new(
        move |back: Option<&PointsBack>| {
            passed.send(back.map(|back| back.at)).unwrap();
        },
    ));
    let at = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();

    board.points_back(None);
    board.points_back(Some(at));
    board.points_back(Some(at));
    board.points_back(None);

    assert_eq!(seen.try_iter().collect::<Vec<_>>(), [Some(at), None]);
    assert_eq!(
        serde_json::to_value(PointsBack { at }).unwrap(),
        serde_json::json!({ "at": "2027-01-15T08:00:00Z" })
    );
}

#[test]
fn the_hour_points_reach_the_ui_from_the_first_answer_until_the_reset_and_only_when_they_change() {
    let (passed, seen) = std::sync::mpsc::channel();
    let mut board = super::testing::quiet_board().with_hour_points(Box::new(
        move |points: Option<&HourPoints>| {
            passed.send(points.copied()).unwrap();
        },
    ));
    let points = HourPoints {
        limit: 5000,
        used: 3760,
        remaining: 1240,
        resets_at: OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap(),
        spent: 3610,
        since: None,
    };
    let later = HourPoints {
        used: 3761,
        remaining: 1239,
        spent: 3611,
        ..points
    };

    board.hour_points(None);
    board.hour_points(Some(points));
    board.hour_points(Some(points));
    board.hour_points(Some(later));
    // The hour reset.
    board.hour_points(None);
    board.hour_points(None);

    assert_eq!(
        seen.try_iter().collect::<Vec<_>>(),
        [Some(points), Some(later), None]
    );
    assert_eq!(
        serde_json::to_value(points).unwrap(),
        serde_json::json!({
            "limit": 5000,
            "used": 3760,
            "remaining": 1240,
            "resets_at": "2027-01-15T08:00:00Z",
            "spent": 3610,
            "since": null,
        })
    );
}
