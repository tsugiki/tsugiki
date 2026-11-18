use time::OffsetDateTime;

use super::*;
use crate::facts::issue_content::IssueContent;
use crate::facts::pull_requests::{PullRequestFacts, ReviewedCommit};
use crate::login::Login;
use crate::repository::RepositoryName;
use crate::store::Store;
use crate::store::testing::{OpenStore, World, database_before, temp_database};
use crate::testing::{cpython, pr_facts, sha};

#[test]
fn a_conversation_kept_before_the_migration_has_no_numbers_until_the_next_read() {
    let (path, folder) = database_before(
        "conversation-before-ids",
        super::DATABASE_IDS,
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
    );

    let db = OpenStore::of(path, folder);
    let (_, kept) = db.reader.pull_request_facts(&cpython(), 1).unwrap();
    db.close();

    let kept = kept.unwrap();
    assert_eq!(kept.conversation.comments.len(), 1);
    assert_eq!(kept.conversation.comments[0].id, None);
    assert_eq!(kept.reviews.len(), 1);
    assert_eq!(kept.reviews[0].id, None);
    let thread = &kept.reviews[0].threads[0];
    assert_eq!(thread.comments.len(), 1);
    assert_eq!(thread.comments[0].id, None);
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
    let (path, _folder) = temp_database("reviewed-commits");
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
}

#[test]
fn a_conversation_gives_a_pr_kept_with_no_head_its_head_and_leaves_a_kept_head_alone() {
    let world = World::new("conversation-head");
    let kept = PullRequestFacts {
        head: Some(sha("aaaaaaa")),
        ..pr_facts(2)
    };
    world.has(vec![pr_facts(1), kept, pr_facts(3)]);
    let store = &world.db.store;
    let save = |number, content| {
        store
            .save_read_of_pull_request(&cpython(), number, content)
            .unwrap()
    };
    let named = |number| PrHead {
        number,
        commit: sha("ccccccc"),
        target_branch: "main".to_owned(),
    };
    let unnamed = PullRequestContent {
        head: None,
        ..with_reviews(vec![])
    };

    assert_eq!(save(1, with_reviews(vec![])), HeadKept::Filled(named(1)));
    assert_eq!(save(2, with_reviews(vec![])), HeadKept::Had(named(2)));
    assert_eq!(save(3, unnamed), HeadKept::Unnamed);
    assert_eq!(save(4, with_reviews(vec![])), HeadKept::NoFacts);

    let head = |number| {
        let (facts, _) = (world.db.reader)
            .pull_request_facts(&cpython(), number)
            .unwrap();
        (facts.pull_requests.into_iter())
            .find(|pr| pr.number == number)
            .and_then(|pr| pr.head)
    };
    assert_eq!(
        (head(1), head(2), head(3)),
        (Some(sha("ccccccc")), Some(sha("aaaaaaa")), None)
    );
    world.finish();
}
