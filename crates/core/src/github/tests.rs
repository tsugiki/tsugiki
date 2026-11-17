use super::testing::{Canned, Scripted, answered, dated, github,reply_ok, signed_in_as};
use super::*;
use std::path::Path;
use time::format_description::well_known::Rfc3339;

fn limited_until(resets_at: Option<u32>) -> ErrorKind {
    ErrorKind::RateLimited { resets_at }
}

fn error_of(transport: Canned) -> crate::error::Error {
    github(transport).viewer().unwrap_err()
}

#[test]
fn github_says_who_the_user_is() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github");
    let viewer = github(replay::Replayer::new(fixtures)).viewer().unwrap();
    assert_eq!(viewer.login, Login::new("octocat"));
}

#[test]
fn no_connection_means_offline() {
    assert_eq!(
        error_of(Canned(Err(Unreachable::Offline))).kind,
        ErrorKind::Offline
    );
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
        date: None,
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
        date: None,
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
fn a_graphql_error_without_a_type_passes_on_githubs_words() {
    let error = error_of(answered(
        200,
        r#"{"errors":[{"message":"Something broke"}]}"#,
    ));
    assert_eq!(error.kind, ErrorKind::QueryRefused);
    assert_eq!(error.message, "GitHub said: Something broke");
}

#[test]
fn a_graphql_forbidden_error_passes_on_githubs_words() {
    let error = error_of(answered(
        200,
        r#"{"errors":[{"type":"FORBIDDEN","message":"Resource not accessible by integration"}]}"#,
    ));
    assert_eq!(error.kind, ErrorKind::QueryRefused);
    assert_eq!(
        error.message,
        "GitHub said: Resource not accessible by integration"
    );
}

#[test]
fn a_graphql_error_of_a_type_tsugiki_doesnt_know_passes_on_githubs_words() {
    // A sync saves a refused query on its Filter and moves on (`blames_the_search` in `sync/filters.rs`), but
    // an unreadable reply fails the step. So a type `GraphqlErrorKind` doesn't name must not make the reply
    // unreadable.
    let error = error_of(answered(
        200,
        r#"{"errors":[{"type":"MAX_NODE_LIMIT_EXCEEDED","message":"Too many nodes"}]}"#,
    ));
    assert_eq!(error.kind, ErrorKind::QueryRefused);
    assert_eq!(error.message, "GitHub said: Too many nodes");
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
        Err(Unreachable::Offline)
    }
}

#[test]
fn checking_a_repository_reads_a_recorded_answer() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github");
    let found = github(replay::Replayer::new(fixtures))
        .check_repository(&name("python/cpython"), the_day())
        .unwrap();
    assert_eq!(
        found,
        FoundRepository {
            name: name("python/cpython"),
            counts: RepositoryCounts {
                open_issues: 6945,
                open_prs: 2324,
                updated_in_window: 4886,
                closed_in_window: 2613,
                disk_kb: 912_867,
            },
        }
    );
}

#[test]
fn a_check_asks_about_the_last_30_days_and_keeps_the_name_out_of_the_query() {
    let (asked, seen) = std::sync::mpsc::channel();
    drop(github(Asked(asked)).check_repository(&name("a-b/c.d"), the_day()));
    assert_eq!(
        seen.recv().unwrap(),
        json!({
            "owner": "a-b",
            "name": "c.d",
            "updated": "repo:a-b/c.d updated:>=2026-10-05",
            "closed": "repo:a-b/c.d is:closed closed:>=2026-10-05",
        })
    );
    assert!(!check_repository_query().contains("a-b"));
}

#[test]
fn a_check_gives_the_name_as_github_spells_it() {
    let body = r#"{"data":{"repository":{"nameWithOwner":"Some-Maintainer/GCMon",
            "issues":{"totalCount":10},"pullRequests":{"totalCount":3}},
            "updated":{"issueCount":43},"closed":{"issueCount":35}}}"#;
    let found = github(answered(200, body))
        .check_repository(&name("some-maintainer/gcmon"), the_day())
        .unwrap();
    assert_eq!(found.name.to_string(), "Some-Maintainer/GCMon");
    // GitHub leaves `diskUsage` out for a Repository with no commits.
    assert_eq!(
        found.counts,
        RepositoryCounts {
            open_issues: 10,
            open_prs: 3,
            updated_in_window: 43,
            closed_in_window: 35,
            disk_kb: 0,
        }
    );
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
        reply_ok(&body.to_string())
    }
}

#[test]
fn a_renamed_repository_is_counted_under_its_new_name() {
    let found = github(Renamed)
        .check_repository(&name("mitsuhiko/flask"), the_day())
        .unwrap();
    assert_eq!(found.name, name("pallets/flask"));
    assert_eq!(
        (
            found.counts.updated_in_window,
            found.counts.closed_in_window
        ),
        (13, 13)
    );
}

#[test]
fn a_repository_github_doesnt_have_says_which_and_why_it_might_be_hidden() {
    // GitHub's own answer for a name it doesn't know, which also holds the searches' counts.
    let body = r#"{"data":{"repository":null,"updated":{"issueCount":0},"closed":{"issueCount":0}},
            "errors":[{"type":"NOT_FOUND","path":["repository"],
            "message":"Could not resolve to a Repository with the name 'python/no-such'."}]}"#;
    let error = github(answered(200, body))
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
    let error = github(Canned(Err(Unreachable::Offline)))
        .check_repository(&name("a/b"), the_day())
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Offline);
    assert_eq!(error.message, "Tsugiki can't reach GitHub.");
}

