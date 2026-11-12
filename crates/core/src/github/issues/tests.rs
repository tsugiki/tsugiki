use super::*;
use crate::error::ErrorKind;
use crate::github::testing::{Scripted, reply_ok, token};
use crate::github::{Reply, Unreachable};
use serde_json::Value;
use std::path::Path;
use time::format_description::well_known::Rfc3339;

const RATE: &str = r#""rateLimit":{"remaining":4000,"resetAt":"2026-11-04T12:00:00Z"}"#;

fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

fn github(script: Scripted) -> GitHub {
    GitHub::new(Box::new(script), token())
}

fn answer(repository: &Value) -> Result<Reply, Unreachable> {
    reply_ok(&format!(
        r#"{{"data":{{"repository":{repository},{RATE}}}}}"#
    ))
}

fn issue_json(number: u32) -> Value {
    json!({
        "__typename": "Issue",
        "number": number,
        "title": format!("Issue {number}"),
        "updatedAt": "2026-11-04T10:00:00Z",
        "state": "OPEN",
        "stateReason": null,
        "assignees": { "nodes": [] },
        "labels": { "nodes": [] },
        "timelineItems": { "nodes": [] },
        "author": { "login": "some-author" },
    })
}

fn event(source: &Value) -> Value {
    json!({ "source": source })
}

fn pr_source(number: u32, repository: &str) -> Value {
    json!({
        "__typename": "PullRequest",
        "id": format!("id{number}"),
        "number": number,
        "updatedAt": "2026-11-04T10:00:00Z",
        "repository": { "nameWithOwner": repository },
    })
}

fn timeline(nodes: &[Value], previous: Option<&str>) -> Result<Reply, Unreachable> {
    let issue = json!({ "timelineItems": {
            "pageInfo": { "hasPreviousPage": previous.is_some(), "startCursor": previous.or(Some("start")) },
            "nodes": nodes,
        } });
    answer(&json!({ "issue": issue }))
}

fn page(after: Option<&str>) -> PageRequest<'_> {
    PageRequest { size: 50, after }
}

fn listed_numbers(found: &CrossReferences) -> Vec<u32> {
    found.listed.iter().map(|pr| pr.number).collect()
}

#[test]
fn cross_references_list_this_repositorys_prs_newest_first() {
    let reply = timeline(
        &[
            event(&pr_source(1, "python/cpython")),
            event(&json!({ "__typename": "Issue" })),
            Value::Null,
            event(&pr_source(2, "python/peps")),
            event(&pr_source(3, "Python/CPython")),
        ],
        None,
    );
    let (script, asked) = Scripted::new(vec![reply]);

    let found = github(script)
        .fetch_cross_references(&cpython(), 7, page(Some("older")))
        .unwrap();

    assert_eq!(listed_numbers(&found), [3, 1]);
    assert_eq!(found.next, None);
    assert_eq!(found.rate_limit.remaining, 4000);
    let (operation, variables) = asked.recv().unwrap();
    assert_eq!(operation, "FetchCrossReferences");
    assert_eq!(
        variables,
        json!({
            "owner": "python",
            "name": "cpython",
            "number": 7,
            "last": 50,
            "before": "older",
        })
    );
}

#[test]
fn cross_references_say_where_the_older_ones_start() {
    let reply = timeline(&[event(&pr_source(1, "python/cpython"))], Some("older"));
    let (script, _asked) = Scripted::new(vec![reply]);

    let found = github(script)
        .fetch_cross_references(&cpython(), 7, page(None))
        .unwrap();

    assert_eq!(found.next.as_deref(), Some("older"));
}

#[test]
fn an_issue_github_no_longer_has_has_no_cross_references() {
    let body = format!(
        r#"{{"data":{{"repository":{{"issue":null}},{RATE}}},"errors":[{{"type":"NOT_FOUND",
                "message":"gone","path":["repository","issue"]}}]}}"#
    );
    let (script, _asked) = Scripted::new(vec![reply_ok(&body)]);

    let found = github(script)
        .fetch_cross_references(&cpython(), 7, page(None))
        .unwrap();

    assert_eq!(listed_numbers(&found), [0_u32; 0]);
    assert_eq!(found.next, None);
}

#[test]
fn a_page_of_cross_references_github_cant_answer_is_asked_for_at_half_size() {
    let bad_gateway = Ok(Reply {
        status: 502,
        reset: None,
        date: None,
        body: "Bad Gateway".to_owned(),
    });
    let (script, asked) = Scripted::new(vec![bad_gateway, timeline(&[], None)]);

    drop(
        github(script)
            .fetch_cross_references(&cpython(), 7, page(None))
            .unwrap(),
    );

    let lasts: Vec<Value> = asked
        .try_iter()
        .map(|(_, sent)| sent["last"].clone())
        .collect();
    assert_eq!(lasts, [json!(50), json!(25)]);
}

#[test]
fn a_fetched_issue_is_read_into_its_facts() {
    let reply = answer(&json!({ "i7": issue_json(7) }));
    let (script, asked) = Scripted::new(vec![reply]);

    let fetched = github(script).fetch_issues(&cpython(), &[7], 25).unwrap();

    assert_eq!(
        fetched.issues,
        [IssueFacts {
            number: 7,
            title: "Issue 7".to_owned(),
            author: Some(Login::new("some-author")),
            assignees: vec![],
            labels: vec![],
            updated_at: OffsetDateTime::parse("2026-11-04T10:00:00Z", &Rfc3339).unwrap(),
            state: Some(IssueState::Open),
        }]
    );
    assert_eq!(fetched.asked_for, 1);
    assert_eq!(fetched.rate_limit.remaining, 4000);
    let (operation, variables) = asked.recv().unwrap();
    assert_eq!(operation, "FetchIssues");
    assert_eq!(variables, json!({ "owner": "python", "name": "cpython" }));
}

