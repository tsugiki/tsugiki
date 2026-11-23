//! The causes of one item's tree once the store holds a user action on it, as `Causes` would read the store
//! then (#1052). The action goes on a copy of the user's state in the tree, as the store writes it, and
//! `Judging` judges the tree with that state as `Causes::of` judges the Repository's, so the judge's rules
//! exist once. Only what an action writes is copied: the judge reads the rest from the page's facts, so no
//! read misses a fact the copy left out. The views then judge the item again with the rules that build
//! their lists, so the outcome each one gives an action follows those rules, with no copy of them either.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use time::{OffsetDateTime, PrimitiveDateTime};

use super::{Causes, Judging, SeenMark, State};
use crate::activity::News;
use crate::facts::WorkFacts;
use crate::facts::activity::{Archive, ItemKey, MutedAuthor, PutAway, hidden_from};
use crate::facts::issues::IssueFacts;
use crate::facts::patch_ids::{StaleFinding, StaleFound};
use crate::facts::pull_requests::PullRequestFacts;
use crate::login::Login;

/// A user action that can finish an item, with what the store writes for it, as the window and the session
/// make it.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Act<'a> {
    /// Mark Seen with the item's mark, as the window draws it.
    MarkSeen(&'a SeenMark),
    /// Archive, which marks the tree seen with the item's mark and puts the item away as `PutAway` says
    /// (#490). Mark Seen on an item the window draws Archived archives it again, as before.
    Archive(&'a SeenMark, PutAway),
    /// Unarchive (#491).
    Unarchive,
    /// Mark Unseen on the item's own Issue or PR, which flags it and unarchives the item (#491). The views
    /// read a flag anywhere in the tree alike, so the flag on the item stands for one on any line of it.
    MarkUnseen,
    /// Mute, which marks the tree seen with the item's mark and mutes the item (#492).
    Mute(&'a SeenMark),
    /// Mute author on `author` (#494), with Mark Seen on each open item they opened, as `authored_by` makes
    /// each mark.
    MuteAuthor {
        author: &'a Login,
        marks: &'a [SeenMark],
    },
    /// A pin of the Issue (#488).
    Pin,
    /// An unpin of the Issue (#488).
    Unpin,
}

/// The facts a page of items came from, and their causes, which `after` judges each item's tree of again
/// once the store holds an action on it.
pub(crate) struct Before<'a> {
    facts: &'a WorkFacts,
    causes: &'a Causes,
    /// The News of the facts, which `Causes::with_news` judged them by. No viewer, nothing.
    news: Option<News<'a>>,
    pull_requests: BTreeMap<u32, &'a PullRequestFacts>,
    issues: BTreeMap<u32, &'a IssueFacts>,
}

impl<'a> Before<'a> {
    /// `facts` and `causes`, the causes `Causes::with_news` finds in them by `news`.
    pub(crate) fn of(facts: &'a WorkFacts, causes: &'a Causes, news: Option<News<'a>>) -> Self {
        Self {
            facts,
            causes,
            news,
            pull_requests: (facts.pull_requests.iter())
                .map(|pr| (pr.number, pr))
                .collect(),
            issues: (facts.issues.iter())
                .map(|issue| (issue.number, issue))
                .collect(),
        }
    }

    /// The causes of the facts as they stand.
    pub(crate) fn causes(&self) -> &'a Causes {
        self.causes
    }

    /// The causes of the tree of the item `key` names, whose tree holds `numbers`, once the store holds what
    /// `act` writes on it. They hold that tree alone.
    pub(crate) fn after(&self, key: ItemKey, numbers: &[u32], act: Act<'_>) -> Causes {
        let (Some(viewer), Some(news)) = (self.facts.viewer.as_ref(), &self.news) else {
            return Causes::default();
        };
        let mut tree = Tree::of(self.facts, numbers);
        tree.write(key, act, self.causes);
        let state = tree.state();
        let judging = Judging {
            viewer,
            pull_requests: picked(&self.pull_requests, numbers).into_values().collect(),
            issues: picked(&self.issues, numbers).into_values().collect(),
            activity: &self.facts.activity,
            state,
            news: news.over(state.last_seen),
        };
        let reviews = picked(&self.causes.reviews, numbers);
        judging.causes(reviews, kept(&self.causes.stale_reviews, numbers))
    }
}

/// The moment the store writes an action at: later than any fact it holds.
fn now() -> OffsetDateTime {
    PrimitiveDateTime::MAX.assume_utc()
}

/// The user's state in one item's tree, which a user action writes to, as the store holds it. Muted
/// authors stay the page's until Mute author writes them.
struct Tree<'a> {
    numbers: &'a [u32],
    last_seen: BTreeMap<u32, OffsetDateTime>,
    acknowledged: BTreeMap<u32, OffsetDateTime>,
    marked_unseen: BTreeMap<u32, OffsetDateTime>,
    archived: BTreeMap<ItemKey, Archive>,
    muted: BTreeMap<ItemKey, OffsetDateTime>,
    muted_authors: Cow<'a, [MutedAuthor]>,
    pinned: BTreeSet<u32>,
    held_closings: BTreeSet<u32>,
    /// The last finding the store kept of each PR that the user's Review of it is stale.
    kept: BTreeMap<u32, StaleFound>,
}

