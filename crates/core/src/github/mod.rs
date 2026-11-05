//! Tsugiki's GitHub client. It talks through a [`Transport`], so tests replay recorded answers (ADR 0009).

pub mod activity;
pub mod checks;
pub mod filters;
pub mod issues;
pub mod live;
pub mod pull_requests;
pub mod record;
pub mod refresh;
pub mod replay;
pub mod window;

use std::borrow::Cow;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;
use time::{Date, Duration};

use crate::error::{Error, ErrorKind};
use crate::repository::{
    RepositoryAccess, RepositoryCheck, RepositoryCounts, RepositoryName, WINDOW_DAYS,
};
use crate::token::{RunGh, Token};

/// One GraphQL request. `operation` names the query, and the replayer finds its recording by that name.
pub struct Request<'a> {
    pub operation: &'a str,
    pub query: Cow<'a, str>,
    /// What the query's `$` names stand for, so what a user typed can't change the query.
    pub variables: Value,
}

/// What GitHub answered.
#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    /// The `x-ratelimit-reset` header, in seconds since the Unix epoch.
    pub reset: Option<u32>,
    pub body: String,
}

/// A REST request that can answer "not modified", which a refresh asks first (ADR 0015).
pub struct ConditionalGet<'a> {
    /// The path on GitHub's API, from its first `/`.
    pub path: &'a str,
    /// The tag of the last answer, sent as `If-None-Match`.
    pub etag: Option<&'a str>,
}

/// What GitHub answered to a [`ConditionalGet`]. A refresh reads these two and drops the body.
#[derive(Debug, Clone)]
pub struct ConditionalReply {
    pub status: u16,
    /// The `ETag` header.
    pub etag: Option<String>,
}

/// GitHub couldn't be reached at all.
#[derive(Debug, Clone, Copy)]
pub struct Unreachable;

/// The number `digits` spell. GitHub's numbers are GraphQL `Int`s, 32 bits and signed, and it refuses a whole
/// query over one that doesn't fit.
pub(crate) fn issue_number(digits: &str) -> Option<u32> {
    digits
        .chars()
        .all(|c| c.is_ascii_digit())
        .then(|| digits.parse::<i32>().ok())
        .flatten()
        .and_then(|number| u32::try_from(number).ok())
}

/// The number in `gh-N`, whatever its case.
pub(crate) fn gh_number(word: &str) -> Option<u32> {
    let digits = word.get(3..)?;
    word.get(..3)?
        .eq_ignore_ascii_case("gh-")
        .then(|| issue_number(digits))
        .flatten()
}

/// A commit's SHA in lower case, whole or cut short as a person wrote it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CommitSha(String);

impl CommitSha {
    /// Git's shortest default abbreviation, and the length of a whole SHA-1.
    const LENGTHS: std::ops::RangeInclusive<usize> = 7..=40;

