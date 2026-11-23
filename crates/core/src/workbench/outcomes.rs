//! The place each view of the Workbench gives an item, and the outcome there of each user action that can
//! finish an item (#1052). An outcome compares where the view lists the item now with where it would once
//! the store holds what the action writes, `Before::after`, judged by the rule that builds the view's list:
//! `place` for the Workbench's views, and the Inbox's own for the Inbox. So no rule exists twice, and the
//! window reads the outcome and keeps none of its own.

use std::collections::BTreeSet;

use serde::Serialize;
use specta::Type;

use super::mute_author::marks_by_author;
use super::{Attention, BackportRow, Hidden, ListedItem, MuteAuthor, PrTree, WorkbenchItem};
use crate::facts::activity::PutAway;
use crate::login::Login;
use crate::work::{Act, Before, Causes, SeenMark};

/// A user action's effect on a listed item in the view that lists it, which the window reads at the
/// keypress to move the cursor on (#1052).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The action does nothing to the item, as Archive does to a Muted one.
    Unchanged,
    /// The view keeps listing the item, in its place or another.
    Stays,
    /// The view moves the item into its fold of Muted items at the list's end (#824).
    Folds,
    /// The view leaves the item out.
    Leaves,
}

/// The `Outcome` of each user action that can finish an item, in the view that lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct Outcomes {
    pub mark_seen: Outcome,
    pub archive: Outcome,
    pub unarchive: Outcome,
    pub mute: Outcome,
    /// Mute author on the author of the item's own Issue or PR (#494).
    pub mute_author: Outcome,
    pub pin: Outcome,
    pub unpin: Outcome,
}

impl Outcome {
    /// The outcome of an action that moves an item from the place `before` gives it to the one `after`
    /// does, where an action that leaves the item listed somewhere keeps it.
    pub(crate) fn between(before: Option<Place>, after: Option<Place>) -> Self {
        match (before, after) {
            (_, None) => Self::Leaves,
            (Some(Place::Listed), Some(Place::Folded)) => Self::Folds,
            (_, Some(_)) => Self::Stays,
        }
    }
}

impl Outcomes {
    /// The outcomes an item carries until the view that lists it judges each action.
    pub(crate) const UNJUDGED: Self = Self {
        mark_seen: Outcome::Unchanged,
        archive: Outcome::Unchanged,
        unarchive: Outcome::Unchanged,
        mute: Outcome::Unchanged,
        mute_author: Outcome::Unchanged,
        pin: Outcome::Unchanged,
        unpin: Outcome::Unchanged,
    };
}

/// The place a view gives an item it lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    /// Among the items, in the list's order.
    Listed,
    /// In the fold of Muted items at the list's end, which only a Filter's view has (#824).
    Folded,
}

/// A view of the Workbench, as `place` reads it.
pub(crate) enum Listing<'a> {
    Whole,
    /// A Filter's view, with each Issue and PR its search found.
    Filter {
        found: &'a BTreeSet<u32>,
    },
    ReviewRequests,
    StaleReviews,
    Archived,
    Pinned,
}

/// An item's pin, which only an Issue takes (#488).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pinning {
    /// A Standalone PR, which takes no pin.
    Unpinnable,
    Unpinned,
    Pinned,
}

impl WorkbenchItem {
    /// The item's pin, as `causes` judge it.
    pub(crate) fn pinning(&self, causes: &Causes) -> Pinning {
        match self {
            Self::Issue(issue) if causes.is_pinned(issue.number) => Pinning::Pinned,
            Self::Issue(_) => Pinning::Unpinned,
            Self::StandalonePr(_) | Self::StandaloneBackport(_) => Pinning::Unpinnable,
        }
    }

    /// The author Mute author on the item's own Issue or PR mutes, if it offers to: the user didn't open
    /// it, and the author isn't Muted yet.
    pub(crate) fn mute_author_offered(&self) -> Option<&Login> {
        offered(self.own_mute_author())
    }

    fn own_mute_author(&self) -> &MuteAuthor {
        match self {
            Self::Issue(issue) => &issue.mute_author,
            Self::StandalonePr(PrTree { row, .. })
            | Self::StandaloneBackport(BackportRow { row, .. }) => &row.mute_author,
        }
    }
}

