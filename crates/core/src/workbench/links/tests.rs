use crate::conventions::Conventions;
use crate::github::pull_requests::PullRequestFacts;
use crate::repository::RepositorySettings;
use crate::store::{ActivityFacts, WorkFacts};
use crate::workbench::testing::{
    backport, cpython, fixing, issue, me, mine, outline, own, stranger, workbench,
};
use crate::workbench::{Workbench, WorkbenchView};
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
        activity: ActivityFacts::default(),
        viewer: Some(me()),
        filters: BTreeMap::new(),
        pull_requests: vec![fixing(10, 1)],
        issues: vec![issue(1)],
    };
    let github = RepositorySettings {
        conventions: Conventions::Github,
        ..cpython()
    };

    let under_cpython = Workbench::from_facts(facts(), &cpython(), &WorkbenchView::Whole);
    let under_github = Workbench::from_facts(facts(), &github, &WorkbenchView::Whole);

    assert_eq!(outline(&under_cpython), [("issue", 1, vec![10])]);
    assert_eq!(outline(&under_github), [("standalone", 10, vec![])]);
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
        activity: ActivityFacts::default(),
        viewer: Some(me()),
        pull_requests: vec![mine(10), own(backport(11, "[3.14] Fix it (GH-10)", vec![]))],
        issues: vec![],
        filters: BTreeMap::new(),
    };
    let found = Workbench::from_facts(facts, &github, &WorkbenchView::Whole);
    assert_eq!(
        outline(&found),
        [("standalone", 11, vec![]), ("standalone", 10, vec![])]
    );
}
