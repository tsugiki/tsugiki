//! Needs attention by number: what on each Issue and PR waits on the user, since when, and why. Its words are
//! the glossary's, and the window turns each `Why` into the words an Alert would use. `workbench` sums them
//! over each item's tree.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use specta::Type;
use time::OffsetDateTime;

use super::reasons::{WorkReason, reasons};
use super::review_state::{SharedReviews, YourReview, can_wait_on, kept_finding, your_reviews};
use crate::activity::{NewActivity, News, WhatHappened, has_failed};
use crate::conventions::Conventions;
use crate::facts::WorkFacts;
use crate::facts::activity::{Activity, ActivityFacts, ActivityKind, Archive, ItemKey};
use crate::facts::issues::{IssueFacts, IssueState};
use crate::facts::patch_ids::{StaleFinding, StaleFound};
use crate::facts::pull_requests::{CiState, LatestReview, PrState, PullRequestFacts, ReviewState};
use crate::login::Login;

/// One reason an item Needs attention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Why {
    ReviewRequested,
    /// The user was assigned to an Issue after they muted it (#492).
    Assigned,
    /// The PR's diff changed since the user's Review. The row names the files.
    StaleReview,
    /// Empty `checks` means Tsugiki hasn't heard which ones failed, or GitHub names none.
    CiFailed {
        checks: Vec<String>,
    },
    CiPassed,
    ChangesRequested {
        by: Login,
    },
    Approved {
        by: Login,
    },
    Reviewed {
        by: Login,
    },
    Comment {
        by: Login,
    },
    Commits {
        by: Login,
    },
    ReadyForReview {
        by: Login,
    },
    Mentioned,
    BackportFailed {
        target: String,
    },
    /// The user marked the item unseen.
    MarkedUnseen,
}

/// The numbers Mark Seen marks, the moment it marks them up to, and the Stale reviews it acknowledges, as
/// the window drew them. The window sends it back as it got it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SeenMark {
    /// The item, and for an Issue every PR in its tree.
    pub numbers: Vec<u32>,
    /// The moment Last seen moves on to: the newest the window drew of GitHub's last update, CI reaching a
    /// result and each event on them. Whatever a later sync brings stays new. Nothing for numbers the facts
    /// don't hold, and Last seen then stays.
    #[serde(with = "time::serde::rfc3339::option")]
    #[specta(type = Option<String>)]
    pub up_to: Option<OffsetDateTime>,
    /// The finding behind each Stale review of `numbers` that waits on the user, as the window drew it. The
    /// mark acknowledges these alone, so a check that finds a Stale review after the window drew the page
    /// still counts (#908).
    pub findings: Vec<StaleFinding>,
}

/// One reason an Issue or PR Needs attention, from the moment it started to.
#[derive(Debug, Clone)]
pub(crate) struct Cause {
    pub(crate) at: OffsetDateTime,
    pub(crate) why: Why,
}

/// The causes that make each of a Repository's Issues and PRs Need attention, and the user's part in each.
#[derive(Debug, Default)]
pub(crate) struct Causes {
    by_number: BTreeMap<u32, Vec<Cause>>,
    /// The PRs that carry a Review request, acknowledged or not, which the Review requests view holds.
    pub(crate) review_requests: BTreeSet<u32>,
    /// The PRs whose Stale review waits on the user, which the Stale reviews view holds. It Needs attention
    /// only until the user acknowledges it.
    pub(crate) stale_reviews: BTreeSet<u32>,
    /// The finding the store kept behind each of `stale_reviews`, where it kept one.
    stale_found: BTreeMap<u32, StaleFound>,
    /// The Issues and PRs with New activity, whoever it waits on.
    with_news: BTreeSet<u32>,
    /// The newest moment the facts hold on each Issue and PR: GitHub's last update, CI reaching a result, or
    /// an event.
    known_until: BTreeMap<u32, OffsetDateTime>,
    /// The Issues and PRs the user marked unseen.
    marked_unseen: BTreeSet<u32>,
    /// Where the user's Review of each PR stands. A PR missing here is Unreviewed.
    reviews: BTreeMap<u32, YourReview>,
    /// The items the user archived, and those archived as they closed (#820).
    archived: BTreeMap<ItemKey, Archive>,
    /// The Issues and PRs closed or merged on GitHub.
    closed: BTreeSet<u32>,
    /// The items the user muted on their own, each with the moment Mute marked it seen up to.
    muted: BTreeMap<ItemKey, OffsetDateTime>,
    /// The items a Muted author opened, each with the moment of the author's mute (#494).
    by_muted_authors: BTreeMap<ItemKey, OffsetDateTime>,
    /// The Repository's Muted authors.
    muted_authors: Vec<Login>,
    /// The user's Pinned issues (#488).
    pinned: BTreeSet<u32>,
    /// The user signed in, whose own items Mute author leaves alone.
    viewer: Option<Login>,
    /// The Review request or the assignment to the user that stands on each Issue and PR, acknowledged or
    /// not.
    asks: BTreeMap<u32, Ask>,
    /// The moment the user last acknowledged each Issue and PR.
    acknowledged: BTreeMap<u32, OffsetDateTime>,
}

