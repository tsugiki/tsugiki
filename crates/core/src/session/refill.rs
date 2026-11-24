//! The job that works out the links the store keeps again, once a release changes the rules that work them
//! out (#950). The store opens with the links it kept and answers reads from them meanwhile. The session
//! takes the job a batch of a Repository's PRs at a time, whenever no message waits, and answers what it's
//! asked between two batches. It asks GitHub nothing, so it runs while the GitHub thread waits on a request.

use std::collections::VecDeque;
use std::num::NonZeroU32;

use crate::repository::RepositoryName;
use crate::status::{Announced, Change};
use crate::store::Store;

/// How many PRs a step works out the links of, at most. The step keeps the session's thread from its next
/// message as long as it runs, and a Repository the size of CPython's would keep it for several of these.
const BATCH: NonZeroU32 = NonZeroU32::MIN.saturating_add(4999);

/// A Repository whose links are still to work out again, from the PR it has got to.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Left {
    repository: RepositoryName,
    /// The number the next batch starts from.
    from: u32,
}

/// The refill the store's links owe, if they owe one.
#[derive(Debug)]
pub(super) struct LinkRefill {
    /// The Repositories whose links are still to work out again, the next first. Nothing while
    /// `LinkRule::SET` worked out the kept links, or once a refill failed.
    left: Option<VecDeque<Left>>,
    /// How many PRs a step works out the links of, at most.
    batch: NonZeroU32,
}

impl Default for LinkRefill {
    fn default() -> Self {
        Self {
            left: None,
            batch: BATCH,
        }
    }
}

impl LinkRefill {
    /// The refill of each Repository whose links `store` kept under rules other than the set. A store
    /// that can't say owes none this launch, and the next launch asks again.
    pub(super) fn owed_by(store: &Store) -> Self {
        let stale = store.stale_links().unwrap_or_else(|problem| {
            log::warn!(
                "Tsugiki couldn't tell whether its links need working out again: {}",
                problem.message
            );
            None
        });
        let left = stale.map(|repositories| {
            let left = repositories.into_iter();
            left.map(|repository| Left {
                repository,
                from: 0,
            })
            .collect()
        });
        Self {
            left,
            ..Self::default()
        }
    }

    /// Whether the refill has a step to take.
    pub(super) fn is_due(&self) -> bool {
        self.left.is_some()
    }

    /// Has the refill take `repository` next, from the PR it has got to, as the window shows it, if its links
    /// are still to work out.
    pub(super) fn put_first(&mut self, repository: &RepositoryName) {
        let Some(left) = &mut self.left else {
            return;
        };
        let at = left
            .iter()
            .position(|next| next.repository.is_same(repository));
        if let Some(next) = at.and_then(|at| left.remove(at)) {
            left.push_front(next);
        }
    }

    /// Works out the links of the next batch of PRs again. Once a Repository has none left, it tells the
    /// window through `board`, and the screens of the Repository read them again. Once no Repository is
    /// left, it saves the names of `LinkRule::SET` as the kept links' rules. A step that fails ends the
    /// refill. Its batch keeps the links it would replace, and the names stay, for the next launch to refill
    /// every Repository. The earlier batches of its Repository keep what they saved, so the window hears of
    /// the Repository then too.
    pub(super) fn step(&mut self, board: &Announced<Change>, store: &Store) {
        let Some(left) = &mut self.left else {
            return;
        };
        if let Some(next) = left.pop_front() {
            let repository = next.repository;
            let changed = |repository| Change {
                repository,
                items: Vec::new(),
            };
            match store.refill_links(&repository, next.from, self.batch) {
                Ok(Some(from)) => {
                    left.push_front(Left { repository, from });
                    return;
                }
                Ok(None) => board.announce(&changed(repository)),
                Err(problem) => {
                    log::warn!(
                        "Tsugiki couldn't work out the links of {repository} again, and tries at the \
                         next launch: {}",
                        problem.message
                    );
                    // A batch from the first number up is the Repository's first.
                    if next.from > 0 {
                        board.announce(&changed(repository));
                    }
                    self.left = None;
                    return;
                }
            }
            if !left.is_empty() {
                return;
            }
        }
        if let Err(problem) = store.keep_link_rules() {
            log::warn!(
                "Tsugiki couldn't save the names of the link rules, and works the links out again at \
                 the next launch: {}",
                problem.message
            );
        }
        self.left = None;
    }
}

#[cfg(test)]
mod tests;
