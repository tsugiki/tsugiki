use std::borrow::Cow;

use serde_json::{Value, json};
use time::OffsetDateTime;
use tsugiki_token::testing::refusal;

use super::*;
use crate::error::{Error, ErrorKind};
use crate::token::WriteScope;

const DATE: &str = "Tue, 10 Nov 2026 12:00:00 GMT";

fn noon() -> Option<OffsetDateTime> {
    Some(OffsetDateTime::from_unix_timestamp(1_794_312_000).unwrap())
}

fn answer(date: Option<&str>, reset: Option<&str>, etag: Option<&str>) -> Answer {
    Answer {
        status: 200,
        body: "{}".to_owned(),
        date: date.map(str::to_owned),
        ratelimit_reset: reset.map(str::to_owned),
        etag: etag.map(str::to_owned),
        scopes: None,
        token_expiration: None,
    }
}

#[test]
fn a_graphql_body_holds_the_query_and_its_variables() {
    let request = Request {
        operation: "Op",
        query: Cow::Borrowed("query Op($n: Int!) { a(n: $n) }"),
        variables: json!({ "n": 3 }),
    };
    let body: Value = serde_json::from_str(&graphql_body(&request)).unwrap();
    assert_eq!(
        body,
        json!({ "query": "query Op($n: Int!) { a(n: $n) }", "variables": { "n": 3 } })
    );
}

#[test]
fn each_recorded_query_passes_the_token_crates_check_for_a_read() {
    let mut checked = 0;
    for entry in std::fs::read_dir(crate::testing::recordings()).unwrap() {
        let path = entry.unwrap().path();
        let recording: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let request = Request {
            operation: "Recorded",
            query: Cow::Owned(recording["query"].as_str().unwrap().to_owned()),
            variables: json!({}),
        };
        assert_eq!(refusal(&graphql_body(&request)), None, "{path:?}");
        checked += 1;
    }
    assert!(checked > 0);
}

#[test]
fn a_reply_reads_githubs_clock_and_its_rate_limit_reset() {
    let reply = reply("Op", Ok(answer(Some(DATE), Some("1794312000"), None))).unwrap();
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, "{}");
    assert_eq!(reply.reset, Some(1_794_312_000));
    assert_eq!(reply.date, noon());
}

#[test]
fn headers_that_dont_parse_are_left_out() {
    let reply = reply("Op", Ok(answer(Some("yesterday"), Some("soon"), None))).unwrap();
    assert_eq!((reply.reset, reply.date), (None, None));
}

fn issues() -> RestPath {
    RestPath::parse("/repos/octo/hello/issues").unwrap()
}

#[test]
fn a_conditional_reply_keeps_the_tag_and_githubs_clock() {
    let sent = answer(Some(DATE), None, Some("W/\"abc\""));
    let reply = conditional_reply(
        &issues(),
        Ok(Answer {
            status: 304,
            ..sent
        }),
    )
    .unwrap();
    assert_eq!(reply.status, 304);
    assert_eq!(reply.etag.as_deref(), Some("W/\"abc\""));
    assert_eq!(reply.date, noon());
}

#[test]
fn a_request_that_got_no_answer_says_what_the_user_can_do() {
    let offline = NotSent::Offline("dns failed".to_owned());
    let dates = NotSent::CertificateDates("expired".to_owned());
    assert_eq!(reply("Op", Err(offline)).unwrap_err(), Unreachable::Offline);
    assert_eq!(
        conditional_reply(&issues(), Err(dates)).unwrap_err(),
        Unreachable::CertificateDates
    );
}

#[test]
fn a_refused_request_fails_as_a_query_github_refused() {
    let refused = reply("Op", Err(NotSent::Refused("holds mutation".to_owned()))).unwrap_err();
    assert_eq!(refused, Unreachable::Refused);
    assert_eq!(Error::from(refused).kind, ErrorKind::QueryRefused);
}

#[test]
fn a_reply_says_what_the_token_s_scopes_let_it_write_and_when_it_expires() {
    let sent = Answer {
        scopes: Some(WriteScope::Public),
        token_expiration: Some("2026-11-10 12:00:00 UTC".to_owned()),
        ..answer(None, None, None)
    };
    let grant = reply("Op", Ok(sent)).unwrap().grant;
    assert_eq!(
        grant,
        Grant {
            scopes: Some(WriteScope::Public),
            expires: noon(),
        }
    );
}

#[test]
fn a_token_expiry_may_name_an_offset_and_one_that_doesnt_parse_is_left_out() {
    assert_eq!(expiration("2026-11-10 13:00:00 +0100"), noon());
    assert_eq!(expiration("2026-11-10 07:00:00 -0500"), noon());
    for unread in [
        "soon",
        "2026-11-10 12:00:00",
        "2026-11-10 12:00:00 CET",
        "x y +1",
    ] {
        assert_eq!(expiration(unread), None, "{unread}");
    }
}
