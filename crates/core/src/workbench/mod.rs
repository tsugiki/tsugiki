//! The Workbench's view model (ADR 0008), worked out from stored facts when the window asks (ADR 0010). Each
//! item is an Issue with the PRs that fix it and their Backports, or a PR outside any Issue (ADR 0011).

mod attention;
mod backports;
mod filters;
pub mod issue_view;
mod issues;
mod links;
mod rows;

pub use attention::{Attention, Why};
pub use filters::FilterLine;
pub use issues::IssueItem;
pub(crate) use rows::reasons;
pub use rows::{WorkReason, WorkbenchRow};

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use specta::Type;
use time::OffsetDateTime;

use crate::error::Error;
use crate::filters::FilterQuery;
use crate::github::issues::IssueFacts;
use crate::links::BackportKind;
use crate::repository::RepositorySettings;
use crate::store::{StoreReader, WorkFacts};
use attention::Causes;
use backports::{plant, standalone};
use filters::FilterFacts;
use issues::issue_item;
use links::link;

/// A Backport's line of the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct BackportRow {
    #[serde(flatten)]
    pub row: WorkbenchRow,
    pub backport: BackportKind,
}

/// A PR with its Backports beneath it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct PrTree {
    #[serde(flatten)]
    pub row: WorkbenchRow,
    pub backports: Vec<BackportRow>,
}

/// A line directly under an Issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TreeEntry {
    Pr(PrTree),
    /// A Backport whose Origin PR Tsugiki can't show: that number names no PR, or a Backport.
    Backport(BackportRow),
}

impl TreeEntry {
    fn rows(&self) -> Vec<&WorkbenchRow> {
        match self {
            Self::Pr(tree) => tree.rows(),
            Self::Backport(backport) => vec![&backport.row],
        }
    }
}

impl PrTree {
    fn rows(&self) -> Vec<&WorkbenchRow> {
        let beneath = self.backports.iter().map(|backport| &backport.row);
        std::iter::once(&self.row).chain(beneath).collect()
    }
}

/// A top-level entry of the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkbenchItem {
    Issue(IssueItem),
    /// A PR that fixes no Issue, with its Backports.
    StandalonePr(PrTree),
    /// A Backport with no Issue and no Origin PR to sit under.
    StandaloneBackport(BackportRow),
}

impl WorkbenchItem {
    pub(crate) fn updated_at_and_number(&self) -> (OffsetDateTime, u32) {
        match self {
            Self::Issue(issue) => (issue.updated_at, issue.number),
            Self::StandalonePr(tree) => (tree.row.updated_at, tree.row.number),
            Self::StandaloneBackport(backport) => (backport.row.updated_at, backport.row.number),
        }
    }

    fn rows(&self) -> Vec<&WorkbenchRow> {
        match self {
            Self::Issue(issue) => issue
                .pull_requests
                .iter()
                .flat_map(TreeEntry::rows)
                .collect(),
            Self::StandalonePr(tree) => tree.rows(),
            Self::StandaloneBackport(backport) => vec![&backport.row],
        }
    }
}

/// A top-level entry of the list, with why it Needs attention, if it does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ListedItem {
    #[serde(flatten)]
    pub item: WorkbenchItem,
    pub attention: Option<Attention>,
}

/// The list the window asks for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkbenchView {
    Whole,
    Filter { query: FilterQuery },
    ReviewRequests,
}

/// A Repository's Workbench or one of its views, with what the Sidebar shows beside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Workbench {
    /// The items that Need attention, newest first by when they started to, then the rest by last update.
    pub items: Vec<ListedItem>,
    /// The Issues with nothing that Needs attention, the latest update first.
    pub quiet: Vec<ListedItem>,
    /// The count of items on the Workbench that Need attention.
    pub count: u32,
    /// The count of items that hold a PR carrying a Review request.
    pub review_requests: u32,
    pub filters: Vec<FilterLine>,
}

impl Workbench {
    /// Reads the view `view` of `repository`'s Workbench. This waits on the database and never on a sync
    /// (ADR 0018).
    pub fn read(
        reader: &StoreReader,
        repository: &RepositorySettings,
        view: &WorkbenchView,
    ) -> Result<Self, Error> {
        let facts = reader.work_facts(&repository.name)?;
        Ok(Self::from_facts(facts, repository, view))
    }

    /// No viewer means nobody has signed in yet, so no PR is the user's.
    fn from_facts(
        mut facts: WorkFacts,
        repository: &RepositorySettings,
        view: &WorkbenchView,
    ) -> Self {
        let filters = FilterFacts::new(repository, std::mem::take(&mut facts.filters));
        let matched = filters.matched();
        let causes = Causes::of(&facts, repository.conventions);
        let selected = match view {
            WorkbenchView::Whole => None,
            WorkbenchView::Filter { query } => Some(filters.numbers_of(query)),
            WorkbenchView::ReviewRequests => Some(&causes.review_requests),
        };
        let listed: Vec<ListedItem> = candidates(facts, repository, selected.unwrap_or(&matched))
            .into_iter()
            .map(|item| ListedItem {
                attention: causes.attention_of(&item),
                item,
            })
            .collect();
        let on_workbench = |listed: &&ListedItem| {
            listed.attention.is_some()
                || any_reason(&listed.item.rows())
                || listed.item.is_reached_by(&matched)
        };
        let lines = filters.lines(&listed);
        let count = count_of(listed.iter().filter(|listed| listed.attention.is_some()));
        let review_requests = count_of(
            listed
                .iter()
                .filter(|listed| listed.item.is_reached_by(&causes.review_requests)),
        );
        let shown = listed.iter().filter(|listed| match selected {
            Some(numbers) => listed.item.is_reached_by(numbers),
            None => on_workbench(listed),
        });
        let (items, quiet) = sections(shown.cloned().collect());
        Self {
            items,
            quiet,
            count,
            review_requests,
            filters: lines,
        }
    }
}

