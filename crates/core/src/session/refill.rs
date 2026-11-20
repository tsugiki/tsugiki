//! The job that works out the links the store keeps again, once a release changes the rules that work them
//! out (#950). The store opens with the links it kept and answers reads from them meanwhile. The session
//! takes the job a Repository at a time, ahead of the syncs' next steps, and answers what it's asked between
//! two Repositories.

use std::collections::VecDeque;

use crate::repository::RepositoryName;
use crate::status::{Change, StatusBoard};
use crate::store::Store;

/// The refill the store's links owe, if they owe one.
#[derive(Debug, Default)]
pub(super) struct LinkRefill {
    /// The Repositories whose links are still to work out again, the next first. Nothing while the kept
    /// links are this version's, or once a refill failed.
    left: Option<VecDeque<RepositoryName>>,
}

impl LinkRefill {
    /// The refill of each Repository whose links `store` kept under another version of the rules. A store
    /// that can't say owes none this launch, and the next launch asks again.
    pub(super) fn owed_by(store: &Store) -> Self {
        let stale = store.stale_links().unwrap_or_else(|problem| {
            log::warn!(
                "Tsugiki couldn't tell whether its links need working out again: {}",
                problem.message
            );
            None
        });
        Self {
            left: stale.map(VecDeque::from),
        }
    }

    /// Whether the refill has a step to take.
    pub(super) fn is_due(&self) -> bool {
        self.left.is_some()
    }

    /// Has the refill take `repository` next, as the window opened it, if its links are still to work out.
    pub(super) fn put_first(&mut self, repository: &RepositoryName) {
        let Some(left) = &mut self.left else {
            return;
        };
        let at = left.iter().position(|name| name.is_same(repository));
        if let Some(name) = at.and_then(|at| left.remove(at)) {
            left.push_front(name);
        }
    }

    /// Works out the links of the next Repository again and tells `board`, whose screens read them again.
    /// Once none are left, it saves this version of the rules as the kept links'. A step that fails ends the
    /// refill, and leaves the kept links and their version for the next launch to refill.
    pub(super) fn step(&mut self, board: &mut StatusBoard, store: &Store) {
        let Some(left) = &mut self.left else {
            return;
        };
        if let Some(repository) = left.pop_front() {
            if let Err(problem) = store.refill_links(&repository) {
                log::warn!(
                    "Tsugiki couldn't work out the links of {repository} again, and tries at the next \
                     launch: {}",
                    problem.message
                );
                self.left = None;
                return;
            }
            board.changed(&Change {
                repository,
                items: Vec::new(),
            });
            if !left.is_empty() {
                return;
            }
        }
        if let Err(problem) = store.keep_link_rules() {
            log::warn!(
                "Tsugiki couldn't save the version of the link rules, and works the links out again at \
                 the next launch: {}",
                problem.message
            );
        }
        self.left = None;
    }
}

#[cfg(test)]
mod tests;
