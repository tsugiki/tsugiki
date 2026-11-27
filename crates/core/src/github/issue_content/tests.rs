use serde_json::{Value, json};
use std::sync::mpsc::Receiver;

use super::*;
use crate::facts::labels::Label;
use crate::github::testing::{RATE_LIMIT, Scripted, answer, github};
use crate::login::Login;
use crate::testing::{cpython, moment};

/// A `GitHub` that gives one `issue`, and what it was asked.
fn answering(issue: &Value) -> (GitHub, Receiver<(String, Value)>) {
    let data = json!({ "repository": { "issue": issue } });
    let (script, asked) = Scripted::new(vec![answer(data, RATE_LIMIT)]);
    (github(script), asked)
}

#[test]
fn an_issue_reads_as_its_rendering_with_the_comments_oldest_first() {
    let (github, asked) = answering(&json!({
        "id": "I_7",
        "bodyHTML": "<p>It crashes</p>",
        "createdAt": "2026-11-01T09:00:00Z",
        "comments": {
            "totalCount": 32,
            "nodes": [
                {
                    "fullDatabaseId": "6065108504",
                    "author": { "login": "some-reviewer" },
                    "createdAt": "2026-11-04T10:00:00Z",
                    "bodyHTML": "<p>Which build?</p>",
                    "isMinimized": false,
                    "minimizedReason": null,
                },
                null,
                {
                    "fullDatabaseId": null,
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
            id: Some(NodeId::new("I_7".to_owned())),
            body_html: "<p>It crashes</p>".to_owned(),
            comments: vec![
                IssueComment {
                    id: Some(CommentId::new(6_065_108_504)),
                    comment: Comment {
                        author: Some(Login::new("some-reviewer")),
                        created_at: moment("2026-11-04T10:00:00Z"),
                        body_html: "<p>Which build?</p>".to_owned(),
                        hidden: None,
                    },
                },
                IssueComment {
                    id: None,
                    comment: Comment {
                        author: None,
                        created_at: moment("2026-11-04T11:00:00Z"),
                        body_html: "<p>Debug</p>".to_owned(),
                        hidden: None,
                    },
                },
            ],
            earlier_comments: 30,
            events: vec![],
            earlier_events: 0,
            opened_at: Some(moment("2026-11-01T09:00:00Z")),
        }
    );
    let (operation, variables) = asked.recv().unwrap();
    assert_eq!(operation, "FetchIssueContent");
    assert_eq!(variables["number"], 7);
    assert_eq!(variables["owner"], "python");
}

#[test]
fn a_comment_number_that_isnt_digits_fails_the_read() {
    let (github, _asked) = answering(&json!({
        "id": "I_7",
        "bodyHTML": "",
        "createdAt": "2026-11-01T09:00:00Z",
        "comments": {
            "totalCount": 1,
            "nodes": [{
                "fullDatabaseId": "60x5",
                "author": null,
                "createdAt": "2026-11-04T10:00:00Z",
                "bodyHTML": "",
                "isMinimized": false,
                "minimizedReason": null,
            }],
        },
    }));

    let problem = github.issue_content(&cpython(), 7).unwrap_err();

    // The parser's words, which name the number, go to the log alone.
    assert_eq!(problem.kind, ErrorKind::Internal);
    assert!(
        problem.message.contains("can't read"),
        "{}",
        problem.message
    );
}

#[test]
fn an_issue_github_doesnt_have_is_not_found() {
    let (github, _asked) = answering(&Value::Null);

    let problem = github.issue_content(&cpython(), 7).unwrap_err();

    assert_eq!(problem.kind, ErrorKind::NotFound);
    assert!(problem.message.contains("#7"), "{}", problem.message);
}

#[test]
fn a_reference_whose_commit_is_gone_shows_nothing_and_an_unknown_close_reason_is_completed() {
    let (github, _asked) = answering(&json!({
        "id": "I_7",
        "bodyHTML": "",
        "createdAt": "2026-11-01T09:00:00Z",
        "comments": { "totalCount": 0, "nodes": [] },
        "events": {
            "totalCount": 3,
            "nodes": [
                {
                    "__typename": "ReferencedEvent",
                    "createdAt": "2026-11-02T09:00:00Z",
                    "actor": { "login": "some-author" },
                    "commit": null,
                },
                {
                    "__typename": "ClosedEvent",
                    "createdAt": "2026-11-03T09:00:00Z",
                    "actor": { "login": "some-author" },
                    "stateReason": "A_REASON_GITHUB_ADDS_LATER",
                },
            ],
        },
    }));

    let content = github.issue_content(&cpython(), 7).unwrap();

    assert_eq!(content.events.len(), 1);
    assert_eq!(
        content.events[0].kind,
        IssueEventKind::Closed { completed: true }
    );
    assert_eq!(content.earlier_events, 2);
}

#[test]
fn an_issue_reads_the_events_the_view_shows_with_what_they_name() {
    let (github, _asked) = answering(&json!({
        "id": "I_7",
        "bodyHTML": "",
        "createdAt": "2026-11-01T09:00:00Z",
        "comments": { "totalCount": 0, "nodes": [] },
        "events": {
            "totalCount": 13,
            "nodes": [
                {
                    "__typename": "CrossReferencedEvent",
                    "createdAt": "2026-11-02T09:00:00Z",
                    "actor": { "login": "some-author" },
                    "source": {
                        "__typename": "PullRequest",
                        "number": 12,
                        "title": "Fix it",
                        "url": "https://github.com/python/cpython/pull/12",
                        "repository": { "nameWithOwner": "python/cpython" },
                    },
                },
                {
                    "__typename": "ReferencedEvent",
                    "createdAt": "2026-11-03T09:00:00Z",
                    "actor": { "login": "some-author" },
                    "commit": {
                        "abbreviatedOid": "0123abc",
                        "messageHeadline": "Fix the crash",
                        "url": "https://github.com/python/cpython/commit/0123abcd",
                    },
                },
                {
                    "__typename": "ConnectedEvent",
                    "createdAt": "2026-11-04T09:00:00Z",
                    "actor": { "login": "some-author" },
                    "source": {
                        "__typename": "Issue",
                        "number": 7,
                        "title": "It crashes",
                        "url": "https://github.com/python/cpython/issues/7",
                        "repository": { "nameWithOwner": "python/cpython" },
                    },
                    "subject": {
                        "__typename": "PullRequest",
                        "number": 12,
                        "title": "Fix it",
                        "url": "https://github.com/python/cpython/pull/12",
                        "repository": { "nameWithOwner": "python/cpython" },
                    },
                },
                {
                    "__typename": "DisconnectedEvent",
                    "createdAt": "2026-11-05T09:00:00Z",
                    "actor": null,
                    "source": {
                        "__typename": "PullRequest",
                        "number": 12,
                        "title": "Fix it",
                        "url": "https://github.com/python/cpython/pull/12",
                        "repository": { "nameWithOwner": "python/cpython" },
                    },
                    "subject": {
                        "__typename": "Issue",
                        "number": 7,
                        "title": "It crashes",
                        "url": "https://github.com/python/cpython/issues/7",
                        "repository": { "nameWithOwner": "python/cpython" },
                    },
                },
                {
                    "__typename": "LabeledEvent",
                    "createdAt": "2026-11-06T09:00:00Z",
                    "actor": { "login": "some-author" },
                    "label": { "name": "type-bug" },
                },
                {
                    "__typename": "UnlabeledEvent",
                    "createdAt": "2026-11-07T09:00:00Z",
                    "actor": { "login": "some-author" },
                    "label": { "name": "type-bug" },
                },
                {
                    "__typename": "AssignedEvent",
                    "createdAt": "2026-11-08T09:00:00Z",
                    "actor": { "login": "some-author" },
                    "assignee": { "login": "some-assignee" },
                },
                {
                    "__typename": "UnassignedEvent",
                    "createdAt": "2026-11-09T09:00:00Z",
                    "actor": { "login": "some-author" },
                    "assignee": null,
                },
                {
                    "__typename": "ClosedEvent",
                    "createdAt": "2026-11-10T09:00:00Z",
                    "actor": { "login": "some-author" },
                    "stateReason": "COMPLETED",
                },
                {
                    "__typename": "ClosedEvent",
                    "createdAt": "2026-11-11T09:00:00Z",
                    "actor": { "login": "some-author" },
                    "stateReason": "NOT_PLANNED",
                },
                {
                    "__typename": "ReopenedEvent",
                    "createdAt": "2026-11-12T09:00:00Z",
                    "actor": { "login": "some-author" },
                },
                {
                    "__typename": "RenamedTitleEvent",
                    "createdAt": "2026-11-13T09:00:00Z",
                    "actor": { "login": "some-author" },
                    "previousTitle": "Old title",
                },
            ],
        },
    }));

    let content = github.issue_content(&cpython(), 7).unwrap();
    let reference = |number: u32, title: &str, url: &str| Reference {
        repository: cpython(),
        number,
        title: title.to_owned(),
        url: url.to_owned(),
    };
    let pull = reference(12, "Fix it", "https://github.com/python/cpython/pull/12");
    let kinds: Vec<IssueEventKind> = content
        .events
        .iter()
        .map(|event| event.kind.clone())
        .collect();
    assert_eq!(
        kinds,
        vec![
            IssueEventKind::CrossReferenced {
                reference: pull.clone()
            },
            IssueEventKind::Referenced {
                commit: CommitReference {
                    sha: "0123abc".to_owned(),
                    message: "Fix the crash".to_owned(),
                    url: "https://github.com/python/cpython/commit/0123abcd".to_owned(),
                },
            },
            IssueEventKind::Connected {
                reference: pull.clone()
            },
            IssueEventKind::Disconnected { reference: pull },
            IssueEventKind::Labeled {
                label: Label::new("type-bug")
            },
            IssueEventKind::Unlabeled {
                label: Label::new("type-bug")
            },
            IssueEventKind::Assigned {
                assignee: Some(Login::new("some-assignee"))
            },
            IssueEventKind::Unassigned { assignee: None },
            IssueEventKind::Closed { completed: true },
            IssueEventKind::Closed { completed: false },
            IssueEventKind::Reopened,
            IssueEventKind::Renamed {
                previous_title: "Old title".to_owned()
            },
        ]
    );
    assert_eq!(content.events[0].actor, Some(Login::new("some-author")));
    assert_eq!(content.events[3].actor, None);
    // One event's count is past the page, so the view can say that GitHub holds older ones.
    assert_eq!(content.earlier_events, 1);
}

#[test]
fn an_issue_query_asks_only_for_the_events_the_view_shows() {
    let query = Thread::Issue.query();
    for shown in [
        "CROSS_REFERENCED_EVENT",
        "REFERENCED_EVENT",
        "CONNECTED_EVENT",
        "DISCONNECTED_EVENT",
        "LABELED_EVENT",
        "UNLABELED_EVENT",
        "ASSIGNED_EVENT",
        "UNASSIGNED_EVENT",
        "CLOSED_EVENT",
        "REOPENED_EVENT",
        "RENAMED_TITLE_EVENT",
    ] {
        assert!(query.contains(shown), "{shown}");
    }
    for left_out in [
        "MENTIONED_EVENT",
        "SUBSCRIBED_EVENT",
        "ADDED_TO_PROJECT_V2_EVENT",
        "PROJECT_V2_ITEM_STATUS_CHANGED_EVENT",
        "MILESTONED_EVENT",
        "DEMILESTONED_EVENT",
        "LOCKED_EVENT",
        "PINNED_EVENT",
        "TRANSFERRED_EVENT",
        "MARKED_AS_DUPLICATE_EVENT",
        "COMMENT_DELETED_EVENT",
    ] {
        assert!(!query.contains(left_out), "{left_out}");
    }
    assert!(query.contains("timelineItems(last: 30"));
    // A PR's Conversation keeps its comments and Reviews, and its events wait for a later ticket.
    assert!(
        !Thread::PullRequest
            .query()
            .contains("timelineItems(last: 30")
    );
}
