use std::borrow::Cow;

use serde_json::{Value, json};
use time::OffsetDateTime;

use super::*;

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
fn a_reply_reads_githubs_clock_and_its_rate_limit_reset() {
    let reply = reply(Ok(answer(Some(DATE), Some("1794312000"), None))).unwrap();
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, "{}");
    assert_eq!(reply.reset, Some(1_794_312_000));
    assert_eq!(reply.date, noon());
}

#[test]
fn headers_that_dont_parse_are_left_out() {
    let reply = reply(Ok(answer(Some("yesterday"), Some("soon"), None))).unwrap();
    assert_eq!((reply.reset, reply.date), (None, None));
}

#[test]
fn a_conditional_reply_keeps_the_tag_and_githubs_clock() {
    let sent = answer(Some(DATE), None, Some("W/\"abc\""));
    let reply = conditional_reply(Ok(Answer {
        status: 304,
        ..sent
    }))
    .unwrap();
    assert_eq!(reply.status, 304);
    assert_eq!(reply.etag.as_deref(), Some("W/\"abc\""));
    assert_eq!(reply.date, noon());
}

#[test]
fn a_request_that_got_no_answer_says_what_the_user_can_do() {
    let offline = NotSent::Offline("dns failed".to_owned());
    let dates = NotSent::CertificateDates("expired".to_owned());
    assert_eq!(reply(Err(offline)).unwrap_err(), Unreachable::Offline);
    assert_eq!(
        conditional_reply(Err(dates)).unwrap_err(),
        Unreachable::CertificateDates
    );
}
