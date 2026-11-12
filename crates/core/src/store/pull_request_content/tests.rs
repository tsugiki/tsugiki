use super::super::testing::temp_database;
use super::super::*;
use crate::repository::RepositoryName;
use std::fs;

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
