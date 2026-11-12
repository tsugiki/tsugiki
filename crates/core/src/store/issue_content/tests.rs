use time::OffsetDateTime;

use super::*;
use crate::github::issue_content::CommentId;
use crate::login::Login;
use crate::store::Store;
use crate::store::reader;
use crate::store::testing::temp_database;

fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

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
        body_html: body_html.to_owned(),
        comments,
        earlier_comments: 2,
        opened_at: OffsetDateTime::from_unix_timestamp(1_700_000_000).ok(),
    }
}

#[test]
fn each_read_replaces_the_last_and_other_issues_keep_theirs() {
    let (path, folder) = temp_database("issue-content");
    let store = Store::open(&path).unwrap();
    let (to_open, reader) = reader();
    let open = to_open.open(&path).unwrap();
    let first = content(
        "<p>One</p>",
        vec![comment(Some(1), "<p>a</p>"), comment(Some(2), "<p>b</p>")],
    );
    store.save_issue_content(&cpython(), 1, first).unwrap();
    // SQLite's integers stop at i64::MAX, and GitHub's numbers don't.
    let other = content(
        "<p>Other</p>",
        vec![
            comment(Some(6_065_108_504), "<p>c</p>"),
            comment(Some(u64::MAX), "<p>e</p>"),
        ],
    );
    store
        .save_issue_content(&cpython(), 2, other.clone())
        .unwrap();
    // A read kept from before Tsugiki asked when an Issue was opened has no time for it, and one from before
    // it asked for GitHub's numbers has none for its comments.
    let second = IssueContent {
        opened_at: None,
        ..content("<p>Two</p>", vec![comment(None, "<p>d</p>")])
    };
    store
        .save_issue_content(&cpython(), 1, second.clone())
        .unwrap();

    let read = |number| reader.issue_facts(&cpython(), number).unwrap().1;

    assert_eq!(read(1), Some(second));
    assert_eq!(read(2), Some(other));
    assert_eq!(read(3), None);
    open.close();
    store.close();
    std::fs::remove_dir_all(folder).unwrap();
}
