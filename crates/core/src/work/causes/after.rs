//! The causes of one item's tree once the store holds a user action on it, as `Causes` would read the store
//! then (#1052). The action goes on a copy of the tree's facts as the store writes it, and `Judging` judges
//! that copy as `Causes::of` judges the Repository's, so the judge's rules exist once. The views then judge
//! the item again with the rules that build their lists, so the outcome each one gives an action follows
//! those rules, with no copy of them either.

use std::collections::{BTreeMap, BTreeSet};

use time::{OffsetDateTime, PrimitiveDateTime};

use super::{Causes, Judging, SeenMark};
use crate::activity::News;
use crate::conventions::Conventions;
use crate::facts::WorkFacts;
use crate::facts::activity::{ActivityFacts, Archive, ItemKey, MutedAuthor, PutAway, hidden_from};
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
    /// Unarchive, and Mark Unseen of an item the window draws Archived, which unarchives it (#491).
    Unarchive,
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
    /// The News of the facts. No viewer, nothing.
    news: Option<News<'a>>,
    pull_requests: BTreeMap<u32, &'a PullRequestFacts>,
    issues: BTreeMap<u32, &'a IssueFacts>,
}

impl<'a> Before<'a> {
    /// `facts` and `causes`, the causes `Causes::of` finds in them under `conventions`.
    pub(crate) fn of(facts: &'a WorkFacts, causes: &'a Causes, conventions: Conventions) -> Self {
        let news = (facts.viewer.as_ref())
            .map(|viewer| News::new(&facts.activity, &facts.pull_requests, viewer, conventions));
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
        let judging = Judging {
            viewer,
            pull_requests: picked(&self.pull_requests, numbers).into_values().collect(),
            issues: picked(&self.issues, numbers).into_values().collect(),
            activity: &tree.activity,
            kept: &tree.kept,
            news: news.over(&tree.activity),
        };
        let reviews = picked(&self.causes.reviews, numbers);
        judging.causes(reviews, kept(&self.causes.stale_reviews, numbers))
    }
}

/// The moment the store writes an action at: later than any fact it holds.
fn now() -> OffsetDateTime {
    PrimitiveDateTime::MAX.assume_utc()
}

/// The store's facts of one item's tree that a user action writes to.
struct Tree {
    numbers: Vec<u32>,
    activity: ActivityFacts,
    /// The last finding the store kept of each PR that the user's Review of it is stale.
    kept: BTreeMap<u32, StaleFound>,
}

impl Tree {
    /// The facts of the Issues and PRs `numbers` that `facts` hold.
    fn of(facts: &WorkFacts, numbers: &[u32]) -> Self {
        let keys: Vec<ItemKey> = (numbers.iter())
            .flat_map(|&number| [ItemKey::Issue { number }, ItemKey::PullRequest { number }])
            .collect();
        let activity = &facts.activity;
        let activity = ActivityFacts {
            first_sync: activity.first_sync,
            last_seen: picked(&activity.last_seen, numbers),
            acknowledged: picked(&activity.acknowledged, numbers),
            marked_unseen: picked(&activity.marked_unseen, numbers),
            archived: picked(&activity.archived, &keys),
            muted: picked(&activity.muted, &keys),
            muted_authors: activity.muted_authors.clone(),
            pinned: kept(&activity.pinned, numbers),
            held_closings: kept(&activity.held_closings, numbers),
            events: picked(&activity.events, numbers),
            ci_finished: picked(&activity.ci_finished, numbers),
            failed_checks: picked(&activity.failed_checks, numbers),
        };
        Self {
            numbers: numbers.to_vec(),
            activity,
            kept: picked(&facts.reviews.stale_found, numbers),
        }
    }

    /// What the store writes for `act` on the item `key` names, as `causes` judge it.
    fn write(&mut self, key: ItemKey, act: Act<'_>, causes: &Causes) {
        match act {
            Act::MarkSeen(mark) => self.mark_seen(&mark.numbers, mark.up_to, &mark.findings),
            Act::Archive(mark, put_away) => self.archive(key, mark, put_away),
            Act::Unarchive => self.activity.archived.retain(|archived, _| *archived != key),
            Act::Mute(mark) => {
                let at = self.hide(mark);
                let muted = self.activity.muted.entry(key).or_insert(at);
                *muted = (*muted).max(at);
            }
            Act::MuteAuthor { author, marks } => {
                for mark in marks {
                    self.mark_seen(&mark.numbers, mark.up_to, &mark.findings);
                }
                let muted = &mut self.activity.muted_authors;
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
        let activity = &mut self.activity;
        for number in numbers.iter().filter(|number| self.numbers.contains(number)) {
            activity.marked_unseen.retain(|flagged, _| flagged != number);
            if let Some(up_to) = up_to {
                for moments in [&mut activity.last_seen, &mut activity.acknowledged] {
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

    /// Mark Seen with `mark`, as Archive and Mute make it, and the moment `hidden_from` gives.
    fn hide(&mut self, mark: &SeenMark) -> OffsetDateTime {
        self.mark_seen(&mark.numbers, mark.up_to, &mark.findings);
        let acknowledged = mark.numbers.iter();
        let acknowledged = acknowledged.filter_map(|number| self.activity.acknowledged.get(number));
        hidden_from(mark.up_to, now(), acknowledged.copied())
    }

    /// The store's `archive` of the item `key` names with `mark`.
    fn archive(&mut self, key: ItemKey, mark: &SeenMark, put_away: PutAway) {
        let at = self.hide(mark);
        let archive = self.activity.archived.entry(key).or_insert(Archive {
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
        if self.activity.pinned.insert(number) {
            self.activity.held_closings.retain(|held| *held != number);
        }
        self.activity.archived.retain(|archived, _| *archived != key);
    }

    /// The store's `unpin` of the Issue `key` names, which archives an Issue closed while pinned, put away
    /// now, and forgets its closing (#820).
    fn unpin(&mut self, key: ItemKey, causes: &Causes) {
        let number = key.number();
        let pinned = self.activity.pinned.contains(&number);
        self.activity.pinned.retain(|pinned| *pinned != number);
        let held = self.activity.held_closings.contains(&number);
        if pinned && causes.closed.contains(&number) && held {
            let archive = Archive {
                seen_up_to: now(),
                archived_at: now(),
            };
            self.activity.archived.entry(key).or_insert(archive).archived_at = now();
            self.activity.held_closings.retain(|held| *held != number);
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
