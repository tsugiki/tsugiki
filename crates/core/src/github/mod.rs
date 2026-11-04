//! Tsugiki's GitHub client. It talks through a [`Transport`], so tests replay recorded answers (ADR 0009).

pub mod issues;
pub mod live;
pub mod pull_requests;
pub mod record;
pub mod replay;

use std::borrow::Cow;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;
use time::{Date, Duration};

use crate::error::{Error, ErrorKind};
use crate::repository::{RepositoryCheck, RepositoryCounts, RepositoryName, WINDOW_DAYS};
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

/// GitHub couldn't be reached at all.
#[derive(Debug, Clone, Copy)]
pub struct Unreachable;

/// How a request reaches GitHub: the network in the app, recorded answers in tests.
pub trait Transport: Send {
    fn post(&self, token: &Token, request: &Request<'_>) -> Result<Reply, Unreachable>;
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
                                $closed: String!) { repository(owner: $owner, name: $name) { nameWithOwner \
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
            issues: Total,
            pull_requests: Total,
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
            | ErrorKind::Internal => problem,
        })?;
        Ok(RepositoryCheck::new(
            data.repository.name_with_owner,
            RepositoryCounts {
                open_issues: data.repository.issues.total_count,
                open_prs: data.repository.pull_requests.total_count,
                updated_in_window: data.updated.issue_count,
                closed_in_window: data.closed.issue_count,
            },
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
            Some(GraphqlErrorKind::Other) | None => {
                internal(&format!("GitHub said: {}", error.message))
            }
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
mod tests {
    use super::testing::{Canned, answered, token};
    use super::*;
    use crate::conventions::Conventions;
    use crate::repository::RepositorySize;
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
    fn a_login_is_the_same_user_whatever_its_case() {
        assert_eq!(Login::new("Octocat"), Login::new("octocat"));
        assert_ne!(Login::new("octocat"), Login::new("octodog"));
    }

    #[test]
    fn a_login_keeps_githubs_casing_for_display() {
        assert_eq!(Login::new("Octocat").as_str(), "Octocat");
    }

    #[test]
    fn github_says_who_the_user_is() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github");
        let viewer = client(replay::Replayer::new(fixtures)).viewer().unwrap();
        assert_eq!(viewer.login, Login::new("octocat"));
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
            r#"{"errors":[{"type":"FORBIDDEN","message":"No such thing"}]}"#,
        ));
        assert_eq!(error.kind, ErrorKind::Internal);
        assert_eq!(error.message, "GitHub said: No such thing");
    }

    #[test]
    fn a_graphql_not_found_is_not_found() {
        let error = error_of(answered(
            200,
            r#"{"errors":[{"type":"NOT_FOUND","message":"No such thing"}]}"#,
        ));
        assert_eq!(error.kind, ErrorKind::NotFound);
        assert_eq!(error.message, "GitHub said: No such thing");
    }

    fn the_day() -> Date {
        Date::from_calendar_date(2026, time::Month::November, 4).unwrap()
    }

    fn name(text: &str) -> RepositoryName {
        RepositoryName::parse(text).unwrap()
    }

    /// Holds on to what it was asked, so a test can read the variables a query went out with.
    struct Asked(std::sync::mpsc::Sender<Value>);

    impl Transport for Asked {
        #[expect(
            clippy::unwrap_in_result,
            reason = "the `Err` here stands for GitHub, and a closed channel is the test's own mistake"
        )]
        fn post(&self, _: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
            self.0.send(request.variables.clone()).unwrap();
            Err(Unreachable)
        }
    }

    #[test]
    fn checking_a_repository_reads_a_recorded_answer() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github");
        let check = client(replay::Replayer::new(fixtures))
            .check_repository(&name("python/cpython"), the_day())
            .unwrap();
        assert_eq!(check.repository.name, name("python/cpython"));
        assert_eq!(check.repository.conventions, Conventions::Cpython);
        assert_eq!((check.open_issues, check.open_prs), (6936, 2308));
        assert_eq!(check.size, RepositorySize::Large);
        assert_eq!(check.first_sync_items, 4755);
    }

    #[test]
    fn a_check_asks_about_the_last_30_days_and_keeps_the_name_out_of_the_query() {
        let (asked, seen) = std::sync::mpsc::channel();
        drop(client(Asked(asked)).check_repository(&name("a-b/c.d"), the_day()));
        assert_eq!(
            seen.recv().unwrap(),
            json!({
                "owner": "a-b",
                "name": "c.d",
                "updated": "repo:a-b/c.d updated:>=2026-10-05",
                "closed": "repo:a-b/c.d is:closed closed:>=2026-10-05",
            })
        );
        assert!(!CHECK_REPOSITORY.contains("a-b"));
    }

    #[test]
    fn a_check_gives_the_name_as_github_spells_it() {
        let body = r#"{"data":{"repository":{"nameWithOwner":"Sergey-Miryanov/GCMon",
            "issues":{"totalCount":10},"pullRequests":{"totalCount":3}},
            "updated":{"issueCount":43},"closed":{"issueCount":35}}}"#;
        let check = client(answered(200, body))
            .check_repository(&name("sergey-miryanov/gcmon"), the_day())
            .unwrap();
        assert_eq!(check.repository.name.to_string(), "Sergey-Miryanov/GCMon");
        assert_eq!(check.size, RepositorySize::Small);
        assert_eq!(check.first_sync_items, 48);
    }

    /// Answers as GitHub did for `mitsuhiko/flask`, which became `pallets/flask`: the Repository follows the
    /// rename and the searches find nothing under the old name.
    struct Renamed;

    impl Transport for Renamed {
        fn post(&self, _: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
            let found = if request.variables["owner"] == "pallets" {
                13
            } else {
                0
            };
            let body = json!({"data": {
                "repository": {
                    "nameWithOwner": "pallets/flask",
                    "issues": {"totalCount": 1},
                    "pullRequests": {"totalCount": 3},
                },
                "updated": {"issueCount": found},
                "closed": {"issueCount": found},
            }});
            Ok(Reply {
                status: 200,
                reset: None,
                body: body.to_string(),
            })
        }
    }

    #[test]
    fn a_renamed_repository_is_counted_under_its_new_name() {
        let check = client(Renamed)
            .check_repository(&name("mitsuhiko/flask"), the_day())
            .unwrap();
        assert_eq!(check.repository.name, name("pallets/flask"));
        assert_eq!(check.first_sync_items, 17);
    }

    #[test]
    fn a_repository_github_doesnt_have_says_which_and_why_it_might_be_hidden() {
        // GitHub's own answer for a name it doesn't know, which also holds the searches' counts.
        let body = r#"{"data":{"repository":null,"updated":{"issueCount":0},"closed":{"issueCount":0}},
            "errors":[{"type":"NOT_FOUND","path":["repository"],
            "message":"Could not resolve to a Repository with the name 'python/no-such'."}]}"#;
        let error = client(answered(200, body))
            .check_repository(&name("python/no-such"), the_day())
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::NotFound);
        assert_eq!(
            error.message,
            "GitHub has no Repository named python/no-such, or your token can't see it."
        );
    }

    #[test]
    fn a_check_that_fails_for_another_reason_says_that_reason() {
        let error = client(Canned(Err(Unreachable)))
            .check_repository(&name("a/b"), the_day())
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Offline);
        assert_eq!(error.message, "Tsugiki can't reach GitHub.");
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
