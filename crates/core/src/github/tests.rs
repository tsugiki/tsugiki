use super::testing::{Canned, Scripted, answered, dated, signed_in_as, token};
use super::*;
use crate::conventions::Conventions;
use crate::filters::Filter;
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
    let viewer = client(Canned(signed_in_as("octocat")));
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
fn a_graphql_error_passes_on_githubs_words() {
    let error = error_of(answered(
        200,
        r#"{"errors":[{"message":"Something broke"}]}"#,
    ));
    assert_eq!(error.kind, ErrorKind::QueryRefused);
    assert_eq!(error.message, "GitHub said: Something broke");
}

#[test]
fn a_graphql_error_of_another_type_passes_on_githubs_words() {
    let error = error_of(answered(
        200,
        r#"{"errors":[{"type":"FORBIDDEN","message":"No such thing"}]}"#,
    ));
    assert_eq!(error.kind, ErrorKind::QueryRefused);
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
    assert_eq!(check.access, RepositoryAccess::Other);
    assert_eq!((check.open_issues, check.open_prs), (6937, 2315));
    assert_eq!(check.size, RepositorySize::Large);
    assert_eq!(check.first_sync_items, 4838);
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

fn checked_with_permission(permission: &str) -> RepositoryCheck {
    let body = format!(
        r#"{{"data":{{"repository":{{"nameWithOwner":"a/b","viewerPermission":{permission},
            "issues":{{"totalCount":1}},"pullRequests":{{"totalCount":1}}}},
            "updated":{{"issueCount":1}},"closed":{{"issueCount":1}}}}}}"#
    );
    client(answered(200, &body))
        .check_repository(&name("a/b"), the_day())
        .unwrap()
}

#[test]
fn a_repository_the_user_administers_is_maintained_and_starts_with_everything_open() {
    let check = checked_with_permission(r#""ADMIN""#);
    assert_eq!(check.access, RepositoryAccess::Maintained);
    assert_eq!(
        check.repository.filters,
        Filter::starters(RepositoryAccess::Maintained)
    );
}

#[test]
fn writing_to_a_repository_doesnt_make_it_maintained() {
    for permission in [r#""WRITE""#, r#""MAINTAIN""#, "null"] {
        let check = checked_with_permission(permission);
        assert_eq!(check.access, RepositoryAccess::Other, "{permission}");
    }
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
            date: None,
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

#[test]
fn githubs_clock_goes_by_the_last_reply_that_has_a_date() {
    let ahead = OffsetDateTime::now_utc() + Duration::minutes(5);
    let (script, _) = Scripted::new(vec![
        dated(signed_in_as("reviewer"), ahead),
        signed_in_as("reviewer"),
    ]);
    let github = client(script);
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
    let github = client(Canned(Ok(proxy)));

    drop(github.viewer().unwrap_err());

    assert!(github.clock().now() < ahead - Duration::minutes(4));
}
