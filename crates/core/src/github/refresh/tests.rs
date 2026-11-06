use super::*;
use crate::error::ErrorKind;
use crate::github::Unreachable;
use crate::github::pull_requests::{PrState, ReviewState};
use crate::github::replay::Replayer;
use crate::github::testing::{Scripted, reply_ok, token};
use crate::github::{ConditionalReply, Reply, Transport};
use crate::token::Token;
use std::path::Path;
use std::sync::mpsc::{self, Sender};

const RATE: &str = r#""rateLimit":{"remaining":4000,"resetAt":"2026-11-04T12:00:00Z"}"#;
const START: ListPlace = ListPlace::Next(None);
const NEWER: &str = "2026-11-04T10:30:00Z";
const OLDER: &str = "2026-11-04T09:00:00Z";

fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

fn github(script: Scripted) -> GitHub {
    GitHub::new(Box::new(script), token())
}

fn since() -> OffsetDateTime {
    OffsetDateTime::parse("2026-11-04T10:00:00Z", &Rfc3339).unwrap()
}

fn filter(text: &str) -> FilterQuery {
    text.to_owned().try_into().unwrap()
}

fn pr(number: u32, updated_at: &str) -> Value {
    json!({ "id": format!("id{number}"), "number": number, "updatedAt": updated_at })
}

fn issue(number: u32) -> Value {
    json!({
        "number": number,
        "title": format!("Issue {number}"),
        "updatedAt": NEWER,
        "assignees": { "nodes": [] },
        "timelineItems": { "nodes": [] },
        "author": { "login": "some-author" },
    })
}

fn list(nodes: &[Value], next: Option<&str>) -> Value {
    json!({
        "pageInfo": { "hasNextPage": next.is_some(), "endCursor": next },
        "nodes": nodes,
    })
}

/// An answer with these lists, and these searches as fields next to them.
fn polled(issues: &Value, prs: &Value, searches: &str) -> Result<Reply, Unreachable> {
    let repository = json!({
        "openIssues": { "totalCount": 7 },
        "openPrs": { "totalCount": 5 },
        "issues": issues,
        "pullRequests": prs,
    });
    reply_ok(&format!(
        r#"{{"data":{{"repository":{repository},{searches}{RATE}}}}}"#
    ))
}

fn poll_of<'a>(
    issues: &'a ListPlace,
    prs: &'a ListPlace,
    filters: &'a [FilterQuery],
) -> PollRequest<'a> {
    PollRequest {
        since: since(),
        issues,
        pull_requests: prs,
        filters,
        size: 50,
    }
}

