//! The Inbox's view model (#827): every open Issue of the Repository with its open PRs under it, grouped as
//! the Workbench groups them, and every open Standalone PR on its own, the latest update first. The window
//! reads it a page at a time: the items updated in the last two days, then older batches. Each page reads
//! the open PRs, the Issues they name and the page's own rows, so a cache that holds years of closed items
//! costs no more per page than a new one. Reading it adds nothing to Reach.

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
use crate::facts::issues::{IssueFacts, IssueState};
use crate::facts::patch_ids::ReviewFacts;
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

    fn of_issue(issue: &IssueFacts) -> Self {
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
    /// The items updated in the last two days.
    Latest,
    /// The next older batch: the items that come after `after` in the Inbox's order.
    Older { after: InboxCursor },
}

/// A page of the Inbox.
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
        let (facts, older) = reader.read_snapshot(&repository.name, move |snapshot| {
            read_page(snapshot, &settings, page, now)
        })?;
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

/// The facts of `page`'s items, and where the next older page starts. An older page that holds nothing
/// while older items remain gives way to the next, so the window never gets an empty page with more to come.
fn read_page(
    snapshot: &Snapshot<'_>,
    repository: &RepositorySettings,
    mut page: InboxPage,
    now: OffsetDateTime,
) -> Result<(WorkFacts, Option<InboxCursor>), Error> {
    let groups = Groups::read(snapshot, repository)?;
    loop {
        let chosen = match page {
            InboxPage::Latest => groups.latest(snapshot, now - LATEST)?,
            InboxPage::Older { after } => groups.older(snapshot, after)?,
        };
        match (page, chosen.older) {
            (InboxPage::Older { .. }, Some(after)) if chosen.members.is_empty() => {
                page = InboxPage::Older { after };
            }
            _ => return Ok((groups.facts(snapshot, chosen.members)?, chosen.older)),
        }
    }
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
    viewer: Option<Login>,
    by_place: BTreeMap<InboxCursor, Member>,
    /// The Issues the open PRs name, whatever their state. The pages read the other open Issues.
    named: BTreeSet<u32>,
}

impl Groups {
    /// Groups the open PRs as the Workbench does, under the Issues they fix.
    fn read(snapshot: &Snapshot<'_>, repository: &RepositorySettings) -> Result<Self, Error> {
        let viewer = snapshot.viewer()?;
        let open_prs = snapshot.open_pull_request_links()?;
        let named: BTreeSet<u32> = (open_prs.iter())
            .flat_map(|pr| fixed_numbers(pr, repository))
            .collect();
        let named_issues = snapshot.issue_links_numbered(&named)?;
        let named = named_issues.iter().map(|issue| issue.number).collect();
        let facts = WorkFacts {
            viewer: viewer.clone(),
            pull_requests: open_prs,
            issues: named_issues,
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

    /// The items updated at `since` or later.
    fn latest(&self, snapshot: &Snapshot<'_>, since: OffsetDateTime) -> Result<Chosen, Error> {
        let start = InboxCursor::at(since);
        let mut members: Vec<(InboxCursor, Member)> = (self.by_place.range(start..))
            .map(|(place, member)| (*place, member.clone()))
            .collect();
        members.extend(self.bare(snapshot.open_issue_links_since(since)?));
        let older_issue = snapshot.open_issue_links_before(since, 0, 1)?;
        let has_older = self.by_place.range(..start).next().is_some() || !older_issue.is_empty();
        Ok(Chosen {
            members,
            older: has_older.then_some(start),
        })
    }

    /// The next `OLDER_BATCH` items after `after`. The open Issues no PR names come from the store a batch
    /// at a time, so an item of the groups joins this batch only if it comes before the last of those.
    fn older(&self, snapshot: &Snapshot<'_>, after: InboxCursor) -> Result<Chosen, Error> {
        let batch = usize::try_from(OLDER_BATCH).unwrap_or(usize::MAX);
        let fetched =
            snapshot.open_issue_links_before(after.updated_at, after.number, OLDER_BATCH)?;
        let floor = (fetched.last())
            .filter(|_| fetched.len() >= batch)
            .map(InboxCursor::of_issue);
        let mut members: Vec<(InboxCursor, Member)> = (self.by_place.range(..after).rev())
            .take_while(|(place, _)| floor.is_none_or(|floor| **place >= floor))
            .map(|(place, member)| (*place, member.clone()))
            .collect();
        members.extend(self.bare(fetched));
        members.sort_by_key(|(place, _)| Reverse(*place));
        let older = if members.len() > batch {
            members.truncate(batch);
            members.last().map(|(place, _)| *place)
        } else {
            floor
        };
        Ok(Chosen { members, older })
    }

    /// The open Issues of `issues` that no open PR names, each on its own.
    fn bare(&self, issues: Vec<IssueFacts>) -> impl Iterator<Item = (InboxCursor, Member)> + '_ {
        (issues.into_iter())
            .filter(|issue| !self.named.contains(&issue.number))
            .map(|issue| {
                let place = InboxCursor::of_issue(&issue);
                let (number, prs) = (issue.number, Vec::new());
                (place, Member::Issue { number, prs })
            })
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
            activity: snapshot.activity_of(self.viewer.as_ref(), &numbers)?,
            reviews: snapshot.reviews_of(&pull_requests)?,
            viewer: self.viewer.clone(),
            issues: snapshot.issues_numbered(&issues)?,
            pull_requests,
            filters: BTreeMap::new(),
        })
    }
}

/// The items of a page, and where the next older page starts.
struct Chosen {
    members: Vec<(InboxCursor, Member)>,
    older: Option<InboxCursor>,
}

#[cfg(test)]
mod tests;
