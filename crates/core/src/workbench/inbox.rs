//! The Inbox's view model (#827): every open Issue of the Repository with its PRs under it, whatever their
//! state, grouped as the Workbench groups them, and every Standalone PR that has an open PR in its tree on
//! its own, the latest update first. A closed Issue that a PR opening on it brought back (#823) is listed
//! as an open one is. The window reads it a page at a time: the items updated in the last two days, then
//! older batches. Reading it adds nothing to Reach. While the Repository's "Show muted" is off, the rows
//! Muted authors wrote leave the pages, and the items stay at the places their whole trees give them (#825).
//!
//! An item's place is the latest update in it: the Issue's own or a PR's in its tree. A page walks the
//! updates down from where it starts, the PRs' and the open Issues', a step at a time. Each step reads the
//! trees of what it met through the links the store keeps (#833), and so finds each item's place, whose
//! latest update the walk meets first. A page reads the PRs and Issues of its own items and their trees, so
//! a cache that holds years of items costs no more per page than a new one.
//!
//! Each page is read on its own. Pages read with nothing changed between them fit together: each item is on
//! one of them, and each page starts where the one before it ended. Two changes can move an item from one
//! page to another:
//!
//! - A sync. A new PR moves its Issue up, and a PR that names another Issue moves to it. The store then
//!   sends `Changed`.
//! - A change to the Repository's Conventions, which picks other links from the store with no change to it
//!   (ADR 0011). No event says so: the window holds the new Conventions in the settings that Apply in the
//!   Settings dialog, or its reread of the files as it opens, answers with.
//!
//! After either, the window reads its pages again from the first, rather than join pages read on either side
//! of the change. An item a sync moves up moves to the time of the change that moved it, so it is on the
//! first page after the sync.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use specta::Type;
use time::{Duration, OffsetDateTime};

use super::attention::summed;
use super::backports::plant;
use super::links::placed;
use super::trees::Trees;
use super::{
    Hidden, ListedItem, PrTree, TreeEntry, Unpin, WorkbenchItem, WorkbenchRow, candidates,
    candidates_of,
};
use crate::conventions::Conventions;
use crate::error::Error;
use crate::facts::WorkFacts;
use crate::facts::activity::{ItemKey, ShowMuted};
use crate::facts::issues::{IssuePlace, IssueState};
use crate::facts::pull_requests::{PrLinks, PrState};
use crate::login::Login;
use crate::repository_settings::RepositorySettings;
use crate::store::{Snapshot, StoreReader, Updates};
use crate::work::{Cause, Causes};

mod search;

use search::{Search, SearchRow};

/// How far back from now the first page reaches.
const LATEST: Duration = Duration::days(2);

/// The most items an older page holds.
const OLDER_BATCH: u32 = 50;

/// The fewest updates of each kind, PRs' and open Issues', a step of the walk reads beyond twice the items
/// it still needs. A step that meets too few items for the page takes another.
const STEP: u32 = 32;

/// Where an item stands in the Inbox's order: the latest update first, then the highest number. Issues and
/// PRs share their numbers in a Repository, so it names one item. An item's update is the latest in it: an
/// Issue's own or a PR's in its tree, as on the Workbench.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
pub struct InboxCursor {
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub updated_at: OffsetDateTime,
    pub number: u32,
}

impl InboxCursor {
    /// Where an item updated at `updated_at` comes before every item updated earlier.
    fn at(updated_at: OffsetDateTime) -> Self {
        Self {
            updated_at,
            number: 0,
        }
    }
}

/// The page of the Inbox the window asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InboxPage {
    /// The items updated in the last two days, or the first older batch when none were.
    Latest,
    /// The next older batch: the items that come after `after` in the Inbox's order.
    Older { after: InboxCursor },
    /// Every item from the first down to `until`, where the pages the window shows end, as one page: those
    /// pages read again after a change, at the cost of what they hold (#822).
    Through { until: InboxCursor },
    /// Every item the cache holds, as one page: the pages the window shows read again once they reached
    /// the end.
    All,
}