#[test]
fn a_poll_asks_for_each_list_and_narrows_each_filter_to_the_same_time() {
    let filters = [filter("is:open OR label:bug"), filter("author:@me")];
    let searches = format!(
        r#""f0":{0},"f1":{0},"#,
        json!({ "issueCount": 0, "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [] })
    );
    let (script, asked) =
        Scripted::new(vec![polled(&list(&[], None), &list(&[], None), &searches)]);

    let answer = github(script)
        .poll_repository(&cpython(), &poll_of(&START, &START, &filters))
        .unwrap();

    let (operation, sent) = asked.recv().unwrap();
    assert_eq!(operation, "RefreshRepository");
    assert_eq!(sent["owner"], "python");
    assert_eq!(sent["name"], "cpython");
    assert_eq!(sent["since"], "2026-11-04T10:00:00Z");
    assert_eq!(sent["issues"], 50);
    assert_eq!(sent["prs"], 50);
    assert_eq!(
        sent["filter0"],
        "repo:python/cpython (is:open OR label:bug) updated:>=2026-11-04T10:00:00Z"
    );
    assert_eq!(
        sent["filter1"],
        "repo:python/cpython (author:@me) updated:>=2026-11-04T10:00:00Z"
    );
    assert_eq!(answer.open_items, 12);
    assert_eq!(answer.filters.len(), 2);
    assert_eq!(answer.filters[1].0, filter("author:@me"));
}

#[test]
fn a_poll_with_no_filters_asks_for_no_searches() {
    assert!(!poll_query(0).contains("search("));
    assert!(!poll_query(0).contains("$first"));
    assert!(poll_query(2).contains("$first: Int!"));
    assert!(poll_query(2).contains("f1: search(query: $filter1"));
}

#[test]
fn a_poll_reads_what_changed() {
    let filters = [filter("a")];
    let hit = json!({
        "__typename": "Issue",
        "number": 3,
        "updatedAt": NEWER,
        "repository": { "nameWithOwner": "python/cpython" },
    });
    let searches = format!(
        r#""f0":{},"#,
        json!({
            "issueCount": 1,
            "pageInfo": { "hasNextPage": true, "endCursor": "more" },
            "nodes": [hit],
        })
    );
    let (script, _asked) = Scripted::new(vec![polled(
        &list(&[issue(1), Value::Null], None),
        &list(&[pr(2, NEWER), Value::Null], None),
        &searches,
    )]);

    let answer = github(script)
        .poll_repository(&cpython(), &poll_of(&START, &START, &filters))
        .unwrap();

    assert_eq!(answer.issues[0].number, 1);
    assert_eq!(answer.issues[0].title, "Issue 1");
    assert_eq!(answer.pull_requests[0].number, 2);
    assert_eq!(answer.next_issues, ListPlace::Finished);
    assert_eq!(answer.next_pull_requests, ListPlace::Finished);
    let (_, page) = &answer.filters[0];
    assert_eq!(page.hits[0].number, 3);
    assert_eq!(page.next.as_deref(), Some("more"));
}

#[test]
fn a_list_with_more_pages_says_where_to_carry_on() {
    let (script, _asked) = Scripted::new(vec![polled(
        &list(&[issue(1)], Some("issues")),
        &list(&[pr(2, NEWER)], Some("prs")),
        "",
    )]);

    let answer = github(script)
        .poll_repository(&cpython(), &poll_of(&START, &START, &[]))
        .unwrap();

    assert_eq!(
        answer.next_issues,
        ListPlace::Next(Some("issues".to_owned()))
    );
    assert_eq!(
        answer.next_pull_requests,
        ListPlace::Next(Some("prs".to_owned()))
    );
}

#[test]
fn the_first_pr_older_than_the_poll_ends_the_list() {
    let (script, _asked) = Scripted::new(vec![polled(
        &list(&[], None),
        &list(&[pr(2, NEWER), pr(3, OLDER)], Some("more")),
        "",
    )]);

    let answer = github(script)
        .poll_repository(&cpython(), &poll_of(&START, &START, &[]))
        .unwrap();

    let numbers: Vec<u32> = answer.pull_requests.iter().map(|pr| pr.number).collect();
    assert_eq!(numbers, [2]);
    assert_eq!(answer.next_pull_requests, ListPlace::Finished);
}

#[test]
fn a_list_already_read_asks_for_nothing_and_stays_finished() {
    let next = ListPlace::Next(Some("cursor".to_owned()));
    let (script, asked) = Scripted::new(vec![polled(
        &list(&[], Some("ignored")),
        &list(&[], None),
        "",
    )]);

    let answer = github(script)
        .poll_repository(&cpython(), &poll_of(&ListPlace::Finished, &next, &[]))
        .unwrap();

    let (_, sent) = asked.recv().unwrap();
    assert_eq!(sent["issues"], 0);
    assert_eq!(sent["issuesAfter"], Value::Null);
    assert_eq!(sent["prs"], 50);
    assert_eq!(sent["prsAfter"], "cursor");
    assert_eq!(answer.next_issues, ListPlace::Finished);
    assert!(answer.next_issues.is_finished());
}

#[test]
fn a_poll_github_refuses_says_so() {
    let refused = reply_ok(r#"{"data":null,"errors":[{"message":"Bad query"}]}"#);
    let (script, _asked) = Scripted::new(vec![refused]);

    let problem = github(script)
        .poll_repository(&cpython(), &poll_of(&START, &START, &[]))
        .unwrap_err();

    assert_eq!(problem.kind, ErrorKind::QueryRefused);
}

/// Answers each list check the same way, and says what it was asked.
struct Lists {
    answer: Result<ConditionalReply, Unreachable>,
    asked: Sender<(String, Option<String>)>,
}

impl Transport for Lists {
    fn post(&self, _: &Token, _: &Request<'_>) -> Result<Reply, Unreachable> {
        Err(Unreachable::Offline)
    }

    fn get(
        &self,
        _: &Token,
        request: &ConditionalGet<'_>,
    ) -> Result<ConditionalReply, Unreachable> {
        let seen = (request.path.to_owned(), request.etag.map(str::to_owned));
        drop(self.asked.send(seen));
        self.answer.clone()
    }
}

fn listing(answer: Result<ConditionalReply, Unreachable>, tag: Option<&Etag>) -> ListCheck {
    let lists = Lists {
        answer,
        asked: mpsc::channel().0,
    };
    GitHub::new(Box::new(lists), token()).check_list(&cpython(), tag)
}

fn replied(status: u16, etag: Option<&str>) -> Result<ConditionalReply, Unreachable> {
    Ok(ConditionalReply {
        date: None,
        status,
        etag: etag.map(str::to_owned),
    })
}

#[test]
fn a_list_check_sends_the_tag_it_was_given() {
    let (sent, asked) = mpsc::channel();
    let lists = Lists {
        answer: replied(304, None),
        asked: sent,
    };
    let tag = Etag::new("W/\"abc\"");

    let check = GitHub::new(Box::new(lists), token()).check_list(&cpython(), Some(&tag));

    assert_eq!(check, ListCheck::Same);
    assert_eq!(
        asked.recv().unwrap(),
        (
            "/repos/python/cpython/issues?state=all&sort=updated&per_page=1".to_owned(),
            Some("W/\"abc\"".to_owned())
        )
    );
}

#[test]
fn a_list_check_says_nothing_about_graphql() {
    let lists = Lists {
        answer: replied(304, None),
        asked: mpsc::channel().0,
    };
    let github = GitHub::new(Box::new(lists), token());

    assert_eq!(github.check_list(&cpython(), None), ListCheck::Same);
    assert_eq!(github.viewer().unwrap_err().kind, ErrorKind::Offline);
}

#[test]
fn a_list_that_moved_gives_its_new_tag() {
    let check = listing(replied(200, Some("\"new\"")), None);
    assert_eq!(check, ListCheck::Moved(Some(Etag::new("\"new\""))));
}

#[test]
fn a_list_check_that_fails_counts_as_moved() {
    assert_eq!(listing(replied(200, None), None), ListCheck::Moved(None));
    assert_eq!(listing(replied(500, None), None), ListCheck::Moved(None));
    assert_eq!(
        listing(Err(Unreachable::Offline), None),
        ListCheck::Moved(None)
    );
}

#[test]
fn a_transport_with_no_rest_counts_as_unreachable() {
    let (script, _asked) = Scripted::new(Vec::new());
    assert_eq!(
        github(script).check_list(&cpython(), None),
        ListCheck::Moved(None)
    );
}

fn probe_answer(fields: &Value) -> Result<Reply, Unreachable> {
    reply_ok(&format!(r#"{{"data":{{"repository":{fields},{RATE}}}}}"#))
}

fn probed(number: u32) -> Value {
    json!({
        "number": number,
        "reviewRequests": { "nodes": [{ "requestedReviewer": { "login": "some-reviewer" } }] },
        "timelineItems": { "nodes": [] },
        "latestReviews": { "nodes": [{ "author": { "login": "some-author" }, "state": "APPROVED" }] },
        "lastCommit": { "nodes": [{ "commit": { "statusCheckRollup": { "state": "SUCCESS" } } }] },
    })
}

#[test]
fn a_probe_reads_ci_and_reviews_and_skips_a_pr_that_has_gone() {
    let fields = json!({ "p1": probed(1), "p2": null });
    let (script, asked) = Scripted::new(vec![probe_answer(&fields)]);

    let batch = github(script)
        .probe_pull_requests(&cpython(), &[1, 2, 3], 2)
        .unwrap();

    assert_eq!(asked.recv().unwrap().0, "ProbePullRequests");
    assert_eq!(batch.asked_for, 2);
    assert_eq!(
        batch.probes,
        [PrProbe {
            number: 1,
            ci: Some(CiState::Success),
            review_requests: vec![Login::new("some-reviewer")],
            latest_reviews: vec![LatestReview {
                author: Login::new("some-author"),
                state: ReviewState::Approved,
            }],
        }]
    );
}

#[test]
fn a_probe_asks_for_half_after_a_bad_gateway() {
    let bad_gateway = Ok(Reply {
        status: 502,
        reset: None,
        date: None,
        body: String::new(),
    });
    let (script, _asked) =
        Scripted::new(vec![bad_gateway, probe_answer(&json!({ "p1": probed(1) }))]);

    let batch = github(script)
        .probe_pull_requests(&cpython(), &[1, 2], 2)
        .unwrap();

    assert_eq!(batch.asked_for, 1);
}

#[test]
fn a_probe_of_each_number_asks_for_it_by_name() {
    let query = probe_query(&[5, 6]);
    assert!(query.contains("p5: pullRequest(number: 5)"));
    assert!(query.contains("p6: pullRequest(number: 6)"));
}

#[test]
fn a_probe_changes_only_what_it_read() {
    let stored = PullRequestFacts {
        number: 1,
        title: "A title".to_owned(),
        state: PrState::Open,
        is_draft: true,
        author: None,
        target_branch: "main".to_owned(),
        ci: Some(CiState::Pending),
        updated_at: since(),
        review_requests: Vec::new(),
        latest_reviews: Vec::new(),
        assignees: vec![Login::new("some-assignee")],
        closing_issues: vec![9],
        merge_commit: None,
        off_default: None,
    };
    let probe = PrProbe {
        number: 1,
        ci: Some(CiState::Failure),
        review_requests: vec![Login::new("some-reviewer")],
        latest_reviews: Vec::new(),
    };

    let after = probe.applied_to(&stored);

    assert_eq!(
        after,
        PullRequestFacts {
            ci: Some(CiState::Failure),
            review_requests: vec![Login::new("some-reviewer")],
            ..stored
        }
    );
}

fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github")
}

/// The text of the recorded query in `<operation>.json`, which says what a test has to ask to replay it.
fn recorded_query(operation: &str) -> String {
    let text = std::fs::read_to_string(fixtures().join(format!("{operation}.json"))).unwrap();
    let recording: Value = serde_json::from_str(&text).unwrap();
    recording["query"].as_str().unwrap().to_owned()
}

#[test]
fn the_recorded_poll_reads_as_a_poll() {
    let filters = [filter("a"), filter("b")];
    let github = GitHub::new(Box::new(Replayer::new(fixtures())), token());

    let answer = github
        .poll_repository(&cpython(), &poll_of(&START, &START, &filters))
        .unwrap();

    assert!(answer.open_items > 0);
    assert_eq!(answer.filters.len(), 2);
}

#[test]
fn the_recorded_probe_reads_as_a_probe() {
    let query = recorded_query("ProbePullRequests");
    let numbers: Vec<u32> = query
        .split(" p")
        .skip(1)
        .filter_map(|field| field.split_once(':')?.0.parse().ok())
        .collect();
    let github = GitHub::new(Box::new(Replayer::new(fixtures())), token());

    let batch = github
        .probe_pull_requests(&cpython(), &numbers, numbers.len())
        .unwrap();

    assert!(!batch.probes.is_empty());
}

#[test]
fn a_list_checks_date_sets_githubs_clock_and_a_proxys_doesnt() {
    let ahead = OffsetDateTime::now_utc() + time::Duration::minutes(5);
    let clock_after = |status: u16| {
        let answer = Ok(ConditionalReply {
            status,
            etag: None,
            date: Some(ahead),
        });
        let lists = Lists {
            answer,
            asked: mpsc::channel().0,
        };
        let github = GitHub::new(Box::new(lists), token());
        drop(github.check_list(&cpython(), None));
        github.clock().now()
    };

    assert!(clock_after(304) >= ahead);
    assert!(clock_after(407) < ahead - time::Duration::minutes(4));
}
