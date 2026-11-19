//! What the Mirror worker knows of one Repository's PR heads (#899). Each PR keeps three things apart, each
//! with its own writers: where the mirror's ref stands, the head the rules want, and the head the user asked
//! for by opening the PR. Both can be live for one PR, as when the rules' fetch of one commit runs while the
//! user asks for another.
//!
//! No state says a fetch is under way: the running fetch's batch holds each PR it fetches and at which
//! commit. A head that comes during the fetch at the batch's commit joins it, as the fetch's outcome speaks
//! for each head at that commit.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use super::Unfetched;
use crate::facts::CommitSha;
use crate::facts::pull_requests::PrHead;

#[cfg(test)]
mod tests;

/// The PR heads of one Repository.
#[derive(Debug, Default)]
pub(super) struct Prs {
    heads: BTreeMap<u32, Head>,
    /// The PRs of the last hand-over: those in Reach. Nothing before the first one. A want goes once it is
    /// met, so the wants can't say which PRs are in Reach.
    handed: Option<BTreeSet<u32>>,
    /// The PRs out of Reach whose refs upkeep keeps, as the rules last said: those of the Archived trees
    /// (#820).
    kept: BTreeSet<u32>,
}

/// What the worker knows of one PR's head. One with nothing in it goes.
#[derive(Debug, Default)]
struct Head {
    /// Where the mirror's ref stands, if the worker fetched it since launch. Written by a fetch's outcome,
    /// `fetch_first`, upkeep and a new clone.
    settled: Option<Settled>,
    /// The head the rules want, until `settled` meets it. Written by a hand-over and a fetch's outcome.
    want: Option<Intent>,
    /// Written by opening the PR, a sync step that saves an awaited head, and a fetch's outcome.
    ask: Option<Ask>,
}

/// Where the mirror's ref of a PR stands, with the Target branch of the fetch that left it there, which a
/// new clone of a broken mirror fetches again.
#[derive(Debug)]
struct Settled {
    commit: CommitSha,
    branch: String,
    found: Found,
}

#[derive(Debug, PartialEq, Eq)]
enum Found {
    Landed,
    /// GitHub had no ref at the retry either, so a hand-over doesn't fetch it until it moves on.
    NotOnGitHub,
}

/// A commit one side wants in the mirror, with the Target branch its fetch brings up to date.
#[derive(Debug)]
struct Intent {
    commit: CommitSha,
    branch: String,
    tries: Tries,
}

/// The fetches that found an intent's head elsewhere. A head is fetched once more after a moment, as GitHub
/// moves `refs/pull/N/head` a few seconds after a push (#201), and after that only when it is handed over or
/// asked for again, so a ref GitHub never moves doesn't keep a fetch going. The app hears of it after that
/// second fetch (#213).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tries {
    Fresh,
    Due(Instant),
    Retried,
}

/// A PR the user opened since launch. Each variant keeps the PR's ref though the PR is out of Reach.
#[derive(Debug)]
enum Ask {
    /// `await_head`: the session doesn't know the head yet.
    Awaited,
    /// `opened`, or a sync step saved a head the mirror holds: nothing to fetch.
    Held,
    /// `fetch_first`, or `Awaited` once a sync step saves a head the mirror lacks. It goes in a fetch of its
    /// own, ahead of the heads the rules want.
    Fetch(Intent),
}

/// The outcome of a fetch that succeeded, for the heads it asked for.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Noted {
    /// The PRs whose heads landed.
    pub(super) arrived: Vec<u32>,
    /// The PRs whose heads didn't land at their retry either, by why. For a PR the user opened, those of
    /// the fetches the user's head was in; for the others, those of the rules' heads.
    pub(super) stuck: BTreeMap<Unfetched, Vec<u32>>,
}

impl Intent {
    fn fresh(commit: CommitSha, branch: String) -> Self {
        Self {
            commit,
            branch,
            tries: Tries::Fresh,
        }
    }

    /// Whether the next fetch takes it: a fresh one, or one whose moment is over.
    fn due(&self, now: Instant) -> bool {
        match self.tries {
            Tries::Fresh => true,
            Tries::Due(at) => at <= now,
            Tries::Retried => false,
        }
    }

    fn head_of(&self, number: u32) -> PrHead {
        PrHead {
            number,
            commit: self.commit.clone(),
            target_branch: self.branch.clone(),
        }
    }