/// A page of the Inbox. It is empty only when the Inbox is, as before anyone signs in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Inbox {
    /// The latest update first.
    pub items: Vec<ListedItem>,
    /// Where the next older page starts, or nothing once the cache holds nothing older. Past that, Load 30
    /// more days reads from GitHub (#469).
    pub older: Option<InboxCursor>,
    /// Whether the page lists the rows Muted authors wrote, as the Repository's "Show muted" stood when it
    /// was read (#825).
    pub show_muted: ShowMuted,
    /// Whether the search in the box asks for something the cache doesn't read, so the list holds no local
    /// rows and says it needs GitHub (#464).
    pub needs_github: bool,
}

/// A page of the Inbox the window asks for, with the forms its search box holds (#464).
#[derive(Debug, Clone, Copy)]
pub struct InboxAsk<'a> {
    pub page: InboxPage,
    /// The search box's text, empty when the box is empty.
    pub search: &'a str,
}

impl Inbox {
    /// Reads `page` of `repository`'s Inbox, the first page reaching two days back from the clock's now,
    /// with the forms the cache matches in `search` (#464).
    pub(crate) fn read_now(
        reader: &StoreReader,
        repository: &RepositorySettings,
        ask: InboxAsk<'_>,
    ) -> Result<Self, Error> {
        Self::read(reader, repository, ask, OffsetDateTime::now_utc())
    }

    /// Reads `page` of `repository`'s Inbox, the first page reaching two days back from `now`. This waits on
    /// the database and never on a sync (ADR 0018). A page with nothing left to list, as when Archive, Mute,
    /// Pin and Show muted took out all it held, gives way to the next older one, so the window never gets an
    /// empty page with more to come. A search that the cache reads lists every match it holds, whatever
    /// `page` says: the box narrows the whole cache, not the pages on show (#464).
    pub(crate) fn read(
        reader: &StoreReader,
        repository: &RepositorySettings,
        ask: InboxAsk<'_>,
        now: OffsetDateTime,
    ) -> Result<Self, Error> {
        let settings = repository.clone();
        let InboxAsk { page, search } = ask;
        let search = Search::parse(search);
        // One snapshot holds the pages it walks, so they fit together as the pages of one read do.
        let job = move |snapshot: &Snapshot<'_>| {
            let show_muted = snapshot.show_muted()?;
            if !search.is_empty() {
                return Self::matching(snapshot, &settings, &search, show_muted);
            }
            let mut page = page;
            loop {
                let Some(read) = read_page(snapshot, settings.conventions, page, now)? else {
                    return Ok(Self {
                        items: Vec::new(),
                        older: None,
                        show_muted,
                        needs_github: false,
                    });
                };
                let inbox = Self::listed(read, &settings, show_muted);
                match (inbox.items.is_empty(), inbox.older) {
                    (true, Some(after)) => page = InboxPage::Older { after },
                    (false, _) | (true, None) => return Ok(inbox),
                }
            }
        };
        reader.read_snapshot(&repository.name, job)
    }

    /// The Inbox `search` narrows the whole cache to, in its order: every row it finds with its header, and
    /// nothing else. It lists every match in one page, so a change reads them again as one.
    fn matching(
        snapshot: &Snapshot<'_>,
        repository: &RepositorySettings,
        search: &Search,
        show: ShowMuted,
    ) -> Result<Self, Error> {
        if search.needs_github() {
            return Ok(Self {
                items: Vec::new(),
                older: None,
                show_muted: show,
                needs_github: true,
            });
        }
        let facts = snapshot.work_facts()?;
        let causes = Causes::of(&facts, repository.conventions);
        let rows = Index::of(&facts, &snapshot.brought_back()?);
        let grouped = candidates_of(facts, repository, &BTreeSet::new(), &causes);
        // An item keeps the place its whole tree gives it, before Show muted and the search drop rows.
        let places: BTreeMap<ItemKey, InboxCursor> = (grouped.iter())
            .map(|item| (item.key(), item_place(item)))
            .collect();
        let mut items: Vec<(InboxCursor, ListedItem)> = (shown(grouped, show, &causes).into_iter())
            .filter_map(|listed| {
                let place = places.get(&listed.key).copied()?;
                let item = narrowed(listed.item, search, &rows)?;
                Some((place, ListedItem::of(&causes, item)))
            })
            .collect();
        items.sort_by_key(|(place, _)| Reverse(*place));
        let items = (items.into_iter()).map(|(_, listed)| listed).collect();
        Ok(Self {
            items,
            older: None,
            show_muted: show,
            needs_github: false,
        })
    }

    /// The page `read` holds, Archived and Muted items and Pinned issues left out, and the rows Muted authors
    /// wrote as `show` says.
    fn listed(read: PageRead, repository: &RepositorySettings, show: ShowMuted) -> Self {
        let causes = Causes::of(&read.facts, repository.conventions);
        let grouped = candidates(
            (read.facts, &read.links),
            repository,
            &BTreeSet::new(),
            &causes,
        );
        let mut items = shown(grouped, show, &causes);
        let place_of =
            |listed: &ListedItem| read.places.get(&listed.item.updated_at_and_number().1);
        items.sort_by_key(|listed| Reverse(place_of(listed).copied()));
        Self {
            items,
            older: read.older,
            show_muted: show,
            needs_github: false,
        }
    }
}

