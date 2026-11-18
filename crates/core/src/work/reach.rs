//! Reach: the Issues and PRs Tsugiki keeps ready to open, their heads in the Mirror and their Conversations
//! read ahead. It comes from each Issue and PR's place on the Workbench and the links between them, never
//! from the rows the list draws, so the way `workbench` groups its items can't change what Tsugiki fetches.

use std::collections::{BTreeMap, BTreeSet};

use super::causes::Causes;
use super::origins;
use super::reasons::{WorkReason, reasons};
use super::review_state::{SharedReviews, patches_to_read, unread_sides};
use crate::facts::activity::ItemKey;
use crate::facts::filters::FilterResults;
use crate::facts::patch_ids::PatchesOf;
use crate::facts::pull_requests::{PrHead, PullRequestFacts};
use crate::facts::{CommitSha, WorkFacts};
use crate::filters::{Filter, FilterQuery, OnWorkbench};
use crate::links::{MergeCommits, backport_of, fixed_numbers};
use crate::repository_settings::RepositorySettings;

/// The Issues and PRs the searches of the `filters` on the Workbench found. A search the user has edited away
/// finds nothing, and so does one whose Filters are all switched off: their items stay out of Reach (#819).
pub(crate) fn matched(
    filters: &[Filter],
    results: &BTreeMap<FilterQuery, FilterResults>,
) -> BTreeSet<u32> {
    let on_workbench = |query: &FilterQuery| {
        (filters.iter())
            .any(|filter| filter.on_workbench == OnWorkbench::On && filter.query == *query)
    };
    let current = results.iter().filter(|(query, _)| on_workbench(query));
    current
        .flat_map(|(_, results)| results.numbers.iter().copied())
        .collect()
}

/// The Issues and PRs on the Workbench in their own right: those a Filter found, each open PR the user wrote
/// or reviewed, and each that Needs attention, as `causes` says. A Review request holds a PR only while it
/// Needs attention, so Mark Seen takes it off (#865). No viewer means nobody has signed in yet, so nothing
/// is the user's work.
pub(crate) fn in_own_right(
    facts: &WorkFacts,
    repository: &RepositorySettings,
    causes: &Causes,
) -> BTreeSet<u32> {
    let Some(viewer) = facts.viewer.as_ref() else {
        return BTreeSet::new();
    };
    let holds = |reason: &WorkReason| *reason != WorkReason::ReviewRequest;
    let reasoned = (facts.pull_requests.iter())
        .filter(|pr| reasons(pr, viewer).iter().any(holds))
        .map(|pr| pr.number);
    let mut own = matched(&repository.filters, &facts.filters);
    own.extend(reasoned);
    own.extend(causes.waiting());
    own
}

/// The links between `facts`' Issues and PRs, which Reach walks, indexed both ways: a walk costs in
/// proportion to what it reaches, once per Muted item too.
#[derive(Default)]
struct Walk {
    /// The numbers of the Issues.
    issues: BTreeSet<u32>,
    /// The Origin PR each PR's family goes by: its own number, or for a Backport its Origin PR's, when
    /// Tsugiki has that PR and it isn't a Backport too. A tree stays two levels deep, as the list draws it.
    family: BTreeMap<u32, u32>,
    /// The PRs of each family, by the Origin PR it goes by.
    members: BTreeMap<u32, Vec<u32>>,
    /// The Issues each PR fixes. Only a number the sync fetched as an Issue counts.
    fixes: BTreeMap<u32, BTreeSet<u32>>,
    /// The PRs that fix each Issue.
    fixing: BTreeMap<u32, Vec<u32>>,
}

impl Walk {
    fn of(facts: &WorkFacts, repository: &RepositorySettings) -> Self {
        let mut walk = Self {
            issues: facts.issues.iter().map(|issue| issue.number).collect(),
            ..Self::default()
        };
        let merges = MergeCommits::of(&facts.pull_requests);
        let links: Vec<_> = (facts.pull_requests.iter())
            .map(|pr| (pr, backport_of(pr, repository.conventions, &merges)))
            .collect();
        let origins = origins(links.iter().map(|(pr, link)| (pr.number, link.as_ref())));
        for (pr, link) in links {
            let family = (link.map(|link| link.origin))
                .filter(|origin| origins.contains(origin))
                .unwrap_or(pr.number);
            let fixed = fixed_numbers(pr, repository);
            let fixes: BTreeSet<u32> = fixed.intersection(&walk.issues).copied().collect();
            for issue in &fixes {
                walk.fixing.entry(*issue).or_default().push(pr.number);
            }
            walk.members.entry(family).or_default().push(pr.number);
            walk.family.extend([(pr.number, family)]);
            walk.fixes.extend([(pr.number, fixes)]);
        }
        walk
    }