    /// Notes a fetch that found the head elsewhere. Says whether it was the retry. One whose moment wasn't
    /// over when the fetch started, which joined another side's fetch, takes nothing from it: GitHub may
    /// move the ref yet.
    fn missed(&mut self, retry: Retry) -> bool {
        match self.tries {
            Tries::Fresh => {
                self.tries = Tries::Due(retry.at);
                false
            }
            Tries::Due(at) if at > retry.started => false,
            Tries::Due(_) | Tries::Retried => {
                self.tries = Tries::Retried;
                true
            }
        }
    }
}

/// When a fetch that succeeded started, and when a head it didn't bring is fetched once more.
#[derive(Debug, Clone, Copy)]
pub(super) struct Retry {
    pub(super) started: Instant,
    pub(super) at: Instant,
}

impl Settled {
    /// An ask fetches whatever the worker noted before, so only a ref that landed at its commit meets it. A
    /// want is met by any record at its commit, as one GitHub has no ref for isn't fetched again.
    fn landed_at(&self, commit: &CommitSha) -> bool {
        self.found == Found::Landed && self.commit == *commit
    }
}

impl Ask {
    fn intent(&self) -> Option<&Intent> {
        match self {
            Self::Fetch(intent) => Some(intent),
            Self::Awaited | Self::Held => None,
        }
    }

    fn intent_mut(&mut self) -> Option<&mut Intent> {
        match self {
            Self::Fetch(intent) => Some(intent),
            Self::Awaited | Self::Held => None,
        }
    }
}

impl Head {
    fn is_empty(&self) -> bool {
        self.settled.is_none() && self.want.is_none() && self.ask.is_none()
    }

    /// The rules' want, which is never met: it goes once it is.
    fn want(&self) -> Option<&Intent> {
        self.want.as_ref()
    }

    fn unmet_ask(&self) -> Option<&Intent> {
        let landed =
            |intent: &Intent| (self.settled.as_ref()).is_some_and(|s| s.landed_at(&intent.commit));
        (self.ask.as_ref())
            .and_then(Ask::intent)
            .filter(|intent| !landed(intent))
    }

    /// Notes where the ref stands. The want goes once it is met, so a want kept is never met.
    fn settle(&mut self, settled: Settled) {
        if (self.want.as_ref()).is_some_and(|want| want.commit == settled.commit) {
            self.want = None;
        }
        self.settled = Some(settled);
    }

    /// Turns where the ref stood into a fresh want, as the ref went. A want the rules hold already stays.
    fn refetch_settled(&mut self) {
        if let Some(Settled { commit, branch, .. }) = self.settled.take()
            && self.want.is_none()
        {
            self.want = Some(Intent::fresh(commit, branch));
        }
    }

    /// Notes where the ref a fetch of `asked` brought was `found`, if it could be read. Says whether the head
    /// arrived, and why the app hears it didn't.
    fn note_fetched(
        &mut self,
        asked: &PrHead,
        found: Result<Option<&CommitSha>, &Unfetched>,
        retry: Retry,
    ) -> (bool, Option<Unfetched>) {
        let settled = |commit: &CommitSha, found| Settled {
            commit: commit.clone(),
            branch: asked.target_branch.clone(),
            found,
        };
        let (arrived, why) = match found {
            Ok(Some(at)) => {
                let arrived = self.arrives(at, &asked.commit);
                self.settle(settled(at, Found::Landed));
                if *at == asked.commit {
                    return (arrived, None);
                }
                (arrived, Unfetched::Behind)
            }
            // `--prune` deleted its ref, if it had one.
            Ok(None) => {
                self.settled = None;
                (false, Unfetched::NotOnGitHub)
            }
            Err(unread) => (false, unread.clone()),
        };
        // Each side at the commit asked for that the outcome doesn't meet moves on a try.
        let commit = &asked.commit;
        let user_asked = self.ask.is_some();
        let unmet_ask = self.unmet_ask().is_some();
        let want_spent = (self.want.as_mut())
            .filter(|want| want.commit == *commit)
            .is_some_and(|want| want.missed(retry));
        let ask_spent = (self.ask.as_mut())
            .and_then(Ask::intent_mut)
            .filter(|intent| unmet_ask && intent.commit == *commit)
            .is_some_and(|intent| intent.missed(retry));
        if (want_spent || ask_spent) && why == Unfetched::NotOnGitHub {
            self.settle(settled(commit, Found::NotOnGitHub));
        }
        let heard = if user_asked { ask_spent } else { want_spent };
        (arrived, heard.then_some(why))
    }

