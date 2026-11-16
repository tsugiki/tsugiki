use super::*;
use crate::facts::pull_requests::{LatestReview, ReviewState};
use crate::workbench::testing::{me, reasons_for, someone, stranger};

fn review(author: Login, state: ReviewState) -> LatestReview {
    LatestReview { author, state }
}

#[test]
fn a_requested_reviewer_has_a_review_request() {
    let facts = PullRequestFacts {
        review_requests: vec![me()],
        ..stranger()
    };
    assert_eq!(reasons_for(&facts), [WorkReason::ReviewRequest]);
}

#[test]
fn an_assignee_has_a_review_request() {
    let facts = PullRequestFacts {
        assignees: vec![me()],
        ..stranger()
    };
    assert_eq!(reasons_for(&facts), [WorkReason::ReviewRequest]);
}

#[test]
fn a_pr_the_user_wrote_is_an_own_pr() {
    let facts = PullRequestFacts {
        author: Some(me()),
        ..stranger()
    };
    assert_eq!(reasons_for(&facts), [WorkReason::OwnPr]);
}

#[test]
fn a_submitted_review_counts_whatever_it_said() {
    for state in [
        ReviewState::Commented,
        ReviewState::Approved,
        ReviewState::ChangesRequested,
        ReviewState::Dismissed,
    ] {
        let facts = PullRequestFacts {
            latest_reviews: vec![review(me(), state)],
            ..stranger()
        };
        assert_eq!(reasons_for(&facts), [WorkReason::Reviewed], "{state:?}");
    }
}

#[test]
fn a_pending_review_isnt_a_review_yet() {
    let facts = PullRequestFacts {
        latest_reviews: vec![review(me(), ReviewState::Pending)],
        ..stranger()
    };
    assert_eq!(reasons_for(&facts), []);
}

#[test]
fn someone_elses_review_or_request_isnt_the_users() {
    let facts = PullRequestFacts {
        review_requests: vec![someone()],
        assignees: vec![someone()],
        latest_reviews: vec![review(someone(), ReviewState::Approved)],
        ..stranger()
    };
    assert_eq!(reasons_for(&facts), []);
}

#[test]
fn a_pr_with_no_author_isnt_the_users() {
    let facts = PullRequestFacts {
        author: None,
        ..stranger()
    };
    assert_eq!(reasons_for(&facts), []);
}

#[test]
fn a_login_matches_whatever_its_case() {
    let facts = PullRequestFacts {
        review_requests: vec![Login::new("OctoCat")],
        ..stranger()
    };
    assert_eq!(reasons_for(&facts), [WorkReason::ReviewRequest]);
}

#[test]
fn every_reason_that_applies_is_listed() {
    let facts = PullRequestFacts {
        author: Some(me()),
        review_requests: vec![me()],
        latest_reviews: vec![review(me(), ReviewState::Commented)],
        ..stranger()
    };
    assert_eq!(
        reasons_for(&facts),
        [
            WorkReason::ReviewRequest,
            WorkReason::OwnPr,
            WorkReason::Reviewed
        ]
    );
}