/// A Review request or an assignment to the user that stands on an Issue or PR.
#[derive(Debug, Clone)]
struct Ask {
    /// `Why::ReviewRequested` on a PR, which covers an assignment to someone else's PR, and `Why::Assigned`
    /// on an Issue.
    why: Why,
    /// Its start, if the timeline still holds it.
    started: Option<OffsetDateTime>,
}

impl Causes {
    /// No viewer means nobody has signed in yet, so nothing waits on anyone.
    pub(crate) fn of(facts: &WorkFacts, conventions: Conventions) -> Self {
        let Some(viewer) = facts.viewer.as_ref() else {
            return Self::default();
        };
        let shared = SharedReviews::of(&facts.pull_requests, conventions);
        let reviews = your_reviews(facts, viewer, &shared);
        let stale_reviews = stale_reviews(facts, viewer, &reviews, &shared);
        let stale_found = stale_found(facts, &reviews, &stale_reviews);
        let judge = Judge {
            news: News::new(&facts.activity, &facts.pull_requests, viewer, conventions),
            facts: &facts.activity,
            viewer,
            stale_reviews: &stale_reviews,
            stale_found: &stale_found,
        };
        let on_pull_requests = facts.pull_requests.iter().map(|pr| {
            let news = judge.news.on_pull_request(pr);
            (pr.number, judge.on_pull_request(pr, &news), news)
        });
        let on_issues = facts.issues.iter().map(|issue| {
            let news = judge.news.on_issue(issue.number);
            (issue.number, judge.on_issue(issue, &news), news)
        });
        let mut with_news = BTreeSet::new();
        let mut by_number = BTreeMap::new();
        for (number, causes, news) in on_pull_requests.chain(on_issues) {
            with_news.extend((!news.is_empty()).then_some(number));
            if !causes.is_empty() {
                by_number.extend([(number, causes)]);
            }
        }
        let requested: BTreeSet<u32> = (facts.pull_requests.iter())
            .filter(|pr| reasons(pr, viewer).contains(&WorkReason::ReviewRequest))
            .map(|pr| pr.number)
            .collect();
        // GitHub keeps a closed Issue's assignees, but a closed Issue asks nothing of them.
        let assigned = (facts.issues.iter())
            .filter(|issue| matches!(issue.state, Some(IssueState::Open) | None))
            .filter(|issue| issue.assignees.contains(viewer))
            .map(|issue| (issue.number, Why::Assigned));
        let asks: BTreeMap<u32, Ask> = (requested.iter())
            .map(|number| (*number, Why::ReviewRequested))
            .chain(assigned)
            .map(|(number, why)| {
                let started = judge.asked_at(number);
                (number, Ask { why, started })
            })
            .collect();
        Self {
            by_number,
            asks,
            review_requests: requested,
            stale_reviews,
            stale_found,
            with_news,
            known_until: known_until(facts),
            marked_unseen: facts.activity.marked_unseen.keys().copied().collect(),
            reviews,
            archived: facts.activity.archived.clone(),
            closed: closed(facts),
            muted: facts.activity.muted.clone(),
            by_muted_authors: by_muted_authors(facts),
            muted_authors: (facts.activity.muted_authors.iter())
                .map(|muted| muted.login.clone())
                .collect(),
            pinned: facts.activity.pinned.clone(),
            viewer: Some(viewer.clone()),
            acknowledged: facts.activity.acknowledged.clone(),
        }
    }

