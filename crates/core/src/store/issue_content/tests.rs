use time::OffsetDateTime;

use super::*;
use crate::facts::issue_content::CommentId;
use crate::login::Login;
use crate::store::testing::OpenStore;
use crate::testing::cpython;

fn comment(id: Option<u64>, body_html: &str) -> IssueComment {
    IssueComment {
        id: id.map(CommentId::new),
        comment: Comment {
            author: Some(Login::new("some-reviewer")),
            created_at: OffsetDateTime::UNIX_EPOCH,
            body_html: body_html.to_owned(),
            hidden: None,
        },
    }
}

fn content(body_html: &str, comments: Vec<IssueComment>) -> IssueContent {
    IssueContent {
        id: Some(NodeId::new(format!("I_{body_html}"))),
        body_html: body_html.to_owned(),
        comments,
        earlier_comments: 2,
        opened_at: OffsetDateTime::from_unix_timestamp(1_700_000_000).ok(),
    }
}

#[test]
fn each_read_replaces_the_last_and_other_issues_keep_theirs() {
    let db = OpenStore::new("issue-content");
    let first = content(
        "<p>One</p>",
        vec![comment(Some(1), "<p>a</p>"), comment(Some(2), "<p>b</p>")],
    );
    db.store.save_issue_content(&cpython(), 1, first).unwrap();
    // SQLite's integers stop at i64::MAX, and GitHub's numbers don't.
    let other = content(
        "<p>Other</p>",
        vec![
            comment(Some(6_065_108_504), "<p>c</p>"),
            comment(Some(u64::MAX), "<p>e</p>"),
        ],
    );
    db.store
        .save_issue_content(&cpython(), 2, other.clone())
        .unwrap();
    // A read kept from before Tsugiki asked when an Issue was opened has no time for it, one from before it
    // asked for GitHub's numbers has none for its comments, and one from before it asked for the Issue's ID
    // has none for the Issue.
    let second = IssueContent {
        id: None,
        opened_at: None,
        ..content("<p>Two</p>", vec![comment(None, "<p>d</p>")])
    };
    db.store
        .save_issue_content(&cpython(), 1, second.clone())
        .unwrap();

    let read = |number| db.reader.issue_content(&cpython(), number).unwrap();

    assert_eq!(read(1), Some(second));
    assert_eq!(read(2), Some(other));
    assert_eq!(read(3), None);
    db.close();
}

#[test]
fn an_items_id_comes_from_its_last_read_in_its_own_repository() {
    let db = OpenStore::new("issue-content-node-ids");
    let gcmon = crate::testing::gcmon();
    db.store
        .save_issue_content(&cpython(), 1, content("<p>One</p>", vec![]))
        .unwrap();
    db.store
        .save_issue_content(&gcmon, 1, content("<p>Elsewhere</p>", vec![]))
        .unwrap();
    let kept_from_before = IssueContent {
        id: None,
        ..content("<p>Two</p>", vec![])
    };
    db.store
        .save_issue_content(&cpython(), 2, kept_from_before)
        .unwrap();

    let id = |repository: &RepositoryName, number| db.store.node_id(repository, number).unwrap();
    let named = |id: &str| Some(NodeId::new(id.to_owned()));

    assert_eq!(id(&cpython(), 1), named("I_<p>One</p>"));
    assert_eq!(id(&gcmon, 1), named("I_<p>Elsewhere</p>"));
    assert_eq!(id(&cpython(), 2), None);
    assert_eq!(id(&cpython(), 3), None);
    db.close();
}
