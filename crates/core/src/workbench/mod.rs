//! The Workbench's view model (ADR 0008), worked out from stored facts when the window asks (ADR 0010). Each
//! item is an Issue with the PRs that fix it and their Backports, or a PR outside any Issue (ADR 0011).

mod acted;
mod attention;
mod backports;
mod filters;
pub mod inbox;
pub mod issue_view;
mod issues;
mod links;
mod mute_author;
mod person;
pub mod pr_files;
pub mod pr_view;
pub mod quick_open;
mod rows;
mod search;
mod trees;

pub use acted::{Answered, Asked, Listing, Page};
pub use attention::{Attention, Seen, SeenState};
pub use filters::FilterLine;
pub use issues::IssueItem;
pub use mute_author::MuteAuthor;
pub(crate) use mute_author::{Authored, authored_by};
pub use rows::WorkbenchRow;

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use specta::Type;
use time::OffsetDateTime;

use crate::error::Error;
use crate::facts::WorkFacts;
use crate::facts::activity::ItemKey;
use crate::facts::inbox_search::RowKind;
use crate::facts::issues::{IssueFacts, IssueState};
use crate::facts::pull_requests::{PrLinks, PrState};
use crate::filters::FilterQuery;
use crate::links::BackportKind;
use crate::login::Login;
use crate::repository_settings::RepositorySettings;
use crate::settings::ListOrder;
use crate::store::StoreReader;
use crate::work::{Causes, SeenMark, in_own_right, unpinned_in_own_right};
use attention::{attention_of, summed};
use backports::{plant, standalone};
use filters::FilterFacts;
use issues::issue_item;
use links::{link, links_of};
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

    fn rows_mut(&mut self) -> Vec<&mut WorkbenchRow> {
        match self {
            Self::Pr(tree) => tree.rows_mut(),
            Self::Backport(backport) => vec![&mut backport.row],
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

    fn rows_mut(&mut self) -> Vec<&mut WorkbenchRow> {
        let beneath = self.backports.iter_mut().map(|backport| &mut backport.row);
        std::iter::once(&mut self.row).chain(beneath).collect()
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
    /// A result a search on GitHub found that the cache lacks, in its place in the list until the user
    /// scrolls to it and a fetch fills it (#465, #466).
    Placeholder(Placeholder),
}

/// A result a search on GitHub found that the cache lacks (#465). The Inbox shows its number and whether it
/// is an Issue or a PR; as the user scrolls to it, a fill fetches what it says (#466).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Placeholder {
    pub number: u32,
    /// Whether GitHub found an Issue or a PR.
    pub item_kind: RowKind,
    /// GitHub's update time, which the row's age shows.
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub updated_at: OffsetDateTime,
}

impl WorkbenchItem {
    pub(crate) fn updated_at_and_number(&self) -> (OffsetDateTime, u32) {
        match self {
            Self::Issue(issue) => (issue.updated_at, issue.number),
            Self::StandalonePr(tree) => (tree.row.updated_at, tree.row.number),
            Self::StandaloneBackport(backport) => (backport.row.updated_at, backport.row.number),
            Self::Placeholder(placeholder) => (placeholder.updated_at, placeholder.number),
        }
    }

    fn rows(&self) -> Vec<&WorkbenchRow> {
        match self {
            Self::Issue(issue) => issue.rows(),
            Self::StandalonePr(tree) => tree.rows(),
            Self::StandaloneBackport(backport) => vec![&backport.row],
            Self::Placeholder(_) => Vec::new(),
        }
    }

    /// The key the user's state that hides the item keeps it under.
    pub(crate) fn key(&self) -> ItemKey {
        match self {
            Self::Issue(issue) => ItemKey::Issue {
                number: issue.number,
            },
            Self::StandalonePr(PrTree { row, .. })
            | Self::StandaloneBackport(BackportRow { row, .. }) => {
                ItemKey::PullRequest { number: row.number }
            }
            Self::Placeholder(placeholder) => match placeholder.item_kind {
                RowKind::Issue => ItemKey::Issue {
                    number: placeholder.number,
                },
                RowKind::PullRequest => ItemKey::PullRequest {
                    number: placeholder.number,
                },
            },
        }
    }

    /// The Mark Seen of each of its lines: the item's own, then each row's in its tree.
    fn marks_mut(&mut self) -> Vec<&mut SeenMark> {
        let rows = match self {
            Self::Issue(issue) => {
                let rows = issue.pull_requests.iter_mut().flat_map(TreeEntry::rows_mut);
                let marks = rows.map(|row| &mut row.seen.mark);
                return std::iter::once(&mut issue.seen.mark).chain(marks).collect();
            }
            Self::StandalonePr(tree) => tree.rows_mut(),
            Self::StandaloneBackport(backport) => vec![&mut backport.row],
            Self::Placeholder(_) => Vec::new(),
        };
        rows.into_iter().map(|row| &mut row.seen.mark).collect()
    }

    /// The item's part in Mark Seen, which covers its tree. A placeholder has none, as the cache holds
    /// nothing of it (#465).
    fn seen(&self) -> Option<&Seen> {
        match self {
            Self::Issue(issue) => Some(&issue.seen),
            Self::StandalonePr(PrTree { row, .. })
            | Self::StandaloneBackport(BackportRow { row, .. }) => Some(&row.seen),
            Self::Placeholder(_) => None,
        }
    }

    /// Whether the item is a Pinned issue (#488).
    pub(crate) fn is_pinned(&self) -> bool {
        matches!(self, Self::Issue(issue) if issue.pinned)
    }

    /// Whether the Issue or PR that heads the item is open. An Issue stored before Tsugiki kept states
    /// counts as open. A placeholder's state is unknown, and it isn't the user's to mute (#465).
    fn is_open(&self) -> bool {
        match self {
            Self::Issue(issue) => matches!(issue.state, Some(IssueState::Open) | None),
            Self::StandalonePr(PrTree { row, .. })
            | Self::StandaloneBackport(BackportRow { row, .. }) => row.state == PrState::Open,
            Self::Placeholder(_) => false,
        }
    }
}

/// A top-level entry of the list, with why it Needs attention, if it does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ListedItem {
    #[serde(flatten)]
    pub item: WorkbenchItem,
    pub attention: Option<Attention>,
    /// The key Archive and Mute keep the item under.
    pub key: ItemKey,
    /// Whether the user's state hides the item, which only some lists show, as `Hidden` says.
    pub hidden: Option<Hidden>,
    /// The unpin's effect on the item. The Inbox lists no Pinned issue, so it says `Stays` of each item.
    pub unpin: Unpin,
}

