//! The link rules, each a variant of `LinkRule`, and the set of them that runs (ADR 0024). What each rule
//! reads in a PR's text is private here, so no rule runs except through `LinkRule`.

use super::{LinkSignals, MergeCommits};
use crate::conventions::Conventions;
use crate::facts::CommitSha;
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
/// and the old variant stays as a retired name outside `SET`, which runs nowhere and can give nothing. Every
/// change to the rows counts: a new keyword, a new Conventions, or how a row spells its Conventions. The
/// pinned test in `store/pr_links/tests.rs` holds each rule's sample PRs and the rows it keeps for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// The Origin PR a cherry-pick line names, as the PR that became its commit, under each Conventions
    /// with Backports. `MergeCommits` finds that PR among the facts, and the store's `Keeper` among the
    /// PRs it holds.
    CherryPickLine,
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

    /// The numbers this rule names as fixed in `signals`, each with the Conventions it counts under, or
    /// `None` for a number every Conventions counts.
    pub fn fixed(
        self,
        signals: &LinkSignals<'_>,
        repository: &RepositoryName,
    ) -> Vec<(u32, Option<Conventions>)> {
        let every = |numbers: Vec<u32>| numbers.into_iter().map(|number| (number, None)).collect();
        match self {
            Self::ClosingReferences => every(signals.closing_issues.to_vec()),
            Self::DescriptionKeywords => every(
                (signals.off_default_description).map_or_else(Vec::new, |description| {
                    closing_keywords(description, repository)
                }),
            ),
            Self::CpythonTitleIssues => cpython_title_issues(signals.title)
                .into_iter()
                .map(|number| (number, Some(Conventions::Cpython)))
                .collect(),
            Self::CpythonTitleOrigin | Self::CherryPickLine => Vec::new(),
        }
    }

    /// The Origin PR this rule names in the title in `signals`, with the Conventions that read it, if the PR
    /// doesn't target the default branch.
    pub fn origin_in_title(self, signals: &LinkSignals<'_>) -> Option<(Conventions, u32)> {
        let off_default = signals.off_default_description.is_some();
        match self {
            Self::CpythonTitleOrigin if off_default => {
                Some((Conventions::Cpython, cpython_title_origin(signals.title)?))
            }
            Self::CpythonTitleOrigin
            | Self::ClosingReferences
            | Self::DescriptionKeywords
            | Self::CpythonTitleIssues
            | Self::CherryPickLine => None,
        }
    }

    /// The Origin PR this rule names in the cherry-pick lines `picked`: the PR among `merges` that became
    /// the first of them that any PR became.
    pub fn origin_picked(self, picked: &[CommitSha], merges: &MergeCommits) -> Option<u32> {
        match self {
            Self::CherryPickLine => picked.iter().find_map(|sha| merges.number_of(sha)),
            Self::ClosingReferences
            | Self::DescriptionKeywords
            | Self::CpythonTitleIssues
            | Self::CpythonTitleOrigin => None,
        }
    }
}

/// The Issues in a title such as `gh-1: Fix it`, `gh-1, gh-2: Fix both` or `[3.14] gh-1: Fix it (GH-9)`.
/// Anything else before the colon means the title has no prefix.
fn cpython_title_issues(title: &str) -> Vec<u32> {
    let Some((head, _)) = title.split_once(':') else {
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
    numbers.unwrap_or_default()
}

/// The last `(GH-N)` in a title such as `[3.14] gh-1: Fix it (GH-9)`.
fn cpython_title_origin(title: &str) -> Option<u32> {
    title
        .rsplit('(')
        .find_map(|piece| gh_number(piece.split_once(')')?.0))
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
