//! Reach: the Issues and PRs Tsugiki keeps ready to open, their heads in the Mirror and their Conversations
//! read ahead. It comes from each Issue and PR's place on the Workbench and the links between them, never
//! from the rows the list draws, so the way `workbench` groups its items can't change what Tsugiki fetches.

use std::collections::{BTreeMap, BTreeSet};

use super::causes::Causes;
use super::origins;
use super::reasons::reasons;
use super::review_state::{SharedReviews, patches_to_read, unread_sides};
use crate::facts::WorkFacts;
use crate::facts::filters::FilterResults;
use crate::facts::patch_ids::PatchesOf;
use crate::facts::pull_requests::PrHead;
use crate::filters::{Filter, FilterQuery};
use crate::links::{MergeCommits, backport_of, fixed_numbers};
use crate::repository_settings::RepositorySettings;

/// The Issues and PRs the searches of `filters` found. A search the user has edited away finds nothing.
pub(crate) fn matched(
    filters: &[Filter],
    results: &BTreeMap<FilterQuery, FilterResults>,
) -> BTreeSet<u32> {
    let current = results
        .iter()
        .filter(|(query, _)| filters.iter().any(|filter| filter.query == **query));
    current
        .flat_map(|(_, results)| results.numbers.iter().copied())
        .collect()
}

/// The Issues and PRs on the Workbench in their own right: those a Filter found, each PR with a
/// `WorkReason`, and each that Needs attention, as `causes` says. No viewer means nobody has signed in yet,
/// so nothing is the user's work.
pub(crate) fn in_own_right(
    facts: &WorkFacts,
    repository: &RepositorySettings,
    causes: &Causes,
) -> BTreeSet<u32> {
    let Some(viewer) = facts.viewer.as_ref() else {
        return BTreeSet::new();
    };
    let reasoned = (facts.pull_requests.iter())
        .filter(|pr| !reasons(pr, viewer).is_empty())
        .map(|pr| pr.number);
    let mut own = matched(&repository.filters, &facts.filters);
    own.extend(reasoned);
    own.extend(causes.waiting());
    own
}

/// A PR with the links Reach follows.
struct Linked {
    number: u32,
    /// The Issues it fixes. Only a number the sync fetched as an Issue counts.
    fixes: BTreeSet<u32>,
    /// The Origin PR its family goes by: its own number, or for a Backport its Origin PR's, when Tsugiki
    /// has that PR and it isn't a Backport too. A tree stays two levels deep, as the list draws it.
    family: u32,
}

/// Each of `facts`' PRs with the Issues it fixes and its family.
fn linked(facts: &WorkFacts, repository: &RepositorySettings) -> Vec<Linked> {
    let issues: BTreeSet<u32> = facts.issues.iter().map(|issue| issue.number).collect();
    let merges = MergeCommits::of(&facts.pull_requests);
    let links: Vec<_> = (facts.pull_requests.iter())
        .map(|pr| (pr, backport_of(pr, repository.conventions, &merges)))
        .collect();
    let origins = origins(links.iter().map(|(pr, link)| (pr.number, link.as_ref())));
    let linked = links.into_iter().map(|(pr, link)| {
        let origin = link.map(|link| link.origin);
        Linked {
            number: pr.number,
            fixes: fixed_numbers(pr, repository)
                .intersection(&issues)
                .copied()
                .collect(),
            family: origin
                .filter(|origin| origins.contains(origin))
                .unwrap_or(pr.number),
        }
    });
    linked.collect()
}

/// The numbers of the Issues and PRs Tsugiki reaches in `facts`:
///
/// 1. each on the Workbench in its own right, with the Origin PR and the Backports of such a PR, and each
///    Issue one of those fixes,
/// 2. each PR that fixes a reached Issue, open, merged or closed,
/// 3. each Backport whose Origin PR is reached, and the Origin PR of each reached Backport.
///
/// That holds every PR the whole Workbench shows, the quiet items' and those in Issues' trees too.
pub(crate) fn reach(facts: &WorkFacts, repository: &RepositorySettings) -> BTreeSet<u32> {
    let causes = Causes::of(facts, repository.conventions);
    let own = in_own_right(facts, repository, &causes);
    let linked = linked(facts, repository);
    let issues = facts.issues.iter().map(|issue| issue.number);
    let mut reached: BTreeSet<u32> = issues.filter(|number| own.contains(number)).collect();
    let families_of = |reached: &BTreeSet<u32>| -> BTreeSet<u32> {
        let members = linked.iter().filter(|pr| reached.contains(&pr.number));
        members.map(|pr| pr.family).collect()
    };
    let own_families = families_of(&own);
    let in_own_families = linked.iter().filter(|pr| own_families.contains(&pr.family));
    for pr in in_own_families {
        reached.extend([pr.number]);
        reached.extend(&pr.fixes);
    }
    let fixing: Vec<u32> = (linked.iter())
        .filter(|pr| pr.fixes.iter().any(|issue| reached.contains(issue)))
        .map(|pr| pr.number)
        .collect();
    reached.extend(fixing);
    let families = families_of(&reached);
    let in_families = linked.iter().filter(|pr| families.contains(&pr.family));
    reached.extend(in_families.map(|pr| pr.number).collect::<Vec<_>>());
    reached
}

/// The Mirror's work in a Repository. It fetches the heads of the PRs in Reach. A PR
/// stored before Tsugiki read heads has none to give. It reads the commits whose per-file patch-ids the Stale
/// review check still needs: the reviewed commit and the head of each PR whose head moved on from the user's
/// Review, apart from a Bot backport, which shares its Origin PR's review state. A merged or closed PR's
/// check reads only while its head is among those fetched. It runs once on the final head, and its verdict
/// freezes with the PR.
pub fn mirror_work(
    facts: &WorkFacts,
    repository: &RepositorySettings,
) -> (Vec<PrHead>, Vec<PatchesOf>) {
    let shared = SharedReviews::of(&facts.pull_requests, repository.conventions);
    let unread = unread_sides(facts, &shared);
    let heads = reached_heads(facts, repository);
    let patches = patches_to_read(unread, &heads);
    (heads, patches)
}

/// The head of each PR Tsugiki reaches, in the order of their numbers.
fn reached_heads(facts: &WorkFacts, repository: &RepositorySettings) -> Vec<PrHead> {
    let reached = reach(facts, repository);
    let heads: BTreeMap<u32, PrHead> = (facts.pull_requests.iter())
        .filter(|pr| reached.contains(&pr.number))
        .filter_map(|pr| {
            let head = PrHead {
                number: pr.number,
                commit: pr.head.clone()?,
                target_branch: pr.target_branch.clone(),
            };
            Some((pr.number, head))
        })
        .collect();
    heads.into_values().collect()
}

#[cfg(test)]
mod tests;