/// The items the Inbox shows of `grouped`: Pinned issues, Archived and Muted items out, and the rows Muted
/// authors wrote as `show` says.
fn shown(grouped: Vec<WorkbenchItem>, show: ShowMuted, causes: &Causes) -> Vec<ListedItem> {
    grouped
        .into_iter()
        .filter(|item| !item.is_pinned())
        .filter_map(|item| match show {
            ShowMuted::On => Some(ListedItem::of(causes, item)),
            ShowMuted::Off => ListedItem::without_muted_authors(causes, item),
        })
        .filter(|listed| match listed.hidden {
            None => true,
            Some(Hidden::Archived | Hidden::Muted) => false,
            Some(Hidden::MutedAuthor) => {
                let key = listed.key;
                !causes.is_muted_on_its_own(key) && !causes.is_archived(key, &listed.item.numbers())
            }
        })
        .collect()
}

/// The cached Issues and PRs a search reads, by number (#464).
struct Index {
    issues: BTreeMap<u32, SearchRow>,
    pull_requests: BTreeMap<u32, SearchRow>,
}

impl Index {
    fn of(facts: &WorkFacts, brought_back: &BTreeSet<u32>) -> Self {
        Self {
            issues: (facts.issues.iter())
                .map(|issue| (issue.number, SearchRow::issue(issue, brought_back)))
                .collect(),
            pull_requests: (facts.pull_requests.iter())
                .map(|pr| (pr.number, SearchRow::pull_request(pr)))
                .collect(),
        }
    }

    /// The row of the PR `number`, which the Inbox draws under an Issue or on its own.
    fn pull_request(&self, number: u32) -> Option<&SearchRow> {
        self.pull_requests.get(&number)
    }
}

/// `item` with only the rows `search` finds, or nothing when it finds none. An Issue's header, and a PR's
/// over the Backports beneath it, stays over the rows that match beneath it, even when it doesn't match
/// itself, so a matching PR stays in its Issue's fold (#464).
fn narrowed(item: WorkbenchItem, search: &Search, rows: &Index) -> Option<WorkbenchItem> {
    let finds = |number: u32| {
        rows.pull_request(number)
            .is_some_and(|row| search.finds(row))
    };
    match item {
        WorkbenchItem::Issue(mut issue) => {
            let header = rows
                .issues
                .get(&issue.number)
                .is_some_and(|row| search.finds(row));
            issue.pull_requests = (issue.pull_requests.into_iter())
                .filter_map(|entry| match entry {
                    TreeEntry::Pr(mut tree) => {
                        let top = finds(tree.row.number);
                        tree.backports.retain(|backport| finds(backport.row.number));
                        (top || !tree.backports.is_empty()).then_some(TreeEntry::Pr(tree))
                    }
                    TreeEntry::Backport(backport) => {
                        finds(backport.row.number).then_some(TreeEntry::Backport(backport))
                    }
                })
                .collect();
            (header || !issue.pull_requests.is_empty()).then_some(WorkbenchItem::Issue(issue))
        }
        WorkbenchItem::StandalonePr(mut tree) => {
            let top = finds(tree.row.number);
            tree.backports.retain(|backport| finds(backport.row.number));
            (top || !tree.backports.is_empty()).then_some(WorkbenchItem::StandalonePr(tree))
        }
        WorkbenchItem::StandaloneBackport(backport) => {
            finds(backport.row.number).then_some(WorkbenchItem::StandaloneBackport(backport))
        }
    }
}

