//! The place each view of the Workbench gives an item, and the outcome there of each user action that can
//! finish an item (#1052). One rule, `place`, both builds each view's list and judges each action, by where
//! the item would stand once the action is done, so the window reads the outcome and keeps no rule of its
//! own.

use std::collections::BTreeSet;

use serde::Serialize;
use specta::Type;

use super::{BackportRow, Hidden, ListedItem, MuteAuthor, PrTree, WorkbenchItem, WorkbenchView};
use crate::login::Login;
use crate::work::Causes;

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
    /// In the fold of Muted items at the list's end, which a Filter's view has (#824).
    Folded,
}

/// One thing that holds of an item, which a view's rule or an action reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Is {
    /// It Needs attention, as the list draws it.
    NeedingAttention,
    /// An Issue or PR in its tree waits on the user, which holds it on the Workbench.
    Waiting,
    /// A Filter's search or an open PR the user wrote or reviewed holds it on the Workbench.
    Held,
    /// A Pinned issue (#488).
    Pinned,
    /// Closed with an archive, so it goes back to Archived once nothing in its tree waits.
    RestingArchived,
    /// A Pinned issue that closed while pinned, which the unpin archives.
    ClosedWhilePinned,
    /// The search of the Filter on show found something in its tree.
    Found,
    /// A Review request stands in its tree, acknowledged or not.
    Requested,
    /// A Review request or an assignment to the user stands in its tree, acknowledged or not.
    Asked,
    /// A Stale review in its tree waits on the user.
    Stale,
    /// An Issue, which alone takes a pin.
    Issue,
    /// Mute author on its own Issue or PR mutes its author: the user didn't open it, and the author isn't
    /// Muted yet.
    OfferingMuteAuthor,
}

/// An item's standing, which decides whether a view lists it, and where: the user's state that hides it,
/// and what holds of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Standing {
    pub(crate) hidden: Option<Hidden>,
    pub(crate) is: BTreeSet<Is>,
}

/// The sets of Issues and PRs a `Standing` reads off each item's tree.
pub(crate) struct Reach<'a> {
    pub(crate) causes: &'a Causes,
    pub(crate) held: &'a BTreeSet<u32>,
    pub(crate) waiting: &'a BTreeSet<u32>,
    /// The Issues and PRs the Filter on show found, in a Filter's view.
    pub(crate) found: Option<&'a BTreeSet<u32>>,
    pub(crate) asked: &'a BTreeSet<u32>,
    /// The Pinned issues that closed while pinned.
    pub(crate) held_closings: &'a BTreeSet<u32>,
}

impl Reach<'_> {
    pub(crate) fn standing_of(&self, listed: &ListedItem) -> Standing {
        let reached = |numbers: &BTreeSet<u32>| listed.item.is_reached_by(numbers);
        let pinned = listed.item.is_pinned();
        let holding = [
            (Is::NeedingAttention, listed.attention.is_some()),
            (Is::Waiting, reached(self.waiting)),
            (Is::Held, reached(self.held)),
            (Is::Pinned, pinned),
            (Is::RestingArchived, self.causes.rests_archived(listed.key)),
            (
                Is::ClosedWhilePinned,
                pinned && self.held_closings.contains(&listed.key.number()),
            ),
            (Is::Found, self.found.is_some_and(reached)),
            (Is::Requested, reached(&self.causes.review_requests)),
            (Is::Asked, reached(self.asked)),
            (Is::Stale, reached(&self.causes.stale_reviews)),
            (Is::Issue, matches!(listed.item, WorkbenchItem::Issue(_))),
            (
                Is::OfferingMuteAuthor,
                listed.item.mute_author_offered().is_some(),
            ),
        ];
        Standing {
            hidden: listed.hidden,
            is: (holding.into_iter())
                .filter_map(|(is, holds)| holds.then_some(is))
                .collect(),
        }
    }
}

/// The place `view` gives an item that stands as `standing` does, if it lists it.
pub(crate) fn place(view: &WorkbenchView, standing: &Standing) -> Option<Place> {
    let hidden = standing.hidden;
    let is = |what| standing.is(what);
    let muted = hidden.is_some_and(Hidden::is_muted);
    let shown = standing.shown_on_workbench();
    let listed = match view {
        // The item is on the Workbench when anything in its tree is, in its own right, or when the user's
        // state hides it and `Standing::shown_on_workbench` shows it all the same.
        WorkbenchView::Whole => {
            let own_right = is(Is::Held) || is(Is::Waiting) || is(Is::Pinned);
            (hidden.is_some() || own_right) && shown
        }
        // A Filter's view lists all its search found, as a search finds archived mail too (#824). An
        // Archived item stays in place, and so does a Muted one while a Review request or an assignment to
        // the user stands in its tree. The other Muted items go in a fold at the end.
        WorkbenchView::Filter { .. } => {
            let folded = muted && !is(Is::Asked);
            return is(Is::Found).then_some(if folded { Place::Folded } else { Place::Listed });
        }
        // The Review requests view keeps a PR whose Review request stood at the archive (#490), and a Muted
        // PR while its request stands (#492), each marked as hidden.
        WorkbenchView::ReviewRequests => is(Is::Requested) && (!muted || is(Is::Asked)),
        WorkbenchView::StaleReviews => is(Is::Stale) && hidden.is_none(),
        WorkbenchView::Archived => hidden == Some(Hidden::Archived),
        // The Pinned issues' view lists each one the Workbench shows, and an Archived one in its place,
        // marked Archived as a Filter's view marks it, so the user finds every pin there to unpin (#489).
        WorkbenchView::Pinned => is(Is::Pinned) && (hidden == Some(Hidden::Archived) || shown),
    };
    listed.then_some(Place::Listed)
}

