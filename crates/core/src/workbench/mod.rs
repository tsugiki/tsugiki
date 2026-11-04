//! The Workbench's view model (ADR 0008), worked out from stored facts when the window asks (ADR 0010). Each
//! item is an Issue with the PRs that fix it and their Backports, or a PR outside any Issue (ADR 0011).

mod filters;

pub use filters::FilterLine;

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use crate::conventions::Conventions;
use crate::error::Error;
use crate::filters::FilterQuery;
use crate::github::Login;
use crate::github::issues::IssueFacts;
use crate::github::pull_requests::{CiState, PrState, PullRequestFacts, ReviewState};
use crate::links::{BackportKind, BackportLink, MergeCommits, backport_of, fixed_numbers};
use crate::repository::RepositorySettings;
use crate::store::{StoreReader, WorkFacts};
use filters::FilterFacts;

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
    pub state: PrState,
    pub is_draft: bool,
    pub target_branch: String,
    pub ci: Option<CiState>,
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub updated_at: OffsetDateTime,
    /// Empty for a PR that another PR put on the Workbench, and for one no longer open.
    pub reasons: Vec<WorkReason>,
}

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
    fn updated_at_and_number(&self) -> (OffsetDateTime, u32) {
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

/// A Repository's Workbench or one of its Filters, with what the Sidebar shows beside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Workbench {
    /// The selected items, the latest update first.
    pub items: Vec<WorkbenchItem>,
    /// How many items the Workbench holds, until Needs attention exists to count instead.
    pub count: u32,
    pub filters: Vec<FilterLine>,
}

impl Workbench {
    /// Reads the Workbench of `repository`, or the Filter `selected`. This waits on the database and never on
    /// a sync (ADR 0018).
    pub fn read(
        reader: &StoreReader,
        repository: &RepositorySettings,
        selected: Option<&FilterQuery>,
    ) -> Result<Self, Error> {
        let facts = reader.work_facts(&repository.name)?;
        Ok(Self::from_facts(facts, repository, selected))
    }

    /// No viewer means nobody has signed in yet, so no PR is the user's.
    fn from_facts(
        mut facts: WorkFacts,
        repository: &RepositorySettings,
        selected: Option<&FilterQuery>,
    ) -> Self {
        let filters = FilterFacts::new(repository, std::mem::take(&mut facts.filters));
        let matched = filters.matched();
        let selected = selected.map(|query| filters.numbers_of(query));
        let candidates = candidates(facts, repository, selected.unwrap_or(&matched));
        let on_workbench =
            |item: &&WorkbenchItem| any_reason(&item.rows()) || item.is_reached_by(&matched);
        let lines = filters.lines(&candidates);
        let count = count_of(candidates.iter().filter(on_workbench));
        let mut items: Vec<WorkbenchItem> = candidates
            .iter()
            .filter(|item| match selected {
                Some(numbers) => item.is_reached_by(numbers),
                None => on_workbench(item),
            })
            .cloned()
            .collect();
        items.sort_by_key(|item| Reverse(item.updated_at_and_number()));
        Self {
            items,
            count,
            filters: lines,
        }
    }
}

fn count_of<'a>(items: impl Iterator<Item = &'a WorkbenchItem>) -> u32 {
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

/// A PR with what the link rules say about it.
struct Linked {
    row: WorkbenchRow,
    /// The Issues it fixes. Only a number the sync fetched as an Issue counts, so a PR's number or a typo
    /// doesn't.
    issues: BTreeSet<u32>,
    backport: Option<BackportLink>,
}

fn link(
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

/// A Backport beneath its Origin PR.
struct Beneath {
    row: BackportRow,
    issues: BTreeSet<u32>,
}

/// The two shapes of a tree's top line.
enum Shape {
    Pr {
        beneath: Vec<Beneath>,
    },
    /// A Backport with no Origin PR to sit under.
    Backport(BackportKind),
}

/// A line that sits directly under an Issue or on its own.
struct Top {
    row: WorkbenchRow,
    shape: Shape,
    /// Each Issue it shows under: its own, and those of the Backports beneath it (ADR 0011).
    issues: BTreeSet<u32>,
}

impl Top {
    /// A Backport shows under an Issue it fixes. One that fixes none follows its Origin PR.
    fn entry_under(&self, issue: u32) -> TreeEntry {
        let row = self.row.clone();
        match &self.shape {
            Shape::Backport(backport) => TreeEntry::Backport(BackportRow {
                row,
                backport: *backport,
            }),
            Shape::Pr { beneath } => TreeEntry::Pr(PrTree {
                row,
                backports: beneath
                    .iter()
                    .filter(|below| below.issues.is_empty() || below.issues.contains(&issue))
                    .map(|below| below.row.clone())
                    .collect(),
            }),
        }
    }
}

/// Puts each Backport beneath its Origin PR when Tsugiki has that PR and it isn't a Backport too, which keeps
/// a tree two levels deep. The order of `linked` stays within each level.
fn plant(linked: Vec<Linked>) -> Vec<Top> {
    let origins: BTreeSet<u32> = linked
        .iter()
        .filter(|pr| pr.backport.is_none())
        .map(|pr| pr.row.number)
        .collect();
    let mut beneath: BTreeMap<u32, Vec<Beneath>> = BTreeMap::new();
    let mut tops = Vec::new();
    for pr in linked {
        match pr.backport {
            Some(link) if origins.contains(&link.origin) => {
                beneath.entry(link.origin).or_default().push(Beneath {
                    row: BackportRow {
                        row: pr.row,
                        backport: link.kind,
                    },
                    issues: pr.issues,
                });
            }
            _ => tops.push(Top {
                row: pr.row,
                shape: pr.backport.map_or_else(
                    || Shape::Pr {
                        beneath: Vec::new(),
                    },
                    |link| Shape::Backport(link.kind),
                ),
                issues: pr.issues,
            }),
        }
    }
    for top in &mut tops {
        if let Shape::Pr { beneath: place } = &mut top.shape
            && let Some(below) = beneath.remove(&top.row.number)
        {
            top.issues
                .extend(below.iter().flat_map(|b| b.issues.iter().copied()));
            *place = below;
        }
    }
    tops
}

/// The item for a line with no Issue. Nothing beneath it fixes an Issue, or the line would show under that
/// Issue.
fn standalone(top: Top) -> WorkbenchItem {
    match top.shape {
        Shape::Backport(backport) => WorkbenchItem::StandaloneBackport(BackportRow {
            row: top.row,
            backport,
        }),
        Shape::Pr { beneath } => WorkbenchItem::StandalonePr(PrTree {
            row: top.row,
            backports: beneath.into_iter().map(|below| below.row).collect(),
        }),
    }
}

fn any_reason(rows: &[&WorkbenchRow]) -> bool {
    rows.iter().any(|row| !row.reasons.is_empty())
}

fn issue_item(
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

/// The PR's line of the list, whatever it has to do with `viewer`.
fn row(facts: PullRequestFacts, viewer: &Login) -> WorkbenchRow {
    let reasons = reasons(&facts, viewer);
    WorkbenchRow {
        number: facts.number,
        title: facts.title,
        author: facts.author,
        state: facts.state,
        is_draft: facts.is_draft,
        target_branch: facts.target_branch,
        ci: facts.ci,
        updated_at: facts.updated_at,
        reasons,
    }
}

/// A PR that has merged or closed waits on nobody, so it keeps no Issue on the Workbench.
pub(crate) fn reasons(facts: &PullRequestFacts, viewer: &Login) -> Vec<WorkReason> {
    if facts.state != PrState::Open {
        return Vec::new();
    }
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
mod tests;