/// Where `item` stands in the Inbox's order: the latest update in its tree, and its number.
fn item_place(item: &WorkbenchItem) -> InboxCursor {
    match item {
        WorkbenchItem::Issue(issue) => InboxCursor {
            updated_at: issue.updated_at,
            number: issue.number,
        },
        WorkbenchItem::StandalonePr(tree) => {
            let updated = (tree.rows().into_iter()).map(|row| row.updated_at);
            InboxCursor {
                updated_at: updated.fold(OffsetDateTime::UNIX_EPOCH, Ord::max),
                number: tree.row.number,
            }
        }
        WorkbenchItem::StandaloneBackport(backport) => InboxCursor {
            updated_at: backport.row.updated_at,
            number: backport.row.number,
        },
    }
}

impl ListedItem {
    /// `item` with each row a Muted author wrote gone, or nothing once no row is left, and what `causes`
    /// judge of the rows left. A Muted author's row stays while a Review request or an assignment to the user
    /// that started after the mute stands on it, the rule that brings a Muted item back to the Workbench
    /// (#817), and goes once Mark Seen acknowledges that ask, as the Muted item leaves the Workbench then. An
    /// ask anywhere in the tree of a Muted author's own item keeps the whole item, which the Workbench lists.
    /// A row stays while a row under it does: an Issue over its PRs, a PR over its Backports. Archive and
    /// attention are judged over the rows left, where a Muted author's row counts only for its fresh asks.
    /// The item keeps its place and its Mark Seen, which cover its whole tree.
    fn without_muted_authors(causes: &Causes, item: WorkbenchItem) -> Option<Self> {
        let key = item.key();
        let fresh = |key: ItemKey, numbers: &[u32]| causes.fresh_asks(key, numbers);
        if causes.is_by_muted_author(key) && !fresh(key, &item.numbers()).is_empty() {
            return Some(Self::of(causes, item));
        }
        let hides =
            |key: ItemKey| causes.is_by_muted_author(key) && fresh(key, &[key.number()]).is_empty();
        let item = item.without(&hides)?;
        let asked: Vec<Cause> = (item.keys().into_iter())
            .filter(|&key| causes.is_by_muted_author(key))
            .flat_map(|key| fresh(key, &[key.number()]))
            .collect();
        let others = (item.keys().into_iter())
            .filter(|&key| !causes.is_by_muted_author(key))
            .flat_map(|key| causes.on(key.number()));
        Some(Self {
            attention: summed(asked.iter().chain(others)),
            hidden: Hidden::of(causes, key, &item.numbers()),
            key,
            item,
            unpin: Unpin::Stays,
        })
    }
}

impl WorkbenchItem {
    /// The key of each row the item shows: its Issue, if it has one, then each PR.
    fn keys(&self) -> Vec<ItemKey> {
        let issue = match self {
            Self::Issue(issue) => Some(ItemKey::Issue {
                number: issue.number,
            }),
            Self::StandalonePr(_) | Self::StandaloneBackport(_) => None,
        };
        let prs = (self.rows().into_iter()).map(|row| ItemKey::PullRequest { number: row.number });
        issue.into_iter().chain(prs).collect()
    }

    fn without(self, hides: &impl Fn(ItemKey) -> bool) -> Option<Self> {
        match self {
            Self::Issue(mut issue) => {
                let entries = std::mem::take(&mut issue.pull_requests).into_iter();
                issue.pull_requests = entries.filter_map(|entry| entry.without(hides)).collect();
                let key = ItemKey::Issue {
                    number: issue.number,
                };
                (!issue.pull_requests.is_empty() || !hides(key)).then_some(Self::Issue(issue))
            }
            Self::StandalonePr(tree) => tree.without(hides).map(Self::StandalonePr),
            Self::StandaloneBackport(backport) => {
                (!backport.row.hidden_by(hides)).then_some(Self::StandaloneBackport(backport))
            }
        }
    }
}

impl TreeEntry {
    fn without(self, hides: &impl Fn(ItemKey) -> bool) -> Option<Self> {
        match self {
            Self::Pr(tree) => tree.without(hides).map(Self::Pr),
            Self::Backport(backport) => {
                (!backport.row.hidden_by(hides)).then_some(Self::Backport(backport))
            }
        }
    }
}

