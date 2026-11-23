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
    let items = candidates_of(&facts, repository, &BTreeSet::new(), &causes);
    // Issues and PRs share their numbers, so the number of an item's head says whether they opened it.
    let theirs: Vec<WorkbenchItem> = (items.into_iter())
        .filter(|item| numbers.contains(&item.key().number()))
        .collect();
    let marks = (theirs.iter().filter_map(mark_of))
        .map(|SeenMark { numbers, up_to, findings }| Seeing {
            numbers,
            up_to,
            findings,
        })
        .collect();
    numbers.extend(theirs.iter().flat_map(WorkbenchItem::numbers));
    Authored {
        marks,
        numbers: numbers.into_iter().collect(),
    }
}

/// Mute author's Mark Seen on `item`, an item the author opened: on its tree, while it's open. The mark
/// comes from the facts, not from a page the window drew, so it acknowledges no finding of a Stale review
/// (#908).
fn mark_of(item: &WorkbenchItem) -> Option<SeenMark> {
    let SeenMark { numbers, up_to, .. } = item.seen().mark.clone();
    item.is_open().then_some(SeenMark {
        numbers,
        up_to,
        findings: vec![],
    })
}

/// The marks Mute author makes on the items of each author `items` offer it for, as `authored_by` makes
/// them, with the author.
pub(crate) fn marks_by_author<'a>(
    items: impl IntoIterator<Item = &'a WorkbenchItem>,
) -> Vec<(Login, Vec<SeenMark>)> {
    let mut marks: Vec<(Login, Vec<SeenMark>)> = Vec::new();
    for item in items {
        let Some(author) = item.mute_author_offered() else {
            continue;
        };
        let mark = mark_of(item);
        match marks.iter_mut().find(|(login, _)| login == author) {
            Some((_, theirs)) => theirs.extend(mark),
            None => marks.push((author.clone(), mark.into_iter().collect())),
        }
    }
    marks
}

#[cfg(test)]
mod tests;
