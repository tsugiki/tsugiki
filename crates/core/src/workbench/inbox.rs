//! The Inbox's view model (#827): every open Issue of the Repository with its open PRs under it, grouped as
//! the Workbench groups them, and every open Standalone PR on its own, the latest update first. The window
//! reads it a page at a time: the items updated in the last two days, then older batches. Each page reads
//! the open PRs, the Issues they name and the page's own rows, so a cache that holds years of closed items
//! costs no more per page than a new one. Reading it adds nothing to Reach.
//!
//! Each page is read on its own. Pages read with no sync between them fit together: each item is on one of
//! them, and each page starts where the one before it ended. A sync can move an item from one page to
//! another, because an Issue's place is the latest update of the Issue and its open PRs: a PR that closes
//! moves its Issue back to where its other PRs put it, and a new PR moves it up. So when the store says it
//! changed, the window reads its pages again from the first, rather than join pages read on either side of
//! a sync. An item moves up only when it changes, and then to the time of that change, so it is on the
//! first page after the sync.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use specta::Type;
use time::{Duration, OffsetDateTime};

use super::attention::attention_of;
use super::{ListedItem, WorkbenchItem, candidates};
use crate::error::Error;
use crate::facts::WorkFacts;
use crate::facts::activity::ActivityFacts;
use crate::facts::issues::{IssueFacts, IssuePlace, IssueState};
use crate::facts::patch_ids::ReviewFacts;
use crate::facts::pull_requests::{PrLinks, PrState, PullRequestFacts};
use crate::links::fixed_numbers;
use crate::login::Login;
use crate::repository_settings::RepositorySettings;
use crate::store::{Snapshot, StoreReader};
use crate::work::Causes;

/// How far back from now the first page reaches.
const LATEST: Duration = Duration::days(2);

/// The most items an older page holds.
const OLDER_BATCH: u32 = 50;

/// Where an item stands in the Inbox's order: the latest update first, then the highest number. Issues and
/// PRs share their numbers in a Repository, so it names one item. An Issue's update is the latest of its
/// own and its PRs', as on the Workbench.
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

    fn of_issue(issue: &IssuePlace) -> Self {
        Self {
            updated_at: issue.updated_at,
            number: issue.number,
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
}

/// A page of the Inbox. It is empty only when the Inbox is, as before anyone signs in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Inbox {
    /// The latest update first.
    pub items: Vec<ListedItem>,
    /// Where the next older page starts, or nothing once the cache holds nothing older. Past that, Load 30
    /// more days reads from GitHub (#469).
    pub older: Option<InboxCursor>,
}

impl Inbox {
    /// Reads `page` of `repository`'s Inbox, the first page reaching two days back from the clock's now.
    pub fn read_now(
        reader: &StoreReader,
        repository: &RepositorySettings,
        page: InboxPage,
    ) -> Result<Self, Error> {
        Self::read(reader, repository, page, OffsetDateTime::now_utc())
    }

    /// Reads `page` of `repository`'s Inbox, the first page reaching two days back from `now`. This waits on
    /// the database and never on a sync (ADR 0018).
    pub fn read(
        reader: &StoreReader,
        repository: &RepositorySettings,
        page: InboxPage,
        now: OffsetDateTime,
    ) -> Result<Self, Error> {
        let settings = repository.clone();
        let read = reader.read_snapshot(&repository.name, move |snapshot| {
            read_page(snapshot, &settings, page, now)
        })?;
        let Some((facts, older)) = read else {
            return Ok(Self {
                items: Vec::new(),
                older: None,
            });
        };
        let causes = Causes::of(&facts, repository.conventions);
        let mut items: Vec<ListedItem> = candidates(facts, repository, &BTreeSet::new(), &causes)
            .into_iter()
            .map(|item| ListedItem {
                attention: attention_of(&causes, &item),
                item,
            })
            .collect();
        items.sort_by_key(|listed| Reverse(listed.item.updated_at_and_number()));
        Ok(Self { items, older })
    }
}

