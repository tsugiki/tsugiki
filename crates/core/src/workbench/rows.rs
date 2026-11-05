//! A PR's row, and the reasons it's the user's work.

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use crate::github::Login;
use crate::github::pull_requests::{CiState, PrState, PullRequestFacts, ReviewState};

/// The reason a PR is on the user's Workbench.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WorkReason {
    /// The user is a requested reviewer or an assignee.
    ReviewRequest,
    OwnPr,
    /// The user has submitted a Review.
    Reviewed,
}

/// A PR's line of the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct WorkbenchRow {
    pub number: u32,
    pub title: String,
    pub author: Option<Login>,
    pub state: PrState,
    pub is_draft: bool,
    pub target_branch: String,
    pub ci: Option<CiState>,
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub updated_at: OffsetDateTime,
    /// Empty for a PR that another PR put on the Workbench, and for one no longer open.
    pub reasons: Vec<WorkReason>,
}

/// The PR's line of the list, whatever it has to do with `viewer`.
pub(super) fn row(facts: PullRequestFacts, viewer: &Login) -> WorkbenchRow {
    let reasons = reasons(&facts, viewer);
    WorkbenchRow {
        number: facts.number,
        title: facts.title,
        author: facts.author,
        state: facts.state,
        is_draft: facts.is_draft,
        target_branch: facts.target_branch,
        ci: facts.ci,
        updated_at: facts.updated_at,
        reasons,
    }
}

/// A PR that has merged or closed waits on nobody, so it keeps no Issue on the Workbench.
pub(crate) fn reasons(facts: &PullRequestFacts, viewer: &Login) -> Vec<WorkReason> {
    if facts.state != PrState::Open {
        return Vec::new();
    }
    let asked = facts.review_requests.contains(viewer) || facts.assignees.contains(viewer);
    let own = facts.author.as_ref() == Some(viewer);
    // A pending Review is a draft only its author can see, so it isn't one yet.
    let reviewed = facts
        .latest_reviews
        .iter()
        .any(|review| review.author == *viewer && review.state != ReviewState::Pending);
    [
        (asked, WorkReason::ReviewRequest),
        (own, WorkReason::OwnPr),
        (reviewed, WorkReason::Reviewed),
    ]
    .into_iter()
    .filter_map(|(applies, reason)| applies.then_some(reason))
    .collect()
}

#[cfg(test)]
mod tests {
    use crate::github::Login;
    use crate::github::pull_requests::{
        CiState, LatestReview, PrState, PullRequestFacts, ReviewState,
    };
    use crate::workbench::testing::{at, me, mine, reasons_for, someone, stranger, workbench};
    use crate::workbench::{PrTree, WorkReason, WorkbenchItem, WorkbenchRow};

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
            assignees: vec![me()],
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

    #[test]
    fn a_row_carries_what_the_list_shows() {
        let found = workbench(
            vec![PullRequestFacts {
                is_draft: true,
                ci: Some(CiState::Pending),
                updated_at: at(3),
                ..mine(3)
            }],
            vec![],
        );
        assert_eq!(
            found.items,
            [WorkbenchItem::StandalonePr(PrTree {
                row: WorkbenchRow {
                    number: 3,
                    title: "PR 3".to_owned(),
                    author: Some(me()),
                    state: PrState::Open,
                    is_draft: true,
                    target_branch: "main".to_owned(),
                    ci: Some(CiState::Pending),
                    updated_at: at(3),
                    reasons: vec![WorkReason::OwnPr],
                },
                backports: vec![],
            })]
        );
    }
}