impl PrTree {
    fn without(mut self, hides: &impl Fn(ItemKey) -> bool) -> Option<Self> {
        self.backports
            .retain(|backport| !backport.row.hidden_by(hides));
        (!self.backports.is_empty() || !self.row.hidden_by(hides)).then_some(self)
    }
}

impl WorkbenchRow {
    /// Whether `hides` takes out this PR's row.
    fn hidden_by(&self, hides: &impl Fn(ItemKey) -> bool) -> bool {
        hides(ItemKey::PullRequest {
            number: self.number,
        })
    }
}

/// What a page reads: its items' facts and their PRs' links, each item's place by its number, and where the
/// next older page starts.
struct PageRead {
    facts: WorkFacts,
    links: BTreeMap<u32, PrLinks>,
    places: BTreeMap<u32, InboxCursor>,
    older: Option<InboxCursor>,
}

/// What `page` reads, or nothing before anyone signs in, when the Inbox is empty. A first page with nothing
/// from the last two days gives way to the first older batch, so the window never gets an empty page with
/// more to come.
fn read_page(
    snapshot: &Snapshot<'_>,
    conventions: Conventions,
    page: InboxPage,
    now: OffsetDateTime,
) -> Result<Option<PageRead>, Error> {
    let Some(viewer) = snapshot.viewer()? else {
        return Ok(None);
    };
    let mut walk = Walk::new(snapshot, conventions)?;
    let chosen = match page {
        InboxPage::Latest => {
            // The store keeps whole seconds, so a page that started within a second would put an item
            // updated in that second on this page by one comparison and on the next page by another.
            let since = now - LATEST;
            let since = since.replace_nanosecond(0).unwrap_or(since);
            let mut latest = walk.step(Updates::Since(since))?.found;
            // A step places each Issue its trees show under, and one whose own tree didn't move belongs on
            // an older page, where the walk meets its latest update.
            latest.retain(|(place, _)| place.updated_at >= since);
            let start = InboxCursor::at(since);
            if latest.is_empty() {
                walk.older(start)?
            } else {
                let has_older = !walk.below(start, 1)?.is_empty();
                Chosen {
                    members: latest,
                    older: has_older.then_some(start),
                }
            }
        }
        InboxPage::Older { after } => walk.older(after)?,
        InboxPage::Through { until } => walk.through(Some(until))?,
        InboxPage::All => walk.through(None)?,
    };
    let places = (chosen.members.iter())
        .map(|(place, _)| (place.number, *place))
        .collect();
    let facts = walk.facts(viewer, chosen.members)?;
    Ok(Some(PageRead {
        facts,
        links: walk.trees.into_links(),
        places,
        older: chosen.older,
    }))
}

/// An item of the Inbox before its rows are read, with the numbers of the PRs it shows.
#[derive(Debug, Clone)]
enum Member {
    /// An open Issue, or a closed one a PR brought back, with no PRs when none fixes it.
    Issue { number: u32, prs: Vec<u32> },
    /// A Standalone PR with its Backports, or a Standalone Backport.
    StandalonePr { prs: Vec<u32> },
}

/// The items of a page, each at its place, and where the next older page starts.
struct Chosen {
    members: Vec<(InboxCursor, Member)>,
    older: Option<InboxCursor>,
}

/// What a step of the walk found: each item its updates or their trees show, at its place, and the earliest
/// update it read, or nothing once it read the last. An item's place can fall outside what the step read, as
/// for an Issue that a met PR's tree names but whose own tree didn't move.
struct Step {
    found: Vec<(InboxCursor, Member)>,
    floor: Option<OffsetDateTime>,
}

/// A walk down the updates of one snapshot, which keeps the links and Issues each step reads for the next.
struct Walk<'s, 't> {
    snapshot: &'s Snapshot<'t>,
    trees: Trees<'s, 't>,
    issues: BTreeMap<u32, IssuePlace>,
    /// The Issues a PR that opened on them brought back (#823), which the Inbox lists closed too.
    brought_back: BTreeSet<u32>,
}

impl<'s, 't> Walk<'s, 't> {
    fn new(snapshot: &'s Snapshot<'t>, conventions: Conventions) -> Result<Self, Error> {
        Ok(Self {
            snapshot,
            trees: Trees::new(snapshot, conventions),
            issues: BTreeMap::new(),
            brought_back: snapshot.brought_back()?,
        })
    }