#[test]
fn a_null_field_doesnt_hide_the_rate_limit_that_explains_it() {
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

#[test]
fn githubs_clock_goes_by_the_last_reply_that_has_a_date() {
    let ahead = OffsetDateTime::now_utc() + Duration::minutes(5);
    let (script, _) = Scripted::new(vec![
        dated(signed_in_as("reviewer"), ahead),
        signed_in_as("reviewer"),
    ]);
    let github = github(script);
    let before = github.clock().now();

    drop(github.viewer().unwrap());
    drop(github.viewer().unwrap());
    let now = github.clock().now();

    assert!(before < ahead - Duration::minutes(4));
    assert!(ahead <= now && now < ahead + Duration::seconds(10), "{now}");
}

#[test]
fn a_reply_github_didnt_send_leaves_its_clock() {
    let ahead = OffsetDateTime::now_utc() + Duration::minutes(5);
    let proxy = Reply {
        status: 403,
        reset: None,
        date: Some(ahead),
        body: "Blocked by your company's proxy".to_owned(),
    };
    let github = github(Canned(Ok(proxy)));

    drop(github.viewer().unwrap_err());

    assert!(github.clock().now() < ahead - Duration::minutes(4));
}

/// GitHub's answer that `reviewer` is signed in, which cost `cost` of the `used` points of an hour that
/// hasn't reset.
fn viewer_costing(cost: u32, used: u32) -> Result<Reply, Unreachable> {
    viewer_resetting(cost, used, "2999-01-01T00:00:00Z")
}

/// GitHub's answer that `reviewer` is signed in, which cost `cost` of the `used` points of the hour that
/// resets `at`.
fn viewer_resetting(cost: u32, used: u32, at: &str) -> Result<Reply, Unreachable> {
    let remaining = 5000 - used;
    super::testing::reply_ok(&format!(
        r#"{{"data":{{"viewer":{{"login":"reviewer"}},"rateLimit":{{"cost":{cost},"limit":5000,
            "used":{used},"remaining":{remaining},"resetAt":"{at}"}}}}}}"#
    ))
}

#[test]
fn the_hours_points_end_at_its_reset_by_githubs_clock_not_the_computers() {
    let now = OffsetDateTime::now_utc();
    let in_an_hour = (now + Duration::hours(1)).format(&Rfc3339).unwrap();
    let an_hour_ago = (now - Duration::hours(1)).format(&Rfc3339).unwrap();
    let cases = [
        // This computer's clock is two hours slow: by it the hour has a while to go.
        (now + Duration::hours(2), in_an_hour, None),
        // Two hours fast: by it the hour reset an hour ago.
        (now - Duration::hours(2), an_hour_ago, Some(1)),
    ];
    for (date, reset, spent) in cases {
        let (script, _) = Scripted::new(vec![dated(viewer_resetting(1, 1, &reset), date)]);
        let github = github(script);

        drop(github.viewer().unwrap());

        let points = github.hour_points();
        assert_eq!(points.map(|points| points.spent), spent, "{reset}");
    }
}

#[test]
fn the_points_of_each_answer_add_up() {
    let (script, _) = Scripted::new(vec![
        signed_in_as("reviewer"),
        viewer_costing(1, 1),
        viewer_costing(2, 3),
    ]);
    let github = github(script);

    drop(github.viewer().unwrap());
    let before = github.hour_points();
    drop(github.viewer().unwrap());
    drop(github.viewer().unwrap());

    assert_eq!(before, None, "an answer that doesn't say counts nothing");
    let points = github.hour_points().unwrap();
    assert_eq!(
        (points.spent, points.used, points.remaining, points.since),
        (3, 3, 4997, None)
    );
}

#[test]
fn a_first_answer_into_a_spent_hour_says_since_when_tsugiki_counted() {
    let at = OffsetDateTime::now_utc() - Duration::minutes(3);
    let (script, _) = Scripted::new(vec![dated(viewer_costing(1, 3350), at)]);
    let github = github(script);

    drop(github.viewer().unwrap());

    let since = github.hour_points().unwrap().since.unwrap();
    assert!(at <= since && since < at + Duration::seconds(10), "{since}");
}

#[test]
fn a_query_that_failed_still_counts_its_points() {
    let body = r#"{"data":{"viewer":null,"rateLimit":{"cost":1,"limit":5000,"used":7,"remaining":4993,
        "resetAt":"2999-01-01T00:00:00Z"}},"errors":[{"message":"Something went wrong"}]}"#;
    let github = github(answered(200, body));

    assert_eq!(github.viewer().unwrap_err().kind, ErrorKind::QueryRefused);
    assert_eq!(github.hour_points().unwrap().spent, 1);
}

#[test]
fn points_that_dont_parse_make_the_answer_unreadable() {
    let body = r#"{"data":{"viewer":{"login":"reviewer"},"rateLimit":{"remaining":4993}}}"#;
    let github = github(answered(200, body));

    assert_eq!(github.viewer().unwrap_err().kind, ErrorKind::Internal);
    assert_eq!(github.hour_points(), None);
}
