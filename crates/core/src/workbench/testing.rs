//! Test helpers for the Workbench: facts to read, and an outline of what they show.

use std::collections::BTreeMap;

use time::OffsetDateTime;

use super::{IssueItem, TreeEntry, WorkReason, Workbench, WorkbenchItem, WorkbenchView, reasons};
use crate::github::issues::{IssueFacts, IssueState};
use crate::github::pull_requests::{OffDefault, PrState, PullRequestFacts};
use crate::login::Login;
use crate::repository::RepositoryName;
use crate::repository_settings::RepositorySettings;
use crate::store::{ActivityFacts, ReviewFacts, WorkFacts};

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
        labels: vec![],
        closing_issues: vec![],
        merge_commit: None,
        base: None,
        head: None,
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
        labels: vec![],
        updated_at: at(1),
        state: Some(IssueState::Open),
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
    reviewed_workbench(pull_requests, issues, ReviewFacts::default())
}

/// The Workbench of `pull_requests` and `issues`, with what `reviews` says of the user's Reviews.
pub(super) fn reviewed_workbench(
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

/// A database with the user signed in, and a reader of it, for the Detail pane's views.
pub(super) struct ViewWorld {
    pub(super) store: crate::store::Store,
    pub(super) reader: crate::store::StoreReader,
    open: crate::store::OpenReader,
    folder: std::path::PathBuf,
}

impl ViewWorld {
    pub(super) fn new(name: &str) -> Self {
        let folder = crate::testing::temp_path(name);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("tsugiki.db");
        let store = crate::store::Store::open(&path).unwrap();
        store.remember_login(&me()).unwrap();
        let (to_open, reader) = crate::store::reader();
        let open = to_open.open(&path).unwrap();
        Self {
            store,
            reader,
            open,
            folder,
        }
    }

    pub(super) fn settings(&self) -> RepositorySettings {
        cpython()
    }

    pub(super) fn has(&self, pull_requests: Vec<PullRequestFacts>, issues: Vec<IssueFacts>) {
        let name = cpython().name;
        self.store.save_pull_requests(&name, pull_requests).unwrap();
        self.store.save_issues(&name, issues).unwrap();
    }

    pub(super) fn finish(self) {
        self.open.close();
        self.store.close();
        std::fs::remove_dir_all(self.folder).unwrap();
    }
}
