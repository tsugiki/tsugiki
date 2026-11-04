//! The Workbench's view model (ADR 0008), worked out from stored facts when the window asks (ADR 0010). Each
//! item is an Issue with the PRs that fix it, or a Standalone PR (ADR 0011).

use std::cmp::Reverse;
use std::collections::BTreeMap;

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use crate::conventions::Conventions;
use crate::error::Error;
use crate::github::Login;
use crate::github::issues::IssueFacts;
use crate::github::pull_requests::{CiState, PullRequestFacts, ReviewState};
use crate::links::fixed_numbers;
use crate::repository::RepositorySettings;
use crate::store::{StoreReader, WorkFacts};

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
    pub is_draft: bool,
    pub target_branch: String,
    pub ci: Option<CiState>,
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub updated_at: OffsetDateTime,
    /// Empty for a PR under an Issue that another PR put on the Workbench.
    pub reasons: Vec<WorkReason>,
}

/// An Issue on the Workbench, with each open PR Tsugiki knows that fixes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct IssueItem {
    pub number: u32,
    pub title: String,
    /// The Issue's author, then the PRs' authors, with each person once and no backport bot.
    pub people: Vec<Login>,
    /// The latest update of the Issue and its PRs.
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub updated_at: OffsetDateTime,
    pub pull_requests: Vec<WorkbenchRow>,
}

/// A top-level entry of the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkbenchItem {
    Issue(IssueItem),
    StandalonePr(WorkbenchRow),
}

impl WorkbenchItem {
    fn updated_at_and_number(&self) -> (OffsetDateTime, u32) {
        match self {
            Self::Issue(issue) => (issue.updated_at, issue.number),
            Self::StandalonePr(row) => (row.updated_at, row.number),
        }
    }
}

/// The items of a Repository's Workbench, the latest update first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Workbench {
    pub items: Vec<WorkbenchItem>,
}

impl Workbench {
    /// Reads the Workbench of `repository`. This waits on the database and never on a sync (ADR 0018).
    pub fn read(reader: &StoreReader, repository: &RepositorySettings) -> Result<Self, Error> {
        let facts = reader.work_facts(&repository.name)?;
        Ok(Self::from_facts(facts, repository))
    }

    /// No viewer means nobody has signed in yet, so no PR is the user's.
    fn from_facts(facts: WorkFacts, repository: &RepositorySettings) -> Self {
        let Some(viewer) = facts.viewer else {
            return Self { items: Vec::new() };
        };
        let mut issues: BTreeMap<u32, (IssueFacts, Vec<WorkbenchRow>)> = facts
            .issues
            .into_iter()
            .map(|issue| (issue.number, (issue, Vec::new())))
            .collect();
        let mut items = Vec::new();
        for facts in facts.pull_requests {
            let fixed = fixed_numbers(&facts, repository);
            let row = row(facts, &viewer);
            // Only a number the sync fetched as an Issue links, so a PR's number or a typo doesn't.
            let under: Vec<_> = issues
                .iter_mut()
                .filter(|(number, _)| fixed.contains(number))
                .collect();
            if under.is_empty() && !row.reasons.is_empty() {
                items.push(WorkbenchItem::StandalonePr(row));
            } else {
                for (_, (_, rows)) in under {
                    rows.push(row.clone());
                }
            }
        }
        items.extend(
            issues
                .into_values()
                .filter(|(_, rows)| rows.iter().any(|row| !row.reasons.is_empty()))
                .map(|(issue, rows)| issue_item(issue, rows, repository.conventions)),
        );
        items.sort_by_key(|item| Reverse(item.updated_at_and_number()));
        Self { items }
    }
}

fn issue_item(
    issue: IssueFacts,
    pull_requests: Vec<WorkbenchRow>,
    conventions: Conventions,
) -> WorkbenchItem {
    let people = people(&issue, &pull_requests, conventions);
    let updated_at = pull_requests
        .iter()
        .map(|row| row.updated_at)
        .fold(issue.updated_at, Ord::max);
    WorkbenchItem::Issue(IssueItem {
        number: issue.number,
        title: issue.title,
        people,
        updated_at,
        pull_requests,
    })
}

fn people(
    issue: &IssueFacts,
    pull_requests: &[WorkbenchRow],
    conventions: Conventions,
) -> Vec<Login> {
    let authors = pull_requests.iter().filter_map(|row| row.author.as_ref());
    let mut people: Vec<Login> = Vec::new();
    for author in issue.author.iter().chain(authors) {
        if !conventions.is_backport_bot(author) && !people.contains(author) {
            people.push(author.clone());
        }
    }
    people
}