    /// Whether the item `key` names is Muted: the user muted it on its own (#492), or its author (#494).
    pub(crate) fn is_muted(&self, key: ItemKey) -> bool {
        self.muted_at(key).is_some()
    }

    /// Whether the user muted the item `key` names on its own (#492), whatever they did to its author.
    pub(crate) fn is_muted_on_its_own(&self, key: ItemKey) -> bool {
        self.muted.contains_key(&key)
    }

    /// Whether the user muted the author of the item `key` names (#494).
    pub(crate) fn is_by_muted_author(&self, key: ItemKey) -> bool {
        self.by_muted_authors.contains_key(&key)
    }

    /// Whether the user muted `author` in this Repository (#494).
    pub(crate) fn is_muted_author(&self, author: &Login) -> bool {
        self.muted_authors.contains(author)
    }

    /// Whether Issue `number` is a Pinned issue (#488).
    pub(crate) fn is_pinned(&self, issue: u32) -> bool {
        self.pinned.contains(&issue)
    }

    /// The user signed in, if anyone has.
    pub(crate) fn viewer(&self) -> Option<&Login> {
        self.viewer.as_ref()
    }

    /// The Muted items, on their own or a Muted author's (#492, #494).
    pub(crate) fn muted(&self) -> BTreeSet<ItemKey> {
        let keys = self.muted.keys().chain(self.by_muted_authors.keys());
        keys.copied().collect()
    }

    /// The moment the mute of the item `key` names began, if it is Muted: the later of its own mute and its
    /// author's, as each marked what stood then seen.
    fn muted_at(&self, key: ItemKey) -> Option<OffsetDateTime> {
        let own = self.muted.get(&key);
        own.into_iter()
            .chain(self.by_muted_authors.get(&key))
            .max()
            .copied()
    }

    /// The reasons the item `key` names, whose tree holds `numbers`, Needs attention while the user has it
    /// muted (#492): none when they don't. Whatever else happens there, only a Review request or an
    /// assignment to the user reaches them. Mute acknowledged those that stood, so only one that started
    /// after it, and that the user hasn't acknowledged since, counts. A start the timeline no longer holds
    /// counts as earlier, as `Judge::standing_from` counts it.
    pub(crate) fn fresh_asks(&self, key: ItemKey, numbers: &[u32]) -> Vec<Cause> {
        (self.fresh(key, numbers).into_iter())
            .map(|(_, cause)| cause)
            .collect()
    }

    /// The numbers of `numbers`, the tree of the item `key` names, that Need attention. While the user has
    /// the item muted, those are the ones `fresh_asks` names.
    pub(crate) fn needing_attention_in(&self, key: ItemKey, numbers: &[u32]) -> BTreeSet<u32> {
        if self.is_muted(key) {
            (self.fresh(key, numbers).into_iter())
                .map(|(number, _)| number)
                .collect()
        } else {
            (numbers.iter().copied())
                .filter(|&number| self.needs_attention(number))
                .collect()
        }
    }

    /// Each of `numbers` that `fresh_asks` counts, with its cause: none unless the user muted the item.
    fn fresh(&self, key: ItemKey, numbers: &[u32]) -> Vec<(u32, Cause)> {
        let fresh = numbers.iter().filter_map(|&number| {
            let muted = self.muted_at(key)?;
            let ask = self.asks.get(&number)?;
            let acknowledged = self
                .acknowledged
                .get(&number)
                .map_or(muted, |at| muted.max(*at));
            let at = ask.started.filter(|at| *at > acknowledged)?;
            let why = ask.why.clone();
            Some((number, Cause { at, why }))
        });
        fresh.collect()
    }

    /// The Issues and PRs where a Review request or an assignment to the user stands, acknowledged or not.
    pub(crate) fn asked(&self) -> BTreeSet<u32> {
        self.asks.keys().copied().collect()
    }