    /// The SHA that `text` spells, if it spells one.
    pub fn parse(text: &str) -> Option<Self> {
        let is_sha =
            Self::LENGTHS.contains(&text.len()) && text.chars().all(|c| c.is_ascii_hexdigit());
        is_sha.then(|| Self(text.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// True when `shorter` is this SHA or the start of it.
    pub fn starts_with(&self, shorter: &Self) -> bool {
        self.0.starts_with(&shorter.0)
    }
}

/// How a request reaches GitHub: the network in the app, recorded answers in tests.
pub trait Transport: Send {
    fn post(&self, token: &Token, request: &Request<'_>) -> Result<Reply, Unreachable>;

    /// Recordings hold GraphQL answers only (ADR 0009). A transport with no REST counts as unreachable here,
    /// and a refresh then asks by GraphQL.
    fn get(
        &self,
        _token: &Token,
        _request: &ConditionalGet<'_>,
    ) -> Result<ConditionalReply, Unreachable> {
        Err(Unreachable)
    }
}

/// How to reach GitHub: the network and gh in the app, stand-ins in tests.
pub struct GitHubAccess {
    pub transport: Box<dyn Transport>,
    pub run_gh: RunGh,
}

/// A GitHub username. It reaches the UI as a plain string, in GitHub's casing.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct Login(String);

/// GitHub logins ignore case, so `Octocat` and `octocat` are one user, and "is this me?" has one answer.
impl PartialEq for Login {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for Login {}

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
    query: Cow::Borrowed("query Viewer { viewer { login } }"),
    variables: Value::Null,
};

const CHECK_REPOSITORY: &str = "query CheckRepository($owner: String!, $name: String!, $updated: String!, \
                                $closed: String!) { repository(owner: $owner, name: $name) { nameWithOwner viewerPermission \
                                issues(states: OPEN) { totalCount } pullRequests(states: OPEN) { totalCount } } \
                                updated: search(query: $updated, type: ISSUE_ADVANCED, first: 0) { issueCount } \
                                closed: search(query: $closed, type: ISSUE_ADVANCED, first: 0) { issueCount } }";

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

    /// Asks GitHub, in one query, whether `name` exists and how much adding it would load (ADR 0015). The
    /// answer carries the name as GitHub spells it, which follows a rename and fixes the case.
    ///
    /// A search doesn't follow a rename, and counts nothing under the old name, so that case takes a second
    /// query with the new one.
    pub fn check_repository(
        &self,
        name: &RepositoryName,
        today: Date,
    ) -> Result<RepositoryCheck, Error> {
        let check = self.count_repository(name, today)?;
        if check.repository.name.is_same(name) {
            Ok(check)
        } else {
            self.count_repository(&check.repository.name, today)
        }
    }

    fn count_repository(
        &self,
        name: &RepositoryName,
        today: Date,
    ) -> Result<RepositoryCheck, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Repository {
            name_with_owner: RepositoryName,
            viewer_permission: Option<Permission>,
            issues: Total,
            pull_requests: Total,
        }
        #[derive(Deserialize)]
        enum Permission {
            #[serde(rename = "ADMIN")]
            Admin,
            #[serde(other)]
            Lower,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Total {
            total_count: u32,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Found {
            issue_count: u32,
        }
        #[derive(Deserialize)]
        struct Data {
            repository: Repository,
            updated: Found,
            closed: Found,
        }

        let since = today.saturating_sub(Duration::days(i64::from(WINDOW_DAYS)));
        let request = Request {
            operation: "CheckRepository",
            query: CHECK_REPOSITORY.into(),
            variables: json!({
                "owner": name.owner(),
                "name": name.name(),
                "updated": format!("repo:{name} updated:>={since}"),
                "closed": format!("repo:{name} is:closed closed:>={since}"),
            }),
        };
        let data: Data = self.query(&request).map_err(|problem| match problem.kind {
            ErrorKind::NotFound => Error::new(
                ErrorKind::NotFound,
                &format!("GitHub has no Repository named {name}, or your token can't see it."),
            ),
            ErrorKind::Offline
            | ErrorKind::Unauthorized
            | ErrorKind::WriteRefused
            | ErrorKind::RateLimited { .. }
            | ErrorKind::Conflict
            | ErrorKind::QueryRefused
            | ErrorKind::Internal => problem,
        })?;
        let access = match data.repository.viewer_permission {
            Some(Permission::Admin) => RepositoryAccess::Maintained,
            Some(Permission::Lower) | None => RepositoryAccess::Other,
        };
        Ok(RepositoryCheck::new(
            data.repository.name_with_owner,
            RepositoryCounts {
                open_issues: data.repository.issues.total_count,
                open_prs: data.repository.pull_requests.total_count,
                updated_in_window: data.updated.issue_count,
                closed_in_window: data.closed.issue_count,
            },
            access,
        ))
    }

    fn query<D: DeserializeOwned>(&self, request: &Request<'_>) -> Result<D, Error> {
        let reply = self
            .transport
            .post(&self.token, request)
            .map_err(|Unreachable| Error::new(ErrorKind::Offline, "Tsugiki can't reach GitHub."))?;
        read(&reply)
    }
}

/// The status GitHub answers with when a query ran out of its 10 seconds (ADR 0015).
const BAD_GATEWAY: u16 = 502;

const RATE_LIMITED: &str =
    "GitHub's rate limit has been reached. Tsugiki can read again after it resets.";

/// Turns an answer into its data, or into the error the UI can act on (ADR 0008).
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
        #[serde(default)]
        path: Vec<Value>,
    }
    #[derive(Deserialize)]
    enum GraphqlErrorKind {
        #[serde(rename = "RATE_LIMITED")]
        RateLimited,
        #[serde(rename = "NOT_FOUND")]
        NotFound,
        #[serde(other)]
        Other,
    }

    let internal = |message: &str| Error::new(ErrorKind::Internal, message);
    let rate_limited = || {
        let resets_at = reply.reset;
        Error::new(ErrorKind::RateLimited { resets_at }, RATE_LIMITED)
    };
    if reply.status != 200 {
        log::warn!(
            "GitHub answered with status {}: {}",
            reply.status,
            reply.body
        );
    }
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

    // The parser's words mean nothing to a user, so they only go to the log.
    let unreadable = |cause: serde_json::Error| {
        log::error!("GitHub's answer didn't parse: {cause}");
        internal("GitHub sent an answer Tsugiki can't read.")
    };
    // The data is read after the errors: a field that failed comes back null, which `D` may not allow.
    let envelope: Envelope = serde_json::from_str(&reply.body).map_err(unreadable)?;
    // `nodes(ids:)` answers null for an ID GitHub no longer knows, and adds this error. `issues.rs` gets the
    // same for a number nothing has, one level inside `repository`. The others are still good.
    let gone_node = |error: &&GraphqlError| {
        let place = error.path.first().and_then(Value::as_str);
        matches!(error.kind, Some(GraphqlErrorKind::NotFound))
            && (place == Some("nodes") || (place == Some("repository") && error.path.len() == 2))
    };
    if let Some(error) = envelope.errors.iter().find(|error| !gone_node(error)) {
        return Err(match error.kind {
            Some(GraphqlErrorKind::RateLimited) => rate_limited(),
            Some(GraphqlErrorKind::NotFound) => Error::new(
                ErrorKind::NotFound,
                &format!("GitHub said: {}", error.message),
            ),
            Some(GraphqlErrorKind::Other) | None => Error::new(
                ErrorKind::QueryRefused,
                &format!("GitHub said: {}", error.message),
            ),
        });
    }
    let data = envelope
        .data
        .ok_or_else(|| internal("GitHub sent an answer with no data."))?;
    serde_json::from_value(data).map_err(unreadable)
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

    /// Answers from a list, in order, and records each operation and its variables for the test.
    pub(crate) struct Scripted {
        replies: std::cell::RefCell<std::collections::VecDeque<Result<Reply, Unreachable>>>,
        asked: std::sync::mpsc::Sender<(String, Value)>,
    }

    impl Scripted {
        pub(crate) fn new(
            replies: Vec<Result<Reply, Unreachable>>,
        ) -> (Self, std::sync::mpsc::Receiver<(String, Value)>) {
            let (asked, seen) = std::sync::mpsc::channel();
            let replies = std::cell::RefCell::new(replies.into());
            (Self { replies, asked }, seen)
        }
    }

    impl Transport for Scripted {
        fn post(&self, _: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
            drop(
                self.asked
                    .send((request.operation.to_owned(), request.variables.clone())),
            );
            self.replies
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err(Unreachable))
        }
    }

    pub(crate) fn reply_ok(body: &str) -> Result<Reply, Unreachable> {
        Ok(Reply {
            status: 200,
            reset: None,
            body: body.to_owned(),
        })
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
mod tests;
