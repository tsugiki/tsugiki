//! The Issues a PR names and the Origin PR a Backport comes from, worked out from what GitHub says now and
//! read through the Repository's Conventions (ADR 0011). A number here can still turn out to be a PR, or to
//! name nothing, so only a number that the sync fetched as an Issue fixes one.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use specta::Type;

use crate::conventions::Conventions;
use crate::github::pull_requests::{OffDefault, PullRequestFacts};
use crate::github::{CommitSha, Login, gh_number, issue_number};
use crate::repository::{RepositoryName, RepositorySettings};

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
mod tests {
    use super::*;
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
            merge_commit: None,
            off_default: None,
        }
    }

    fn numbers(facts: &PullRequestFacts, repository: &RepositorySettings) -> Vec<u32> {
        fixed_numbers(facts, repository).into_iter().collect()
    }

    /// The numbers a description names on a PR GitHub ignores closing keywords on.
    fn in_description(text: &str) -> Vec<u32> {
        let facts = PullRequestFacts {
            merge_commit: None,
            off_default: Some(OffDefault::described(text)),
            ..titled("Fix it")
        };
        numbers(&facts, &cpython())
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
            merge_commit: None,
            off_default: Some(OffDefault::described("Fixes #2\nCloses #3")),
            ..titled("gh-1: Fix it")
        };
        assert_eq!(numbers(&facts, &cpython()), [1, 2, 3]);
    }

    const MERGE: &str = "0123456789abcdef0123456789abcdef01234567";
    const MERGE_START: &str = "0123456";
    const OTHER: &str = "fedcba9876543210fedcba9876543210fedcba98";

    fn sha(text: &str) -> CommitSha {
        CommitSha::parse(text).unwrap()
    }

    fn bot() -> Login {
        Login::new("miss-islington")
    }

    fn person() -> Login {
        Login::new("some-author")
    }

    fn off_default(
        cherry_picked_from: Vec<CommitSha>,
        committers: Vec<Option<Login>>,
    ) -> OffDefault {
        OffDefault {
            description: String::new(),
            cherry_picked_from,
            committers,
        }
    }

    /// A PR on 3.14 titled `title`, opened by `author` and committed by `committers`.
    fn on_a_branch(title: &str, author: Login, committers: Vec<Option<Login>>) -> PullRequestFacts {
        PullRequestFacts {
            author: Some(author),
            off_default: Some(off_default(vec![], committers)),
            ..titled(title)
        }
    }

    fn merged_as(number: u32, oid: &str) -> PullRequestFacts {
        PullRequestFacts {
            number,
            state: PrState::Merged,
            merge_commit: Some(sha(oid)),
            ..titled("gh-1: Fix it")
        }
    }

    fn no_merges() -> MergeCommits {
        MergeCommits::of(&[])
    }

    fn origin_under_cpython(facts: &PullRequestFacts, merges: &MergeCommits) -> Option<u32> {
        backport_of(facts, Conventions::Cpython, merges).map(|link| link.origin)
    }

    #[test]
    fn a_pr_off_the_default_branch_with_the_marker_is_a_backport() {
        let facts = on_a_branch("[3.14] gh-1: Fix it (GH-9)", person(), vec![]);
        assert_eq!(origin_under_cpython(&facts, &no_merges()), Some(9));
    }

    #[test]
    fn a_pr_on_the_default_branch_with_the_marker_isnt_a_backport() {
        let facts = titled("gh-1: Revert it (GH-9)");
        assert_eq!(origin_under_cpython(&facts, &no_merges()), None);
    }

    #[test]
    fn a_pr_off_the_default_branch_with_no_signal_isnt_a_backport() {
        let facts = on_a_branch("gh-1: Stacked on another PR", person(), vec![]);
        assert_eq!(origin_under_cpython(&facts, &no_merges()), None);
    }

    #[test]
    fn a_cherry_pick_line_finds_the_merged_pr_with_that_merge_commit() {
        let merges = MergeCommits::of(&[merged_as(9, MERGE), merged_as(8, OTHER)]);
        let facts = PullRequestFacts {
            off_default: Some(off_default(vec![sha(MERGE)], vec![])),
            ..titled("gh-1: Fix it by hand")
        };
        assert_eq!(origin_under_cpython(&facts, &merges), Some(9));
    }

    #[test]
    fn a_cherry_pick_line_can_name_a_short_sha() {
        let merges = MergeCommits::of(&[merged_as(9, MERGE)]);
        let facts = PullRequestFacts {
            off_default: Some(off_default(vec![sha(MERGE_START)], vec![])),
            ..titled("gh-1: Fix it by hand")
        };
        assert_eq!(origin_under_cpython(&facts, &merges), Some(9));
    }

    #[test]
    fn a_cherry_pick_line_that_matches_no_merge_commit_isnt_a_backport() {
        let merges = MergeCommits::of(&[merged_as(9, MERGE)]);
        let facts = PullRequestFacts {
            off_default: Some(off_default(vec![sha(OTHER)], vec![])),
            ..titled("gh-1: Fix it by hand")
        };
        assert_eq!(origin_under_cpython(&facts, &merges), None);
    }

    #[test]
    fn the_marker_wins_over_a_cherry_pick_line() {
        let merges = MergeCommits::of(&[merged_as(9, MERGE)]);
        let facts = PullRequestFacts {
            off_default: Some(off_default(vec![sha(MERGE)], vec![])),
            ..titled("[3.14] gh-1: Fix it (GH-5)")
        };
        assert_eq!(origin_under_cpython(&facts, &merges), Some(5));
    }

    #[test]
    fn an_open_pr_has_no_merge_commit_to_find() {
        let open = PullRequestFacts {
            number: 9,
            merge_commit: None,
            ..titled("gh-1: Fix it")
        };
        let merges = MergeCommits::of(&[open]);
        let facts = PullRequestFacts {
            off_default: Some(off_default(vec![sha(MERGE)], vec![])),
            ..titled("gh-1: Fix it by hand")
        };
        assert_eq!(origin_under_cpython(&facts, &merges), None);
    }

    #[test]
    fn githubs_conventions_have_no_backports_whatever_the_pr_says() {
        let merges = MergeCommits::of(&[merged_as(9, MERGE)]);
        let facts = PullRequestFacts {
            off_default: Some(off_default(vec![sha(MERGE)], vec![])),
            ..titled("[3.14] gh-1: Fix it (GH-5)")
        };
        assert_eq!(backport_of(&facts, Conventions::Github, &merges), None);
    }

    fn kind_of(author: Login, committers: Vec<Option<Login>>) -> BackportKind {
        let facts = on_a_branch("[3.14] gh-1: Fix it (GH-9)", author, committers);
        backport_of(&facts, Conventions::Cpython, &no_merges())
            .unwrap()
            .kind
    }

    #[test]
    fn the_bot_opening_it_and_committing_every_commit_makes_a_bot_backport() {
        let committers = vec![Some(bot()), Some(Login::new("Miss-Islington"))];
        assert_eq!(kind_of(bot(), committers), BackportKind::Bot);
    }

    #[test]
    fn a_person_opening_it_makes_a_manual_backport() {
        assert_eq!(kind_of(person(), vec![Some(bot())]), BackportKind::Manual);
    }

    #[test]
    fn someone_elses_commit_makes_a_bots_pr_a_manual_backport() {
        let committers = vec![Some(bot()), Some(person())];
        assert_eq!(kind_of(bot(), committers), BackportKind::Manual);
    }

    #[test]
    fn a_commit_by_nobody_makes_a_bots_pr_a_manual_backport() {
        assert_eq!(
            kind_of(bot(), vec![Some(bot()), None]),
            BackportKind::Manual
        );
    }

    #[test]
    fn a_bots_pr_with_no_commits_known_is_a_manual_backport() {
        assert_eq!(kind_of(bot(), vec![]), BackportKind::Manual);
    }

    #[test]
    fn a_deleted_author_makes_a_manual_backport() {
        let facts = PullRequestFacts {
            author: None,
            ..on_a_branch("[3.14] gh-1: Fix it (GH-9)", bot(), vec![Some(bot())])
        };
        let link = backport_of(&facts, Conventions::Cpython, &no_merges()).unwrap();
        assert_eq!(link.kind, BackportKind::Manual);
    }

    #[test]
    fn a_sync_fetches_the_origin_a_title_could_name_only_off_the_default_branch() {
        let on_branch = on_a_branch("[3.14] gh-1: Fix it (GH-9)", person(), vec![]);
        assert_eq!(possible_origin(&on_branch), Some(9));
        assert_eq!(possible_origin(&titled("gh-1: Revert it (GH-9)")), None);
        assert_eq!(
            possible_origin(&on_a_branch("Fix it", person(), vec![])),
            None
        );
    }

    #[test]
    fn a_backport_names_what_its_title_and_its_live_keywords_name() {
        let description = "(cherry picked from commit 0123abc)\r\n\r\n\
            Co-authored-by: some-author <some-author@example.com>\r\n\r\n\
            <!-- gh-issue-number: gh-2 -->\r\n\
            * Issue: gh-2\r\n\
            <!-- /gh-issue-number -->\r\n\r\n\
            Fixes gh-3.\r\n\r\n\
            ```\r\nFixes gh-4\r\n```\r\n\
            See gh-5 and `closes gh-6`.";
        let facts = PullRequestFacts {
            target_branch: "3.14".to_owned(),
            merge_commit: None,
            off_default: Some(OffDefault::described(description)),
            ..titled("[3.14] gh-1: Fix it (GH-9)")
        };
        assert_eq!(numbers(&facts, &cpython()), [1, 3]);
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
            merge_commit: None,
            off_default: Some(OffDefault::described("Fixes #3")),
            ..titled("gh-1: Fix it")
        };
        let possible: Vec<u32> = possible_numbers(&facts, &gcmon().name)
            .into_iter()
            .collect();
        assert_eq!(possible, [1, 2, 3]);
        assert_eq!(numbers(&facts, &gcmon()), [2, 3]);
    }
}
