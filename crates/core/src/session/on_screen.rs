//! The Issues and PRs on screen, which the session reads from GitHub again a minute after each one's last
//! read, but not while it's hidden (ADR 0015). It decides and does no IO: the session tells it what the user
//! did in the window and what it read, and asks it what's due.

use std::num::NonZeroU32;
use std::time::{Duration, Instant};

use crate::fetching::{Fetched, Item, ScreenMove};

/// The items the window shows, whether the user can see each, and when each is read again.
pub(super) struct OnScreen {
    /// How long after its last read an item on screen is read again.
    every: Duration,
    items: Vec<Shown>,
    window_hidden: bool,
}

/// An item the window holds a view of.
struct Shown {
    read: Fetched,
    /// How many views of it the window holds. A view that takes over from another of the same item opens
    /// before the other's close arrives, as that close waits for git to answer the other's opening.
    views: NonZeroU32,
    /// Whether another view hides it, as a PR from an Issue's tree hides the Issue's.
    hidden: bool,
    /// When its minute comes round, a minute after its last read.
    due: Instant,
}

impl OnScreen {
    /// Nothing on screen, in a window that shows, with each item read again `every` after its last read.
    pub(super) fn new(every: Duration) -> Self {
        Self {
            every,
            items: Vec::new(),
            window_hidden: false,
        }
    }

    /// The user did `moved` at `now`. Opening an item reads it, so its minute starts then.
    pub(super) fn moved(&mut self, moved: ScreenMove, now: Instant) {
        let due = now + self.every;
        match moved {
            ScreenMove::Opened(read) => {
                if let Some(shown) = self.of(read.item()).next() {
                    shown.views = shown.views.saturating_add(1);
                    shown.hidden = false;
                    shown.due = due;
                } else {
                    self.items.push(Shown {
                        read,
                        views: NonZeroU32::MIN,
                        hidden: false,
                        due,
                    });
                }
            }
            ScreenMove::Hid(item) => self.of(&item).for_each(|shown| shown.hidden = true),
            ScreenMove::Showed(item) => self.of(&item).for_each(|shown| shown.hidden = false),
            ScreenMove::Closed(item) => self.close(&item),
            ScreenMove::WindowHid => self.window_hidden = true,
            ScreenMove::WindowShowed => self.window_hidden = false,
            ScreenMove::WindowLoaded => {
                self.items.clear();
                self.window_hidden = false;
            }
        }
    }

    /// The session read `item` from GitHub at `now`, as a demand or its minute asked, or tried to.
    pub(super) fn read(&mut self, item: &Item, now: Instant) {
        let due = now + self.every;
        self.of(item).for_each(|shown| shown.due = due);
    }

    /// When the next minute comes round of an item the user can see, or nothing while there's none.
    pub(super) fn wake(&self) -> Option<Instant> {
        self.seen().map(|shown| shown.due).min()
    }

    /// The read of the item the user can see whose minute came round first, by `now`. One that shows again
    /// after its minute came round is due at once.
    pub(super) fn due(&self, now: Instant) -> Option<Fetched> {
        let came_round = self.seen().filter(|shown| shown.due <= now);
        let first = came_round.min_by_key(|shown| shown.due);
        first.map(|shown| shown.read.clone())
    }

    /// The items the user can see: none while the window is hidden.
    fn seen(&self) -> impl Iterator<Item = &Shown> {
        let window_hidden = self.window_hidden;
        (self.items.iter()).filter(move |shown| !window_hidden && !shown.hidden)
    }

    /// The view of `item`, if the window holds one.
    fn of<'a>(&'a mut self, item: &'a Item) -> impl Iterator<Item = &'a mut Shown> {
        (self.items.iter_mut()).filter(move |shown| shown.read.item().is(item))
    }

    /// One view of `item` went: the item goes once none is left.
    fn close(&mut self, item: &Item) {
        self.items.retain_mut(|shown| {
            if !shown.read.item().is(item) {
                return true;
            }
            match NonZeroU32::new(shown.views.get().saturating_sub(1)) {
                Some(left) => {
                    shown.views = left;
                    true
                }
                None => false,
            }
        });
    }
}

#[cfg(test)]
mod tests;
