//! What GitHub Releases answers in the tests of the release check, and of the app around it, which reach it
//! through the core's `testing` feature.

use tsugiki_token::{Reply, Unreachable};

/// GitHub Releases' answer that `tag` is the latest release.
pub fn release(tag: &str) -> Result<Reply, Unreachable> {
    answered(
        200,
        &format!(r#"{{"tag_name": "{tag}", "draft": false, "prerelease": false}}"#),
    )
}

/// GitHub Releases' answer with `status` and `body`.
pub fn answered(status: u16, body: &str) -> Result<Reply, Unreachable> {
    Ok(Reply {
        status,
        body: body.to_owned(),
        date: None,
        ratelimit_reset: None,
        etag: None,
        scopes: None,
        token_expiration: None,
    })
}

/// What asking GitHub Releases gives with no network.
pub fn offline() -> Result<Reply, Unreachable> {
    Err(Unreachable::Offline("no network".to_owned()))
}
