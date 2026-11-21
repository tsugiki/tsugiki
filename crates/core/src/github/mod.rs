//! Tsugiki's GitHub client. It talks through a [`Transport`], so tests replay recorded answers (ADR 0009).

pub mod activity;
pub mod checks;
pub mod clock;
pub mod filters;
mod gate;
mod https;
pub mod issue_content;
pub mod issues;
pub mod item;
pub mod labels;
pub mod live;
pub mod points;
pub mod pull_request_content;
pub mod pull_requests;
pub mod record;
pub mod references;
pub mod refresh;
pub mod replay;
pub mod window;

use std::borrow::Cow;
use std::cell::Cell;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use time::{Date, Duration, OffsetDateTime};

use crate::error::{Error, ErrorKind};
use crate::github::clock::{GitHubClock, skew_warning};
use crate::github::gate::Gate;
use crate::github::points::HourPoints;
use crate::github::pull_requests::RateLimit;
use crate::login::Login;
use crate::repository::{FoundRepository, RepositoryCounts, RepositoryName, WINDOW_DAYS};
use crate::token::{Token, TokenFrom};
pub use tsugiki_token::RestPath;

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
    /// The `Date` header: GitHub's time when it answered.
    pub date: Option<OffsetDateTime>,
    pub body: String,
}

/// A REST request that can answer "not modified", which a refresh asks first (ADR 0015).
pub struct ConditionalGet<'a> {
    /// The path on GitHub's API, from its first `/`, with its query.
    pub path: &'a RestPath,
    /// The tag of the last answer, sent as `If-None-Match`.
    pub etag: Option<&'a str>,
}

/// What GitHub answered to a [`ConditionalGet`]. A refresh reads these two and drops the body.
#[derive(Debug, Clone)]
pub struct ConditionalReply {
    pub status: u16,
    /// The `ETag` header.
    pub etag: Option<String>,
    /// The `Date` header: GitHub's time when it answered.
    pub date: Option<OffsetDateTime>,
}

/// GitHub couldn't be reached at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreachable {
    /// No network, no answer in time, or a connection that failed some other way, a certificate that failed
    /// for a reason other than its dates among them.
    Offline,
    /// GitHub's certificate isn't valid at this computer's date and time. The clock may be wrong, or the
    /// certificate may have expired, and the check can't tell which.
    CertificateDates,
}

impl From<Unreachable> for Error {
    fn from(unreachable: Unreachable) -> Self {
        match unreachable {
            Unreachable::Offline => Error::new(ErrorKind::Offline, "Tsugiki can't reach GitHub."),
            Unreachable::CertificateDates => Error::new(
                ErrorKind::CertificateDates,
                "GitHub's certificate isn't valid by this computer's clock. Check the date and time.",
            ),
        }
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
        Err(Unreachable::Offline)
    }
}

/// How to reach GitHub: the network, the environment and gh in the app, stand-ins in tests.
pub struct GitHubAccess {
    pub transport: Box<dyn Transport>,
    pub token_from: TokenFrom,
}

/// The signed-in user.
#[derive(Debug, PartialEq, Eq, Deserialize)]
pub struct Viewer {
    pub login: Login,
}

/// The fields of `rateLimit` each query asks for, which `RateLimit` reads. An answer whose points don't parse
/// fails, so a query that missed a field would fail each time.
const RATE_LIMIT_FIELDS: &str = "rateLimit { cost limit used remaining resetAt }";

/// The question of who is signed in. The status refresh asks it once a minute, and each query costs at least
/// a point, so it asks what its points were too.
fn viewer_request() -> Request<'static> {
    Request {
        operation: "Viewer",
        query: format!("query Viewer {{ viewer {{ login }} {RATE_LIMIT_FIELDS} }}").into(),
        variables: Value::Null,
    }
}

fn check_repository_query() -> String {
    format!(
        "query CheckRepository($owner: String!, $name: String!, $updated: String!, \
         $closed: String!) {{ repository(owner: $owner, name: $name) {{ nameWithOwner diskUsage \
         issues(states: OPEN) {{ totalCount }} pullRequests(states: OPEN) {{ totalCount }} }} \
         updated: search(query: $updated, type: ISSUE_ADVANCED, first: 0) {{ issueCount }} \
         closed: search(query: $closed, type: ISSUE_ADVANCED, first: 0) {{ issueCount }} \
         {RATE_LIMIT_FIELDS} }}"
    )
}