/// The facts of `page`'s items, and where the next older page starts, or nothing before anyone signs in,
/// when the Inbox is empty. A first page with nothing from the last two days gives way to the first older
/// batch, so the window never gets an empty page with more to come.
fn read_page(
    snapshot: &Snapshot<'_>,
    repository: &RepositorySettings,
    page: InboxPage,
    now: OffsetDateTime,
) -> Result<Option<(WorkFacts, Option<InboxCursor>)>, Error> {
    let Some(viewer) = snapshot.viewer()? else {
        return Ok(None);
    };
    let groups = Groups::read(snapshot, repository, viewer)?;
    let chosen = match page {
        InboxPage::Latest => {
            let latest = groups.latest(snapshot, whole_second(now - LATEST))?;
            match latest.older {
                Some(after) if latest.members.is_empty() => groups.older(snapshot, after)?,
                _ => latest,
            }
        }
        InboxPage::Older { after } => groups.older(snapshot, after)?,
    };
    Ok(Some((
        groups.facts(snapshot, chosen.members)?,
        chosen.older,
    )))
}

/// `at` without its fraction of a second. The store keeps whole seconds, so a page that started within a
/// second would put an item updated in that second on the first page by one comparison and on the next page
/// by another.
fn whole_second(at: OffsetDateTime) -> OffsetDateTime {
    at - Duration::nanoseconds(at.nanosecond().into())
}

/// An item of the Inbox before its rows are read, with the numbers of the PRs it shows.
#[derive(Debug, Clone)]
enum Member {
    /// An open Issue, with no PRs when no open PR names it.
    Issue {
        number: u32,
        prs: Vec<u32>,
    },
    StandalonePr {
        prs: Vec<u32>,
    },
}

/// The items the open PRs make, each at its place in the order: each open Issue they fix with its open PRs,
/// and each Standalone PR. An Issue that closed leaves with its PRs, as it does from the Workbench. Only
/// what places the items is read here, and a page then reads its own items whole.
struct Groups {
    viewer: Login,
    by_place: BTreeMap<InboxCursor, Member>,
    /// The Issues the open PRs name, whatever their state. The pages read the other open Issues.
    named: BTreeSet<u32>,
}

impl Groups {
    /// Groups the open PRs as the Workbench does, under the Issues they fix.
    fn read(
        snapshot: &Snapshot<'_>,
        repository: &RepositorySettings,
        viewer: Login,
    ) -> Result<Self, Error> {
        let open_prs: Vec<PullRequestFacts> = (snapshot.open_pull_request_links()?)
            .into_iter()
            .map(to_group)
            .collect();
        let named: BTreeSet<u32> = (open_prs.iter())
            .flat_map(|pr| fixed_numbers(pr, repository))
            .collect();
        let named_issues = snapshot.issue_places_numbered(&named)?;
        let named = named_issues.iter().map(|issue| issue.number).collect();
        let facts = WorkFacts {
            viewer: Some(viewer.clone()),
            pull_requests: open_prs,
            issues: named_issues.into_iter().map(to_place).collect(),
            filters: BTreeMap::new(),
            activity: ActivityFacts::default(),
            reviews: ReviewFacts::default(),
        };
        let items = candidates(facts, repository, &BTreeSet::new(), &Causes::default());
        let by_place = items.iter().filter_map(|item| {
            let prs = item.rows().iter().map(|row| row.number).collect();
            let member = match item {
                WorkbenchItem::Issue(issue) if issue.state == Some(IssueState::Open) => {
                    Member::Issue {
                        number: issue.number,
                        prs,
                    }
                }
                WorkbenchItem::Issue(_) => return None,
                WorkbenchItem::StandalonePr(_) | WorkbenchItem::StandaloneBackport(_) => {
                    Member::StandalonePr { prs }
                }
            };
            let (updated_at, number) = item.updated_at_and_number();
            Some((InboxCursor { updated_at, number }, member))
        });
        Ok(Self {
            by_place: by_place.collect(),
            viewer,
            named,
        })
    }