    /// The PRs of the families of the PRs among `numbers`.
    fn in_families_of<'a>(&self, numbers: impl IntoIterator<Item = &'a u32>) -> BTreeSet<u32> {
        let families: BTreeSet<u32> = (numbers.into_iter())
            .filter_map(|number| self.family.get(number).copied())
            .collect();
        let members = families
            .iter()
            .filter_map(|family| self.members.get(family));
        members.flatten().copied().collect()
    }

    /// The numbers of the Issues and PRs the walk reaches from `starts`, three steps deep and no deeper,
    /// those of `cut` taken out after the first:
    ///
    /// 1. each of `starts`, with the Origin PR and the Backports of such a PR, and each Issue one of those
    ///    fixes,
    /// 2. each PR that fixes a reached Issue, open, merged or closed,
    /// 3. each Backport whose Origin PR is reached, and the Origin PR of each reached Backport.
    ///
    /// A PR that fixes a reached Issue and a second one brings in neither the second Issue nor its PRs. A
    /// walk that ran until it found nothing new would chain trees through each Issue they share, and pull in
    /// much of a Repository the size of CPython. Each step takes each number on its own, so the walks from
    /// two sets joined reach what the walk from their union does.
    fn from(&self, starts: &BTreeSet<u32>, cut: &BTreeSet<u32>) -> BTreeSet<u32> {
        let mut reached: BTreeSet<u32> = self.issues.intersection(starts).copied().collect();
        for pr in self.in_families_of(starts) {
            reached.extend([pr]);
            reached.extend(self.fixes.get(&pr).into_iter().flatten());
        }
        reached.retain(|number| !cut.contains(number));
        let fixing = reached.iter().filter_map(|issue| self.fixing.get(issue));
        let fixing: Vec<u32> = fixing.flatten().copied().collect();
        reached.extend(fixing);
        let in_families = self.in_families_of(&reached);
        reached.extend(in_families);
        reached
    }

    /// The tree of the item `key` names: what the walk reaches from it alone, which Mute takes out. A PR
    /// heads an item only while it heads its family and its tree holds no Issue, as the list draws it: the
    /// key of one that has since come to sit under an Issue or an Origin PR names no tree.
    fn tree(&self, key: ItemKey) -> BTreeSet<u32> {
        let number = key.number();
        let tree = self.from(&BTreeSet::from([number]), &BTreeSet::new());
        let heads = |tree: &BTreeSet<u32>| {
            self.family.get(&number) == Some(&number) && tree.is_disjoint(&self.issues)
        };
        match key {
            ItemKey::Issue { .. } => tree,
            ItemKey::PullRequest { .. } if heads(&tree) => tree,
            ItemKey::PullRequest { .. } => BTreeSet::new(),
        }
    }
}

/// The two sets Reach joins (#918).
#[derive(Debug)]
struct Reach {
    /// The walk from the Issues and PRs on the Workbench in their own right and those carrying a Review
    /// request, with each Muted tree cut out after its first step. A PR in a Muted tree comes back when the
    /// walk reaches it otherwise: through a second Issue it fixes, or a Backport family it shares.
    without_muted: BTreeSet<u32>,
    /// The tree of each Muted item where a Review request or an assignment to the user stands, acknowledged
    /// or not. The Filters and the Review requests view list those, and the Workbench too once the ask
    /// started after the mute, so the user will open them.
    muted_asking: BTreeSet<u32>,
}

impl Reach {
    fn of(facts: &WorkFacts, repository: &RepositorySettings) -> Self {
        let causes = Causes::of(facts, repository.conventions);
        let walk = Walk::of(facts, repository);
        let mut starts = in_own_right(facts, repository, &causes);
        // The Review requests view lists a PR the user marked seen, which has left the Workbench.
        starts.extend(&causes.review_requests);
        let asked = causes.asked();
        let mut muted = BTreeSet::new();
        let mut muted_asking = BTreeSet::new();
        for key in causes.muted() {
            let tree = walk.tree(key);
            if !tree.is_disjoint(&asked) {
                muted_asking.extend(&tree);
            }
            muted.extend(tree);
        }
        Self {
            without_muted: walk.from(&starts, &muted),
            muted_asking,
        }
    }
}

