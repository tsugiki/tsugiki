//! The causes of one item's tree once the store holds a user action on it, as `Causes` would read the store
//! then (#1052). The views judge the item again with the rules that build their lists, so the outcome each
//! one gives an action follows those rules, with no copy of them.

use std::collections::{BTreeMap, BTreeSet};

use time::{OffsetDateTime, PrimitiveDateTime};

use super::{Causes, SeenMark, Why};
use crate::facts::activity::{Archive, ItemKey};
use crate::login::Login;

/// A user action that can finish an item, with what the store writes for it, as the window and the session
/// make it.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Act<'a> {
    /// Mark Seen with the item's mark, as the window draws it.
    MarkSeen(&'a SeenMark),
    /// Archive, which marks the tree seen with the item's mark and puts the item away (#490). Mark Seen on an
    /// item the window draws Archived archives it again, which keeps it Archived.
    Archive(&'a SeenMark),
    /// Unarchive, and Mark Unseen of an item the window draws Archived, which unarchives it (#491).
    Unarchive,
    /// Mute, which marks the tree seen with the item's mark and mutes the item (#492).
    Mute(&'a SeenMark),
    /// Mute author on `author`, who opened each of `theirs` in the tree (#494). It marks the tree seen with
    /// `mark`, the item's, when they opened the item and it's open.
    MuteAuthor {
        author: &'a Login,
        theirs: &'a [ItemKey],
        mark: Option<&'a SeenMark>,
    },
    /// A pin of the Issue (#488).
    Pin,
    /// An unpin of the Issue (#488).
    Unpin,
}

/// The moment the store writes an action at: later than any fact it holds.
fn now() -> OffsetDateTime {
    PrimitiveDateTime::MAX.assume_utc()
}

impl Causes {
    /// The causes of the tree of the item `key` names, whose tree holds `numbers`, once the store holds what
    /// `act` writes on it. They hold that tree alone, and judge it as `self` does but for the act.
    pub(crate) fn after(&self, key: ItemKey, numbers: &[u32], act: Act<'_>) -> Self {
        let mut after = self.of_tree(numbers);
        match act {
            Act::MarkSeen(mark) => after.mark_seen(mark),
            Act::Archive(mark) => {
                let at = after.hide(mark);
                let archive = after.archived.entry(key).or_insert(Archive {
                    seen_up_to: at,
                    archived_at: now(),
                });
                archive.seen_up_to = archive.seen_up_to.max(at);
            }
            Act::Unarchive => after.archived.retain(|archived, _| *archived != key),
            Act::Mute(mark) => {
                let at = after.hide(mark);
                let muted = after.muted.entry(key).or_insert(at);
                *muted = (*muted).max(at);
            }
            Act::MuteAuthor {
                author,
                theirs,
                mark,
            } => {
                if let Some(mark) = mark {
                    after.mark_seen(mark);
                }
                after.muted_authors.push(author.clone());
                after
                    .by_muted_authors
                    .extend(theirs.iter().map(|key| (*key, now())));
            }
            // A pin forgets a closing the sync hasn't judged, and unarchives the Issue.
            Act::Pin => {
                let number = key.number();
                if after.pinned.insert(number) {
                    after.held_closings.retain(|held| *held != number);
                }
                after.archived.retain(|archived, _| *archived != key);
            }
            // The unpin archives an Issue whose closing the pin held back, put away now (#820).
            Act::Unpin => {
                let number = key.number();
                let held = after.pinned.remove(&number) && after.closed.contains(&number);
                if held && after.held_closings.remove(&number) {
                    let archive = Archive {
                        seen_up_to: now(),
                        archived_at: now(),
                    };
                    after.archived.entry(key).or_insert(archive).archived_at = now();
                }
            }
        }
        after
    }

    /// The causes of the Issues and PRs `numbers` alone, each judged as `self` judges it.
    fn of_tree(&self, numbers: &[u32]) -> Self {
        let keys: Vec<ItemKey> = (numbers.iter())
            .flat_map(|&number| [ItemKey::Issue { number }, ItemKey::PullRequest { number }])
            .collect();
        Self {
            by_number: picked(&self.by_number, numbers),
            review_requests: kept(&self.review_requests, numbers),
            stale_reviews: kept(&self.stale_reviews, numbers),
            stale_found: picked(&self.stale_found, numbers),
            with_news: kept(&self.with_news, numbers),
            known_until: picked(&self.known_until, numbers),
            marked_unseen: kept(&self.marked_unseen, numbers),
            reviews: picked(&self.reviews, numbers),
            archived: picked(&self.archived, &keys),
            closed: kept(&self.closed, numbers),
            muted: picked(&self.muted, &keys),
            by_muted_authors: picked(&self.by_muted_authors, &keys),
            muted_authors: self.muted_authors.clone(),
            pinned: kept(&self.pinned, numbers),
            held_closings: kept(&self.held_closings, numbers),
            viewer: self.viewer.clone(),
            asks: picked(&self.asks, numbers),
            acknowledged: picked(&self.acknowledged, numbers),
        }
    }

    /// Mark Seen with `mark`, as the store writes it: each of its numbers loses its Marked unseen, and with
    /// an `up_to` the user has acknowledged it up to that moment, which ends each cause on it from then or
    /// before, as `Judge` would find. A Stale review whose finding the store kept stands while neither the
    /// mark nor an earlier one acknowledges that finding (#908).
    fn mark_seen(&mut self, mark: &SeenMark) {
        self.marked_unseen
            .retain(|flagged| !mark.numbers.contains(flagged));
        for &number in &mark.numbers {
            if let Some(up_to) = mark.up_to {
                let acknowledged = self.acknowledged.entry(number).or_insert(up_to);
                *acknowledged = (*acknowledged).max(up_to);
            }
            let found = self.stale_found.get_mut(&number);
            let unacknowledged = found.is_some_and(|found| {
                found.acknowledged |= mark.findings.contains(&found.on(number));
                !found.acknowledged
            });
            let Some(causes) = self.by_number.get_mut(&number) else {
                continue;
            };
            causes.retain(|cause| {
                let flag = matches!(cause.why, Why::MarkedUnseen);
                let found = matches!(cause.why, Why::StaleReview) && unacknowledged;
                let later = mark.up_to.is_none_or(|up_to| cause.at > up_to);
                !flag && (found || later)
            });
        }
        self.by_number.retain(|_, causes| !causes.is_empty());
    }

    /// Mark Seen with `mark`, as Archive and Mute make it. Returns the moment the item they hide is hidden
    /// from: the mark's `up_to`, or now when the window drew no sync, or a later moment one of its numbers
    /// was acknowledged at.
    fn hide(&mut self, mark: &SeenMark) -> OffsetDateTime {
        self.mark_seen(mark);
        let acknowledged = (mark.numbers.iter()).filter_map(|number| self.acknowledged.get(number));
        acknowledged.fold(mark.up_to.unwrap_or_else(now), |at, acknowledged| {
            at.max(*acknowledged)
        })
    }
}

/// The entries of `map` under `keys`.
fn picked<K: Ord + Copy, V: Clone>(map: &BTreeMap<K, V>, keys: &[K]) -> BTreeMap<K, V> {
    let found = keys
        .iter()
        .filter_map(|key| Some((*key, map.get(key)?.clone())));
    found.collect()
}

/// The members of `set` among `keys`.
fn kept<K: Ord + Copy>(set: &BTreeSet<K>, keys: &[K]) -> BTreeSet<K> {
    keys.iter()
        .filter(|key| set.contains(key))
        .copied()
        .collect()
}
