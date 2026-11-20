//! An item's Needs attention and its part in Mark Seen: what `work` finds on each Issue and PR, summed over
//! the item's tree.

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use super::{IssueItem, WorkbenchItem};
use crate::work::{Cause, Causes, SeenMark, Why};

/// An item's part in Mark Seen and Mark Unseen: what Mark Seen sends, and which of the two its button offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Seen {
    pub mark: SeenMark,
    pub state: SeenState,
}

/// Whether the user has seen an item as it now is. The header offers Mark Seen unless they have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SeenState {
    /// The item, or a PR in an Issue's tree, is Marked unseen, whatever its activity.
    MarkedUnseen,
    /// The item, or a PR in an Issue's tree, has New activity, whoever it waits on.
    NewActivity,
    Seen,
}

/// The reasons an item Needs attention, and since when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Attention {
    /// The start of its oldest reason, which places the item in a list.
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub since: OffsetDateTime,
    /// The newest first, each once.
    pub why: Vec<Why>,
}

/// The part in Mark Seen of the item whose numbers are `numbers`, the item's own first.
pub(super) fn seen_of(causes: &Causes, numbers: Vec<u32>) -> Seen {
    let up_to = numbers
        .iter()
        .filter_map(|number| causes.known_until(*number))
        .max();
    let any = |holds: fn(&Causes, u32) -> bool| numbers.iter().any(|number| holds(causes, *number));
    let state = if any(Causes::is_marked_unseen) {
        SeenState::MarkedUnseen
    } else if any(Causes::has_news) {
        SeenState::NewActivity
    } else {
        SeenState::Seen
    };
    let findings = causes.findings_in(&numbers);
    Seen {
        mark: SeenMark {
            numbers,
            up_to,
            findings,
        },
        state,
    }
}

/// The sum of `item` and the rest of its tree.
pub(super) fn attention_of(causes: &Causes, item: &WorkbenchItem) -> Option<Attention> {
    let found = item.numbers().into_iter().map(|number| causes.on(number));
    summed(found.flatten())
}

/// The sum of `found`, the causes on an item's tree: none when there are none.
pub(super) fn summed<'a>(found: impl IntoIterator<Item = &'a Cause>) -> Option<Attention> {
    let mut found: Vec<&Cause> = found.into_iter().collect();
    let since = found.iter().map(|cause| cause.at).min()?;
    found.sort_by_key(|cause| std::cmp::Reverse(cause.at));
    let mut why: Vec<Why> = Vec::new();
    for cause in found {
        if !why.contains(&cause.why) {
            why.push(cause.why.clone());
        }
    }
    Some(Attention { since, why })
}

impl WorkbenchItem {
    /// The numbers of what this item shows.
    pub(crate) fn numbers(&self) -> Vec<u32> {
        match self {
            Self::Issue(issue) => issue.numbers(),
            Self::StandalonePr(_) | Self::StandaloneBackport(_) => {
                self.rows().iter().map(|row| row.number).collect()
            }
        }
    }
}

impl IssueItem {
    /// The Issue's number, then the number of each PR its tree holds.
    pub(super) fn numbers(&self) -> Vec<u32> {
        let rows = self.rows().into_iter().map(|row| row.number);
        std::iter::once(self.number).chain(rows).collect()
    }
}

#[cfg(test)]
mod tests;
