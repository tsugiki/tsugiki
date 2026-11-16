//! The Issues a PR names and the Origin PR a Backport comes from, worked out from what GitHub says now and
//! read through the Repository's Conventions (ADR 0011). A number here can still turn out to be a PR, or to
//! name nothing, so only a number that the sync fetched as an Issue fixes one.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use specta::Type;

use crate::conventions::{Conventions, gh_number};
use crate::facts::CommitSha;
use crate::facts::pull_requests::PullRequestFacts;
use crate::login::Login;
use crate::repository::{RepositoryName, issue_number};
use crate::repository_settings::RepositorySettings;

/// GitHub's closing keywords, in each form it accepts.
const KEYWORDS: [&str; 9] = [
    "close", "closes", "closed", "fix", "fixes", "fixed", "resolve", "resolves", "resolved",
];
const GITHUB_URL: &str = "https://github.com/";

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

/// The merged PRs Tsugiki has, by the commit each became, for the cherry-pick lines to find.
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
    let origin = conventions.origin_in_title(&facts.title).or_else(|| {
        let mut picked = off_default.cherry_picked_from.iter();
        picked.find_map(|sha| merges.number_of(sha))
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
    facts
        .off_default
        .as_ref()
        .and_then(|_| Conventions::origin_in_title_under_any(&facts.title))
}

/// The numbers `facts` names as fixed under `repository`'s Conventions. All signals add up, with no order of
/// trust (ADR 0011).
pub fn fixed_numbers(facts: &PullRequestFacts, repository: &RepositorySettings) -> BTreeSet<u32> {
    repository
        .conventions
        .issues_in_title(&facts.title)
        .into_iter()
        .chain(signals_of_every_convention(
            &LinkSignals::of(facts),
            &repository.name,
        ))
        .collect()
}

/// The numbers `facts` could name under any Conventions. A sync fetches all of these, so changing a
/// Repository's Conventions re-groups its Workbench with no re-sync (ADR 0011).
pub fn possible_numbers(facts: &PullRequestFacts, repository: &RepositoryName) -> BTreeSet<u32> {
    Conventions::issues_in_title_under_any(&facts.title)
        .into_iter()
        .chain(signals_of_every_convention(
            &LinkSignals::of(facts),
            repository,
        ))
        .collect()
}

/// The version of what `fixed_under_each` and `origins_in_title` give, which the store keeps beside the links
/// it worked out with them. Raise it with any change to what they give, a new Conventions among them: the
/// store then works out the links of every PR it holds again as it opens, since a PR that never syncs again
/// would keep the old ones. A test pins what they give to this number.
pub const RULES_VERSION: u32 = 1;

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

/// The numbers `signals` names as fixed, each with the Conventions it counts under, or `None` for a number
/// every Conventions counts. The store keeps these, so a change of Conventions picks other rows and rewrites
/// none (#833). Under `conventions`, the numbers whose `Conventions` is `None` or `conventions` are
/// `fixed_numbers`.
pub fn fixed_under_each(
    signals: &LinkSignals<'_>,
    repository: &RepositoryName,
) -> Vec<(u32, Option<Conventions>)> {
    let in_titles = Conventions::ALL.into_iter().flat_map(|conventions| {
        let named = conventions.issues_in_title(signals.title).into_iter();
        named.map(move |number| (number, Some(conventions)))
    });
    let every = signals_of_every_convention(signals, repository).into_iter();
    in_titles
        .chain(every.map(|number| (number, None)))
        .collect()
}

/// The Origin PR that the title in `signals` names under each Conventions with Backports, if the PR doesn't
/// target the default branch. The store keeps these beside the cherry-pick lines, which name a commit: under
/// a Conventions with Backports, the title's marker comes first, as `backport_of` reads them.
pub fn origins_in_title(signals: &LinkSignals<'_>) -> Vec<(Conventions, u32)> {
    if signals.off_default_description.is_none() {
        return Vec::new();
    }
    let with_backports = Conventions::ALL.into_iter().filter(|c| c.has_backports());
    let named = with_backports.filter_map(|c| Some((c, c.origin_in_title(signals.title)?)));
    named.collect()
}

fn signals_of_every_convention(signals: &LinkSignals<'_>, repository: &RepositoryName) -> Vec<u32> {
    let in_description = (signals.off_default_description).map_or_else(Vec::new, |description| {
        closing_keywords(description, repository)
    });
    let closing = signals.closing_issues.iter().copied();
    closing.chain(in_description).collect()
}

/// The numbers a closing keyword names in `text`, leaving out the parts GitHub doesn't read as text:
/// comments, code blocks and inline code.
fn closing_keywords(text: &str, repository: &RepositoryName) -> Vec<u32> {
    let shown = without_comments(text);
    let mut in_code = false;
    let mut found = Vec::new();
    for line in shown.lines() {
        let line_start = line.trim_start();
        if line_start.starts_with("```") || line_start.starts_with("~~~") {
            in_code = !in_code;
        } else if !in_code {
            // Every second piece between backticks is inline code.
            for piece in line.split('`').step_by(2) {
                found.extend(keywords_in(piece, repository));
            }
        }
    }
    found
}

/// `text` without its HTML comments. A comment nobody closes runs to the end, as it does on GitHub.
fn without_comments(text: &str) -> String {
    let mut shown = String::new();
    let mut rest = text;
    while let Some((before, comment)) = rest.split_once("<!--") {
        shown.push_str(before);
        rest = comment.split_once("-->").map_or("", |(_, after)| after);
    }
    shown.push_str(rest);
    shown
}

fn keywords_in(text: &str, repository: &RepositoryName) -> Vec<u32> {
    let words = text.split_whitespace();
    words
        .clone()
        .zip(words.skip(1))
        .filter(|(word, _)| is_keyword(word))
        .filter_map(|(_, next)| named_issue(next, repository))
        .collect()
}

fn is_keyword(word: &str) -> bool {
    let word = word
        .trim_start_matches(['(', '*', '_'])
        .trim_end_matches([':', '*', '_']);
    KEYWORDS
        .iter()
        .any(|keyword| word.eq_ignore_ascii_case(keyword))
}

/// The number in `#N`, `GH-N`, `owner/name#N` or an Issue's URL, if it is in `repository`.
fn named_issue(word: &str, repository: &RepositoryName) -> Option<u32> {
    let word = word
        .trim_start_matches(['(', '[', '*', '_'])
        .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '*', '_']);
    let here =
        |named: &str| RepositoryName::parse(named).is_ok_and(|name| name.is_same(repository));
    if let Some(path) = word.strip_prefix(GITHUB_URL) {
        let (named, rest) = path.split_once("/issues/")?;
        let (digits, _) = rest.split_once('#').unwrap_or((rest, ""));
        return here(named).then(|| issue_number(digits)).flatten();
    }
    match word.split_once('#') {
        Some(("", digits)) => issue_number(digits),
        Some((named, digits)) => here(named).then(|| issue_number(digits)).flatten(),
        None => gh_number(word),
    }
}

#[cfg(test)]
mod tests;
