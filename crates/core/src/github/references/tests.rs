use super::*;
use crate::error::ErrorKind;
use crate::github::testing::{Scripted, reply_ok, token};
use serde_json::{Value, json};

const RATE: &str = r#""rateLimit":{"remaining":4000,"resetAt":"2026-11-04T12:00:00Z"}"#;

fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

fn pr_event(number: u32, repository: &str) -> Value {
    json!({ "source": {
        "__typename": "PullRequest",
        "id": format!("id{number}"),
        "number": number,
        "updatedAt": "2026-11-04T10:00:00Z",
        "repository": { "nameWithOwner": repository },
    }})
}

fn timeline(nodes: &[Value], older: Option<&str>) -> Value {
    json!({
        "pageInfo": { "hasPreviousPage": older.is_some(), "startCursor": older },
        "nodes": nodes,
    })
}

#[test]
fn a_read_by_number_answers_each_issue_and_passes_over_a_pr() {
    let repository = json!({
        "i7": {
            "__typename": "Issue",
            "number": 7,
            "timelineItems": timeline(
                &[pr_event(1, "python/cpython"), pr_event(2, "python/peps"), pr_event(3, "python/cpython")],
                Some("older"),
            ),
        },
        "i8": { "__typename": "PullRequest" },
        "i9": null,
    });
    let reply = reply_ok(&format!(
        r#"{{"data":{{"repository":{repository},{RATE}}}}}"#
    ));
    let (script, asked) = Scripted::new(vec![reply]);
    let github = GitHub::new(Box::new(script), token());

    let found = github
        .fetch_issue_references(&cpython(), &[7, 8, 9, 10], 3)
        .unwrap();

    assert_eq!(found.asked_for, 3);
    assert_eq!(found.rate_limit.remaining, 4000);
    let listed = |number: u32| -> ListedPr {
        serde_json::from_value(json!({
            "id": format!("id{number}"),
            "number": number,
            "updatedAt": "2026-11-04T10:00:00Z",
        }))
        .unwrap()
    };
    assert_eq!(
        found.references,
        [IssueReferences {
            number: 7,
            listed: vec![listed(3), listed(1)],
            next: Some("older".to_owned()),
        }]
    );
    let (operation, variables) = asked.recv().unwrap();
    assert_eq!(operation, "FetchIssueReferences");
    assert_eq!(variables, json!({ "owner": "python", "name": "cpython" }));
}

#[test]
fn a_source_github_wont_show_costs_only_its_own_event() {
    for kind in ["FORBIDDEN", "NOT_FOUND"] {
        let body = json!({
            "data": {
                "repository": { "i7": {
                    "__typename": "Issue",
                    "number": 7,
                    "timelineItems": timeline(&[Value::Null, pr_event(3, "python/cpython")], None),
                } },
                "rateLimit": { "remaining": 4000, "resetAt": "2026-11-04T23:00:00Z" },
            },
            "errors": [{
                "type": kind,
                "message": "Resource protected by organization SAML enforcement.",
                "path": ["repository", "i7", "timelineItems", "nodes", 0, "source"],
            }],
        });
        let (script, _asked) = Scripted::new(vec![reply_ok(&body.to_string())]);
        let github = GitHub::new(Box::new(script), token());

        let found = github.fetch_issue_references(&cpython(), &[7], 1).unwrap();

        let numbers = found.references.iter().flat_map(|read| &read.listed);
        assert_eq!(numbers.map(|pr| pr.number).collect::<Vec<_>>(), [3]);
    }
}

#[test]
fn another_refusal_inside_a_read_stops_it() {
    let body = json!({
        "data": null,
        "errors": [{
            "type": "FORBIDDEN",
            "message": "No.",
            "path": ["repository", "i7", "timelineItems"],
        }],
    });
    let (script, _asked) = Scripted::new(vec![reply_ok(&body.to_string())]);
    let github = GitHub::new(Box::new(script), token());

    let refused = github.fetch_issue_references(&cpython(), &[7], 1);

    assert_eq!(refused.unwrap_err().kind, ErrorKind::QueryRefused);
}

#[test]
fn the_query_names_only_the_numbers_it_asks_for() {
    let query = fetch_references_query(&[7, 8]);
    assert!(query.contains("i7: issueOrPullRequest(number: 7)"));
    assert!(query.contains("i8: issueOrPullRequest(number: 8)"));
    assert_eq!(query.matches("issueOrPullRequest").count(), 2);
}
