use time::OffsetDateTime;

use super::*;
use crate::login::Login;
use crate::store::Store;
use crate::store::reader;
use crate::store::testing::temp_database;

fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

fn comment(body_html: &str) -> IssueComment {
    IssueComment {
        author: Some(Login::new("some-reviewer")),
        created_at: OffsetDateTime::UNIX_EPOCH,
        body_html: body_html.to_owned(),
        hidden: None,
    }
}

fn content(body_html: &str, comments: Vec<IssueComment>) -> IssueContent {
    IssueContent {
        body_html: body_html.to_owned(),
        comments,
        earlier_comments: 2,
    }
}

#[test]
fn each_read_replaces_the_last_and_other_issues_keep_theirs() {
    let (path, folder) = temp_database("issue-content");
    let store = Store::open(&path).unwrap();
    let (to_open, reader) = reader();
    let open = to_open.open(&path).unwrap();
    let first = content("<p>One</p>", vec![comment("<p>a</p>"), comment("<p>b</p>")]);
    store.save_issue_content(&cpython(), 1, first).unwrap();
    let other = content("<p>Other</p>", vec![comment("<p>c</p>")]);
    store
        .save_issue_content(&cpython(), 2, other.clone())
        .unwrap();
    let second = content("<p>Two</p>", vec![comment("<p>d</p>")]);
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
