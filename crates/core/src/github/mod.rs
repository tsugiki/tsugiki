//! Tsugiki's GitHub client. It talks through a [`Transport`], so tests replay recorded answers (ADR 0009).

pub mod live;
pub mod record;
pub mod replay;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use crate::error::{Error, ErrorKind};
use crate::token::Token;

/// One GraphQL request. `operation` names the query, and the replayer finds its recording by that name.
pub struct Request<'a> {
    pub operation: &'a str,
    pub query: &'a str,
}

/// What GitHub answered.
#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    /// The `x-ratelimit-reset` header, in seconds since the Unix epoch.
    pub reset: Option<u32>,
    pub body: String,
}

/// GitHub couldn't be reached at all.
#[derive(Debug, Clone, Copy)]
pub struct Unreachable;

/// How a request reaches GitHub: the network in the app, recorded answers in tests.
pub trait Transport: Send + Sync {
    fn post(&self, token: &Token, request: &Request<'_>) -> Result<Reply, Unreachable>;
}

/// A GitHub username. It reaches the UI as a plain string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct Login(String);

impl Login {
    pub fn new(login: &str) -> Self {
        Self(login.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The signed-in user.
#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct Viewer {
    pub login: Login,
}

const VIEWER: Request<'static> = Request {
    operation: "Viewer",
    query: "query Viewer { viewer { login } }",
};

/// A GraphQL client that holds the token, so no caller has to.
pub struct GitHub {
    transport: Box<dyn Transport>,
    token: Token,
}

impl GitHub {
    pub fn new(transport: Box<dyn Transport>, token: Token) -> Self {
        Self { transport, token }
    }

    /// Asks GitHub who the token belongs to.
    pub fn viewer(&self) -> Result<Viewer, Error> {
        #[derive(Deserialize)]
        struct Data {
            viewer: Viewer,
        }
        let data: Data = self.query(&VIEWER)?;
        Ok(data.viewer)
    }

    fn query<D: DeserializeOwned>(&self, request: &Request<'_>) -> Result<D, Error> {
        let reply = self
            .transport
            .post(&self.token, request)
            .map_err(|Unreachable| Error::new(ErrorKind::Offline, "Tsugiki can't reach GitHub."))?;
        read(&reply)
    }
}

const RATE_LIMITED: &str =
    "GitHub's rate limit has been reached. Tsugiki can read again after it resets.";

/// Turns an answer into its data, or into the error the UI can act on (ADR 0008).
#[expect(
    clippy::map_err_ignore,
    reason = "the parser's words mean nothing to a user; they go to the local log once #58 adds one"
)]
fn read<D: DeserializeOwned>(reply: &Reply) -> Result<D, Error> {
    #[derive(Deserialize)]
    struct Envelope {
        data: Option<Value>,
        #[serde(default)]
        errors: Vec<GraphqlError>,
    }
    #[derive(Deserialize)]
    struct GraphqlError {
        #[serde(rename = "type")]
        kind: Option<GraphqlErrorKind>,
        message: String,
    }
    #[derive(Deserialize)]
    enum GraphqlErrorKind {
        #[serde(rename = "RATE_LIMITED")]
        RateLimited,
        #[serde(other)]
        Other,
    }

    let internal = |message: &str| Error::new(ErrorKind::Internal, message);
    let rate_limited = || {
        let resets_at = reply.reset;
        Error::new(ErrorKind::RateLimited { resets_at }, RATE_LIMITED)
    };
    match reply.status {
        200 => {}
        401 => {
            return Err(Error::new(
                ErrorKind::Unauthorized,
                "GitHub refused the token. Run `gh auth login`, and restart Tsugiki.",
            ));
        }
        429 => return Err(rate_limited()),
        403 if reply.body.contains("rate limit") => return Err(rate_limited()),
        status => return Err(internal(&format!("GitHub answered with status {status}."))),
    }

    const UNREADABLE: &str = "GitHub sent an answer Tsugiki can't read.";
    // The data is read after the errors: a field that failed comes back null, which `D` may not allow.
    let envelope: Envelope = serde_json::from_str(&reply.body).map_err(|_| internal(UNREADABLE))?;
    if let Some(error) = envelope.errors.first() {
        return Err(match error.kind {
            Some(GraphqlErrorKind::RateLimited) => rate_limited(),
            Some(GraphqlErrorKind::Other) | None => {
                internal(&format!("GitHub said: {}", error.message))
            }
        });
    }
    let data = envelope
        .data
        .ok_or_else(|| internal("GitHub sent an answer with no data."))?;
    serde_json::from_value(data).map_err(|_| internal(UNREADABLE))
}

