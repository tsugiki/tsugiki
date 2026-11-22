//! What the core sends through `tsugiki-token`'s `Https`, and what it makes of the answers (ADR 0019). The
//! `Transport` for `Https` that calls these is in `live.rs`, because it reaches the network.

use std::fmt;
use std::sync::OnceLock;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tsugiki_token::{Reply as Answer, RestPath, Unreachable as NotSent};

use super::clock::http_date;
use super::{ConditionalReply, Grant, Reply, Request, Unreachable};

/// `request` as the JSON body that GitHub's GraphQL endpoint takes.
pub(super) fn graphql_body(request: &Request<'_>) -> String {
    serde_json::json!({ "query": request.query, "variables": request.variables }).to_string()
}

/// Fails the test that sends `request` if `tsugiki-token` would refuse it. The client runs it on each
/// request in the core's unit tests and in each build with `testing` on, the app's tests among them, so a
/// query that holds `mutation` fails a test, whatever transport answers it, before `Https` refuses it in the
/// app.
#[cfg(any(test, feature = "testing"))]
pub(super) fn assert_read(request: &Request<'_>) {
    let refusal = tsugiki_token::testing::refusal(&graphql_body(request));
    assert_eq!(refusal, None, "{} isn't a read", request.operation);
}

/// A GraphQL answer to `operation`, with the headers the client reads parsed.
pub(super) fn reply(operation: &str, sent: Result<Answer, NotSent>) -> Result<Reply, Unreachable> {
    let answer = sent.map_err(|failure| unreachable(&operation, failure))?;
    Ok(Reply {
        status: answer.status,
        reset: answer.ratelimit_reset.and_then(|reset| reset.parse().ok()),
        date: answer.date.as_deref().and_then(http_date),
        body: answer.body,
        grant: Grant {
            scopes: answer.scopes,
            expires: answer.token_expiration.as_deref().and_then(expiration),
        },
    })
}

/// The time in `GitHub-Authentication-Token-Expiration`, which GitHub writes as `2026-12-01 09:30:00 UTC`,
/// or with an offset such as `+0100` in place of `UTC`. Anything else is no time, and the log says so once a
/// run, as each answer brings the header again.
fn expiration(text: &str) -> Option<OffsetDateTime> {
    static WARNED: OnceLock<()> = OnceLock::new();
    let parsed = text.split_once(' ').and_then(|(date, rest)| {
        let (clock, zone) = rest.split_once(' ')?;
        let offset = match zone {
            "UTC" => "Z".to_owned(),
            _ => {
                let (hours, minutes) = zone.split_at_checked(3)?;
                format!("{hours}:{minutes}")
            }
        };
        OffsetDateTime::parse(&format!("{date}T{clock}{offset}"), &Rfc3339).ok()
    });
    if parsed.is_none() && WARNED.set(()).is_ok() {
        log::warn!("GitHub named a token expiry Tsugiki can't read: {text}");
    }
    parsed
}

/// A REST answer to a conditional request for `path`. A refresh reads its tag and date and drops the body.
pub(super) fn conditional_reply(
    path: &RestPath,
    sent: Result<Answer, NotSent>,
) -> Result<ConditionalReply, Unreachable> {
    let answer = sent.map_err(|failure| unreachable(path, failure))?;
    Ok(ConditionalReply {
        status: answer.status,
        etag: answer.etag,
        date: answer.date.as_deref().and_then(http_date),
    })
}

/// The status bar says only that GitHub can't be reached or that its certificate's dates don't fit the
/// clock, and a request Tsugiki refused fails on its own, so this is the one place the whole reason shows: a
/// proxy that inspects TLS, for example, or the query `tsugiki-token` wouldn't send. `asked` names the
/// request.
fn unreachable(asked: &dyn fmt::Display, failure: NotSent) -> Unreachable {
    let (cause, unreachable) = match failure {
        NotSent::Offline(cause) => (cause, Unreachable::Offline),
        NotSent::CertificateDates(cause) => (cause, Unreachable::CertificateDates),
        NotSent::Refused(cause) => (cause, Unreachable::Refused),
    };
    log::warn!("A request to GitHub for {asked} failed: {cause}");
    unreachable
}

#[cfg(test)]
mod tests;
