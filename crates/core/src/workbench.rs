//! The Workbench's view model (ADR 0008), worked out from stored facts when the window asks (ADR 0010). For
//! now each PR is a row of its own. Issues, and the PRs under them, come with the link rules.

use serde::Serialize;
use specta::Type;

use crate::error::Error;
use crate::github::Login;
use crate::github::pull_requests::{CiState, PullRequestFacts, ReviewState};
use crate::repository::RepositoryName;
use crate::store::{StoreReader, WorkFacts};

/// Why a PR is on the user's Workbench.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WorkReason {
    /// The user is a requested reviewer or an assignee.
    ReviewRequest,
    OwnPr,
    /// The user has submitted a Review.
    Reviewed,
}

/// One line of the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct WorkbenchRow {
    pub number: u32,
    pub title: String,
    pub author: Option<Login>,
    pub is_draft: bool,
    pub target_branch: String,
    pub ci: Option<CiState>,
    pub reasons: Vec<WorkReason>,
}

/// What the Workbench of a Repository shows, the latest update first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Workbench {
    pub rows: Vec<WorkbenchRow>,
}

impl Workbench {
    /// Reads the Workbench of `repository`. This waits on the database and never on a sync (ADR 0018).
    pub fn read(reader: &StoreReader, repository: &RepositoryName) -> Result<Self, Error> {
        Ok(Self::from_facts(reader.work_facts(repository)?))
    }

    /// Nobody has signed in yet if there is no viewer, and then nothing is the user's.
    fn from_facts(facts: WorkFacts) -> Self {
        let rows = facts.viewer.map_or_else(Vec::new, |viewer| {
            facts
                .pull_requests
                .into_iter()
                .filter_map(|pull_request| row(pull_request, &viewer))
                .collect()
        });
        Self { rows }
    }
}

/// The PR's row, if it has anything to do with `viewer`.
fn row(facts: PullRequestFacts, viewer: &Login) -> Option<WorkbenchRow> {
    let reasons = reasons(&facts, viewer);
    (!reasons.is_empty()).then_some(WorkbenchRow {
        number: facts.number,
        title: facts.title,
        author: facts.author,
        is_draft: facts.is_draft,
        target_branch: facts.target_branch,
        ci: facts.ci,
        reasons,
    })
}

fn reasons(facts: &PullRequestFacts, viewer: &Login) -> Vec<WorkReason> {
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
    use super::*;
    use crate::github::pull_requests::{LatestReview, PrState};

    fn me() -> Login {
        Login::new("octocat")
    }

    fn someone() -> Login {
        Login::new("vstinner")
    }

    /// A PR that has nothing to do with the viewer.
    fn stranger() -> PullRequestFacts {
        PullRequestFacts {
            number: 1,
            title: "gh-1: Fix it".to_owned(),
            state: PrState::Open,
            is_draft: false,
            author: Some(someone()),
            target_branch: "main".to_owned(),
            ci: None,
            review_requests: vec![],
            latest_reviews: vec![],
            assignees: vec![],
        }
    }

    fn reasons_for(facts: &PullRequestFacts) -> Vec<WorkReason> {
        reasons(facts, &me())
    }

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
    fn the_workbench_keeps_the_users_prs_in_their_order() {
        let mine = |number| PullRequestFacts {
            number,
            title: format!("PR {number}"),
            author: Some(me()),
            is_draft: true,
            ci: Some(CiState::Pending),
            ..stranger()
        };

        let workbench = Workbench::from_facts(WorkFacts {
            viewer: Some(me()),
            pull_requests: vec![mine(3), stranger(), mine(2)],
        });

        assert_eq!(
            workbench.rows,
            [
                WorkbenchRow {
                    number: 3,
                    title: "PR 3".to_owned(),
                    author: Some(me()),
                    is_draft: true,
                    target_branch: "main".to_owned(),
                    ci: Some(CiState::Pending),
                    reasons: vec![WorkReason::OwnPr],
                },
                WorkbenchRow {
                    number: 2,
                    title: "PR 2".to_owned(),
                    author: Some(me()),
                    is_draft: true,
                    target_branch: "main".to_owned(),
                    ci: Some(CiState::Pending),
                    reasons: vec![WorkReason::OwnPr],
                },
            ]
        );
    }

    #[test]
    fn nothing_is_the_users_before_anyone_signs_in() {
        let workbench = Workbench::from_facts(WorkFacts {
            viewer: None,
            pull_requests: vec![stranger()],
        });
        assert_eq!(workbench.rows, []);
    }
}
