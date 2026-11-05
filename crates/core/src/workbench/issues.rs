//! An Issue on the Workbench, with its tree and its people.

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use super::{TreeEntry, WorkbenchItem};
use crate::conventions::Conventions;
use crate::github::Login;
use crate::github::issues::IssueFacts;

/// An Issue on the Workbench, with its tree: each PR Tsugiki knows that fixes it, whatever its state.
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
    pub pull_requests: Vec<TreeEntry>,
}

pub(super) fn issue_item(
    issue: IssueFacts,
    pull_requests: Vec<TreeEntry>,
    conventions: Conventions,
) -> WorkbenchItem {
    let people = people(&issue, &pull_requests, conventions);
    let updated_at = pull_requests
        .iter()
        .flat_map(TreeEntry::rows)
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

fn people(issue: &IssueFacts, pull_requests: &[TreeEntry], conventions: Conventions) -> Vec<Login> {
    let authors = pull_requests
        .iter()
        .flat_map(TreeEntry::rows)
        .filter_map(|row| row.author.as_ref());
    let mut people: Vec<Login> = Vec::new();
    for author in issue.author.iter().chain(authors) {
        if !conventions.is_backport_bot(author) && !people.contains(author) {
            people.push(author.clone());
        }
    }
    people
}

#[cfg(test)]
mod tests {
    use crate::github::Login;
    use crate::github::issues::IssueFacts;
    use crate::github::pull_requests::PullRequestFacts;
    use crate::workbench::testing::{
        at, backport, bots_backport, first_issue, fixing, issue, me, stranger, workbench,
    };

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
    fn an_issues_people_and_time_count_its_backports() {
        let later = PullRequestFacts {
            updated_at: at(7),
            author: Some(Login::new("some-backporter")),
            ..backport(11, "[3.14] gh-1: Fix it (GH-10)", vec![])
        };
        let found = workbench(
            vec![
                fixing(10, 1),
                later,
                bots_backport(12, "[3.13] gh-1: Fix it (GH-10)"),
            ],
            vec![issue(1)],
        );
        let item = first_issue(&found);
        assert_eq!(item.updated_at, at(7));
        assert_eq!(
            item.people,
            [
                Login::new("issue-author"),
                me(),
                Login::new("some-backporter")
            ]
        );
    }
}
