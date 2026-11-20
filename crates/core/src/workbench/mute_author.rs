//! Mute author (#494): what each row offers for its author, and what muting or unmuting an author covers.
//! A Muted author's items count as Muted from the author each holds, as `work` works them out.

use std::collections::BTreeSet;

use serde::Serialize;
use specta::Type;

use super::{WorkbenchItem, candidates_of};
use crate::facts::WorkFacts;
use crate::login::Login;
use crate::repository_settings::RepositorySettings;
use crate::store::Seeing;
use crate::work::{Causes, SeenMark};

/// What Mute author offers on a row for the author of its Issue or PR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MuteAuthor {
    /// Nothing: the user opened it, or its author's account has gone.
    Off,
    /// Mute author mutes `login`.
    Mute { login: Login },
    /// `login` is a Muted author, and Unmute author unmutes them.
    Unmute { login: Login },
}

impl MuteAuthor {
    /// What Mute author offers for `author` as `causes` judge them.
    pub(crate) fn of(causes: &Causes, author: Option<&Login>) -> Self {
        match author {
            None => Self::Off,
            Some(author) if causes.viewer() == Some(author) => Self::Off,
            Some(author) => {
                let login = author.clone();
                if causes.is_muted_author(author) {
                    Self::Unmute { login }
                } else {
                    Self::Mute { login }
                }
            }
        }
    }
}

/// What muting or unmuting an author touches in a Repository's facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Authored {
    /// Mark Seen on each open item they opened, its tree included, as the window would draw it, which
    /// Mute author makes as Mute marks one item.
    pub(crate) marks: Vec<Seeing>,
    /// Each Issue and PR whose view the mute changes: each they opened, and the trees of their items.
    pub(crate) numbers: Vec<u32>,
}

/// What muting or unmuting `author` touches in `facts`, linked under `repository`'s Conventions.
pub(crate) fn authored_by(
    facts: WorkFacts,
    repository: &RepositorySettings,
    author: &Login,
) -> Authored {
    let by_them = |by: Option<&Login>| by == Some(author);
    let issues = (facts.issues.iter())
        .filter(|issue| by_them(issue.author.as_ref()))
        .map(|issue| issue.number);
    let pull_requests = (facts.pull_requests.iter())
        .filter(|pr| by_them(pr.author.as_ref()))
        .map(|pr| pr.number);
    let mut numbers: BTreeSet<u32> = issues.chain(pull_requests).collect();
    let causes = Causes::of(&facts, repository.conventions);
    let items = candidates_of(facts, repository, &BTreeSet::new(), &causes);
    // Issues and PRs share their numbers, so the number of an item's head says whether they opened it.
    let theirs: Vec<WorkbenchItem> = (items.into_iter())
        .filter(|item| numbers.contains(&item.key().number()))
        .collect();
    let marks = (theirs.iter())
        .filter(|item| item.is_open())
        .map(|item| {
            // These marks come from the facts, not from a page the window drew, so they acknowledge no
            // finding of a Stale review (#908).
            let SeenMark { numbers, up_to, .. } = item.seen().mark.clone();
            Seeing {
                numbers,
                up_to,
                findings: vec![],
            }
        })
        .collect();
    numbers.extend(theirs.iter().flat_map(WorkbenchItem::numbers));
    Authored {
        marks,
        numbers: numbers.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests;
