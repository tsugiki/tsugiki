use super::testing::{
    Canned, RATE_LIMIT, Scripted, answer, answered, dated, github, reply_ok, signed_in_as,
};
use super::*;
use time::format_description::well_known::Rfc3339;

fn limited_until(resets_at: Option<u32>) -> ErrorKind {
    ErrorKind::RateLimited { resets_at }
}

/// The time `seconds` from now, as `x-ratelimit-reset` gives it, in whole seconds.
fn seconds_from_now(seconds: i64) -> u32 {
    u32::try_from(OffsetDateTime::now_utc().unix_timestamp() + seconds).unwrap()
}

/// The seconds from now an error of the rate limit says Tsugiki can ask again.
fn seconds_left(error: &crate::error::Error) -> i64 {
    let ErrorKind::RateLimited {
        resets_at: Some(resets_at),
    } = error.kind
    else {
        panic!("{error:?} names no time");
    };
    i64::from(resets_at) - OffsetDateTime::now_utc().unix_timestamp()
}

fn error_of(transport: Canned) -> crate::error::Error {
    github(transport).viewer().unwrap_err()
}

#[test]
fn github_says_who_the_user_is() {
    let fixtures = crate::testing::recordings();
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
    let reset = seconds_from_now(1200);
    let error = error_of(Canned(Ok(Reply {
        status: 403,
        reset: Some(reset),
        date: None,
        body: r#"{"message":"API rate limit exceeded for user"}"#.to_owned(),
    })));
    assert_eq!(error.kind, limited_until(Some(reset)));
}

#[test]
fn a_secondary_rate_limit_holds_a_minute_not_until_the_hours_reset() {
    let error = error_of(Canned(Ok(Reply {
        status: 403,
        reset: Some(seconds_from_now(3000)),
        date: None,
        body: r#"{"message":"You have exceeded a secondary rate limit."}"#.to_owned(),
    })));
    assert!((59..=61).contains(&seconds_left(&error)), "{error:?}");
}

#[test]
fn a_request_after_a_refusal_says_github_refused_it_not_that_points_are_low() {
    let github = github(answered(429, ""));
    drop(github.viewer().unwrap_err());

    let held = github.viewer().unwrap_err();

    assert_eq!(held.message, RATE_LIMITED);
}

#[test]
fn a_rate_limit_that_names_no_reset_holds_a_minute() {
    let error = error_of(answered(429, ""));
    assert!((59..=61).contains(&seconds_left(&error)), "{error:?}");
}

#[test]
fn a_graphql_rate_limit_is_a_rate_limit() {
    let reset = seconds_from_now(1200);
    let body = r#"{"errors":[{"type":"RATE_LIMITED","message":"API rate limit exceeded"}]}"#;
    let error = error_of(Canned(Ok(Reply {
        status: 200,
        reset: Some(reset),
        date: None,
        body: body.to_owned(),
    })));
    assert_eq!(error.kind, limited_until(Some(reset)));
}

#[test]
fn a_rate_limit_says_its_reset_by_this_computers_clock() {
    for local_is_off_by in [300, -300] {
        let github_now = OffsetDateTime::now_utc() - time::Duration::seconds(local_is_off_by);
        let reset = u32::try_from(github_now.unix_timestamp() + 1200).unwrap();
        let body = r#"{"errors":[{"type":"RATE_LIMITED","message":"API rate limit exceeded"}]}"#;
        let refused = Reply {
            status: 200,
            reset: Some(reset),
            date: None,
            body: body.to_owned(),
        };
        let (transport, _asked) = Scripted::new(vec![
            dated(signed_in_as("someone"), github_now),
            Ok(refused),
        ]);
        let github = github(transport);
        drop(github.viewer());

        let error = github.viewer().unwrap_err();

        assert!((1199..=1201).contains(&seconds_left(&error)), "{error:?}");
    }
}