/// The numbers of the Issues and PRs Tsugiki reaches in `facts`, the two sets of `Reach` joined. That holds
/// every PR the whole Workbench shows, those in Issues' trees too. An Archived item stays in it, since its
/// Stale review check runs only on PRs in Reach, and a Stale review brings the item back.
fn reach(facts: &WorkFacts, repository: &RepositorySettings) -> BTreeSet<u32> {
    let Reach {
        mut without_muted,
        muted_asking,
    } = Reach::of(facts, repository);
    without_muted.extend(muted_asking);
    without_muted
}

/// The Mirror's work in a Repository, each part the whole set, worked out from one Reach.
#[derive(Debug, PartialEq, Eq)]
pub struct MirrorWork {
    /// The heads the Mirror fetches: those of the PRs in Reach, in the order of their numbers. A PR stored
    /// before Tsugiki read heads has none to give.
    pub heads: Vec<PrHead>,
    /// The commits whose per-file patch-ids the Stale review check still needs: the reviewed commit and the
    /// head of each PR whose head moved on from the user's Review, apart from a Bot backport, which shares
    /// its Origin PR's review state. A merged or closed PR's check reads only while its head is among
    /// `heads`. It runs once on the final head, and its verdict freezes with the PR.
    pub patches: Vec<PatchesOf>,
}

impl MirrorWork {
    /// The Mirror's work in the Repository of `repository`, whose stored facts are `facts`.
    pub fn of(facts: &WorkFacts, repository: &RepositorySettings) -> Self {
        Self::in_reach(facts, repository, &reach(facts, repository))
    }

    /// The Mirror's work of the PRs `reached` under `repository`.
    fn in_reach(
        facts: &WorkFacts,
        repository: &RepositorySettings,
        reached: &BTreeSet<u32>,
    ) -> Self {
        let shared = SharedReviews::of(&facts.pull_requests, repository.conventions);
        let unread = unread_sides(facts, &shared);
        let heads = reached_heads(facts, reached);
        let patches = patches_to_read(unread, &heads);
        Self { heads, patches }
    }
}

/// The stored facts and the settings of a Repository, which Reach is worked out from.
#[derive(Clone, Copy)]
pub(crate) struct ReachFrom<'a> {
    pub(crate) facts: &'a WorkFacts,
    pub(crate) settings: &'a RepositorySettings,
}

/// The Mirror's work after Reach moved from `before` to `now`, and the PRs whose Conversations are owed a
/// read from now on: those `now` puts in Reach, apart from each whose Conversation `read_at` says was read at
/// its head. A Filter switched on (#819) and an Unmute (#918) move it so, and no change on GitHub would owe a
/// read of theirs.
pub(crate) fn moved(
    before: ReachFrom<'_>,
    now: ReachFrom<'_>,
    read_at: &BTreeMap<u32, CommitSha>,
) -> (MirrorWork, Vec<u32>) {
    let ReachFrom { facts, settings } = now;
    let reached_before = reach(before.facts, before.settings);
    let reached = reach(facts, settings);
    let read_at_head = |pr: &PullRequestFacts| {
        (pr.head.as_ref()).is_some_and(|head| read_at.get(&pr.number) == Some(head))
    };
    let owed = (facts.pull_requests.iter())
        .filter(|pr| reached.contains(&pr.number) && !reached_before.contains(&pr.number))
        .filter(|pr| !read_at_head(pr))
        .map(|pr| pr.number)
        .collect();
    (MirrorWork::in_reach(facts, settings, &reached), owed)
}

/// The Conversations worth reading ahead in the Repository of `repository`, whose stored facts are `facts`:
/// the PRs in Reach among `owed`, those whose Conversations the store owes a read. They're worth reading
/// before the user opens them, and a stranger's PR waits until then.
pub(crate) fn conversations_to_read(
    facts: &WorkFacts,
    owed: &BTreeSet<u32>,
    repository: &RepositorySettings,
) -> BTreeSet<u32> {
    owed.intersection(&reach(facts, repository))
        .copied()
        .collect()
}

/// The head of each PR of `reached`, in the order of their numbers.
fn reached_heads(facts: &WorkFacts, reached: &BTreeSet<u32>) -> Vec<PrHead> {
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