    /// Whether the item `key` names, whose tree holds `numbers`, is Archived (#490). The user archived it,
    /// or it closed (#820), and it stays put away until something in its tree Needs attention again. An
    /// open item then comes back, and stays back once the user marks it seen past the moment of the archive,
    /// as Mark Seen moves the moment they acknowledged it on. A closed one goes back to Archived once nothing
    /// in its tree Needs attention, as nothing else would show it.
    pub(crate) fn is_archived(&self, key: ItemKey, numbers: &[u32]) -> bool {
        let Some(archive) = self.archived.get(&key) else {
            return false;
        };
        let closed = self.closed.contains(&key.number());
        numbers.iter().all(|number| {
            let acknowledged = self.acknowledged.get(number);
            let seen = closed || acknowledged.is_none_or(|at| *at <= archive.seen_up_to);
            !self.needs_attention(*number) && seen
        })
    }

    /// When the user last put away the item `key` names, if they archived it, which orders the Archived view
    /// (#491).
    pub(crate) fn archived_at(&self, key: ItemKey) -> Option<OffsetDateTime> {
        self.archived.get(&key).map(|archive| archive.archived_at)
    }

    /// Where the user's Review of PR `number` stands.
    pub(crate) fn review_of(&self, number: u32) -> YourReview {
        let review = self.reviews.get(&number).cloned();
        review.unwrap_or(YourReview::Unreviewed)
    }

    /// True when Issue or PR `number` itself Needs attention.
    pub(crate) fn needs_attention(&self, number: u32) -> bool {
        self.by_number.contains_key(&number)
    }

    /// The Issues and PRs that themselves Need attention.
    pub(crate) fn waiting(&self) -> impl Iterator<Item = u32> + '_ {
        self.by_number.keys().copied()
    }

    /// The causes that make Issue or PR `number` itself Need attention: none when it doesn't.
    pub(crate) fn on(&self, number: u32) -> &[Cause] {
        self.by_number.get(&number).map_or(&[], Vec::as_slice)
    }

    /// True when Issue or PR `number` has New activity, whoever it waits on.
    pub(crate) fn has_news(&self, number: u32) -> bool {
        self.with_news.contains(&number)
    }

    /// True when the user marked Issue or PR `number` unseen.
    pub(crate) fn is_marked_unseen(&self, number: u32) -> bool {
        self.marked_unseen.contains(&number)
    }

    /// The newest moment the facts hold on Issue or PR `number`, which Mark Seen moves Last seen to.
    pub(crate) fn known_until(&self, number: u32) -> Option<OffsetDateTime> {
        self.known_until.get(&number).copied()
    }

    /// The finding behind each Stale review of `numbers` that waits on the user, where the store kept one,
    /// for a mark drawn now to acknowledge.
    pub(crate) fn findings_in(&self, numbers: &[u32]) -> Vec<StaleFinding> {
        let found = numbers.iter().filter_map(|number| {
            let found = self.stale_found.get(number)?;
            Some(found.on(*number))
        });
        found.collect()
    }
}

/// The PRs whose Stale review waits on the user: each open PR where `viewer`'s Review is a Stale review. A
/// merged or closed PR waits on nobody, whatever its last check found, and nor does a colleague's Draft PR.
/// A Bot backport's Stale review is its Origin PR's, and counts only on the Origin PR, so not at all once
/// that has merged.
fn stale_reviews(
    facts: &WorkFacts,
    viewer: &Login,
    reviews: &BTreeMap<u32, YourReview>,
    shared: &SharedReviews,
) -> BTreeSet<u32> {
    let waiting = facts.pull_requests.iter().filter(|pr| {
        let borrowed = shared.origin_of(pr.number).is_some();
        let stale = reviews.get(&pr.number).is_some_and(YourReview::is_stale);
        can_wait_on(pr, viewer) && !borrowed && stale
    });
    waiting.map(|pr| pr.number).collect()
}

/// The finding the store kept of each of `stale_reviews`, where it kept the one the PR shows, as
/// `kept_finding` matches it (#908).
fn stale_found(
    facts: &WorkFacts,
    reviews: &BTreeMap<u32, YourReview>,
    stale_reviews: &BTreeSet<u32>,
) -> BTreeMap<u32, StaleFound> {
    let shown = (facts.pull_requests.iter()).filter(|pr| stale_reviews.contains(&pr.number));
    let found = shown.filter_map(|pr| {
        let found = kept_finding(reviews.get(&pr.number)?, pr, &facts.reviews)?;
        Some((pr.number, found.clone()))
    });
    found.collect()
}

