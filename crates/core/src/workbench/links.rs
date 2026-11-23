//! The Issues each PR fixes and the Origin PR each Backport names, by the link rules (ADR 0011), as the
//! grouping reads them.

use std::collections::{BTreeMap, BTreeSet};

use crate::conventions::Conventions;
use crate::facts::issues::IssueFacts;
use crate::facts::pull_requests::{PrLinks, PullRequestFacts};
use crate::links::{BackportLink, MergeCommits, backport_of, fixed_numbers};
use crate::repository_settings::RepositorySettings;

/// A PR with what the link rules say about it, and its line `row`. `K` is a Backport's kind, which a
/// grouping that only places PRs leaves as `()`.
pub(super) struct Linked<R, K> {
    pub(super) number: u32,
    pub(super) row: R,
    /// The Issues it fixes, of those the facts hold.
    pub(super) issues: BTreeSet<u32>,
    pub(super) backport: Option<BackportLink<K>>,
}

/// The links of each of `pull_requests`, worked out from their facts. Only a number that the sync fetched as
/// an Issue, one of `issues`, counts as one. The store keeps the same links, for a read that can't hold
/// every PR (#833). The Workbench holds every PR's facts, and working their links out from those costs less
/// than reading the store's (#842).
pub(super) fn links_of(
    pull_requests: &[PullRequestFacts],
    issues: &[IssueFacts],
    repository: &RepositorySettings,
) -> BTreeMap<u32, PrLinks> {
    let merges = MergeCommits::of(pull_requests);
    let known: BTreeSet<u32> = issues.iter().map(|issue| issue.number).collect();
    let linked = pull_requests.iter().map(|facts| {
        let fixed = fixed_numbers(facts, repository);
        let backport = backport_of(facts, repository.conventions, &merges);
        let links = PrLinks {
            number: facts.number,
            state: facts.state,
            updated_at: facts.updated_at,
            fixes: fixed.intersection(&known).copied().collect(),
            origin: backport.map(|link| link.origin),
        };
        (facts.number, links)
    });
    linked.collect()
}

/// Links each of `pull_requests` by what `links` says of it, with the kind of each Backport, and draws its
/// line with `row`. A PR that `links` lacks links nothing.
pub(super) fn link<R>(
    pull_requests: &[PullRequestFacts],
    links: &BTreeMap<u32, PrLinks>,
    conventions: Conventions,
    row: impl Fn(&PullRequestFacts) -> R,
) -> Vec<Linked<R, crate::links::BackportKind>> {
    pull_requests
        .iter()
        .map(|facts| {
            let found = links.get(&facts.number);
            let backport = (found.and_then(|links| links.origin)).map(|origin| BackportLink {
                origin,
                kind: crate::links::backport_kind(facts, conventions),
            });
            Linked {
                number: facts.number,
                issues: found.map(|links| links.fixes.clone()).unwrap_or_default(),
                backport,
                row: row(facts),
            }
        })
        .collect()
}

/// Links each of `links` with no row to draw beyond what places it, as the Inbox does before it reads its
/// page's rows.
pub(super) fn placed<'a>(links: impl Iterator<Item = &'a PrLinks>) -> Vec<Linked<&'a PrLinks, ()>> {
    links
        .map(|links| Linked {
            number: links.number,
            row: links,
            issues: links.fixes.clone(),
            backport: links.origin.map(|origin| BackportLink { origin, kind: () }),
        })
        .collect()
}

#[cfg(test)]
mod tests;