    /// Whether the Inbox lists the Issue `issue` places: an open one, or one a PR that opened on it brought
    /// back.
    fn lists(&self, issue: &IssuePlace) -> bool {
        issue.state == Some(IssueState::Open) || self.brought_back.contains(&issue.number)
    }

    /// The next `OLDER_BATCH` items after `after`, and where the batch after them starts, if any are left.
    fn older(&mut self, after: InboxCursor) -> Result<Chosen, Error> {
        let batch = usize::try_from(OLDER_BATCH).unwrap_or(usize::MAX);
        let mut members = self.below(after, batch + 1)?;
        let more = members.len() > batch;
        members.truncate(batch);
        let older = more.then(|| members.last().map(|(place, _)| *place));
        Ok(Chosen {
            members,
            older: older.flatten(),
        })
    }

    /// Every item from the first down to `until` and `until` itself, and `until` again as where the next
    /// batch starts, if any item is left after it. Without `until`, every item, and none left. Each next
    /// batch starts where it started before, so pages read through `until` again fit the next one. With
    /// none left down to `until`, the next batch after it, so the window never gets an empty page with more
    /// to come.
    fn through(&mut self, until: Option<InboxCursor>) -> Result<Chosen, Error> {
        let since = until.map_or(OffsetDateTime::UNIX_EPOCH, |until| until.updated_at);
        let mut members = self.step(Updates::Since(since))?.found;
        // A step places each Issue its trees show under, and one whose latest update is below `until`
        // belongs on a later page.
        members.retain(|(place, _)| until.is_none_or(|until| *place >= until));
        let Some(until) = until else {
            return Ok(Chosen {
                members,
                older: None,
            });
        };
        if members.is_empty() {
            return self.older(until);
        }
        let left = !self.below(until, 1)?.is_empty();
        Ok(Chosen {
            members,
            older: left.then_some(until),
        })
    }

    /// The items after `after`, the latest first: at least `need` of them, or all that are left.
    fn below(
        &mut self,
        after: InboxCursor,
        need: usize,
    ) -> Result<Vec<(InboxCursor, Member)>, Error> {
        let mut before = after.updated_at + Duration::SECOND;
        let mut members = Vec::new();
        loop {
            let missing = u32::try_from(need - members.len()).unwrap_or(u32::MAX);
            let at_least = missing.saturating_mul(2).saturating_add(STEP);
            let step = self.step(Updates::Before { before, at_least })?;
            // An item placed at or past `before` was on an earlier step, and one placed below the floor
            // has an update this step didn't read.
            let mut found: Vec<_> = (step.found.into_iter())
                .filter(|(place, _)| *place < after && place.updated_at < before)
                .filter(|(place, _)| step.floor.is_none_or(|floor| place.updated_at >= floor))
                .collect();
            found.sort_by_key(|(place, _)| Reverse(*place));
            members.extend(found);
            match step.floor {
                Some(floor) if members.len() < need => before = floor,
                _ => return Ok(members),
            }
        }
    }

