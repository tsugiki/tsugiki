use super::*;
use crate::github::pull_requests::PrState;
use time::OffsetDateTime;

fn cpython() -> RepositorySettings {
    RepositorySettings::new(RepositoryName::parse("python/cpython").unwrap())
}

fn gcmon() -> RepositorySettings {
    RepositorySettings {
        name: RepositoryName::parse("some-maintainer/gcmon").unwrap(),
        conventions: Conventions::Github,
        filters: Vec::new(),
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
        base: None,
        head: None,
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
        base: None,
        head: None,
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
        base: None,
        head: None,
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

fn off_default(cherry_picked_from: Vec<CommitSha>, committers: Vec<Option<Login>>) -> OffDefault {
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
        base: None,
        head: None,
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
        base: None,
        head: None,
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
        base: None,
        head: None,
        off_default: Some(OffDefault::described("Fixes #3")),
        ..titled("gh-1: Fix it")
    };
    let possible: Vec<u32> = possible_numbers(&facts, &gcmon().name)
        .into_iter()
        .collect();
    assert_eq!(possible, [1, 2, 3]);
    assert_eq!(numbers(&facts, &gcmon()), [2, 3]);
}
