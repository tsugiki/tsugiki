//! The Issues a PR names and the Origin PR a Backport comes from, worked out from what GitHub says now and
//! read through the Repository's Conventions (ADR 0011). A number here can still turn out to be a PR, or to
//! name nothing, so only a number that the sync fetched as an Issue fixes one.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use specta::Type;

pub use rules::{FixedReader, LinkRule, Reading};

use crate::conventions::Conventions;
use crate::facts::CommitSha;
use crate::facts::pull_requests::PullRequestFacts;
use crate::login::Login;
use crate::repository::RepositoryName;
use crate::repository_settings::RepositorySettings;

/// A Backport's kind: it shares its Origin PR's review state, or it gets a review of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BackportKind {
    /// A backport bot opened it and committed every commit but merges.
    Bot,
    /// A person opened it, or someone else pushed to it.
    Manual,
}

/// The facts that make a PR a Backport. A grouping that only places PRs, and draws no rows, has no `kind`
/// to give and gives `()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackportLink<K = BackportKind> {
    /// The number the PR names. Tsugiki may not have that PR, or it may turn out not to be one.
    pub origin: u32,
    pub kind: K,
}

/// The merged PRs Tsugiki has, by the commit each became, for the cherry-pick lines to find. How it finds
/// one is `LinkRule::CherryPickLine`'s, as the store's `Keeper` finds one, so a change to it is a new rule.
pub struct MergeCommits(BTreeMap<CommitSha, u32>);

impl MergeCommits {
    pub fn of(pull_requests: &[PullRequestFacts]) -> Self {
        let merged = pull_requests
            .iter()
            .filter_map(|pr| Some((pr.merge_commit.clone()?, pr.number)));
        Self(merged.collect())
    }

    /// The PR that became `sha`. A short SHA sorts just before the whole ones it starts.
    fn number_of(&self, sha: &CommitSha) -> Option<u32> {
        let (whole, number) = self.0.range(sha.clone()..).next()?;
        whole.starts_with(sha).then_some(*number)
    }
}

/// The Origin PR and the kind of Backport that `facts` is under `conventions`, if it is one. The title's
/// marker comes first, and a cherry-pick line speaks only for a title without one (ADR 0011).
pub fn backport_of(
    facts: &PullRequestFacts,
    conventions: Conventions,
    merges: &MergeCommits,
) -> Option<BackportLink> {
    let off_default = facts.off_default.as_ref()?;
    if !conventions.has_backports() {
        return None;
    }
    let in_title = origins_in_title(&LinkRule::SET, &LinkSignals::of(facts)).into_iter();
    let origin = in_title
        .filter(|(under, _)| *under == conventions)
        .map(|(_, origin)| origin)
        .next()
        .or_else(|| {
            let mut picked = off_default.cherry_picked_from.iter();
            let reads = reads_cherry_picks(&LinkRule::SET);
            reads
                .then(|| picked.find_map(|sha| merges.number_of(sha)))
                .flatten()
        })?;
    Some(BackportLink {
        origin,
        kind: backport_kind(facts, conventions),
    })
}

/// The kind of Backport `facts` is, if the link rules make it one. A merge isn't among the committers, so
/// GitHub's Update branch leaves a Bot backport as it was. With no commits known, as for a PR stored before
/// Tsugiki kept them, nothing says the bot made it.
pub fn backport_kind(facts: &PullRequestFacts, conventions: Conventions) -> BackportKind {
    let is_bot = |login: &Login| conventions.is_backport_bot(login);
    let committers = facts
        .off_default
        .as_ref()
        .map_or(&[][..], |off_default| &off_default.committers);
    let bot_made_it = facts.author.as_ref().is_some_and(is_bot)
        && !committers.is_empty()
        && committers
            .iter()
            .all(|committer| committer.as_ref().is_some_and(is_bot));
    if bot_made_it {
        BackportKind::Bot
    } else {
        BackportKind::Manual
    }
}

