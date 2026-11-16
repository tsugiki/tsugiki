//! The Workbench's view model (ADR 0008), worked out from stored facts when the window asks (ADR 0010). Each
//! item is an Issue with the PRs that fix it and their Backports, or a PR outside any Issue (ADR 0011).

mod attention;
mod backports;
mod filters;
pub mod inbox;
pub mod issue_view;
mod issues;
mod links;
pub mod pr_files;
pub mod pr_view;
pub mod quick_open;
mod rows;

pub use attention::{Attention, Seen, SeenState};
pub use filters::FilterLine;
pub use issues::IssueItem;
pub use rows::WorkbenchRow;

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use specta::Type;
use time::OffsetDateTime;

use crate::error::Error;
use crate::facts::WorkFacts;
use crate::facts::issues::IssueFacts;
use crate::filters::FilterQuery;
use crate::links::BackportKind;
use crate::repository_settings::RepositorySettings;
use crate::settings::ListOrder;
use crate::store::StoreReader;
use crate::work::{Causes, in_own_right};
use attention::{attention_of, seen_of};
use backports::{plant, standalone};
use filters::FilterFacts;
use issues::issue_item;
use links::link;
use rows::row;

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

impl IssueItem {
    /// Each PR its tree holds, in the order the list shows them.
    fn rows(&self) -> Vec<&WorkbenchRow> {
        self.pull_requests
            .iter()
            .flat_map(TreeEntry::rows)
            .collect()
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
            Self::Issue(issue) => issue.rows(),
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
    StaleReviews,
}

/// A Repository's Workbench or one of its views, with what the Sidebar shows beside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Workbench {
    /// The items outside the quiet section, in the list's order: by attention, what Needs attention newest
    /// first by when it started to, then the rest by last update; or by last update alone.
    pub items: Vec<ListedItem>,
    /// The Issues with nothing that Needs attention, the latest update first.
    pub quiet: Vec<ListedItem>,
    /// The count of items on the Workbench that Need attention.
    pub count: u32,
    /// The count of items that hold a PR carrying a Review request.
    pub review_requests: u32,
    /// The count of PRs whose Stale review Needs attention, a stale Origin PR and its Bot backports as one.
    pub stale_reviews: u32,
    pub filters: Vec<FilterLine>,
}

impl Workbench {
    /// Reads the view `view` of `repository`'s Workbench, its items in `order`. This waits on the database
    /// and never on a sync (ADR 0018).
    pub fn read(
        reader: &StoreReader,
        repository: &RepositorySettings,
        view: &WorkbenchView,
        order: ListOrder,
    ) -> Result<Self, Error> {
        let facts = reader.work_facts(&repository.name)?;
        Ok(Self::from_facts(facts, repository, view).ordered(order))
    }

    /// The Workbench with its items in `order`. Its quiet section keeps its own, the latest update first.
    fn ordered(mut self, order: ListOrder) -> Self {
        match order {
            ListOrder::Attention => {}
            ListOrder::LastUpdate => self
                .items
                .sort_by_key(|listed| Reverse(listed.item.updated_at_and_number())),
        }
        self
    }

    /// No viewer means nobody has signed in yet, so no PR is the user's.
    fn from_facts(
        mut facts: WorkFacts,
        repository: &RepositorySettings,
        view: &WorkbenchView,
    ) -> Self {
        let causes = Causes::of(&facts, repository.conventions);
        let own_right = in_own_right(&facts, repository, &causes);
        let filters = FilterFacts::new(repository, std::mem::take(&mut facts.filters));
        let matched = filters.matched();
        let selected = match view {
            WorkbenchView::Whole => None,
            WorkbenchView::Filter { query } => Some(filters.numbers_of(query)),
            WorkbenchView::ReviewRequests => Some(&causes.review_requests),
            WorkbenchView::StaleReviews => Some(&causes.stale_reviews),
        };
        let listed: Vec<ListedItem> =
            candidates(facts, repository, selected.unwrap_or(&matched), &causes)
                .into_iter()
                .map(|item| ListedItem {
                    attention: attention_of(&causes, &item),
                    item,
                })
                .collect();
        let lines = filters.lines(&listed);
        let count = count_of(listed.iter().filter(|listed| listed.attention.is_some()));
        let review_requests = count_of(
            listed
                .iter()
                .filter(|listed| listed.item.is_reached_by(&causes.review_requests)),
        );
        let stale_reviews = prs_of(&listed, &causes.stale_reviews);
        let shown = listed.iter().filter(|listed| match selected {
            Some(numbers) => listed.item.is_reached_by(numbers),
            // The item is on the Workbench when anything in its tree is, in its own right.
            None => listed.item.is_reached_by(&own_right),
        });
        let (items, quiet) = sections(shown.cloned().collect());
        Self {
            items,
            quiet,
            count,
            review_requests,
            stale_reviews,
            filters: lines,
        }
    }
}

/// The count of the PRs in `numbers` that `listed` shows. A PR that fixes two Issues counts once.
fn prs_of(listed: &[ListedItem], numbers: &BTreeSet<u32>) -> u32 {
    let rows = listed.iter().flat_map(|listed| listed.item.rows());
    let held: BTreeSet<u32> = (rows.map(|row| row.number))
        .filter(|number| numbers.contains(number))
        .collect();
    u32::try_from(held.len()).unwrap_or(u32::MAX)
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
/// the view on show, comes first in its Issue's tree. `causes` are the facts' own, and give each item and PR
/// its part in Mark Seen.
fn candidates(
    facts: WorkFacts,
    repository: &RepositorySettings,
    first: &BTreeSet<u32>,
    causes: &Causes,
) -> Vec<WorkbenchItem> {
    let Some(viewer) = facts.viewer else {
        return Vec::new();
    };
    let linked = link(facts.pull_requests, &facts.issues, repository, |pr| {
        let (seen, review) = (
            seen_of(causes, vec![pr.number]),
            causes.review_of(pr.number),
        );
        row(pr, &viewer, review, seen)
    });
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
        let tree = entries
            .iter()
            .flat_map(TreeEntry::rows)
            .map(|row| row.number);
        let seen = seen_of(causes, std::iter::once(issue.number).chain(tree).collect());
        issue_item(issue, entries, repository.conventions, seen)
    }));
    items
}

#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