/// Whether the Workbench shows an item `hidden` hides, which Needs attention as `attention` says: an
/// Archived one never, and a Muted one while an ask that started after the mute stands (#492).
pub(crate) fn shown_on_workbench(hidden: Option<Hidden>, attention: Option<&Attention>) -> bool {
    match hidden {
        None => true,
        Some(Hidden::Archived) => false,
        Some(Hidden::Muted | Hidden::MutedAuthor) => attention.is_some(),
    }
}

/// The place `listing` gives `item` as `causes` judge it, if it lists it. `held` holds each Issue and PR
/// that holds an item on the Workbench in its own right without a pin: a Filter's search or an open PR the
/// user wrote or reviewed.
pub(crate) fn place(
    listing: &Listing<'_>,
    held: &BTreeSet<u32>,
    causes: &Causes,
    item: &WorkbenchItem,
) -> Option<Place> {
    let (hidden, attention) = ListedItem::judged(causes, item);
    let numbers = item.numbers();
    let any = |holds: &dyn Fn(&u32) -> bool| numbers.iter().any(holds);
    let muted = hidden.is_some_and(Hidden::is_muted);
    let asked = any(&|number| causes.is_asked(*number));
    let shown = shown_on_workbench(hidden, attention.as_ref());
    let pinned = item.pinning(causes) == Pinning::Pinned;
    let listed = match listing {
        // The item is on the Workbench when anything in its tree is, in its own right, or when the user's
        // state hides it and `shown_on_workbench` shows it all the same.
        Listing::Whole => {
            let waiting = any(&|number| causes.needs_attention(*number));
            let own_right = any(&|number| held.contains(number)) || waiting || pinned;
            (hidden.is_some() || own_right) && shown
        }
        // A Filter's view lists all its search found, as a search finds archived mail too (#824). An
        // Archived item stays in place, and so does a Muted one while a Review request or an assignment to
        // the user stands in its tree. The other Muted items go in a fold at the end.
        Listing::Filter { found } => {
            let folded = muted && !asked;
            let place = if folded { Place::Folded } else { Place::Listed };
            return any(&|number| found.contains(number)).then_some(place);
        }
        // The Review requests view keeps a PR whose Review request stood at the archive (#490), and a Muted
        // PR while its request stands (#492), each marked as hidden.
        Listing::ReviewRequests => {
            any(&|number| causes.review_requests.contains(number)) && (!muted || asked)
        }
        Listing::StaleReviews => {
            any(&|number| causes.stale_reviews.contains(number)) && hidden.is_none()
        }
        Listing::Archived => hidden == Some(Hidden::Archived),
        // The Pinned issues' view lists each one the Workbench shows, and an Archived one in its place,
        // marked Archived as a Filter's view marks it, so the user finds every pin there to unpin (#489).
        Listing::Pinned => pinned && (hidden == Some(Hidden::Archived) || shown),
    };
    listed.then_some(Place::Listed)
}

/// What the outcomes of a page's items are judged against: the facts and causes the page came from, and the
/// marks Mute author makes on the items of each author the page offers it for.
pub(crate) struct Acting<'a> {
    before: Before<'a>,
    marks: Vec<(Login, Vec<SeenMark>)>,
}

