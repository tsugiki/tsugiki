//! Reading each Issue's cross-references, which list the PRs of its tree (ADR 0011).

use std::collections::BTreeSet;

use super::{Batches, Leads, Link, Moved, Reading, Resumable, SEARCH_PAGE, Step, pause_until};
use crate::error::Error;
use crate::github::issues::ListedPr;
use crate::github::pull_requests::{NodeId, PageRequest, PrFetch};

pub(super) fn read_cross_references(
    link: &Link<'_>,
    reading: &mut Reading,
    fetched: &BTreeSet<u32>,
) -> Result<Moved, Error> {
    let Reading {
        issues,
        cursor,
        listed,
    } = reading;
    let Some(issue) = issues.first().copied() else {
        let frozen = link.store.frozen_pull_requests(link.repository)?;
        let frozen: BTreeSet<u32> = frozen.iter().map(|pr| pr.number).collect();
        let leads = leads_of_those_held(link, listed, fetched)?;
        let next = Resumable::Fetching(Batches {
            ids: not_yet_fetched(std::mem::take(listed), fetched, &frozen),
            why: PrFetch::Tree,
            leads,
        })
        .into();
        return Ok((Step::Moved, Some(next)));
    };
    let asked = PageRequest {
        size: SEARCH_PAGE,
        after: cursor.as_deref(),
    };
    let page = link
        .github
        .fetch_cross_references(link.repository, issue, asked)?;
    listed.extend(page.listed);
    *cursor = page.next;
    if cursor.is_none() {
        drop(issues.drain(..1));
    }
    let pause_until = pause_until(page.rate_limit);
    Ok((Step::Searched { pause_until }, None))
}

/// The leads of the PRs in `listed` that this refresh has already saved. It saves a changed PR without
/// following it, because few are in any tree, so a Backport it brought only gets its Origin PR once a tree
/// lists it.
fn leads_of_those_held(
    link: &Link<'_>,
    listed: &[ListedPr],
    fetched: &BTreeSet<u32>,
) -> Result<Leads, Error> {
    let numbers = listed.iter().map(|pr| pr.number);
    let held: BTreeSet<u32> = numbers.filter(|number| fetched.contains(number)).collect();
    let mut leads = Leads::default();
    if !held.is_empty() {
        let stored = link.store.pull_requests(link.repository)?;
        for pull_request in stored.iter().filter(|pr| held.contains(&pr.number)) {
            leads.learn_from(pull_request, link.repository);
        }
    }
    Ok(leads)
}

/// The IDs of the PRs in `listed` to fetch, each once. A PR Tsugiki keeps as merged or closed stays frozen,
/// and an open one is fetched again, because it may not be open any more (ADR 0011). The link rules read what
/// comes back when a screen asks, so a PR that only mentions an Issue stays out of its tree.
fn not_yet_fetched(
    listed: Vec<ListedPr>,
    fetched: &BTreeSet<u32>,
    frozen: &BTreeSet<u32>,
) -> Vec<NodeId> {
    let mut seen = BTreeSet::new();
    listed
        .into_iter()
        .filter(|pr| {
            !fetched.contains(&pr.number) && !frozen.contains(&pr.number) && seen.insert(pr.number)
        })
        .map(|pr| pr.id)
        .collect()
}

#[cfg(test)]
mod tests;
