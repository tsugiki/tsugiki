//! The Issues a PR names, worked out from what GitHub says now and read through the Repository's Conventions
//! (ADR 0011). A number here can still turn out to be a PR, or to name nothing, so only a number that the
//! sync fetched as an Issue fixes one.

use std::collections::BTreeSet;

use crate::github::pull_requests::PullRequestFacts;
use crate::repository::{RepositoryName, RepositorySettings};

/// GitHub's closing keywords, in each form it accepts.
const KEYWORDS: [&str; 9] = [
    "close", "closes", "closed", "fix", "fixes", "fixed", "resolve", "resolves", "resolved",
];
const GITHUB_URL: &str = "https://github.com/";

/// The numbers `facts` names as fixed under `repository`'s Conventions. All signals add up, with no order of
/// trust (ADR 0011).
pub fn fixed_numbers(facts: &PullRequestFacts, repository: &RepositorySettings) -> BTreeSet<u32> {
    let in_title = if repository.conventions.names_issues_in_titles() {
        title_issues(&facts.title)
    } else {
        Vec::new()
    };
    in_title
        .into_iter()
        .chain(signals_of_every_convention(facts, &repository.name))
        .collect()
}

/// The numbers `facts` could name under any Conventions. A sync fetches all of these, so changing a
/// Repository's Conventions re-groups its Workbench with no re-sync (ADR 0011).
pub fn possible_numbers(facts: &PullRequestFacts, repository: &RepositoryName) -> BTreeSet<u32> {
    title_issues(&facts.title)
        .into_iter()
        .chain(signals_of_every_convention(facts, repository))
        .collect()
}

fn signals_of_every_convention(facts: &PullRequestFacts, repository: &RepositoryName) -> Vec<u32> {
    let in_description = facts
        .off_default_description
        .as_deref()
        .map_or_else(Vec::new, |text| closing_keywords(text, repository));
    let closing = facts.closing_issues.iter().copied();
    closing.chain(in_description).collect()
}

/// The Issues in a title such as `gh-1: Fix it`, `gh-1, gh-2: Fix both` or `[3.14] gh-1: Fix it (GH-9)`.
/// Anything else before the colon means the title has no prefix.
fn title_issues(title: &str) -> Vec<u32> {
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
        return here(named).then(|| number(digits)).flatten();
    }
    match word.split_once('#') {
        Some(("", digits)) => number(digits),
        Some((named, digits)) => here(named).then(|| number(digits)).flatten(),
        None => gh_number(word),
    }
}

/// The number in `gh-N`, whatever its case.
fn gh_number(word: &str) -> Option<u32> {
    let digits = word.get(3..)?;
    word.get(..3)?
        .eq_ignore_ascii_case("gh-")
        .then(|| number(digits))
        .flatten()
}

