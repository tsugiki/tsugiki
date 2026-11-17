//! Test helpers for the Workbench: facts to read, and an outline of what they show.

use std::collections::BTreeMap;

use time::OffsetDateTime;

use super::{IssueItem, TreeEntry, Workbench, WorkbenchItem, WorkbenchRow, WorkbenchView};
use crate::facts::WorkFacts;
use crate::facts::activity::ActivityFacts;
use crate::facts::issues::IssueFacts;
use crate::facts::patch_ids::ReviewFacts;
use crate::facts::pull_requests::{OffDefault, PullRequestFacts};
use crate::login::Login;
use crate::repository_settings::RepositorySettings;
use crate::store::testing::OpenStore;
use crate::testing::{cpython, cpython_settings, issue_facts, pr_facts};
use crate::work::{WorkReason, reasons};

pub(crate) use crate::testing::me;

pub(crate) fn someone() -> Login {
    Login::new("some-author")
}

/// A PR that has nothing to do with the viewer.
pub(crate) fn stranger() -> PullRequestFacts {
    PullRequestFacts {
        title: "gh-1: Fix it".to_owned(),
        author: Some(someone()),
        updated_at: at(1),
        ..pr_facts(1)
    }
}

pub(crate) fn at(seconds: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(seconds).unwrap()
}

pub(crate) fn issue(number: u32) -> IssueFacts {
    IssueFacts {
        author: Some(Login::new("issue-author")),
        updated_at: at(1),
        ..issue_facts(number)
    }
}

/// The user's PR, whose title names no Issue.
pub(crate) fn mine(number: u32) -> PullRequestFacts {
    PullRequestFacts {
        number,
        title: format!("PR {number}"),
        author: Some(me()),
        ..stranger()
    }
}

/// The user's PR, whose title names `issue`.
pub(crate) fn fixing(number: u32, issue: u32) -> PullRequestFacts {
    PullRequestFacts {
        title: format!("gh-{issue}: Fix it"),
        ..mine(number)
    }
}

pub(crate) fn reasons_for(facts: &PullRequestFacts) -> Vec<WorkReason> {
    reasons(facts, &me())
}

pub(crate) fn workbench(
    pull_requests: Vec<PullRequestFacts>,
    issues: Vec<IssueFacts>,
) -> Workbench {
    reviewed_workbench(pull_requests, issues, ReviewFacts::default())
}

/// The view `view` of the Workbench of `facts`.
pub(crate) fn view_of(facts: WorkFacts, view: &WorkbenchView) -> Workbench {
    Workbench::from_facts(facts, &cpython_settings(), view)
}

/// Each PR `item` shows, in the order the list shows them.
pub(crate) fn rows_of(item: &WorkbenchItem) -> Vec<WorkbenchRow> {
    item.rows().into_iter().cloned().collect()
}

/// The Workbench of `pull_requests` and `issues`, with what `reviews` says of the user's Reviews.
pub(crate) fn reviewed_workbench(
    pull_requests: Vec<PullRequestFacts>,
    issues: Vec<IssueFacts>,
    reviews: ReviewFacts,
) -> Workbench {
    let facts = WorkFacts {
        viewer: Some(me()),
        pull_requests,
        issues,
        filters: BTreeMap::new(),
        activity: ActivityFacts::default(),
        reviews,
    };
    Workbench::from_facts(facts, &cpython_settings(), &WorkbenchView::Whole)
}

/// Each item as its kind and number, with an Issue's PRs after it.
pub(crate) fn outline(workbench: &Workbench) -> Vec<(&'static str, u32, Vec<u32>)> {
    workbench
        .items
        .iter()
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

pub(crate) fn first_issue(workbench: &Workbench) -> &IssueItem {
    let mut issues = workbench
        .items
        .iter()
        .filter_map(|listed| match &listed.item {
            WorkbenchItem::Issue(issue) => Some(issue),
            WorkbenchItem::StandalonePr(_) | WorkbenchItem::StandaloneBackport(_) => None,
        });
    issues.next().unwrap()
}

pub(crate) const BOT: &str = "miss-islington";

/// A PR on 3.14 by someone else, titled `title` and committed by `committers`.
pub(crate) fn backport(
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

pub(crate) fn bots_backport(number: u32, title: &str) -> PullRequestFacts {
    PullRequestFacts {
        author: Some(Login::new(BOT)),
        ..backport(number, title, vec![Some(Login::new(BOT))])
    }
}

pub(crate) fn own(facts: PullRequestFacts) -> PullRequestFacts {
    PullRequestFacts {
        author: Some(me()),
        ..facts
    }
}

/// A database with the user signed in, and a reader of it, for the Detail pane's views.
pub(crate) struct ViewWorld {
    pub(crate) db: OpenStore,
}

impl ViewWorld {
    pub(crate) fn new(name: &str) -> Self {
        let db = OpenStore::new(name);
        db.store.remember_login(&me()).unwrap();
        Self { db }
    }

    pub(crate) fn settings(&self) -> RepositorySettings {
        cpython_settings()
    }

    pub(crate) fn has(&self, pull_requests: Vec<PullRequestFacts>, issues: Vec<IssueFacts>) {
        let store = &self.db.store;
        store.save_pull_requests(&cpython(), pull_requests).unwrap();
        store.save_issues(&cpython(), issues).unwrap();
    }

    pub(crate) fn finish(self) {
        self.db.close();
    }
}
