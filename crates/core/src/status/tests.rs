use super::testing::{announced, watched_board};
use super::*;
use crate::error::ErrorKind;
use crate::testing::cpython;

#[test]
fn a_change_or_a_last_sync_with_nobody_listening_isnt_a_new_status() {
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

    assert!(announced(&seen).is_empty());
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
    board.set(Status::Connecting);
    assert!(announced(&seen).is_empty(), "it was connecting already");
}

#[test]
fn every_change_is_announced() {
    let (mut board, seen) = watched_board();
    board.set(Status::Offline);
    board.set(Status::Connecting);
    assert_eq!(announced(&seen), [Status::Offline, Status::Connecting]);
}

#[test]
fn the_status_reaches_the_ui_tagged_by_its_state() {
    let hubot = || Login::new("hubot");
    let problem = Error::new(ErrorKind::Unauthorized, "Sign in.");
    let cases = [
        (Status::Offline, serde_json::json!({ "state": "offline" })),
        (
            Status::Connecting,
            serde_json::json!({ "state": "connecting" }),
        ),
        (
            Status::Online {
                login: Login::new("octocat"),
            },
            serde_json::json!({ "state": "online", "login": "octocat" }),
        ),
        (
            Status::ConnectingAs {
                last_login: hubot(),
            },
            serde_json::json!({ "state": "connecting_as", "last_login": "hubot" }),
        ),
        (
            Status::OfflineAs {
                last_login: hubot(),
            },
            serde_json::json!({ "state": "offline_as", "last_login": "hubot" }),
        ),
        (
            Status::Reconnecting,
            serde_json::json!({ "state": "reconnecting" }),
        ),
        (
            Status::ReconnectingAs {
                last_login: hubot(),
            },
            serde_json::json!({ "state": "reconnecting_as", "last_login": "hubot" }),
        ),
        (
            Status::Problem { problem },
            serde_json::json!({
                "state": "problem",
                "problem": { "kind": "unauthorized", "message": "Sign in." },
            }),
        ),
    ];
    for (status, expected) in cases {
        assert_eq!(serde_json::to_value(status).unwrap(), expected);
    }
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
