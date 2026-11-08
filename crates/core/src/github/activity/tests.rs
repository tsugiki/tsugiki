use serde_json::{Value, json};
use time::format_description::well_known::Rfc3339;

use super::*;

const AT: &str = "2026-11-04T10:00:00Z";

fn actor(kind: &str, login: &str) -> Value {
    json!({ "__typename": kind, "login": login })
}

fn user(login: &str) -> Value {
    json!({ "name": login, "user": { "login": login } })
}

fn commit(author: &Value, committer: &Value) -> Value {
    json!({
        "__typename": "PullRequestCommit",
        "commit": { "committedDate": AT, "author": author, "committer": committer },
    })
}

fn comment(author: &Value, text: &str) -> Value {
    json!({ "__typename": "IssueComment", "createdAt": AT, "author": author, "bodyText": text })
}

fn activity(item: Value, conventions: Conventions) -> ItemActivity {
    let timeline = serde_json::from_value(json!({ "nodes": [item] })).unwrap();
    pull_request_activity(7, timeline, conventions)
}

fn one(kind: ActivityKind, who: &str) -> Vec<Activity> {
    vec![Activity {
        kind,
        who: Login::new(who),
        at: OffsetDateTime::parse(AT, &Rfc3339).unwrap(),
    }]
}

#[test]
fn a_commit_belongs_to_whoever_pushed_it() {
    let read = activity(
        commit(&user("original-author"), &user("some-author")),
        Conventions::Github,
    );
    assert_eq!(read.number, 7);
    assert_eq!(read.events, one(ActivityKind::Commit, "some-author"));
}

#[test]
fn a_commit_made_in_githubs_web_pages_belongs_to_its_author() {
    let read = activity(
        commit(&user("some-author"), &user("web-flow")),
        Conventions::Github,
    );
    assert_eq!(read.events, one(ActivityKind::Commit, "some-author"));
}

#[test]
fn a_commit_with_no_committer_belongs_to_its_author() {
    let read = activity(
        commit(&user("some-author"), &Value::Null),
        Conventions::Github,
    );
    assert_eq!(read.events, one(ActivityKind::Commit, "some-author"));
}

#[test]
fn a_commit_with_no_account_still_counts_as_someone() {
    let nobody = json!({ "name": "Someone", "user": null });
    let read = activity(commit(&nobody, &nobody), Conventions::Github);
    assert_eq!(read.events, one(ActivityKind::Commit, "ghost"));
    let read = activity(commit(&Value::Null, &Value::Null), Conventions::Github);
    assert_eq!(read.events, one(ActivityKind::Commit, "ghost"));
}

#[test]
fn a_committers_email_with_no_account_leaves_the_commit_to_its_author() {
    let nobody = json!({ "name": "Someone", "user": null });
    let read = activity(commit(&user("some-author"), &nobody), Conventions::Github);
    assert_eq!(read.events, one(ActivityKind::Commit, "some-author"));
}

#[test]
fn an_app_committing_for_a_person_doesnt_count() {
    let app = json!({ "name": "dependabot[bot]", "user": null });
    let read = activity(commit(&user("some-author"), &app), Conventions::Github);
    assert_eq!(read.events, []);
}

#[test]
fn an_apps_commit_through_githubs_web_pages_doesnt_count() {
    let app = json!({ "name": "dependabot[bot]", "user": null });
    let read = activity(commit(&app, &user("web-flow")), Conventions::Github);
    assert_eq!(read.events, []);
}

#[test]
fn a_bots_comment_doesnt_count() {
    let read = activity(
        comment(&actor("Bot", "bedevere-app"), "GH-2 is a backport"),
        Conventions::Cpython,
    );
    assert_eq!(read.events, []);
}

#[test]
fn a_persons_comment_counts_and_so_does_one_from_an_account_that_has_gone() {
    let read = activity(
        comment(&actor("User", "some-reviewer"), "Looks good"),
        Conventions::Cpython,
    );
    assert_eq!(read.events, one(ActivityKind::Comment, "some-reviewer"));
    let read = activity(comment(&Value::Null, "Looks good"), Conventions::Cpython);
    assert_eq!(read.events, one(ActivityKind::Comment, "ghost"));
}

#[test]
fn the_bots_conflict_comment_says_a_bot_backport_failed() {
    let text = "Sorry, @some-author and @some-reviewer, I could not cleanly backport this to \
                    3.14 due to a conflict.\nPlease backport manually with cherry_picker.";
    let read = activity(
        comment(&actor("Bot", "miss-islington-app"), text),
        Conventions::Cpython,
    );
    let kind = ActivityKind::BackportFailed {
        target: "3.14".to_owned(),
    };
    assert_eq!(read.events, one(kind, "miss-islington-app"));
}