#[test]
fn a_closed_issue_keeps_how_it_closed_as_github_draws_it() {
    let closed = |number: u32, reason: Value| {
        let mut issue = issue_json(number);
        issue["state"] = json!("CLOSED");
        issue["stateReason"] = reason;
        issue
    };
    let mut reopened = issue_json(6);
    reopened["stateReason"] = json!("REOPENED");
    let mut repository = json!({ "i6": reopened });
    for (number, reason) in [
        (7, json!("COMPLETED")),
        (8, json!("NOT_PLANNED")),
        (9, json!("DUPLICATE")),
        (10, Value::Null),
        (11, json!("A_REASON_FROM_LATER")),
    ] {
        repository[format!("i{number}")] = closed(number, reason);
    }
    let (script, _asked) = Scripted::new(vec![answer(&repository)]);

    let fetched = github(script)
        .fetch_issues(&cpython(), &[6, 7, 8, 9, 10, 11], 25)
        .unwrap();

    let mut states: Vec<(u32, Option<IssueState>)> = fetched
        .issues
        .iter()
        .map(|issue| (issue.number, issue.state))
        .collect();
    states.sort_by_key(|&(number, _)| number);
    assert_eq!(
        states,
        [
            (6, Some(IssueState::Open)),
            (7, Some(IssueState::Closed)),
            (8, Some(IssueState::NotPlanned)),
            (9, Some(IssueState::Duplicate)),
            (10, Some(IssueState::Closed)),
            (11, Some(IssueState::Closed)),
        ]
    );
}

#[test]
fn an_issue_keeps_the_names_of_its_labels_in_name_order_whatever_their_case() {
    let mut issue = issue_json(7);
    issue["labels"] = json!({ "nodes": [
        { "name": "type-bug" },
        null,
        { "name": "OS-windows" },
        { "name": "3.14" },
        { "name": "interpreter-core" },
    ] });
    let mut unlabelled = issue_json(8);
    unlabelled["labels"] = Value::Null;
    let repository = json!({ "i7": issue, "i8": unlabelled });
    let (script, _asked) = Scripted::new(vec![answer(&repository)]);

    let fetched = github(script)
        .fetch_issues(&cpython(), &[7, 8], 25)
        .unwrap();

    let names: Vec<&str> = fetched.issues[0].labels.iter().map(Label::as_str).collect();
    assert_eq!(
        names,
        ["3.14", "interpreter-core", "OS-windows", "type-bug"]
    );
    assert_eq!(fetched.issues[1].labels, []);
}

#[test]
fn an_issue_whose_author_has_gone_has_no_author() {
    let mut issue = issue_json(7);
    issue["author"] = Value::Null;
    let (script, _asked) = Scripted::new(vec![answer(&json!({ "i7": issue }))]);

    let fetched = github(script).fetch_issues(&cpython(), &[7], 25).unwrap();

    assert_eq!(fetched.issues[0].author, None);
}

#[test]
fn a_number_that_is_a_pr_or_names_nothing_leaves_no_issue() {
    let body = format!(
        r#"{{"data":{{"repository":{},{RATE}}},"errors":[{{"type":"NOT_FOUND","message":"gone",
                "path":["repository","i3"]}}]}}"#,
        json!({ "i1": issue_json(1), "i2": { "__typename": "PullRequest" }, "i3": null })
    );
    let (script, _asked) = Scripted::new(vec![reply_ok(&body)]);

    let fetched = github(script)
        .fetch_issues(&cpython(), &[1, 2, 3], 25)
        .unwrap();

    let numbers: Vec<u32> = fetched.issues.iter().map(|issue| issue.number).collect();
    assert_eq!(numbers, [1]);
    assert_eq!(fetched.asked_for, 3);
}

#[test]
fn a_repository_that_has_gone_is_still_an_error() {
    let body = r#"{"data":{"repository":null},"errors":[{"type":"NOT_FOUND","message":"gone",
                       "path":["repository"]}]}"#;
    let (script, _asked) = Scripted::new(vec![reply_ok(body)]);

    let refused = github(script)
        .fetch_issues(&cpython(), &[1], 25)
        .unwrap_err();

    assert_eq!(refused.kind, ErrorKind::NotFound);
}

#[test]
fn a_batch_asks_for_its_size_and_a_retry_for_half() {
    let replies = vec![
        Err(Unreachable::Offline),
        answer(&json!({ "i1": issue_json(1) })),
    ];
    let (script, _asked) = Scripted::new(replies);

    let fetched = github(script)
        .fetch_issues(&cpython(), &[1, 2, 3, 4], 2)
        .unwrap();

    assert_eq!(fetched.asked_for, 1);
}

#[test]
fn the_query_names_each_number_and_opens_with_its_operation() {
    let query = fetch_issues_query(&[12, 34]);
    assert!(query.starts_with("query FetchIssues("), "{query}");
    assert!(
        query.contains("i12: issueOrPullRequest(number: 12)"),
        "{query}"
    );
    assert!(
        query.contains("i34: issueOrPullRequest(number: 34)"),
        "{query}"
    );
}

#[test]
fn the_recording_holds_the_query_the_client_sends() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github/FetchIssues.json");
    let recording: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let recorded = recording["query"].as_str().unwrap();
    let numbers: Vec<u32> = recorded
        .split("number: ")
        .skip(1)
        .map(|rest| rest.split(')').next().unwrap().parse().unwrap())
        .collect();

    assert!(!numbers.is_empty());
    assert_eq!(recorded, fetch_issues_query(&numbers));
}
