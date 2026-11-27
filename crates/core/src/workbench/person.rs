//! The person Filter a clicked name opens (#472): what that person authored, open, as a typed search finds
//! it. It reads the same cached facts and the same answer to the same search the Inbox's search box reads,
//! in a Filter's shape: Issues, with the person's PRs as Matched PRs and Standalone PRs as rows. It lists the
//! cache's matches at once and GitHub's answer after, a placeholder where the cache lacks a result the answer
//! names. It changes nothing: no item joins the Workbench, and the Repository keeps no Filter until the user
//! saves this one.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use super::search::{Index, IssueOpen, Search, narrowed, narrowed_by, rank_of};
use super::{
    Hidden, Judged, ListedItem, Placeholder, Unpin, Workbench, WorkbenchItem, buckets,
    by_attention, figures, judged,
};
use crate::error::Error;
use crate::facts::WorkFacts;
use crate::facts::inbox_search::InboxSearch;
use crate::facts::issues::IssueState;
use crate::filters::FilterQuery;
use crate::login::Login;
use crate::repository_settings::RepositorySettings;
use crate::store::StoreReader;

impl Workbench {
    /// The person Filter for `login` of `repository` (#472). This waits on the database, not GitHub: the
    /// search reads the answer the last open left, and the window asks GitHub again as it opens it.
    pub(super) fn read_person(
        reader: &StoreReader,
        repository: &RepositorySettings,
        login: &Login,
    ) -> Result<Self, Error> {
        let query = FilterQuery::person(login);
        let repository = repository.clone();
        let name = repository.name.clone();
        reader.read_snapshot(&name, move |snapshot| {
            let facts = snapshot.work_facts()?;
            let answer = snapshot.inbox_search(query.as_str())?;
            Ok(Self::from_person_facts(
                facts,
                &repository,
                &query,
                answer.as_ref(),
            ))
        })
    }

    /// The person Filter of `query` from `facts`, narrowed by GitHub's `answer` or, until one comes, by the
    /// forms the cache reads.
    fn from_person_facts(
        facts: WorkFacts,
        repository: &RepositorySettings,
        query: &FilterQuery,
        answer: Option<&InboxSearch>,
    ) -> Self {
        let search = Search::parse(query.as_str());
        let index = Index::of(&facts, |issue| {
            if matches!(issue.state, Some(IssueState::Open) | None) {
                IssueOpen::Open
            } else {
                IssueOpen::Closed
            }
        });
        // GitHub's place for each result it listed, which ranks the list and names the rows the cache lacks.
        let positions: BTreeMap<u32, usize> = answer
            .map(|answer| {
                answer
                    .hits
                    .iter()
                    .enumerate()
                    .map(|(position, hit)| (hit.number, position))
                    .collect()
            })
            .unwrap_or_default();
        // The numbers the view narrows to: GitHub's results, or the cached rows the forms find.
        let first = if answer.is_some() {
            positions.keys().copied().collect()
        } else {
            index.matching(&search)
        };
        let Judged {
            causes,
            filters,
            listed,
            ..
        } = judged(facts, repository, move |_, _| Some(first));
        // The Sidebar's figures count the whole Repository, not the person's matches.
        let figures = {
            let buckets = buckets(&listed);
            figures(&causes, &filters, &listed, &buckets)
        };
        let mut items: Vec<(usize, ListedItem)> = (listed.into_iter())
            .filter_map(|listed| {
                let item = match answer {
                    Some(_) => {
                        narrowed_by(listed.item, &|_, number| positions.contains_key(&number))?
                    }
                    None => narrowed(listed.item, &search, &index)?,
                };
                // The place GitHub gave the first result the item holds. The cache's matches keep the
                // list's own order until GitHub answers.
                let rank = rank_of(&item, &positions).unwrap_or(usize::MAX);
                let mut judged = ListedItem::of(&causes, item);
                // The item keeps the state the whole Repository judged of it: only attention follows the
                // rows the search leaves.
                judged.hidden = listed.hidden;
                judged.unpin = listed.unpin;
                Some((rank, judged))
            })
            .collect();
        if let Some(answer) = answer {
            for (position, hit) in answer.hits.iter().enumerate() {
                if index.holds(hit.number) {
                    continue;
                }
                let item = WorkbenchItem::Placeholder(Placeholder {
                    number: hit.number,
                    item_kind: hit.kind,
                    updated_at: hit.updated_at,
                });
                items.push((
                    position,
                    ListedItem {
                        key: item.key(),
                        item,
                        attention: None,
                        hidden: None,
                        unpin: Unpin::Stays,
                    },
                ));
            }
        }
        let ordered: Vec<ListedItem> = match answer {
            Some(_) => {
                items.sort_by_key(|(rank, listed)| {
                    (*rank, Reverse(listed.item.updated_at_and_number()))
                });
                items.into_iter().map(|(_, listed)| listed).collect()
            }
            None => by_attention(items.into_iter().map(|(_, listed)| listed).collect()),
        };
        // As a Filter's view lists its finds: an Archived item stays in place, and a Muted one goes in the
        // fold at the end unless something asks the user about it.
        let asked = causes.asked();
        let (muted, items): (Vec<ListedItem>, Vec<ListedItem>) =
            ordered.into_iter().partition(|listed| {
                listed.hidden.is_some_and(Hidden::is_muted) && !listed.item.is_reached_by(&asked)
            });
        Self {
            items,
            muted,
            count: figures.count,
            review_requests: figures.review_requests,
            stale_reviews: figures.stale_reviews,
            pinned: figures.pinned,
            filters: figures.filters,
            filter_query: Some(query.clone()),
        }
    }
}

#[cfg(test)]
mod tests;
