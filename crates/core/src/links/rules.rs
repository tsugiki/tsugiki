//! The link rules, each a variant of `LinkRule`, and the set of them that runs (ADR 0024). What each rule
//! reads in a PR's text is private here, so no rule runs except through `LinkRule`.

use super::LinkSignals;
use crate::conventions::Conventions;
use crate::repository::{RepositoryName, issue_number};

/// GitHub's closing keywords, in each form it accepts.
const KEYWORDS: [&str; 9] = [
    "close", "closes", "closed", "fix", "fixes", "fixed", "resolve", "resolves", "resolved",
];
const GITHUB_URL: &str = "https://github.com/";

/// A rule that works out a PR's links from its facts: the numbers it names as fixed, or its Origin PR.
///
/// The store keeps the rows the rules of `SET` give (#833), and a PR that never syncs again keeps them past
/// a release, as the schema keeps what its migrations wrote. So a released rule never changes what it gives,
/// as a released migration never changes. A change to one is a new variant that takes its place in `SET`,
/// and the old variant stays as a retired name outside `SET`, which runs nowhere. The store keeps the name
/// of each rule that wrote its rows (`store/pr_links.rs`), refills them when those aren't `SET`, and
/// refuses to open on a name no variant spells, which a newer Tsugiki wrote. Every change to the rows
/// counts: a new keyword, a new Conventions, how a row spells its Conventions, or how a cherry-pick line
/// finds its PR. The pinned test in `store/pr_links/tests.rs` holds each rule's sample PRs and the rows it
/// keeps for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LinkRule {
    /// The Issues GitHub says the PR closes, under every Conventions.
    ClosingReferences,
    /// The Issues a closing keyword names in the description of a PR off the default branch, where GitHub
    /// ignores the keywords, under every Conventions.
    DescriptionKeywords,
    /// The Issues a CPython title's `gh-N` prefix names, under CPython's Conventions.
    CpythonTitleIssues,
    /// The Origin PR the last `(GH-N)` in the title of a PR off the default branch names, under CPython's
    /// Conventions.
    CpythonTitleOrigin,
    /// The Origin PR a cherry-pick line names, as the PR that became the first of its commits that a PR
    /// became, in the order of their SHAs, under each Conventions with Backports. A short SHA names the
    /// first whole one it starts.
    CherryPickLine,
}

/// What reads the numbers a PR names as fixed in the Repository named, each with the Conventions it counts
/// under, or `None` for a number every Conventions counts.
pub type FixedReader = fn(&LinkSignals<'_>, &RepositoryName) -> Vec<(u32, Option<Conventions>)>;

/// How a link rule reads a PR, by what it gives.
#[derive(Clone, Copy)]
pub enum Reading {
    /// The numbers it names as fixed.
    Fixed(FixedReader),
    /// The Origin PR its title names, with the Conventions that read it, if the PR doesn't target the
    /// default branch.
    OriginInTitle(fn(&LinkSignals<'_>) -> Option<(Conventions, u32)>),
    /// The Origin PR its cherry-pick lines name. A line names a commit, and only the PRs Tsugiki has can
    /// say which became it: `MergeCommits` among the facts, and the store's `Keeper` among the PRs it holds.
    /// How each finds that PR is this rule's, and a change to either is a new rule.
    OriginPicked,
}

impl LinkRule {
    /// The rules that run, as the schema's migrations do.
    pub const SET: [Self; 5] = [
        Self::ClosingReferences,
        Self::DescriptionKeywords,
        Self::CpythonTitleIssues,
        Self::CpythonTitleOrigin,
        Self::CherryPickLine,
    ];

    /// How this rule reads a PR. Each rule gives one kind of link, so the match names its reading once.
    pub fn reading(self) -> Reading {
        match self {
            Self::ClosingReferences => Reading::Fixed(closing_references),
            Self::DescriptionKeywords => Reading::Fixed(description_keywords),
            Self::CpythonTitleIssues => Reading::Fixed(cpython_title_issues),
            Self::CpythonTitleOrigin => Reading::OriginInTitle(cpython_title_origin),
            Self::CherryPickLine => Reading::OriginPicked,
        }
    }
}

fn closing_references(
    signals: &LinkSignals<'_>,
    _: &RepositoryName,
) -> Vec<(u32, Option<Conventions>)> {
    let closing = signals.closing_issues.iter();
    closing.map(|number| (*number, None)).collect()
}

fn description_keywords(
    signals: &LinkSignals<'_>,
    repository: &RepositoryName,
) -> Vec<(u32, Option<Conventions>)> {
    let named = (signals.off_default_description).map_or_else(Vec::new, |description| {
        closing_keywords(description, repository)
    });
    named.into_iter().map(|number| (number, None)).collect()
}

/// The Issues in a title such as `gh-1: Fix it`, `gh-1, gh-2: Fix both` or `[3.14] gh-1: Fix it (GH-9)`.
/// Anything else before the colon means the title has no prefix.
fn cpython_title_issues(
    signals: &LinkSignals<'_>,
    _: &RepositoryName,
) -> Vec<(u32, Option<Conventions>)> {
    let Some((head, _)) = signals.title.split_once(':') else {
        return Vec::new();
    };
    let head = head.trim_start();
    let names = head
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .map_or(head, |(_, after)| after);
    let numbers: Option<Vec<u32>> = names
        .split([',', ' '])
        .filter(|name| !name.is_empty())
        .map(gh_number)
        .collect();
    let numbers = numbers.unwrap_or_default().into_iter();
    numbers
        .map(|number| (number, Some(Conventions::Cpython)))
        .collect()
}

/// The last `(GH-N)` in a title such as `[3.14] gh-1: Fix it (GH-9)`, on a PR off the default branch.
fn cpython_title_origin(signals: &LinkSignals<'_>) -> Option<(Conventions, u32)> {
    let title = signals.off_default_description.and(Some(signals.title))?;
    let mut pieces = title.rsplit('(');
    let origin = pieces.find_map(|piece| gh_number(piece.split_once(')')?.0))?;
    Some((Conventions::Cpython, origin))
}

/// The number in `gh-N`, whatever its case.
fn gh_number(word: &str) -> Option<u32> {
    let digits = word.get(3..)?;
    word.get(..3)?
        .eq_ignore_ascii_case("gh-")
        .then(|| issue_number(digits))
        .flatten()
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