    /// The items updated at `since` or later, a whole second.
    fn latest(&self, snapshot: &Snapshot<'_>, since: OffsetDateTime) -> Result<Chosen, Error> {
        let start = InboxCursor::at(since);
        let mut members: Vec<(InboxCursor, Member)> = (self.by_place.range(start..))
            .map(|(place, member)| (*place, member.clone()))
            .collect();
        members.extend(bare(snapshot.open_issue_places_since(since, &self.named)?));
        let older_issue = snapshot.open_issue_places_after((since, 0), 1, &self.named)?;
        let has_older = self.by_place.range(..start).next().is_some() || !older_issue.is_empty();
        Ok(Chosen {
            members,
            older: has_older.then_some(start),
        })
    }

    /// The next `OLDER_BATCH` items after `after`. The open Issues no PR names come from the store a batch
    /// at a time, one more than a page holds, so an item of the groups joins this batch only if it comes
    /// before the last of those, and the one past the batch says whether more remain.
    fn older(&self, snapshot: &Snapshot<'_>, after: InboxCursor) -> Result<Chosen, Error> {
        let batch = usize::try_from(OLDER_BATCH).unwrap_or(usize::MAX);
        let fetched = snapshot.open_issue_places_after(
            (after.updated_at, after.number),
            OLDER_BATCH + 1,
            &self.named,
        )?;
        let floor = (fetched.get(batch)).map(InboxCursor::of_issue);
        let mut members: Vec<(InboxCursor, Member)> = (self.by_place.range(..after).rev())
            .take_while(|(place, _)| floor.is_none_or(|floor| **place >= floor))
            .map(|(place, member)| (*place, member.clone()))
            .collect();
        members.extend(bare(fetched));
        members.sort_by_key(|(place, _)| Reverse(*place));
        let more = members.len() > batch;
        members.truncate(batch);
        let older = more
            .then(|| members.last().map(|(place, _)| *place))
            .flatten();
        Ok(Chosen { members, older })
    }

    /// The whole facts of `members` alone, with their activity and their Reviews.
    fn facts(
        &self,
        snapshot: &Snapshot<'_>,
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
        let pull_requests = snapshot.pull_requests_numbered(&prs)?;
        let numbers: BTreeSet<u32> = issues.iter().chain(&prs).copied().collect();
        Ok(WorkFacts {
            activity: snapshot.activity_of(Some(&self.viewer), &numbers)?,
            reviews: snapshot.reviews_of(&pull_requests)?,
            viewer: Some(self.viewer.clone()),
            issues: snapshot.issues_numbered(&issues)?,
            pull_requests,
            filters: BTreeMap::new(),
        })
    }
}

/// The open Issues of `issues`, each on its own, which the store read besides those the open PRs name.
fn bare(issues: Vec<IssuePlace>) -> impl Iterator<Item = (InboxCursor, Member)> {
    issues.into_iter().map(|issue| {
        let place = InboxCursor::of_issue(&issue);
        let (number, prs) = (issue.number, Vec::new());
        (place, Member::Issue { number, prs })
    })
}

/// `links` as the facts the grouping reads, to place and group the open PRs with no cause to judge them
/// by. What a `PrLinks` leaves out stays empty, so these facts go no further than `Groups::read`.
fn to_group(links: PrLinks) -> PullRequestFacts {
    PullRequestFacts {
        number: links.number,
        title: links.title,
        state: PrState::Open,
        is_draft: false,
        author: links.author,
        target_branch: String::new(),
        ci: None,
        updated_at: links.updated_at,
        review_requests: Vec::new(),
        latest_reviews: Vec::new(),
        assignees: Vec::new(),
        labels: Vec::new(),
        closing_issues: links.closing_issues,
        merge_commit: links.merge_commit,
        base: None,
        head: None,
        off_default: links.off_default,
    }
}

/// `place` as the facts the grouping reads, as `to_group` makes a PR's.
fn to_place(place: IssuePlace) -> IssueFacts {
    IssueFacts {
        number: place.number,
        title: String::new(),
        author: None,
        assignees: Vec::new(),
        labels: Vec::new(),
        updated_at: place.updated_at,
        state: place.state,
    }
}

/// The items of a page, and where the next older page starts.
struct Chosen {
    members: Vec<(InboxCursor, Member)>,
    older: Option<InboxCursor>,
}

#[cfg(test)]
mod tests;