const REPOSITORY_SIZE: &str = "query RepositorySize($owner: String!, $name: String!) { \
                               repository(owner: $owner, name: $name) { diskUsage } }";

/// A GraphQL client that holds the token, so no caller has to.
pub struct GitHub {
    transport: Box<dyn Transport>,
    token: Token,
    /// GitHub's time, as its last dated reply told it. It lives in memory only, because a saved time is
    /// already in GitHub's time.
    clock: Cell<GitHubClock>,
    /// The hour's points, summed from every answer that reported them, whichever job asked. Nothing until
    /// the first such answer.
    points: Cell<Option<HourPoints>>,
    /// The rate limit's gate, which each request passes. It holds while `points` are below the floor, and
    /// after GitHub refused a request for its rate limit.
    gate: Cell<Gate>,
}

impl GitHub {
    pub fn new(transport: Box<dyn Transport>, token: Token) -> Self {
        Self {
            transport,
            token,
            clock: Cell::new(GitHubClock::default()),
            points: Cell::new(None),
            gate: Cell::new(Gate::default()),
        }
    }

    /// GitHub's clock, as its replies so far tell it.
    pub fn clock(&self) -> GitHubClock {
        self.clock.get()
    }

    /// The time by this computer's clock that GitHub's points come back, while the gate holds every
    /// request: below the floor of points, or after GitHub refused a request for its rate limit.
    pub fn points_back(&self) -> Option<OffsetDateTime> {
        let clock = self.clock.get();
        let until = self
            .gate
            .get()
            .holds_until(self.points.get(), clock.now())?;
        Some(clock.local(until))
    }

    /// Lets a request through the gate, or answers at once, with the time the points come back, while the
    /// gate holds.
    fn pass_gate(&self) -> Result<(), Error> {
        match self.points_back() {
            Some(back) => Err(rate_limited(Some(back), HELD)),
            None => Ok(()),
        }
    }

    /// Notes in the gate a refusal for GitHub's rate limit, and gives `problem` the time by this computer's
    /// clock the gate opens. GitHub's reset is a time on its own clock. Any other problem passes as it is.
    fn note_refusal(&self, problem: Error) -> Error {
        let ErrorKind::RateLimited { resets_at } = problem.kind else {
            return problem;
        };
        let reset = resets_at.and_then(|at| OffsetDateTime::from_unix_timestamp(at.into()).ok());
        let gate = self.gate.get().refused(reset, self.clock.get().now());
        self.gate.set(gate);
        rate_limited(self.points_back(), &problem.message)
    }

    /// The hour's points as this run's answers count them. Nothing before the first answer that said, and
    /// nothing once the hour has reset by GitHub's clock, until an answer of the next hour says.
    pub fn hour_points(&self) -> Option<HourPoints> {
        self.shown_hour().map(|(points, _)| points)
    }

    /// The time left until the hour's points reset by GitHub's clock, while there are any to show.
    pub fn hour_left(&self) -> Option<std::time::Duration> {
        self.shown_hour()
            .and_then(|(_, left)| std::time::Duration::try_from(left).ok())
    }

    /// The hour's points and the time left until they reset, by one reading of GitHub's clock, so the points
    /// and the time left agree.
    fn shown_hour(&self) -> Option<(HourPoints, Duration)> {
        let points = self.points.get()?;
        let left = points.resets_at - self.clock.get().now();
        left.is_positive().then_some((points, left))
    }

    /// Adds the points of an answer to the hour's.
    fn count(&self, answer: RateLimit) {
        let now = self.clock.get().now();
        if let Some(points) = HourPoints::after(self.points.get(), answer, now) {
            self.points.set(Some(points));
        }
    }

    /// Turns `reply` into its data, and counts the points it cost. A refusal for the rate limit holds the
    /// gate.
    fn answer<D: DeserializeOwned>(&self, reply: &Reply) -> Result<D, Error> {
        read(reply, |answer| self.count(answer)).map_err(|problem| self.note_refusal(problem))
    }

    /// Reads `reply` as `answer` does, leaving out each field under `repository` that `refusable` names and
    /// GitHub refused, as `read_with_refused` says.
    fn answer_with_refused<D: DeserializeOwned>(
        &self,
        reply: &Reply,
        refusable: impl Fn(&str) -> bool,
    ) -> Result<(D, Vec<Refused>), Error> {
        read_with_refused(reply, |answer| self.count(answer), refusable)
            .map_err(|problem| self.note_refusal(problem))
    }