    /// Reads the updates `updates` takes, and the trees of what they belong to, and places every item those
    /// trees show. That takes in each open Issue they name, whose place falls below every update the step
    /// read when its own tree didn't move. Each caller keeps only the items placed within what the step
    /// read, and the step that reads an item's place finds it again (#891). A step that reads `at_least`
    /// updates of a kind reads no lower than the last of them, since the next step reads on from there.
    fn step(&mut self, updates: Updates) -> Result<Step, Error> {
        let prs = self.snapshot.pull_requests_updated(updates)?;
        let issues = self.snapshot.open_issue_places(updates)?;
        let floor = match updates {
            Updates::Since(since) => Some(since),
            Updates::Before { at_least, .. } => {
                let enough = |count: usize| u32::try_from(count).unwrap_or(u32::MAX) >= at_least;
                let pr_floor = (prs.last())
                    .filter(|_| enough(prs.len()))
                    .map(|(_, at)| *at);
                let issue_floor = (issues.last()).filter(|_| enough(issues.len()));
                pr_floor.max(issue_floor.map(|issue| issue.updated_at))
            }
        };
        let read = |at: OffsetDateTime| floor.is_none_or(|floor| at >= floor);
        let met: BTreeSet<u32> = (prs.into_iter())
            .filter(|(_, at)| read(*at))
            .map(|(number, _)| number)
            .collect();
        let mut trees = self.trees.of_prs(&met)?;
        // The Issues that the lines of the met PRs show under.
        let mut named = BTreeSet::new();
        for top in plant(placed(self.trees.links_of(&trees))) {
            if top.rows().iter().any(|row| met.contains(&row.number)) {
                named.extend(top.issues);
            }
        }
        let met_issues = issues.into_iter().filter(|issue| read(issue.updated_at));
        let met_issues: Vec<IssuePlace> = met_issues.collect();
        named.extend(met_issues.iter().map(|issue| issue.number));
        self.issues
            .extend(met_issues.into_iter().map(|issue| (issue.number, issue)));
        let unread: BTreeSet<u32> = (named.iter())
            .filter(|number| !self.issues.contains_key(number))
            .copied()
            .collect();
        if !unread.is_empty() {
            let read_now = self.snapshot.issue_places_numbered(&unread)?;
            self.issues
                .extend(read_now.into_iter().map(|issue| (issue.number, issue)));
        }
        let open: BTreeSet<u32> = (named.into_iter())
            .filter(|number| (self.issues.get(number)).is_some_and(|issue| self.lists(issue)))
            .collect();
        trees.extend(self.trees.under(&open)?);
        Ok(Step {
            found: self.place(&open, &met, &trees),
            floor,
        })
    }

    /// Each of the `open` Issues, and each Standalone PR with an open PR in its tree and one of `met` among
    /// its lines, at its place. `trees` hold the whole trees of each.
    fn place(
        &self,
        open: &BTreeSet<u32>,
        met: &BTreeSet<u32>,
        trees: &BTreeSet<u32>,
    ) -> Vec<(InboxCursor, Member)> {
        let mut under: BTreeMap<u32, Vec<&PrLinks>> = BTreeMap::new();
        let mut found = Vec::new();
        for top in plant(placed(self.trees.links_of(trees))) {
            for issue in top.issues.intersection(open) {
                under
                    .entry(*issue)
                    .or_default()
                    .extend(top.rows_under(*issue));
            }
            let rows = top.rows();
            let is_open = rows.iter().any(|row| row.state == PrState::Open);
            let is_met = rows.iter().any(|row| met.contains(&row.number));
            if top.issues.is_empty() && is_met && is_open {
                let updated_at = rows.iter().map(|row| row.updated_at);
                let place = InboxCursor {
                    updated_at: updated_at.fold(OffsetDateTime::UNIX_EPOCH, Ord::max),
                    number: top.number,
                };
                let prs = rows.iter().map(|row| row.number).collect();
                found.push((place, Member::StandalonePr { prs }));
            }
        }
        for (number, issue) in open.iter().filter_map(|n| Some((*n, self.issues.get(n)?))) {
            let rows = under.remove(&number).unwrap_or_default();
            let updated_at = rows.iter().map(|row| row.updated_at);
            // The walk reads the updates of open Issues alone, so a closed one a PR brought back stands at
            // its PRs' latest update, where the walk meets it.
            let own = match issue.state {
                Some(IssueState::Open) => issue.updated_at,
                _ => OffsetDateTime::UNIX_EPOCH,
            };
            let place = InboxCursor {
                updated_at: updated_at.fold(own, Ord::max),
                number,
            };
            let prs = rows.iter().map(|row| row.number).collect();
            found.push((place, Member::Issue { number, prs }));
        }
        found
    }

    /// The whole facts of `members` alone, with their activity and their Reviews.
    fn facts(
        &self,
        viewer: Login,
        members: Vec<(InboxCursor, Member)>,
    ) -> Result<WorkFacts, Error> {
        let mut issues = BTreeSet::new();
        let mut prs = BTreeSet::new();
        for (_, member) in members {
            match member {
                Member::Issue { number, prs: tree } => {
                    issues.extend([number]);
                    prs.extend(tree);
                }
                Member::StandalonePr { prs: tree } => prs.extend(tree),
            }
        }
        self.trees.facts(viewer, &issues, &prs)
    }
}

#[cfg(test)]
mod tests;