#[test]
fn a_request_while_the_rate_limit_holds_asks_nothing_and_says_when_it_can() {
    let reset = seconds_from_now(1200);
    let refused = Reply {
        status: 429,
        reset: Some(reset),
        date: None,
        body: String::new(),
    };
    let (transport, asked) = Scripted::new(vec![Ok(refused)]);
    let github = github(transport);
    drop(github.viewer());
    assert_eq!(asked.try_iter().count(), 1);

    let error = github.viewer().unwrap_err();

    assert_eq!(error.kind, limited_until(Some(reset)));
    assert_eq!(asked.try_iter().count(), 0);
    assert_eq!(
        github.points_back().map(|at| at.unix_timestamp()),
        Some(reset.into())
    );
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
fn github_gives_a_repositorys_size_in_kilobytes_or_nothing_when_its_empty() {
    let sized = |disk_usage: Value| {
        let answered = answer(
            json!({ "repository": { "diskUsage": disk_usage } }),
            RATE_LIMIT,
        );
        github(Canned(answered)).repository_size(&name("octo-org/hello"))
    };

    assert_eq!(sized(json!(2)), Ok(2048));
    assert_eq!(sized(Value::Null), Ok(0));
}

#[test]
fn a_repositorys_size_counts_in_the_hours_points() {
    let answered = answer(json!({ "repository": { "diskUsage": 2 } }), RATE_LIMIT);
    let github = github(Canned(answered));

    let size = github.repository_size(&name("octo-org/hello"));

    assert_eq!(size, Ok(2048));
    assert!(github.hour_points().is_some());
}

#[test]
fn checking_a_repository_reads_a_recorded_answer() {
    let fixtures = crate::testing::recordings();
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
    let error = error_of(answered(200, body));
    assert!(
        matches!(error.kind, ErrorKind::RateLimited { .. }),
        "{error:?}"
    );
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

/// What a test heard, in the order it happened: the client's news of the request in flight, and the
/// request reaching GitHub.
#[derive(Debug, PartialEq, Eq)]
enum Heard {
    InFlight(Option<InFlight>),
    Sent,
}

/// Answers each request with `reply`, and tells the test it went.
struct Telling {
    reply: Result<Reply, Unreachable>,
    heard: std::sync::mpsc::Sender<Heard>,
}

impl Transport for Telling {
    fn post(&self, _: &Token, _: &Request<'_>) -> Result<Reply, Unreachable> {
        tsugiki_test_git::pass_on(&self.heard, Heard::Sent);
        self.reply.clone()
    }
}

/// A client whose every answer is `reply`, and what it told of its requests, in order.
fn telling(reply: Result<Reply, Unreachable>) -> (GitHub, std::sync::mpsc::Receiver<Heard>) {
    let (heard, seen) = std::sync::mpsc::channel();
    let mut github = github(Telling {
        reply,
        heard: heard.clone(),
    });
    github.announce_in_flight(Box::new(move |now: Option<&InFlight>| {
        tsugiki_test_git::pass_on(&heard, Heard::InFlight(now.cloned()));
    }));
    (github, seen)
}

#[test]
fn a_request_tells_what_it_asks_before_it_goes_and_nothing_once_github_answers() {
    let (github, heard) = telling(signed_in_as("octocat"));
    let before = OffsetDateTime::now_utc();

    drop(github.asking(Asking::Status, || github.viewer()).unwrap());

    let heard: Vec<_> = heard.try_iter().collect();
    let [
        Heard::InFlight(Some(in_flight)),
        Heard::Sent,
        Heard::InFlight(None),
    ] = heard.as_slice()
    else {
        panic!("{heard:?}");
    };
    assert_eq!(in_flight.asked, Asking::Status);
    assert!(
        before <= in_flight.since && in_flight.since <= OffsetDateTime::now_utc(),
        "{in_flight:?}"
    );
}

#[test]
fn a_request_github_never_answers_ends_too() {
    let (github, heard) = telling(Err(Unreachable::Offline));
    let asked = Asking::Size {
        repository: crate::testing::gcmon(),
    };

    drop(
        github
            .asking(asked.clone(), || github.viewer())
            .unwrap_err(),
    );

    let heard: Vec<_> = heard.try_iter().collect();
    let [
        Heard::InFlight(Some(in_flight)),
        Heard::Sent,
        Heard::InFlight(None),
    ] = heard.as_slice()
    else {
        panic!("{heard:?}");
    };
    assert_eq!(in_flight.asked, asked);
}

#[test]
fn a_request_outside_any_asking_goes_unannounced() {
    let (github, heard) = telling(signed_in_as("octocat"));

    drop(github.viewer().unwrap());

    assert_eq!(heard.try_iter().collect::<Vec<_>>(), [Heard::Sent]);
}

#[test]
fn an_asking_inside_another_names_its_own_requests_and_gives_the_outer_one_back() {
    let (script, _) = Scripted::new(vec![signed_in_as("octocat"), signed_in_as("octocat")]);
    let mut github = github(script);
    let (announce, heard) = super::testing::in_flight_heard();
    github.announce_in_flight(announce);
    let outer = Asking::Sync {
        repository: crate::testing::gcmon(),
    };

    github.asking(outer.clone(), || {
        drop(github.asking(Asking::Status, || github.viewer()).unwrap());
        drop(github.viewer().unwrap());
    });

    let heard: Vec<_> = heard.try_iter().collect();
    assert_eq!(heard, [Some(Asking::Status), None, Some(outer), None]);
}