/// The user's own state that hides an item. A Muted item that is Archived too is Muted, which hides more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Hidden {
    /// Only the Filters' views show it (#824), the Review requests view for a PR whose Review request stood
    /// at the archive (#490), and the Archived view.
    Archived,
    /// Only the Filters' views show it, in their fold of Muted items unless a Review request or an
    /// assignment to the user stands in its tree (#824). The Review requests view shows a PR while its
    /// request stands, and the Workbench while one that started after the mute does (#492).
    Muted,
    /// A Muted author opened it (#494), whether or not the user muted it on its own too. Muted, and only
    /// Unmute author brings it back: Mute, Archive and the marks do nothing on it.
    MutedAuthor,
}

impl Hidden {
    /// The state that hides the item `key` names, whose tree holds `numbers`, as `causes` judge it, if any.
    pub(crate) fn of(causes: &Causes, key: ItemKey, numbers: &[u32]) -> Option<Self> {
        if causes.is_by_muted_author(key) {
            Some(Self::MutedAuthor)
        } else if causes.is_muted(key) {
            Some(Self::Muted)
        } else {
            causes.is_archived(key, numbers).then_some(Self::Archived)
        }
    }

    /// Whether it hides a Muted item, on its own or a Muted author's.
    pub(crate) fn is_muted(self) -> bool {
        match self {
            Self::Muted | Self::MutedAuthor => true,
            Self::Archived => false,
        }
    }
}

