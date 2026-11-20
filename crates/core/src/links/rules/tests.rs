use super::*;

fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

/// The signals of a PR titled `title` with no closing references, and off the default branch with an empty
/// description.
fn off_default(title: &str) -> LinkSignals<'_> {
    LinkSignals {
        title,
        closing_issues: &[],
        off_default_description: Some(""),
    }
}

/// The numbers `rule` names as fixed in a title, with the Conventions each counts under.
fn fixed_in_title(rule: LinkRule, title: &str) -> Vec<(u32, Option<Conventions>)> {
    rule.fixed(&off_default(title), &cpython())
}

#[test]
fn a_cpython_title_names_its_issues_with_its_prefix() {
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
        let under_cpython: Vec<_> = expected
            .into_iter()
            .map(|number| (number, Some(Conventions::Cpython)))
            .collect();
        assert_eq!(
            fixed_in_title(LinkRule::CpythonTitleIssues, title),
            under_cpython,
            "{title}"
        );
    }
}

#[test]
fn a_cpython_title_names_its_origin_pr_with_its_marker() {
    for (title, expected) in [
        ("[3.14] gh-1: Fix it (GH-9)", Some(9)),
        ("[3.14] Doc: Fix it (gh-9)", Some(9)),
        ("[3.14] gh-1: Fix it (GH-9) and more", Some(9)),
        ("[3.14] gh-1: Revert (GH-8) (GH-9)", Some(9)),
        ("[3.14] gh-1: Fix it (GH-9) (see notes)", Some(9)),
        ("[3.14] gh-1: Fix it", None),
        ("[3.14] gh-1: Fix it GH-9", None),
        ("[3.14] gh-1: Fix it (#9)", None),
        ("[3.14] gh-1: Fix it (GH-)", None),
        ("[3.14] gh-1: Fix it (GH-9", None),
        ("Fix it", None),
    ] {
        let named = LinkRule::CpythonTitleOrigin.origin_in_title(&off_default(title));
        let under_cpython = expected.map(|origin| (Conventions::Cpython, origin));
        assert_eq!(named, under_cpython, "{title}");
    }
}

#[test]
fn a_title_s_marker_names_no_origin_on_the_default_branch() {
    let signals = LinkSignals {
        off_default_description: None,
        ..off_default("[3.14] gh-1: Fix it (GH-9)")
    };
    assert_eq!(LinkRule::CpythonTitleOrigin.origin_in_title(&signals), None);
}

#[test]
fn each_rule_names_only_what_it_reads() {
    let signals = LinkSignals {
        title: "[3.14] gh-1: Fix it (GH-9)",
        closing_issues: &[2],
        off_default_description: Some("Fixes #3"),
    };
    let fixed = LinkRule::SET.map(|rule| rule.fixed(&signals, &cpython()));
    let origins = LinkRule::SET.map(|rule| rule.origin_in_title(&signals));
    assert_eq!(
        fixed,
        [
            vec![(2, None)],
            vec![(3, None)],
            vec![(1, Some(Conventions::Cpython))],
            vec![],
            vec![],
        ]
    );
    assert_eq!(
        origins,
        [None, None, None, Some((Conventions::Cpython, 9)), None]
    );
}

#[test]
fn only_the_cherry_pick_rule_reads_a_cherry_pick_line() {
    let merge = "0123456789abcdef0123456789abcdef01234567";
    let merged = crate::facts::pull_requests::PullRequestFacts {
        merge_commit: Some(crate::testing::sha(merge)),
        ..crate::testing::pr_facts(9)
    };
    let merges = MergeCommits::of(&[merged]);
    let picked = [crate::testing::sha(merge)];
    let origins = LinkRule::SET.map(|rule| rule.origin_picked(&picked, &merges));
    assert_eq!(origins, [None, None, None, None, Some(9)]);
}
