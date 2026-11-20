//! Picks the Mirror worker's next job from what it holds queued. The worker keeps its thread, its queue and
//! git, and runs what this module picks.
//!
//! A Lookup checkout's move goes first, as a reader may wait for it and it needs no network. Then come the
//! heads of the PRs the user opened, in a fetch of their own, the heads the rules want, the commits the
//! user's Reviews covered, and the reads of patch-ids. Each takes a moment where a clone can take minutes,
//! so they go ahead of the clones. A mirror's upkeep can wait, so it comes last. Within each kind, the
//! Repository whose turn comes first goes first.
//!
//! Nothing starts without a usable git, and short of disk clones wait. A move that would make its Lookup
//! checkout afresh is refused then too, but by the move itself, as it answers the readers who wait for it.
//! Offline, only a move and upkeep start, as neither needs the network. While fetches wait for news after
//! one failed, reads of patch-ids and clones go on.

use std::collections::BTreeSet;

use crate::repository::RepositoryName;

/// Which git may start next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    Going,
    /// The last fetch failed, so the next waits for news: heads handed over, or a new status. Clones go on.
    Stalled,
    /// GitHub can't be reached, so no git that needs it starts.
    Held,
}

/// A problem the worker found with git or the disk, which holds jobs back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// No git, or one too old for the mirrors, so nothing starts.
    Git,
    /// The disk lacks room for the next clone, so clones wait.
    DiskShort,
}

/// The Mirror worker's queue, with the problem and the pace that hold jobs back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queue {
    pub problem: Option<Problem>,
    pub pace: Pace,
    /// Whether the worker has somewhere to hand the patch-ids it reads, and a git new enough to read them.
    pub reads_patch_ids: bool,
    /// The jobs of each Repository whose mirror is there, in the order the Repositories take their turns.
    /// A Repository whose mirror isn't there keeps its jobs until its clone.
    pub repositories: Vec<Queued>,
    /// The Repository first in line for its clone.
    pub clone: Option<RepositoryName>,
    /// The Repositories whose mirror's upkeep is due, first in line first.
    pub upkeep: Vec<RepositoryName>,
}

/// The jobs waiting in one Repository whose mirror is there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queued {
    pub repository: RepositoryName,
    pub waiting: BTreeSet<Waiting>,
}

/// A kind of job that waits in a Repository whose mirror is there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Waiting {
    /// A move of its Lookup checkout.
    LookupMove,
    /// The heads of PRs the user opened, to be fetched.
    DemandedHeads,
    /// Heads to be fetched.
    Heads,
    /// Commits the user's Reviews covered, to be kept.
    Reviewed,
    /// Commits whose patch-ids are to be read.
    PatchIds,
}

/// The job the worker runs next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// Move the Repository's Lookup checkout to where the PR on screen wants it.
    MoveLookup(RepositoryName),
    /// Fetch the heads of the PRs the user opened in the Repository.
    FetchDemanded(RepositoryName),
    /// Fetch the heads that wait in the Repository.
    FetchWanted(RepositoryName),
    /// Keep the commits the user's Reviews covered in the Repository.
    KeepReviewed(RepositoryName),
    /// Read the patch-ids of one commit that waits in the Repository.
    ReadPatchIds(RepositoryName),
    /// Clone the Repository's mirror.
    Clone(RepositoryName),
    /// Run the upkeep of the Repository's mirror.
    Upkeep(RepositoryName),
}

/// The job to run next from `queue`, if one can start.
pub fn next(queue: &Queue) -> Option<Next> {
    if queue.problem == Some(Problem::Git) {
        return None;
    }
    let first = |kind: Waiting| {
        (queue.repositories.iter())
            .find(|queued| queued.waiting.contains(&kind))
            .map(|queued| queued.repository.clone())
    };
    if let Some(repository) = first(Waiting::LookupMove) {
        return Some(Next::MoveLookup(repository));
    }
    if queue.pace == Pace::Going {
        if let Some(repository) = first(Waiting::DemandedHeads) {
            return Some(Next::FetchDemanded(repository));
        }
        if let Some(repository) = first(Waiting::Heads) {
            return Some(Next::FetchWanted(repository));
        }
        if let Some(repository) = first(Waiting::Reviewed) {
            return Some(Next::KeepReviewed(repository));
        }
    }
    if queue.pace != Pace::Held {
        if queue.reads_patch_ids
            && let Some(repository) = first(Waiting::PatchIds)
        {
            return Some(Next::ReadPatchIds(repository));
        }
        if queue.problem.is_none()
            && let Some(repository) = &queue.clone
        {
            return Some(Next::Clone(repository.clone()));
        }
    }
    queue.upkeep.first().cloned().map(Next::Upkeep)
}

#[cfg(test)]
mod tests;