/// The Issues and PRs `facts` hold closed or merged. An Issue saved before Tsugiki kept states counts as
/// open, as it does for an assignment.
fn closed(facts: &WorkFacts) -> BTreeSet<u32> {
    let pull_requests = (facts.pull_requests.iter())
        .filter(|pr| pr.state != PrState::Open)
        .map(|pr| pr.number);
    let issues = (facts.issues.iter())
        .filter(|issue| !matches!(issue.state, Some(IssueState::Open) | None))
        .map(|issue| issue.number);
    pull_requests.chain(issues).collect()
}

/// The newest moment `facts` hold on each Issue and PR, which Mark Seen moves Last seen to. It covers what
/// isn't New activity yet, such as CI on a PR the user hasn't reviewed, so that stays seen once it would be.
fn known_until(facts: &WorkFacts) -> BTreeMap<u32, OffsetDateTime> {
    let updated = facts
        .pull_requests
        .iter()
        .map(|pr| (pr.number, pr.updated_at))
        .chain(
            facts
                .issues
                .iter()
                .map(|issue| (issue.number, issue.updated_at)),
        );
    let activity = &facts.activity;
    let events = activity
        .events
        .iter()
        .flat_map(|(number, events)| events.iter().map(|event| (*number, event.at)));
    let ci = activity
        .ci_finished
        .iter()
        .map(|(number, at)| (*number, *at));
    let mut newest = BTreeMap::new();
    for (number, at) in updated.chain(events).chain(ci) {
        let known = newest.entry(number).or_insert(at);
        *known = (*known).max(at);
    }
    newest
}

/// The items a Muted author opened (#494): each Issue and PR `facts` hold whose author the user muted, with
/// the moment of the mute, those they open after it too. A PR inside another author's Issue is no item, so
/// nothing reads its key, and it stays as it is. The user's own items never count: a mute of their login,
/// made under another account in the same data folder, would hide them where nothing offers Unmute author.
fn by_muted_authors(facts: &WorkFacts) -> BTreeMap<ItemKey, OffsetDateTime> {
    let muted = &facts.activity.muted_authors;
    if muted.is_empty() {
        return BTreeMap::new();
    }
    let mute_of = |author: Option<&Login>| {
        let author = author.filter(|author| facts.viewer.as_ref() != Some(*author))?;
        let found = muted.iter().find(|muted| muted.login == *author);
        found.map(|muted| muted.at)
    };
    let issues = (facts.issues.iter()).filter_map(|issue| {
        let at = mute_of(issue.author.as_ref())?;
        Some((
            ItemKey::Issue {
                number: issue.number,
            },
            at,
        ))
    });
    let pull_requests = (facts.pull_requests.iter()).filter_map(|pr| {
        let at = mute_of(pr.author.as_ref())?;
        Some((ItemKey::PullRequest { number: pr.number }, at))
    });
    issues.chain(pull_requests).collect()
}

/// The Muted items `facts` hold, those the user muted on their own and those of Muted authors (#494), as
/// `Causes::muted` names them, for a rule that reads the facts before the rest of `Causes` is built.
pub(crate) fn muted_items(facts: &WorkFacts) -> BTreeSet<ItemKey> {
    let own = facts.activity.muted.keys().copied();
    own.chain(by_muted_authors(facts).into_keys()).collect()
}

/// Judges what on each Issue and PR waits on the user.
struct Judge<'a> {
    news: News<'a>,
    facts: &'a ActivityFacts,
    viewer: &'a Login,
    stale_reviews: &'a BTreeSet<u32>,
    stale_found: &'a BTreeMap<u32, StaleFound>,
}

