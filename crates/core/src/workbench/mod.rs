//! The Workbench's view model (ADR 0008), worked out from stored facts when the window asks (ADR 0010). Each
//! item is an Issue with the PRs that fix it and their Backports, or a PR outside any Issue (ADR 0011).

mod attention;
mod backports;
mod filters;
pub mod issue_view;
mod issues;
mod links;
pub mod pr_files;
pub mod pr_view;
mod review_state;
mod rows;

pub use attention::{Attention, Seen, SeenMark, SeenState, Why};
pub use filters::FilterLine;
pub use issues::IssueItem;
pub use review_state::YourReview;
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
use crate::github::pull_requests::PrHead;
use crate::links::BackportKind;
use crate::patch_ids::PatchesOf;
use crate::repository_settings::RepositorySettings;
use crate::settings::ListOrder;
use crate::store::{LastCheck, StoreReader, WorkFacts};
use attention::Causes;
use backports::{plant, standalone};
use filters::FilterFacts;
use issues::issue_item;
use links::link;
use review_state::SharedReviews;
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
    pub filters: Vec<FilterLine>,
}

impl Workbench {
    /// What the mirror is to do for the user's work. It fetches the heads of the PRs that work reaches:
    /// each PR on the whole Workbench, the quiet items' and those in Issues' trees too. A PR stored before
    /// Tsugiki read heads has none to give. It reads the commits whose per-file patch-ids the Stale review
    /// check still needs: the reviewed commit and the head of each PR whose head moved on from the user's
    /// Review, apart from a Bot backport, which shares its Origin PR's review state. A merged or closed PR's
    /// check reads only while its head is among those fetched. It runs once on the final head, and its
    /// verdict freezes with the PR.
    pub fn mirror_work(
        facts: WorkFacts,
        repository: &RepositorySettings,
    ) -> (Vec<PrHead>, Vec<PatchesOf>) {
        let shared = SharedReviews::of(&facts.pull_requests, repository.conventions);
        let unread = review_state::unread_sides(&facts, &shared);
        let heads = Self::reached_heads(facts, repository);
        let patches = review_state::patches_to_read(unread, &heads);
        (heads, patches)
    }

    fn reached_heads(facts: WorkFacts, repository: &RepositorySettings) -> Vec<PrHead> {
        let mut heads: BTreeMap<u32, PrHead> = facts
            .pull_requests
            .iter()
            .filter_map(|pr| {
                let head = PrHead {
                    number: pr.number,
                    commit: pr.head.clone()?,
                    target_branch: pr.target_branch.clone(),
                };
                Some((pr.number, head))
            })
            .collect();
        Self::reached(facts, repository)
            .into_iter()
            .filter_map(|number| heads.remove(&number))
            .collect()
    }

    /// The two commits of each PR whose check has both sides read, for the store to keep as its last check,
    /// those it keeps already left out. Their verdict stands while a newer check waits.
    pub fn checks_to_keep(facts: &WorkFacts) -> Vec<(u32, LastCheck)> {
        review_state::checks_to_keep(facts)
    }

    /// The numbers of the Issues and PRs the user's work reaches: each on the whole Workbench, the quiet
    /// items and those in Issues' trees too.
    pub(crate) fn reached(facts: WorkFacts, repository: &RepositorySettings) -> BTreeSet<u32> {
        let whole = Self::from_facts(facts, repository, &WorkbenchView::Whole);
        whole
            .items
            .iter()
            .chain(&whole.quiet)
            .flat_map(|listed| listed.item.rows())
            .map(|row| row.number)
            .collect()
    }

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
        let filters = FilterFacts::new(repository, std::mem::take(&mut facts.filters));
        let matched = filters.matched();
        let causes = Causes::of(&facts, repository.conventions);
        let selected = match view {
            WorkbenchView::Whole => None,
            WorkbenchView::Filter { query } => Some(filters.numbers_of(query)),
            WorkbenchView::ReviewRequests => Some(&causes.review_requests),
        };
        let listed: Vec<ListedItem> =
            candidates(facts, repository, selected.unwrap_or(&matched), &causes)
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
        let (seen, review) = (causes.seen_of(vec![pr.number]), causes.review_of(pr.number));
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
        let seen = causes.seen_of(std::iter::once(issue.number).chain(tree).collect());
        issue_item(issue, entries, repository.conventions, seen)
    }));
    items
}

fn any_reason(rows: &[&WorkbenchRow]) -> bool {
    rows.iter().any(|row| !row.reasons.is_empty())
}

#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;
