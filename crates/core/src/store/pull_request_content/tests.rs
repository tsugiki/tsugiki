use time::OffsetDateTime;

use super::*;
use crate::github::issue_content::IssueContent;
use crate::github::pull_requests::ReviewedCommit;
use crate::login::Login;
use crate::store::Store;
use crate::store::testing::temp_database;

fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

fn sha(hex: &str) -> CommitSha {
    CommitSha::parse(hex).unwrap()
}

/// A Review by `author` of the commit `commit`, `hours` after 1970.
fn review(author: &str, state: ReviewState, commit: Option<&str>, hours: i64) -> Review {
    Review {
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
    std::fs::remove_dir_all(folder).unwrap();
}