impl<'a> Tree<'a> {
    /// The user's state on the Issues and PRs `numbers` that `facts` hold.
    fn of(facts: &'a WorkFacts, numbers: &'a [u32]) -> Self {
        let keys: Vec<ItemKey> = (numbers.iter())
            .flat_map(|&number| [ItemKey::Issue { number }, ItemKey::PullRequest { number }])
            .collect();
        let activity = &facts.activity;
        Self {
            numbers,
            last_seen: picked(&activity.last_seen, numbers),
            acknowledged: picked(&activity.acknowledged, numbers),
            marked_unseen: picked(&activity.marked_unseen, numbers),
            archived: picked(&activity.archived, &keys),
            muted: picked(&activity.muted, &keys),
            muted_authors: Cow::Borrowed(&activity.muted_authors),
            pinned: kept(&activity.pinned, numbers),
            held_closings: kept(&activity.held_closings, numbers),
            kept: picked(&facts.reviews.stale_found, numbers),
        }
    }

    /// The state as `Judging` reads it.
    fn state(&self) -> State<'_> {
        State {
            last_seen: &self.last_seen,
            acknowledged: &self.acknowledged,
            marked_unseen: &self.marked_unseen,
            archived: &self.archived,
            muted: &self.muted,
            muted_authors: &self.muted_authors,
            pinned: &self.pinned,
            kept: &self.kept,
        }
    }

    /// What the store writes for `act` on the item `key` names, as `causes` judge it.
    fn write(&mut self, key: ItemKey, act: Act<'_>, causes: &Causes) {
        match act {
            Act::MarkSeen(mark) => self.mark_seen(&mark.numbers, mark.up_to, &mark.findings),
            Act::Archive(mark, put_away) => self.archive(key, mark, put_away),
            Act::Unarchive => self.archived.retain(|archived, _| *archived != key),
            Act::MarkUnseen => self.mark_unseen(key),
            Act::Mute(mark) => {
                let at = self.hide(mark);
                let muted = self.muted.entry(key).or_insert(at);
                *muted = (*muted).max(at);
            }
            Act::MuteAuthor { author, marks } => {
                for mark in marks {
                    self.mark_seen(&mark.numbers, mark.up_to, &mark.findings);
                }
                let muted = self.muted_authors.to_mut();
                match muted.iter_mut().find(|muted| muted.login == *author) {
                    Some(muted) => muted.at = muted.at.max(now()),
                    None => muted.push(MutedAuthor {
                        login: author.clone(),
                        at: now(),
                    }),
                }
            }
            Act::Pin => self.pin(key),
            Act::Unpin => self.unpin(key, causes),
        }
    }

    /// Mark Seen on those of `numbers` in the tree, as the store's `mark_seen` writes it.
    fn mark_seen(
        &mut self,
        numbers: &[u32],
        up_to: Option<OffsetDateTime>,
        findings: &[StaleFinding],
    ) {
        for number in numbers
            .iter()
            .filter(|number| self.numbers.contains(number))
        {
            self.marked_unseen.retain(|flagged, _| flagged != number);
            if let Some(up_to) = up_to {
                for moments in [&mut self.last_seen, &mut self.acknowledged] {
                    let moment = moments.entry(*number).or_insert(up_to);
                    *moment = (*moment).max(up_to);
                }
            }
        }
        for finding in findings {
            let kept = self.kept.get_mut(&finding.number);
            if let Some(kept) = kept.filter(|kept| kept.on(finding.number) == *finding) {
                kept.acknowledged = true;
            }
        }
    }

    /// The store's `mark_unseen` of the Issue or PR `key` names, as the flag of the item drawn Archived:
    /// the flag, the unarchive of the item, and of each item under its number.
    fn mark_unseen(&mut self, key: ItemKey) {
        let number = key.number();
        let _: Option<OffsetDateTime> = self.marked_unseen.insert(number, now());
        self.archived
            .retain(|archived, _| archived.number() != number);
    }

    /// Mark Seen with `mark`, as Archive and Mute make it, and the moment `hidden_from` gives.
    fn hide(&mut self, mark: &SeenMark) -> OffsetDateTime {
        self.mark_seen(&mark.numbers, mark.up_to, &mark.findings);
        let acknowledged = mark.numbers.iter();
        let acknowledged = acknowledged.filter_map(|number| self.acknowledged.get(number));
        hidden_from(mark.up_to, now(), acknowledged.copied())
    }

    /// The store's `archive` of the item `key` names with `mark`.
    fn archive(&mut self, key: ItemKey, mark: &SeenMark, put_away: PutAway) {
        let at = self.hide(mark);
        let archive = self.archived.entry(key).or_insert(Archive {
            seen_up_to: at,
            archived_at: now(),
        });
        archive.seen_up_to = archive.seen_up_to.max(at);
        if put_away == PutAway::Now {
            archive.archived_at = now();
        }
    }

    /// The store's `pin` of the Issue `key` names: a new pin forgets a closing the sync hasn't judged, and
    /// the pin unarchives the Issue.
    fn pin(&mut self, key: ItemKey) {
        let number = key.number();
        if self.pinned.insert(number) {
            self.held_closings.retain(|held| *held != number);
        }
        self.archived.retain(|archived, _| *archived != key);
    }

    /// The store's `unpin` of the Issue `key` names, which archives an Issue closed while pinned, put away
    /// now, and forgets its closing (#820).
    fn unpin(&mut self, key: ItemKey, causes: &Causes) {
        let number = key.number();
        let pinned = self.pinned.contains(&number);
        self.pinned.retain(|pinned| *pinned != number);
        let held = self.held_closings.contains(&number);
        if pinned && causes.closed.contains(&number) && held {
            let archive = Archive {
                seen_up_to: now(),
                archived_at: now(),
            };
            self.archived.entry(key).or_insert(archive).archived_at = now();
            self.held_closings.retain(|held| *held != number);
        }
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