/// The PR's line of the list, whatever it has to do with `viewer`.
fn row(facts: PullRequestFacts, viewer: &Login) -> WorkbenchRow {
    let reasons = reasons(&facts, viewer);
    WorkbenchRow {
        number: facts.number,
        title: facts.title,
        author: facts.author,
        is_draft: facts.is_draft,
        target_branch: facts.target_branch,
        ci: facts.ci,
        updated_at: facts.updated_at,
        reasons,
    }
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
    use crate::repository::RepositoryName;

    fn me() -> Login {
        Login::new("octocat")
    }

    fn someone() -> Login {
        Login::new("some-author")
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
            updated_at: at(1),
            review_requests: vec![],
            latest_reviews: vec![],
            assignees: vec![],
            closing_issues: vec![],
            off_default_description: None,
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

    fn at(seconds: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(seconds).unwrap()
    }

    fn cpython() -> RepositorySettings {
        RepositorySettings::new(RepositoryName::parse("python/cpython").unwrap())
    }

    fn issue(number: u32) -> IssueFacts {
        IssueFacts {
            number,
            title: format!("Issue {number}"),
            author: Some(Login::new("issue-author")),
            updated_at: at(1),
        }
    }

    /// The user's PR, whose title names no Issue.
    fn mine(number: u32) -> PullRequestFacts {
        PullRequestFacts {
            number,
            title: format!("PR {number}"),
            author: Some(me()),
            ..stranger()
        }
    }

    /// The user's PR, whose title names `issue`.
    fn fixing(number: u32, issue: u32) -> PullRequestFacts {
        PullRequestFacts {
            title: format!("gh-{issue}: Fix it"),
            ..mine(number)
        }
    }

    fn workbench(pull_requests: Vec<PullRequestFacts>, issues: Vec<IssueFacts>) -> Workbench {
        let facts = WorkFacts {
            viewer: Some(me()),
            pull_requests,
            issues,
        };
        Workbench::from_facts(facts, &cpython())
    }

    /// Each item as its kind and number, with an Issue's PRs after it.
    fn outline(workbench: &Workbench) -> Vec<(&'static str, u32, Vec<u32>)> {
        workbench
            .items
            .iter()
            .map(|item| match item {
                WorkbenchItem::Issue(issue) => {
                    let numbers = issue.pull_requests.iter().map(|row| row.number);
                    ("issue", issue.number, numbers.collect())
                }
                WorkbenchItem::StandalonePr(row) => ("standalone", row.number, vec![]),
            })
            .collect()
    }

    fn first_issue(workbench: &Workbench) -> &IssueItem {
        let issues = workbench.items.iter().filter_map(|item| match item {
            WorkbenchItem::Issue(issue) => Some(issue),
            WorkbenchItem::StandalonePr(_) => None,
        });
        let mut issues = issues;
        issues.next().unwrap()
    }

    #[test]
    fn a_pr_naming_an_issue_sits_under_it() {
        let found = workbench(vec![fixing(10, 1)], vec![issue(1)]);
        assert_eq!(outline(&found), [("issue", 1, vec![10])]);
    }

    #[test]
    fn a_pr_naming_no_issue_is_a_standalone_pr() {
        let found = workbench(vec![mine(10)], vec![issue(1)]);
        assert_eq!(outline(&found), [("standalone", 10, vec![])]);
    }

    #[test]
    fn a_number_the_sync_didnt_fetch_as_an_issue_doesnt_link() {
        let found = workbench(vec![fixing(10, 9)], vec![issue(1)]);
        assert_eq!(outline(&found), [("standalone", 10, vec![])]);
    }

    #[test]
    fn changing_a_repositorys_conventions_regroups_the_same_facts() {
        let facts = || WorkFacts {
            viewer: Some(me()),
            pull_requests: vec![fixing(10, 1)],
            issues: vec![issue(1)],
        };
        let github = RepositorySettings {
            conventions: Conventions::Github,
            ..cpython()
        };

        let under_cpython = Workbench::from_facts(facts(), &cpython());
        let under_github = Workbench::from_facts(facts(), &github);

        assert_eq!(outline(&under_cpython), [("issue", 1, vec![10])]);
        assert_eq!(outline(&under_github), [("standalone", 10, vec![])]);
    }

    #[test]
    fn a_pr_fixing_several_issues_appears_under_each() {
        let both = PullRequestFacts {
            closing_issues: vec![2],
            ..fixing(10, 1)
        };
        let found = workbench(vec![both], vec![issue(1), issue(2)]);
        assert_eq!(
            outline(&found),
            [("issue", 2, vec![10]), ("issue", 1, vec![10])]
        );
    }

    #[test]
    fn a_retitled_pr_moves_to_the_issue_its_title_names_now() {
        let before = workbench(vec![fixing(10, 1)], vec![issue(1), issue(2)]);
        let after = workbench(vec![fixing(10, 2)], vec![issue(1), issue(2)]);
        assert_eq!(outline(&before), [("issue", 1, vec![10])]);
        assert_eq!(outline(&after), [("issue", 2, vec![10])]);
    }

    #[test]
    fn an_issue_with_no_pr_of_the_users_isnt_listed() {
        let other = PullRequestFacts {
            title: "gh-1: Fix it".to_owned(),
            ..stranger()
        };
        let found = workbench(vec![other], vec![issue(1), issue(2)]);
        assert_eq!(outline(&found), []);
    }

    #[test]
    fn a_pr_of_someone_elses_with_no_issue_isnt_listed() {
        let found = workbench(vec![stranger()], vec![]);
        assert_eq!(outline(&found), []);
    }

    #[test]
    fn an_issue_lists_every_pr_fixing_it_once_one_is_the_users() {
        let other = PullRequestFacts {
            number: 11,
            title: "gh-1: Fix it too".to_owned(),
            ..stranger()
        };
        let found = workbench(vec![other, fixing(10, 1)], vec![issue(1)]);
        assert_eq!(outline(&found), [("issue", 1, vec![11, 10])]);
    }

    #[test]
    fn an_issues_people_are_its_author_then_its_prs_authors_once_each() {
        let issue_author = PullRequestFacts {
            number: 11,
            title: "gh-1: Fix it".to_owned(),
            author: Some(Login::new("Issue-Author")),
            ..stranger()
        };
        let bot = PullRequestFacts {
            number: 12,
            title: "[3.14] gh-1: Fix it".to_owned(),
            author: Some(Login::new("miss-islington")),
            ..stranger()
        };
        let gone = PullRequestFacts {
            number: 13,
            title: "gh-1: Fix it".to_owned(),
            author: None,
            ..stranger()
        };
        let found = workbench(vec![fixing(10, 1), issue_author, bot, gone], vec![issue(1)]);
        assert_eq!(
            first_issue(&found).people,
            [Login::new("issue-author"), me()],
        );
    }

    #[test]
    fn an_issue_with_no_author_starts_its_people_at_its_prs() {
        let gone = IssueFacts {
            author: None,
            ..issue(1)
        };
        let found = workbench(vec![fixing(10, 1)], vec![gone]);
        assert_eq!(first_issue(&found).people, [me()]);
    }

    #[test]
    fn an_issues_time_is_the_latest_of_it_and_its_prs() {
        let later_pr = PullRequestFacts {
            updated_at: at(5),
            ..fixing(10, 1)
        };
        let found = workbench(vec![later_pr.clone()], vec![issue(1)]);
        assert_eq!(first_issue(&found).updated_at, at(5));

        let later_issue = IssueFacts {
            updated_at: at(9),
            ..issue(1)
        };
        let found = workbench(vec![later_pr], vec![later_issue]);
        assert_eq!(first_issue(&found).updated_at, at(9));
    }

    #[test]
    fn items_come_latest_first_and_a_tie_goes_to_the_larger_number() {
        let latest = PullRequestFacts {
            updated_at: at(9),
            ..mine(30)
        };
        let tied = PullRequestFacts {
            updated_at: at(5),
            ..mine(20)
        };
        let under = PullRequestFacts {
            updated_at: at(5),
            ..fixing(10, 1)
        };
        let found = workbench(vec![tied, latest, under], vec![issue(1)]);
        assert_eq!(
            outline(&found),
            [
                ("standalone", 30, vec![]),
                ("standalone", 20, vec![]),
                ("issue", 1, vec![10]),
            ]
        );
        assert_eq!(first_issue(&found).number, 1);
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
            [WorkbenchItem::StandalonePr(WorkbenchRow {
                number: 3,
                title: "PR 3".to_owned(),
                author: Some(me()),
                is_draft: true,
                target_branch: "main".to_owned(),
                ci: Some(CiState::Pending),
                updated_at: at(3),
                reasons: vec![WorkReason::OwnPr],
            })]
        );
    }

    #[test]
    fn the_window_gets_the_kind_of_each_item_and_its_times_as_text() {
        let found = workbench(vec![mine(3), fixing(4, 1)], vec![issue(1)]);
        let sent = serde_json::to_value(&found).unwrap();
        assert_eq!(sent["items"][0]["kind"], "standalone_pr");
        assert_eq!(sent["items"][0]["number"], 3);
        assert_eq!(sent["items"][0]["updated_at"], "1970-01-01T00:00:01Z");
        assert_eq!(sent["items"][1]["kind"], "issue");
        assert_eq!(sent["items"][1]["updated_at"], "1970-01-01T00:00:01Z");
        assert_eq!(sent["items"][1]["pull_requests"][0]["number"], 4);
    }

    #[test]
    fn nothing_is_the_users_before_anyone_signs_in() {
        let found = Workbench::from_facts(
            WorkFacts {
                viewer: None,
                pull_requests: vec![mine(1)],
                issues: vec![issue(1)],
            },
            &cpython(),
        );
        assert_eq!(found.items, []);
    }
}