    /// Whether the app hears that a ref which landed at `at`, in a fetch of `asked`, brought the head. For
    /// a PR the user asked for, it hears only of their head, the first time it lands, wherever that fetch
    /// came from: the Files tab asks again for a head that arrived elsewhere. For the others, it hears of
    /// the commit asked for, and of the rules' head that the ref moved to during the fetch.
    fn arrives(&self, at: &CommitSha, asked: &CommitSha) -> bool {
        match self.ask.as_ref().map(Ask::intent) {
            Some(Some(_)) => self.unmet_ask().is_some_and(|intent| intent.commit == *at),
            Some(None) | None => {
                *at == *asked || (self.want.as_ref()).is_some_and(|want| want.commit == *at)
            }
        }
    }
}

impl Prs {
    /// Takes `heads` as the rules' want, which replaces the last: a PR it leaves out loses its want. A head
    /// the mirror holds needs no fetch. A head the rules want already keeps its tries, as a hand-over during
    /// its fetch or its moment's wait changes nothing, unless its retry is spent: a hand-over is a reason to
    /// try it again.
    pub(super) fn hand_over(&mut self, heads: Vec<PrHead>) {
        let handed: BTreeSet<u32> = heads.iter().map(|head| head.number).collect();
        for (number, head) in &mut self.heads {
            if !handed.contains(number) {
                head.want = None;
            }
        }
        for PrHead {
            number,
            commit,
            target_branch,
        } in heads
        {
            let head = self.heads.entry(number).or_default();
            match &mut head.want {
                Some(want) if want.commit == commit => {
                    want.branch = target_branch;
                    if want.tries == Tries::Retried {
                        want.tries = Tries::Fresh;
                    }
                }
                Some(_) | None => {
                    let met = (head.settled.as_ref()).is_some_and(|s| s.commit == commit);
                    head.want = (!met).then(|| Intent::fresh(commit, target_branch));
                }
            }
        }
        self.handed = Some(handed);
        self.tidy();
    }

    /// Keeps PR `number`'s ref until the next launch: the user opened it, and the mirror holds its head.
    pub(super) fn opened(&mut self, number: u32) {
        let head = self.heads.entry(number).or_default();
        if head.ask.is_none() {
            head.ask = Some(Ask::Held);
        }
    }

    /// Notes PR `number` opened before the session knew its head, whose head goes first once a sync step
    /// saves it. A head the worker was asked to fetch keeps its place.
    pub(super) fn await_head(&mut self, number: u32) {
        let head = self.heads.entry(number).or_default();
        if !matches!(head.ask, Some(Ask::Fetch(_))) {
            head.ask = Some(Ask::Awaited);
        }
    }

    /// PR `number` awaits its head no more, as no read names it. Its ref stays.
    pub(super) fn no_head(&mut self, number: u32) {
        if let Some(head) = self.heads.get_mut(&number)
            && matches!(head.ask, Some(Ask::Awaited))
        {
            head.ask = Some(Ask::Held);
        }
    }

    /// Puts the head of a PR the user opened in a fetch of its own, whatever the worker noted of it: the app
    /// looked in the mirror and found it missing. A ref the worker noted at another commit still stands
    /// there, so a hand-over of that commit needs no fetch.
    pub(super) fn fetch_first(&mut self, head: PrHead) {
        let known = self.heads.entry(head.number).or_default();
        if (known.settled.as_ref()).is_some_and(|settled| settled.commit == head.commit) {
            known.settled = None;
        }
        known.ask = Some(Ask::Fetch(Intent::fresh(head.commit, head.target_branch)));
    }

    /// Asks for the head of each PR of `heads` that awaited it, unless the mirror fetched that head since
    /// launch. Says whether it asked for one.
    pub(super) fn saved(&mut self, heads: Vec<PrHead>) -> bool {
        let mut asked = false;
        for PrHead {
            number,
            commit,
            target_branch,
        } in heads
        {
            let Some(head) = self.heads.get_mut(&number) else {
                continue;
            };
            if !matches!(head.ask, Some(Ask::Awaited)) {
                continue;
            }
            let held = (head.settled.as_ref()).is_some_and(|s| s.landed_at(&commit));
            head.ask = Some(if held {
                Ask::Held
            } else {
                asked = true;
                Ask::Fetch(Intent::fresh(commit, target_branch))
            });
        }
        asked
    }