/// The number `digits` spell. GitHub's numbers are GraphQL `Int`s, 32 bits and signed, and it refuses a whole
/// query over one that doesn't fit.
fn number(digits: &str) -> Option<u32> {
    digits
        .chars()
        .all(|c| c.is_ascii_digit())
        .then(|| digits.parse::<i32>().ok())
        .flatten()
        .and_then(|number| u32::try_from(number).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conventions::Conventions;
    use crate::github::pull_requests::PrState;
    use time::OffsetDateTime;

    fn cpython() -> RepositorySettings {
        RepositorySettings::new(RepositoryName::parse("python/cpython").unwrap())
    }

    fn gcmon() -> RepositorySettings {
        RepositorySettings {
            name: RepositoryName::parse("sergey-miryanov/gcmon").unwrap(),
            conventions: Conventions::Github,
        }
    }

    fn titled(title: &str) -> PullRequestFacts {
        PullRequestFacts {
            number: 9000,
            title: title.to_owned(),
            state: PrState::Open,
            is_draft: false,
            author: None,
            target_branch: "main".to_owned(),
            ci: None,
            updated_at: OffsetDateTime::UNIX_EPOCH,
            review_requests: vec![],
            latest_reviews: vec![],
            assignees: vec![],
            closing_issues: vec![],
            off_default_description: None,
        }
    }

    fn numbers(facts: &PullRequestFacts, repository: &RepositorySettings) -> Vec<u32> {
        fixed_numbers(facts, repository).into_iter().collect()
    }

    /// The numbers a description names on a PR GitHub ignores closing keywords on.
    fn in_description(text: &str) -> Vec<u32> {
        let facts = PullRequestFacts {
            off_default_description: Some(text.to_owned()),
            ..titled("Fix it")
        };
        numbers(&facts, &cpython())
    }

    #[test]
    fn a_cpython_title_names_its_issue_with_its_prefix() {
        for (title, expected) in [
            ("gh-123: Fix it", vec![123]),
            ("GH-123: Fix it", vec![123]),
            ("[3.14] gh-123: Fix it (GH-456)", vec![123]),
            ("gh-123, gh-456: Fix both", vec![123, 456]),
            ("gh-123 gh-456: Fix both", vec![123, 456]),
            ("[3.14] gh-123, GH-456: Fix both", vec![123, 456]),
            ("gh-123, see gh-456: Fix it", vec![]),
            (": Fix it", vec![]),
            ("Fix it", vec![]),
            ("Fix: gh-123 again", vec![]),
            ("see gh-123: it broke", vec![]),
            ("gh-: Fix it", vec![]),
            ("gh-12x: Fix it", vec![]),
            ("gé: Fix it", vec![]),
            ("[3.14 gh-123: Fix it", vec![]),
        ] {
            assert_eq!(numbers(&titled(title), &cpython()), expected, "{title}");
        }
    }

    #[test]
    fn a_title_prefix_means_nothing_under_githubs_conventions() {
        assert_eq!(numbers(&titled("gh-123: Fix it"), &gcmon()), [0_u32; 0]);
    }

    #[test]
    fn closing_references_count_under_every_convention() {
        let facts = PullRequestFacts {
            closing_issues: vec![7, 8],
            ..titled("Fix it")
        };
        assert_eq!(numbers(&facts, &cpython()), [7, 8]);
        assert_eq!(numbers(&facts, &gcmon()), [7, 8]);
    }

    #[test]
    fn every_signal_adds_up_and_a_number_counts_once() {
        let facts = PullRequestFacts {
            closing_issues: vec![1, 2],
            off_default_description: Some("Fixes #2\nCloses #3".to_owned()),
            ..titled("gh-1: Fix it")
        };
        assert_eq!(numbers(&facts, &cpython()), [1, 2, 3]);
    }

    #[test]
    fn a_description_counts_only_where_github_ignores_its_keywords() {
        assert_eq!(numbers(&titled("Fix it"), &cpython()), [0_u32; 0]);
        assert_eq!(in_description("Fixes #5"), [5]);
    }

    #[test]
    fn each_form_of_a_keyword_names_an_issue() {
        for keyword in [
            "close",
            "closes",
            "closed",
            "fix",
            "fixes",
            "fixed",
            "resolve",
            "resolves",
            "resolved",
            "FIXES",
            "Fixes:",
            "(fixes",
            "**Fixes**",
        ] {
            assert_eq!(in_description(&format!("{keyword} #5")), [5], "{keyword}");
        }
    }

    #[test]
    fn each_way_of_writing_the_number_names_an_issue() {
        for target in [
            "#5",
            "#5.",
            "#5,",
            "(#5)",
            "GH-5",
            "gh-5",
            "python/cpython#5",
            "Python/CPython#5",
            "https://github.com/python/cpython/issues/5",
            "https://github.com/python/cpython/issues/5#issuecomment-1",
            "https://github.com/Python/CPython/issues/5.",
        ] {
            assert_eq!(in_description(&format!("Fixes {target}")), [5], "{target}");
        }
    }

    #[test]
    fn a_link_to_another_repository_names_nothing() {
        for target in [
            "python/peps#5",
            "https://github.com/python/peps/issues/5",
            "https://github.com/python/cpython/pull/5",
        ] {
            assert_eq!(in_description(&format!("Fixes {target}")), [0_u32; 0]);
        }
    }

    #[test]
    fn a_bare_mention_names_nothing() {
        for text in [
            "see gh-5",
            "#5",
            "Fixes the bug in #5",
            "prefixes #5",
            "Fixes",
            "Fixes #x",
            "`fixes #5`",
            "Fixes gh-",
            "```\nFixes #5\n```",
            "~~~\nFixes #5\n~~~",
            "<!-- Fixes #5 -->",
            "<!--\nFixes #5\n-->",
            "<!-- never closed\nFixes #5",
            "this `fixes #5` too",
            "Fixes `#5`",
            "Fixes `x` #5",
        ] {
            assert_eq!(in_description(text), [0_u32; 0], "{text}");
        }
    }

    #[test]
    fn a_keyword_after_a_code_block_counts() {
        assert_eq!(in_description("```\nFixes #5\n```\nFixes #6"), [6]);
    }

    #[test]
    fn a_keyword_around_a_comment_or_inline_code_counts() {
        assert_eq!(
            in_description("<!-- a --> Fixes #5 <!-- b --> `c` Closes #6"),
            [5, 6]
        );
    }

    #[test]
    fn a_number_github_cant_take_names_nothing() {
        assert_eq!(in_description("Fixes #2147483647"), [2_147_483_647]);
        assert_eq!(in_description("Fixes #2147483648"), [0_u32; 0]);
        assert_eq!(in_description("Fixes #99999999999"), [0_u32; 0]);
    }

    #[test]
    fn a_sync_asks_for_what_any_conventions_could_name() {
        let facts = PullRequestFacts {
            closing_issues: vec![2],
            off_default_description: Some("Fixes #3".to_owned()),
            ..titled("gh-1: Fix it")
        };
        let possible: Vec<u32> = possible_numbers(&facts, &gcmon().name)
            .into_iter()
            .collect();
        assert_eq!(possible, [1, 2, 3]);
        assert_eq!(numbers(&facts, &gcmon()), [2, 3]);
    }
}
