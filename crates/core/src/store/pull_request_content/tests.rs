use std::fs;

use time::OffsetDateTime;

use super::super::*;
use super::*;
use crate::github::issue_content::IssueContent;
use crate::github::pull_requests::ReviewedCommit;
use crate::login::Login;
use crate::repository::RepositoryName;
use crate::store::Store;
use crate::store::testing::temp_database;

fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

#[test]
fn a_conversation_kept_before_the_migration_has_no_numbers_until_the_next_read() {
    let (path, folder) = temp_database("conversation-before-ids");
    let before = MIGRATIONS
        .iter()
        .position(|step| step.sql == super::DATABASE_IDS)
        .unwrap();
    open_with(&path, &MIGRATIONS[..before]).unwrap().close();
    // A store of that age can't run today's save, which writes the numbers, so SQL stands in for it.
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "INSERT INTO issue_content (repository, number, body_html, earlier_comments, opened_at)
             VALUES ('python/cpython', 1, '<p>Fixes it</p>', 0, 1);
             INSERT INTO issue_comments (repository, number, position, author, created_at, body_html)
             VALUES ('python/cpython', 1, 0, 'some-author', 2, '<p>Thanks</p>');
             INSERT INTO pr_reviews (repository, number, position, author, state, submitted_at, body_html)
             VALUES ('python/cpython', 1, 0, 'some-reviewer', 'COMMENTED', 3, '<p>Notes</p>');
             INSERT INTO review_threads (repository, number, review, position, node_id, path, words,
                                         replies, resolution, anchor)
             VALUES ('python/cpython', 1, 0, 0, 'T1', 'Lib/ast.py', 'Why?', 0, 'open', 'current');
             INSERT INTO review_thread_comments (repository, number, review, thread, position, author,
                                                 created_at, body_html)
             VALUES ('python/cpython', 1, 0, 0, 0, 'some-reviewer', 3, '<p>Why?</p>');",
        )
        .unwrap();

    let store = Store::open(&path).unwrap();
    let (to_open, reader) = reader();
    let open = to_open.open(&path).unwrap();
    let (_, kept) = reader.pull_request_facts(&cpython(), 1).unwrap();
    open.close();
    store.close();
    fs::remove_dir_all(folder).unwrap();

    let kept = kept.unwrap();
    assert_eq!(kept.conversation.comments.len(), 1);
    assert_eq!(kept.conversation.comments[0].id, None);
    assert_eq!(kept.reviews.len(), 1);
    assert_eq!(kept.reviews[0].id, None);
    let thread = &kept.reviews[0].threads[0];
    assert_eq!(thread.comments.len(), 1);
    assert_eq!(thread.comments[0].id, None);
}

fn sha(hex: &str) -> CommitSha {
    CommitSha::parse(hex).unwrap()
}

/// A Review by `author` of the commit `commit`, `hours` after 1970.
fn review(author: &str, state: ReviewState, commit: Option<&str>, hours: i64) -> Review {
    Review {
        id: None,
        author: Some(Login::new(author)),
        state,
        commit: commit.map(sha),
        submitted_at: OffsetDateTime::UNIX_EPOCH + time::Duration::hours(hours),
        body_html: "<p>Some notes</p>".to_owned(),
        hidden: None,
        threads: vec![],
    }
}

fn with_reviews(reviews: Vec<Review>) -> PullRequestContent {
    PullRequestContent {
        conversation: IssueContent {
            body_html: "<p>Fixes the crash</p>".to_owned(),
            comments: vec![],
            earlier_comments: 0,
            opened_at: Some(OffsetDateTime::UNIX_EPOCH),
        },
        reviews,
        earlier_reviews: 0,
        head: CommitSha::parse("ccccccc"),
        pending: Vec::new(),
    }
}

#[test]
fn the_users_review_is_their_latest_in_any_state_and_names_the_commit_it_covered() {
    let (path, folder) = temp_database("reviewed-commits");
    let store = Store::open(&path).unwrap();
    let none_yet = store.reviewed_commits(&cpython()).unwrap();
    store.remember_login(&Login::new("some-reviewer")).unwrap();
    // Approved at one commit, then a colleague's Review, then the user's own, dismissed since.
    let moved_on = with_reviews(vec![
        review("some-reviewer", ReviewState::Approved, Some("aaaaaaa"), 0),
        review("other-reviewer", ReviewState::Approved, Some("bbbbbbb"), 1),
        review("some-reviewer", ReviewState::Dismissed, Some("ccccccc"), 2),
    ]);
    store
        .save_pull_request_content(&cpython(), 1, moved_on)
        .unwrap();
    let only_colleagues = with_reviews(vec![review(
        "other-reviewer",
        ReviewState::Commented,
        Some("bbbbbbb"),
        0,
    )]);
    store
        .save_pull_request_content(&cpython(), 2, only_colleagues)
        .unwrap();
    // GitHub no longer has the commit of the user's latest Review, so an earlier one doesn't stand in.
    let lost = with_reviews(vec![
        review("some-reviewer", ReviewState::Commented, Some("aaaaaaa"), 0),
        review("some-reviewer", ReviewState::ChangesRequested, None, 1),
    ]);
    store
        .save_pull_request_content(&cpython(), 3, lost)
        .unwrap();

    let reviewed = store.reviewed_commits(&cpython()).unwrap();
    let elsewhere = store
        .reviewed_commits(&RepositoryName::parse("octo-org/hello").unwrap())
        .unwrap();

    assert_eq!(none_yet, []);
    assert_eq!(
        reviewed,
        [ReviewedCommit {
            number: 1,
            commit: sha("ccccccc"),
        }]
    );
    assert_eq!(elsewhere, []);
    store.close();
    fs::remove_dir_all(folder).unwrap();
}