impl Judge<'_> {
    fn events_of(&self, number: u32) -> impl Iterator<Item = &Activity> {
        self.facts.events.get(&number).into_iter().flatten()
    }

    /// True when the user has commented on item `number`.
    fn has_commented(&self, number: u32) -> bool {
        self.events_of(number)
            .any(|event| event.kind == ActivityKind::Comment && event.who == *self.viewer)
    }

    /// The moment of the newest event on item `number` that `matches`, if the facts hold one.
    fn last_at(&self, number: u32, matches: impl Fn(&Activity) -> bool) -> Option<OffsetDateTime> {
        let found = self.events_of(number).filter(|event| matches(event));
        found.map(|event| event.at).max()
    }

    /// The moment the user was last asked to review PR `number`, or assigned to Issue or PR `number`, if its
    /// timeline still holds it.
    fn asked_at(&self, number: u32) -> Option<OffsetDateTime> {
        self.last_at(number, |event| {
            let kind = matches!(
                event.kind,
                ActivityKind::ReviewRequested | ActivityKind::Assigned
            );
            kind && event.who == *self.viewer
        })
    }

    /// The moment Tsugiki noticed the last push to PR `number`, which the store keeps apart from the
    /// commit's own date.
    fn pushed_at(&self, number: u32) -> Option<OffsetDateTime> {
        self.last_at(number, |event| event.kind == ActivityKind::Commit)
    }

    /// `why` as a cause on `pull_request` from `started`, or none once the user has acknowledged it (#865).
    /// A start Tsugiki doesn't know counts as earlier than any acknowledgement, since what starts a cause
    /// anew, a request, a push or a failed run, leaves its moment in the facts. Such a cause counts from the
    /// PR's last update.
    fn standing_from(
        &self,
        pull_request: &PullRequestFacts,
        started: Option<OffsetDateTime>,
        why: Why,
    ) -> Option<Cause> {
        let acknowledged = self.facts.acknowledged.get(&pull_request.number);
        let before = |acknowledged: &OffsetDateTime| started.is_none_or(|at| at <= *acknowledged);
        (!acknowledged.is_some_and(before)).then(|| Cause {
            at: started.unwrap_or(pull_request.updated_at),
            why,
        })
    }

    /// The Stale review on `pull_request`, from the push or from when the check found it, whichever came
    /// later. The window shows none before the check finds it, however long after the push that comes
    /// (#908). So it stands until a mark acknowledges the finding the window drew, and marks up to the push.
    /// A Stale review whose finding the store couldn't keep goes by the push alone, as before Tsugiki kept
    /// findings, so Mark Seen still clears it on a store that can't write.
    fn stale_review(&self, pull_request: &PullRequestFacts) -> Option<Cause> {
        let pushed = self.pushed_at(pull_request.number);
        let cause = self.standing_from(pull_request, pushed, Why::StaleReview);
        let Some(found) = self.stale_found.get(&pull_request.number) else {
            return cause;
        };
        let at = pushed.unwrap_or(pull_request.updated_at).max(found.at);
        (cause.is_some() || !found.acknowledged).then_some(Cause {
            at,
            why: Why::StaleReview,
        })
    }

    fn failed_checks_of(&self, number: u32) -> &[String] {
        let checks = self.facts.failed_checks.get(&number);
        checks.map_or(&[], Vec::as_slice)
    }

    /// The causes that wait on the user until they acknowledge them, whatever Last seen says: a Review
    /// request, and failed CI on an Own PR.
    fn standing(&self, pull_request: &PullRequestFacts) -> Vec<Cause> {
        let number = pull_request.number;
        let asked = reasons(pull_request, self.viewer).contains(&WorkReason::ReviewRequest);
        let requested = (asked.then_some(Why::ReviewRequested))
            .and_then(|why| self.standing_from(pull_request, self.asked_at(number), why));
        let own = pull_request.author.as_ref() == Some(self.viewer);
        let open = pull_request.state == PrState::Open;
        let failed = (own && open && has_failed(pull_request.ci)).then(|| Why::CiFailed {
            checks: self.failed_checks_of(number).to_vec(),
        });
        let failed =
            failed.and_then(|why| self.standing_from(pull_request, self.failed_at(number), why));
        requested.into_iter().chain(failed).collect()
    }

    /// The moment PR `number`'s CI failed. The store moves its moment only when CI changes state, so a run
    /// that fails again after a push, with no pending state synced in between, dates from the push: GitHub
    /// reports CI on the head alone.
    fn failed_at(&self, number: u32) -> Option<OffsetDateTime> {
        let finished = self.facts.ci_finished.get(&number).copied();
        finished.max(self.pushed_at(number))
    }

    /// Marked unseen, from the moment of the flag, whatever the user's part in the item.
    fn flag(&self, number: u32) -> Option<Cause> {
        let at = *self.facts.marked_unseen.get(&number)?;
        Some(Cause {
            at,
            why: Why::MarkedUnseen,
        })
    }

    fn on_pull_request(&self, pull_request: &PullRequestFacts, news: &[NewActivity]) -> Vec<Cause> {
        let number = pull_request.number;
        let mut causes = self.standing(pull_request);
        causes.extend(self.flag(number));
        let own = pull_request.author.as_ref() == Some(self.viewer);
        // A colleague's Draft PR needs attention only when it carries a Review request.
        if pull_request.is_draft && !own {
            return causes;
        }
        if self.stale_reviews.contains(&number) {
            causes.extend(self.stale_review(pull_request));
        }
        let involvement = if pull_request.is_written_or_reviewed_by(self.viewer) {
            Involvement::WroteOrReviewed
        } else if pull_request.assignees.contains(self.viewer) || self.has_commented(number) {
            Involvement::InConversation
        } else {
            Involvement::Follows
        };
        let counted = news.iter().filter(|new| involvement.counts(&new.what));
        causes.extend(counted.map(|new| Cause {
            at: new.at,
            why: why_of(
                new.what.clone(),
                &pull_request.latest_reviews,
                self.failed_checks_of(number),
            ),
        }));
        causes
    }

    fn on_issue(&self, issue: &IssueFacts, news: &[NewActivity]) -> Vec<Cause> {
        let involvement = if issue.author.as_ref() == Some(self.viewer)
            || issue.assignees.contains(self.viewer)
            || self.has_commented(issue.number)
        {
            Involvement::InConversation
        } else {
            Involvement::Follows
        };
        let counted = news.iter().filter(|new| involvement.counts(&new.what));
        let causes = counted.map(|new| Cause {
            at: new.at,
            why: why_of(new.what.clone(), &[], &[]),
        });
        causes.chain(self.flag(issue.number)).collect()
    }
}

