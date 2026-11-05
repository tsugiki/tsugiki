//! Test helpers for the Workbench: facts to read, and an outline of what they show.

use std::collections::BTreeMap;

use time::OffsetDateTime;

use super::{IssueItem, TreeEntry, WorkReason, Workbench, WorkbenchItem, WorkbenchView, reasons};
use crate::github::Login;
use crate::github::issues::IssueFacts;
use crate::github::pull_requests::{OffDefault, PrState, PullRequestFacts};
use crate::repository::{RepositoryName, RepositorySettings};
use crate::store::{ActivityFacts, WorkFacts};

pub(super) fn me() -> Login {
    Login::new("octocat")
}

pub(super) fn someone() -> Login {
    Login::new("some-author")
}

/// A PR that has nothing to do with the viewer.
pub(super) fn stranger() -> PullRequestFacts {
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
        merge_commit: None,
        off_default: None,
    }
}

pub(super) fn reasons_for(facts: &PullRequestFacts) -> Vec<WorkReason> {
    reasons(facts, &me())
}

pub(super) fn at(seconds: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(seconds).unwrap()
}

pub(super) fn cpython() -> RepositorySettings {
    RepositorySettings::new(RepositoryName::parse("python/cpython").unwrap())
}

pub(super) fn issue(number: u32) -> IssueFacts {
    IssueFacts {
        number,
        title: format!("Issue {number}"),
        author: Some(Login::new("issue-author")),
        assignees: vec![],
        updated_at: at(1),
    }
}

/// The user's PR, whose title names no Issue.
pub(super) fn mine(number: u32) -> PullRequestFacts {
    PullRequestFacts {
        number,
        title: format!("PR {number}"),
        author: Some(me()),
        ..stranger()
    }
}

/// The user's PR, whose title names `issue`.
pub(super) fn fixing(number: u32, issue: u32) -> PullRequestFacts {
    PullRequestFacts {
        title: format!("gh-{issue}: Fix it"),
        ..mine(number)
    }
}

pub(super) fn workbench(
    pull_requests: Vec<PullRequestFacts>,
    issues: Vec<IssueFacts>,
) -> Workbench {
    let facts = WorkFacts {
        viewer: Some(me()),
        pull_requests,
        issues,
        filters: BTreeMap::new(),
        activity: ActivityFacts::default(),
    };
    Workbench::from_facts(facts, &cpython(), &WorkbenchView::Whole)
}

/// Each item as its kind and number, with an Issue's PRs after it.
pub(super) fn outline(workbench: &Workbench) -> Vec<(&'static str, u32, Vec<u32>)> {
    workbench
        .items
        .iter()
        .chain(&workbench.quiet)
        .map(|listed| match &listed.item {
            WorkbenchItem::Issue(issue) => {
                let rows = issue.pull_requests.iter().flat_map(TreeEntry::rows);
                ("issue", issue.number, rows.map(|row| row.number).collect())
            }
            WorkbenchItem::StandalonePr(tree) => {
                let beneath = tree.backports.iter().map(|below| below.row.number);
                ("standalone", tree.row.number, beneath.collect())
            }
            WorkbenchItem::StandaloneBackport(backport) => {
                ("standalone backport", backport.row.number, vec![])
            }
        })
        .collect()
}

pub(super) fn first_issue(workbench: &Workbench) -> &IssueItem {
    let listed = workbench.items.iter().chain(&workbench.quiet);
    let issues = listed.filter_map(|listed| match &listed.item {
        WorkbenchItem::Issue(issue) => Some(issue),
        WorkbenchItem::StandalonePr(_) | WorkbenchItem::StandaloneBackport(_) => None,
    });
    let mut issues = issues;
    issues.next().unwrap()
}

pub(super) const BOT: &str = "miss-islington";

/// A PR on 3.14 by someone else, titled `title` and committed by `committers`.
pub(super) fn backport(
    number: u32,
    title: &str,
    committers: Vec<Option<Login>>,
) -> PullRequestFacts {
    PullRequestFacts {
        number,
        title: title.to_owned(),
        target_branch: "3.14".to_owned(),
        off_default: Some(OffDefault {
            description: String::new(),
            cherry_picked_from: vec![],
            committers,
        }),
        ..stranger()
    }
}

pub(super) fn bots_backport(number: u32, title: &str) -> PullRequestFacts {
    PullRequestFacts {
        author: Some(Login::new(BOT)),
        ..backport(number, title, vec![Some(Login::new(BOT))])
    }
}

pub(super) fn own(facts: PullRequestFacts) -> PullRequestFacts {
    PullRequestFacts {
        author: Some(me()),
        ..facts
    }
}