#[test]
fn a_repository_with_no_backport_bot_reads_those_words_as_any_bots_comment() {
    let text = "I could not cleanly backport this to 3.14 due to a conflict.";
    let read = activity(
        comment(&actor("Bot", "miss-islington-app"), text),
        Conventions::Github,
    );
    assert_eq!(read.events, []);
}

#[test]
fn a_submitted_review_counts_and_a_pending_one_doesnt() {
    let review = |submitted: &Value| {
        json!({
            "__typename": "PullRequestReview",
            "submittedAt": submitted,
            "author": actor("User", "some-reviewer"),
        })
    };
    let read = activity(review(&json!(AT)), Conventions::Github);
    assert_eq!(read.events, one(ActivityKind::Review, "some-reviewer"));
    let read = activity(review(&Value::Null), Conventions::Github);
    assert_eq!(read.events, []);
}

#[test]
fn ready_for_review_and_a_rewritten_history_count() {
    let event = |kind: &str| json!({ "__typename": kind, "createdAt": AT, "actor": actor("User", "some-author") });
    let read = activity(event("ReadyForReviewEvent"), Conventions::Github);
    assert_eq!(
        read.events,
        one(ActivityKind::ReadyForReview, "some-author")
    );
    let read = activity(event("HeadRefForcePushedEvent"), Conventions::Github);
    assert_eq!(read.events, one(ActivityKind::Commit, "some-author"));
}

#[test]
fn an_issues_comments_count_unless_a_bot_wrote_them() {
    let comments = json!({ "nodes": [
            { "__typename": "IssueComment", "createdAt": AT, "author": actor("User", "issue-author") },
            { "__typename": "IssueComment", "createdAt": AT, "author": actor("Bot", "bedevere-app") },
            null,
        ] });
    let read = issue_activity(3, serde_json::from_value(comments).unwrap());
    assert_eq!(read.number, 3);
    assert_eq!(read.events, one(ActivityKind::Comment, "issue-author"));
}

#[test]
fn a_mention_of_someone_on_an_issue_names_the_person_it_mentions() {
    let timeline = json!({ "nodes": [
            { "__typename": "MentionedEvent", "createdAt": AT, "actor": actor("User", "issue-author") },
            { "__typename": "MentionedEvent", "createdAt": AT, "actor": actor("Bot", "bedevere-app") },
        ] });
    let read = issue_activity(3, serde_json::from_value(timeline).unwrap());
    assert_eq!(read.events, one(ActivityKind::Mentioned, "issue-author"));
}

#[test]
fn a_review_request_and_an_assignment_on_a_pr_name_the_person_they_ask() {
    let asked = |kind: &str, field: &str| {
        json!({
            "__typename": kind, "createdAt": AT, field: { "login": "some-reviewer" },
        })
    };
    let nodes = |item: Value| json!({ "nodes": [item] });
    let read_one = |item: Value| {
        let timeline = serde_json::from_value(nodes(item)).unwrap();
        pull_request_activity(3, timeline, Conventions::Github).events
    };

    assert_eq!(
        read_one(asked("ReviewRequestedEvent", "requestedReviewer")),
        one(ActivityKind::ReviewRequested, "some-reviewer")
    );
    assert_eq!(
        read_one(asked("AssignedEvent", "assignee")),
        one(ActivityKind::Assigned, "some-reviewer")
    );
    assert_eq!(
        read_one(json!({
            "__typename": "MentionedEvent", "createdAt": AT, "actor": actor("User", "some-reviewer"),
        })),
        one(ActivityKind::Mentioned, "some-reviewer")
    );
}

#[test]
fn a_review_request_of_a_team_and_an_assignee_gone_from_github_name_nobody() {
    let nodes = json!({ "nodes": [
            { "__typename": "ReviewRequestedEvent", "createdAt": AT, "requestedReviewer": {} },
            { "__typename": "ReviewRequestedEvent", "createdAt": AT, "requestedReviewer": null },
            { "__typename": "AssignedEvent", "createdAt": AT, "assignee": {} },
            { "__typename": "AssignedEvent", "createdAt": AT, "assignee": null },
        ] });
    let read = pull_request_activity(
        3,
        serde_json::from_value(nodes).unwrap(),
        Conventions::Github,
    );

    assert_eq!(read.events, []);
}