impl ListedItem {
    /// `item` with what `causes` judge of its tree. A Muted item Needs attention only for what
    /// `Causes::fresh_asks` finds. An unpin leaves it as it is until `unpin_judged` says otherwise.
    fn of(causes: &Causes, item: WorkbenchItem) -> Self {
        let key = item.key();
        let numbers = item.numbers();
        let hidden = Hidden::of(causes, key, &numbers);
        let attention = match hidden {
            Some(Hidden::Muted | Hidden::MutedAuthor) => summed(&causes.fresh_asks(key, &numbers)),
            Some(Hidden::Archived) | None => attention_of(causes, &item),
        };
        Self {
            attention,
            hidden,
            key,
            item,
            unpin: Unpin::Stays,
        }
    }

    fn is_archived(&self) -> bool {
        self.hidden == Some(Hidden::Archived)
    }

    /// The item with what an unpin does to it, if it is a Pinned issue (#1052): `unpinned` holds what keeps
    /// an item on the Workbench but the pins, and `held_closings` the pins that closed while pinned.
    fn unpin_judged(self, unpinned: &BTreeSet<u32>, held_closings: &BTreeSet<u32>) -> Self {
        let unpin = match (&self.item, self.hidden) {
            (WorkbenchItem::Issue(issue), None | Some(Hidden::Archived)) if issue.pinned => {
                if held_closings.contains(&issue.number) {
                    Unpin::Archives
                } else if self.hidden.is_none() && !self.item.is_reached_by(unpinned) {
                    Unpin::Leaves
                } else {
                    Unpin::Stays
                }
            }
            // A Muted pin stays as Muted as it was, which hides more than Archived.
            _ => Unpin::Stays,
        };
        Self { unpin, ..self }
    }
}

/// An unpin's effect on a listed item on the Workbench (#488), which the window reads to judge whether the
/// unpin finishes the item (#1052).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Unpin {
    /// The item stays where it is: it has no pin, something besides the pin keeps it on the Workbench, or
    /// the Workbench doesn't list it.
    Stays,
    /// Nothing but its pin keeps the Issue on the Workbench, which the unpin takes it off.
    Leaves,
    /// The Issue closed while pinned, so the unpin archives it.
    Archives,
}

/// The list the window asks for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkbenchView {
    Whole,
    Filter {
        query: FilterQuery,
    },
    ReviewRequests,
    StaleReviews,
    /// The Archived items, the last archived first (#491).
    Archived,
    /// The Pinned issues (#489).
    Pinned,
    /// The temporary Filter a clicked name opens (#472): what the person authored, open, as a typed search
    /// finds it.
    Person {
        login: Login,
    },
}

/// A Repository's Workbench or one of its views, with what the Sidebar shows beside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Workbench {
    /// The items in the list's order: by attention, what Needs attention newest first by when it started
    /// to, then the rest by last update; or by last update alone.
    pub items: Vec<ListedItem>,
    /// The items the list folds at its end behind "Show N muted", in the list's order. In a Filter's view,
    /// the Muted items its search found that nobody asks the user about (#824). Empty in every other view.
    pub muted: Vec<ListedItem>,
    /// The count of items on the Workbench that Need attention.
    pub count: u32,
    /// The count of items that hold a PR carrying a Review request.
    pub review_requests: u32,
    /// The count of PRs whose Stale review Needs attention, a stale Origin PR and its Bot backports as one.
    pub stale_reviews: u32,
    /// The count of Pinned issues on the Workbench that Need attention (#489).
    pub pinned: u32,
    pub filters: Vec<FilterLine>,
    /// The search a person Filter runs, spelled out, which its Save as Filter dialog takes (#472). Empty in
    /// another view.
    pub filter_query: Option<FilterQuery>,
}

