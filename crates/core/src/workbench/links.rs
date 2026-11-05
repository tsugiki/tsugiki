//! The Issues each PR fixes, by the link rules (ADR 0011).

use std::collections::BTreeSet;

use super::WorkbenchRow;
use super::rows::row;
use crate::github::Login;
use crate::github::issues::IssueFacts;
use crate::github::pull_requests::PullRequestFacts;
use crate::links::{BackportLink, MergeCommits, backport_of, fixed_numbers};
use crate::repository::RepositorySettings;

/// A PR with what the link rules say about it.
pub(super) struct Linked {
    pub(super) row: WorkbenchRow,
    /// The Issues it fixes. Only a number the sync fetched as an Issue counts, so a PR's number or a typo
    /// doesn't.
    pub(super) issues: BTreeSet<u32>,
    pub(super) backport: Option<BackportLink>,
}

pub(super) fn link(
    pull_requests: Vec<PullRequestFacts>,
    issues: &[IssueFacts],
    repository: &RepositorySettings,
    viewer: &Login,
) -> Vec<Linked> {
    let merges = MergeCommits::of(&pull_requests);
    let known: BTreeSet<u32> = issues.iter().map(|issue| issue.number).collect();
    pull_requests
        .into_iter()
        .map(|facts| {
            let fixed = fixed_numbers(&facts, repository);
            Linked {
                issues: fixed.intersection(&known).copied().collect(),
                backport: backport_of(&facts, repository.conventions, &merges),
                row: row(facts, viewer),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::conventions::Conventions;
    use crate::github::pull_requests::PullRequestFacts;
    use crate::repository::RepositorySettings;
    use crate::store::WorkFacts;
    use crate::workbench::Workbench;
    use crate::workbench::testing::{
        backport, cpython, fixing, issue, me, mine, outline, own, stranger, workbench,
    };
    use std::collections::BTreeMap;

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
            filters: BTreeMap::new(),
            pull_requests: vec![fixing(10, 1)],
            issues: vec![issue(1)],
        };
        let github = RepositorySettings {
            conventions: Conventions::Github,
            ..cpython()
        };

        let under_cpython = Workbench::from_facts(facts(), &cpython(), None);
        let under_github = Workbench::from_facts(facts(), &github, None);

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
    fn under_githubs_conventions_a_pr_on_a_branch_is_an_ordinary_pr() {
        let github = RepositorySettings {
            conventions: Conventions::Github,
            ..cpython()
        };
        let facts = WorkFacts {
            viewer: Some(me()),
            pull_requests: vec![mine(10), own(backport(11, "[3.14] Fix it (GH-10)", vec![]))],
            issues: vec![],
            filters: BTreeMap::new(),
        };
        let found = Workbench::from_facts(facts, &github, None);
        assert_eq!(
            outline(&found),
            [("standalone", 11, vec![]), ("standalone", 10, vec![])]
        );
    }
}