    /// The heads the user asked for that the next fetch of their own takes.
    pub(super) fn demanded(&self, now: Instant) -> impl Iterator<Item = PrHead> + '_ {
        self.due(now, Head::unmet_ask)
            .map(|(number, intent)| intent.head_of(number))
    }

    /// The heads the rules want that the next fetch of them takes.
    pub(super) fn wanted(&self, now: Instant) -> impl Iterator<Item = PrHead> + '_ {
        self.due(now, Head::want)
            .map(|(number, intent)| intent.head_of(number))
    }

    /// Whether `demanded` has a head.
    pub(super) fn demands(&self, now: Instant) -> bool {
        self.due(now, Head::unmet_ask).next().is_some()
    }

    /// Whether `wanted` has a head.
    pub(super) fn wants(&self, now: Instant) -> bool {
        self.due(now, Head::want).next().is_some()
    }

    /// The intents of one side, `side` picks, that the next fetch takes, by PR.
    fn due(
        &self,
        now: Instant,
        side: fn(&Head) -> Option<&Intent>,
    ) -> impl Iterator<Item = (u32, &Intent)> {
        (self.heads.iter()).filter_map(move |(number, head)| {
            Some((*number, side(head).filter(|intent| intent.due(now))?))
        })
    }

    /// Whether a head the user asked for waits for a fetch, now or once its moment is over.
    pub(super) fn asks_wait(&self) -> bool {
        (self.heads.values())
            .filter_map(Head::unmet_ask)
            .any(|intent| intent.tries != Tries::Retried)
    }

    /// When the first head whose moment isn't over yet is due to be fetched once more.
    pub(super) fn next_due(&self, now: Instant) -> Option<Instant> {
        let unmet =
            (self.heads.values()).flat_map(|head| head.unmet_ask().into_iter().chain(head.want()));
        unmet
            .filter_map(|intent| match intent.tries {
                Tries::Due(at) => (at > now).then_some(at),
                Tries::Fresh | Tries::Retried => None,
            })
            .min()
    }

    /// Notes where the heads of `batch`, a fetch that succeeded, `landed`, or why that couldn't be read.
    /// The outcome speaks for each head at the commit the batch asked for, whichever side wants it and
    /// whether it was in the batch or came during the fetch. A side at another commit takes nothing from
    /// it, apart from a ref that landed at its commit. A head that didn't land is fetched once more as
    /// `retry` says.
    pub(super) fn note_fetched(
        &mut self,
        batch: &[PrHead],
        landed: Result<&BTreeMap<u32, CommitSha>, &Unfetched>,
        retry: Retry,
    ) -> Noted {
        let mut noted = Noted::default();
        for asked in batch {
            let head = self.heads.entry(asked.number).or_default();
            let found = landed.map(|landed| landed.get(&asked.number));
            let (arrived, stuck) = head.note_fetched(asked, found, retry);
            if arrived {
                noted.arrived.push(asked.number);
            }
            if let Some(why) = stuck {
                noted.stuck.entry(why).or_default().push(asked.number);
            }
        }
        self.tidy();
        noted
    }

    /// Takes `kept` as the PRs out of Reach whose refs upkeep keeps, which replaces the last.
    pub(super) fn keep(&mut self, kept: Vec<u32>) {
        self.kept = kept.into_iter().collect();
    }

    /// Says of a PR whether the last hand-over didn't reach it, the rules don't keep its ref and the user
    /// hasn't opened it since launch. None before the first hand-over, which says what's in Reach.
    pub(super) fn out_of_reach(&self) -> Option<impl Fn(&u32) -> bool + '_> {
        let handed = self.handed.as_ref()?;
        Some(|number: &u32| {
            !handed.contains(number) && !self.kept.contains(number) && !self.user_opened(*number)
        })
    }

    fn user_opened(&self, number: u32) -> bool {
        (self.heads.get(&number)).is_some_and(|head| head.ask.is_some())
    }

    /// Forgets where the refs of `pruned` stood, as upkeep deleted them: a PR that comes back in Reach has
    /// its head fetched again. One that came back, or that the user opened, while the refs went is fetched
    /// again at once.
    pub(super) fn pruned(&mut self, pruned: &[u32]) {
        for number in pruned {
            let kept = (self.handed.as_ref()).is_some_and(|handed| handed.contains(number))
                || self.user_opened(*number);
            let Some(head) = self.heads.get_mut(number) else {
                continue;
            };
            if kept {
                head.refetch_settled();
            } else {
                head.settled = None;
            }
        }
        self.tidy();
    }

    /// Fetches again each head fetched since launch and each head a side wants, at once, into a new clone
    /// of a broken mirror.
    pub(super) fn fetch_again(&mut self) {
        for head in self.heads.values_mut() {
            head.refetch_settled();
            let ask = head.ask.as_mut().and_then(Ask::intent_mut);
            for intent in head.want.as_mut().into_iter().chain(ask) {
                intent.tries = Tries::Fresh;
            }
        }
    }

    fn tidy(&mut self) {
        self.heads.retain(|_, head| !head.is_empty());
    }
}