    /// Sends `request` if the gate lets it through, and sets GitHub's clock by the reply's `Date`.
    fn post(&self, request: &Request<'_>) -> Result<Reply, Error> {
        self.pass_gate()?;
        let reply = self.transport.post(&self.token, request)?;
        if reply.status == 200 {
            self.set_clock(reply.date);
        }
        Ok(reply)
    }

    /// Sets GitHub's clock by `date`, from a reply GitHub sent. Another status can come from a proxy, whose
    /// clock is its own. No `date` leaves the clock as it was.
    fn set_clock(&self, date: Option<OffsetDateTime>) {
        let Some(date) = date else {
            return;
        };
        let (clock, skew) = self.clock.get().after(date, OffsetDateTime::now_utc());
        self.clock.set(clock);
        if let Some(skew) = skew {
            log::warn!("{}", skew_warning(skew));
        }
    }

    /// The size of `repository` on GitHub in bytes, about what a clone of it takes. GitHub gives kilobytes,
    /// or nothing for a repository with no commits.
    pub fn repository_size(&self, repository: &RepositoryName) -> Result<u64, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Holder {
            disk_usage: Option<u64>,
        }
        #[derive(Deserialize)]
        struct Data {
            repository: Holder,
        }
        let data: Data = self.query(&Request {
            operation: "RepositorySize",
            query: Cow::Borrowed(REPOSITORY_SIZE),
            variables: json!({ "owner": repository.owner(), "name": repository.name() }),
        })?;
        Ok(data.repository.disk_usage.unwrap_or(0).saturating_mul(1024))
    }

    /// Asks GitHub who the token belongs to.
    pub fn viewer(&self) -> Result<Viewer, Error> {
        #[derive(Deserialize)]
        struct Data {
            viewer: Viewer,
        }
        let data: Data = self.query(&viewer_request())?;
        Ok(data.viewer)
    }

    /// Asks GitHub, in one query, whether `name` exists, what it holds and what the user may do in it (ADR
    /// 0015). The answer carries the name as GitHub spells it, which follows a rename and fixes the case.
    ///
    /// A search doesn't follow a rename, and counts nothing under the old name, so that case takes a second
    /// query with the new one.
    pub fn check_repository(
        &self,
        name: &RepositoryName,
        today: Date,
    ) -> Result<FoundRepository, Error> {
        let found = self.count_repository(name, today)?;
        if found.name.is_same(name) {
            Ok(found)
        } else {
            self.count_repository(&found.name, today)
        }
    }

    fn count_repository(
        &self,
        name: &RepositoryName,
        today: Date,
    ) -> Result<FoundRepository, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Repository {
            name_with_owner: RepositoryName,
            /// In kilobytes, or nothing for a Repository with no commits.
            disk_usage: Option<u64>,
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
            query: check_repository_query().into(),
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
            | ErrorKind::CertificateDates
            | ErrorKind::Unauthorized
            | ErrorKind::WriteRefused
            | ErrorKind::RateLimited { .. }
            | ErrorKind::Conflict
            | ErrorKind::QueryRefused
            | ErrorKind::Internal => problem,
        })?;
        Ok(FoundRepository {
            name: data.repository.name_with_owner,
            counts: RepositoryCounts {
                open_issues: data.repository.issues.total_count,
                open_prs: data.repository.pull_requests.total_count,
                updated_in_window: data.updated.issue_count,
                closed_in_window: data.closed.issue_count,
                disk_kb: data.repository.disk_usage.unwrap_or(0),
            },
        })
    }

    fn query<D: DeserializeOwned>(&self, request: &Request<'_>) -> Result<D, Error> {
        let reply = self.post(request)?;
        self.answer(&reply)
    }
}

/// The status GitHub answers with when a query ran out of its 10 seconds (ADR 0015).
const BAD_GATEWAY: u16 = 502;

const RATE_LIMITED: &str =
    "GitHub's rate limit has been reached. Tsugiki can read again after it resets.";

const HELD: &str = "GitHub's rate limit is low, so Tsugiki leaves the hour's last points to your other \
                    tools until it resets.";

/// The error for a request GitHub's rate limit holds back, which can go at `back`, if known.
fn rate_limited(back: Option<OffsetDateTime>, message: &str) -> Error {
    let resets_at = back.and_then(|at| u32::try_from(at.unix_timestamp()).ok());
    Error::new(ErrorKind::RateLimited { resets_at }, message)
}