/// The Origin PR `facts` could name under any Conventions. A sync fetches it, so changing a Repository's
/// Conventions re-groups its Workbench with no re-sync (ADR 0011). A cherry-pick line isn't here, because it
/// names a commit, and only the PRs Tsugiki has can answer for that.
pub fn possible_origin(facts: &PullRequestFacts) -> Option<u32> {
    let named = origins_in_title(&LinkRule::SET, &LinkSignals::of(facts));
    named.first().map(|(_, origin)| *origin)
}

/// The numbers `facts` names as fixed under `repository`'s Conventions. All signals add up, with no order of
/// trust (ADR 0011).
pub fn fixed_numbers(facts: &PullRequestFacts, repository: &RepositorySettings) -> BTreeSet<u32> {
    let named = fixed_under_each(&LinkRule::SET, &LinkSignals::of(facts), &repository.name);
    let counted = named
        .into_iter()
        .filter(|(_, under)| under.is_none_or(|conventions| conventions == repository.conventions));
    counted.map(|(number, _)| number).collect()
}

/// The numbers `facts` could name under any Conventions. A sync fetches all of these, so changing a
/// Repository's Conventions re-groups its Workbench with no re-sync (ADR 0011).
pub fn possible_numbers(facts: &PullRequestFacts, repository: &RepositoryName) -> BTreeSet<u32> {
    let named = fixed_under_each(&LinkRule::SET, &LinkSignals::of(facts), repository);
    named.into_iter().map(|(number, _)| number).collect()
}

/// What the links that the store keeps are worked out from (#833): the title, GitHub's closing references,
/// and the description of a PR that doesn't target the default branch, where GitHub ignores closing
/// keywords.
#[derive(Debug, Clone, Copy)]
pub struct LinkSignals<'a> {
    pub title: &'a str,
    pub closing_issues: &'a [u32],
    pub off_default_description: Option<&'a str>,
}

impl<'a> LinkSignals<'a> {
    pub fn of(facts: &'a PullRequestFacts) -> Self {
        Self {
            title: &facts.title,
            closing_issues: &facts.closing_issues,
            off_default_description: (facts.off_default.as_ref())
                .map(|off_default| off_default.description.as_str()),
        }
    }
}

/// The numbers the rules of `rules` name as fixed in `signals`, each with the Conventions it counts under,
/// or `None` for a number every Conventions counts. The store keeps these, so a change of Conventions picks
/// other rows and rewrites none (#833). Under `conventions`, the numbers whose `Conventions` is `None` or
/// `conventions` are `fixed_numbers`.
pub fn fixed_under_each(
    rules: &[LinkRule],
    signals: &LinkSignals<'_>,
    repository: &RepositoryName,
) -> Vec<(u32, Option<Conventions>)> {
    let named = rules.iter().flat_map(|rule| match rule.reading() {
        Reading::Fixed(read) => read(signals, repository),
        Reading::OriginInTitle(_) | Reading::OriginPicked => Vec::new(),
    });
    named.collect()
}

/// The Origin PRs that the rules of `rules` name in the title in `signals`, each with the Conventions that
/// read it, if the PR doesn't target the default branch. The store keeps these beside the cherry-pick lines,
/// which name a commit: under a Conventions with Backports, the title's marker comes first, as `backport_of`
/// reads them.
pub fn origins_in_title(rules: &[LinkRule], signals: &LinkSignals<'_>) -> Vec<(Conventions, u32)> {
    let named = rules.iter().filter_map(|rule| match rule.reading() {
        Reading::OriginInTitle(read) => read(signals),
        Reading::Fixed(_) | Reading::OriginPicked => None,
    });
    named.collect()
}

/// True when one of `rules` reads cherry-pick lines, which only the PRs Tsugiki has can answer for.
pub fn reads_cherry_picks(rules: &[LinkRule]) -> bool {
    let reads = |rule: &LinkRule| matches!(rule.reading(), Reading::OriginPicked);
    rules.iter().any(reads)
}

mod rules;

#[cfg(test)]
mod tests;