impl Workbench {
    /// Reads the view `view` of `repository`'s Workbench, its items in `order`. This waits on the database
    /// and never on a sync (ADR 0018).
    pub(crate) fn read(
        reader: &StoreReader,
        repository: &RepositorySettings,
        view: &WorkbenchView,
        order: ListOrder,
    ) -> Result<Self, Error> {
        if let WorkbenchView::Person { login } = view {
            return Self::read_person(reader, repository, login);
        }
        let facts = reader.work_facts(&repository.name)?;
        let workbench = Self::from_facts(facts, repository, view);
        Ok(match view {
            // Archived keeps its own order, the last archived first.
            WorkbenchView::Archived => workbench,
            WorkbenchView::Whole
            | WorkbenchView::Filter { .. }
            | WorkbenchView::ReviewRequests
            | WorkbenchView::StaleReviews
            | WorkbenchView::Pinned
            | WorkbenchView::Person { .. } => workbench.ordered(order),
        })
    }

    /// The Workbench with its items in `order`.
    fn ordered(mut self, order: ListOrder) -> Self {
        match order {
            ListOrder::Attention => {}
            ListOrder::LastUpdate => {
                for list in [&mut self.items, &mut self.muted] {
                    list.sort_by_key(|listed| Reverse(listed.item.updated_at_and_number()));
                }
            }
        }
        self
    }

    /// No viewer means nobody has signed in yet, so no PR is the user's. A Person reads through
    /// `read_person`, and reads here as the Workbench, which no caller asks for.
    fn from_facts(facts: WorkFacts, repository: &RepositorySettings, view: &WorkbenchView) -> Self {
        let Judged {
            causes,
            filters,
            listed,
            selected,
            own_right,
        } = judged(facts, repository, |causes, filters| match view {
            WorkbenchView::Whole
            | WorkbenchView::Archived
            | WorkbenchView::Pinned
            | WorkbenchView::Person { .. } => None,
            WorkbenchView::Filter { query } => Some(filters.numbers_of(query).clone()),
            WorkbenchView::ReviewRequests => Some(causes.review_requests.clone()),
            WorkbenchView::StaleReviews => Some(causes.stale_reviews.clone()),
        });
        let requested = |listed: &ListedItem| listed.item.is_reached_by(&causes.review_requests);
        let asked = causes.asked();
        let buckets = buckets(&listed);
        // The Review requests view keeps a PR whose Review request stood at the archive (#490), and a Muted
        // PR while its request stands (#492), each marked as hidden.
        let in_review_requests: Vec<&ListedItem> = (listed.iter())
            .filter(|listed| match listed.hidden {
                None => true,
                Some(Hidden::Archived) => requested(listed),
                Some(Hidden::Muted | Hidden::MutedAuthor) => listed.item.is_reached_by(&asked),
            })
            .collect();
        // A Filter's view lists all its search found, as a search finds archived mail too (#824). An
        // Archived item stays in place, and so does a Muted one while a Review request or an assignment to
        // the user stands in its tree. The other Muted items go in a fold at the end.
        let (folded, in_place): (Vec<&ListedItem>, Vec<&ListedItem>) =
            listed.iter().partition(|listed| {
                listed.hidden.is_some_and(Hidden::is_muted) && !listed.item.is_reached_by(&asked)
            });
        // The Pinned issues' view lists each one the Workbench shows, and an Archived one in its place,
        // marked Archived as a Filter's view marks it, so the user finds every pin there to unpin (#489).
        let in_pinned: Vec<&ListedItem> = (listed.iter())
            .filter(|listed| listed.item.is_pinned())
            .filter(|listed| match listed.hidden {
                None | Some(Hidden::Archived) => true,
                Some(Hidden::Muted | Hidden::MutedAuthor) => listed.attention.is_some(),
            })
            .collect();
        let figures = figures(&causes, &filters, &listed, &buckets);
        let shown_of = |from: &[&ListedItem]| {
            let shown = from.iter().filter(|listed| match &selected {
                Some(numbers) => listed.item.is_reached_by(numbers),
                // The item is on the Workbench when anything in its tree is, in its own right, or when it
                // is a Muted item `on_workbench` holds.
                None => listed.hidden.is_some() || listed.item.is_reached_by(&own_right),
            });
            by_attention(shown.map(|listed| (*listed).clone()).collect())
        };
        let (items, muted) = match view {
            WorkbenchView::Filter { .. } => (shown_of(&in_place), shown_of(&folded)),
            WorkbenchView::ReviewRequests => (shown_of(&in_review_requests), Vec::new()),
            WorkbenchView::Whole | WorkbenchView::Person { .. } => {
                (shown_of(&buckets.on_workbench), Vec::new())
            }
            WorkbenchView::StaleReviews => (shown_of(&buckets.unhidden), Vec::new()),
            WorkbenchView::Archived => (last_archived_first(&listed, &causes), Vec::new()),
            WorkbenchView::Pinned => (shown_of(&in_pinned), Vec::new()),
        };
        Self {
            items,
            muted,
            count: figures.count,
            review_requests: figures.review_requests,
            stale_reviews: figures.stale_reviews,
            pinned: figures.pinned,
            filters: figures.filters,
            filter_query: None,
        }
    }
}