/// The user's part in an item, which decides what on it waits on them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Involvement {
    /// They wrote the PR or have a Review on it.
    WroteOrReviewed,
    /// They opened the Issue, are assigned to the item or commented on it.
    InConversation,
    /// It's only on their list. A mention still reaches them.
    Follows,
}

impl Involvement {
    /// True when `what` waits on a user who comes into the item this way.
    fn counts(self, what: &WhatHappened) -> bool {
        match what {
            WhatHappened::Mentioned => true,
            WhatHappened::Comment(_) => self != Self::Follows,
            WhatHappened::Commit(_)
            | WhatHappened::Review(_)
            | WhatHappened::ReadyForReview(_)
            | WhatHappened::BackportConflict { .. }
            | WhatHappened::BackportCiFailed { .. }
            | WhatHappened::CiFinished(_) => self == Self::WroteOrReviewed,
        }
    }
}

fn why_of(what: WhatHappened, reviews: &[LatestReview], checks: &[String]) -> Why {
    match what {
        WhatHappened::Commit(by) => Why::Commits { by },
        WhatHappened::Comment(by) => Why::Comment { by },
        WhatHappened::Review(by) => {
            let review = reviews.iter().find(|review| review.author == by);
            match review.map(|review| review.state) {
                Some(ReviewState::ChangesRequested) => Why::ChangesRequested { by },
                Some(ReviewState::Approved) => Why::Approved { by },
                Some(ReviewState::Commented | ReviewState::Dismissed | ReviewState::Pending)
                | None => Why::Reviewed { by },
            }
        }
        WhatHappened::ReadyForReview(by) => Why::ReadyForReview { by },
        WhatHappened::BackportConflict { target }
        | WhatHappened::BackportCiFailed { target, .. } => Why::BackportFailed { target },
        WhatHappened::CiFinished(CiState::Success) => Why::CiPassed,
        WhatHappened::CiFinished(
            CiState::Failure | CiState::Error | CiState::Expected | CiState::Pending,
        ) => Why::CiFailed {
            checks: checks.to_vec(),
        },
        WhatHappened::Mentioned => Why::Mentioned,
    }
}