impl<'a> Acting<'a> {
    /// The actions on `items`, every item of the page `before` holds the facts of, listed or not.
    pub(crate) fn new<'b>(
        before: Before<'a>,
        items: impl IntoIterator<Item = &'b WorkbenchItem>,
    ) -> Self {
        Self {
            before,
            marks: marks_by_author(items),
        }
    }

    /// The outcome of each action on `item`, which a view draws hidden as `hidden` says, and of Mute author
    /// on the author of each PR the view shows in its tree. `shown` gives what the view shows of the item
    /// as the causes it gets judge it: the page's for now, and for each action the causes once the store
    /// holds what the action writes.
    pub(crate) fn outcomes_of(
        &self,
        item: &WorkbenchItem,
        hidden: Option<Hidden>,
        shown: impl Fn(&Causes) -> Option<Shown>,
    ) -> (Outcomes, Vec<RowOutcome>) {
        let causes = self.before.causes();
        let (key, numbers) = (item.key(), item.numbers());
        let now = shown(causes);
        let place_now = now.as_ref().map(|now| now.place);
        let after = |act: Act<'_>| shown(&self.before.after(key, &numbers, act));
        let outcome = |act: Option<Act<'_>>| {
            let Some(act) = act else {
                return Outcome::Unchanged;
            };
            Outcome::between(place_now, after(act).map(|after| after.place))
        };
        let mark = &item.seen().mark;
        let archived = hidden == Some(Hidden::Archived);
        let muted = hidden.is_some_and(Hidden::is_muted);
        let pinning = item.pinning(causes);
        let own_author = item.mute_author_offered();
        let outcomes = Outcomes {
            mark_seen: outcome(Some(if archived {
                Act::Archive(mark, PutAway::AsBefore)
            } else {
                Act::MarkSeen(mark)
            })),
            // The window offers neither Archive nor Mute on a Muted item, which Unmute or Unmute author
            // bring back (#492, #494), and Archive on an Archived one finds nothing to put away.
            archive: outcome(hidden.is_none().then_some(Act::Archive(mark, PutAway::Now))),
            unarchive: outcome(archived.then_some(Act::Unarchive)),
            mute: outcome((!muted).then_some(Act::Mute(mark))),
            mute_author: outcome(own_author.map(|author| self.mute_author(author))),
            pin: outcome((pinning == Pinning::Unpinned).then_some(Act::Pin)),
            unpin: outcome((pinning == Pinning::Pinned).then_some(Act::Unpin)),
        };
        (outcomes, self.row_outcomes(item, now.as_ref(), after))
    }

    /// The outcome of Mute author on the author of each PR in `item`'s tree that the view shows, as `now`
    /// says, where `after` gives what the view shows once the store holds an action. The item's own
    /// `mute_author` covers the PRs of the item's own author.
    fn row_outcomes(
        &self,
        item: &WorkbenchItem,
        now: Option<&Shown>,
        after: impl Fn(Act<'_>) -> Option<Shown>,
    ) -> Vec<RowOutcome> {
        let own_author = item.mute_author_offered();
        let shown = now.map_or(&[][..], |now| now.rows.as_slice());
        let by: Vec<(u32, &Login)> = (item.rows().into_iter())
            .filter(|row| shown.contains(&row.number))
            .filter_map(|row| Some((row.number, offered(&row.mute_author)?)))
            .filter(|(_, author)| own_author != Some(*author))
            .collect();
        let mut muting: Vec<(&Login, Option<Shown>)> = Vec::new();
        let mut rows = Vec::new();
        for (number, author) in by {
            if !muting.iter().any(|(muted, _)| *muted == author) {
                muting.push((author, after(self.mute_author(author))));
            }
            let muted = muting.iter().find(|(muted, _)| *muted == author);
            let kept = muted.and_then(|(_, after)| after.as_ref());
            let place = kept.filter(|after| after.rows.contains(&number));
            rows.push(RowOutcome {
                number,
                mute_author: Outcome::between(now.map(|now| now.place), place.map(|after| after.place)),
            });
        }
        rows
    }

    /// Mute author on `author`, with the marks it makes on the page's items they opened.
    fn mute_author<'b>(&'b self, author: &'b Login) -> Act<'b> {
        let marks = (self.marks.iter()).find(|(login, _)| login == author);
        Act::MuteAuthor {
            author,
            marks: marks.map_or(&[], |(_, marks)| marks.as_slice()),
        }
    }
}

/// The author Mute author mutes where `offered` offers it.
fn offered(offered: &MuteAuthor) -> Option<&Login> {
    match offered {
        MuteAuthor::Mute { login } => Some(login),
        MuteAuthor::Off | MuteAuthor::Unmute { .. } => None,
    }
}

/// What a view shows of an item: its place, and each PR it shows in the item's tree.
pub(crate) struct Shown {
    pub(crate) place: Place,
    pub(crate) rows: Vec<u32>,
}

impl Shown {
    /// `item` in `place`, with each PR its tree holds.
    pub(crate) fn whole(place: Place, item: &WorkbenchItem) -> Self {
        let rows = item.rows().into_iter().map(|row| row.number).collect();
        Self { place, rows }
    }
}

/// The outcome of Mute author on the author of a PR in a listed item's tree, for that PR's row, in the
/// view that lists the item (#1052). The item's own `Outcomes` covers the item's own author.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct RowOutcome {
    pub number: u32,
    pub mute_author: Outcome,
}

#[cfg(test)]
mod tests;