/// `shown` in the order of a list: what Needs attention, then the rest, and apart from them the quiet
/// section, which is every Issue with nothing that Needs attention.
fn sections(shown: Vec<ListedItem>) -> (Vec<ListedItem>, Vec<ListedItem>) {
    let (mut waiting, others): (Vec<_>, Vec<_>) = shown
        .into_iter()
        .partition(|listed| listed.attention.is_some());
    let (mut quiet, mut rest): (Vec<_>, Vec<_>) = others
        .into_iter()
        .partition(|listed| matches!(listed.item, WorkbenchItem::Issue(_)));
    waiting.sort_by_key(|listed| {
        let since = listed.attention.as_ref().map(|attention| attention.since);
        Reverse((since, listed.item.updated_at_and_number().1))
    });
    rest.sort_by_key(|listed| Reverse(listed.item.updated_at_and_number()));
    quiet.sort_by_key(|listed| Reverse(listed.item.updated_at_and_number()));
    waiting.append(&mut rest);
    (waiting, quiet)
}

fn count_of<'a>(items: impl Iterator<Item = &'a ListedItem>) -> u32 {
    u32::try_from(items.count()).unwrap_or(u32::MAX)
}

/// Each item the Repository's facts could show, on the Workbench or not. A PR in `first`, the Matched PRs of
/// the view on show, comes first in its Issue's tree.
fn candidates(
    facts: WorkFacts,
    repository: &RepositorySettings,
    first: &BTreeSet<u32>,
) -> Vec<WorkbenchItem> {
    let Some(viewer) = facts.viewer else {
        return Vec::new();
    };
    let linked = link(facts.pull_requests, &facts.issues, repository, &viewer);
    let mut issues: BTreeMap<u32, (IssueFacts, Vec<TreeEntry>)> = facts
        .issues
        .into_iter()
        .map(|issue| (issue.number, (issue, Vec::new())))
        .collect();
    let mut items = Vec::new();
    for top in plant(linked) {
        if top.issues.is_empty() {
            items.push(standalone(top));
            continue;
        }
        for number in &top.issues {
            if let Some((_, entries)) = issues.get_mut(number) {
                entries.push(top.entry_under(*number));
            }
        }
    }
    items.extend(issues.into_values().map(|(issue, mut entries)| {
        entries.sort_by_key(|entry| !entry.rows().iter().any(|row| first.contains(&row.number)));
        issue_item(issue, entries, repository.conventions)
    }));
    items
}

fn any_reason(rows: &[&WorkbenchRow]) -> bool {
    rows.iter().any(|row| !row.reasons.is_empty())
}

#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::github::pull_requests::{PrState, PullRequestFacts};
    use crate::store::ActivityFacts;
    use crate::workbench::testing::{
        at, cpython, first_issue, fixing, issue, mine, outline, reasons_for, stranger, workbench,
    };

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
    fn the_window_gets_the_kind_of_each_item_and_its_times_as_text() {
        let found = workbench(vec![mine(3), fixing(4, 1)], vec![issue(1)]);
        let sent = serde_json::to_value(&found).unwrap();
        assert_eq!(sent["items"][0]["kind"], "standalone_pr");
        assert_eq!(sent["items"][0]["number"], 3);
        assert_eq!(sent["items"][0]["updated_at"], "1970-01-01T00:00:01Z");
        assert_eq!(sent["items"][0]["attention"], Value::Null);
        assert_eq!(sent["quiet"][0]["kind"], "issue");
        assert_eq!(sent["quiet"][0]["updated_at"], "1970-01-01T00:00:01Z");
        assert_eq!(sent["quiet"][0]["pull_requests"][0]["kind"], "pr");
        assert_eq!(sent["quiet"][0]["pull_requests"][0]["number"], 4);
    }

    #[test]
    fn a_merged_pr_of_the_users_keeps_no_issue_on_the_workbench() {
        let merged = PullRequestFacts {
            state: PrState::Merged,
            ..fixing(10, 1)
        };
        assert_eq!(reasons_for(&merged), []);
        assert_eq!(outline(&workbench(vec![merged], vec![issue(1)])), []);
    }

    #[test]
    fn nothing_is_the_users_before_anyone_signs_in() {
        let found = Workbench::from_facts(
            WorkFacts {
                activity: ActivityFacts::default(),
                viewer: None,
                pull_requests: vec![mine(1)],
                issues: vec![issue(1)],
                filters: BTreeMap::new(),
            },
            &cpython(),
            &WorkbenchView::Whole,
        );
        assert_eq!(found.items, []);
    }
}