/// What the core's tests share.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::token::GhOutput;

    /// Answers every request the same way: the small builder ADR 0009 allows for rare cases.
    pub(crate) struct Canned(pub(crate) Result<Reply, Unreachable>);

    impl Transport for Canned {
        fn post(&self, _: &Token, _: &Request<'_>) -> Result<Reply, Unreachable> {
            self.0.clone()
        }
    }

    pub(crate) fn answered(status: u16, body: &str) -> Canned {
        Canned(Ok(Reply {
            status,
            reset: None,
            body: body.to_owned(),
        }))
    }

    pub(crate) fn signed_in_gh(_: &[&str]) -> std::io::Result<GhOutput> {
        Ok(GhOutput {
            success: true,
            stdout: "gho_example".to_owned(),
        })
    }

    pub(crate) fn token() -> Token {
        Token::from_gh(signed_in_gh).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{Canned, answered, token};
    use super::*;
    use std::path::Path;

    fn client(transport: impl Transport + 'static) -> GitHub {
        GitHub::new(Box::new(transport), token())
    }

    fn limited_until(resets_at: Option<u32>) -> ErrorKind {
        ErrorKind::RateLimited { resets_at }
    }

    fn error_of(transport: Canned) -> crate::error::Error {
        client(transport).viewer().unwrap_err()
    }

    #[test]
    fn github_says_who_the_user_is() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github");
        let viewer = client(replay::Replayer::new(fixtures)).viewer().unwrap();
        assert_eq!(viewer.login, Login::new("sergey-miryanov"));
    }

    #[test]
    fn a_login_is_read_from_the_viewer() {
        let viewer = client(answered(200, r#"{"data":{"viewer":{"login":"octocat"}}}"#));
        assert_eq!(
            viewer.viewer().unwrap(),
            Viewer {
                login: Login::new("octocat")
            }
        );
    }

    #[test]
    fn no_connection_means_offline() {
        assert_eq!(error_of(Canned(Err(Unreachable))).kind, ErrorKind::Offline);
    }

    #[test]
    fn a_refused_token_is_unauthorized_and_says_how_to_fix_it() {
        let error = error_of(answered(401, r#"{"message":"Bad credentials"}"#));
        assert_eq!(error.kind, ErrorKind::Unauthorized);
        assert!(error.message.contains("gh auth login"), "{}", error.message);
    }

    #[test]
    fn the_primary_rate_limit_says_when_it_resets() {
        let error = error_of(Canned(Ok(Reply {
            status: 403,
            reset: Some(1_800_000_000),
            body: r#"{"message":"API rate limit exceeded for user"}"#.to_owned(),
        })));
        assert_eq!(error.kind, limited_until(Some(1_800_000_000)));
    }

    #[test]
    fn a_429_is_a_rate_limit() {
        assert_eq!(error_of(answered(429, "")).kind, limited_until(None));
    }

    #[test]
    fn a_graphql_rate_limit_is_a_rate_limit() {
        let body = r#"{"errors":[{"type":"RATE_LIMITED","message":"API rate limit exceeded"}]}"#;
        let error = error_of(Canned(Ok(Reply {
            status: 200,
            reset: Some(1_800_000_000),
            body: body.to_owned(),
        })));
        assert_eq!(error.kind, limited_until(Some(1_800_000_000)));
    }

    #[test]
    fn a_403_for_another_reason_is_no_rate_limit() {
        let error = error_of(answered(403, r#"{"message":"Resource not accessible"}"#));
        assert_eq!(error.kind, ErrorKind::Internal);
    }

    #[test]
    fn a_502_names_the_status() {
        let error = error_of(answered(502, "<html>Bad Gateway</html>"));
        assert_eq!(error.kind, ErrorKind::Internal);
        assert!(error.message.contains("502"), "{}", error.message);
    }

    #[test]
    fn a_graphql_error_passes_on_githubs_words() {
        let error = error_of(answered(
            200,
            r#"{"errors":[{"message":"Something broke"}]}"#,
        ));
        assert_eq!(error.kind, ErrorKind::Internal);
        assert_eq!(error.message, "GitHub said: Something broke");
    }

    #[test]
    fn a_graphql_error_of_another_type_passes_on_githubs_words() {
        let error = error_of(answered(
            200,
            r#"{"errors":[{"type":"NOT_FOUND","message":"No such thing"}]}"#,
        ));
        assert_eq!(error.kind, ErrorKind::Internal);
        assert_eq!(error.message, "GitHub said: No such thing");
    }

    #[test]
    fn a_null_field_doesnt_hide_the_error_that_explains_it() {
        let body = r#"{"data":{"viewer":null},"errors":[{"type":"RATE_LIMITED","message":"x"}]}"#;
        assert_eq!(error_of(answered(200, body)).kind, limited_until(None));
    }

    #[test]
    fn data_of_the_wrong_shape_is_internal() {
        let error = error_of(answered(200, r#"{"data":{"viewer":null}}"#));
        assert_eq!(error.kind, ErrorKind::Internal);
    }

    #[test]
    fn an_unreadable_answer_is_internal() {
        assert_eq!(
            error_of(answered(200, "not json")).kind,
            ErrorKind::Internal
        );
    }

    #[test]
    fn an_answer_without_data_is_internal() {
        assert_eq!(error_of(answered(200, "{}")).kind, ErrorKind::Internal);
    }
}
