use super::testing::{announced, watched_board};
use super::*;
use crate::error::ErrorKind;

#[test]
fn a_change_or_a_last_sync_with_nobody_listening_isnt_a_new_status() {
    let (mut board, seen) = watched_board();
    let repository = RepositoryName::parse("python/cpython").unwrap();

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
    let repository = RepositoryName::parse("python/cpython").unwrap();
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