/// The facts judged into the Repository's items, with the Filters and the view's numbers beside them: what
/// the Workbench's figures and a view's items read.
struct Judged<'a> {
    causes: Causes,
    filters: FilterFacts<'a>,
    listed: Vec<ListedItem>,
    /// The numbers of the items the view names, or nothing for a view that judges its own membership.
    selected: Option<BTreeSet<u32>>,
    /// The items the facts put on the Workbench in their own right, which the Workbench's view lists.
    own_right: BTreeSet<u32>,
}

/// `facts` judged into the candidate items, with `selected` naming the numbers of the view on show, which
/// puts its Matched PRs first in their trees.
fn judged<'a>(
    mut facts: WorkFacts,
    repository: &'a RepositorySettings,
    selected: impl FnOnce(&Causes, &FilterFacts<'a>) -> Option<BTreeSet<u32>>,
) -> Judged<'a> {
    let causes = Causes::of(&facts, repository.conventions);
    // The Workbench's own right reads the Filters' matches, so this comes before they leave the facts.
    let own_right = in_own_right(&facts, repository, &causes);
    let unpinned = unpinned_in_own_right(&facts, repository, &causes);
    let held_closings = std::mem::take(&mut facts.activity.held_closings);
    let filters = FilterFacts::new(repository, std::mem::take(&mut facts.filters));
    let selected = selected(&causes, &filters);
    let matched = filters.matched();
    let first = selected.as_ref().unwrap_or(&matched);
    let listed = candidates_of(facts, repository, first, &causes)
        .into_iter()
        .map(|item| ListedItem::of(&causes, item).unpin_judged(&unpinned, &held_closings))
        .collect();
    Judged {
        causes,
        filters,
        listed,
        selected,
        own_right,
    }
}

/// The figures the Sidebar shows beside the Workbench, whichever view a list shows.
struct Figures {
    count: u32,
    review_requests: u32,
    stale_reviews: u32,
    pinned: u32,
    filters: Vec<FilterLine>,
}

/// The items of `listed` a view and the Sidebar's figures read: those the user's state doesn't hide, and
/// those the Workbench shows.
struct Buckets<'a> {
    unhidden: Vec<&'a ListedItem>,
    on_workbench: Vec<&'a ListedItem>,
}

/// The buckets of `listed`.
fn buckets(listed: &[ListedItem]) -> Buckets<'_> {
    Buckets {
        unhidden: (listed.iter())
            .filter(|listed| listed.hidden.is_none())
            .collect(),
        // The Workbench shows a Muted item while one that started after the mute stands, which is then all
        // it Needs attention for.
        on_workbench: (listed.iter())
            .filter(|listed| match listed.hidden {
                None => true,
                Some(Hidden::Archived) => false,
                Some(Hidden::Muted | Hidden::MutedAuthor) => listed.attention.is_some(),
            })
            .collect(),
    }
}

