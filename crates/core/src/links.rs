//! The Issues a PR names and the Origin PR a Backport comes from, worked out from what GitHub says now and
//! read through the Repository's Conventions (ADR 0011). A number here can still turn out to be a PR, or to
//! name nothing, so only a number that the sync fetched as an Issue fixes one.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use specta::Type;

use crate::conventions::{Conventions, gh_number};
use crate::github::CommitSha;
use crate::github::pull_requests::{OffDefault, PullRequestFacts};
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

/// The facts that make a PR a Backport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackportLink {
    /// The number the PR names. Tsugiki may not have that PR, or it may turn out not to be one.
    pub origin: u32,
    pub kind: BackportKind,
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
        kind: kind_of(facts, off_default, conventions),
    })
}

/// A merge isn't among the committers, so GitHub's Update branch leaves a Bot backport as it was. With no
/// commits known, as for a PR stored before Tsugiki kept them, nothing says the bot made it.
fn kind_of(
    facts: &PullRequestFacts,
    off_default: &OffDefault,
    conventions: Conventions,
) -> BackportKind {
    let is_bot = |login: &Login| conventions.is_backport_bot(login);
    let committers = &off_default.committers;
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
        .chain(signals_of_every_convention(facts, &repository.name))
        .collect()
}

/// The numbers `facts` could name under any Conventions. A sync fetches all of these, so changing a
/// Repository's Conventions re-groups its Workbench with no re-sync (ADR 0011).
pub fn possible_numbers(facts: &PullRequestFacts, repository: &RepositoryName) -> BTreeSet<u32> {
    Conventions::issues_in_title_under_any(&facts.title)
        .into_iter()
        .chain(signals_of_every_convention(facts, repository))
        .collect()
}

fn signals_of_every_convention(facts: &PullRequestFacts, repository: &RepositoryName) -> Vec<u32> {
    let in_description = facts
        .off_default
        .as_ref()
        .map_or_else(Vec::new, |off_default| {
            closing_keywords(&off_default.description, repository)
        });
    let closing = facts.closing_issues.iter().copied();
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
