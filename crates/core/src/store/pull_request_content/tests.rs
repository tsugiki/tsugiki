use time::OffsetDateTime;

use super::*;
use crate::facts::issue_content::IssueContent;
use crate::facts::pull_requests::{PullRequestFacts, ReviewedCommit};
use crate::login::Login;
use crate::repository::RepositoryName;
use crate::store::Store;
use crate::store::testing::{OpenStore, World, at_second, database_before, temp_database};
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
        .save_pull_request_content(&cpython(), 1, (moved_on, at_second(0)))
        .unwrap();
    let only_colleagues = with_reviews(vec![review(
        "other-reviewer",
        ReviewState::Commented,
        Some("bbbbbbb"),
        0,
    )]);
    store
        .save_pull_request_content(&cpython(), 2, (only_colleagues, at_second(0)))
        .unwrap();
    // GitHub no longer has the commit of the user's latest Review, so an earlier one doesn't stand in.
    let lost = with_reviews(vec![
        review("some-reviewer", ReviewState::Commented, Some("aaaaaaa"), 0),
        review("some-reviewer", ReviewState::ChangesRequested, None, 1),
    ]);
    store
        .save_pull_request_content(&cpython(), 3, (lost, at_second(0)))
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
fn a_conversation_says_when_the_commit_of_the_users_latest_review_moved() {
    let world = World::new("reviewed-moved");
    let reviewed = |commit| {
        with_reviews(vec![review(
            "octocat",
            ReviewState::Approved,
            Some(commit),
            0,
        )])
    };
    let moved = |content| {
        (world.db.store)
            .save_read_of_pull_request(&cpython(), 1, (content, at_second(0)))
            .unwrap()
            .reviewed_moved
    };

    assert!(moved(reviewed("aaaaaaa")), "the user's first Review");
    assert!(!moved(reviewed("aaaaaaa")), "the same Review read again");
    assert!(moved(reviewed("bbbbbbb")), "a Review of a newer commit");
    world.finish();
}

/// PR `number` at `commit`, as a `HeadKept` names it.
fn head_of(number: u32, commit: &str) -> PrHead {
    PrHead {
        number,
        commit: sha(commit),
        target_branch: "main".to_owned(),
    }
}

/// The head PR `number`'s facts keep, and the head its kept Conversation was read at.
fn heads(world: &World, number: u32) -> (Option<CommitSha>, Option<CommitSha>) {
    let (facts, conversation) = (world.db.reader)
        .pull_request_facts(&cpython(), number)
        .unwrap();
    let facts = facts
        .pull_requests
        .into_iter()
        .find(|pr| pr.number == number);
    (
        facts.and_then(|pr| pr.head),
        conversation.and_then(|read| read.head),
    )
}

#[test]
fn a_conversation_keeps_its_head_in_a_pr_with_none_or_an_older_one() {
    let world = World::new("conversation-head");
    let at = |seconds, head: Option<&str>| PullRequestFacts {
        updated_at: at_second(seconds),
        head: head.map(sha),
        ..pr_facts(2)
    };
    world.has(vec![
        PullRequestFacts {
            number: 1,
            ..at(10, None)
        },
        at(10, Some("aaaaaaa")),
        PullRequestFacts {
            number: 3,
            ..at(10, Some("aaaaaaa"))
        },
        PullRequestFacts {
            number: 5,
            ..at(10, None)
        },
    ]);
    let store = &world.db.store;
    let save = |number, content, seconds| {
        store
            .save_read_of_pull_request(&cpython(), number, (content, at_second(seconds)))
            .unwrap()
            .head
    };
    let unnamed = PullRequestContent {
        head: None,
        ..with_reviews(vec![])
    };

    assert_eq!(
        save(1, with_reviews(vec![]), 5),
        HeadKept::Moved(head_of(1, "ccccccc")),
        "a PR kept with no head takes the Conversation's, however old"
    );
    assert_eq!(
        save(2, with_reviews(vec![]), 10),
        HeadKept::Had(head_of(2, "aaaaaaa")),
        "a read as old as the list's leaves its head"
    );
    assert_eq!(
        save(3, with_reviews(vec![]), 11),
        HeadKept::Moved(head_of(3, "ccccccc"))
    );
    assert_eq!(save(5, unnamed, 11), HeadKept::Unnamed);
    assert_eq!(save(4, with_reviews(vec![]), 11), HeadKept::NoFacts);

    let kept = |number| heads(&world, number).0;
    assert_eq!(
        [kept(1), kept(2), kept(3), kept(5)],
        [
            Some(sha("ccccccc")),
            Some(sha("aaaaaaa")),
            Some(sha("ccccccc")),
            None
        ]
    );
    world.finish();
}

/// The read of a PR that saves first: its list read or its Conversation.
#[derive(Debug, Clone, Copy)]
enum First {
    List,
    Conversation,
}

/// The head PR 1's facts keep and the head its Conversation was read at, after a list read at `list_at`
/// naming `bbbbbbb` and a Conversation read at `conversation_at` naming `ccccccc` save in the order `first`
/// says. The facts held `aaaaaaa` from a list read at second 1. Also what the Conversation's save did to
/// the head.
fn after_both(
    first: First,
    (list_at, conversation_at): (i64, i64),
) -> (HeadKept, Option<CommitSha>, Option<CommitSha>) {
    let world = World::new(&format!("both-reads-{first:?}-{list_at}"));
    let at = |seconds, commit| PullRequestFacts {
        updated_at: at_second(seconds),
        head: Some(sha(commit)),
        ..pr_facts(1)
    };
    world.has(vec![at(1, "aaaaaaa")]);
    let list = || world.has(vec![at(list_at, "bbbbbbb")]);
    let conversation = || {
        (world.db.store)
            .save_read_of_pull_request(
                &cpython(),
                1,
                (with_reviews(vec![]), at_second(conversation_at)),
            )
            .unwrap()
            .head
    };
    let saved = match first {
        First::List => {
            list();
            conversation()
        }
        First::Conversation => {
            let saved = conversation();
            list();
            saved
        }
    };
    let (kept, read_at) = heads(&world, 1);
    world.finish();
    (saved, kept, read_at)
}

#[test]
fn the_newer_reads_head_stays_whichever_saves_first() {
    let (bbb, ccc) = (Some(sha("bbbbbbb")), Some(sha("ccccccc")));
    let moved = HeadKept::Moved(head_of(1, "ccccccc"));

    assert_eq!(
        after_both(First::List, (10, 20)),
        (moved.clone(), ccc.clone(), ccc.clone())
    );
    assert_eq!(
        after_both(First::Conversation, (10, 20)),
        (moved.clone(), ccc.clone(), ccc.clone())
    );
    // The Conversation's head names the commit its threads' lines count in, and lags the facts' until the
    // next read of it. The rules read the facts'.
    assert_eq!(
        after_both(First::List, (20, 10)),
        (
            HeadKept::Had(head_of(1, "bbbbbbb")),
            bbb.clone(),
            ccc.clone()
        )
    );
    assert_eq!(after_both(First::Conversation, (20, 10)), (moved, bbb, ccc));
}
