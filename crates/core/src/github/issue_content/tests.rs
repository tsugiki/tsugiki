use serde_json::{Value, json};
use std::sync::mpsc::Receiver;
use time::format_description::well_known::Rfc3339;

use super::*;
use crate::github::testing::{Scripted, reply_ok, token};

fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

fn moment(text: &str) -> OffsetDateTime {
    OffsetDateTime::parse(text, &Rfc3339).unwrap()
}

/// A `GitHub` that gives one `issue`, and what it was asked.
fn answering(issue: &Value) -> (GitHub, Receiver<(String, Value)>) {
    let rate_limit = json!({ "remaining": 4000, "resetAt": "2999-01-01T00:00:00Z" });
    let data = json!({ "repository": { "issue": issue }, "rateLimit": rate_limit });
    let body = json!({ "data": data }).to_string();
    let (script, asked) = Scripted::new(vec![reply_ok(&body)]);
    (GitHub::new(Box::new(script), token()), asked)
}

#[test]
fn an_issue_reads_as_its_rendering_with_the_comments_oldest_first() {
    let (github, asked) = answering(&json!({
        "bodyHTML": "<p>It crashes</p>",
        "createdAt": "2026-11-01T09:00:00Z",
        "comments": {
            "totalCount": 32,
            "nodes": [
                {
                    "author": { "login": "some-reviewer" },
                    "createdAt": "2026-11-04T10:00:00Z",
                    "bodyHTML": "<p>Which build?</p>",
                    "isMinimized": false,
                    "minimizedReason": null,
                },
                null,
                {
                    "author": null,
                    "createdAt": "2026-11-04T11:00:00Z",
                    "bodyHTML": "<p>Debug</p>",
                    "isMinimized": false,
                    "minimizedReason": null,
                },
            ],
        },
    }));

    let content = github.issue_content(&cpython(), 7).unwrap();

    assert_eq!(
        content,
        IssueContent {
            body_html: "<p>It crashes</p>".to_owned(),
            comments: vec![
                IssueComment {
                    author: Some(Login::new("some-reviewer")),
                    created_at: moment("2026-11-04T10:00:00Z"),
                    body_html: "<p>Which build?</p>".to_owned(),
                    hidden: None,
                },
                IssueComment {
                    author: None,
                    created_at: moment("2026-11-04T11:00:00Z"),
                    body_html: "<p>Debug</p>".to_owned(),
                    hidden: None,
                },
            ],
            earlier_comments: 30,
            opened_at: Some(moment("2026-11-01T09:00:00Z")),
        }
    );
    let (operation, variables) = asked.recv().unwrap();
    assert_eq!(operation, "FetchIssueContent");
    assert_eq!(variables["number"], 7);
    assert_eq!(variables["owner"], "python");
}

#[test]
fn an_issue_github_doesnt_have_is_not_found() {
    let (github, _asked) = answering(&Value::Null);

    let problem = github.issue_content(&cpython(), 7).unwrap_err();

    assert_eq!(problem.kind, ErrorKind::NotFound);
    assert!(problem.message.contains("#7"), "{}", problem.message);
}
