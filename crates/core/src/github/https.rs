//! What the core sends through `tsugiki-token`'s `Https`, and what it makes of the answers (ADR 0019). The
//! `Transport` for `Https` that calls these is in `live.rs`, because it reaches the network.

use tsugiki_token::{Reply as Answer, Unreachable as NotSent};

use super::clock::http_date;
use super::{ConditionalReply, Reply, Request, Unreachable};

/// `request` as the JSON body that GitHub's GraphQL endpoint takes.
pub(super) fn graphql_body(request: &Request<'_>) -> String {
    serde_json::json!({ "query": request.query, "variables": request.variables }).to_string()
}

/// A GraphQL answer, with the headers the client reads parsed.
pub(super) fn reply(sent: Result<Answer, NotSent>) -> Result<Reply, Unreachable> {
    let answer = sent.map_err(unreachable)?;
    Ok(Reply {
        status: answer.status,
        reset: answer.ratelimit_reset.and_then(|reset| reset.parse().ok()),
        date: answer.date.as_deref().and_then(http_date),
        body: answer.body,
    })
}

/// A REST answer to a conditional request. A refresh reads its tag and date and drops the body.
pub(super) fn conditional_reply(
    sent: Result<Answer, NotSent>,
) -> Result<ConditionalReply, Unreachable> {
    let answer = sent.map_err(unreachable)?;
    Ok(ConditionalReply {
        status: answer.status,
        etag: answer.etag,
        date: answer.date.as_deref().and_then(http_date),
    })
}

/// The status bar says only that GitHub can't be reached, or that its certificate's dates don't fit the
/// clock, so this is the one place the whole reason shows: a proxy that inspects TLS, for example.
fn unreachable(failure: NotSent) -> Unreachable {
    let (cause, unreachable) = match failure {
        NotSent::Offline(cause) => (cause, Unreachable::Offline),
        NotSent::CertificateDates(cause) => (cause, Unreachable::CertificateDates),
    };
    log::warn!("A request to GitHub failed: {cause}");
    unreachable
}

#[cfg(test)]
mod tests;
