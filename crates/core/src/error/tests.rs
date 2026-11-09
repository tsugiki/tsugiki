use super::*;
use serde_json::json;

#[test]
fn an_error_reaches_the_ui_as_a_kind_and_a_message() {
    let error = Error::new(ErrorKind::NotFound, "No such Issue.");
    assert_eq!(
        serde_json::to_value(error).unwrap(),
        json!({ "kind": "not_found", "message": "No such Issue." })
    );
}

#[test]
fn the_kinds_are_the_ones_adr_0008_names() {
    let kinds = [
        (ErrorKind::Offline, "offline"),
        (ErrorKind::CertificateDates, "certificate_dates"),
        (ErrorKind::Unauthorized, "unauthorized"),
        (ErrorKind::WriteRefused, "write_refused"),
        (ErrorKind::RateLimited { resets_at: None }, "rate_limited"),
        (ErrorKind::NotFound, "not_found"),
        (ErrorKind::Conflict, "conflict"),
        (ErrorKind::QueryRefused, "query_refused"),
        (ErrorKind::Internal, "internal"),
    ];
    for (kind, name) in kinds {
        assert_eq!(serde_json::to_value(kind).unwrap()["kind"], name);
    }
}

#[test]
fn only_a_rate_limit_says_when_it_resets() {
    let limited = ErrorKind::RateLimited {
        resets_at: Some(1_800_000_000),
    };
    assert_eq!(
        serde_json::to_value(Error::new(limited, "Wait.")).unwrap(),
        json!({ "kind": "rate_limited", "resets_at": 1_800_000_000, "message": "Wait." })
    );
}