/// The Sidebar's figures of `listed`: the count of what Needs attention, the counts of the Reviews, and the
/// Filters' lines. A Filter's count leaves out what it found that the user put away (#824); one switched on
/// counts what Needs attention as the Workbench does, a Muted item asked about after the mute included.
fn figures(
    causes: &Causes,
    filters: &FilterFacts,
    listed: &[ListedItem],
    buckets: &Buckets<'_>,
) -> Figures {
    let on_workbench = &buckets.on_workbench;
    Figures {
        count: count_of(
            on_workbench
                .iter()
                .filter(|listed| listed.attention.is_some()),
        ),
        review_requests: count_of(
            listed
                .iter()
                .filter(|listed| listed.item.is_reached_by(&causes.review_requests)),
        ),
        stale_reviews: prs_of(&buckets.unhidden, &causes.stale_reviews),
        pinned: count_of(
            (on_workbench.iter())
                .filter(|listed| listed.item.is_pinned() && listed.attention.is_some()),
        ),
        filters: filters.lines(on_workbench, &buckets.unhidden),
    }
}

/// The Archived items of `listed`, the last archived first as `causes` say, and the higher number first of
/// two archived at once.
fn last_archived_first(listed: &[ListedItem], causes: &Causes) -> Vec<ListedItem> {
    let mut items: Vec<ListedItem> = (listed.iter())
        .filter(|listed| listed.is_archived())
        .cloned()
        .collect();
    items.sort_by_key(|listed| Reverse((causes.archived_at(listed.key), listed.key.number())));
    items
}

/// The count of the PRs in `numbers` that `listed` shows. A PR that fixes two Issues counts once.
fn prs_of(listed: &[&ListedItem], numbers: &BTreeSet<u32>) -> u32 {
    let rows = listed.iter().flat_map(|listed| listed.item.rows());
    let held: BTreeSet<u32> = (rows.map(|row| row.number))
        .filter(|number| numbers.contains(number))
        .collect();
    u32::try_from(held.len()).unwrap_or(u32::MAX)
}

/// `shown` in the list's order by attention: what Needs attention, newest first by when it started to, then
/// the rest by last update.
fn by_attention(shown: Vec<ListedItem>) -> Vec<ListedItem> {
    let (mut waiting, mut rest): (Vec<_>, Vec<_>) = shown
        .into_iter()
        .partition(|listed| listed.attention.is_some());
    waiting.sort_by_key(|listed| {
        let since = listed.attention.as_ref().map(|attention| attention.since);
        Reverse((since, listed.item.updated_at_and_number().1))
    });
    rest.sort_by_key(|listed| Reverse(listed.item.updated_at_and_number()));
    waiting.append(&mut rest);
    waiting
}

fn count_of<T>(items: impl Iterator<Item = T>) -> u32 {
    u32::try_from(items.count()).unwrap_or(u32::MAX)
}

/// Each item the Repository's facts could show, on the Workbench or not, linked by the link rules as they
/// read the facts.
fn candidates_of(
    facts: WorkFacts,
    repository: &RepositorySettings,
    first: &BTreeSet<u32>,
    causes: &Causes,
) -> Vec<WorkbenchItem> {
    let links = links_of(&facts.pull_requests, &facts.issues, repository);
    candidates((facts, &links), repository, first, causes)
}

/// Each item the facts could show, on the Workbench or not, with each PR linked as `links` says. A PR in
/// `first`, the Matched PRs of the view on show, comes first in its Issue's tree. `causes` are the facts'
/// own, and give each item and PR its part in Mark Seen.
fn candidates(
    (facts, links): (WorkFacts, &BTreeMap<u32, PrLinks>),
    repository: &RepositorySettings,
    first: &BTreeSet<u32>,
    causes: &Causes,
) -> Vec<WorkbenchItem> {
    let Some(viewer) = facts.viewer else {
        return Vec::new();
    };
    let linked = link(facts.pull_requests, links, repository.conventions, |pr| {
        row(pr, &viewer, causes)
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
        issue_item(issue, entries, repository.conventions, causes)
    }));
    // A Muted item Needs attention only for a fresh ask, so it draws no Stale review for a mark to
    // acknowledge (#908).
    for item in &mut items {
        if causes.is_muted(item.key()) {
            for mark in item.marks_mut() {
                mark.findings.clear();
            }
        }
    }
    items
}

#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
