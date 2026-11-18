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

/// The links between `facts`' Issues and PRs, which Reach walks.
struct Walk {
    linked: Vec<Linked>,
    /// The numbers of the Issues.
    issues: BTreeSet<u32>,
}

impl Walk {
    fn of(facts: &WorkFacts, repository: &RepositorySettings) -> Self {
        Self {
            linked: linked(facts, repository),
            issues: facts.issues.iter().map(|issue| issue.number).collect(),
        }
    }

    /// The numbers of the Issues and PRs the walk reaches from `starts`, three steps deep and no deeper:
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
    fn from(&self, starts: &BTreeSet<u32>) -> BTreeSet<u32> {
        let linked = &self.linked;
        let mut reached: BTreeSet<u32> = self.issues.intersection(starts).copied().collect();
        let families_of = |reached: &BTreeSet<u32>| -> BTreeSet<u32> {
            let members = linked.iter().filter(|pr| reached.contains(&pr.number));
            members.map(|pr| pr.family).collect()
        };
        let own_families = families_of(starts);
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

    /// The tree of the item `key` names: what the walk reaches from it alone. It is the tree Mute takes out.
    fn tree(&self, key: ItemKey) -> BTreeSet<u32> {
        self.from(&BTreeSet::from([key.number()]))
    }
}

/// The two sets Reach joins (#918).
#[derive(Debug)]
struct Reach {
    /// The walk from the Issues and PRs on the Workbench in their own right and those carrying a Review
    /// request, each Muted tree left out of them. A PR in a Muted tree that another item links to comes back
    /// through those links.
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
        let mut muted_asking = BTreeSet::new();
        for key in causes.muted() {
            let tree = walk.tree(key);
            starts.retain(|number| !tree.contains(number));
            if !tree.is_disjoint(&asked) {
                muted_asking.extend(tree);
            }
        }
        Self {
            without_muted: walk.from(&starts),
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

/// The Mirror's work under settings `now`, which replace `before`, and the PRs whose Conversations are owed
/// a read from now on: those `now` puts in Reach, apart from each whose Conversation `read_at` says was read
/// at its head. No change on GitHub would owe a read of theirs (#819).
pub(crate) fn switched(
    facts: &WorkFacts,
    read_at: &BTreeMap<u32, CommitSha>,
    now: &RepositorySettings,
    before: &RepositorySettings,
) -> (MirrorWork, Vec<u32>) {
    newly_reached(facts, read_at, now, &reach(facts, before))
}

/// The Mirror's work after Unmute of the item `key`, which `facts` hold Muted until this takes it out of
/// them, and the PRs whose Conversations are owed a read from now on, as `switched` says. Unmute puts the
/// item's tree back in Reach, which no change on GitHub would have the Mirror fetch the heads of or a refresh
/// read ahead (#918).
pub(crate) fn unmuted(
    facts: &mut WorkFacts,
    key: ItemKey,
    read_at: &BTreeMap<u32, CommitSha>,
    repository: &RepositorySettings,
) -> (MirrorWork, Vec<u32>) {
    let reached_before = reach(facts, repository);
    let _was_muted = facts.activity.muted.remove(&key);
    newly_reached(facts, read_at, repository, &reached_before)
}

/// The Mirror's work in `facts` under `repository`, and the PRs in its Reach that `reached_before` left out,
/// apart from each whose Conversation `read_at` says was read at its head.
fn newly_reached(
    facts: &WorkFacts,
    read_at: &BTreeMap<u32, CommitSha>,
    repository: &RepositorySettings,
    reached_before: &BTreeSet<u32>,
) -> (MirrorWork, Vec<u32>) {
    let reached = reach(facts, repository);
    let read_at_head = |pr: &PullRequestFacts| {
        (pr.head.as_ref()).is_some_and(|head| read_at.get(&pr.number) == Some(head))
    };
    let owed = (facts.pull_requests.iter())
        .filter(|pr| reached.contains(&pr.number) && !reached_before.contains(&pr.number))
        .filter(|pr| !read_at_head(pr))
        .map(|pr| pr.number)
        .collect();
    (MirrorWork::in_reach(facts, repository, &reached), owed)
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