/// Turns an answer into its data, or into the error the UI can act on (ADR 0008). An answer whose data
/// reports its points hands them to `count` first, even when the query failed, since GitHub charged for it.
fn read<D: DeserializeOwned>(reply: &Reply, count: impl FnOnce(RateLimit)) -> Result<D, Error> {
    let (data, _refused) = read_with_refused(reply, count, |_| false)?;
    Ok(data)
}

/// A field under `repository` that GitHub refused while it answered the others, with what it said.
struct Refused {
    field: String,
    message: String,
}

/// Reads an answer as `read` does, but an error of any type whose path is `repository` and one field that
/// `refusable` names leaves only that field out: GitHub answers it with null and the rest stays good.
fn read_with_refused<D: DeserializeOwned>(
    reply: &Reply,
    count: impl FnOnce(RateLimit),
    refusable: impl Fn(&str) -> bool,
) -> Result<(D, Vec<Refused>), Error> {
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
        #[serde(rename = "FORBIDDEN")]
        Forbidden,
        #[serde(other)]
        Other,
    }

    let internal = |message: &str| Error::new(ErrorKind::Internal, message);
    // The reset is GitHub's here, which the client turns into this computer's as it holds its gate.
    let refused = || {
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
        429 => return Err(refused()),
        403 if reply.body.contains("rate limit") => return Err(refused()),
        status => return Err(internal(&format!("GitHub answered with status {status}."))),
    }

    // The parser's words mean nothing to a user, so they only go to the log.
    let unreadable = |cause: serde_json::Error| {
        log::error!("GitHub's answer didn't parse: {cause}");
        internal("GitHub sent an answer Tsugiki can't read.")
    };
    // The data is read after the errors: a field that failed comes back null, which `D` may not allow.
    let envelope: Envelope = serde_json::from_str(&reply.body).map_err(unreadable)?;
    let points = envelope
        .data
        .as_ref()
        .and_then(|data| data.get("rateLimit"))
        .filter(|points| !points.is_null())
        .map(RateLimit::deserialize)
        .transpose();
    if let Ok(Some(points)) = points {
        count(points);
    }
    // `nodes(ids:)` answers null for an ID GitHub no longer knows, and adds this error. `issues.rs` gets the
    // same for a number nothing has, one level inside `repository`. The others are still good.
    let gone_node = |error: &&GraphqlError| {
        let place = error.path.first().and_then(Value::as_str);
        matches!(error.kind, Some(GraphqlErrorKind::NotFound))
            && (place == Some("nodes") || (place == Some("repository") && error.path.len() == 2))
    };
    // A cross-reference can come from something this token can't see, such as a repository behind SAML. Its
    // event comes back null, and a PR Tsugiki can't read can't join a tree. Failing would hold back a whole
    // page of Issues on every retry.
    let hidden_source = |error: &&GraphqlError| {
        matches!(
            error.kind,
            Some(GraphqlErrorKind::NotFound | GraphqlErrorKind::Forbidden)
        ) && error.path.last().and_then(Value::as_str) == Some("source")
    };
    let refused_field = |error: &GraphqlError| match error.path.as_slice() {
        [Value::String(place), Value::String(field)]
            if place == "repository" && refusable(field) =>
        {
            Some(field.clone())
        }
        _ => None,
    };
    let failed = |error: &&GraphqlError| {
        !gone_node(error) && !hidden_source(error) && refused_field(error).is_none()
    };
    if let Some(error) = envelope.errors.iter().find(failed) {
        return Err(match error.kind {
            Some(GraphqlErrorKind::RateLimited) => refused(),
            Some(GraphqlErrorKind::NotFound) => Error::new(
                ErrorKind::NotFound,
                &format!("GitHub said: {}", error.message),
            ),
            Some(GraphqlErrorKind::Forbidden | GraphqlErrorKind::Other) | None => Error::new(
                ErrorKind::QueryRefused,
                &format!("GitHub said: {}", error.message),
            ),
        });
    }
    if let Err(cause) = points {
        return Err(unreadable(cause));
    }
    let refused = (envelope.errors.iter())
        .filter(|error| !gone_node(error))
        .filter_map(|error| {
            let field = refused_field(error)?;
            let message = error.message.clone();
            Some(Refused { field, message })
        })
        .collect();
    let data = envelope
        .data
        .ok_or_else(|| internal("GitHub sent an answer with no data."))?;
    Ok((serde_json::from_value(data).map_err(unreadable)?, refused))
}

/// The fake GitHub the core's tests share, and share with other crates' tests.
#[cfg(any(test, feature = "testing"))]
pub mod testing;

#[cfg(test)]
mod tests;