/// The outcome in `view` of each action on an item that stands as `standing` does.
pub(crate) fn outcomes_of(view: &WorkbenchView, standing: &Standing) -> Outcomes {
    let now = place(view, standing);
    let outcome = |after: Option<Standing>| match after {
        None => Outcome::Unchanged,
        Some(after) => match (now, place(view, &after)) {
            (_, None) => Outcome::Leaves,
            (Some(Place::Listed), Some(Place::Folded)) => Outcome::Folds,
            _ => Outcome::Stays,
        },
    };
    Outcomes {
        mark_seen: outcome(standing.marked_seen()),
        archive: outcome(standing.archived()),
        unarchive: outcome(standing.unarchived()),
        mute: outcome(standing.muted()),
        mute_author: outcome(standing.author_muted()),
        pin: outcome(standing.pinned()),
        unpin: outcome(standing.unpinned()),
    }
}

impl Standing {
    fn is(&self, what: Is) -> bool {
        self.is.contains(&what)
    }

    /// Whether the Workbench shows the item for what hides it: an Archived one never, and a Muted one while
    /// an ask that started after the mute stands (#492).
    pub(crate) fn shown_on_workbench(&self) -> bool {
        match self.hidden {
            None => true,
            Some(Hidden::Archived) => false,
            Some(Hidden::Muted | Hidden::MutedAuthor) => self.is(Is::NeedingAttention),
        }
    }

    /// The standing with `hidden`, and with none of `gone`.
    fn after(&self, hidden: Option<Hidden>, gone: &[Is]) -> Self {
        let is = self.is.iter().filter(|is| !gone.contains(is)).copied();
        Self {
            hidden,
            is: is.collect(),
        }
    }

    /// The standing once the user acknowledged what waits in the item's tree and hid it as `hidden` says,
    /// as Mark Seen, Archive, Mute and Mute author each do. A closed item with an archive that nothing
    /// else hides goes back to Archived then.
    fn settled(&self, hidden: Option<Hidden>) -> Self {
        let hidden = match hidden {
            None if self.is(Is::RestingArchived) => Some(Hidden::Archived),
            hidden => hidden,
        };
        self.after(hidden, &[Is::NeedingAttention, Is::Waiting, Is::Stale])
    }

    /// Mark Seen, which does nothing to a Muted item.
    fn marked_seen(&self) -> Option<Self> {
        let muted = self.hidden.is_some_and(Hidden::is_muted);
        (!muted).then(|| self.settled(self.hidden))
    }

    /// Archive, which puts away an item nothing hides yet.
    fn archived(&self) -> Option<Self> {
        (self.hidden.is_none()).then(|| self.settled(Some(Hidden::Archived)))
    }

    fn unarchived(&self) -> Option<Self> {
        (self.hidden == Some(Hidden::Archived)).then(|| self.after(None, &[]))
    }

    /// Mute, which does nothing to a Muted item.
    fn muted(&self) -> Option<Self> {
        let muted = self.hidden.is_some_and(Hidden::is_muted);
        (!muted).then(|| self.settled(Some(Hidden::Muted)))
    }

    /// Mute author on the author of the item's own Issue or PR, while the item offers it.
    fn author_muted(&self) -> Option<Self> {
        (self.is(Is::OfferingMuteAuthor)).then(|| self.settled(Some(Hidden::MutedAuthor)))
    }

    /// A pin, which only an Issue without one takes, and which unarchives it.
    fn pinned(&self) -> Option<Self> {
        let hidden = self.hidden.filter(|hidden| *hidden != Hidden::Archived);
        (self.is(Is::Issue) && !self.is(Is::Pinned)).then(|| {
            let mut pinned = self.after(hidden, &[]);
            pinned.is.extend([Is::Pinned]);
            pinned
        })
    }

    /// An unpin, which archives a pin that closed while pinned, once nothing waits there.
    fn unpinned(&self) -> Option<Self> {
        let hidden = match self.hidden {
            None if self.is(Is::ClosedWhilePinned) && !self.is(Is::Waiting) => {
                Some(Hidden::Archived)
            }
            hidden => hidden,
        };
        (self.is(Is::Pinned)).then(|| self.after(hidden, &[Is::Pinned, Is::ClosedWhilePinned]))
    }
}

impl WorkbenchItem {
    /// The author Mute author on the item's own Issue or PR mutes, if it offers to: the user didn't open
    /// it, and the author isn't Muted yet.
    pub(crate) fn mute_author_offered(&self) -> Option<&Login> {
        let offered = match self {
            Self::Issue(issue) => &issue.mute_author,
            Self::StandalonePr(PrTree { row, .. })
            | Self::StandaloneBackport(BackportRow { row, .. }) => &row.mute_author,
        };
        match offered {
            MuteAuthor::Mute { login } => Some(login),
            MuteAuthor::Off | MuteAuthor::Unmute { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests;
